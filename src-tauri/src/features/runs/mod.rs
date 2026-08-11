use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::Duration,
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
        timeline::{self, EventRefs},
    },
    platform::{
        database::Database,
        environment::{self, PortLeases, RuntimePaths},
        git::{GitDiff, GitService},
        keychain::SecretStore,
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
    secrets: Arc<dyn SecretStore>,
}
impl RunService {
    pub fn new(
        database: Database,
        paths: RuntimePaths,
        git: GitService,
        drafts: ContextDrafts,
        processes: ProcessSupervisor,
        ports: PortLeases,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            database,
            paths,
            git,
            drafts,
            processes,
            ports,
            secrets,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assignment {
    pub provider_id: String,
    pub instruction: String,
    #[serde(default = "default_role")]
    pub role: String,
    #[serde(default)]
    pub title: Option<String>,
    pub context_token: String,
    pub approved_context: String,
    #[serde(default)]
    pub environment_files: Vec<String>,
    #[serde(default)]
    pub full_access: bool,
    pub unit_limit: Option<u64>,
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
    #[serde(default)]
    pub tail: bool,
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
pub struct PlanId {
    pub plan_id: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovePlan {
    pub plan_id: String,
    #[serde(default)]
    pub full_access: bool,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentInput {
    pub project_id: String,
    pub files: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuitInput {
    pub decision: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub task_id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub instruction: String,
    pub role: String,
    pub title: Option<String>,
    pub status: String,
    pub waiting_reason: Option<String>,
    pub worktree_path: Option<String>,
    pub raw_log_path: Option<String>,
    pub context_pack_path: Option<String>,
    pub provider_session_id: Option<String>,
    pub can_resume: bool,
    pub resume_count: u32,
    pub full_access: bool,
    pub reported_input_tokens: Option<u64>,
    pub reported_output_tokens: Option<u64>,
    pub depends_on_run_ids: Vec<String>,
    pub retry_of_run_id: Option<String>,
    pub unit_limit: Option<u64>,
    pub port: Option<u16>,
    pub updated_at: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPlanAssignment {
    pub id: String,
    pub title: String,
    pub instruction: String,
    pub role: String,
    pub allowed_paths: Vec<String>,
    pub depends_on: Vec<String>,
    pub position: u32,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPlan {
    pub id: String,
    pub task_id: String,
    pub planner_run_id: String,
    pub summary: String,
    pub status: String,
    pub assignments: Vec<TaskPlanAssignment>,
    pub created_at: String,
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
pub fn runs_enqueue(
    input: StartInput,
    service: State<RunService>,
) -> Result<Vec<Run>, CommandError> {
    service.enqueue(input)
}
#[tauri::command]
pub fn runs_list(input: TaskId, service: State<RunService>) -> Result<Page<Run>, CommandError> {
    Ok(Page::first(service.list(&input.task_id)?))
}
#[tauri::command]
pub fn runs_plan_get(
    input: TaskId,
    service: State<RunService>,
) -> Result<Option<TaskPlan>, CommandError> {
    service.plan(&input.task_id)
}
#[tauri::command]
pub fn runs_plan_approve(
    input: ApprovePlan,
    on_event: Channel<RunStreamEvent>,
    service: State<RunService>,
) -> Result<TaskPlan, CommandError> {
    service.approve_plan(&input.plan_id, input.full_access, on_event)
}
#[tauri::command]
pub fn runs_plan_reject(
    input: PlanId,
    service: State<RunService>,
) -> Result<TaskPlan, CommandError> {
    service.reject_plan(&input.plan_id)
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
    let limit = input.limit.unwrap_or(64 * 1024);
    let (bytes, next_cursor) = if input.tail {
        process::read_log_tail(Path::new(&path), limit)?
    } else {
        process::read_log(Path::new(&path), input.cursor, limit)?
    };
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
#[tauri::command]
pub fn runs_mark_complete(input: RunId, service: State<RunService>) -> Result<Run, CommandError> {
    service.mark_complete(&input.run_id)
}
#[tauri::command]
pub fn runs_resume(
    input: RunId,
    on_event: Channel<RunStreamEvent>,
    service: State<RunService>,
) -> Result<Run, CommandError> {
    service.resume(&input.run_id, on_event)
}
#[tauri::command]
pub fn runs_retry(
    input: RunId,
    on_event: Channel<RunStreamEvent>,
    service: State<RunService>,
) -> Result<Run, CommandError> {
    service.retry(&input.run_id, on_event)
}
#[tauri::command]
pub fn runs_diff(input: RunId, service: State<RunService>) -> Result<GitDiff, CommandError> {
    service.diff(&input.run_id)
}

#[tauri::command]
pub fn runs_resources(
    input: RunId,
    service: State<RunService>,
) -> Result<process::ProcessUsage, CommandError> {
    if service.get(&input.run_id)?.is_none() {
        return Err(CommandError::new("run_not_found", "Run was not found"));
    }
    Ok(service.processes.usage(&input.run_id))
}

#[tauri::command]
pub fn runs_decide_quit<R: tauri::Runtime>(
    input: QuitInput,
    service: State<RunService>,
    app: tauri::AppHandle<R>,
    window: tauri::WebviewWindow<R>,
) -> Result<(), CommandError> {
    match input.decision.as_str() {
        "preserve" => window.minimize().map_err(tauri_error),
        "stop" => {
            service.stop_active()?;
            app.exit(0);
            Ok(())
        }
        "cancel" => Ok(()),
        _ => Err(CommandError::new(
            "invalid_quit_decision",
            "Quit decision must be preserve, stop, or cancel",
        )),
    }
}

struct Prepared {
    run_id: String,
    task_id: String,
    project_id: String,
    provider: providers::ResolvedProvider,
    worktree: PathBuf,
    log: PathBuf,
    config_root: PathBuf,
    port: u16,
    prompt: String,
    provider_session_id: Option<String>,
    resume: bool,
    role: String,
    full_access: bool,
}

impl RunService {
    pub(crate) fn start_approved(&self, input: StartInput) -> Result<Vec<Run>, CommandError> {
        self.start(input, Channel::new(|_| Ok(())))
    }

    pub(crate) fn clean_resources(&self, id: &str) -> Result<(), CommandError> {
        let run = self
            .get(id)?
            .ok_or_else(|| CommandError::new("run_not_found", "Run was not found"))?;
        let project_path: PathBuf = self
            .database
            .connect()?
            .query_row(
                "SELECT p.path FROM tasks t JOIN projects p ON p.id=t.project_id WHERE t.id=?1",
                [&run.task_id],
                |row| row.get::<_, String>(0),
            )
            .map(PathBuf::from)?;
        if self.processes.is_active(id) {
            self.processes.stop(id)?;
        }
        if let Some(path) = run.worktree_path.as_deref() {
            self.git.remove_worktree(&project_path, Path::new(path))?;
        }
        let config = self.paths.data_dir.join("runs").join(id).join("config");
        if config.exists() {
            fs::remove_dir_all(config).map_err(io_error)?;
        }
        if let Some(port) = run.port {
            self.ports.release(port);
        }
        let mut connection = self.database.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute("UPDATE agent_runs SET status='cancelled',waiting_reason=NULL,ended_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1", [id])?;
        transaction.execute("UPDATE worktrees SET state='discarded',cleaned_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE agent_run_id=?1", [id])?;
        tasks::rollup_in_transaction(&transaction, &run.task_id)?;
        transaction.commit()?;
        Ok(())
    }

    fn retry(&self, id: &str, channel: Channel<RunStreamEvent>) -> Result<Run, CommandError> {
        let previous = self
            .get(id)?
            .ok_or_else(|| CommandError::new("run_not_found", "Run was not found"))?;
        if !matches!(
            previous.status.as_str(),
            "failed" | "cancelled" | "succeeded"
        ) {
            return Err(CommandError::new(
                "run_not_finished",
                "Only finished Runs can be re-run",
            ));
        }
        ensure_usage_available(&self.database, &previous.task_id, Some(id))?;
        let task = tasks::get(&self.database, &previous.task_id)?
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
        let (manifest_json, environment_json): (String, Option<String>) = self.database.connect()?.query_row(
            "SELECT r.context_manifest_json,w.environment_manifest_json FROM agent_runs r LEFT JOIN worktrees w ON w.agent_run_id=r.id WHERE r.id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let content =
            fs::read_to_string(previous.context_pack_path.as_deref().ok_or_else(|| {
                CommandError::new(
                    "context_unavailable",
                    "The original Run context is unavailable",
                )
            })?)
            .map_err(io_error)?;
        let token = context::restore(
            &self.drafts,
            serde_json::from_str(&manifest_json).map_err(json_error)?,
        );
        let environment_files = environment_json
            .as_deref()
            .map(serde_json::from_str::<environment::EnvironmentPreview>)
            .transpose()
            .map_err(json_error)?
            .map(|preview| preview.files)
            .unwrap_or_default();
        let prior_patch = previous
            .worktree_path
            .as_deref()
            .map(|path| self.git.exact_diff(Path::new(path), &task.base_revision))
            .transpose()?;
        let prepared = self
            .prepare(
                &task,
                &project_path,
                Assignment {
                    provider_id: previous.provider_id.clone(),
                    instruction: previous.instruction.clone(),
                    role: previous.role.clone(),
                    title: previous.title.clone(),
                    context_token: token,
                    approved_context: content,
                    environment_files,
                    full_access: previous.full_access,
                    unit_limit: previous.unit_limit,
                },
                "preparing",
            )
            .map_err(|(_, error)| error)?;
        let new_id = prepared.run_id.clone();
        if let Some(patch) = prior_patch
            && let Err(error) =
                self.git
                    .restore_snapshot(&prepared.worktree, &task.base_revision, &patch.patch)
        {
            self.ports.release(prepared.port);
            let _ = self.mark_preparation_failed(&task, &new_id);
            return Err(error);
        }
        let update = (|| -> Result<(), CommandError> {
            let mut connection = self.database.connect()?;
            let transaction = connection.transaction()?;
            transaction.execute(
                "UPDATE agent_runs SET retry_of_run_id=?1 WHERE id=?2",
                params![id, new_id],
            )?;
            transaction.execute(
                "UPDATE agent_runs SET depends_on_run_ids_json=replace(depends_on_run_ids_json,json_quote(?1),json_quote(?2)) WHERE task_id=?3 AND status='queued' AND EXISTS(SELECT 1 FROM json_each(depends_on_run_ids_json) WHERE value=?1)",
                params![id, new_id, task.id],
            )?;
            transaction.commit()?;
            Ok(())
        })();
        if let Err(error) = update {
            self.ports.release(prepared.port);
            let _ = self.mark_preparation_failed(&task, &new_id);
            return Err(error);
        }
        self.launch(prepared, channel)?;
        self.get(&new_id)?
            .ok_or_else(|| CommandError::new("run_not_found", "Re-run was not created"))
    }

    pub(crate) fn pause_for_checkpoint(&self, id: &str) -> Result<(), CommandError> {
        let run = self
            .get(id)?
            .ok_or_else(|| CommandError::new("run_not_found", "Run was not found"))?;
        if !matches!(run.status.as_str(), "preparing" | "running" | "waiting") {
            return Ok(());
        }
        if !run.can_resume {
            return Err(CommandError::new(
                "checkpoint_unsupported",
                "This provider cannot resume the session after a checkpoint",
            ));
        }
        self.processes.stop(id)?;
        for _ in 0..40 {
            if !self.processes.is_active(id) {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(25));
        }
        Err(CommandError::new(
            "checkpoint_timeout",
            "The agent did not pause; its worktree was left unchanged",
        ))
    }

    fn start(
        &self,
        input: StartInput,
        channel: Channel<RunStreamEvent>,
    ) -> Result<Vec<Run>, CommandError> {
        let task_id = input.task_id.clone();
        self.enqueue(input)?;
        self.dispatch_task(&task_id, channel)?;
        self.list(&task_id)
    }

    fn enqueue(&self, input: StartInput) -> Result<Vec<Run>, CommandError> {
        if input.assignments.is_empty() {
            return Err(CommandError::new(
                "invalid_assignments",
                "At least one assignment is required",
            ));
        }
        let task = tasks::get(&self.database, &input.task_id)?
            .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
        ensure_usage_available(&self.database, &task.id, None)?;
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
            match self.prepare(&task, &project_path, assignment, "queued") {
                Ok(run) => prepared.push(run),
                Err((_, error)) => return Err(error),
            }
        }
        for run in &prepared {
            self.ports.release(run.port);
        }
        let mut connection = self.database.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute("UPDATE tasks SET queue_position=COALESCE(queue_position,(SELECT COALESCE(MAX(queue_position),0)+1 FROM tasks WHERE project_id=?1)),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2", params![task.project_id, task.id])?;
        transaction.execute("UPDATE worktrees SET environment_manifest_json=json_set(COALESCE(environment_manifest_json,'{}'),'$.port',NULL) WHERE agent_run_id IN (SELECT id FROM agent_runs WHERE task_id=?1 AND status='queued')", [&task.id])?;
        tasks::rollup_in_transaction(&transaction, &task.id)?;
        transaction.commit()?;
        self.list(&input.task_id)
    }

    fn prepare(
        &self,
        task: &tasks::Task,
        project_path: &Path,
        assignment: Assignment,
        initial_status: &str,
    ) -> Result<Prepared, (String, CommandError)> {
        let run_id = Uuid::new_v4().to_string();
        let result = (|| {
            if !matches!(
                assignment.role.as_str(),
                "planner"
                    | "executor"
                    | "implementer"
                    | "research"
                    | "test"
                    | "tester"
                    | "reviewer"
                    | "debugger"
            ) {
                return Err(CommandError::new(
                    "invalid_run_role",
                    "Run role is not supported",
                ));
            }
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
            let unit_limit = assignment
                .unit_limit
                .map(i64::try_from)
                .transpose()
                .map_err(|_| CommandError::new("invalid_unit_limit", "Usage limit is too large"))?;
            if assignment.full_access {
                provider.ensure_full_access_supported()?;
            }
            let provider_session_id = provider.new_session_id();
            let run_dir = self.paths.data_dir.join("runs").join(&run_id);
            let worktree = self
                .paths
                .data_dir
                .join("worktrees")
                .join(&task.project_id)
                .join(&run_id);
            let config_root = provider.runtime_config_root(run_dir.join("config"));
            let log = run_dir.join("output.log");
            let context_path = run_dir.join("context.md");
            fs::create_dir_all(&run_dir).map_err(io_error)?;
            let mut connection = self.database.connect()?;
            let transaction = connection.transaction()?;
            transaction.execute("INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,role,assignment_title,status,merge_order,raw_log_path,context_pack_path,context_manifest_json,context_sha256,provider_session_id,full_access,unit_limit,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,0,?8,?9,?10,?11,?12,?13,?14,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'))",params![run_id,task.id,provider.id,assignment.instruction,assignment.role,assignment.title,initial_status,log.to_string_lossy(),context_path.to_string_lossy(),serde_json::to_string(&manifest).unwrap(),context_sha256,provider_session_id,assignment.full_access,unit_limit])?;
            timeline::append(
                &transaction,
                EventRefs {
                    project_id: &task.project_id,
                    task_id: Some(&task.id),
                    run_id: Some(&run_id),
                    provider_id: Some(&provider.id),
                },
                &format!("run.{initial_status}"),
                serde_json::json!({ "instruction": assignment.instruction }),
            )?;
            tasks::rollup_in_transaction(&transaction, &task.id)?;
            transaction.commit()?;
            self.git
                .create_worktree(project_path, &task.base_revision, &worktree)?;
            let mut environment =
                environment::copy_files(project_path, &worktree, &assignment.environment_files)?;
            if config_root == run_dir.join("config")
                && let Some(source) = provider.config_source_path.as_deref()
            {
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
                task_id: task.id.clone(),
                project_id: task.project_id.clone(),
                provider,
                worktree,
                log,
                config_root,
                port,
                prompt: assignment.approved_context,
                provider_session_id,
                resume: false,
                role: assignment.role,
                full_access: assignment.full_access,
            })
        })();
        if result.is_err() {
            let _ = self.mark_preparation_failed(task, &run_id);
        }
        result.map_err(|error| (run_id, error))
    }

    fn mark_preparation_failed(
        &self,
        task: &tasks::Task,
        run_id: &str,
    ) -> Result<(), CommandError> {
        let mut connection = self.database.connect()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute("UPDATE agent_runs SET status='failed',ended_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",[run_id])?;
        if changed == 0 {
            return Ok(());
        }
        timeline::append(
            &transaction,
            EventRefs {
                project_id: &task.project_id,
                task_id: Some(&task.id),
                run_id: Some(run_id),
                provider_id: None,
            },
            "run.status_changed",
            serde_json::json!({"to":"failed","reason":"preparation_failed"}),
        )?;
        tasks::rollup_in_transaction(&transaction, &task.id)?;
        transaction.commit()?;
        Ok(())
    }

    fn dispatch_task(
        &self,
        task_id: &str,
        channel: Channel<RunStreamEvent>,
    ) -> Result<bool, CommandError> {
        let task = tasks::get(&self.database, task_id)?
            .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
        let connection = self.database.connect()?;
        let other_active: i64 = connection.query_row(
            "SELECT COUNT(*) FROM agent_runs r JOIN tasks t ON t.id=r.task_id WHERE t.project_id=?1 AND t.id<>?2 AND r.status IN('preparing','running','waiting')",
            params![task.project_id, task_id],
            |row| row.get(0),
        )?;
        if other_active > 0 {
            return Ok(false);
        }
        connection.execute(
            "UPDATE agent_runs AS queued SET waiting_reason=CASE WHEN EXISTS(SELECT 1 FROM json_each(queued.depends_on_run_ids_json) dependency JOIN agent_runs required ON required.id=dependency.value WHERE required.status<>'succeeded') THEN 'Waiting for prerequisite agent' ELSE NULL END WHERE queued.task_id=?1 AND queued.status='queued'",
            [task_id],
        )?;
        let mut statement = connection.prepare(
            "SELECT queued.id FROM agent_runs queued WHERE queued.task_id=?1 AND queued.status='queued' AND NOT EXISTS(SELECT 1 FROM json_each(queued.depends_on_run_ids_json) dependency JOIN agent_runs required ON required.id=dependency.value WHERE required.status<>'succeeded') ORDER BY queued.created_at",
        )?;
        let ids = statement
            .query_map([task_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        drop(connection);
        if ids.is_empty() {
            return Ok(false);
        }
        let mut prepared = Vec::new();
        for id in ids {
            let run = self
                .get(&id)?
                .ok_or_else(|| CommandError::new("run_not_found", "Queued Run was not found"))?;
            let provider = providers::resolve(&self.database, &run.provider_id)?;
            let worktree = run
                .worktree_path
                .as_deref()
                .map(PathBuf::from)
                .filter(|path| path.is_dir())
                .ok_or_else(|| {
                    CommandError::new("worktree_not_found", "Queued Run worktree is unavailable")
                })?;
            let log = run
                .raw_log_path
                .as_deref()
                .map(PathBuf::from)
                .ok_or_else(|| {
                    CommandError::new("output_unavailable", "Queued Run log is unavailable")
                })?;
            let context_path = run
                .context_pack_path
                .as_deref()
                .map(PathBuf::from)
                .ok_or_else(|| {
                    CommandError::new("context_unavailable", "Queued Run context is unavailable")
                })?;
            let prompt = fs::read_to_string(context_path).map_err(io_error)?;
            let port = self.ports.acquire()?;
            let mut connection = self.database.connect()?;
            let transaction = connection.transaction()?;
            transaction.execute("UPDATE worktrees SET environment_manifest_json=json_set(COALESCE(environment_manifest_json,'{}'),'$.port',?1) WHERE agent_run_id=?2", params![port, id])?;
            transaction.execute("UPDATE agent_runs SET status='preparing',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1 AND status='queued'", [&id])?;
            timeline::append(
                &transaction,
                EventRefs {
                    project_id: &task.project_id,
                    task_id: Some(task_id),
                    run_id: Some(&id),
                    provider_id: Some(&run.provider_id),
                },
                "run.preparing",
                serde_json::json!({ "source": "queue" }),
            )?;
            tasks::rollup_in_transaction(&transaction, task_id)?;
            transaction.commit()?;
            let config_root = provider
                .runtime_config_root(self.paths.data_dir.join("runs").join(&id).join("config"));
            prepared.push(Prepared {
                run_id: run.id,
                task_id: run.task_id,
                project_id: task.project_id.clone(),
                provider,
                worktree,
                log,
                config_root,
                port,
                prompt,
                provider_session_id: run.provider_session_id,
                resume: false,
                role: run.role,
                full_access: run.full_access,
            });
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
        self.database.connect()?.execute(
            "UPDATE tasks SET queue_position=NULL WHERE id=?1",
            [task_id],
        )?;
        Ok(launched)
    }

    fn dispatch_next(
        &self,
        project_id: &str,
        channel: Channel<RunStreamEvent>,
    ) -> Result<bool, CommandError> {
        let connection = self.database.connect()?;
        let active: i64 = connection.query_row(
            "SELECT COUNT(*) FROM agent_runs r JOIN tasks t ON t.id=r.task_id WHERE t.project_id=?1 AND r.status IN('preparing','running','waiting')",
            [project_id],
            |row| row.get(0),
        )?;
        if active > 0 {
            return Ok(false);
        }
        let task_id: Option<String> = connection
            .query_row(
                "SELECT t.id FROM tasks t WHERE t.project_id=?1 AND EXISTS(SELECT 1 FROM agent_runs r WHERE r.task_id=t.id AND r.status='queued' AND NOT EXISTS(SELECT 1 FROM json_each(r.depends_on_run_ids_json) dependency JOIN agent_runs required ON required.id=dependency.value WHERE required.status<>'succeeded')) ORDER BY t.queue_position,t.created_at LIMIT 1",
                [project_id],
                |row| row.get(0),
            )
            .optional()?;
        drop(connection);
        match task_id {
            Some(task_id) => self.dispatch_task(&task_id, channel),
            None => Ok(false),
        }
    }

    fn plan(&self, task_id: &str) -> Result<Option<TaskPlan>, CommandError> {
        let connection = self.database.connect()?;
        let plan = connection
            .query_row(
                "SELECT id,planner_run_id,summary,status,created_at FROM task_plans WHERE task_id=?1 ORDER BY attempt_number DESC LIMIT 1",
                [task_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?)),
            )
            .optional()?;
        let Some((id, planner_run_id, summary, status, created_at)) = plan else {
            return Ok(None);
        };
        let assignments = {
            let mut statement = connection.prepare("SELECT id,title,instruction,role,allowed_paths_json,depends_on_json,position FROM task_plan_assignments WHERE plan_id=?1 ORDER BY position")?;
            statement
                .query_map([&id], |row| {
                    Ok(TaskPlanAssignment {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        instruction: row.get(2)?,
                        role: row.get(3)?,
                        allowed_paths: serde_json::from_str(&row.get::<_, String>(4)?)
                            .unwrap_or_default(),
                        depends_on: serde_json::from_str(&row.get::<_, String>(5)?)
                            .unwrap_or_default(),
                        position: row.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(Some(TaskPlan {
            id,
            task_id: task_id.into(),
            planner_run_id,
            summary,
            status,
            assignments,
            created_at,
        }))
    }

    fn approve_plan(
        &self,
        plan_id: &str,
        full_access: bool,
        channel: Channel<RunStreamEvent>,
    ) -> Result<TaskPlan, CommandError> {
        let (task_id, planner_run_id, status): (String, String, String) = self
            .database
            .connect()?
            .query_row(
                "SELECT task_id,planner_run_id,status FROM task_plans WHERE id=?1",
                [plan_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or_else(|| CommandError::new("plan_not_found", "Task plan was not found"))?;
        if status == "rejected" {
            return Err(CommandError::new(
                "plan_rejected",
                "This task plan was rejected",
            ));
        }
        if status == "proposed"
            && !self.launch_submitted_plan(&planner_run_id, full_access, channel.clone())?
        {
            return Err(CommandError::new(
                "plan_not_launchable",
                "Task plan is no longer ready to launch",
            ));
        }
        if status == "proposed" {
            let task = tasks::get(&self.database, &task_id)?
                .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
            let assignment_count = self
                .plan(&task_id)?
                .map(|plan| plan.assignments.len())
                .unwrap_or(0);
            let mut connection = self.database.connect()?;
            let transaction = connection.transaction()?;
            transaction.execute("UPDATE agent_runs SET status='succeeded',waiting_reason=NULL,ended_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1 AND status IN('running','waiting')", [&planner_run_id])?;
            timeline::append(
                &transaction,
                EventRefs {
                    project_id: &task.project_id,
                    task_id: Some(&task_id),
                    run_id: Some(&planner_run_id),
                    provider_id: None,
                },
                "plan.approved",
                serde_json::json!({"planId":plan_id,"assignmentCount":assignment_count,"fullAccess":full_access}),
            )?;
            tasks::rollup_in_transaction(&transaction, &task_id)?;
            transaction.commit()?;
            let _ = channel.send(RunStreamEvent::StatusChanged {
                run_id: planner_run_id.clone(),
                status: "succeeded".into(),
            });
            let _ = self.processes.stop(&planner_run_id);
        }
        self.plan(&task_id)?.ok_or_else(|| {
            CommandError::new("plan_not_found", "Task plan was not found after launch")
        })
    }

    fn reject_plan(&self, plan_id: &str) -> Result<TaskPlan, CommandError> {
        let (task_id, planner_run_id, project_id, status): (String, String, String, String) = self.database.connect()?.query_row(
            "SELECT p.task_id,p.planner_run_id,t.project_id,p.status FROM task_plans p JOIN tasks t ON t.id=p.task_id WHERE p.id=?1",
            [plan_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?.ok_or_else(|| CommandError::new("plan_not_found", "Task plan was not found"))?;
        if status == "launched" {
            return Err(CommandError::new(
                "plan_already_launched",
                "A launched plan cannot be rejected",
            ));
        }
        if status == "proposed" {
            let mut connection = self.database.connect()?;
            let transaction = connection.transaction()?;
            transaction.execute(
                "UPDATE task_plans SET status='rejected' WHERE id=?1 AND status='proposed'",
                [plan_id],
            )?;
            timeline::append(
                &transaction,
                EventRefs {
                    project_id: &project_id,
                    task_id: Some(&task_id),
                    run_id: Some(&planner_run_id),
                    provider_id: None,
                },
                "plan.rejected",
                serde_json::json!({"planId":plan_id}),
            )?;
            transaction.commit()?;
            let _ = self.stop(&planner_run_id);
        }
        self.plan(&task_id)?.ok_or_else(|| {
            CommandError::new("plan_not_found", "Task plan was not found after rejection")
        })
    }

    fn launch_submitted_plan(
        &self,
        planner_run_id: &str,
        full_access: bool,
        channel: Channel<RunStreamEvent>,
    ) -> Result<bool, CommandError> {
        let connection = self.database.connect()?;
        let plan: Option<(String, String)> = connection
            .query_row(
                "SELECT id,task_id FROM task_plans WHERE planner_run_id=?1 AND status='proposed' ORDER BY attempt_number DESC LIMIT 1",
                [planner_run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((plan_id, task_id)) = plan else {
            return Ok(false);
        };
        let assignments = {
            let mut statement = connection.prepare(
                "SELECT title,instruction,role,allowed_paths_json,depends_on_json FROM task_plan_assignments WHERE plan_id=?1 ORDER BY position",
            )?;
            statement
                .query_map([&plan_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        serde_json::from_str::<Vec<String>>(&row.get::<_, String>(3)?)
                            .unwrap_or_default(),
                        serde_json::from_str::<Vec<String>>(&row.get::<_, String>(4)?)
                            .unwrap_or_default(),
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        drop(connection);
        let providers = providers::list(&self.database)?;
        if providers.is_empty() {
            return Err(CommandError::new(
                "provider_not_found",
                "Configure at least one coding CLI before launching the plan",
            ));
        }
        let dependencies = assignments
            .iter()
            .map(|(title, _, _, _, dependencies)| (title.clone(), dependencies.clone()))
            .collect::<Vec<_>>();
        let mut prepared = Vec::new();
        for (position, (title, instruction, role, allowed_paths, _)) in
            assignments.into_iter().enumerate()
        {
            let instruction = if allowed_paths.is_empty() {
                instruction
            } else {
                format!(
                    "{instruction}\n\nAssignment boundaries (do not modify outside these paths):\n{}",
                    allowed_paths.join("\n")
                )
            };
            let preview = context::build(
                context::PreviewInput {
                    task_id: task_id.clone(),
                    instruction: instruction.clone(),
                    selected_files: allowed_paths,
                    pattern: None,
                },
                &self.database,
                &self.git,
                &self.drafts,
            )?;
            prepared.push(Assignment {
                provider_id: providers[position % providers.len()].id.clone(),
                instruction,
                role,
                title: Some(title),
                context_token: preview.token,
                approved_context: preview.content,
                environment_files: Vec::new(),
                full_access,
                unit_limit: None,
            });
        }
        let runs = self.enqueue(StartInput {
            task_id: task_id.clone(),
            assignments: prepared,
        })?;
        let run_ids = runs
            .iter()
            .filter_map(|run| {
                run.title
                    .as_ref()
                    .map(|title| (title.to_lowercase(), run.id.clone()))
            })
            .collect::<std::collections::HashMap<_, _>>();
        let mut connection = self.database.connect()?;
        let transaction = connection.transaction()?;
        for (title, dependency_titles) in dependencies {
            let run_id = run_ids.get(&title.to_lowercase()).ok_or_else(|| {
                CommandError::new("plan_run_not_found", "A planned Run was not created")
            })?;
            let dependencies = dependency_titles
                .iter()
                .filter_map(|dependency| run_ids.get(&dependency.to_lowercase()).cloned())
                .collect::<Vec<_>>();
            transaction.execute(
                "UPDATE agent_runs SET depends_on_run_ids_json=?1,waiting_reason=CASE WHEN json_array_length(?1)>0 THEN 'Waiting for prerequisite agent' ELSE NULL END WHERE id=?2",
                params![serde_json::to_string(&dependencies).unwrap(), run_id],
            )?;
        }
        transaction.commit()?;
        self.dispatch_task(&task_id, channel)?;
        self.database.connect()?.execute(
            "UPDATE task_plans SET status='launched',launched_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1 AND status='proposed'",
            [&plan_id],
        )?;
        Ok(true)
    }

    fn mark_planning_attention(
        &self,
        task_id: &str,
        project_id: &str,
        planner_run_id: &str,
        reason: &str,
    ) -> Result<(), CommandError> {
        let mut connection = self.database.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE tasks SET status='waiting',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
            [task_id],
        )?;
        timeline::append(
            &transaction,
            EventRefs {
                project_id,
                task_id: Some(task_id),
                run_id: Some(planner_run_id),
                provider_id: None,
            },
            "plan.needs_attention",
            serde_json::json!({"reason": reason}),
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn recover_and_dispatch(&self) -> Result<(), CommandError> {
        let mut connection = self.database.connect()?;
        let active = {
            let mut statement = connection.prepare(
                "SELECT r.id,r.task_id,t.project_id,r.provider_account_id,r.process_identity,(json_array_length(g.resume_arguments_json)>0 AND (instr(g.resume_arguments_json,'{sessionId}')=0 OR r.provider_session_id IS NOT NULL)) FROM agent_runs r JOIN tasks t ON t.id=r.task_id JOIN generic_provider_profiles g ON g.provider_account_id=r.provider_account_id WHERE r.status IN('preparing','running','waiting')",
            )?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, bool>(5)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let transaction = connection.transaction()?;
        let mut resumable = Vec::new();
        for (run_id, task_id, project_id, provider_id, process_identity, recoverable) in active {
            if self.processes.is_active(&run_id) {
                continue;
            }
            let process_still_running = process_identity
                .as_deref()
                .is_some_and(ProcessSupervisor::identity_is_active);
            let waiting_reason = if process_still_running {
                "Process is still running but its terminal cannot be reattached; stop it before resuming"
            } else if recoverable {
                "Restart detected; resuming the provider session"
            } else {
                "Process lost; start a new session"
            };
            transaction.execute(
                "UPDATE agent_runs SET status='failed',process_identity=CASE WHEN ?1 THEN process_identity ELSE NULL END,waiting_reason=?2,ended_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?3",
                params![process_still_running, waiting_reason, run_id],
            )?;
            timeline::append(
                &transaction,
                EventRefs {
                    project_id: &project_id,
                    task_id: Some(&task_id),
                    run_id: Some(&run_id),
                    provider_id: Some(&provider_id),
                },
                "run.process_lost",
                serde_json::json!({ "to": "failed", "processIdentity": process_identity, "recoverable": recoverable, "processStillRunning": process_still_running }),
            )?;
            tasks::rollup_in_transaction(&transaction, &task_id)?;
            if recoverable && !process_still_running {
                resumable.push(run_id);
            }
        }
        transaction.commit()?;

        for run_id in resumable {
            // A failed resume remains an actionable failed Run; startup itself must stay usable.
            let _ = self.resume(&run_id, Channel::new(|_| Ok(())));
        }

        let connection = self.database.connect()?;
        let projects = {
            let mut statement = connection.prepare(
                "SELECT DISTINCT project_id FROM tasks WHERE EXISTS(SELECT 1 FROM agent_runs WHERE task_id=tasks.id AND status='queued')",
            )?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        drop(connection);
        for project_id in projects {
            self.dispatch_next(&project_id, Channel::new(|_| Ok(())))?;
        }
        Ok(())
    }

    pub fn active_count(&self) -> Result<u32, CommandError> {
        Ok(self.database.connect()?.query_row(
            "SELECT COUNT(*) FROM agent_runs WHERE status IN('queued','preparing','running','waiting')",
            [],
            |row| row.get(0),
        )?)
    }

    fn stop_active(&self) -> Result<(), CommandError> {
        let connection = self.database.connect()?;
        let ids = {
            let mut statement = connection.prepare(
                "SELECT id FROM agent_runs WHERE status IN('queued','preparing','running','waiting')",
            )?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        drop(connection);
        for id in ids {
            self.stop(&id)?;
        }
        Ok(())
    }

    fn launch(&self, run: Prepared, channel: Channel<RunStreamEvent>) -> Result<(), CommandError> {
        let (executable, arguments, stdin) = if run.resume {
            run.provider.resume_command(
                &run.config_root,
                run.provider_session_id.as_deref(),
                run.full_access,
            )?
        } else {
            run.provider.launch_command(
                &run.prompt,
                &run.config_root,
                run.provider_session_id.as_deref(),
                run.full_access,
            )?
        };
        let mut environment =
            base_environment(&run.config_root, run.port, run.provider.inherit_user_home);
        environment.extend([
            (
                "SUBSHELL_DATA_DIR".into(),
                self.paths.data_dir.to_string_lossy().into(),
            ),
            ("SUBSHELL_PROJECT_ID".into(), run.project_id.clone()),
            ("SUBSHELL_TASK_ID".into(), run.task_id.clone()),
            ("SUBSHELL_RUN_ID".into(), run.run_id.clone()),
            ("SUBSHELL_RUN_ROLE".into(), run.role.clone()),
        ]);
        if let Ok(executable) = std::env::current_exe() {
            environment.push((
                "SUBSHELL_CONTROL".into(),
                executable.to_string_lossy().into(),
            ));
        }
        if let Some(name) = &run.provider.config_root_env_var {
            environment.push((name.clone(), run.config_root.to_string_lossy().into()));
        }
        let mut redactions = Vec::new();
        if let Some(name) = run.provider.secret_environment_key()
            && let Some(secret) = self.secrets.get(&run.provider.id)?
        {
            let value = String::from_utf8(secret.clone()).map_err(|_| {
                CommandError::new(
                    "invalid_provider_secret",
                    "Provider credential is not UTF-8",
                )
            })?;
            environment.push((name.into(), value));
            redactions.push(secret);
        }
        let database = self.database.clone();
        let ports = self.ports.clone();
        let run_id = run.run_id.clone();
        let task_id = run.task_id.clone();
        let project_id = run.project_id.clone();
        let provider_id = run.provider.id.clone();
        let event_run_id = run_id.clone();
        let event_task_id = task_id.clone();
        let event_project_id = project_id.clone();
        let event_provider_id = provider_id.clone();
        let event_role = run.role.clone();
        let event_log = run.log.clone();
        let output_parser = run.provider;
        let event_channel = channel.clone();
        let dispatcher = self.clone();
        let sink = Arc::new(move |notice| match notice {
            ProcessNotice::Output { bytes, cursor } => {
                let _ = event_channel.send(RunStreamEvent::Output {
                    run_id: event_run_id.clone(),
                    bytes,
                    cursor,
                });
            }
            ProcessNotice::Exited { success, .. } => {
                let parsed = fs::read(&event_log)
                    .map(|output| output_parser.parse_output(&output))
                    .unwrap_or_default();
                let input_tokens = parsed
                    .usage
                    .and_then(|usage| usage.input_tokens)
                    .and_then(|tokens| i64::try_from(tokens).ok());
                let output_tokens = parsed
                    .usage
                    .and_then(|usage| usage.output_tokens)
                    .and_then(|tokens| i64::try_from(tokens).ok());
                let status = if success { "succeeded" } else { "failed" };
                let mut reported_status = status.to_string();
                if let Ok(mut connection) = database.connect()
                    && let Ok(transaction) = connection.transaction()
                {
                    let _=transaction.execute("UPDATE agent_runs SET status=CASE WHEN status IN('cancelled','succeeded') THEN status ELSE ?1 END,reported_input_units=?2,reported_output_units=?3,ended_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?4",params![status,input_tokens,output_tokens,event_run_id]);
                    if !success && parsed.auth_required {
                        let _ = transaction.execute("UPDATE provider_accounts SET status='needs_reauth',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1", [&event_provider_id]);
                        let _ = timeline::append(
                            &transaction,
                            EventRefs {
                                project_id: &event_project_id,
                                task_id: Some(&event_task_id),
                                run_id: Some(&event_run_id),
                                provider_id: Some(&event_provider_id),
                            },
                            "provider.auth_required",
                            serde_json::json!({}),
                        );
                    }
                    reported_status = transaction
                        .query_row(
                            "SELECT status FROM agent_runs WHERE id=?1",
                            [&event_run_id],
                            |row| row.get(0),
                        )
                        .unwrap_or(reported_status);
                    if reported_status == "succeeded"
                        && let Ok(Some(exceeded)) =
                            exceeded_usage(&transaction, &event_task_id, Some(&event_run_id), false)
                    {
                        let _ = transaction.execute(
                            "UPDATE agent_runs SET status='failed',waiting_reason=?1 WHERE id=?2",
                            params![
                                format!("{} usage limit exceeded", exceeded.scope),
                                event_run_id
                            ],
                        );
                        reported_status = "failed".into();
                        let _ = timeline::append(
                            &transaction,
                            EventRefs {
                                project_id: &event_project_id,
                                task_id: Some(&event_task_id),
                                run_id: Some(&event_run_id),
                                provider_id: Some(&event_provider_id),
                            },
                            "budget.exceeded",
                            serde_json::json!({ "scope": exceeded.scope, "used": exceeded.used, "limit": exceeded.limit }),
                        );
                    }
                    let _ = timeline::append(
                        &transaction,
                        EventRefs {
                            project_id: &event_project_id,
                            task_id: Some(&event_task_id),
                            run_id: Some(&event_run_id),
                            provider_id: Some(&event_provider_id),
                        },
                        "run.status_changed",
                        serde_json::json!({ "to": reported_status.clone(), "reportedUsage": { "inputTokens": input_tokens, "outputTokens": output_tokens } }),
                    );
                    let _ = tasks::rollup_in_transaction(&transaction, &event_task_id);
                    let _ = transaction.commit();
                }
                ports.release(run.port);
                let _ = event_channel.send(RunStreamEvent::StatusChanged {
                    run_id: event_run_id.clone(),
                    status: reported_status,
                });
                if success && event_role == "planner" {
                    let plan_status = database.connect().ok().and_then(|connection| connection.query_row("SELECT status FROM task_plans WHERE planner_run_id=?1 ORDER BY attempt_number DESC LIMIT 1", [&event_run_id], |row| row.get::<_, String>(0)).optional().ok().flatten());
                    match plan_status.as_deref() {
                        Some("proposed") => {
                            let _ = dispatcher.mark_planning_attention(
                                &event_task_id,
                                &event_project_id,
                                &event_run_id,
                                "Plan ready for approval",
                            );
                        }
                        None => {
                            let _ = dispatcher.mark_planning_attention(
                                &event_task_id,
                                &event_project_id,
                                &event_run_id,
                                "Planner finished without submitting assignments",
                            );
                        }
                        _ => {}
                    }
                }
                let _ = dispatcher.dispatch_task(&event_task_id, event_channel.clone());
                let _ = dispatcher.dispatch_next(&event_project_id, event_channel.clone());
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
                redactions,
            },
            sink,
        ) {
            Ok(id) => id,
            Err(error) => {
                self.ports.release(run.port);
                let mut connection = self.database.connect()?;
                let transaction = connection.transaction()?;
                transaction.execute("UPDATE agent_runs SET status='failed',ended_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1", [&run_id])?;
                timeline::append(
                    &transaction,
                    EventRefs {
                        project_id: &project_id,
                        task_id: Some(&task_id),
                        run_id: Some(&run_id),
                        provider_id: Some(&provider_id),
                    },
                    "run.status_changed",
                    serde_json::json!({ "to": "failed", "error": error.message.clone() }),
                )?;
                tasks::rollup_in_transaction(&transaction, &task_id)?;
                transaction.commit()?;
                return Err(error);
            }
        };
        let mut connection = self.database.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute("UPDATE agent_runs SET status='running',process_identity=?1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2",params![process_id.map(|id|id.to_string()),run_id])?;
        timeline::append(
            &transaction,
            EventRefs {
                project_id: &project_id,
                task_id: Some(&task_id),
                run_id: Some(&run_id),
                provider_id: Some(&provider_id),
            },
            "run.status_changed",
            serde_json::json!({ "to": "running" }),
        )?;
        tasks::rollup_in_transaction(&transaction, &task_id)?;
        transaction.commit()?;
        let _ = channel.send(RunStreamEvent::Started { run_id });
        Ok(())
    }

    pub(crate) fn list(&self, task_id: &str) -> Result<Vec<Run>, CommandError> {
        let connection = self.database.connect()?;
        let mut statement=connection.prepare("SELECT r.id,r.task_id,r.provider_account_id,p.display_name,r.instruction,r.role,r.assignment_title,r.status,r.waiting_reason,w.path,r.raw_log_path,r.context_pack_path,w.environment_manifest_json,r.provider_session_id,r.resume_count,(json_array_length(g.resume_arguments_json)>0 AND (instr(g.resume_arguments_json,'{sessionId}')=0 OR r.provider_session_id IS NOT NULL)),r.updated_at,r.full_access,r.reported_input_units,r.reported_output_units,r.depends_on_run_ids_json,r.retry_of_run_id,r.unit_limit FROM agent_runs r JOIN provider_accounts p ON p.id=r.provider_account_id JOIN generic_provider_profiles g ON g.provider_account_id=r.provider_account_id LEFT JOIN worktrees w ON w.agent_run_id=r.id WHERE r.task_id=?1 ORDER BY r.created_at")?;
        statement
            .query_map([task_id], row_to_run)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
    pub(crate) fn list_project(&self, project_id: &str) -> Result<Vec<Run>, CommandError> {
        let connection = self.database.connect()?;
        let mut statement=connection.prepare("SELECT r.id,r.task_id,r.provider_account_id,p.display_name,r.instruction,r.role,r.assignment_title,r.status,r.waiting_reason,w.path,r.raw_log_path,r.context_pack_path,w.environment_manifest_json,r.provider_session_id,r.resume_count,(json_array_length(g.resume_arguments_json)>0 AND (instr(g.resume_arguments_json,'{sessionId}')=0 OR r.provider_session_id IS NOT NULL)),r.updated_at,r.full_access,r.reported_input_units,r.reported_output_units,r.depends_on_run_ids_json,r.retry_of_run_id,r.unit_limit FROM agent_runs r JOIN tasks t ON t.id=r.task_id JOIN provider_accounts p ON p.id=r.provider_account_id JOIN generic_provider_profiles g ON g.provider_account_id=r.provider_account_id LEFT JOIN worktrees w ON w.agent_run_id=r.id WHERE t.project_id=?1 ORDER BY r.created_at")?;
        statement
            .query_map([project_id], row_to_run)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
    pub(crate) fn usage(&self, run_id: &str) -> process::ProcessUsage {
        self.processes.usage(run_id)
    }
    fn get(&self, id: &str) -> Result<Option<Run>, CommandError> {
        let connection = self.database.connect()?;
        connection.query_row("SELECT r.id,r.task_id,r.provider_account_id,p.display_name,r.instruction,r.role,r.assignment_title,r.status,r.waiting_reason,w.path,r.raw_log_path,r.context_pack_path,w.environment_manifest_json,r.provider_session_id,r.resume_count,(json_array_length(g.resume_arguments_json)>0 AND (instr(g.resume_arguments_json,'{sessionId}')=0 OR r.provider_session_id IS NOT NULL)),r.updated_at,r.full_access,r.reported_input_units,r.reported_output_units,r.depends_on_run_ids_json,r.retry_of_run_id,r.unit_limit FROM agent_runs r JOIN provider_accounts p ON p.id=r.provider_account_id JOIN generic_provider_profiles g ON g.provider_account_id=r.provider_account_id LEFT JOIN worktrees w ON w.agent_run_id=r.id WHERE r.id=?1",[id],row_to_run).optional().map_err(Into::into)
    }
    fn stop(&self, id: &str) -> Result<(), CommandError> {
        let run = self
            .get(id)?
            .ok_or_else(|| CommandError::new("run_not_found", "Run was not found"))?;
        let task = tasks::get(&self.database, &run.task_id)?
            .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
        ensure_usage_available(&self.database, &task.id, Some(id))?;
        if !matches!(
            run.status.as_str(),
            "queued" | "preparing" | "running" | "waiting"
        ) {
            return Err(CommandError::new(
                "run_not_active",
                "Run is no longer active",
            ));
        }
        let mut connection = self.database.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute("UPDATE agent_runs SET status='cancelled',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1 AND status IN('queued','preparing','running','waiting')",[id])?;
        timeline::append(
            &transaction,
            EventRefs {
                project_id: &task.project_id,
                task_id: Some(&task.id),
                run_id: Some(id),
                provider_id: Some(&run.provider_id),
            },
            "run.status_changed",
            serde_json::json!({ "to": "cancelled", "source": "user" }),
        )?;
        tasks::rollup_in_transaction(&transaction, &task.id)?;
        transaction.commit()?;
        if matches!(run.status.as_str(), "preparing" | "running" | "waiting") {
            match self.processes.stop(id) {
                Err(error) if error.code == "run_not_active" => Ok(()),
                result => result,
            }
        } else {
            Ok(())
        }
    }
    fn mark_complete(&self, id: &str) -> Result<Run, CommandError> {
        let run = self
            .get(id)?
            .ok_or_else(|| CommandError::new("run_not_found", "Run was not found"))?;
        if run.role == "planner" {
            return Err(CommandError::new(
                "planner_not_completable",
                "Approve or reject the submitted plan instead",
            ));
        }
        if run.status == "succeeded" {
            return Ok(run);
        }
        if !matches!(
            run.status.as_str(),
            "running" | "waiting" | "failed" | "cancelled"
        ) {
            return Err(CommandError::new(
                "run_not_completable",
                "This Run cannot be marked ready for review",
            ));
        }
        let task = tasks::get(&self.database, &run.task_id)?
            .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
        let was_active = matches!(run.status.as_str(), "running" | "waiting");
        let mut connection = self.database.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute("UPDATE agent_runs SET status='succeeded',waiting_reason=NULL,ended_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1", [id])?;
        timeline::append(
            &transaction,
            EventRefs {
                project_id: &task.project_id,
                task_id: Some(&task.id),
                run_id: Some(id),
                provider_id: Some(&run.provider_id),
            },
            "run.status_changed",
            serde_json::json!({ "from": run.status, "to": "succeeded", "source": "user_review" }),
        )?;
        tasks::rollup_in_transaction(&transaction, &task.id)?;
        transaction.commit()?;
        if was_active {
            let _ = self.processes.stop(id);
        }
        self.get(id)?
            .ok_or_else(|| CommandError::new("run_not_found", "Run was not found after update"))
    }
    fn resume(&self, id: &str, channel: Channel<RunStreamEvent>) -> Result<Run, CommandError> {
        let run = self
            .get(id)?
            .ok_or_else(|| CommandError::new("run_not_found", "Run was not found"))?;
        if matches!(
            run.status.as_str(),
            "queued" | "preparing" | "running" | "waiting"
        ) {
            return Err(CommandError::new(
                "run_already_active",
                "Run is already active",
            ));
        }
        let process_identity = self.database.connect()?.query_row(
            "SELECT process_identity FROM agent_runs WHERE id=?1",
            [id],
            |row| row.get::<_, Option<String>>(0),
        )?;
        if process_identity
            .as_deref()
            .is_some_and(ProcessSupervisor::identity_is_active)
        {
            return Err(CommandError::new(
                "process_still_running",
                "The previous process is still running; wait for it to finish before resuming",
            ));
        }
        if !run.can_resume {
            return Err(CommandError::new(
                "resume_unsupported",
                "This session cannot be resumed exactly; start a new session instead",
            ));
        }
        let worktree = run
            .worktree_path
            .as_deref()
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
            .ok_or_else(|| {
                CommandError::new("worktree_not_found", "Run worktree is unavailable")
            })?;
        let log = run
            .raw_log_path
            .as_deref()
            .map(PathBuf::from)
            .ok_or_else(|| CommandError::new("output_unavailable", "Run output is unavailable"))?;
        let provider = providers::resolve(&self.database, &run.provider_id)?;
        if !provider.can_resume() {
            return Err(CommandError::new(
                "resume_unsupported",
                "This CLI profile does not support session resume",
            ));
        }
        let task = tasks::get(&self.database, &run.task_id)?
            .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
        if matches!(task.status.as_str(), "approved" | "merged" | "archived") {
            return Err(CommandError::new(
                "task_not_executable",
                "This Task has left the execution lifecycle",
            ));
        }
        let port = self.ports.acquire()?;
        let mut connection = self.database.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE worktrees SET environment_manifest_json=json_set(COALESCE(environment_manifest_json,'{}'),'$.port',?1) WHERE agent_run_id=?2",
            params![port, id],
        )?;
        transaction.execute(
            "UPDATE agent_runs SET status='preparing',process_identity=NULL,ended_at=NULL,resume_count=resume_count+1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
            [id],
        )?;
        timeline::append(
            &transaction,
            EventRefs {
                project_id: &task.project_id,
                task_id: Some(&task.id),
                run_id: Some(id),
                provider_id: Some(&run.provider_id),
            },
            "run.resumed",
            serde_json::json!({ "resumeCount": run.resume_count + 1 }),
        )?;
        tasks::rollup_in_transaction(&transaction, &task.id)?;
        transaction.commit()?;
        let config_root =
            provider.runtime_config_root(self.paths.data_dir.join("runs").join(id).join("config"));
        let prepared = Prepared {
            run_id: run.id.clone(),
            task_id: run.task_id.clone(),
            project_id: task.project_id,
            provider,
            worktree,
            log,
            config_root,
            port,
            prompt: String::new(),
            provider_session_id: run.provider_session_id,
            resume: true,
            role: run.role,
            full_access: run.full_access,
        };
        if let Err(error) = self.launch(prepared, channel) {
            self.ports.release(port);
            return Err(error);
        }
        self.get(id)?
            .ok_or_else(|| CommandError::new("run_not_found", "Run was not found"))
    }
    fn diff(&self, id: &str) -> Result<GitDiff, CommandError> {
        let connection = self.database.connect()?;
        let worktree = connection
            .query_row(
                "SELECT path,base_revision FROM worktrees WHERE agent_run_id=?1",
                [id],
                |row| {
                    Ok((
                        PathBuf::from(row.get::<_, String>(0)?),
                        row.get::<_, String>(1)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| CommandError::new("worktree_not_found", "Run worktree is not ready"))?;
        self.git.diff(&worktree.0, &worktree.1)
    }
}

struct ExceededUsage {
    scope: &'static str,
    used: i64,
    limit: i64,
}

fn exceeded_usage(
    connection: &rusqlite::Connection,
    task_id: &str,
    run_id: Option<&str>,
    at_limit: bool,
) -> Result<Option<ExceededUsage>, rusqlite::Error> {
    let reached = |used: i64, limit: i64| used > limit || (at_limit && used == limit);
    let (project_limit, task_limit, project_used, task_used): (
        Option<i64>,
        Option<i64>,
        i64,
        i64,
    ) = connection.query_row(
        "SELECT p.unit_limit,t.unit_limit,(SELECT COALESCE(SUM(COALESCE(r.reported_input_units,0)+COALESCE(r.reported_output_units,0)),0) FROM agent_runs r JOIN tasks used_task ON used_task.id=r.task_id WHERE used_task.project_id=t.project_id),(SELECT COALESCE(SUM(COALESCE(reported_input_units,0)+COALESCE(reported_output_units,0)),0) FROM agent_runs WHERE task_id=t.id) FROM tasks t JOIN projects p ON p.id=t.project_id WHERE t.id=?1",
        [task_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    if let Some(limit) = project_limit
        && reached(project_used, limit)
    {
        return Ok(Some(ExceededUsage {
            scope: "Project",
            used: project_used,
            limit,
        }));
    }
    if let Some(limit) = task_limit
        && reached(task_used, limit)
    {
        return Ok(Some(ExceededUsage {
            scope: "Task",
            used: task_used,
            limit,
        }));
    }
    if let Some(run_id) = run_id {
        let (limit, used): (Option<i64>, i64) = connection.query_row(
            "SELECT unit_limit,COALESCE(reported_input_units,0)+COALESCE(reported_output_units,0) FROM agent_runs WHERE id=?1",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if let Some(limit) = limit
            && reached(used, limit)
        {
            return Ok(Some(ExceededUsage {
                scope: "Agent",
                used,
                limit,
            }));
        }
    }
    Ok(None)
}

fn ensure_usage_available(
    database: &Database,
    task_id: &str,
    run_id: Option<&str>,
) -> Result<(), CommandError> {
    let connection = database.connect()?;
    let Some(exceeded) = exceeded_usage(&connection, task_id, run_id, true)? else {
        return Ok(());
    };
    Err(CommandError::new(
        "usage_limit_exceeded",
        format!(
            "{} usage is {} units with a {} unit limit. Raise the limit before starting more work.",
            exceeded.scope, exceeded.used, exceeded.limit
        ),
    ))
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<Run> {
    let manifest: Option<String> = row.get(12)?;
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
        role: row.get(5)?,
        title: row.get(6)?,
        status: row.get(7)?,
        waiting_reason: row.get(8)?,
        worktree_path: row.get(9)?,
        raw_log_path: row.get(10)?,
        context_pack_path: row.get(11)?,
        provider_session_id: row.get(13)?,
        resume_count: row.get(14)?,
        can_resume: row.get(15)?,
        port,
        updated_at: row.get(16)?,
        full_access: row.get(17)?,
        reported_input_tokens: row
            .get::<_, Option<i64>>(18)?
            .and_then(|tokens| u64::try_from(tokens).ok()),
        reported_output_tokens: row
            .get::<_, Option<i64>>(19)?
            .and_then(|tokens| u64::try_from(tokens).ok()),
        depends_on_run_ids: serde_json::from_str(&row.get::<_, String>(20)?).unwrap_or_default(),
        retry_of_run_id: row.get(21)?,
        unit_limit: row
            .get::<_, Option<i64>>(22)?
            .and_then(|units| u64::try_from(units).ok()),
    })
}
fn default_role() -> String {
    "executor".into()
}
fn tauri_error(error: tauri::Error) -> CommandError {
    CommandError::new("window_error", error.to_string())
}
fn base_environment(home: &Path, port: u16, inherit_user_home: bool) -> Vec<(String, String)> {
    let mut values = Vec::new();
    for key in [
        "PATH",
        "LANG",
        "LC_ALL",
        "TERM",
        "COLORTERM",
        "SYSTEMROOT",
        "WINDIR",
        "PATHEXT",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
    ] {
        if let Ok(value) = std::env::var(key) {
            values.push((key.into(), value));
        }
    }
    if !values.iter().any(|(key, _)| key == "TERM") {
        values.push(("TERM".into(), "xterm-256color".into()));
    }
    if inherit_user_home {
        // The user explicitly chose a detected CLI, so its existing login remains available.
        for key in [
            "HOME",
            "USER",
            "LOGNAME",
            "SHELL",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "APPDATA",
            "LOCALAPPDATA",
            "USERPROFILE",
        ] {
            if let Ok(value) = std::env::var(key) {
                values.push((key.into(), value));
            }
        }
        if !values.iter().any(|(key, _)| key == "HOME") {
            values.push(("HOME".into(), home.to_string_lossy().into()));
        }
    } else {
        values.extend([
            ("HOME".into(), home.to_string_lossy().into()),
            (
                "XDG_CONFIG_HOME".into(),
                home.join("xdg").to_string_lossy().into(),
            ),
        ]);
    }
    values.push(("SUBSHELL_PORT".into(), port.to_string()));
    values
}
fn io_error(error: std::io::Error) -> CommandError {
    CommandError::new("filesystem_error", error.to_string())
}

fn json_error(error: serde_json::Error) -> CommandError {
    CommandError::new("invalid_stored_data", error.to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::features::{
        agent_api::{PlanAssignment, SubmitPlanInput, submit_plan},
        context::PreviewInput,
        projects,
        providers::GenericProfile,
        tasks::CreateTask,
    };
    use std::{os::unix::fs::PermissionsExt, process::Command, thread, time::Duration};
    use tempfile::tempdir;

    #[test]
    fn startup_reconciles_a_lost_process_without_discarding_resume_state() {
        let root = tempdir().unwrap();
        let paths = RuntimePaths {
            data_dir: root.path().join("data"),
        };
        let database = Database::initialize(&paths.data_dir.join("db.sqlite3")).unwrap();
        let executable = root.path().join("resume-agent");
        fs::write(
            &executable,
            "#!/bin/sh\nprintf 'after restart\\n'\nsleep 0.2\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let profile = GenericProfile {
            id: "claude-account".into(),
            display_name: "Claude fixture".into(),
            provider_type: "claude".into(),
            status: "active".into(),
            executable_path: executable.to_string_lossy().into(),
            arguments: vec![
                "--session-id".into(),
                "{sessionId}".into(),
                "{prompt}".into(),
            ],
            resume_arguments: vec!["--resume".into(), "{sessionId}".into()],
            prompt_mode: "argument".into(),
            config_root_env_var: Some("CLAUDE_CONFIG_DIR".into()),
            config_source_path: None,
            inherit_user_home: false,
        };
        providers::save(&profile, &database, &paths).unwrap();
        let worktree = root.path().join("worktree");
        let live_worktree = root.path().join("live-worktree");
        let log = root.path().join("output.log");
        fs::create_dir_all(&worktree).unwrap();
        fs::create_dir_all(&live_worktree).unwrap();
        fs::write(&log, "before restart\n").unwrap();
        let connection = database.connect().unwrap();
        connection.execute("INSERT INTO projects(id,name,path,created_at,updated_at) VALUES('project','Project','/tmp/project','now','now')", []).unwrap();
        connection.execute("INSERT INTO tasks(id,project_id,title,status,base_branch,base_revision,created_at,updated_at) VALUES('task','project','Task','working','main','abc','now','now')", []).unwrap();
        connection.execute("INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,status,raw_log_path,process_identity,provider_session_id,created_at,updated_at) VALUES('run','task','claude-account','work','running',?1,'4242','session-1','now','now')", [log.to_string_lossy()]).unwrap();
        connection.execute("INSERT INTO worktrees(id,agent_run_id,path,base_branch,base_revision,state,created_at) VALUES('worktree','run',?1,'main','abc','active','now')", [worktree.to_string_lossy()]).unwrap();
        connection.execute("INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,status,raw_log_path,process_identity,provider_session_id,created_at,updated_at) VALUES('live-run','task','claude-account','work','running',?1,?2,'session-2','now','now')", params![root.path().join("live.log").to_string_lossy(), std::process::id().to_string()]).unwrap();
        connection.execute("INSERT INTO worktrees(id,agent_run_id,path,base_branch,base_revision,state,created_at) VALUES('live-worktree','live-run',?1,'main','abc','active','now')", [live_worktree.to_string_lossy()]).unwrap();
        drop(connection);

        let service = RunService::new(
            database.clone(),
            paths,
            GitService::default(),
            ContextDrafts::default(),
            ProcessSupervisor::default(),
            PortLeases::default(),
            Arc::new(crate::platform::keychain::MemorySecretStore::default()),
        );
        service.recover_and_dispatch().unwrap();

        let run = service.get("run").unwrap().unwrap();
        assert!(matches!(run.status.as_str(), "running" | "succeeded"));
        assert!(run.can_resume);
        assert_eq!(run.resume_count, 1);
        assert_eq!(run.worktree_path.as_deref(), worktree.to_str());
        assert_eq!(run.raw_log_path.as_deref(), log.to_str());
        for _ in 0..50 {
            if fs::read_to_string(&log).unwrap().contains("after restart") {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let output = fs::read_to_string(&log).unwrap();
        assert!(output.starts_with("before restart\n"));
        assert_eq!(output.matches("after restart").count(), 1);
        assert_eq!(database.connect().unwrap().query_row::<u32, _, _>("SELECT COUNT(*) FROM timeline_events WHERE agent_run_id='run' AND event_type='run.resumed'", [], |row| row.get(0)).unwrap(), 1);
        let live = service.get("live-run").unwrap().unwrap();
        assert_eq!(live.status, "failed");
        assert_eq!(live.resume_count, 0);
        assert!(live.waiting_reason.unwrap().contains("still running"));
        assert_eq!(
            service
                .resume("live-run", Channel::new(|_| Ok(())))
                .unwrap_err()
                .code,
            "process_still_running"
        );
    }

    #[test]
    fn interactive_runs_always_receive_a_terminal_type() {
        assert!(
            base_environment(Path::new("/tmp/config"), 4100, false)
                .iter()
                .any(|(key, value)| key == "TERM" && !value.is_empty())
        );
    }

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
            "#!/bin/sh\ncase \"$1:$ANTHROPIC_API_KEY\" in first:alpha-secret|resume:alpha-secret|second:beta-secret) ;; *) printf 'cross-account credential\\n'; exit 2;; esac\nprintf 'argc:%s mode:%s session:%s config:%s secret:%s\\n' \"$#\" \"$1\" \"$2\" \"$CLAUDE_CONFIG_DIR\" \"$ANTHROPIC_API_KEY\"\ncase \"$1:$ANTHROPIC_API_KEY:$3\" in first:alpha-secret:*'Parallel fixture'*) sleep 5;; *) sleep 0.2;; esac\nprintf '{\"usage\":{\"input_tokens\":12,\"output_tokens\":4}}\\nfinished\\n'\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let profile = GenericProfile {
            id: "stand-in".into(),
            display_name: "Stand-in".into(),
            provider_type: "claude".into(),
            status: "active".into(),
            executable_path: executable.to_string_lossy().into(),
            arguments: vec!["first".into(), "{sessionId}".into(), "{prompt}".into()],
            resume_arguments: vec!["resume".into(), "{sessionId}".into()],
            prompt_mode: "argument".into(),
            config_root_env_var: Some("CLAUDE_CONFIG_DIR".into()),
            config_source_path: None,
            inherit_user_home: false,
        };
        providers::save(&profile, &database, &paths).unwrap();
        let second_profile = GenericProfile {
            id: "stand-in-two".into(),
            display_name: "Stand-in two".into(),
            arguments: vec!["second".into(), "{sessionId}".into(), "{prompt}".into()],
            ..profile.clone()
        };
        providers::save(&second_profile, &database, &paths).unwrap();
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
        let secrets = Arc::new(crate::platform::keychain::MemorySecretStore::default());
        secrets.set(&profile.id, b"alpha-secret").unwrap();
        secrets.set(&second_profile.id, b"beta-secret").unwrap();
        let service = RunService::new(
            database.clone(),
            paths,
            git,
            drafts.clone(),
            ProcessSupervisor::default(),
            PortLeases::default(),
            secrets,
        );
        let assignment = |preview: context::ContextPreview, provider_id: &str| Assignment {
            provider_id: provider_id.into(),
            instruction: "Work safely".into(),
            role: "executor".into(),
            title: None,
            context_token: preview.token,
            approved_context: preview.content,
            environment_files: vec![],
            full_access: false,
            unit_limit: None,
        };
        let runs = service
            .start(
                StartInput {
                    task_id: task.id.clone(),
                    assignments: vec![
                        assignment(first, &profile.id),
                        assignment(second, &second_profile.id),
                    ],
                },
                Channel::new(|_| Ok(())),
            )
            .unwrap();
        assert_eq!(runs.len(), 2);
        assert_ne!(runs[0].worktree_path, runs[1].worktree_path);
        assert_ne!(runs[0].port, runs[1].port);
        let queued_task = tasks::create(
            CreateTask {
                project_id: task.project_id.clone(),
                title: "Queued fixture".into(),
                description: "Runs after the active task".into(),
                acceptance_criteria: vec![],
                allowed_paths: vec![],
                validation_commands: vec![],
                decisions: vec![],
                confirm_dirty_base: true,
            },
            &database,
            &service.git,
        )
        .unwrap();
        let queued_preview = context::build(
            PreviewInput {
                task_id: queued_task.id.clone(),
                instruction: "Wait your turn".into(),
                selected_files: vec![],
                pattern: None,
            },
            &database,
            &service.git,
            &drafts,
        )
        .unwrap();
        let queued = service
            .enqueue(StartInput {
                task_id: queued_task.id.clone(),
                assignments: vec![assignment(queued_preview, &profile.id)],
            })
            .unwrap();
        assert_eq!(queued[0].status, "queued");
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
        assert_eq!(states[1].reported_input_tokens, Some(12));
        assert_eq!(states[1].reported_output_tokens, Some(4));
        for state in &states {
            let output = fs::read_to_string(state.raw_log_path.as_ref().unwrap()).unwrap();
            assert!(
                output.contains(
                    &root
                        .path()
                        .join("data/runs")
                        .join(&state.id)
                        .join("config")
                        .to_string_lossy()
                        .into_owned()
                )
            );
            assert!(!output.contains("alpha-secret"));
            assert!(!output.contains("beta-secret"));
            assert!(!output.contains("cross-account credential"));
        }
        assert_eq!(
            service.mark_complete(&states[0].id).unwrap().status,
            "succeeded"
        );
        assert_eq!(
            tasks::get(&database, &task.id).unwrap().unwrap().status,
            "review"
        );
        for _ in 0..50 {
            if service.list(&queued_task.id).unwrap()[0].status == "succeeded" {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            service.list(&queued_task.id).unwrap()[0].status,
            "succeeded"
        );
        let session_id = states[0].provider_session_id.clone().unwrap();
        let resumed = service
            .resume(&states[0].id, Channel::new(|_| Ok(())))
            .unwrap();
        assert_eq!(resumed.status, "running");
        assert_eq!(
            resumed.provider_session_id.as_deref(),
            Some(session_id.as_str())
        );
        assert_eq!(resumed.resume_count, 1);
        for _ in 0..50 {
            if service.get(&resumed.id).unwrap().unwrap().status == "succeeded" {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let log = fs::read_to_string(resumed.raw_log_path.as_ref().unwrap()).unwrap();
        assert!(log.contains(&format!("argc:3 mode:first session:{session_id}")));
        assert!(log.contains(&format!("argc:2 mode:resume session:{session_id}")));
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
        let retry_source = service.list(&queued_task.id).unwrap().remove(0);
        database.connect().unwrap().execute(
            "INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,status,depends_on_run_ids_json,created_at,updated_at) VALUES('retry-dependent',?1,?2,'Wait for retry','queued',?3,'now','now')",
            params![queued_task.id, profile.id, serde_json::json!([retry_source.id]).to_string()],
        ).unwrap();
        fs::write(
            Path::new(retry_source.worktree_path.as_ref().unwrap()).join("retry.txt"),
            "preserved state",
        )
        .unwrap();
        let retried = service
            .retry(&retry_source.id, Channel::new(|_| Ok(())))
            .unwrap();
        assert_eq!(
            retried.retry_of_run_id.as_deref(),
            Some(retry_source.id.as_str())
        );
        let dependency: String = database
            .connect()
            .unwrap()
            .query_row(
                "SELECT depends_on_run_ids_json FROM agent_runs WHERE id='retry-dependent'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&dependency).unwrap(),
            std::slice::from_ref(&retried.id)
        );
        database
            .connect()
            .unwrap()
            .execute("DELETE FROM agent_runs WHERE id='retry-dependent'", [])
            .unwrap();
        assert_ne!(retried.worktree_path, retry_source.worktree_path);
        assert_eq!(
            fs::read_to_string(
                Path::new(retried.worktree_path.as_ref().unwrap()).join("retry.txt")
            )
            .unwrap(),
            "preserved state"
        );

        let automatic_task = tasks::create(
            CreateTask {
                project_id: task.project_id.clone(),
                title: "Automatic plan fixture".into(),
                description: "Split this into independent work".into(),
                acceptance_criteria: vec![],
                allowed_paths: vec![],
                validation_commands: vec![],
                decisions: vec![],
                confirm_dirty_base: true,
            },
            &database,
            &service.git,
        )
        .unwrap();
        database.connect().unwrap().execute(
            "INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,role,status,created_at,updated_at) VALUES('planner',?1,?2,'Plan','planner','running','now','now')",
            params![automatic_task.id, profile.id],
        ).unwrap();
        let plan_id = submit_plan(
            &database,
            SubmitPlanInput {
                run_id: "planner".into(),
                task_title: "Inspect implementation coverage".into(),
                summary: "Two independent assignments".into(),
                assignments: vec![
                    PlanAssignment {
                        title: "Frontend".into(),
                        instruction: "Inspect the frontend".into(),
                        role: "executor".into(),
                        allowed_paths: vec!["README.md".into()],
                        depends_on: vec![],
                    },
                    PlanAssignment {
                        title: "Tests".into(),
                        instruction: "Inspect test coverage".into(),
                        role: "test".into(),
                        allowed_paths: vec!["README.md".into()],
                        depends_on: vec!["Frontend".into()],
                    },
                ],
            },
        )
        .unwrap();
        let approved = service
            .approve_plan(&plan_id, false, Channel::new(|_| Ok(())))
            .unwrap();
        assert_eq!(approved.status, "launched");
        let automatic_runs = service.list(&automatic_task.id).unwrap();
        assert_eq!(automatic_runs.len(), 4);
        assert!(automatic_runs.iter().all(|run| !run.full_access));
        assert_eq!(
            automatic_runs
                .iter()
                .filter(|run| run.role != "planner")
                .count(),
            3
        );
        let worktrees = automatic_runs
            .iter()
            .filter(|run| run.role != "planner")
            .filter_map(|run| run.worktree_path.as_deref())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(worktrees.len(), 3);
        let frontend = automatic_runs
            .iter()
            .find(|run| run.title.as_deref() == Some("Frontend"))
            .unwrap();
        let tests = automatic_runs
            .iter()
            .find(|run| run.title.as_deref() == Some("Tests"))
            .unwrap();
        assert_eq!(tests.status, "queued");
        assert_eq!(tests.depends_on_run_ids, std::slice::from_ref(&frontend.id));
        assert_eq!(
            tests.waiting_reason.as_deref(),
            Some("Waiting for prerequisite agent")
        );
        let reviewer = automatic_runs
            .iter()
            .find(|run| run.role == "reviewer")
            .unwrap();
        assert_eq!(reviewer.status, "queued");
        assert_eq!(
            reviewer
                .depends_on_run_ids
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<_>>(),
            [frontend.id.clone(), tests.id.clone()]
                .into_iter()
                .collect()
        );
        for _ in 0..50 {
            if service
                .list(&automatic_task.id)
                .unwrap()
                .iter()
                .all(|run| run.status == "succeeded")
            {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            service
                .list(&automatic_task.id)
                .unwrap()
                .iter()
                .all(|run| run.status == "succeeded")
        );
    }
}
