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
pub fn runs_diff(input: RunId, service: State<RunService>) -> Result<GitDiff, CommandError> {
    service.diff(&input.run_id)
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
                "planner" | "executor" | "research" | "test" | "reviewer"
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
            let config_root = run_dir.join("config");
            let log = run_dir.join("output.log");
            let context_path = run_dir.join("context.md");
            fs::create_dir_all(&run_dir).map_err(io_error)?;
            let mut connection = self.database.connect()?;
            let transaction = connection.transaction()?;
            transaction.execute("INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,role,assignment_title,status,merge_order,raw_log_path,context_pack_path,context_manifest_json,context_sha256,provider_session_id,full_access,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,0,?8,?9,?10,?11,?12,?13,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'))",params![run_id,task.id,provider.id,assignment.instruction,assignment.role,assignment.title,initial_status,log.to_string_lossy(),context_path.to_string_lossy(),serde_json::to_string(&manifest).unwrap(),context_sha256,provider_session_id,assignment.full_access])?;
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
        let mut statement = connection.prepare(
            "SELECT id FROM agent_runs WHERE task_id=?1 AND status='queued' ORDER BY created_at",
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
            prepared.push(Prepared {
                run_id: run.id,
                task_id: run.task_id,
                project_id: task.project_id.clone(),
                provider,
                worktree,
                log,
                config_root: self.paths.data_dir.join("runs").join(&id).join("config"),
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
                "SELECT t.id FROM tasks t WHERE t.project_id=?1 AND EXISTS(SELECT 1 FROM agent_runs r WHERE r.task_id=t.id AND r.status='queued') ORDER BY t.queue_position,t.created_at LIMIT 1",
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
            let mut statement = connection.prepare("SELECT id,title,instruction,role,allowed_paths_json,position FROM task_plan_assignments WHERE plan_id=?1 ORDER BY position")?;
            statement
                .query_map([&id], |row| {
                    Ok(TaskPlanAssignment {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        instruction: row.get(2)?,
                        role: row.get(3)?,
                        allowed_paths: serde_json::from_str(&row.get::<_, String>(4)?)
                            .unwrap_or_default(),
                        position: row.get(5)?,
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
                "SELECT title,instruction,role,allowed_paths_json FROM task_plan_assignments WHERE plan_id=?1 ORDER BY position",
            )?;
            statement
                .query_map([&plan_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        serde_json::from_str::<Vec<String>>(&row.get::<_, String>(3)?)
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
        let mut prepared = Vec::new();
        for (position, (title, instruction, role, allowed_paths)) in
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
            });
        }
        self.start(
            StartInput {
                task_id: task_id.clone(),
                assignments: prepared,
            },
            channel,
        )?;
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
                "SELECT r.id,r.task_id,t.project_id,r.provider_account_id FROM agent_runs r JOIN tasks t ON t.id=r.task_id WHERE r.status IN('preparing','running','waiting')",
            )?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let transaction = connection.transaction()?;
        for (run_id, task_id, project_id, provider_id) in active {
            transaction.execute(
                "UPDATE agent_runs SET status='failed',ended_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
                [&run_id],
            )?;
            timeline::append(
                &transaction,
                EventRefs {
                    project_id: &project_id,
                    task_id: Some(&task_id),
                    run_id: Some(&run_id),
                    provider_id: Some(&provider_id),
                },
                "run.status_changed",
                serde_json::json!({ "to": "failed", "reason": "application_restarted" }),
            )?;
            tasks::rollup_in_transaction(&transaction, &task_id)?;
        }
        transaction.commit()?;

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

    fn list(&self, task_id: &str) -> Result<Vec<Run>, CommandError> {
        let connection = self.database.connect()?;
        let mut statement=connection.prepare("SELECT r.id,r.task_id,r.provider_account_id,p.display_name,r.instruction,r.role,r.assignment_title,r.status,r.waiting_reason,w.path,r.raw_log_path,r.context_pack_path,w.environment_manifest_json,r.provider_session_id,r.resume_count,(json_array_length(g.resume_arguments_json)>0 AND (instr(g.resume_arguments_json,'{sessionId}')=0 OR r.provider_session_id IS NOT NULL)),r.updated_at,r.full_access,r.reported_input_units,r.reported_output_units FROM agent_runs r JOIN provider_accounts p ON p.id=r.provider_account_id JOIN generic_provider_profiles g ON g.provider_account_id=r.provider_account_id LEFT JOIN worktrees w ON w.agent_run_id=r.id WHERE r.task_id=?1 ORDER BY r.created_at")?;
        statement
            .query_map([task_id], row_to_run)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
    fn get(&self, id: &str) -> Result<Option<Run>, CommandError> {
        let connection = self.database.connect()?;
        connection.query_row("SELECT r.id,r.task_id,r.provider_account_id,p.display_name,r.instruction,r.role,r.assignment_title,r.status,r.waiting_reason,w.path,r.raw_log_path,r.context_pack_path,w.environment_manifest_json,r.provider_session_id,r.resume_count,(json_array_length(g.resume_arguments_json)>0 AND (instr(g.resume_arguments_json,'{sessionId}')=0 OR r.provider_session_id IS NOT NULL)),r.updated_at,r.full_access,r.reported_input_units,r.reported_output_units FROM agent_runs r JOIN provider_accounts p ON p.id=r.provider_account_id JOIN generic_provider_profiles g ON g.provider_account_id=r.provider_account_id LEFT JOIN worktrees w ON w.agent_run_id=r.id WHERE r.id=?1",[id],row_to_run).optional().map_err(Into::into)
    }
    fn stop(&self, id: &str) -> Result<(), CommandError> {
        let run = self
            .get(id)?
            .ok_or_else(|| CommandError::new("run_not_found", "Run was not found"))?;
        let task = tasks::get(&self.database, &run.task_id)?
            .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
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
            self.processes.stop(id)
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
        let prepared = Prepared {
            run_id: run.id.clone(),
            task_id: run.task_id.clone(),
            project_id: task.project_id,
            provider,
            worktree,
            log,
            config_root: self.paths.data_dir.join("runs").join(id).join("config"),
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
    })
}
fn default_role() -> String {
    "executor".into()
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
            "XDG_RUNTIME_DIR",
            "DBUS_SESSION_BUS_ADDRESS",
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
            "#!/bin/sh\nprintf 'argc:%s mode:%s session:%s\\n' \"$#\" \"$1\" \"$2\"\nsleep 0.4\nprintf '{\"usage\":{\"input_tokens\":12,\"output_tokens\":4}}\\nfinished\\n'\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let profile = GenericProfile {
            id: "stand-in".into(),
            display_name: "Stand-in".into(),
            provider_type: "claude".into(),
            status: "active".into(),
            executable_path: executable.to_string_lossy().into(),
            arguments: vec!["start".into(), "{sessionId}".into(), "{prompt}".into()],
            resume_arguments: vec!["resume".into(), "{sessionId}".into()],
            prompt_mode: "argument".into(),
            config_root_env_var: Some("STANDIN_HOME".into()),
            config_source_path: None,
            inherit_user_home: false,
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
            drafts.clone(),
            ProcessSupervisor::default(),
            PortLeases::default(),
            Arc::new(crate::platform::keychain::MemorySecretStore::default()),
        );
        let assignment = |preview: context::ContextPreview| Assignment {
            provider_id: profile.id.clone(),
            instruction: "Work safely".into(),
            role: "executor".into(),
            title: None,
            context_token: preview.token,
            approved_context: preview.content,
            environment_files: vec![],
            full_access: false,
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
                assignments: vec![assignment(queued_preview)],
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
        assert!(log.contains(&format!("argc:3 mode:start session:{session_id}")));
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
                    },
                    PlanAssignment {
                        title: "Tests".into(),
                        instruction: "Inspect test coverage".into(),
                        role: "test".into(),
                        allowed_paths: vec!["README.md".into()],
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
        assert_eq!(automatic_runs.len(), 3);
        assert!(automatic_runs.iter().all(|run| !run.full_access));
        assert_eq!(
            automatic_runs
                .iter()
                .filter(|run| run.role != "planner")
                .count(),
            2
        );
        assert_ne!(
            automatic_runs[1].worktree_path,
            automatic_runs[2].worktree_path
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
