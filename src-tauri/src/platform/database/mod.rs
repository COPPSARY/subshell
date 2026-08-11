mod migrations;

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::Connection;
use thiserror::Error;

use migrations::{apply, embedded};

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("failed to access application data: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("invalid migration: {0}")]
    InvalidMigration(String),
    #[error("database schema {found} is newer than this app supports ({supported})")]
    NewerSchema { found: u32, supported: u32 },
}

#[derive(Clone)]
pub struct Database {
    path: PathBuf,
}

impl Database {
    pub fn initialize(path: &Path) -> Result<Self, DatabaseError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut connection = open(path)?;
        apply(&mut connection, &embedded()?)?;
        Ok(Self { path: path.into() })
    }

    pub fn schema_version(&self) -> Result<u32, DatabaseError> {
        let connection = self.connect()?;
        Ok(connection.pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    pub fn connect(&self) -> Result<Connection, DatabaseError> {
        open(&self.path)
    }
}

fn open(path: &Path) -> Result<Connection, DatabaseError> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", true)?;
    Ok(connection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use tempfile::tempdir;

    const EXPECTED_TABLES: &[&str] = &[
        "agent_runs",
        "app_settings",
        "approval_requests",
        "attention_acknowledgements",
        "conflict_flags",
        "context_shares",
        "generic_provider_profiles",
        "merge_attempts",
        "notification_deliveries",
        "projects",
        "provider_accounts",
        "review_attempts",
        "review_records",
        "run_branches",
        "task_plan_assignments",
        "task_plans",
        "tasks",
        "timeline_events",
        "worktrees",
    ];

    #[test]
    fn initializes_complete_schema_and_enforces_ownership() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("subshell.sqlite3");
        Database::initialize(&path).unwrap();
        let connection = open(&path).unwrap();
        let mut statement = connection
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .unwrap();
        let tables: Vec<String> = statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(tables, EXPECTED_TABLES);
        assert_eq!(
            connection
                .pragma_query_value::<u32, _>(None, "foreign_keys", |row| row.get(0))
                .unwrap(),
            1
        );
        assert!(
            connection
                .execute(
                    "INSERT INTO tasks \
                     (id, project_id, title, status, base_branch, base_revision, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, 'task', 'main', 'abc', ?4, ?4)",
                    params!["task-1", "missing", "Orphan", "2026-01-01T00:00:00Z"],
                )
                .is_err()
        );
    }

    #[test]
    fn startup_is_idempotent_and_preserves_data() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("subshell.sqlite3");
        Database::initialize(&path).unwrap();
        let connection = open(&path).unwrap();
        connection
            .execute(
                "INSERT INTO projects (id, name, path, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![
                    "project-1",
                    "Fixture",
                    "/tmp/fixture",
                    "2026-01-01T00:00:00Z"
                ],
            )
            .unwrap();
        drop(connection);

        let database = Database::initialize(&path).unwrap();
        let connection = open(&path).unwrap();
        assert_eq!(
            connection
                .query_row::<u32, _, _>("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
                .unwrap(),
            1
        );
        assert_eq!(database.schema_version().unwrap(), 10);
    }

    #[test]
    fn schema_contains_no_secret_columns() {
        let directory = tempdir().unwrap();
        let database = Database::initialize(&directory.path().join("db.sqlite3")).unwrap();
        let connection = open(&database.path).unwrap();
        let schema: String = connection
            .query_row(
                "SELECT group_concat(sql, ' ') FROM sqlite_master WHERE type = 'table'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let schema = schema.to_lowercase();
        for forbidden in ["password", "token", "credential", "secret"] {
            assert!(
                !schema.contains(forbidden),
                "found forbidden column: {forbidden}"
            );
        }
    }
}
