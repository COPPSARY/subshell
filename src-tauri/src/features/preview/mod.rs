use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sha2::Digest;
use tauri::State;
use uuid::Uuid;

use crate::{
    contracts::CommandError,
    features::review,
    platform::{
        database::Database,
        environment::{PortLeases, RuntimePaths},
        git::GitService,
        process::{self, ProcessNotice, ProcessSpec, ProcessSupervisor},
    },
};

#[derive(Clone)]
enum Launcher {
    Static,
    Process,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparePreviewInput {
    pub attempt_id: String,
    pub fingerprint: String,
    pub run_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartPreviewInput {
    pub preview_id: String,
    pub command_fingerprint: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewIdInput {
    pub preview_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewLogInput {
    pub preview_id: String,
    pub cursor: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub id: String,
    pub attempt_id: String,
    pub review_fingerprint: String,
    pub run_id: Option<String>,
    pub scope_label: String,
    pub status: String,
    pub url: String,
    pub port: u16,
    pub command: PreviewCommand,
    pub combined_patch: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewLogChunk {
    pub content: String,
    pub cursor: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewCommand {
    pub display: String,
    pub executable: String,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub working_directory: String,
    pub fingerprint: String,
}

#[derive(Clone)]
struct LaunchPlan {
    kind: Launcher,
    command: PreviewCommand,
}

struct StaticHandle {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

struct PreviewSession {
    view: Preview,
    root: PathBuf,
    worktree: PathBuf,
    project_path: PathBuf,
    log_path: PathBuf,
    launcher: Launcher,
    static_handle: Option<StaticHandle>,
    process_key: Option<String>,
}

#[derive(Clone)]
pub struct PreviewService {
    database: Database,
    paths: RuntimePaths,
    git: GitService,
    processes: ProcessSupervisor,
    ports: PortLeases,
    sessions: Arc<Mutex<HashMap<String, PreviewSession>>>,
}

impl PreviewService {
    pub fn new(
        database: Database,
        paths: RuntimePaths,
        git: GitService,
        processes: ProcessSupervisor,
        ports: PortLeases,
    ) -> Self {
        Self {
            database,
            paths,
            git,
            processes,
            ports,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn prepare(&self, input: PreparePreviewInput) -> Result<Preview, CommandError> {
        let verified = review::verified_review(
            &input.attempt_id,
            &input.fingerprint,
            &self.database,
            &self.git,
        )?;
        if let Some(existing) = self
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .find(|session| {
                session.view.attempt_id == input.attempt_id
                    && session.view.review_fingerprint == input.fingerprint
                    && session.view.run_id == input.run_id
            })
            .map(|session| session.view.clone())
        {
            return Ok(existing);
        }
        let runs = if let Some(run_id) = input.run_id.as_deref() {
            vec![
                verified
                    .review
                    .runs
                    .iter()
                    .find(|run| run.run_id == run_id)
                    .ok_or_else(|| {
                        CommandError::new("preview_run_not_found", "Run is not part of this review")
                    })?,
            ]
        } else {
            verified.review.runs.iter().collect::<Vec<_>>()
        };
        let id = Uuid::new_v4().to_string();
        let root = self.paths.data_dir.join("previews").join(&id);
        let worktree = root.join("worktree");
        let port = self.ports.acquire()?;
        let prepared = (|| {
            fs::create_dir_all(&root).map_err(io_error)?;
            materialize(
                &self.git,
                &verified.project_path,
                &verified.review.base_revision,
                &runs,
                &id,
                &root,
                &worktree,
            )?;
            let diff = self
                .git
                .exact_diff(&worktree, &verified.review.base_revision)?;
            link_node_modules(&verified.project_path, &worktree)?;
            let plan = detect_launcher(&worktree, port)?;
            let mut preview_command = plan.command.clone();
            preview_command.working_directory = worktree.to_string_lossy().into();
            let log_path = root.join("server.log");
            fs::write(&log_path, []).map_err(io_error)?;
            let scope_label = input
                .run_id
                .as_deref()
                .and_then(|run_id| verified.review.runs.iter().find(|run| run.run_id == run_id))
                .map(|run| run.title.clone())
                .unwrap_or_else(|| "Combined application".into());
            Ok::<_, CommandError>((
                plan,
                log_path,
                Preview {
                    id: id.clone(),
                    attempt_id: input.attempt_id,
                    review_fingerprint: input.fingerprint,
                    run_id: input.run_id,
                    scope_label,
                    status: "ready".into(),
                    url: format!("http://127.0.0.1:{port}"),
                    port,
                    command: preview_command,
                    combined_patch: String::from_utf8_lossy(&diff.patch).into_owned(),
                    error: None,
                },
            ))
        })();
        let (plan, log_path, view) = match prepared {
            Ok(value) => value,
            Err(error) => {
                self.ports.release(port);
                let _ = self.git.remove_worktree(&verified.project_path, &worktree);
                let _ = fs::remove_dir_all(&root);
                return Err(error);
            }
        };
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(
                id,
                PreviewSession {
                    view: view.clone(),
                    root,
                    worktree,
                    project_path: verified.project_path,
                    log_path,
                    launcher: plan.kind,
                    static_handle: None,
                    process_key: None,
                },
            );
        Ok(view)
    }

    fn get(&self, id: &str) -> Result<Preview, CommandError> {
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .map(|session| session.view.clone())
            .ok_or_else(|| {
                CommandError::new("preview_not_found", "Preview is closed or unavailable")
            })
    }

    fn start(&self, input: StartPreviewInput) -> Result<Preview, CommandError> {
        let (launcher, command, worktree, log_path, port) = {
            let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
            let session = sessions.get_mut(&input.preview_id).ok_or_else(|| {
                CommandError::new("preview_not_found", "Preview is closed or unavailable")
            })?;
            if session.view.command.fingerprint != input.command_fingerprint {
                return Err(CommandError::new(
                    "preview_command_changed",
                    "Review the current command before starting this preview",
                ));
            }
            if matches!(session.view.status.as_str(), "starting" | "running") {
                return Ok(session.view.clone());
            }
            session.view.status = "starting".into();
            session.view.error = None;
            (
                session.launcher.clone(),
                session.view.command.clone(),
                session.worktree.clone(),
                session.log_path.clone(),
                session.view.port,
            )
        };
        append_log(&log_path, &format!("\n$ {}\n", command.display));
        match launcher {
            Launcher::Static => {
                let result = TcpListener::bind(("127.0.0.1", port))
                    .map_err(io_error)
                    .and_then(|listener| start_static(worktree, listener, log_path.clone()));
                let mut handle = match result {
                    Ok(handle) => handle,
                    Err(error) => {
                        if let Some(session) = self
                            .sessions
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .get_mut(&input.preview_id)
                        {
                            session.view.status = "failed".into();
                            session.view.error = Some(error.message.clone());
                        }
                        return Err(error);
                    }
                };
                append_log(
                    &log_path,
                    &format!("Serving {}\n", self.get(&input.preview_id)?.url),
                );
                let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
                let Some(session) = sessions.get_mut(&input.preview_id) else {
                    handle.stop();
                    return Err(CommandError::new(
                        "preview_not_found",
                        "Preview was closed before the server started",
                    ));
                };
                session.static_handle = Some(handle);
                session.view.status = "running".into();
            }
            Launcher::Process => {
                self.start_process(&input.preview_id, command, worktree, log_path, port)?
            }
        }
        self.get(&input.preview_id)
    }

    fn start_process(
        &self,
        id: &str,
        command: PreviewCommand,
        worktree: PathBuf,
        log_path: PathBuf,
        port: u16,
    ) -> Result<(), CommandError> {
        let sessions = self.sessions.clone();
        let event_id = id.to_string();
        let process_key = format!("preview-{id}-{}", Uuid::new_v4());
        let event_key = process_key.clone();
        if let Some(session) = self
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_mut(id)
        {
            session.process_key = Some(process_key.clone());
        }
        let sink = Arc::new(move |notice| {
            if let ProcessNotice::Exited { success, exit_code } = notice {
                let mut sessions = sessions.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(session) = sessions.get_mut(&event_id)
                    && session.process_key.as_deref() == Some(&event_key)
                    && session.view.status != "stopped"
                {
                    session.process_key = None;
                    session.view.status = "failed".into();
                    session.view.error = Some(if success {
                        "Server stopped before the preview was closed".into()
                    } else {
                        format!("Server exited with code {exit_code}")
                    });
                }
            }
        });
        if let Err(error) = self.processes.launch(
            process_key.clone(),
            ProcessSpec {
                executable: command.executable,
                arguments: command.arguments,
                cwd: worktree,
                environment: child_environment(&command.environment),
                log_path,
                stdin: None,
                redactions: vec![],
            },
            sink,
        ) {
            if let Some(session) = self
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get_mut(id)
            {
                session.process_key = None;
                session.view.status = "failed".into();
                session.view.error = Some(error.message.clone());
            }
            return Err(error);
        }
        let sessions = self.sessions.clone();
        let processes = self.processes.clone();
        let readiness_id = id.to_string();
        thread::spawn(move || {
            for _ in 0..240 {
                thread::sleep(Duration::from_millis(250));
                let status = sessions
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get(&readiness_id)
                    .map(|session| {
                        (
                            session.view.status.clone(),
                            session.process_key.as_deref() == Some(&process_key),
                        )
                    });
                if !matches!(status, Some((ref status, true)) if status == "starting") {
                    return;
                }
                if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                    if let Some(session) = sessions
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .get_mut(&readiness_id)
                        .filter(|session| session.process_key.as_deref() == Some(&process_key))
                    {
                        session.view.status = "running".into();
                    }
                    return;
                }
            }
            if let Some(session) = sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get_mut(&readiness_id)
                .filter(|session| session.process_key.as_deref() == Some(&process_key))
            {
                session.process_key = None;
                session.view.status = "failed".into();
                session.view.error = Some(format!("Server did not listen on port {port}"));
            }
            let _ = processes.stop(&process_key);
        });
        Ok(())
    }

    fn stop(&self, id: &str) -> Result<Preview, CommandError> {
        let (mut static_handle, process_key) = {
            let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
            let session = sessions.get_mut(id).ok_or_else(|| {
                CommandError::new("preview_not_found", "Preview is closed or unavailable")
            })?;
            let process_key = session.process_key.take();
            session.view.status = "stopped".into();
            (session.static_handle.take(), process_key)
        };
        if let Some(handle) = static_handle.as_mut() {
            handle.stop();
        }
        if let Some(process_key) = process_key
            && let Err(error) = self.processes.stop(&process_key)
            && error.code != "run_not_active"
        {
            return Err(error);
        }
        self.get(id)
    }

    fn restart(&self, id: &str) -> Result<Preview, CommandError> {
        let stopped = self.stop(id)?;
        self.start(StartPreviewInput {
            preview_id: id.into(),
            command_fingerprint: stopped.command.fingerprint,
        })
    }

    fn close(&self, id: &str) -> Result<(), CommandError> {
        self.stop(id)?;
        let (project_path, worktree, root, port) = {
            let sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
            let session = sessions.get(id).ok_or_else(|| {
                CommandError::new("preview_not_found", "Preview is closed or unavailable")
            })?;
            (
                session.project_path.clone(),
                session.worktree.clone(),
                session.root.clone(),
                session.view.port,
            )
        };
        self.git.remove_worktree(&project_path, &worktree)?;
        fs::remove_dir_all(&root).map_err(io_error)?;
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(id);
        self.ports.release(port);
        Ok(())
    }

    fn read_log(&self, input: PreviewLogInput) -> Result<PreviewLogChunk, CommandError> {
        let log_path = self
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&input.preview_id)
            .map(|session| session.log_path.clone())
            .ok_or_else(|| {
                CommandError::new("preview_not_found", "Preview is closed or unavailable")
            })?;
        let (bytes, cursor) = process::read_log(&log_path, input.cursor, 64 * 1024)?;
        Ok(PreviewLogChunk {
            content: String::from_utf8_lossy(&bytes).into_owned(),
            cursor,
        })
    }
}

#[tauri::command]
pub fn preview_prepare(
    input: PreparePreviewInput,
    service: State<PreviewService>,
) -> Result<Preview, CommandError> {
    service.prepare(input)
}

#[tauri::command]
pub fn preview_get(
    input: PreviewIdInput,
    service: State<PreviewService>,
) -> Result<Preview, CommandError> {
    service.get(&input.preview_id)
}

#[tauri::command]
pub fn preview_start(
    input: StartPreviewInput,
    service: State<PreviewService>,
) -> Result<Preview, CommandError> {
    service.start(input)
}

#[tauri::command]
pub fn preview_stop(
    input: PreviewIdInput,
    service: State<PreviewService>,
) -> Result<Preview, CommandError> {
    service.stop(&input.preview_id)
}

#[tauri::command]
pub fn preview_restart(
    input: PreviewIdInput,
    service: State<PreviewService>,
) -> Result<Preview, CommandError> {
    service.restart(&input.preview_id)
}

#[tauri::command]
pub fn preview_close(
    input: PreviewIdInput,
    service: State<PreviewService>,
) -> Result<(), CommandError> {
    service.close(&input.preview_id)
}

#[tauri::command]
pub fn preview_read_log(
    input: PreviewLogInput,
    service: State<PreviewService>,
) -> Result<PreviewLogChunk, CommandError> {
    service.read_log(input)
}

fn materialize(
    git: &GitService,
    repository: &Path,
    base_revision: &str,
    runs: &[&review::RunSnapshot],
    preview_id: &str,
    root: &Path,
    worktree: &Path,
) -> Result<(), CommandError> {
    let mut revisions = Vec::new();
    let mut branches = Vec::new();
    let result = (|| {
        for (position, run) in runs.iter().enumerate() {
            let patch = fs::read(&run.patch_path).map_err(io_error)?;
            if format!("{:x}", sha2::Sha256::digest(&patch)) != run.patch_sha256 {
                return Err(CommandError::new(
                    "review_corrupt",
                    "A stored review patch no longer matches its fingerprint",
                ));
            }
            let branch = format!(
                "subshell/preview/{}/{:02}",
                preview_id
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .take(12)
                    .collect::<String>(),
                position + 1
            );
            let revision = git.create_snapshot_branch(
                repository,
                base_revision,
                &root.join(format!("snapshot-{position}")),
                &branch,
                &patch,
                &format!("Preview: {}", run.title),
            )?;
            branches.push(branch);
            revisions.push(revision);
        }
        git.prepare_integration(repository, base_revision, &revisions, worktree)?;
        Ok(())
    })();
    let mut cleanup_error = None;
    for branch in branches {
        if let Err(error) = git.remove_branch(repository, &branch) {
            cleanup_error.get_or_insert(error);
        }
    }
    result.and_then(|()| cleanup_error.map_or(Ok(()), Err))
}

fn child_environment(explicit: &[(String, String)]) -> Vec<(String, String)> {
    let mut values = Vec::new();
    for key in [
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "SHELL",
        "LANG",
        "LC_ALL",
        "TERM",
        "COLORTERM",
        "SYSTEMROOT",
        "WINDIR",
        "PATHEXT",
        "APPDATA",
        "LOCALAPPDATA",
        "USERPROFILE",
    ] {
        if let Ok(value) = std::env::var(key) {
            values.push((key.into(), value));
        }
    }
    values.extend(explicit.iter().cloned());
    values
}

fn link_node_modules(project: &Path, worktree: &Path) -> Result<(), CommandError> {
    let source = project.join("node_modules");
    let destination = worktree.join("node_modules");
    if !source.is_dir() || destination.exists() {
        return Ok(());
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(source, destination).map_err(io_error)?;
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(source, destination).map_err(io_error)?;
    Ok(())
}

impl StaticHandle {
    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn detect_launcher(root: &Path, port: u16) -> Result<LaunchPlan, CommandError> {
    if let Some((manager, mut arguments, vite)) = package_launcher(root)? {
        if vite {
            arguments.extend([
                "--".into(),
                "--host".into(),
                "127.0.0.1".into(),
                "--port".into(),
                port.to_string(),
                "--strictPort".into(),
            ]);
        }
        return Ok(process_plan(root, port, manager, arguments));
    }
    if root.join("Cargo.toml").is_file() {
        return Ok(process_plan(root, port, "cargo".into(), vec!["run".into()]));
    }
    if root.join("index.html").is_file() {
        let executable = "SubShell static server".to_string();
        let arguments = vec![
            "--root".into(),
            ".".into(),
            "--port".into(),
            port.to_string(),
        ];
        return Ok(LaunchPlan {
            kind: Launcher::Static,
            command: command(root, port, executable, arguments),
        });
    }
    Err(CommandError::new(
        "preview_command_not_found",
        "No package dev script, Cargo.toml, or index.html was found",
    ))
}

fn package_launcher(root: &Path) -> Result<Option<(String, Vec<String>, bool)>, CommandError> {
    let path = root.join("package.json");
    if !path.is_file() {
        return Ok(None);
    }
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path).map_err(io_error)?)
        .map_err(|error| CommandError::new("invalid_package_json", error.to_string()))?;
    let Some(script) = value
        .get("scripts")
        .and_then(|scripts| scripts.get("dev"))
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(None);
    };
    let (manager, arguments) = if root.join("pnpm-lock.yaml").is_file() {
        ("pnpm", vec!["dev".into()])
    } else if root.join("yarn.lock").is_file() {
        ("yarn", vec!["dev".into()])
    } else if root.join("bun.lock").is_file() || root.join("bun.lockb").is_file() {
        ("bun", vec!["run".into(), "dev".into()])
    } else {
        ("npm", vec!["run".into(), "dev".into()])
    };
    let vite = script.split_whitespace().any(|word| {
        word.trim_matches(|character: char| !character.is_ascii_alphanumeric()) == "vite"
    });
    Ok(Some((manager.into(), arguments, vite)))
}

fn process_plan(root: &Path, port: u16, executable: String, arguments: Vec<String>) -> LaunchPlan {
    LaunchPlan {
        kind: Launcher::Process,
        command: command(root, port, executable, arguments),
    }
}

fn command(root: &Path, port: u16, executable: String, arguments: Vec<String>) -> PreviewCommand {
    let environment: Vec<(String, String)> = vec![
        ("HOST".into(), "127.0.0.1".into()),
        ("PORT".into(), port.to_string()),
        ("BROWSER".into(), "none".into()),
    ];
    let display = format!(
        "HOST=127.0.0.1 PORT={port} BROWSER=none {}",
        std::iter::once(executable.as_str())
            .chain(arguments.iter().map(String::as_str))
            .map(display_token)
            .collect::<Vec<_>>()
            .join(" ")
    );
    let mut digest = sha2::Sha256::new();
    let root_text = root.to_string_lossy();
    for value in std::iter::once(executable.as_str())
        .chain(arguments.iter().map(String::as_str))
        .chain(
            environment
                .iter()
                .flat_map(|(key, value)| [key.as_str(), value.as_str()]),
        )
        .chain(std::iter::once(root_text.as_ref()))
    {
        digest.update(value.as_bytes());
        digest.update([0]);
    }
    let fingerprint = format!("{:x}", digest.finalize());
    PreviewCommand {
        display,
        executable,
        arguments,
        environment,
        working_directory: root.to_string_lossy().into(),
        fingerprint,
    }
}

fn display_token(value: &str) -> String {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-._/:".contains(character))
    {
        value.into()
    } else {
        serde_json::to_string(value).unwrap_or_else(|_| "\"?\"".into())
    }
}

fn start_static(
    root: PathBuf,
    listener: TcpListener,
    log_path: PathBuf,
) -> Result<StaticHandle, CommandError> {
    listener.set_nonblocking(true).map_err(io_error)?;
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let thread = thread::spawn(move || {
        while !thread_stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Err(error) = serve_static(stream, &root) {
                        append_log(&log_path, &format!("Request failed: {error}\n"));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(25));
                }
                Err(error) => {
                    append_log(&log_path, &format!("Static server failed: {error}\n"));
                    break;
                }
            }
        }
    });
    Ok(StaticHandle {
        stop,
        thread: Some(thread),
    })
}

fn serve_static(mut stream: TcpStream, root: &Path) -> Result<(), std::io::Error> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut request = [0_u8; 8192];
    let count = stream.read(&mut request)?;
    let first = String::from_utf8_lossy(&request[..count]);
    let mut parts = first.lines().next().unwrap_or_default().split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if !matches!(method, "GET" | "HEAD") {
        return respond(
            &mut stream,
            method,
            405,
            "text/plain",
            b"Method not allowed",
        );
    }
    let Some(path) = static_path(root, target) else {
        return respond(&mut stream, method, 404, "text/plain", b"Not found");
    };
    let bytes = fs::read(&path)?;
    respond(&mut stream, method, 200, content_type(&path), &bytes)
}

fn static_path(root: &Path, target: &str) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let decoded = percent_decode(target.split(['?', '#']).next().unwrap_or("/"))?;
    let relative = decoded.trim_start_matches('/');
    if Path::new(relative).components().any(|part| match part {
        Component::ParentDir | Component::RootDir | Component::Prefix(_) => true,
        Component::Normal(name) => name.to_string_lossy().starts_with('.'),
        _ => false,
    }) {
        return None;
    }
    let mut path = root.join(relative);
    if path.is_dir() {
        path.push("index.html");
    }
    let path = path.canonicalize().ok()?;
    (path.starts_with(root) && path.is_file()).then_some(path)
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex(*bytes.get(index + 1)?)?;
            let low = hex(*bytes.get(index + 2)?)?;
            output.push(high * 16 + low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).ok()
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn respond(
    stream: &mut TcpStream,
    method: &str,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<(), std::io::Error> {
    let label = if status == 200 {
        "OK"
    } else if status == 405 {
        "Method Not Allowed"
    } else {
        "Not Found"
    };
    write!(
        stream,
        "HTTP/1.1 {status} {label}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    if method != "HEAD" {
        stream.write_all(body)?;
    }
    stream.flush()
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
    {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn append_log(path: &Path, message: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(message.as_bytes());
        let _ = file.flush();
    }
}

fn io_error(error: std::io::Error) -> CommandError {
    CommandError::new("filesystem_error", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn detects_vite_with_the_project_package_manager_and_exact_port() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("package.json"),
            r#"{"scripts":{"dev":"vite"}}"#,
        )
        .unwrap();
        fs::write(root.path().join("pnpm-lock.yaml"), "").unwrap();

        let plan = detect_launcher(root.path(), 43123).unwrap();

        assert!(matches!(plan.kind, Launcher::Process));
        assert_eq!(plan.command.executable, "pnpm");
        assert_eq!(
            plan.command.arguments,
            [
                "dev",
                "--",
                "--host",
                "127.0.0.1",
                "--port",
                "43123",
                "--strictPort"
            ]
        );
        assert!(plan.command.display.contains("PORT=43123"));
        assert!(plan.command.display.contains("pnpm dev"));
    }

    #[test]
    fn falls_back_to_the_builtin_static_server() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("index.html"), "hello").unwrap();

        let plan = detect_launcher(root.path(), 43124).unwrap();

        assert!(matches!(plan.kind, Launcher::Static));
        assert_eq!(plan.command.executable, "SubShell static server");
    }

    #[test]
    fn static_server_serves_assets_and_rejects_parent_paths() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("index.html"), "<h1>Preview</h1>").unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut server =
            start_static(root.path().into(), listener, root.path().join("server.log")).unwrap();

        let get = |path: &str| {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
            let mut response = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => response.extend_from_slice(&buffer[..count]),
                    Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                    Err(error) => panic!("failed to read preview response: {error}"),
                }
            }
            String::from_utf8(response).unwrap()
        };
        assert!(get("/").contains("<h1>Preview</h1>"));
        assert!(get("/%2e%2e/secret").starts_with("HTTP/1.1 404"));
        assert!(get("/.git").starts_with("HTTP/1.1 404"));
        server.stop();
    }

    #[test]
    fn combines_three_run_worktrees_and_serves_without_touching_the_checkout() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repository");
        fs::create_dir(&repository).unwrap();
        for arguments in [
            ["init", "-q"].as_slice(),
            ["config", "user.email", "test@example.com"].as_slice(),
            ["config", "user.name", "Test"].as_slice(),
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(arguments)
                    .current_dir(&repository)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        fs::write(repository.join("index.html"), "<h1>Base</h1>").unwrap();
        fs::write(repository.join("styles.css"), "body{}").unwrap();
        fs::write(repository.join("app.js"), "console.log('base')").unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["add", "."])
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["commit", "-qm", "base"])
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );

        let git = GitService::default();
        let status = git.status(&repository).unwrap();
        let base = status.revision.unwrap();
        let branch = status.branch.unwrap();
        let paths = RuntimePaths {
            data_dir: root.path().join("data"),
        };
        let database = Database::initialize(&paths.data_dir.join("db.sqlite3")).unwrap();
        let connection = database.connect().unwrap();
        connection.execute("INSERT INTO projects(id,name,path,created_at,updated_at) VALUES('project','Project',?1,'now','now')", [repository.to_string_lossy()]).unwrap();
        connection.execute("INSERT INTO provider_accounts(id,provider_type,display_name,config_scope_path,status,created_at,updated_at) VALUES('provider','generic','Codex','/tmp/preview-provider','active','now','now')", []).unwrap();
        connection.execute("INSERT INTO tasks(id,project_id,title,status,base_branch,base_revision,created_at,updated_at) VALUES('task','project','Web preview','review',?1,?2,'now','now')", params![branch, base]).unwrap();
        let changes = [
            ("index.html", "<h1>Combined</h1>"),
            ("styles.css", "body{color:tomato}"),
            ("app.js", "console.log('combined')"),
        ];
        for (index, (file, content)) in changes.iter().enumerate() {
            let run_id = format!("run-{index}");
            let worktree = root.path().join(format!("run-worktree-{index}"));
            git.create_worktree(&repository, &base, &worktree).unwrap();
            fs::write(worktree.join(file), content).unwrap();
            connection.execute("INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,role,assignment_title,status,merge_order,context_sha256,created_at,updated_at) VALUES(?1,'task','provider','Implement','executor',?2,'succeeded',?3,?4,'now','now')", params![run_id, file, index as i64, format!("context-{index}")]).unwrap();
            connection.execute("INSERT INTO worktrees(id,agent_run_id,path,base_branch,base_revision,state,created_at) VALUES(?1,?2,?3,?4,?5,'active','now')", params![format!("worktree-{index}"), run_id, worktree.to_string_lossy(), branch, base]).unwrap();
        }
        let review = review::get_or_create("task", &database, &paths, &git).unwrap();
        let service = PreviewService::new(
            database,
            paths,
            git.clone(),
            ProcessSupervisor::default(),
            PortLeases::default(),
        );

        let combined = service
            .prepare(PreparePreviewInput {
                attempt_id: review.id.clone(),
                fingerprint: review.fingerprint.clone(),
                run_id: None,
            })
            .unwrap();
        let combined_path = service
            .sessions
            .lock()
            .unwrap()
            .get(&combined.id)
            .unwrap()
            .worktree
            .clone();
        assert_eq!(
            fs::read_to_string(combined_path.join("index.html")).unwrap(),
            "<h1>Combined</h1>"
        );
        assert_eq!(
            fs::read_to_string(combined_path.join("styles.css")).unwrap(),
            "body{color:tomato}"
        );
        assert_eq!(
            fs::read_to_string(combined_path.join("app.js")).unwrap(),
            "console.log('combined')"
        );
        assert_eq!(
            fs::read_to_string(repository.join("index.html")).unwrap(),
            "<h1>Base</h1>"
        );

        let running = service
            .start(StartPreviewInput {
                preview_id: combined.id.clone(),
                command_fingerprint: combined.command.fingerprint.clone(),
            })
            .unwrap();
        assert_eq!(running.status, "running");
        let mut stream = TcpStream::connect(("127.0.0.1", running.port)).unwrap();
        write!(stream, "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("<h1>Combined</h1>"));

        let single = service
            .prepare(PreparePreviewInput {
                attempt_id: review.id,
                fingerprint: review.fingerprint,
                run_id: Some("run-1".into()),
            })
            .unwrap();
        assert_ne!(single.port, running.port);
        let single_path = service
            .sessions
            .lock()
            .unwrap()
            .get(&single.id)
            .unwrap()
            .worktree
            .clone();
        assert_eq!(
            fs::read_to_string(single_path.join("index.html")).unwrap(),
            "<h1>Base</h1>"
        );
        assert_eq!(
            fs::read_to_string(single_path.join("styles.css")).unwrap(),
            "body{color:tomato}"
        );

        service.close(&single.id).unwrap();
        service.close(&combined.id).unwrap();
        assert!(!combined_path.exists());
        assert_eq!(
            fs::read_to_string(repository.join("index.html")).unwrap(),
            "<h1>Base</h1>"
        );
        assert_eq!(
            git.status(&repository).unwrap().revision.as_deref(),
            Some(base.as_str())
        );
    }
}
