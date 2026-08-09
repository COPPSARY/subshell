use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{State, ipc::Channel};
use uuid::Uuid;

use crate::{
    contracts::{CommandError, Page},
    features::{
        context::{self, ContextDrafts},
        providers, tasks,
    },
    platform::{
        database::Database,
        environment::{self, PortLeases, RuntimePaths},
        git::GitService,
        process::{self, ProcessNotice, ProcessSpec, ProcessSupervisor},
    },
};

#[derive(Clone)]
pub struct RunService {
    database: Database,
    paths: RuntimePaths,
    git: GitService,
    drafts: ContextDrafts,
    processes: ProcessSupervisor,
    ports: PortLeases,
}
impl RunService {
    pub fn new(
        database: Database,
        paths: RuntimePaths,
        git: GitService,
        drafts: ContextDrafts,
        processes: ProcessSupervisor,
        ports: PortLeases,
    ) -> Self {
        Self {
            database,
            paths,
            git,
            drafts,
            processes,
            ports,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assignment {
    pub provider_id: String,
    pub instruction: String,
    pub context_token: String,
    pub approved_context: String,
    #[serde(default)]
    pub environment_files: Vec<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartInput {
    pub task_id: String,
    pub assignments: Vec<Assignment>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskId {
    pub task_id: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputInput {
    pub run_id: String,
    #[serde(default)]
    pub cursor: u64,
    pub limit: Option<usize>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputBytes {
    pub run_id: String,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResizeInput {
    pub run_id: String,
    pub rows: u16,
    pub cols: u16,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunId {
    pub run_id: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentInput {
    pub project_id: String,
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub task_id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub instruction: String,
    pub status: String,
    pub worktree_path: Option<String>,
    pub raw_log_path: Option<String>,
    pub context_pack_path: Option<String>,
    pub port: Option<u16>,
    pub updated_at: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RunStreamEvent {
    Started {
        run_id: String,
    },
    Output {
        run_id: String,
        bytes: Vec<u8>,
        cursor: u64,
    },
    StatusChanged {
        run_id: String,
        status: String,
    },
    Failed {
        run_id: String,
        error: CommandError,
    },
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputPage {
    pub bytes: Vec<u8>,
    pub next_cursor: u64,
}

#[tauri::command]
pub fn runs_environment_preview(
    input: EnvironmentInput,
    database: State<Database>,
) -> Result<environment::EnvironmentPreview, CommandError> {
    let path: PathBuf = database
        .connect()?
        .query_row(
            "SELECT path FROM projects WHERE id=?1",
            [input.project_id],
            |row| row.get::<_, String>(0),
        )
        .map(PathBuf::from)?;
    let files = environment::validate_files(&path, &input.files)?
        .into_iter()
        .map(|p| p.0)
        .collect();
    Ok(environment::EnvironmentPreview { files, port: None })
}
#[tauri::command]
pub fn runs_start(
    input: StartInput,
    on_event: Channel<RunStreamEvent>,
    service: State<RunService>,
) -> Result<Vec<Run>, CommandError> {
    service.start(input, on_event)
}
#[tauri::command]
pub fn runs_list(input: TaskId, service: State<RunService>) -> Result<Page<Run>, CommandError> {
    Ok(Page::first(service.list(&input.task_id)?))
}
#[tauri::command]
pub fn runs_read_output(
    input: OutputInput,
    service: State<RunService>,
) -> Result<OutputPage, CommandError> {
    let run = service
        .get(&input.run_id)?
        .ok_or_else(|| CommandError::new("run_not_found", "Run was not found"))?;
    let path = run
        .raw_log_path
        .ok_or_else(|| CommandError::new("output_unavailable", "Run output is not ready"))?;
    let (bytes, next_cursor) = process::read_log(
        Path::new(&path),
        input.cursor,
        input.limit.unwrap_or(64 * 1024),
    )?;
    Ok(OutputPage { bytes, next_cursor })
}
#[tauri::command]
pub fn runs_write_input(input: InputBytes, service: State<RunService>) -> Result<(), CommandError> {
    service.processes.write_input(&input.run_id, &input.bytes)
}
#[tauri::command]
pub fn runs_resize(input: ResizeInput, service: State<RunService>) -> Result<(), CommandError> {
    service
        .processes
        .resize(&input.run_id, input.rows.max(1), input.cols.max(1))
}
#[tauri::command]
pub fn runs_stop(input: RunId, service: State<RunService>) -> Result<(), CommandError> {
    service.stop(&input.run_id)
}

struct Prepared {
    run_id: String,
    provider: providers::ResolvedProvider,
    worktree: PathBuf,
    log: PathBuf,
    config_root: PathBuf,
    port: u16,
    prompt: String,
}

impl RunService {
    fn start(
        &self,
        input: StartInput,
        channel: Channel<RunStreamEvent>,
    ) -> Result<Vec<Run>, CommandError> {
        if input.assignments.is_empty() {
            return Err(CommandError::new(
                "invalid_assignments",
                "At least one assignment is required",
            ));
        }
        let task = tasks::get(&self.database, &input.task_id)?
            .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
        let project_path: PathBuf = self
            .database
            .connect()?
            .query_row(
                "SELECT path FROM projects WHERE id=?1",
                [&task.project_id],
                |row| row.get::<_, String>(0),
            )
            .map(PathBuf::from)?;
        let live = self.git.status(&project_path)?;
        if live.revision.as_deref() != Some(&task.base_revision) {
            return Err(CommandError::new(
                "base_revision_changed",
                "The project HEAD changed after this task was created",
            ));
        }
        let mut prepared = Vec::new();
        for assignment in input.assignments {
            match self.prepare(&task, &project_path, assignment) {
                Ok(run) => prepared.push(run),
                Err((run_id, error)) => {
                    let _ = channel.send(RunStreamEvent::Failed { run_id, error });
                }
            }
        }
        if !prepared.is_empty() {
            self.database.connect()?.execute("UPDATE tasks SET status='working',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1", [&input.task_id])?;
        }
        let mut launched = false;
        for run in prepared {
            let run_id = run.run_id.clone();
            match self.launch(run, channel.clone()) {
                Ok(()) => launched = true,
                Err(error) => {
                    let _ = channel.send(RunStreamEvent::Failed { run_id, error });
                }
            }
        }
        if !launched {
            self.database.connect()?.execute("UPDATE tasks SET status='failed',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1", [&input.task_id])?;
        }
        self.list(&input.task_id)
    }

    fn prepare(
        &self,
        task: &tasks::Task,
        project_path: &Path,
        assignment: Assignment,
    ) -> Result<Prepared, (String, CommandError)> {
        let run_id = Uuid::new_v4().to_string();
        let result = (|| {
            if assignment.approved_context.len() > 64 * 1024 {
                return Err(CommandError::new(
                    "context_too_large",
                    "Approved context exceeds 64 KiB",
                ));
            }
            let mut manifest = context::take(&self.drafts, &assignment.context_token)?;
            let context_sha256 = format!(
                "{:x}",
                Sha256::digest(assignment.approved_context.as_bytes())
            );
            manifest.was_edited = manifest.sha256 != context_sha256;
            manifest.sha256 = context_sha256.clone();
            manifest.total_bytes = assignment.approved_context.len();
            let provider = providers::resolve(&self.database, &assignment.provider_id)?;
            let run_dir = self.paths.data_dir.join("runs").join(&run_id);
            let worktree = self
                .paths
                .data_dir
                .join("worktrees")
                .join(&task.project_id)
                .join(&run_id);
            let config_root = run_dir.join("config");
            let log = run_dir.join("output.log");
            let context_path = run_dir.join("context.md");
            fs::create_dir_all(&run_dir).map_err(io_error)?;
            let connection = self.database.connect()?;
            connection.execute("INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,status,merge_order,raw_log_path,context_pack_path,context_manifest_json,context_sha256,created_at,updated_at) VALUES(?1,?2,?3,?4,'preparing',0,?5,?6,?7,?8,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'))",params![run_id,task.id,provider.id,assignment.instruction,log.to_string_lossy(),context_path.to_string_lossy(),serde_json::to_string(&manifest).unwrap(),context_sha256])?;
            self.git
                .create_worktree(project_path, &task.base_revision, &worktree)?;
            let mut environment =
                environment::copy_files(project_path, &worktree, &assignment.environment_files)?;
            if let Some(source) = provider.config_source_path.as_deref() {
                environment::copy_directory(Path::new(source), &config_root)?;
            } else {
                fs::create_dir_all(&config_root).map_err(io_error)?;
            }
            fs::write(&context_path, &assignment.approved_context).map_err(io_error)?;
            let port = self.ports.acquire()?;
            environment.port = Some(port);
            if let Err(error) = connection.execute("INSERT INTO worktrees(id,agent_run_id,path,base_branch,base_revision,state,environment_manifest_json,created_at) VALUES(?1,?2,?3,?4,?5,'active',?6,strftime('%Y-%m-%dT%H:%M:%fZ','now'))",params![Uuid::new_v4().to_string(),run_id,worktree.to_string_lossy(),task.base_branch,task.base_revision,serde_json::to_string(&environment).unwrap()]) {
                self.ports.release(port);
                return Err(error.into());
            }
            Ok(Prepared {
                run_id: run_id.clone(),
                provider,
                worktree,
                log,
                config_root,
                port,
                prompt: assignment.approved_context,
            })
        })();
        if result.is_err() {
            let _=self.database.connect().and_then(|connection|connection.execute("UPDATE agent_runs SET status='failed',ended_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",[&run_id]).map(|_|()).map_err(Into::into));
        }
        result.map_err(|error| (run_id, error))
    }

    fn launch(&self, run: Prepared, channel: Channel<RunStreamEvent>) -> Result<(), CommandError> {
        let (executable, arguments, stdin) =
            run.provider.launch_command(&run.prompt, &run.config_root);
        let mut environment = base_environment(&run.config_root, run.port);
        if let Some(name) = &run.provider.config_root_env_var {
            environment.push((name.clone(), run.config_root.to_string_lossy().into()));
        }
        let database = self.database.clone();
        let ports = self.ports.clone();
        let run_id = run.run_id.clone();
        let event_run_id = run_id.clone();
        let event_channel = channel.clone();
        let sink = Arc::new(move |notice| match notice {
            ProcessNotice::Output { bytes, cursor } => {
                let _ = event_channel.send(RunStreamEvent::Output {
                    run_id: event_run_id.clone(),
                    bytes,
                    cursor,
                });
            }
            ProcessNotice::Exited { success, .. } => {
                let status = if success { "succeeded" } else { "failed" };
                let mut reported_status = status.to_string();
                if let Ok(connection) = database.connect() {
                    let _=connection.execute("UPDATE agent_runs SET status=CASE WHEN status='cancelled' THEN status ELSE ?1 END,ended_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2",params![status,event_run_id]);
                    reported_status = connection
                        .query_row(
                            "SELECT status FROM agent_runs WHERE id=?1",
                            [&event_run_id],
                            |row| row.get(0),
                        )
                        .unwrap_or(reported_status);
                    let _=connection.execute("UPDATE tasks SET status=CASE WHEN EXISTS(SELECT 1 FROM agent_runs WHERE task_id=(SELECT task_id FROM agent_runs WHERE id=?1) AND status IN('queued','preparing','running')) THEN status WHEN EXISTS(SELECT 1 FROM agent_runs WHERE task_id=(SELECT task_id FROM agent_runs WHERE id=?1) AND status='failed') THEN 'failed' WHEN EXISTS(SELECT 1 FROM agent_runs WHERE task_id=(SELECT task_id FROM agent_runs WHERE id=?1) AND status='cancelled') THEN 'cancelled' ELSE 'review' END,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=(SELECT task_id FROM agent_runs WHERE id=?1)",[&event_run_id]);
                }
                ports.release(run.port);
                let _ = event_channel.send(RunStreamEvent::StatusChanged {
                    run_id: event_run_id.clone(),
                    status: reported_status,
                });
            }
        });
        let process_id = match self.processes.launch(
            run_id.clone(),
            ProcessSpec {
                executable,
                arguments,
                cwd: run.worktree,
                environment,
                log_path: run.log,
                stdin: stdin.then(|| {
                    let mut bytes = run.prompt.into_bytes();
                    bytes.push(b'\n');
                    bytes
                }),
            },
            sink,
        ) {
            Ok(id) => id,
            Err(error) => {
                self.ports.release(run.port);
                self.database.connect()?.execute("UPDATE agent_runs SET status='failed',ended_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1", [&run_id])?;
                return Err(error);
            }
        };
        self.database.connect()?.execute("UPDATE agent_runs SET status='running',process_identity=?1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2",params![process_id.map(|id|id.to_string()),run_id])?;
        let _ = channel.send(RunStreamEvent::Started { run_id });
        Ok(())
    }

    fn list(&self, task_id: &str) -> Result<Vec<Run>, CommandError> {
        let connection = self.database.connect()?;
        let mut statement=connection.prepare("SELECT r.id,r.task_id,r.provider_account_id,p.display_name,r.instruction,r.status,w.path,r.raw_log_path,r.context_pack_path,w.environment_manifest_json,r.updated_at FROM agent_runs r JOIN provider_accounts p ON p.id=r.provider_account_id LEFT JOIN worktrees w ON w.agent_run_id=r.id WHERE r.task_id=?1 ORDER BY r.created_at")?;
        statement
            .query_map([task_id], row_to_run)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
    fn get(&self, id: &str) -> Result<Option<Run>, CommandError> {
        let connection = self.database.connect()?;
        connection.query_row("SELECT r.id,r.task_id,r.provider_account_id,p.display_name,r.instruction,r.status,w.path,r.raw_log_path,r.context_pack_path,w.environment_manifest_json,r.updated_at FROM agent_runs r JOIN provider_accounts p ON p.id=r.provider_account_id LEFT JOIN worktrees w ON w.agent_run_id=r.id WHERE r.id=?1",[id],row_to_run).optional().map_err(Into::into)
    }
    fn stop(&self, id: &str) -> Result<(), CommandError> {
        self.database.connect()?.execute("UPDATE agent_runs SET status='cancelled',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1 AND status IN('queued','preparing','running')",[id])?;
        self.processes.stop(id)
    }
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<Run> {
    let manifest: Option<String> = row.get(9)?;
    let port = manifest
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|v| v.get("port").and_then(|p| p.as_u64()))
        .map(|p| p as u16);
    Ok(Run {
        id: row.get(0)?,
        task_id: row.get(1)?,
        provider_id: row.get(2)?,
        provider_name: row.get(3)?,
        instruction: row.get(4)?,
        status: row.get(5)?,
        worktree_path: row.get(6)?,
        raw_log_path: row.get(7)?,
        context_pack_path: row.get(8)?,
        port,
        updated_at: row.get(10)?,
    })
}
fn base_environment(home: &Path, port: u16) -> Vec<(String, String)> {
    let mut values = Vec::new();
    for key in [
        "PATH",
        "LANG",
        "LC_ALL",
        "TERM",
        "SYSTEMROOT",
        "WINDIR",
        "PATHEXT",
    ] {
        if let Ok(value) = std::env::var(key) {
            values.push((key.into(), value));
        }
    }
    values.extend([
        ("HOME".into(), home.to_string_lossy().into()),
        (
            "XDG_CONFIG_HOME".into(),
            home.join("xdg").to_string_lossy().into(),
        ),
        ("SUBSHELL_PORT".into(), port.to_string()),
    ]);
    values
}
fn io_error(error: std::io::Error) -> CommandError {
    CommandError::new("filesystem_error", error.to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::{
        context::PreviewInput, projects, providers::GenericProfile, tasks::CreateTask,
    };
    use std::{os::unix::fs::PermissionsExt, process::Command, thread, time::Duration};
    use tempfile::tempdir;

    #[test]
    fn runs_two_stand_ins_in_isolated_worktrees_and_stops_only_one() {
        let root = tempdir().unwrap();
        let repository = root.path().join("repository");
        fs::create_dir(&repository).unwrap();
        for arguments in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            assert!(
                Command::new("git")
                    .args(arguments)
                    .current_dir(&repository)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        fs::write(repository.join("README.md"), "unchanged").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "."])
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-qm", "base"])
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );

        let paths = RuntimePaths {
            data_dir: root.path().join("data"),
        };
        let database = Database::initialize(&paths.data_dir.join("db.sqlite3")).unwrap();
        let git = GitService::default();
        let project = projects::open(repository.to_str().unwrap(), &database, &git).unwrap();
        let task = tasks::create(
            CreateTask {
                project_id: project.id,
                title: "Parallel fixture".into(),
                description: "Run both".into(),
                acceptance_criteria: vec!["Both finish".into()],
                allowed_paths: vec!["README.md".into()],
                validation_commands: vec![],
                decisions: vec![],
                confirm_dirty_base: false,
            },
            &database,
            &git,
        )
        .unwrap();
        let executable = root.path().join("stand-in");
        fs::write(
            &executable,
            "#!/bin/sh\nprintf 'started\\n'\nsleep 0.4\nprintf 'finished\\n'\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let profile = GenericProfile {
            id: "stand-in".into(),
            display_name: "Stand-in".into(),
            executable_path: executable.to_string_lossy().into(),
            arguments: vec!["{prompt}".into()],
            prompt_mode: "argument".into(),
            config_root_env_var: Some("STANDIN_HOME".into()),
            config_source_path: None,
        };
        providers::save(&profile, &database, &paths).unwrap();
        let drafts = ContextDrafts::default();
        let preview = || {
            context::build(
                PreviewInput {
                    task_id: task.id.clone(),
                    instruction: "Work safely".into(),
                    selected_files: vec!["README.md".into()],
                    pattern: None,
                },
                &database,
                &git,
                &drafts,
            )
            .unwrap()
        };
        let first = preview();
        let second = preview();
        assert_eq!(first.content, second.content);
        fs::write(repository.join("large.txt"), vec![b'x'; 70 * 1024]).unwrap();
        let omitted = context::build(
            PreviewInput {
                task_id: task.id.clone(),
                instruction: "Work safely".into(),
                selected_files: vec!["large.txt".into()],
                pattern: None,
            },
            &database,
            &git,
            &drafts,
        )
        .unwrap();
        assert!(omitted.manifest.total_bytes <= omitted.manifest.budget_bytes);
        assert!(
            omitted
                .manifest
                .entries
                .iter()
                .any(|entry| entry.source == "large.txt" && !entry.included)
        );
        let service = RunService::new(
            database.clone(),
            paths,
            git,
            drafts,
            ProcessSupervisor::default(),
            PortLeases::default(),
        );
        let assignment = |preview: context::ContextPreview| Assignment {
            provider_id: profile.id.clone(),
            instruction: "Work safely".into(),
            context_token: preview.token,
            approved_context: preview.content,
            environment_files: vec![],
        };
        let runs = service
            .start(
                StartInput {
                    task_id: task.id.clone(),
                    assignments: vec![assignment(first), assignment(second)],
                },
                Channel::new(|_| Ok(())),
            )
            .unwrap();
        assert_eq!(runs.len(), 2);
        assert_ne!(runs[0].worktree_path, runs[1].worktree_path);
        assert_ne!(runs[0].port, runs[1].port);
        service.stop(&runs[0].id).unwrap();
        for _ in 0..50 {
            let states = service.list(&task.id).unwrap();
            if states
                .iter()
                .all(|run| matches!(run.status.as_str(), "cancelled" | "succeeded" | "failed"))
            {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let states = service.list(&task.id).unwrap();
        assert_eq!(states[0].status, "cancelled");
        assert_eq!(states[1].status, "succeeded");
        assert_eq!(
            fs::read_to_string(repository.join("README.md")).unwrap(),
            "unchanged"
        );
        assert!(states.iter().all(|run| {
            !Path::new(run.worktree_path.as_ref().unwrap())
                .join("context.md")
                .exists()
        }));
        assert!(states.iter().all(|run| {
            run.raw_log_path
                .as_ref()
                .is_some_and(|path| Path::new(path).exists())
        }));
    }
}
