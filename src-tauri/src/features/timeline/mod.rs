use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;
use uuid::Uuid;

use crate::{
    contracts::{CommandError, Page},
    platform::database::Database,
};

#[derive(Clone, Debug, Default)]
pub struct EventRefs<'a> {
    pub project_id: &'a str,
    pub task_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub provider_id: Option<&'a str>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineQuery {
    pub project_id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub provider_id: Option<String>,
    pub event_type: Option<String>,
    pub after_sequence: Option<i64>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEvent {
    pub id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub provider_id: Option<String>,
    pub sequence: i64,
    pub event_type: String,
    pub payload: Value,
    pub created_at: String,
}

#[tauri::command]
pub fn timeline_list(
    input: TimelineQuery,
    database: State<Database>,
) -> Result<Page<TimelineEvent>, CommandError> {
    Ok(Page::first(list(&database, &input)?))
}

pub fn append(
    connection: &Connection,
    refs: EventRefs<'_>,
    event_type: &str,
    payload: Value,
) -> Result<TimelineEvent, CommandError> {
    let sequence: i64 = connection.query_row(
        "SELECT COALESCE(MAX(sequence),0)+1 FROM timeline_events WHERE project_id=?1",
        [refs.project_id],
        |row| row.get(0),
    )?;
    let id = Uuid::new_v4().to_string();
    connection.execute(
        "INSERT INTO timeline_events(id,project_id,task_id,agent_run_id,provider_account_id,sequence,event_type,payload_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        params![id, refs.project_id, refs.task_id, refs.run_id, refs.provider_id, sequence, event_type, payload.to_string()],
    )?;
    connection.query_row(
        "SELECT id,project_id,task_id,agent_run_id,provider_account_id,sequence,event_type,payload_json,created_at FROM timeline_events WHERE id=?1",
        [&id],
        row_to_event,
    ).map_err(Into::into)
}

fn list(database: &Database, query: &TimelineQuery) -> Result<Vec<TimelineEvent>, CommandError> {
    let connection = database.connect()?;
    let limit = query.limit.unwrap_or(100).clamp(1, 500) as i64;
    let mut statement = connection.prepare(
        "SELECT id,project_id,task_id,agent_run_id,provider_account_id,sequence,event_type,payload_json,created_at FROM timeline_events
         WHERE project_id=?1
           AND (?2 IS NULL OR task_id=?2)
           AND (?3 IS NULL OR agent_run_id=?3)
           AND (?4 IS NULL OR provider_account_id=?4)
           AND (?5 IS NULL OR event_type=?5)
           AND (?6 IS NULL OR sequence>?6)
         ORDER BY sequence DESC LIMIT ?7",
    )?;
    statement
        .query_map(
            params![
                query.project_id,
                query.task_id,
                query.run_id,
                query.provider_id,
                query.event_type,
                query.after_sequence,
                limit
            ],
            row_to_event,
        )?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<TimelineEvent> {
    let payload: String = row.get(7)?;
    Ok(TimelineEvent {
        id: row.get(0)?,
        project_id: row.get(1)?,
        task_id: row.get(2)?,
        run_id: row.get(3)?,
        provider_id: row.get(4)?,
        sequence: row.get(5)?,
        event_type: row.get(6)?,
        payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
        created_at: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn assigns_project_sequences_and_filters_without_timestamp_ordering() {
        let root = tempdir().unwrap();
        let database = Database::initialize(&root.path().join("db.sqlite3")).unwrap();
        let connection = database.connect().unwrap();
        connection.execute("INSERT INTO projects(id,name,path,created_at,updated_at) VALUES('p','P','/tmp/p','now','now')", []).unwrap();
        append(
            &connection,
            EventRefs {
                project_id: "p",
                ..Default::default()
            },
            "project.opened",
            serde_json::json!({}),
        )
        .unwrap();
        append(
            &connection,
            EventRefs {
                project_id: "p",
                task_id: Some("missing"),
                ..Default::default()
            },
            "task.changed",
            serde_json::json!({}),
        )
        .unwrap_err();
        append(
            &connection,
            EventRefs {
                project_id: "p",
                ..Default::default()
            },
            "project.refreshed",
            serde_json::json!({}),
        )
        .unwrap();
        let events = list(
            &database,
            &TimelineQuery {
                project_id: "p".into(),
                task_id: None,
                run_id: None,
                provider_id: None,
                event_type: None,
                after_sequence: None,
                limit: None,
            },
        )
        .unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
            [2, 1]
        );
        assert_eq!(events[0].event_type, "project.refreshed");
    }
}
