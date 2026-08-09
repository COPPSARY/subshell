use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    contracts::{CommandError, Page},
    platform::database::Database,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionItem {
    pub key: String,
    pub reason: String,
    pub project_id: String,
    pub task_id: String,
    pub run_id: Option<String>,
    pub approval_request_id: Option<String>,
    pub title: String,
    pub detail: String,
    pub state_fingerprint: String,
    pub acknowledged: bool,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInput {
    pub project_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemInput {
    pub key: String,
    pub state_fingerprint: String,
}

#[tauri::command]
pub fn attention_list(
    input: ProjectInput,
    database: State<Database>,
) -> Result<Page<AttentionItem>, CommandError> {
    Ok(Page::first(list(&database, &input.project_id)?))
}

#[tauri::command]
pub fn attention_acknowledge(
    input: ItemInput,
    database: State<Database>,
) -> Result<(), CommandError> {
    acknowledge(&database, &input)
}

fn acknowledge(database: &Database, input: &ItemInput) -> Result<(), CommandError> {
    database.connect()?.execute(
        "INSERT INTO attention_acknowledgements(item_key,state_fingerprint,acknowledged_at) VALUES(?1,?2,strftime('%Y-%m-%dT%H:%M:%fZ','now')) ON CONFLICT(item_key) DO UPDATE SET state_fingerprint=excluded.state_fingerprint,acknowledged_at=excluded.acknowledged_at",
        params![input.key, input.state_fingerprint],
    )?;
    Ok(())
}

#[tauri::command]
pub fn attention_claim_notification(
    input: ItemInput,
    database: State<Database>,
) -> Result<bool, CommandError> {
    claim_notification(&database, &input)
}

fn claim_notification(database: &Database, input: &ItemInput) -> Result<bool, CommandError> {
    let connection = database.connect()?;
    let delivered: Option<String> = connection
        .query_row(
            "SELECT state_fingerprint FROM notification_deliveries WHERE item_key=?1",
            [&input.key],
            |row| row.get(0),
        )
        .optional()?;
    if delivered.as_deref() == Some(&input.state_fingerprint) {
        return Ok(false);
    }
    connection.execute(
        "INSERT INTO notification_deliveries(item_key,state_fingerprint,last_notified_at) VALUES(?1,?2,strftime('%Y-%m-%dT%H:%M:%fZ','now')) ON CONFLICT(item_key) DO UPDATE SET state_fingerprint=excluded.state_fingerprint,last_notified_at=excluded.last_notified_at",
        params![input.key, input.state_fingerprint],
    )?;
    Ok(true)
}

pub fn list(database: &Database, project_id: &str) -> Result<Vec<AttentionItem>, CommandError> {
    let connection = database.connect()?;
    let mut statement = connection.prepare(
        "SELECT item_key,reason,project_id,task_id,run_id,request_id,title,detail,fingerprint,created_at,
                EXISTS(SELECT 1 FROM attention_acknowledgements a WHERE a.item_key=items.item_key AND a.state_fingerprint=items.fingerprint)
         FROM (
           SELECT 'run:blocked:'||r.id item_key,'blocked' reason,t.project_id,r.task_id,r.id run_id,NULL request_id,t.title,COALESCE(r.waiting_reason,'Agent is waiting for input') detail,r.status||':'||r.updated_at fingerprint,r.updated_at created_at
           FROM agent_runs r JOIN tasks t ON t.id=r.task_id WHERE t.project_id=?1 AND r.status='waiting'
           UNION ALL
           SELECT 'run:failed:'||r.id,'failed',t.project_id,r.task_id,r.id,NULL,t.title,'Agent run failed',r.status||':'||r.updated_at,r.updated_at
           FROM agent_runs r JOIN tasks t ON t.id=r.task_id WHERE t.project_id=?1 AND r.status='failed'
           UNION ALL
           SELECT 'task:review:'||t.id,'completed_unreviewed',t.project_id,t.id,NULL,NULL,t.title,'All agent runs completed; review is ready',t.status||':'||t.updated_at,t.updated_at
           FROM tasks t WHERE t.project_id=?1 AND t.status='review'
           UNION ALL
           SELECT 'approval:'||a.id,'approval_waiting',a.project_id,a.task_id,a.agent_run_id,a.id,t.title,'Agent requested: '||a.action,a.status||':'||a.created_at,a.created_at
           FROM approval_requests a JOIN tasks t ON t.id=a.task_id WHERE a.project_id=?1 AND a.status='pending'
         ) items ORDER BY 11 ASC,created_at DESC LIMIT 500",
    )?;
    statement
        .query_map([project_id], |row| {
            Ok(AttentionItem {
                key: row.get(0)?,
                reason: row.get(1)?,
                project_id: row.get(2)?,
                task_id: row.get(3)?,
                run_id: row.get(4)?,
                approval_request_id: row.get(5)?,
                title: row.get(6)?,
                detail: row.get(7)?,
                state_fingerprint: row.get(8)?,
                created_at: row.get(9)?,
                acknowledged: row.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn derives_deduplicated_items_and_acknowledges_only_the_current_state() {
        let root = tempdir().unwrap();
        let database = Database::initialize(&root.path().join("db.sqlite3")).unwrap();
        let connection = database.connect().unwrap();
        connection.execute("INSERT INTO projects(id,name,path,created_at,updated_at) VALUES('p','P','/tmp/p','now','now')", []).unwrap();
        connection.execute("INSERT INTO tasks(id,project_id,title,status,base_branch,base_revision,created_at,updated_at) VALUES('t','p','Review me','review','main','abc','now','now')", []).unwrap();
        let item = list(&database, "p").unwrap().pop().unwrap();
        assert_eq!(item.reason, "completed_unreviewed");
        acknowledge(
            &database,
            &ItemInput {
                key: item.key.clone(),
                state_fingerprint: item.state_fingerprint.clone(),
            },
        )
        .unwrap();
        assert!(list(&database, "p").unwrap()[0].acknowledged);
        connection
            .execute("UPDATE tasks SET updated_at='later' WHERE id='t'", [])
            .unwrap();
        assert!(!list(&database, "p").unwrap()[0].acknowledged);
        assert!(
            claim_notification(
                &database,
                &ItemInput {
                    key: item.key.clone(),
                    state_fingerprint: "new".into()
                }
            )
            .unwrap()
        );
        assert!(
            !claim_notification(
                &database,
                &ItemInput {
                    key: item.key,
                    state_fingerprint: "new".into()
                }
            )
            .unwrap()
        );
    }
}
