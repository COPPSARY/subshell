use std::path::Path;

use rusqlite::OptionalExtension;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use tauri::State;

use crate::{
    contracts::CommandError,
    features::{
        agent_api::{self, ApprovalRequest, DecisionInput},
        context_sharing, review,
        runs::{self, RunService},
    },
    platform::{
        database::Database, environment::RuntimePaths, git::GitService, process::ProcessSupervisor,
    },
};

#[tauri::command]
pub fn workspace_decide_action(
    input: DecisionInput,
    database: State<Database>,
    runs: State<RunService>,
    paths: State<RuntimePaths>,
    git: State<GitService>,
    processes: State<ProcessSupervisor>,
) -> Result<ApprovalRequest, CommandError> {
    decide_and_execute(&input, &database, &runs, &paths, &git, &processes)
}

fn decide_and_execute(
    input: &DecisionInput,
    database: &Database,
    runs: &RunService,
    paths: &RuntimePaths,
    git: &GitService,
    processes: &ProcessSupervisor,
) -> Result<ApprovalRequest, CommandError> {
    let request = agent_api::decide(database, &input.request_id, &input.decision)?;
    if request.status != "approved" || !agent_api::claim_execution(&database, &request.id)? {
        return agent_api::get_approval(database, &request.id);
    }
    let result = execute(&request, database, runs, paths, git, processes);
    agent_api::finish_execution(database, &request, result)
}

fn execute(
    request: &ApprovalRequest,
    database: &Database,
    runs: &RunService,
    paths: &RuntimePaths,
    git: &GitService,
    processes: &ProcessSupervisor,
) -> Result<Value, CommandError> {
    match request.action.as_str() {
        "start_run" => {
            let assignment = request
                .arguments
                .get("assignment")
                .cloned()
                .unwrap_or_else(|| request.arguments.clone());
            let assignment = from_json(assignment)?;
            json(runs.start_approved(runs::StartInput {
                task_id: request.task_id.clone(),
                assignments: vec![assignment],
            })?)
        }
        "share_context" => {
            let input = context_sharing::SharePreviewInput {
                source_run_id: request.run_id.clone(),
                target_run_id: string(&request.arguments, "targetRunId")?.into(),
                kind: string(&request.arguments, "kind")?.into(),
                content_reference: request
                    .arguments
                    .get("contentReference")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                summary: request
                    .arguments
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
            };
            let preview = context_sharing::preview(database, &input)?;
            json(context_sharing::deliver(
                database,
                processes,
                context_sharing::ShareDeliverInput {
                    source_run_id: input.source_run_id,
                    target_run_id: input.target_run_id,
                    kind: input.kind,
                    content_reference: input.content_reference,
                    content: preview.content,
                    preview_sha256: preview.sha256,
                },
            )?)
        }
        "create_branch" => create_branch(request, database, paths, git),
        "clean_resources" => {
            let run_id = request
                .arguments
                .get("runId")
                .and_then(Value::as_str)
                .or(request.run_id.as_deref())
                .ok_or_else(|| CommandError::new("run_not_found", "Choose a Run to clean"))?;
            ensure_run_task(database, run_id, &request.task_id)?;
            runs.clean_resources(run_id)?;
            Ok(serde_json::json!({"runId":run_id,"cleaned":true}))
        }
        "approve_task" => {
            let attempt_id = string(&request.arguments, "attemptId")?;
            ensure_attempt_task(database, attempt_id, &request.task_id)?;
            json(review::decide(
                review::ReviewDecisionInput {
                    attempt_id: attempt_id.into(),
                    fingerprint: string(&request.arguments, "fingerprint")?.into(),
                    feedback: String::new(),
                },
                "approved",
                database,
                git,
            )?)
        }
        "merge_task" => {
            let attempt_id = string(&request.arguments, "attemptId")?;
            ensure_attempt_task(database, attempt_id, &request.task_id)?;
            Ok(serde_json::json!({"revision":review::merge(
                review::MergeInput {
                    attempt_id: attempt_id.into(),
                    fingerprint: string(&request.arguments, "fingerprint")?.into(),
                },
                database,
                paths,
                git,
            )?}))
        }
        _ => Err(CommandError::new(
            "unauthorized_action",
            "WorkspaceControl does not expose that action",
        )),
    }
}

fn create_branch(
    request: &ApprovalRequest,
    database: &Database,
    paths: &RuntimePaths,
    git: &GitService,
) -> Result<Value, CommandError> {
    let run_id = request
        .arguments
        .get("runId")
        .and_then(Value::as_str)
        .or(request.run_id.as_deref())
        .ok_or_else(|| CommandError::new("run_not_found", "Choose a Run to publish"))?;
    ensure_run_task(database, run_id, &request.task_id)?;
    let (project_path, worktree, base): (String, String, String) = database.connect()?.query_row(
        "SELECT p.path,w.path,t.base_revision FROM agent_runs r JOIN tasks t ON t.id=r.task_id JOIN projects p ON p.id=t.project_id JOIN worktrees w ON w.agent_run_id=r.id WHERE r.id=?1",
        [run_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let name = string(&request.arguments, "name")?;
    let patch = git.exact_diff(Path::new(&worktree), &base)?;
    let revision = git.create_snapshot_branch(
        Path::new(&project_path),
        &base,
        &paths.data_dir.join("approved-actions").join(&request.id),
        name,
        &patch.patch,
        request
            .arguments
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Publish approved agent work"),
    )?;
    Ok(serde_json::json!({"branch":name,"revision":revision,"runId":run_id}))
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, CommandError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CommandError::new("invalid_arguments", format!("{key} is required")))
}

fn from_json<T: DeserializeOwned>(value: Value) -> Result<T, CommandError> {
    serde_json::from_value(value)
        .map_err(|error| CommandError::new("invalid_arguments", error.to_string()))
}

fn json(value: impl Serialize) -> Result<Value, CommandError> {
    serde_json::to_value(value)
        .map_err(|error| CommandError::new("invalid_action_result", error.to_string()))
}

fn ensure_run_task(database: &Database, run_id: &str, task_id: &str) -> Result<(), CommandError> {
    let owner: Option<String> = database
        .connect()?
        .query_row(
            "SELECT task_id FROM agent_runs WHERE id=?1",
            [run_id],
            |row| row.get(0),
        )
        .optional()?;
    if owner.as_deref() == Some(task_id) {
        Ok(())
    } else {
        Err(CommandError::new(
            "run_not_found",
            "Run does not belong to this Task",
        ))
    }
}

fn ensure_attempt_task(
    database: &Database,
    attempt_id: &str,
    task_id: &str,
) -> Result<(), CommandError> {
    let owner: Option<String> = database.connect()?.query_row(
        "SELECT r.task_id FROM review_attempts a JOIN review_records r ON r.id=a.review_record_id WHERE a.id=?1",
        [attempt_id],
        |row| row.get(0),
    ).optional()?;
    if owner.as_deref() == Some(task_id) {
        Ok(())
    } else {
        Err(CommandError::new(
            "review_not_found",
            "Review does not belong to this Task",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        features::context::ContextDrafts,
        platform::{environment::PortLeases, keychain::MemorySecretStore},
    };
    use std::{fs, process::Command, sync::Arc};

    fn fixture() -> (
        tempfile::TempDir,
        Database,
        RunService,
        RuntimePaths,
        GitService,
        ProcessSupervisor,
    ) {
        let root = tempfile::tempdir().unwrap();
        let paths = RuntimePaths {
            data_dir: root.path().join("data"),
        };
        let database = Database::initialize(&paths.data_dir.join("db.sqlite3")).unwrap();
        let repository = root.path().join("repository");
        fs::create_dir(&repository).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&repository)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        fs::write(repository.join("base.txt"), "base\n").unwrap();
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
        let git = GitService::default();
        let status = git.status(&repository).unwrap();
        let base = status.revision.unwrap();
        let branch = status.branch.unwrap();
        let worktree = root.path().join("worktree");
        git.create_worktree(&repository, &base, &worktree).unwrap();
        fs::write(worktree.join("change.txt"), "approved\n").unwrap();
        let connection = database.connect().unwrap();
        connection.execute("INSERT INTO projects(id,name,path,created_at,updated_at) VALUES('project','Project',?1,'now','now')", [repository.to_string_lossy()]).unwrap();
        connection.execute("INSERT INTO provider_accounts(id,provider_type,display_name,config_scope_path,status,created_at,updated_at) VALUES('provider','generic','Provider','/tmp/provider','active','now','now')", []).unwrap();
        connection.execute("INSERT INTO tasks(id,project_id,title,status,base_branch,base_revision,created_at,updated_at) VALUES('task','project','Task','working',?1,?2,'now','now')", rusqlite::params![branch, base]).unwrap();
        connection.execute("INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,role,status,created_at,updated_at) VALUES('run','task','provider','Work','implementer','running','now','now')", []).unwrap();
        connection.execute("INSERT INTO worktrees(id,agent_run_id,path,base_branch,base_revision,state,created_at) VALUES('worktree','run',?1,?2,?3,'active','now')", rusqlite::params![worktree.to_string_lossy(), branch, base]).unwrap();
        drop(connection);
        let processes = ProcessSupervisor::default();
        let runs = RunService::new(
            database.clone(),
            paths.clone(),
            git.clone(),
            ContextDrafts::default(),
            processes.clone(),
            PortLeases::default(),
            Arc::new(MemorySecretStore::default()),
        );
        (root, database, runs, paths, git, processes)
    }

    #[test]
    fn approved_branch_is_invisible_before_approval_and_executes_once() {
        let (root, database, runs, paths, git, processes) = fixture();
        let approved = agent_api::request(
            &database,
            agent_api::RequestInput {
                run_id: "run".into(),
                action: "create_branch".into(),
                arguments: serde_json::json!({"name":"approved/change"}),
            },
        )
        .unwrap();
        assert!(
            !Command::new("git")
                .args([
                    "show-ref",
                    "--verify",
                    "--quiet",
                    "refs/heads/approved/change"
                ])
                .current_dir(root.path().join("repository"))
                .status()
                .unwrap()
                .success()
        );
        let executed = decide_and_execute(
            &DecisionInput {
                request_id: approved.id.clone(),
                decision: "approved".into(),
            },
            &database,
            &runs,
            &paths,
            &git,
            &processes,
        )
        .unwrap();
        assert_eq!(executed.execution_status, "succeeded");
        assert!(
            Command::new("git")
                .args([
                    "show-ref",
                    "--verify",
                    "--quiet",
                    "refs/heads/approved/change"
                ])
                .current_dir(root.path().join("repository"))
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            database
                .connect()
                .unwrap()
                .query_row::<i64, _, _>(
                    "SELECT COUNT(*) FROM timeline_events WHERE event_type='approval.executed'",
                    [],
                    |row| row.get(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            decide_and_execute(
                &DecisionInput {
                    request_id: approved.id,
                    decision: "approved".into()
                },
                &database,
                &runs,
                &paths,
                &git,
                &processes
            )
            .unwrap_err()
            .code,
            "approval_not_pending"
        );

        let denied = agent_api::request(
            &database,
            agent_api::RequestInput {
                run_id: "run".into(),
                action: "create_branch".into(),
                arguments: serde_json::json!({"name":"denied/change"}),
            },
        )
        .unwrap();
        assert_eq!(
            decide_and_execute(
                &DecisionInput {
                    request_id: denied.id,
                    decision: "denied".into()
                },
                &database,
                &runs,
                &paths,
                &git,
                &processes
            )
            .unwrap()
            .status,
            "denied"
        );
        assert!(
            !Command::new("git")
                .args([
                    "show-ref",
                    "--verify",
                    "--quiet",
                    "refs/heads/denied/change"
                ])
                .current_dir(root.path().join("repository"))
                .status()
                .unwrap()
                .success()
        );

        let invalid = agent_api::request(
            &database,
            agent_api::RequestInput {
                run_id: "run".into(),
                action: "create_branch".into(),
                arguments: serde_json::json!({"name":"invalid name"}),
            },
        )
        .unwrap();
        let failed = decide_and_execute(
            &DecisionInput {
                request_id: invalid.id,
                decision: "approved".into(),
            },
            &database,
            &runs,
            &paths,
            &git,
            &processes,
        )
        .unwrap();
        assert_eq!(failed.execution_status, "failed");
        assert_eq!(
            failed.execution_error_code.as_deref(),
            Some("invalid_branch_name")
        );
        assert_eq!(
            database
                .connect()
                .unwrap()
                .query_row::<String, _, _>(
                    "SELECT status FROM agent_runs WHERE id='run'",
                    [],
                    |row| row.get(0)
                )
                .unwrap(),
            "waiting"
        );
    }
}
