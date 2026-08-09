use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;
use uuid::Uuid;

use crate::{
    contracts::CommandError,
    features::timeline::{self, EventRefs},
    platform::{
        database::Database,
        process::{ProcessSupervisor, read_log_tail},
    },
};

const MAX_SHARE_BYTES: usize = 64 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharePreviewInput {
    pub source_run_id: Option<String>,
    pub target_run_id: String,
    pub kind: String,
    pub content_reference: Option<String>,
    #[serde(default)]
    pub summary: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareDeliverInput {
    pub source_run_id: Option<String>,
    pub target_run_id: String,
    pub kind: String,
    pub content_reference: Option<String>,
    pub content: String,
    pub preview_sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharePreview {
    pub content: String,
    pub sha256: String,
    pub size_bytes: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextShare {
    pub id: String,
    pub task_id: String,
    pub source_run_id: Option<String>,
    pub target_run_id: String,
    pub kind: String,
    pub content_reference: Option<String>,
    pub content_summary: String,
    pub delivery_status: String,
    pub preview_sha256: String,
    pub size_bytes: usize,
    pub delivery_error: Option<String>,
    pub created_at: String,
}

#[tauri::command]
pub fn context_share_preview(
    input: SharePreviewInput,
    database: State<Database>,
) -> Result<SharePreview, CommandError> {
    preview(&database, &input)
}

#[tauri::command]
pub fn context_share_deliver(
    input: ShareDeliverInput,
    database: State<Database>,
    processes: State<ProcessSupervisor>,
) -> Result<ContextShare, CommandError> {
    deliver(&database, &processes, input)
}

fn preview(database: &Database, input: &SharePreviewInput) -> Result<SharePreview, CommandError> {
    validate_kind(&input.kind)?;
    let (task_id, _) = target(database, &input.target_run_id)?;
    let content = match input.kind.as_str() {
        "summary" => input.summary.trim().to_string(),
        "file" => {
            let source = source(database, input.source_run_id.as_deref(), &task_id)?;
            let reference = input.content_reference.as_deref().ok_or_else(|| {
                CommandError::new("missing_context_reference", "Choose a source file")
            })?;
            fs::read_to_string(safe_file(&source.worktree, reference)?).map_err(|error| {
                CommandError::new("context_source_unavailable", error.to_string())
            })?
        }
        "output_excerpt" => {
            let source = source(database, input.source_run_id.as_deref(), &task_id)?;
            let path = source.log.ok_or_else(|| {
                CommandError::new("output_unavailable", "The source Run has no output log")
            })?;
            let (bytes, _) = read_log_tail(&path, MAX_SHARE_BYTES)?;
            String::from_utf8_lossy(&bytes).into_owned()
        }
        _ => unreachable!(),
    };
    if content.is_empty() {
        return Err(CommandError::new(
            "empty_context_share",
            "Shared context cannot be empty",
        ));
    }
    if content.len() > MAX_SHARE_BYTES {
        return Err(CommandError::new(
            "context_share_too_large",
            "Shared context exceeds 64 KiB",
        ));
    }
    Ok(SharePreview {
        sha256: format!("{:x}", Sha256::digest(content.as_bytes())),
        size_bytes: content.len(),
        content,
    })
}

fn deliver(
    database: &Database,
    processes: &ProcessSupervisor,
    input: ShareDeliverInput,
) -> Result<ContextShare, CommandError> {
    validate_kind(&input.kind)?;
    if input.content.len() > MAX_SHARE_BYTES {
        return Err(CommandError::new(
            "context_share_too_large",
            "Shared context exceeds 64 KiB",
        ));
    }
    let actual = format!("{:x}", Sha256::digest(input.content.as_bytes()));
    if actual != input.preview_sha256 {
        return Err(CommandError::new(
            "context_preview_changed",
            "Preview the exact context again before delivering it",
        ));
    }
    let (task_id, target_status) = target(database, &input.target_run_id)?;
    if input.kind != "summary" {
        source(database, input.source_run_id.as_deref(), &task_id)?;
    }
    let id = Uuid::new_v4().to_string();
    let connection = database.connect()?;
    connection.execute(
        "INSERT INTO context_shares(id,task_id,source_agent_run_id,target_agent_run_id,kind,content_reference,content_summary,delivery_status,preview_sha256,size_bytes,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'pending',?8,?9,strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        params![id,task_id,input.source_run_id,input.target_run_id,input.kind,input.content_reference,summary(&input.content),actual,input.content.len() as i64],
    )?;
    let result = if matches!(target_status.as_str(), "preparing" | "running" | "waiting") {
        let message = format!(
            "\n\n[SubShell shared {}]\n{}\n[/SubShell shared context]\n",
            input.kind, input.content
        );
        processes.write_input(&input.target_run_id, message.as_bytes())
    } else {
        Err(CommandError::new(
            "target_run_inactive",
            "The target Run is no longer active",
        ))
    };
    let (status, error) = match result {
        Ok(()) => ("delivered", None),
        Err(error) => ("failed", Some(error.message)),
    };
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "UPDATE context_shares SET delivery_status=?1,delivery_error=?2,delivered_at=CASE WHEN ?1='delivered' THEN strftime('%Y-%m-%dT%H:%M:%fZ','now') END WHERE id=?3",
        params![status,error,id],
    )?;
    let project_id: String = transaction.query_row(
        "SELECT project_id FROM tasks WHERE id=?1",
        [&task_id],
        |row| row.get(0),
    )?;
    timeline::append(
        &transaction,
        EventRefs {
            project_id: &project_id,
            task_id: Some(&task_id),
            run_id: Some(&input.target_run_id),
            provider_id: None,
        },
        "context.shared",
        serde_json::json!({ "shareId": id, "kind": input.kind, "status": status, "sourceRunId": input.source_run_id }),
    )?;
    transaction.commit()?;
    get(database, &id)
}

fn validate_kind(kind: &str) -> Result<(), CommandError> {
    if matches!(kind, "file" | "output_excerpt" | "summary") {
        Ok(())
    } else {
        Err(CommandError::new(
            "invalid_context_kind",
            "Context kind is not supported",
        ))
    }
}

struct Source {
    worktree: PathBuf,
    log: Option<PathBuf>,
}
fn source(database: &Database, id: Option<&str>, task_id: &str) -> Result<Source, CommandError> {
    let id = id.ok_or_else(|| CommandError::new("source_run_required", "Choose a source Run"))?;
    database.connect()?.query_row(
        "SELECT w.path,r.raw_log_path FROM agent_runs r JOIN worktrees w ON w.agent_run_id=r.id WHERE r.id=?1 AND r.task_id=?2",
        params![id,task_id],
        |row| Ok(Source { worktree: PathBuf::from(row.get::<_, String>(0)?), log: row.get::<_, Option<String>>(1)?.map(PathBuf::from) }),
    ).optional()?.ok_or_else(|| CommandError::new("invalid_source_run", "Source and target Runs must belong to the same Task"))
}

fn target(database: &Database, id: &str) -> Result<(String, String), CommandError> {
    database
        .connect()?
        .query_row(
            "SELECT task_id,status FROM agent_runs WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| CommandError::new("run_not_found", "Target Run was not found"))
}

fn safe_file(root: &Path, relative: &str) -> Result<PathBuf, CommandError> {
    let root = root
        .canonicalize()
        .map_err(|error| CommandError::new("worktree_not_found", error.to_string()))?;
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|error| CommandError::new("context_source_unavailable", error.to_string()))?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err(CommandError::new(
            "invalid_context_source",
            "Context file leaves the source worktree",
        ));
    }
    Ok(path)
}

fn summary(content: &str) -> String {
    content
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(160)
        .collect()
}

fn get(database: &Database, id: &str) -> Result<ContextShare, CommandError> {
    database.connect()?.query_row(
        "SELECT id,task_id,source_agent_run_id,target_agent_run_id,kind,content_reference,content_summary,delivery_status,preview_sha256,size_bytes,delivery_error,created_at FROM context_shares WHERE id=?1", [id],
        |row| Ok(ContextShare { id:row.get(0)?,task_id:row.get(1)?,source_run_id:row.get(2)?,target_run_id:row.get(3)?,kind:row.get(4)?,content_reference:row.get(5)?,content_summary:row.get(6)?,delivery_status:row.get(7)?,preview_sha256:row.get(8)?,size_bytes:row.get::<_,i64>(9)? as usize,delivery_error:row.get(10)?,created_at:row.get(11)? })
    ).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::platform::process::{ProcessNotice, ProcessSpec};
    #[cfg(unix)]
    use std::{os::unix::fs::PermissionsExt, sync::Arc};
    use tempfile::tempdir;

    #[test]
    fn rejects_paths_outside_a_source_worktree() {
        let root = tempdir().unwrap();
        let worktree = root.path().join("worktree");
        fs::create_dir(&worktree).unwrap();
        fs::write(worktree.join("inside"), "ok").unwrap();
        fs::write(root.path().join("outside"), "no").unwrap();
        assert_eq!(
            safe_file(&worktree, "../outside").unwrap_err().code,
            "invalid_context_source"
        );
        assert_eq!(
            safe_file(&worktree, "inside").unwrap(),
            worktree.join("inside")
        );
    }

    #[test]
    #[cfg(unix)]
    fn previews_bounds_and_honestly_records_delivery() {
        let root = tempdir().unwrap();
        let database = Database::initialize(&root.path().join("db.sqlite3")).unwrap();
        let source_tree = root.path().join("source");
        let target_tree = root.path().join("target");
        fs::create_dir(&source_tree).unwrap();
        fs::create_dir(&target_tree).unwrap();
        fs::write(source_tree.join("note.txt"), "shared fact").unwrap();
        let connection = database.connect().unwrap();
        connection.execute("INSERT INTO projects(id,name,path,created_at,updated_at) VALUES('p','P','/tmp/p','now','now')", []).unwrap();
        connection.execute("INSERT INTO provider_accounts(id,provider_type,display_name,config_scope_path,status,created_at,updated_at) VALUES('provider','generic','P','/tmp/provider','active','now','now')", []).unwrap();
        connection.execute("INSERT INTO tasks(id,project_id,title,status,base_branch,base_revision,created_at,updated_at) VALUES('t','p','T','working','main','abc','now','now')", []).unwrap();
        for (id, status) in [("source", "succeeded"), ("target", "succeeded")] {
            connection.execute("INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,status,created_at,updated_at) VALUES(?1,'t','provider','I',?2,'now','now')", params![id,status]).unwrap();
        }
        connection.execute("INSERT INTO worktrees(id,agent_run_id,path,base_branch,base_revision,state,created_at) VALUES('ws','source',?1,'main','abc','active','now')", [source_tree.to_string_lossy().as_ref()]).unwrap();
        connection.execute("INSERT INTO worktrees(id,agent_run_id,path,base_branch,base_revision,state,created_at) VALUES('wt','target',?1,'main','abc','active','now')", [target_tree.to_string_lossy().as_ref()]).unwrap();
        let file_preview = preview(
            &database,
            &SharePreviewInput {
                source_run_id: Some("source".into()),
                target_run_id: "target".into(),
                kind: "file".into(),
                content_reference: Some("note.txt".into()),
                summary: String::new(),
            },
        )
        .unwrap();
        assert_eq!(file_preview.content, "shared fact");
        assert_eq!(
            preview(
                &database,
                &SharePreviewInput {
                    source_run_id: None,
                    target_run_id: "target".into(),
                    kind: "summary".into(),
                    content_reference: None,
                    summary: "x".repeat(MAX_SHARE_BYTES + 1)
                }
            )
            .unwrap_err()
            .code,
            "context_share_too_large"
        );
        let failed = deliver(
            &database,
            &ProcessSupervisor::default(),
            ShareDeliverInput {
                source_run_id: Some("source".into()),
                target_run_id: "target".into(),
                kind: "file".into(),
                content_reference: Some("note.txt".into()),
                content: file_preview.content,
                preview_sha256: file_preview.sha256,
            },
        )
        .unwrap();
        assert_eq!(failed.delivery_status, "failed");

        connection
            .execute(
                "UPDATE agent_runs SET status='running' WHERE id='target'",
                [],
            )
            .unwrap();
        let script = root.path().join("agent");
        fs::write(&script, "#!/bin/sh\nsleep 2\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let processes = ProcessSupervisor::default();
        processes
            .launch(
                "target".into(),
                ProcessSpec {
                    executable: script.to_string_lossy().into(),
                    arguments: vec![],
                    cwd: target_tree,
                    environment: vec![("PATH".into(), std::env::var("PATH").unwrap())],
                    log_path: root.path().join("output.log"),
                    stdin: None,
                },
                Arc::new(|_: ProcessNotice| {}),
            )
            .unwrap();
        let summary_preview = preview(
            &database,
            &SharePreviewInput {
                source_run_id: None,
                target_run_id: "target".into(),
                kind: "summary".into(),
                content_reference: None,
                summary: "coordinate here".into(),
            },
        )
        .unwrap();
        let delivered = deliver(
            &database,
            &processes,
            ShareDeliverInput {
                source_run_id: None,
                target_run_id: "target".into(),
                kind: "summary".into(),
                content_reference: None,
                content: summary_preview.content,
                preview_sha256: summary_preview.sha256,
            },
        )
        .unwrap();
        assert_eq!(delivered.delivery_status, "delivered");
        processes.stop("target").unwrap();
    }
}
