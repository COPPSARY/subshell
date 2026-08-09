use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;

use crate::contracts::CommandError;

pub struct ProcessSpec {
    pub executable: String,
    pub arguments: Vec<String>,
    pub cwd: PathBuf,
    pub environment: Vec<(String, String)>,
    pub log_path: PathBuf,
    pub stdin: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProcessNotice {
    Output { bytes: Vec<u8>, cursor: u64 },
    Exited { success: bool, exit_code: u32 },
}

struct Handle {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

#[derive(Clone, Default)]
pub struct ProcessSupervisor {
    handles: Arc<Mutex<HashMap<String, Handle>>>,
}

impl ProcessSupervisor {
    pub fn launch(
        &self,
        run_id: String,
        spec: ProcessSpec,
        sink: Arc<dyn Fn(ProcessNotice) + Send + Sync>,
    ) -> Result<Option<u32>, CommandError> {
        if let Some(parent) = spec.log_path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let log = Arc::new(Mutex::new(File::create(&spec.log_path).map_err(io_error)?));
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(process_error)?;
        let mut command = CommandBuilder::new(&spec.executable);
        command.args(&spec.arguments);
        command.cwd(&spec.cwd);
        command.env_clear();
        for (key, value) in &spec.environment {
            command.env(key, value);
        }
        let mut child = pair.slave.spawn_command(command).map_err(process_error)?;
        drop(pair.slave);
        let process_id = child.process_id();
        let mut reader = pair.master.try_clone_reader().map_err(process_error)?;
        let mut writer = pair.master.take_writer().map_err(process_error)?;
        if let Some(input) = spec.stdin {
            writer.write_all(&input).map_err(io_error)?;
            writer.flush().map_err(io_error)?;
        }
        let killer = child.clone_killer();
        self.handles
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(
                run_id.clone(),
                Handle {
                    master: pair.master,
                    writer,
                    killer,
                },
            );
        let cursor = Arc::new(AtomicU64::new(0));
        let read_sink = sink.clone();
        let read_log = log.clone();
        let read_cursor = cursor.clone();
        thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                let bytes = buffer[..count].to_vec();
                if let Ok(mut file) = read_log.lock() {
                    let _ = file.write_all(&bytes);
                    let _ = file.flush();
                }
                let end = read_cursor.fetch_add(count as u64, Ordering::SeqCst) + count as u64;
                read_sink(ProcessNotice::Output { bytes, cursor: end });
            }
        });
        let handles = self.handles.clone();
        thread::spawn(move || {
            let status = child.wait();
            handles
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&run_id);
            match status {
                Ok(status) => sink(ProcessNotice::Exited {
                    success: status.success(),
                    exit_code: status.exit_code(),
                }),
                Err(_) => sink(ProcessNotice::Exited {
                    success: false,
                    exit_code: 1,
                }),
            }
        });
        Ok(process_id)
    }
    pub fn write_input(&self, run_id: &str, bytes: &[u8]) -> Result<(), CommandError> {
        let mut handles = self.handles.lock().unwrap_or_else(|p| p.into_inner());
        let handle = handles
            .get_mut(run_id)
            .ok_or_else(|| CommandError::new("run_not_active", "Run is no longer active"))?;
        handle.writer.write_all(bytes).map_err(io_error)?;
        handle.writer.flush().map_err(io_error)
    }
    pub fn resize(&self, run_id: &str, rows: u16, cols: u16) -> Result<(), CommandError> {
        let handles = self.handles.lock().unwrap_or_else(|p| p.into_inner());
        let handle = handles
            .get(run_id)
            .ok_or_else(|| CommandError::new("run_not_active", "Run is no longer active"))?;
        handle
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(process_error)
    }
    pub fn stop(&self, run_id: &str) -> Result<(), CommandError> {
        let mut handles = self.handles.lock().unwrap_or_else(|p| p.into_inner());
        let handle = handles
            .get_mut(run_id)
            .ok_or_else(|| CommandError::new("run_not_active", "Run is no longer active"))?;
        handle.killer.kill().map_err(io_error)
    }
}

pub fn read_log(
    path: &std::path::Path,
    cursor: u64,
    limit: usize,
) -> Result<(Vec<u8>, u64), CommandError> {
    use std::io::{Seek, SeekFrom};
    let mut file = OpenOptions::new().read(true).open(path).map_err(io_error)?;
    file.seek(SeekFrom::Start(cursor)).map_err(io_error)?;
    let mut bytes = vec![0; limit.min(64 * 1024)];
    let count = file.read(&mut bytes).map_err(io_error)?;
    bytes.truncate(count);
    Ok((bytes, cursor + count as u64))
}
fn process_error(error: impl std::fmt::Display) -> CommandError {
    CommandError::new("process_error", error.to_string())
}
fn io_error(error: std::io::Error) -> CommandError {
    CommandError::new("process_error", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, sync::mpsc, time::Duration};
    use tempfile::tempdir;
    #[test]
    fn streams_logs_and_reaps_a_stand_in() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("agent");
        fs::write(
            &script,
            "#!/bin/sh\nprintf 'first\\n'\nsleep 0.05\nprintf 'second\\n'\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let log = dir.path().join("out.log");
        let supervisor = ProcessSupervisor::default();
        let (tx, rx) = mpsc::channel();
        supervisor
            .launch(
                "r".into(),
                ProcessSpec {
                    executable: script.to_string_lossy().into(),
                    arguments: vec![],
                    cwd: dir.path().into(),
                    environment: vec![("PATH".into(), std::env::var("PATH").unwrap())],
                    log_path: log.clone(),
                    stdin: None,
                },
                Arc::new(move |event| {
                    tx.send(event).unwrap();
                }),
            )
            .unwrap();
        let mut exited = false;
        for _ in 0..4 {
            if matches!(
                rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                ProcessNotice::Exited { success: true, .. }
            ) {
                exited = true;
                break;
            }
        }
        assert!(exited);
        assert!(
            String::from_utf8(fs::read(log).unwrap())
                .unwrap()
                .contains("second")
        );
    }
}
