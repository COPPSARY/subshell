use serde::Serialize;
use tauri::State;

use crate::{contracts::CommandError, platform::database::Database};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    status: &'static str,
    schema_version: u32,
}

#[tauri::command]
pub fn health_status(database: State<'_, Database>) -> Result<Health, CommandError> {
    health(&database)
}

fn health(database: &Database) -> Result<Health, CommandError> {
    Ok(Health {
        status: "ok",
        schema_version: database.schema_version()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn returns_the_wire_contract_with_the_current_schema() {
        let directory = tempdir().unwrap();
        let database = Database::initialize(&directory.path().join("subshell.sqlite3")).unwrap();

        let response = health(&database).unwrap();

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({ "status": "ok", "schemaVersion": 12 })
        );
    }

    #[test]
    fn returns_a_structured_storage_error() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("subshell.sqlite3");
        let database = Database::initialize(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();

        let error = health(&database).unwrap_err();
        let value = serde_json::to_value(error).unwrap();

        assert_eq!(value["code"], "storage_unavailable");
        assert_eq!(value["retryable"], true);
        assert!(value["message"].as_str().unwrap().contains("database"));
    }
}
