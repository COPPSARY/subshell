use std::io::{self, BufRead, Write};
use std::path::{Component, Path};
use std::{thread, time::Duration};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;
use uuid::Uuid;

use crate::{
    contracts::{CommandError, Page},
    features::{
        attention::{self, AttentionItem},
        tasks::{self, Task},
        timeline::{self, EventRefs},
    },
    platform::database::Database,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorInput {
    pub run_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestInput {
    pub run_id: String,
    pub action: String,
    pub arguments: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityInput {
    pub run_id: String,
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitPlanInput {
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub task_title: String,
    #[serde(default)]
    pub summary: String,
    pub assignments: Vec<PlanAssignment>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanAssignment {
    pub title: String,
    pub instruction: String,
    #[serde(default = "executor_role")]
    pub role: String,
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionInput {
    pub request_id: String,
    pub decision: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInput {
    pub project_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRun {
    pub id: String,
    pub provider_name: String,
    pub role: String,
    pub title: Option<String>,
    pub instruction: String,
    pub status: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub task: Task,
    pub runs: Vec<WorkspaceRun>,
    pub attention: Vec<AttentionItem>,
    pub summary: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub run_id: Option<String>,
    pub action: String,
    pub arguments: Value,
    pub status: String,
    pub requested_by: String,
    pub created_at: String,
    pub decided_at: Option<String>,
    pub execution_status: String,
    pub execution_result: Option<Value>,
    pub execution_error_code: Option<String>,
    pub execution_error_message: Option<String>,
    pub executed_at: Option<String>,
}

#[tauri::command]
pub fn workspace_snapshot(
    input: ActorInput,
    database: State<Database>,
) -> Result<WorkspaceSnapshot, CommandError> {
    snapshot(&database, &input.run_id)
}

#[tauri::command]
pub fn workspace_request_action(
    input: RequestInput,
    database: State<Database>,
) -> Result<ApprovalRequest, CommandError> {
    request(&database, input)
}

#[tauri::command]
pub fn workspace_report_activity(
    input: ActivityInput,
    database: State<Database>,
) -> Result<(), CommandError> {
    report_activity(&database, input)
}

#[tauri::command]
pub fn workspace_submit_plan(
    input: SubmitPlanInput,
    database: State<Database>,
) -> Result<String, CommandError> {
    submit_plan(&database, input)
}

#[tauri::command]
pub fn workspace_list_approvals(
    input: ProjectInput,
    database: State<Database>,
) -> Result<Page<ApprovalRequest>, CommandError> {
    Ok(Page::first(list_approvals(&database, &input.project_id)?))
}

pub fn snapshot(database: &Database, run_id: &str) -> Result<WorkspaceSnapshot, CommandError> {
    let (task_id, project_id): (String, String) = database.connect()?.query_row(
        "SELECT r.task_id,t.project_id FROM agent_runs r JOIN tasks t ON t.id=r.task_id WHERE r.id=?1", [run_id],
        |row| Ok((row.get(0)?,row.get(1)?)),
    ).optional()?.ok_or_else(|| CommandError::new("run_not_found", "Calling Run was not found"))?;
    let task = tasks::get(database, &task_id)?
        .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
    let connection = database.connect()?;
    let runs = {
        let mut statement = connection.prepare("SELECT r.id,p.display_name,r.role,r.assignment_title,r.instruction,r.status,r.updated_at FROM agent_runs r JOIN provider_accounts p ON p.id=r.provider_account_id WHERE r.task_id=?1 ORDER BY r.created_at")?;
        statement
            .query_map([&task_id], |row| {
                Ok(WorkspaceRun {
                    id: row.get(0)?,
                    provider_name: row.get(1)?,
                    role: row.get(2)?,
                    title: row.get(3)?,
                    instruction: row.get(4)?,
                    status: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let summary = runs
        .iter()
        .map(|run| format!("{}: {}", run.provider_name, run.status))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(WorkspaceSnapshot {
        task,
        runs,
        attention: attention::list(database, &project_id)?,
        summary,
    })
}

pub fn request(database: &Database, input: RequestInput) -> Result<ApprovalRequest, CommandError> {
    const ACTIONS: [&str; 6] = [
        "start_run",
        "share_context",
        "create_branch",
        "clean_resources",
        "approve_task",
        "merge_task",
    ];
    if !ACTIONS.contains(&input.action.as_str()) {
        return Err(CommandError::new(
            "unauthorized_action",
            "WorkspaceControl does not expose that action",
        ));
    }
    let arguments = serde_json::to_string(&input.arguments)
        .map_err(|error| CommandError::new("invalid_arguments", error.to_string()))?;
    if arguments.len() > 16 * 1024 {
        return Err(CommandError::new(
            "arguments_too_large",
            "Action arguments exceed 16 KiB",
        ));
    }
    let connection = database.connect()?;
    let (task_id, project_id, run_status): (String, String, String) = connection.query_row(
        "SELECT r.task_id,t.project_id,r.status FROM agent_runs r JOIN tasks t ON t.id=r.task_id WHERE r.id=?1", [&input.run_id],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
    ).optional()?.ok_or_else(|| CommandError::new("run_not_found", "Calling Run was not found"))?;
    if !matches!(run_status.as_str(), "running" | "waiting") {
        return Err(CommandError::new(
            "run_not_active",
            "Only a live Run can request an action",
        ));
    }
    let id = Uuid::new_v4().to_string();
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    transaction.execute("UPDATE agent_runs SET status='waiting',waiting_reason=?1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2", params![format!("Approval requested: {}", input.action),input.run_id])?;
    if run_status != "waiting" {
        timeline::append(
            &transaction,
            EventRefs {
                project_id: &project_id,
                task_id: Some(&task_id),
                run_id: Some(&input.run_id),
                provider_id: None,
            },
            "run.status_changed",
            serde_json::json!({"from":run_status,"to":"waiting","reason":"approval_requested"}),
        )?;
    }
    tasks::rollup_in_transaction(&transaction, &task_id)?;
    let fingerprint: String = transaction.query_row("SELECT r.status||':'||r.updated_at||':'||t.status||':'||t.updated_at FROM agent_runs r JOIN tasks t ON t.id=r.task_id WHERE r.id=?1", [&input.run_id], |row| row.get(0))?;
    transaction.execute("INSERT INTO approval_requests(id,project_id,task_id,agent_run_id,action,arguments_json,state_fingerprint,status,requested_by,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'pending','agent',strftime('%Y-%m-%dT%H:%M:%fZ','now'))", params![id,project_id,task_id,input.run_id,input.action,arguments,fingerprint])?;
    timeline::append(
        &transaction,
        EventRefs {
            project_id: &project_id,
            task_id: Some(&task_id),
            run_id: Some(&input.run_id),
            provider_id: None,
        },
        "approval.requested",
        serde_json::json!({"requestId":id,"action":input.action}),
    )?;
    transaction.commit()?;
    get_approval(database, &id)
}

pub fn report_activity(database: &Database, input: ActivityInput) -> Result<(), CommandError> {
    if !matches!(
        input.kind.as_str(),
        "progress" | "validation" | "changed_path"
    ) {
        return Err(CommandError::new(
            "invalid_activity_kind",
            "Activity kind is not supported",
        ));
    }
    let detail = input.detail.trim();
    if detail.is_empty() || detail.len() > 2048 {
        return Err(CommandError::new(
            "invalid_activity",
            "Activity detail must be between 1 and 2048 bytes",
        ));
    }
    let mut connection = database.connect()?;
    let (task_id, project_id): (String, String) = connection.query_row(
        "SELECT r.task_id,t.project_id FROM agent_runs r JOIN tasks t ON t.id=r.task_id WHERE r.id=?1",
        [&input.run_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?.ok_or_else(|| CommandError::new("run_not_found", "Calling Run was not found"))?;
    let transaction = connection.transaction()?;
    timeline::append(
        &transaction,
        EventRefs {
            project_id: &project_id,
            task_id: Some(&task_id),
            run_id: Some(&input.run_id),
            provider_id: None,
        },
        &format!("agent.reported_{}", input.kind),
        serde_json::json!({"detail":detail,"authority":"agent_authored"}),
    )?;
    transaction.commit()?;
    Ok(())
}

pub fn submit_plan(database: &Database, input: SubmitPlanInput) -> Result<String, CommandError> {
    let task_title = input.task_title.trim();
    if task_title.chars().count() > 72 || task_title.contains(['\n', '\r']) {
        return Err(CommandError::new(
            "invalid_task_title",
            "Task title must be a single line of at most 72 characters",
        ));
    }
    if input.assignments.is_empty() || input.assignments.len() > 8 {
        return Err(CommandError::new(
            "invalid_plan",
            "A plan needs between 1 and 8 independent assignments",
        ));
    }
    let mut titles = std::collections::HashSet::new();
    for assignment in &input.assignments {
        let title = assignment.title.trim();
        let instruction = assignment.instruction.trim();
        if title.is_empty()
            || title.len() > 120
            || instruction.is_empty()
            || instruction.len() > 8_000
        {
            return Err(CommandError::new(
                "invalid_plan_assignment",
                "Every assignment needs a short title and bounded instruction",
            ));
        }
        if !titles.insert(title.to_lowercase()) {
            return Err(CommandError::new(
                "duplicate_plan_assignment",
                "Assignment titles must be unique",
            ));
        }
        if !matches!(
            assignment.role.as_str(),
            "executor" | "implementer" | "research" | "test" | "tester" | "reviewer" | "debugger"
        ) {
            return Err(CommandError::new(
                "invalid_run_role",
                "Planner assignments cannot create another planner",
            ));
        }
        if assignment.allowed_paths.len() > 32
            || assignment.allowed_paths.iter().any(|path| {
                Path::new(path).is_absolute()
                    || Path::new(path).components().any(|part| {
                        matches!(
                            part,
                            Component::ParentDir | Component::RootDir | Component::Prefix(_)
                        )
                    })
            })
        {
            return Err(CommandError::new(
                "invalid_allowed_path",
                "Allowed paths must stay inside the Project",
            ));
        }
    }
    validate_dependencies(&input.assignments)?;
    let mut connection = database.connect()?;
    let (task_id, project_id, role, status): (String, String, String, String) = connection
        .query_row(
            "SELECT r.task_id,t.project_id,r.role,r.status FROM agent_runs r JOIN tasks t ON t.id=r.task_id WHERE r.id=?1",
            [&input.run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?
        .ok_or_else(|| CommandError::new("run_not_found", "Calling Run was not found"))?;
    if role != "planner" || !matches!(status.as_str(), "running" | "waiting") {
        return Err(CommandError::new(
            "plan_not_authorized",
            "Only the live Task planner can submit a plan",
        ));
    }
    if connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM task_plans WHERE planner_run_id=?1)",
        [&input.run_id],
        |row| row.get::<_, bool>(0),
    )? {
        return Err(CommandError::new(
            "plan_already_submitted",
            "This planner already submitted its plan",
        ));
    }
    let attempt: u32 = connection.query_row(
        "SELECT COALESCE(MAX(attempt_number),0)+1 FROM task_plans WHERE task_id=?1",
        [&task_id],
        |row| row.get(0),
    )?;
    let plan_id = Uuid::new_v4().to_string();
    let transaction = connection.transaction()?;
    if !task_title.is_empty() {
        transaction.execute(
            "UPDATE tasks SET title=?1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2",
            params![task_title, task_id],
        )?;
    }
    transaction.execute(
        "INSERT INTO task_plans(id,task_id,planner_run_id,attempt_number,summary,created_at) VALUES(?1,?2,?3,?4,?5,strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        params![plan_id, task_id, input.run_id, attempt, input.summary.trim()],
    )?;
    for (position, assignment) in input.assignments.iter().enumerate() {
        transaction.execute(
            "INSERT INTO task_plan_assignments(id,plan_id,title,instruction,role,allowed_paths_json,depends_on_json,position) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![Uuid::new_v4().to_string(), plan_id, assignment.title.trim(), assignment.instruction.trim(), assignment.role, serde_json::to_string(&assignment.allowed_paths).unwrap(), serde_json::to_string(&assignment.depends_on).unwrap(), position as i64],
        )?;
    }
    transaction.execute(
        "UPDATE agent_runs SET status='waiting',waiting_reason='Plan ready for approval',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
        [&input.run_id],
    )?;
    if status != "waiting" {
        timeline::append(
            &transaction,
            EventRefs {
                project_id: &project_id,
                task_id: Some(&task_id),
                run_id: Some(&input.run_id),
                provider_id: None,
            },
            "run.status_changed",
            serde_json::json!({"from":status,"to":"waiting","reason":"plan_approval"}),
        )?;
    }
    tasks::rollup_in_transaction(&transaction, &task_id)?;
    timeline::append(
        &transaction,
        EventRefs {
            project_id: &project_id,
            task_id: Some(&task_id),
            run_id: Some(&input.run_id),
            provider_id: None,
        },
        "plan.submitted",
        serde_json::json!({"planId": plan_id, "assignmentCount": input.assignments.len(), "summary": input.summary.trim(), "taskTitle": task_title}),
    )?;
    transaction.commit()?;
    Ok(plan_id)
}

fn validate_dependencies(assignments: &[PlanAssignment]) -> Result<(), CommandError> {
    let titles = assignments
        .iter()
        .map(|assignment| assignment.title.trim().to_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let mut remaining = assignments
        .iter()
        .map(|assignment| {
            let title = assignment.title.trim().to_lowercase();
            let dependencies = assignment
                .depends_on
                .iter()
                .map(|dependency| dependency.trim().to_lowercase())
                .collect::<std::collections::HashSet<_>>();
            if dependencies.contains(&title)
                || dependencies
                    .iter()
                    .any(|dependency| !titles.contains(dependency))
            {
                return Err(CommandError::new(
                    "invalid_plan_dependency",
                    "Dependencies must name another assignment in this plan",
                ));
            }
            Ok((title, dependencies))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut resolved = std::collections::HashSet::new();
    loop {
        let ready = remaining
            .iter()
            .filter(|(_, dependencies)| dependencies.is_subset(&resolved))
            .map(|(title, _)| title.clone())
            .collect::<Vec<_>>();
        if ready.is_empty() {
            break;
        }
        resolved.extend(ready);
        remaining.retain(|(title, _)| !resolved.contains(title));
    }
    if remaining.is_empty() {
        Ok(())
    } else {
        Err(CommandError::new(
            "cyclic_plan_dependency",
            "Assignment dependencies must not form a cycle",
        ))
    }
}

pub fn decide(
    database: &Database,
    id: &str,
    decision: &str,
) -> Result<ApprovalRequest, CommandError> {
    if !matches!(decision, "approved" | "denied") {
        return Err(CommandError::new(
            "invalid_decision",
            "Decision must be approved or denied",
        ));
    }
    let request = get_approval(database, id)?;
    if request.status != "pending" {
        return Err(CommandError::new(
            "approval_not_pending",
            "Approval request is no longer pending",
        ));
    }
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    let current: String = transaction.query_row("SELECT r.status||':'||r.updated_at||':'||t.status||':'||t.updated_at FROM approval_requests a JOIN agent_runs r ON r.id=a.agent_run_id JOIN tasks t ON t.id=a.task_id WHERE a.id=?1", [id], |row| row.get(0))?;
    let expected: String = transaction.query_row(
        "SELECT state_fingerprint FROM approval_requests WHERE id=?1",
        [id],
        |row| row.get(0),
    )?;
    let status = if current == expected {
        decision
    } else {
        "expired"
    };
    transaction.execute("UPDATE approval_requests SET status=?1,decided_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2 AND status='pending'", params![status,id])?;
    timeline::append(
        &transaction,
        EventRefs {
            project_id: &request.project_id,
            task_id: Some(&request.task_id),
            run_id: request.run_id.as_deref(),
            provider_id: None,
        },
        &format!("approval.{status}"),
        serde_json::json!({"requestId":id,"action":request.action}),
    )?;
    if status != "approved"
        && let Some(run_id) = request.run_id.as_deref()
    {
        let changed = transaction.execute("UPDATE agent_runs SET status='running',waiting_reason=NULL,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1 AND status='waiting'", [run_id])?;
        if changed > 0 {
            timeline::append(
                &transaction,
                EventRefs {
                    project_id: &request.project_id,
                    task_id: Some(&request.task_id),
                    run_id: Some(run_id),
                    provider_id: None,
                },
                "run.status_changed",
                serde_json::json!({"from":"waiting","to":"running","reason":format!("approval_{status}")}),
            )?;
            tasks::rollup_in_transaction(&transaction, &request.task_id)?;
        }
    }
    transaction.commit()?;
    get_approval(database, id)
}

fn list_approvals(
    database: &Database,
    project_id: &str,
) -> Result<Vec<ApprovalRequest>, CommandError> {
    let connection = database.connect()?;
    let mut statement = connection.prepare("SELECT id,project_id,task_id,agent_run_id,action,arguments_json,status,requested_by,created_at,decided_at,execution_status,execution_result_json,execution_error_code,execution_error_message,executed_at FROM approval_requests WHERE project_id=?1 ORDER BY created_at DESC LIMIT 500")?;
    statement
        .query_map([project_id], row_to_approval)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub(crate) fn get_approval(database: &Database, id: &str) -> Result<ApprovalRequest, CommandError> {
    database.connect()?.query_row("SELECT id,project_id,task_id,agent_run_id,action,arguments_json,status,requested_by,created_at,decided_at,execution_status,execution_result_json,execution_error_code,execution_error_message,executed_at FROM approval_requests WHERE id=?1", [id], row_to_approval).optional()?.ok_or_else(|| CommandError::new("approval_not_found", "Approval request was not found"))
}

pub(crate) fn interrupted_executions(
    database: &Database,
) -> Result<Vec<ApprovalRequest>, CommandError> {
    let connection = database.connect()?;
    let mut statement = connection.prepare("SELECT id,project_id,task_id,agent_run_id,action,arguments_json,status,requested_by,created_at,decided_at,execution_status,execution_result_json,execution_error_code,execution_error_message,executed_at FROM approval_requests WHERE status='approved' AND execution_status IN('not_started','running') ORDER BY created_at")?;
    statement
        .query_map([], row_to_approval)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn row_to_approval(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApprovalRequest> {
    let arguments: String = row.get(5)?;
    let result: Option<String> = row.get(11)?;
    Ok(ApprovalRequest {
        id: row.get(0)?,
        project_id: row.get(1)?,
        task_id: row.get(2)?,
        run_id: row.get(3)?,
        action: row.get(4)?,
        arguments: serde_json::from_str(&arguments).unwrap_or(Value::Null),
        status: row.get(6)?,
        requested_by: row.get(7)?,
        created_at: row.get(8)?,
        decided_at: row.get(9)?,
        execution_status: row.get(10)?,
        execution_result: result.and_then(|value| serde_json::from_str(&value).ok()),
        execution_error_code: row.get(12)?,
        execution_error_message: row.get(13)?,
        executed_at: row.get(14)?,
    })
}

pub(crate) fn claim_execution(database: &Database, id: &str) -> Result<bool, CommandError> {
    Ok(database.connect()?.execute(
        "UPDATE approval_requests SET execution_status='running' WHERE id=?1 AND status='approved' AND execution_status='not_started'",
        [id],
    )? == 1)
}

pub(crate) fn finish_execution(
    database: &Database,
    request: &ApprovalRequest,
    result: Result<Value, CommandError>,
) -> Result<ApprovalRequest, CommandError> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    match result {
        Ok(value) => {
            transaction.execute(
                "UPDATE approval_requests SET execution_status='succeeded',execution_result_json=?1,executed_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2 AND execution_status='running'",
                params![serde_json::to_string(&value).unwrap_or_else(|_| "null".into()), request.id],
            )?;
            if let Some(run_id) = request.run_id.as_deref() {
                transaction.execute("UPDATE agent_runs SET status='running',waiting_reason=NULL,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1 AND status='waiting'", [run_id])?;
                tasks::rollup_in_transaction(&transaction, &request.task_id)?;
            }
            timeline::append(
                &transaction,
                EventRefs {
                    project_id: &request.project_id,
                    task_id: Some(&request.task_id),
                    run_id: request.run_id.as_deref(),
                    provider_id: None,
                },
                "approval.executed",
                serde_json::json!({"requestId":request.id,"action":request.action}),
            )?;
        }
        Err(error) => {
            transaction.execute(
                "UPDATE approval_requests SET execution_status='failed',execution_error_code=?1,execution_error_message=?2,executed_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?3 AND execution_status='running'",
                params![error.code, error.message, request.id],
            )?;
            if let Some(run_id) = request.run_id.as_deref() {
                transaction.execute("UPDATE agent_runs SET waiting_reason=?1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2 AND status='waiting'", params![format!("Approved action failed: {}", error.message), run_id])?;
            }
            timeline::append(
                &transaction,
                EventRefs {
                    project_id: &request.project_id,
                    task_id: Some(&request.task_id),
                    run_id: request.run_id.as_deref(),
                    provider_id: None,
                },
                "approval.execution_failed",
                serde_json::json!({"requestId":request.id,"action":request.action,"code":error.code,"message":error.message}),
            )?;
        }
    }
    transaction.commit()?;
    get_approval(database, &request.id)
}

pub fn run_adapter() -> Result<bool, String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if !matches!(args.first().map(String::as_str), Some("control" | "mcp")) {
        return Ok(false);
    }
    let data_dir = std::env::var_os("SUBSHELL_DATA_DIR").ok_or("SUBSHELL_DATA_DIR is not set")?;
    let run_id = std::env::var("SUBSHELL_RUN_ID").map_err(|_| "SUBSHELL_RUN_ID is not set")?;
    let database =
        Database::initialize(&std::path::PathBuf::from(data_dir).join("subshell.sqlite3"))
            .map_err(|error| error.to_string())?;
    if args[0] == "mcp" {
        run_mcp(&database, &run_id)?;
        return Ok(true);
    }
    let value = match args.get(1).map(String::as_str) {
        Some("snapshot") => {
            serde_json::to_value(snapshot(&database, &run_id).map_err(|error| error.message)?)
                .unwrap()
        }
        Some("request") => {
            let action = args.get(2).ok_or("action is required")?.clone();
            let arguments = args
                .get(3)
                .map(|value| serde_json::from_str(value))
                .transpose()
                .map_err(|error| error.to_string())?
                .unwrap_or_else(|| serde_json::json!({}));
            serde_json::to_value(wait_for_decision(
                &database,
                &request(
                    &database,
                    RequestInput {
                        run_id,
                        action,
                        arguments,
                    },
                ).map_err(|error| error.message)?,
            ).map_err(|error| error.message)?)
            .unwrap()
        }
        Some("report") => {
            report_activity(
                &database,
                ActivityInput {
                    run_id,
                    kind: args.get(2).ok_or("activity kind is required")?.clone(),
                    detail: args.get(3).ok_or("activity detail is required")?.clone(),
                },
            )
            .map_err(|error| error.message)?;
            serde_json::json!({"recorded":true})
        }
        Some("submit-plan") => {
            let value: Value = serde_json::from_str(args.get(2).ok_or("plan JSON is required")?)
                .map_err(|error| error.to_string())?;
            let mut input: SubmitPlanInput = serde_json::from_value(value)
                .map_err(|error| error.to_string())?;
            input.run_id = run_id;
            let plan_id = submit_plan(&database, input).map_err(|error| error.message)?;
            serde_json::json!({"planId":plan_id,"submitted":true})
        }
        _ => return Err(
            "usage: subshell control snapshot | request <action> [json] | report <kind> <detail> | submit-plan <json>"
                .into(),
        ),
    };
    println!("{}", serde_json::to_string_pretty(&value).unwrap());
    Ok(true)
}

fn run_mcp(database: &Database, run_id: &str) -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let result = match method {
            "initialize" => {
                serde_json::json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"subshell","version":"0.1.0"}})
            }
            "tools/list" => serde_json::json!({"tools":[
                {"name":"workspace_snapshot","description":"Read this Run's Task, sibling Runs, and attention state","inputSchema":{"type":"object","properties":{}}},
                {"name":"request_action","description":"Request a visible human-approved application command and wait for its result","inputSchema":{"type":"object","required":["action","arguments"],"properties":{"action":{"type":"string"},"arguments":{"type":"object"}}}},
                {"name":"report_activity","description":"Publish explicitly agent-authored progress without changing lifecycle state","inputSchema":{"type":"object","required":["kind","detail"],"properties":{"kind":{"enum":["progress","validation","changed_path"]},"detail":{"type":"string"}}}}
                ,{"name":"submit_plan","description":"Planner only: name the Task and submit 1-8 independent, non-recursive assignments for SubShell to launch","inputSchema":{"type":"object","required":["assignments"],"properties":{"taskTitle":{"type":"string","maxLength":72},"summary":{"type":"string"},"assignments":{"type":"array","minItems":1,"maxItems":8,"items":{"type":"object","required":["title","instruction"],"properties":{"title":{"type":"string"},"instruction":{"type":"string"},"role":{"enum":["executor","research","test","reviewer"]},"allowedPaths":{"type":"array","items":{"type":"string"}}}}}}}}
            ]}),
            "tools/call" => {
                let params = request.get("params").cloned().unwrap_or(Value::Null);
                let value = match params.get("name").and_then(Value::as_str) {
                    Some("workspace_snapshot") => serde_json::to_value(
                        snapshot(database, run_id).map_err(|error| error.message)?,
                    )
                    .unwrap(),
                    Some("request_action") => {
                        serde_json::to_value(request_action_from_mcp(database, run_id, &params)?)
                            .unwrap()
                    }
                    Some("report_activity") => {
                        report_activity_from_mcp(database, run_id, &params)?;
                        serde_json::json!({"recorded":true})
                    }
                    Some("submit_plan") => {
                        let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
                        let mut input: SubmitPlanInput =
                            serde_json::from_value(arguments).map_err(|error| error.to_string())?;
                        input.run_id = run_id.into();
                        serde_json::json!({"planId":submit_plan(database, input).map_err(|error| error.message)?})
                    }
                    _ => return Err("unknown MCP tool".into()),
                };
                serde_json::json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&value).unwrap()}]})
            }
            "notifications/initialized" => continue,
            _ => serde_json::json!({}),
        };
        println!(
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id":id,"result":result})
        );
        io::stdout().flush().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn executor_role() -> String {
    "executor".into()
}

fn request_action_from_mcp(
    database: &Database,
    run_id: &str,
    params: &Value,
) -> Result<ApprovalRequest, String> {
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .ok_or("action is required")?
        .to_string();
    let action_arguments = arguments
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let request = request(
        database,
        RequestInput {
            run_id: run_id.into(),
            action,
            arguments: action_arguments,
        },
    )
    .map_err(|error| error.message)?;
    wait_for_decision(database, &request).map_err(|error| error.message)
}

fn wait_for_decision(
    database: &Database,
    request: &ApprovalRequest,
) -> Result<ApprovalRequest, CommandError> {
    loop {
        let current = get_approval(database, &request.id)?;
        if matches!(current.status.as_str(), "denied" | "expired")
            || matches!(current.execution_status.as_str(), "succeeded" | "failed")
        {
            return Ok(current);
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn report_activity_from_mcp(
    database: &Database,
    run_id: &str,
    params: &Value,
) -> Result<(), String> {
    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
    report_activity(
        database,
        ActivityInput {
            run_id: run_id.into(),
            kind: arguments
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            detail: arguments
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
        },
    )
    .map_err(|error| error.message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn rejects_recursive_or_unknown_agent_mutations() {
        let root = tempdir().unwrap();
        let database = Database::initialize(&root.path().join("db.sqlite3")).unwrap();
        let error = request(
            &database,
            RequestInput {
                run_id: "missing".into(),
                action: "spawn_agent".into(),
                arguments: serde_json::json!({}),
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "unauthorized_action");
    }

    #[test]
    fn planner_submits_only_bounded_non_recursive_assignments() {
        let root = tempdir().unwrap();
        let database = Database::initialize(&root.path().join("db.sqlite3")).unwrap();
        let connection = database.connect().unwrap();
        connection.execute("INSERT INTO projects(id,name,path,created_at,updated_at) VALUES('project','Project','/tmp/project','now','now')", []).unwrap();
        connection.execute("INSERT INTO provider_accounts(id,provider_type,display_name,config_scope_path,status,created_at,updated_at) VALUES('provider','generic','Provider','/tmp/provider','active','now','now')", []).unwrap();
        connection.execute("INSERT INTO tasks(id,project_id,title,description,status,base_branch,base_revision,created_at,updated_at) VALUES('task','project','Goal','Original detailed user request','working','main','abc','now','now')", []).unwrap();
        connection.execute("INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,role,status,created_at,updated_at) VALUES('planner','task','provider','Plan','planner','running','now','now')", []).unwrap();

        let plan_id = submit_plan(
            &database,
            SubmitPlanInput {
                run_id: "planner".into(),
                task_title: "Build project view".into(),
                summary: "Split UI and API".into(),
                assignments: vec![
                    PlanAssignment {
                        title: "UI".into(),
                        instruction: "Build the view".into(),
                        role: "executor".into(),
                        allowed_paths: vec!["src/features/view".into()],
                        depends_on: vec![],
                    },
                    PlanAssignment {
                        title: "API".into(),
                        instruction: "Build the command".into(),
                        role: "test".into(),
                        allowed_paths: vec!["src-tauri/src/features/view".into()],
                        depends_on: vec!["UI".into()],
                    },
                ],
            },
        )
        .unwrap();

        assert_eq!(
            database
                .connect()
                .unwrap()
                .query_row::<i64, _, _>(
                    "SELECT COUNT(*) FROM task_plan_assignments WHERE plan_id=?1",
                    [plan_id],
                    |row| row.get(0)
                )
                .unwrap(),
            2
        );
        let (title, description): (String, String) = database
            .connect()
            .unwrap()
            .query_row(
                "SELECT title,description FROM tasks WHERE id='task'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(title, "Build project view");
        assert_eq!(description, "Original detailed user request");
        let error = submit_plan(
            &database,
            SubmitPlanInput {
                run_id: "planner".into(),
                task_title: String::new(),
                summary: String::new(),
                assignments: vec![PlanAssignment {
                    title: "Nested".into(),
                    instruction: "Plan again".into(),
                    role: "planner".into(),
                    allowed_paths: vec![],
                    depends_on: vec![],
                }],
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "invalid_run_role");
    }

    #[test]
    fn isolates_reads_and_keeps_mutations_behind_fresh_human_approval() {
        let root = tempdir().unwrap();
        let database = Database::initialize(&root.path().join("db.sqlite3")).unwrap();
        let connection = database.connect().unwrap();
        for (id, path) in [("p1", "/tmp/p1"), ("p2", "/tmp/p2")] {
            connection.execute("INSERT INTO projects(id,name,path,created_at,updated_at) VALUES(?1,?1,?2,'now','now')", params![id,path]).unwrap();
        }
        connection.execute("INSERT INTO provider_accounts(id,provider_type,display_name,config_scope_path,status,created_at,updated_at) VALUES('provider','generic','Provider','/tmp/provider','active','now','now')", []).unwrap();
        for (task, project) in [("t1", "p1"), ("t2", "p2")] {
            connection.execute("INSERT INTO tasks(id,project_id,title,status,base_branch,base_revision,created_at,updated_at) VALUES(?1,?2,?1,'working','main','abc','now','now')", params![task,project]).unwrap();
        }
        for (run, task) in [("r1", "t1"), ("r2", "t2")] {
            connection.execute("INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,status,created_at,updated_at) VALUES(?1,?2,'provider','Work','running','now','now')", params![run,task]).unwrap();
        }
        let state = snapshot(&database, "r1").unwrap();
        assert_eq!(state.task.id, "t1");
        assert_eq!(
            state
                .runs
                .iter()
                .map(|run| run.id.as_str())
                .collect::<Vec<_>>(),
            ["r1"]
        );

        let approved = request(
            &database,
            RequestInput {
                run_id: "r1".into(),
                action: "start_run".into(),
                arguments: serde_json::json!({"instruction":"next"}),
            },
        )
        .unwrap();
        assert_eq!(approved.status, "pending");
        assert_eq!(
            decide(&database, &approved.id, "approved").unwrap().status,
            "approved"
        );
        let denied = request(
            &database,
            RequestInput {
                run_id: "r1".into(),
                action: "clean_resources".into(),
                arguments: serde_json::json!({}),
            },
        )
        .unwrap();
        assert_eq!(
            decide(&database, &denied.id, "denied").unwrap().status,
            "denied"
        );
        let stale = request(
            &database,
            RequestInput {
                run_id: "r1".into(),
                action: "merge_task".into(),
                arguments: serde_json::json!({}),
            },
        )
        .unwrap();
        connection
            .execute("UPDATE tasks SET updated_at='later' WHERE id='t1'", [])
            .unwrap();
        assert_eq!(
            decide(&database, &stale.id, "approved").unwrap().status,
            "expired"
        );
        assert_eq!(
            database
                .connect()
                .unwrap()
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM agent_runs", [], |row| row.get(0))
                .unwrap(),
            2
        );
    }
}
