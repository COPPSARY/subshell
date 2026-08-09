use std::path::Path;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::{
    contracts::{CommandError, Page},
    platform::{
        database::Database,
        git::{GitService, GitStatus},
    },
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub last_opened_at: String,
    pub git: GitStatus,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPath {
    pub path: String,
}

#[tauri::command]
pub fn projects_open(
    input: ProjectPath,
    database: State<Database>,
    git: State<GitService>,
) -> Result<Project, CommandError> {
    open(&input.path, &database, &git)
}

#[tauri::command]
pub fn projects_list(
    database: State<Database>,
    git: State<GitService>,
) -> Result<Page<Project>, CommandError> {
    Ok(Page::first(list(&database, &git)?))
}

#[tauri::command]
pub fn projects_restore(
    database: State<Database>,
    git: State<GitService>,
) -> Result<Option<Project>, CommandError> {
    Ok(list(&database, &git)?.into_iter().next())
}

#[tauri::command]
pub fn projects_status(
    input: ProjectPath,
    git: State<GitService>,
) -> Result<GitStatus, CommandError> {
    git.status(Path::new(&input.path))
}

pub(crate) fn open(
    path: &str,
    database: &Database,
    git: &GitService,
) -> Result<Project, CommandError> {
    let path = Path::new(path)
        .canonicalize()
        .map_err(|error| CommandError::new("invalid_path", error.to_string()))?;
    if !path.is_dir() {
        return Err(CommandError::new(
            "invalid_path",
            "Project path must be a directory",
        ));
    }
    let path_text = path.to_string_lossy().into_owned();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&path_text)
        .to_owned();
    let connection = database.connect()?;
    let id = connection
        .query_row(
            "SELECT id FROM projects WHERE path = ?1",
            [&path_text],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    connection.execute(
        "INSERT INTO projects (id,name,path,last_opened_at,created_at,updated_at) VALUES (?1,?2,?3,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now')) ON CONFLICT(path) DO UPDATE SET name=excluded.name,last_opened_at=excluded.last_opened_at,updated_at=excluded.updated_at",
        params![id, name, path_text],
    )?;
    load(&connection, &id, git)
}

fn list(database: &Database, git: &GitService) -> Result<Vec<Project>, CommandError> {
    let connection = database.connect()?;
    let mut statement = connection.prepare("SELECT id,name,path,COALESCE(last_opened_at,created_at) FROM projects ORDER BY last_opened_at DESC LIMIT 100")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    rows.map(|row| {
        let (id, name, path, last_opened_at) = row?;
        let status = git.status(Path::new(&path)).unwrap_or(GitStatus {
            is_repository: false,
            branch: None,
            revision: None,
            dirty: false,
        });
        Ok(Project {
            id,
            name,
            path,
            last_opened_at,
            git: status,
        })
    })
    .collect()
}

fn load(
    connection: &rusqlite::Connection,
    id: &str,
    git: &GitService,
) -> Result<Project, CommandError> {
    let (name, path, last_opened_at) = connection.query_row(
        "SELECT name,path,COALESCE(last_opened_at,created_at) FROM projects WHERE id=?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let status = git.status(Path::new(&path))?;
    Ok(Project {
        id: id.into(),
        name,
        path,
        last_opened_at,
        git: status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    #[test]
    fn canonical_open_is_idempotent_and_recent() {
        let root = tempdir().unwrap();
        let db = Database::initialize(&root.path().join("db")).unwrap();
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let git = GitService::default();
        let first = open(project.to_str().unwrap(), &db, &git).unwrap();
        let second = open(project.join(".").to_str().unwrap(), &db, &git).unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(list(&db, &git).unwrap().len(), 1);
    }

    #[test]
    fn rejects_a_missing_project_path_with_a_stable_code() {
        let root = tempdir().unwrap();
        let db = Database::initialize(&root.path().join("db")).unwrap();
        let error = open(
            root.path().join("missing").to_str().unwrap(),
            &db,
            &GitService::default(),
        )
        .unwrap_err();
        assert_eq!(error.code, "invalid_path");
    }
}
