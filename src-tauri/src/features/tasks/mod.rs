use std::path::Path;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::{
    contracts::{CommandError, Page},
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub description: String,
    pub status: String,
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
    Ok(Page::first(list(&database, &input.project_id)?))
}

#[tauri::command]
pub fn tasks_get(id: String, database: State<Database>) -> Result<Task, CommandError> {
    get(&database, &id)?.ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))
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
    let connection = database.connect()?;
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
    if status.dirty && !input.confirm_dirty_base {
        return Err(CommandError::new(
            "dirty_base_requires_confirmation",
            "Uncommitted changes are excluded; confirm the committed base to continue",
        ));
    }
    let id = Uuid::new_v4().to_string();
    connection.execute("INSERT INTO tasks (id,project_id,title,description,status,base_branch,base_revision,acceptance_criteria_json,allowed_paths_json,validation_commands_json,decisions_json,created_at,updated_at) VALUES (?1,?2,?3,?4,'task',?5,?6,?7,?8,?9,?10,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'))",params![id,input.project_id,title,input.description,status.branch.unwrap_or_else(||"HEAD".into()),status.revision.unwrap(),json(&input.acceptance_criteria)?,json(&input.allowed_paths)?,json(&input.validation_commands)?,json(&input.decisions)?])?;
    get(database, &id)?
        .ok_or_else(|| CommandError::new("task_not_found", "Task was not found after creation"))
}

fn list(database: &Database, project_id: &str) -> Result<Vec<Task>, CommandError> {
    let connection = database.connect()?;
    let mut statement=connection.prepare("SELECT id FROM tasks WHERE project_id=?1 AND archived_at IS NULL ORDER BY updated_at DESC LIMIT 100")?;
    let ids = statement
        .query_map([project_id], |row| row.get::<_, String>(0))?
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
    connection.query_row("SELECT id,project_id,title,description,status,base_branch,base_revision,acceptance_criteria_json,allowed_paths_json,validation_commands_json,decisions_json,updated_at FROM tasks WHERE id=?1",[id],|row|Ok(Task{id:row.get(0)?,project_id:row.get(1)?,title:row.get(2)?,description:row.get(3)?,status:row.get(4)?,base_branch:row.get(5)?,base_revision:row.get(6)?,acceptance_criteria:parse(row.get(7)?),allowed_paths:parse(row.get(8)?),validation_commands:parse(row.get(9)?),decisions:parse(row.get(10)?),updated_at:row.get(11)?})).optional().map_err(Into::into)
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
        assert!(create(make(true), &db, &git).is_ok());
    }
}
