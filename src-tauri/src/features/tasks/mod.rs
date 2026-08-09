use std::path::Path;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::{
    contracts::{CommandError, Page},
    features::timeline::{self, EventRefs},
    platform::{database::Database, git::GitService},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTask {
    pub project_id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    #[serde(default)]
    pub validation_commands: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub confirm_dirty_base: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectId {
    pub project_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskStatus {
    pub task_id: String,
    pub status: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub queue_position: Option<i64>,
    pub base_branch: String,
    pub base_revision: String,
    pub acceptance_criteria: Vec<String>,
    pub allowed_paths: Vec<String>,
    pub validation_commands: Vec<String>,
    pub decisions: Vec<String>,
    pub updated_at: String,
}

#[tauri::command]
pub fn tasks_create(
    input: CreateTask,
    database: State<Database>,
    git: State<GitService>,
) -> Result<Task, CommandError> {
    create(input, &database, &git)
}

#[tauri::command]
pub fn tasks_list(input: ProjectId, database: State<Database>) -> Result<Page<Task>, CommandError> {
    Ok(Page::first(list(&database, &input.project_id, false)?))
}

#[tauri::command]
pub fn tasks_list_archived(
    input: ProjectId,
    database: State<Database>,
) -> Result<Page<Task>, CommandError> {
    Ok(Page::first(list(&database, &input.project_id, true)?))
}

#[tauri::command]
pub fn tasks_get(id: String, database: State<Database>) -> Result<Task, CommandError> {
    get(&database, &id)?.ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))
}

#[tauri::command]
pub fn tasks_update_status(
    input: UpdateTaskStatus,
    database: State<Database>,
) -> Result<Task, CommandError> {
    update_status(&database, &input.task_id, &input.status)
}

pub(crate) fn create(
    input: CreateTask,
    database: &Database,
    git: &GitService,
) -> Result<Task, CommandError> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err(CommandError::new("invalid_task", "Title is required"));
    }
    let mut connection = database.connect()?;
    let path: String = connection
        .query_row(
            "SELECT path FROM projects WHERE id=?1",
            [&input.project_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| CommandError::new("project_not_found", "Project was not found"))?;
    let status = git.status(Path::new(&path))?;
    if !status.is_repository {
        return Err(CommandError::new(
            "non_repository",
            "Tasks require a Git repository",
        ));
    }
    let revision = status.revision.ok_or_else(|| {
        CommandError::new(
            "unborn_repository",
            "Create the repository's first commit before starting an agent",
        )
    })?;
    if status.dirty && !input.confirm_dirty_base {
        return Err(CommandError::new(
            "dirty_base_requires_confirmation",
            "Uncommitted changes are excluded; confirm the committed base to continue",
        ));
    }
    let id = Uuid::new_v4().to_string();
    let transaction = connection.transaction()?;
    transaction.execute("INSERT INTO tasks (id,project_id,title,description,status,base_branch,base_revision,acceptance_criteria_json,allowed_paths_json,validation_commands_json,decisions_json,created_at,updated_at) VALUES (?1,?2,?3,?4,'task',?5,?6,?7,?8,?9,?10,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'))",params![id,input.project_id,title,input.description,status.branch.unwrap_or_else(||"HEAD".into()),revision,json(&input.acceptance_criteria)?,json(&input.allowed_paths)?,json(&input.validation_commands)?,json(&input.decisions)?])?;
    timeline::append(
        &transaction,
        EventRefs {
            project_id: &input.project_id,
            task_id: Some(&id),
            ..Default::default()
        },
        "task.created",
        serde_json::json!({ "status": "task", "title": title }),
    )?;
    transaction.commit()?;
    get(database, &id)?
        .ok_or_else(|| CommandError::new("task_not_found", "Task was not found after creation"))
}

fn list(database: &Database, project_id: &str, archived: bool) -> Result<Vec<Task>, CommandError> {
    let connection = database.connect()?;
    let mut statement = connection.prepare(
        "SELECT id FROM tasks WHERE project_id=?1 AND ((?2=1 AND archived_at IS NOT NULL) OR (?2=0 AND archived_at IS NULL)) ORDER BY COALESCE(archived_at,updated_at) DESC LIMIT 100",
    )?;
    let ids = statement
        .query_map(params![project_id, archived], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.iter()
        .map(|id| {
            get(database, id)?
                .ok_or_else(|| CommandError::new("task_not_found", "Task disappeared"))
        })
        .collect()
}

pub fn get(database: &Database, id: &str) -> Result<Option<Task>, CommandError> {
    let connection = database.connect()?;
    connection.query_row("SELECT id,project_id,title,description,status,queue_position,base_branch,base_revision,acceptance_criteria_json,allowed_paths_json,validation_commands_json,decisions_json,updated_at FROM tasks WHERE id=?1",[id],|row|Ok(Task{id:row.get(0)?,project_id:row.get(1)?,title:row.get(2)?,description:row.get(3)?,status:row.get(4)?,queue_position:row.get(5)?,base_branch:row.get(6)?,base_revision:row.get(7)?,acceptance_criteria:parse(row.get(8)?),allowed_paths:parse(row.get(9)?),validation_commands:parse(row.get(10)?),decisions:parse(row.get(11)?),updated_at:row.get(12)?})).optional().map_err(Into::into)
}

fn update_status(database: &Database, id: &str, status: &str) -> Result<Task, CommandError> {
    const STATUSES: [&str; 11] = [
        "idea",
        "task",
        "queued",
        "working",
        "waiting",
        "review",
        "approved",
        "merged",
        "archived",
        "failed",
        "cancelled",
    ];
    if !STATUSES.contains(&status) {
        return Err(CommandError::new(
            "invalid_task_status",
            "Task status is not supported",
        ));
    }
    let mut connection = database.connect()?;
    let (project_id, current): (String, String) = connection
        .query_row(
            "SELECT project_id,status FROM tasks WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
    if current != status && !can_transition(&current, status) {
        return Err(CommandError::new(
            "illegal_task_transition",
            format!("Task cannot move from {current} to {status}"),
        ));
    }
    let active_runs: i64 = connection.query_row("SELECT COUNT(*) FROM agent_runs WHERE task_id=?1 AND status IN ('queued','preparing','running')", [id], |row| row.get(0))?;
    if active_runs > 0 && !matches!(status, "working" | "waiting") {
        return Err(CommandError::new(
            "task_has_active_runs",
            "Stop active agents before moving this task",
        ));
    }
    let transaction = connection.transaction()?;
    let changed = transaction.execute("UPDATE tasks SET status=?1,archived_at=CASE WHEN ?1='archived' THEN strftime('%Y-%m-%dT%H:%M:%fZ','now') ELSE NULL END,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2", params![status, id])?;
    if changed == 0 {
        return Err(CommandError::new("task_not_found", "Task was not found"));
    }
    if current != status {
        timeline::append(
            &transaction,
            EventRefs {
                project_id: &project_id,
                task_id: Some(id),
                ..Default::default()
            },
            "task.status_changed",
            serde_json::json!({ "from": current, "to": status }),
        )?;
    }
    transaction.commit()?;
    get(database, id)?
        .ok_or_else(|| CommandError::new("task_not_found", "Task was not found after update"))
}

pub(crate) fn rollup_run_statuses(statuses: &[String]) -> Option<&'static str> {
    if statuses.iter().any(|status| status == "failed") {
        Some("failed")
    } else if statuses.iter().any(|status| status == "waiting") {
        Some("waiting")
    } else if statuses
        .iter()
        .any(|status| matches!(status.as_str(), "preparing" | "running"))
    {
        Some("working")
    } else if !statuses.is_empty() && statuses.iter().all(|status| status == "succeeded") {
        Some("review")
    } else if !statuses.is_empty()
        && statuses
            .iter()
            .all(|status| matches!(status.as_str(), "succeeded" | "failed" | "cancelled"))
        && statuses.iter().any(|status| status == "cancelled")
    {
        Some("cancelled")
    } else if statuses.iter().any(|status| status == "queued") {
        Some("queued")
    } else {
        None
    }
}

pub(crate) fn rollup_in_transaction(
    connection: &rusqlite::Connection,
    task_id: &str,
) -> Result<Option<String>, CommandError> {
    let (project_id, current): (String, String) = connection.query_row(
        "SELECT project_id,status FROM tasks WHERE id=?1",
        [task_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let mut statement =
        connection.prepare("SELECT status FROM agent_runs WHERE task_id=?1 ORDER BY created_at")?;
    let statuses = statement
        .query_map([task_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let Some(next) = rollup_run_statuses(&statuses) else {
        return Ok(None);
    };
    if matches!(current.as_str(), "approved" | "merged" | "archived") {
        return Ok(Some(current));
    }
    if current == next {
        return Ok(Some(current));
    }
    connection.execute(
        "UPDATE tasks SET status=?1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2",
        params![next, task_id],
    )?;
    timeline::append(
        connection,
        EventRefs {
            project_id: &project_id,
            task_id: Some(task_id),
            ..Default::default()
        },
        "task.status_changed",
        serde_json::json!({ "from": current, "to": next, "source": "run_rollup" }),
    )?;
    Ok(Some(next.into()))
}

fn can_transition(from: &str, to: &str) -> bool {
    const BOARD_STATUSES: [&str; 5] = ["task", "working", "review", "failed", "approved"];
    if from != "archived" && BOARD_STATUSES.contains(&to) {
        return true;
    }
    matches!(
        (from, to),
        ("idea", "task" | "archived")
            | ("task", "queued" | "working" | "archived")
            | ("queued", "task" | "working" | "cancelled" | "archived")
            | ("working", "waiting" | "review" | "failed" | "cancelled")
            | ("waiting", "working" | "failed" | "cancelled")
            | ("review", "approved" | "working" | "archived")
            | ("approved", "review" | "merged" | "archived")
            | ("merged", "archived")
            | ("failed" | "cancelled", "queued" | "working" | "archived")
            | ("archived", "task")
    )
}

fn json(values: &[String]) -> Result<String, CommandError> {
    serde_json::to_string(values).map_err(|e| CommandError::new("invalid_task", e.to_string()))
}
fn parse(value: String) -> Vec<String> {
    serde_json::from_str(&value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::projects;
    use tempfile::tempdir;

    #[test]
    fn rejects_an_unborn_repository_with_a_stable_error() {
        let root = tempdir().unwrap();
        let project = root.path().join("repo");
        std::fs::create_dir(&project).unwrap();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&project)
            .status()
            .unwrap();
        let db = Database::initialize(&root.path().join("db")).unwrap();
        let git = GitService::default();
        let opened = projects::open(project.to_str().unwrap(), &db, &git).unwrap();
        let error = create(
            CreateTask {
                project_id: opened.id,
                title: "T".into(),
                description: String::new(),
                acceptance_criteria: vec![],
                allowed_paths: vec![],
                validation_commands: vec![],
                decisions: vec![],
                confirm_dirty_base: false,
            },
            &db,
            &git,
        )
        .unwrap_err();
        assert_eq!(error.code, "unborn_repository");
    }

    #[test]
    fn rejects_dirty_base_until_confirmed() {
        let root = tempdir().unwrap();
        let project = root.path().join("repo");
        std::fs::create_dir(&project).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "a@b.c"],
            vec!["config", "user.name", "A"],
        ] {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&project)
                .status()
                .unwrap();
        }
        std::fs::write(project.join("a"), "a").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&project)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-qm", "a"])
            .current_dir(&project)
            .status()
            .unwrap();
        let db = Database::initialize(&root.path().join("db")).unwrap();
        let git = GitService::default();
        let p = projects::open(project.to_str().unwrap(), &db, &git).unwrap();
        std::fs::write(project.join("a"), "b").unwrap();
        let make = |confirm| CreateTask {
            project_id: p.id.clone(),
            title: "T".into(),
            description: "".into(),
            acceptance_criteria: vec![],
            allowed_paths: vec![],
            validation_commands: vec![],
            decisions: vec![],
            confirm_dirty_base: confirm,
        };
        assert_eq!(
            create(make(false), &db, &git).unwrap_err().code,
            "dirty_base_requires_confirmation"
        );
        let task = create(make(true), &db, &git).unwrap();
        assert_eq!(
            update_status(&db, &task.id, "approved").unwrap().status,
            "approved"
        );
        assert_eq!(
            update_status(&db, &task.id, "working").unwrap().status,
            "working"
        );
        assert_eq!(
            update_status(&db, &task.id, "review").unwrap().status,
            "review"
        );
        for status in ["failed", "approved", "failed", "review"] {
            assert_eq!(update_status(&db, &task.id, status).unwrap().status, status);
        }
        assert_eq!(
            update_status(&db, &task.id, "archived").unwrap().status,
            "archived"
        );
        assert_eq!(list(&db, &task.project_id, true).unwrap().len(), 1);
        assert_eq!(update_status(&db, &task.id, "task").unwrap().status, "task");
        assert!(list(&db, &task.project_id, true).unwrap().is_empty());
        assert_eq!(
            update_status(&db, &task.id, "unknown").unwrap_err().code,
            "invalid_task_status"
        );
    }

    #[test]
    fn run_rollup_uses_the_required_precedence() {
        let cases = [
            (vec!["running", "failed"], Some("failed")),
            (vec!["running", "waiting"], Some("waiting")),
            (vec!["succeeded", "running"], Some("working")),
            (vec!["succeeded", "succeeded"], Some("review")),
            (vec!["succeeded", "cancelled"], Some("cancelled")),
            (vec!["queued", "queued"], Some("queued")),
            (vec![], None),
        ];
        for (statuses, expected) in cases {
            assert_eq!(
                rollup_run_statuses(&statuses.into_iter().map(str::to_string).collect::<Vec<_>>()),
                expected
            );
        }
    }
}
