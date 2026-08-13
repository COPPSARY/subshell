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

#[cfg(unix)]
use std::process::{Command, Stdio};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, STILL_ACTIVE},
    System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_INFORMATION,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, PROCESS_VM_READ,
            TerminateProcess,
        },
    },
};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;

use crate::contracts::CommandError;

pub(crate) fn prepare_command(executable: &str, arguments: &[String]) -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        let mut script = PathBuf::from(executable);
        if script.extension().is_none() {
            let command_script = script.with_extension("cmd");
            if command_script.is_file() {
                script = command_script;
            }
        }
        script = windows_friendly_path(&script);

        let extension = script
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat") {
            let powershell_script = script.with_extension("ps1");
            if powershell_script.is_file() {
                let mut wrapped = vec![
                    "-NoLogo".into(),
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-ExecutionPolicy".into(),
                    "Bypass".into(),
                    "-File".into(),
                    powershell_script.to_string_lossy().into_owned(),
                ];
                wrapped.extend_from_slice(arguments);
                return ("powershell.exe".into(), wrapped);
            }

            let mut wrapped = vec!["/D".into(), "/S".into(), "/C".into()];
            wrapped.push(script.to_string_lossy().into_owned());
            wrapped.extend_from_slice(arguments);
            return ("cmd.exe".into(), wrapped);
        }
        if extension.eq_ignore_ascii_case("ps1") {
            let mut wrapped = vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                script.to_string_lossy().into_owned(),
            ];
            wrapped.extend_from_slice(arguments);
            return ("powershell.exe".into(), wrapped);
        }
    }

    (executable.into(), arguments.to_vec())
}

#[cfg(windows)]
fn windows_friendly_path(path: &std::path::Path) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(value) = value.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{value}"));
    }
    if let Some(value) = value.strip_prefix(r"\\?\") {
        return PathBuf::from(value);
    }
    path.into()
}

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
    process_id: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessUsage {
    pub active: bool,
    pub process_id: Option<u32>,
    pub cpu_percent: Option<f32>,
    pub resident_bytes: Option<u64>,
}

#[derive(Clone, Default)]
pub struct ProcessSupervisor {
    handles: Arc<Mutex<HashMap<String, Handle>>>,
}

impl ProcessSupervisor {
    pub fn identity_is_active(identity: &str) -> bool {
        let Ok(process_id) = identity.parse::<u32>() else {
            return false;
        };
        #[cfg(unix)]
        {
            Command::new("kill")
                .args(["-0", &process_id.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        }
        #[cfg(windows)]
        {
            windows_process_is_active(process_id)
        }
    }

    pub fn usage(&self, run_id: &str) -> ProcessUsage {
        let process_id = self
            .handles
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(run_id)
            .and_then(|handle| handle.process_id);
        let Some(process_id) = process_id else {
            return ProcessUsage {
                active: false,
                process_id: None,
                cpu_percent: None,
                resident_bytes: None,
            };
        };
        let metrics = process_metrics(process_id);
        ProcessUsage {
            active: true,
            process_id: Some(process_id),
            cpu_percent: metrics.as_ref().and_then(|metrics| metrics.0),
            resident_bytes: metrics.and_then(|metrics| metrics.1),
        }
    }

    pub fn is_active(&self, run_id: &str) -> bool {
        self.handles
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(run_id)
    }

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
        let (executable, arguments) = prepare_command(&spec.executable, &spec.arguments);
        let mut command = CommandBuilder::new(executable);
        command.args(arguments);
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
                    process_id,
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
        #[cfg(windows)]
        if let Some(process_id) = handle.process_id {
            return terminate_windows_process(process_id);
        }
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

#[cfg(unix)]
fn process_metrics(process_id: u32) -> Option<(Option<f32>, Option<u64>)> {
    Command::new("ps")
        .args(["-o", "%cpu=,rss=", "-p", &process_id.to_string()])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|output| {
            let mut fields = output.split_whitespace();
            Some((
                fields.next()?.parse().ok(),
                fields.next()?.parse::<u64>().ok().map(|value| value * 1024),
            ))
        })
}

#[cfg(windows)]
fn windows_process_is_active(process_id: u32) -> bool {
    // SAFETY: the handle is checked before use, the out pointer is valid, and every opened
    // handle is closed before returning.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
        if handle.is_null() {
            return false;
        }
        let mut exit_code = 0;
        let active =
            GetExitCodeProcess(handle, &mut exit_code) != 0 && exit_code == STILL_ACTIVE as u32;
        CloseHandle(handle);
        active
    }
}

#[cfg(windows)]
fn terminate_windows_process(process_id: u32) -> Result<(), CommandError> {
    // SAFETY: the handle is checked before use and closed on every path after it is opened.
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, process_id);
        if handle.is_null() {
            return if windows_process_is_active(process_id) {
                Err(process_error(std::io::Error::last_os_error()))
            } else {
                Ok(())
            };
        }
        let terminated = TerminateProcess(handle, 1) != 0;
        let error = std::io::Error::last_os_error();
        CloseHandle(handle);
        if terminated || !windows_process_is_active(process_id) {
            Ok(())
        } else {
            Err(process_error(error))
        }
    }
}

#[cfg(windows)]
fn process_metrics(process_id: u32) -> Option<(Option<f32>, Option<u64>)> {
    // SAFETY: the handle is checked before use, the counter pointer and size match, and every
    // opened handle is closed before returning.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, process_id);
        if handle.is_null() {
            return None;
        }
        let mut counters = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..Default::default()
        };
        let read = GetProcessMemoryInfo(handle, &mut counters, counters.cb) != 0;
        CloseHandle(handle);
        read.then_some((None, Some(counters.WorkingSetSize as u64)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::{sync::mpsc, time::Duration};
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
    fn recognizes_the_current_process_identity() {
        assert!(ProcessSupervisor::identity_is_active(
            &std::process::id().to_string()
        ));
        assert!(!ProcessSupervisor::identity_is_active("not-a-pid"));
    }

    #[cfg(windows)]
    #[test]
    fn reads_the_current_process_memory_without_spawning_a_shell() {
        let (_, resident_bytes) = process_metrics(std::process::id()).unwrap();

        assert!(resident_bytes.is_some_and(|bytes| bytes > 0));
    }

    #[cfg(windows)]
    #[test]
    fn stops_a_windows_pty_process_without_a_false_kill_error() {
        let dir = tempdir().unwrap();
        let supervisor = ProcessSupervisor::default();
        let process_id = supervisor
            .launch(
                "windows-stop".into(),
                ProcessSpec {
                    executable: "powershell.exe".into(),
                    arguments: vec![
                        "-NoLogo".into(),
                        "-NoProfile".into(),
                        "-NonInteractive".into(),
                        "-Command".into(),
                        "while ($true) { Start-Sleep -Seconds 1 }".into(),
                    ],
                    cwd: dir.path().into(),
                    environment: vec![("PATH".into(), std::env::var("PATH").unwrap())],
                    log_path: dir.path().join("stop.log"),
                    stdin: None,
                    redactions: vec![],
                },
                Arc::new(|_| {}),
            )
            .unwrap()
            .unwrap();

        supervisor.stop("windows-stop").unwrap();

        assert!(!ProcessSupervisor::identity_is_active(
            &process_id.to_string()
        ));
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
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

    #[cfg(windows)]
    #[test]
    fn npm_command_shims_use_their_powershell_entrypoint() {
        let dir = tempdir().unwrap();
        let extensionless = dir.path().join("codex");
        fs::write(&extensionless, "#!/bin/sh").unwrap();
        fs::write(extensionless.with_extension("cmd"), "@echo off").unwrap();
        fs::write(extensionless.with_extension("ps1"), "Write-Output ok").unwrap();

        let (executable, arguments) = prepare_command(
            &extensionless.to_string_lossy(),
            &["login".into(), "status".into()],
        );

        assert_eq!(executable, "powershell.exe");
        assert_eq!(arguments[5], "-File");
        assert!(arguments[6].ends_with("codex.ps1"));
        assert_eq!(&arguments[7..], ["login", "status"]);
    }
}
