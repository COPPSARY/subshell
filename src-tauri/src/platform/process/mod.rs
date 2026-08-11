use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
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
    pub redactions: Vec<Vec<u8>>,
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
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&spec.log_path)
            .map_err(io_error)?;
        let initial_cursor = log_file.metadata().map_err(io_error)?.len();
        let log = Arc::new(Mutex::new(log_file));
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
        let cursor = Arc::new(AtomicU64::new(initial_cursor));
        let read_sink = sink.clone();
        let read_log = log.clone();
        let read_cursor = cursor.clone();
        let mut redactor = Redactor::new(spec.redactions);
        let reader_thread = thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                let bytes = redactor.push(&buffer[..count], false);
                if bytes.is_empty() {
                    continue;
                }
                if let Ok(mut file) = read_log.lock() {
                    let _ = file.write_all(&bytes);
                    let _ = file.flush();
                }
                let end = read_cursor.fetch_add(bytes.len() as u64, Ordering::SeqCst)
                    + bytes.len() as u64;
                read_sink(ProcessNotice::Output { bytes, cursor: end });
            }
            let bytes = redactor.push(&[], true);
            if !bytes.is_empty() {
                if let Ok(mut file) = read_log.lock() {
                    let _ = file.write_all(&bytes);
                    let _ = file.flush();
                }
                let end = read_cursor.fetch_add(bytes.len() as u64, Ordering::SeqCst)
                    + bytes.len() as u64;
                read_sink(ProcessNotice::Output { bytes, cursor: end });
            }
        });
        let handles = self.handles.clone();
        thread::spawn(move || {
            let status = child.wait();
            let _ = reader_thread.join();
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

struct Redactor {
    patterns: Vec<Vec<u8>>,
    pending: Vec<u8>,
}

impl Redactor {
    fn new(patterns: Vec<Vec<u8>>) -> Self {
        Self {
            patterns: patterns
                .into_iter()
                .filter(|value| !value.is_empty())
                .collect(),
            pending: Vec::new(),
        }
    }

    fn push(&mut self, bytes: &[u8], finish: bool) -> Vec<u8> {
        if self.patterns.is_empty() {
            return bytes.to_vec();
        }
        self.pending.extend_from_slice(bytes);
        let mut output = Vec::new();
        let mut index = 0;
        while index < self.pending.len() {
            if let Some(pattern) = self
                .patterns
                .iter()
                .find(|pattern| self.pending[index..].starts_with(pattern))
            {
                output.extend_from_slice(b"[REDACTED]");
                index += pattern.len();
                continue;
            }
            if !finish
                && self.patterns.iter().any(|pattern| {
                    pattern.starts_with(&self.pending[index..])
                        && self.pending.len() - index < pattern.len()
                })
            {
                break;
            }
            output.push(self.pending[index]);
            index += 1;
        }
        self.pending.drain(..index);
        output
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

pub fn read_log_tail(path: &std::path::Path, limit: usize) -> Result<(Vec<u8>, u64), CommandError> {
    let length = fs::metadata(path).map_err(io_error)?.len();
    read_log(
        path,
        length.saturating_sub(limit.min(64 * 1024) as u64),
        limit,
    )
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
    fn reads_only_the_latest_log_bytes_for_terminal_restore() {
        let dir = tempdir().unwrap();
        let log = dir.path().join("out.log");
        fs::write(&log, b"0123456789").unwrap();
        let (bytes, cursor) = read_log_tail(&log, 4).unwrap();
        assert_eq!(bytes, b"6789");
        assert_eq!(cursor, 10);
    }

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
                    redactions: vec![],
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

    #[test]
    fn appends_output_when_a_session_is_resumed() {
        let dir = tempdir().unwrap();
        let script = dir.path().join("agent");
        fs::write(&script, "#!/bin/sh\nprintf 'turn\\n'\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let log = dir.path().join("out.log");
        let supervisor = ProcessSupervisor::default();
        for _ in 0..2 {
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
                        redactions: vec![],
                    },
                    Arc::new(move |event| {
                        tx.send(event).unwrap();
                    }),
                )
                .unwrap();
            while !matches!(
                rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                ProcessNotice::Exited { .. }
            ) {}
        }
        assert_eq!(fs::read_to_string(log).unwrap().matches("turn").count(), 2);
    }

    #[test]
    fn redacts_secrets_split_across_output_chunks() {
        let mut redactor = Redactor::new(vec![b"secret-marker".to_vec()]);
        assert_eq!(redactor.push(b"before secret-", false), b"before ");
        assert_eq!(redactor.push(b"marker after", false), b"[REDACTED] after");
        assert!(redactor.push(&[], true).is_empty());
    }
}
