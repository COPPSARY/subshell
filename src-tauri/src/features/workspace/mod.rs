use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::{
    contracts::{CommandError, Page},
    features::{
        review,
        runs::{Run, RunService},
        timeline::{self, EventRefs},
    },
    platform::{
        database::Database,
        environment::{self, RuntimePaths},
        git::GitService,
        process,
    },
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInput {
    pub project_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyProfileInput {
    pub profile_id: String,
    pub task_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplorerAgent {
    pub run: Run,
    pub usage: process::ProcessUsage,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityInput {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplate {
    #[serde(default)]
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub provider_id: Option<String>,
    pub role: String,
    #[serde(default)]
    pub instruction: String,
    #[serde(default)]
    pub environment_files: Vec<String>,
    pub unit_limit: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentProfile {
    #[serde(default)]
    pub id: String,
    pub project_id: String,
    pub name: String,
    #[serde(default)]
    pub environment_files: Vec<String>,
    #[serde(default)]
    pub validation_commands: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchInput {
    pub project_id: Option<String>,
    pub query: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub kind: String,
    pub id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkInput {
    pub project_id: String,
    pub target_kind: String,
    pub target_id: String,
    #[serde(default)]
    pub label: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub event_id: Option<String>,
    pub label: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInput {
    pub run_id: String,
    pub kind: String,
    #[serde(default)]
    pub label: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub kind: String,
    pub label: String,
    pub base_revision: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueInput {
    pub attempt_id: String,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeQueueItem {
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub fingerprint: String,
    pub status: String,
    pub result_revision: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitInput {
    pub scope: String,
    pub id: String,
    pub unit_limit: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    pub active_agents: u64,
    pub pending_tasks: u64,
    pub blocked_tasks: u64,
    pub reviews: u64,
    pub failures: u64,
    pub queued_merges: u64,
    pub snapshots: u64,
    pub bookmarks: u64,
    pub total_reported_units: u64,
    pub attention_items: u64,
    pub recent_activity: Vec<DashboardActivity>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardActivity {
    pub event_type: String,
    pub task_id: Option<String>,
    pub created_at: String,
}

#[tauri::command]
pub fn workspace_templates_list(
    input: ProjectInput,
    database: State<Database>,
) -> Result<Page<AgentTemplate>, CommandError> {
    Ok(Page::first(list_templates(&database, &input.project_id)?))
}
#[tauri::command]
pub fn workspace_template_save(
    input: AgentTemplate,
    database: State<Database>,
) -> Result<AgentTemplate, CommandError> {
    save_template(&database, input)
}
#[tauri::command]
pub fn workspace_template_remove(
    input: EntityInput,
    database: State<Database>,
) -> Result<(), CommandError> {
    remove_owned(&database, "agent_templates", &input.id)
}
#[tauri::command]
pub fn workspace_profiles_list(
    input: ProjectInput,
    database: State<Database>,
) -> Result<Page<EnvironmentProfile>, CommandError> {
    Ok(Page::first(list_profiles(&database, &input.project_id)?))
}
#[tauri::command]
pub fn workspace_profile_save(
    input: EnvironmentProfile,
    database: State<Database>,
) -> Result<EnvironmentProfile, CommandError> {
    save_profile(&database, input)
}
#[tauri::command]
pub fn workspace_profile_remove(
    input: EntityInput,
    database: State<Database>,
) -> Result<(), CommandError> {
    remove_owned(&database, "environment_profiles", &input.id)
}
#[tauri::command]
pub fn workspace_profile_apply(
    input: ApplyProfileInput,
    database: State<Database>,
) -> Result<EnvironmentProfile, CommandError> {
    let profile = get_profile(&database, &input.profile_id)?;
    let task_project: String = database
        .connect()?
        .query_row(
            "SELECT project_id FROM tasks WHERE id=?1",
            [&input.task_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
    if task_project != profile.project_id {
        return Err(CommandError::new(
            "project_mismatch",
            "Environment profile belongs to another Project",
        ));
    }
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "UPDATE tasks SET validation_commands_json=?1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2",
        params![json(&profile.validation_commands)?, input.task_id],
    )?;
    timeline::append(
        &transaction,
        EventRefs {
            project_id: &profile.project_id,
            task_id: Some(&input.task_id),
            ..Default::default()
        },
        "environment.profile_applied",
        serde_json::json!({"profileId":profile.id,"name":profile.name}),
    )?;
    transaction.commit()?;
    Ok(profile)
}
#[tauri::command]
pub fn workspace_search(
    input: SearchInput,
    database: State<Database>,
    git: State<GitService>,
) -> Result<Page<SearchResult>, CommandError> {
    Ok(Page::first(search(&database, &git, input)?))
}
#[tauri::command]
pub fn workspace_bookmarks_list(
    input: ProjectInput,
    database: State<Database>,
) -> Result<Page<Bookmark>, CommandError> {
    Ok(Page::first(list_bookmarks(&database, &input.project_id)?))
}
#[tauri::command]
pub fn workspace_bookmark_toggle(
    input: BookmarkInput,
    database: State<Database>,
) -> Result<Option<Bookmark>, CommandError> {
    toggle_bookmark(&database, input)
}
#[tauri::command]
pub fn workspace_snapshots_list(
    input: ProjectInput,
    database: State<Database>,
) -> Result<Page<WorkspaceSnapshot>, CommandError> {
    Ok(Page::first(list_snapshots(&database, &input.project_id)?))
}
#[tauri::command]
pub fn workspace_snapshot_create(
    input: SnapshotInput,
    database: State<Database>,
    paths: State<RuntimePaths>,
    git: State<GitService>,
    runs: State<RunService>,
) -> Result<WorkspaceSnapshot, CommandError> {
    create_snapshot(&database, &paths, &git, &runs, input)
}
#[tauri::command]
pub fn workspace_snapshot_rollback(
    input: EntityInput,
    database: State<Database>,
    paths: State<RuntimePaths>,
    git: State<GitService>,
) -> Result<WorkspaceSnapshot, CommandError> {
    rollback(&database, &paths, &git, &input.id)
}
#[tauri::command]
pub fn workspace_merge_queue_list(
    input: ProjectInput,
    database: State<Database>,
) -> Result<Page<MergeQueueItem>, CommandError> {
    Ok(Page::first(list_queue(&database, &input.project_id)?))
}
#[tauri::command]
pub fn workspace_merge_queue_enqueue(
    input: QueueInput,
    database: State<Database>,
) -> Result<MergeQueueItem, CommandError> {
    enqueue_merge(&database, input)
}
#[tauri::command]
pub fn workspace_merge_queue_process(
    input: ProjectInput,
    database: State<Database>,
    paths: State<RuntimePaths>,
    git: State<GitService>,
) -> Result<MergeQueueItem, CommandError> {
    process_merge(&database, &paths, &git, &input.project_id)
}
#[tauri::command]
pub fn workspace_set_limit(
    input: LimitInput,
    database: State<Database>,
) -> Result<(), CommandError> {
    set_limit(&database, input)
}
#[tauri::command]
pub fn workspace_dashboard(
    input: ProjectInput,
    database: State<Database>,
) -> Result<Dashboard, CommandError> {
    dashboard(&database, &input.project_id)
}

#[tauri::command]
pub fn workspace_agents_list(
    input: ProjectInput,
    runs: State<RunService>,
) -> Result<Page<ExplorerAgent>, CommandError> {
    Ok(Page::first(
        runs.list_project(&input.project_id)?
            .into_iter()
            .map(|run| ExplorerAgent {
                usage: runs.usage(&run.id),
                run,
            })
            .collect(),
    ))
}

fn valid_role(role: &str) -> bool {
    matches!(
        role,
        "planner"
            | "executor"
            | "implementer"
            | "research"
            | "test"
            | "tester"
            | "reviewer"
            | "debugger"
    )
}
fn validate_name(name: &str) -> Result<&str, CommandError> {
    let value = name.trim();
    if value.is_empty() || value.len() > 80 {
        Err(CommandError::new(
            "invalid_name",
            "Name must be between 1 and 80 bytes",
        ))
    } else {
        Ok(value)
    }
}
fn json<T: Serialize>(value: &T) -> Result<String, CommandError> {
    serde_json::to_string(value)
        .map_err(|error| CommandError::new("invalid_json", error.to_string()))
}
fn strings(value: String) -> Vec<String> {
    serde_json::from_str(&value).unwrap_or_default()
}
fn project_path(database: &Database, project_id: &str) -> Result<PathBuf, CommandError> {
    database
        .connect()?
        .query_row(
            "SELECT path FROM projects WHERE id=?1",
            [project_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(PathBuf::from)
        .ok_or_else(|| CommandError::new("project_not_found", "Project was not found"))
}
fn validate_relative(values: &[String]) -> Result<(), CommandError> {
    if values.len() > 64
        || values.iter().any(|value| {
            value.is_empty()
                || Path::new(value).is_absolute()
                || Path::new(value).components().any(|part| {
                    matches!(
                        part,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                })
        })
    {
        Err(CommandError::new(
            "invalid_path",
            "Profile paths must stay inside the Project",
        ))
    } else {
        Ok(())
    }
}
fn limit_i64(value: Option<u64>) -> Result<Option<i64>, CommandError> {
    value
        .map(i64::try_from)
        .transpose()
        .map_err(|_| CommandError::new("invalid_unit_limit", "Usage limit is too large"))
}

fn save_template(
    database: &Database,
    mut input: AgentTemplate,
) -> Result<AgentTemplate, CommandError> {
    let name = validate_name(&input.name)?.to_owned();
    if !valid_role(&input.role) {
        return Err(CommandError::new(
            "invalid_run_role",
            "Template role is not supported",
        ));
    }
    validate_relative(&input.environment_files)?;
    if input.instruction.len() > 8_000 {
        return Err(CommandError::new(
            "instruction_too_large",
            "Template instruction exceeds 8,000 bytes",
        ));
    }
    project_path(database, &input.project_id)?;
    if let Some(provider_id) = input.provider_id.as_deref() {
        database
            .connect()?
            .query_row(
                "SELECT 1 FROM provider_accounts WHERE id=?1 AND removed_at IS NULL",
                [provider_id],
                |_| Ok(()),
            )
            .optional()?
            .ok_or_else(|| {
                CommandError::new("provider_not_found", "Template provider was not found")
            })?;
    }
    if input.id.is_empty() {
        input.id = Uuid::new_v4().to_string();
    }
    let changed = database.connect()?.execute("INSERT INTO agent_templates(id,project_id,name,provider_account_id,role,instruction,environment_files_json,unit_limit,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now')) ON CONFLICT(id) DO UPDATE SET name=excluded.name,provider_account_id=excluded.provider_account_id,role=excluded.role,instruction=excluded.instruction,environment_files_json=excluded.environment_files_json,unit_limit=excluded.unit_limit,updated_at=excluded.updated_at WHERE project_id=excluded.project_id", params![input.id,input.project_id,name,input.provider_id,input.role,input.instruction,json(&input.environment_files)?,limit_i64(input.unit_limit)?])?;
    if changed == 0 {
        return Err(CommandError::new(
            "project_mismatch",
            "Agent template belongs to another Project",
        ));
    }
    get_template(database, &input.id)
}
fn list_templates(
    database: &Database,
    project_id: &str,
) -> Result<Vec<AgentTemplate>, CommandError> {
    let connection = database.connect()?;
    let mut statement=connection.prepare("SELECT id,project_id,name,provider_account_id,role,instruction,environment_files_json,unit_limit FROM agent_templates WHERE project_id=?1 ORDER BY name")?;
    statement
        .query_map([project_id], template_row)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}
fn get_template(database: &Database, id: &str) -> Result<AgentTemplate, CommandError> {
    database.connect()?.query_row("SELECT id,project_id,name,provider_account_id,role,instruction,environment_files_json,unit_limit FROM agent_templates WHERE id=?1", [id], template_row).optional()?.ok_or_else(|| CommandError::new("template_not_found", "Agent template was not found"))
}
fn template_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentTemplate> {
    Ok(AgentTemplate {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        provider_id: row.get(3)?,
        role: row.get(4)?,
        instruction: row.get(5)?,
        environment_files: strings(row.get(6)?),
        unit_limit: row
            .get::<_, Option<i64>>(7)?
            .and_then(|v| u64::try_from(v).ok()),
    })
}

fn save_profile(
    database: &Database,
    mut input: EnvironmentProfile,
) -> Result<EnvironmentProfile, CommandError> {
    let name = validate_name(&input.name)?.to_owned();
    validate_relative(&input.environment_files)?;
    let path = project_path(database, &input.project_id)?;
    environment::validate_files(&path, &input.environment_files)?;
    if input.validation_commands.len() > 20
        || input
            .validation_commands
            .iter()
            .any(|command| command.trim().is_empty() || command.len() > 500)
    {
        return Err(CommandError::new(
            "invalid_validation_command",
            "Profiles support at most 20 bounded validation commands",
        ));
    }
    if input.id.is_empty() {
        input.id = Uuid::new_v4().to_string();
    }
    let changed = database.connect()?.execute("INSERT INTO environment_profiles(id,project_id,name,environment_files_json,validation_commands_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now')) ON CONFLICT(id) DO UPDATE SET name=excluded.name,environment_files_json=excluded.environment_files_json,validation_commands_json=excluded.validation_commands_json,updated_at=excluded.updated_at WHERE project_id=excluded.project_id",params![input.id,input.project_id,name,json(&input.environment_files)?,json(&input.validation_commands)?])?;
    if changed == 0 {
        return Err(CommandError::new(
            "project_mismatch",
            "Environment profile belongs to another Project",
        ));
    }
    get_profile(database, &input.id)
}
fn list_profiles(
    database: &Database,
    project_id: &str,
) -> Result<Vec<EnvironmentProfile>, CommandError> {
    let connection = database.connect()?;
    let mut statement=connection.prepare("SELECT id,project_id,name,environment_files_json,validation_commands_json FROM environment_profiles WHERE project_id=?1 ORDER BY name")?;
    statement
        .query_map([project_id], profile_row)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}
fn get_profile(database: &Database, id: &str) -> Result<EnvironmentProfile, CommandError> {
    database.connect()?.query_row("SELECT id,project_id,name,environment_files_json,validation_commands_json FROM environment_profiles WHERE id=?1",[id],profile_row).optional()?.ok_or_else(||CommandError::new("profile_not_found","Environment profile was not found"))
}
fn profile_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EnvironmentProfile> {
    Ok(EnvironmentProfile {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        environment_files: strings(row.get(3)?),
        validation_commands: strings(row.get(4)?),
    })
}
fn remove_owned(database: &Database, table: &str, id: &str) -> Result<(), CommandError> {
    let sql = match table {
        "agent_templates" => "DELETE FROM agent_templates WHERE id=?1",
        "environment_profiles" => "DELETE FROM environment_profiles WHERE id=?1",
        _ => {
            return Err(CommandError::new(
                "invalid_entity",
                "Entity type is not removable",
            ));
        }
    };
    if database.connect()?.execute(sql, [id])? == 0 {
        return Err(CommandError::new(
            "entity_not_found",
            "Saved item was not found",
        ));
    }
    Ok(())
}

fn search(
    database: &Database,
    git: &GitService,
    input: SearchInput,
) -> Result<Vec<SearchResult>, CommandError> {
    let query = input.query.trim().to_lowercase();
    if query.len() < 2 {
        return Ok(vec![]);
    }
    if query.chars().count() > 200 {
        return Err(CommandError::new(
            "search_too_long",
            "Search is limited to 200 characters",
        ));
    }
    let like = format!("%{query}%");
    let project_filter = input.project_id.as_deref();
    let connection = database.connect()?;
    let mut results = Vec::new();
    {
        let mut statement=connection.prepare("SELECT id,name,path FROM projects WHERE (?1 IS NULL OR id=?1) AND lower(name||' '||path) LIKE ?2 ORDER BY last_opened_at DESC LIMIT 20")?;
        for row in statement.query_map(params![project_filter, like], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })? {
            let (id, title, detail) = row?;
            results.push(SearchResult {
                kind: "project".into(),
                project_id: id.clone(),
                id,
                title,
                detail,
                task_id: None,
                run_id: None,
            });
        }
    }
    {
        let mut statement = connection.prepare(
            "SELECT id,name,path FROM projects WHERE (?1 IS NULL OR id=?1) ORDER BY last_opened_at DESC LIMIT 20",
        )?;
        let projects = statement
            .query_map([project_filter], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        // ponytail: scan recent registered repositories; add an indexed commit table only if profiling shows this is slow.
        for (project_id, project_name, path) in projects {
            let Ok(commits) = git.search_commits(Path::new(&path), &query, 20) else {
                continue;
            };
            for (id, subject) in commits {
                results.push(SearchResult {
                    kind: "commit".into(),
                    id: id.clone(),
                    project_id: project_id.clone(),
                    task_id: None,
                    run_id: None,
                    title: subject,
                    detail: format!("{project_name} · {}", &id[..id.len().min(12)]),
                });
            }
        }
    }
    {
        let mut statement=connection.prepare("SELECT id,project_id,title,description,base_revision FROM tasks WHERE (?1 IS NULL OR project_id=?1) AND lower(title||' '||description||' '||base_revision) LIKE ?2 ORDER BY updated_at DESC LIMIT 40")?;
        for row in statement.query_map(params![project_filter, like], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })? {
            let (id, project_id, title, description, revision) = row?;
            results.push(SearchResult {
                kind: "task".into(),
                project_id,
                task_id: Some(id.clone()),
                run_id: None,
                id,
                title,
                detail: if description.is_empty() {
                    revision
                } else {
                    description
                },
            });
        }
    }
    {
        let mut statement=connection.prepare("SELECT r.id,r.task_id,t.project_id,COALESCE(r.assignment_title,p.display_name),r.role||' · '||r.instruction,r.raw_log_path FROM agent_runs r JOIN tasks t ON t.id=r.task_id JOIN provider_accounts p ON p.id=r.provider_account_id WHERE (?1 IS NULL OR t.project_id=?1) ORDER BY r.updated_at DESC LIMIT 200")?;
        for row in statement.query_map([project_filter], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
            ))
        })? {
            let (id, task_id, project_id, title, detail, log) = row?;
            if format!("{title} {detail}").to_lowercase().contains(&query) {
                results.push(SearchResult {
                    kind: "agent".into(),
                    project_id: project_id.clone(),
                    task_id: Some(task_id.clone()),
                    run_id: Some(id.clone()),
                    id: id.clone(),
                    title: title.clone(),
                    detail: detail.clone(),
                });
            }
            if let Some(path) = log
                && let Ok((bytes, _)) = process::read_log_tail(Path::new(&path), 64 * 1024)
            {
                let output = String::from_utf8_lossy(&bytes);
                let normalized = output.to_lowercase();
                if let Some(index) = normalized.find(&query) {
                    let start = normalized[..index].chars().count().saturating_sub(80);
                    let detail = normalized
                        .chars()
                        .skip(start)
                        .take(query.chars().count() + 160)
                        .collect::<String>();
                    results.push(SearchResult {
                        kind: "log".into(),
                        project_id,
                        task_id: Some(task_id),
                        run_id: Some(id.clone()),
                        id,
                        title,
                        detail: detail.replace('\n', " "),
                    });
                }
            }
        }
    }
    {
        let mut statement=connection.prepare("SELECT e.id,e.project_id,e.task_id,e.agent_run_id,e.event_type,e.payload_json FROM timeline_events e WHERE (?1 IS NULL OR e.project_id=?1) AND lower(e.event_type||' '||e.payload_json) LIKE ?2 ORDER BY e.created_at DESC LIMIT 40")?;
        for row in statement.query_map(params![project_filter, like], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })? {
            let (id, project_id, task_id, run_id, title, detail) = row?;
            results.push(SearchResult {
                kind: "event".into(),
                id,
                project_id,
                task_id,
                run_id,
                title,
                detail,
            });
        }
    }
    {
        let mut statement=connection.prepare("SELECT a.id,t.project_id,t.id,'Review #'||a.attempt_number,a.decision||' · '||a.base_revision||' · '||COALESCE(a.feedback,'') FROM review_attempts a JOIN review_records record ON record.id=a.review_record_id JOIN tasks t ON t.id=record.task_id WHERE (?1 IS NULL OR t.project_id=?1) AND lower(a.decision||' '||a.base_revision||' '||a.input_fingerprint||' '||COALESCE(a.feedback,'')) LIKE ?2 ORDER BY a.created_at DESC LIMIT 40")?;
        for row in statement.query_map(params![project_filter, like], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })? {
            let (id, project_id, task_id, title, detail) = row?;
            results.push(SearchResult {
                kind: "review".into(),
                id,
                project_id,
                task_id: Some(task_id),
                run_id: None,
                title,
                detail,
            });
        }
    }
    results.truncate(100);
    Ok(results)
}

fn toggle_bookmark(
    database: &Database,
    input: BookmarkInput,
) -> Result<Option<Bookmark>, CommandError> {
    let (task_id, run_id, event_id) = match input.target_kind.as_str() {
        "task" => {
            let project: String = database
                .connect()?
                .query_row(
                    "SELECT project_id FROM tasks WHERE id=?1",
                    [&input.target_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
            if project != input.project_id {
                return Err(CommandError::new(
                    "project_mismatch",
                    "Task belongs to another Project",
                ));
            }
            (Some(input.target_id.clone()), None, None)
        }
        "run" => {
            let(task,project):(String,String)=database.connect()?.query_row("SELECT r.task_id,t.project_id FROM agent_runs r JOIN tasks t ON t.id=r.task_id WHERE r.id=?1",[&input.target_id],|row|Ok((row.get(0)?,row.get(1)?))).optional()?.ok_or_else(||CommandError::new("run_not_found","Run was not found"))?;
            if project != input.project_id {
                return Err(CommandError::new(
                    "project_mismatch",
                    "Run belongs to another Project",
                ));
            }
            (Some(task), Some(input.target_id.clone()), None)
        }
        "event" => {
            let (project, task, run): (String, Option<String>, Option<String>) = database
                .connect()?
                .query_row(
                    "SELECT project_id,task_id,agent_run_id FROM timeline_events WHERE id=?1",
                    [&input.target_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?
                .ok_or_else(|| CommandError::new("event_not_found", "Event was not found"))?;
            if project != input.project_id {
                return Err(CommandError::new(
                    "project_mismatch",
                    "Event belongs to another Project",
                ));
            }
            (task, run, Some(input.target_id.clone()))
        }
        _ => {
            return Err(CommandError::new(
                "invalid_bookmark_target",
                "Bookmark target must be a task, run, or event",
            ));
        }
    };
    let connection = database.connect()?;
    let existing:Option<String>=connection.query_row("SELECT id FROM bookmarks WHERE project_id=?1 AND task_id IS ?2 AND agent_run_id IS ?3 AND timeline_event_id IS ?4",params![input.project_id,task_id,run_id,event_id],|row|row.get(0)).optional()?;
    if let Some(id) = existing {
        connection.execute("DELETE FROM bookmarks WHERE id=?1", [id])?;
        return Ok(None);
    }
    let id = Uuid::new_v4().to_string();
    connection.execute("INSERT INTO bookmarks(id,project_id,task_id,agent_run_id,timeline_event_id,label,created_at) VALUES(?1,?2,?3,?4,?5,?6,strftime('%Y-%m-%dT%H:%M:%fZ','now'))",params![id,input.project_id,task_id,run_id,event_id,input.label.trim()])?;
    Ok(Some(get_bookmark(database, &id)?))
}
fn list_bookmarks(database: &Database, project_id: &str) -> Result<Vec<Bookmark>, CommandError> {
    let connection = database.connect()?;
    let mut statement=connection.prepare("SELECT id,project_id,task_id,agent_run_id,timeline_event_id,label,created_at FROM bookmarks WHERE project_id=?1 ORDER BY created_at DESC")?;
    statement
        .query_map([project_id], bookmark_row)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}
fn get_bookmark(database: &Database, id: &str) -> Result<Bookmark, CommandError> {
    database.connect()?.query_row("SELECT id,project_id,task_id,agent_run_id,timeline_event_id,label,created_at FROM bookmarks WHERE id=?1",[id],bookmark_row).map_err(Into::into)
}
fn bookmark_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Bookmark> {
    Ok(Bookmark {
        id: row.get(0)?,
        project_id: row.get(1)?,
        task_id: row.get(2)?,
        run_id: row.get(3)?,
        event_id: row.get(4)?,
        label: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn create_snapshot(
    database: &Database,
    paths: &RuntimePaths,
    git: &GitService,
    runs: &RunService,
    input: SnapshotInput,
) -> Result<WorkspaceSnapshot, CommandError> {
    if !matches!(input.kind.as_str(), "checkpoint" | "snapshot") {
        return Err(CommandError::new(
            "invalid_snapshot_kind",
            "Snapshot kind is not supported",
        ));
    }
    if input.kind == "checkpoint" {
        runs.pause_for_checkpoint(&input.run_id)?;
    }
    save_snapshot(
        database,
        paths,
        git,
        &input.run_id,
        &input.kind,
        &input.label,
    )
}
fn save_snapshot(
    database: &Database,
    paths: &RuntimePaths,
    git: &GitService,
    run_id: &str,
    kind: &str,
    label: &str,
) -> Result<WorkspaceSnapshot, CommandError> {
    let(worktree,base,task_id,project_id):(String,String,String,String)=database.connect()?.query_row("SELECT w.path,w.base_revision,r.task_id,t.project_id FROM worktrees w JOIN agent_runs r ON r.id=w.agent_run_id JOIN tasks t ON t.id=r.task_id WHERE r.id=?1 AND w.state='active'",[run_id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional()?.ok_or_else(||CommandError::new("worktree_not_found","An active Run worktree was not found"))?;
    let root = paths.data_dir.join("worktrees");
    let worktree = PathBuf::from(worktree);
    if !worktree.starts_with(&root) {
        return Err(CommandError::new(
            "unsafe_worktree",
            "Snapshot worktree is outside SubShell storage",
        ));
    }
    let diff = git.exact_diff(&worktree, &base)?;
    let id = Uuid::new_v4().to_string();
    let directory = paths.data_dir.join("snapshots").join(&project_id);
    fs::create_dir_all(&directory).map_err(io_error)?;
    let patch = directory.join(format!("{id}.patch"));
    fs::write(&patch, diff.patch).map_err(io_error)?;
    let label = if label.trim().is_empty() {
        if kind == "checkpoint" {
            "Agent checkpoint"
        } else {
            "Workspace snapshot"
        }
    } else {
        label.trim()
    };
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    transaction.execute("INSERT INTO workspace_snapshots(id,project_id,task_id,agent_run_id,kind,label,base_revision,patch_path,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,strftime('%Y-%m-%dT%H:%M:%fZ','now'))",params![id,project_id,task_id,run_id,kind,label,base,patch.to_string_lossy()])?;
    timeline::append(
        &transaction,
        EventRefs {
            project_id: &project_id,
            task_id: Some(&task_id),
            run_id: Some(run_id),
            provider_id: None,
        },
        "snapshot.created",
        serde_json::json!({"snapshotId":id,"kind":kind,"label":label}),
    )?;
    transaction.commit()?;
    get_snapshot(database, &id)
}
fn rollback(
    database: &Database,
    paths: &RuntimePaths,
    git: &GitService,
    id: &str,
) -> Result<WorkspaceSnapshot, CommandError> {
    let target = get_snapshot(database, id)?;
    let run_id = target.run_id.as_deref().ok_or_else(|| {
        CommandError::new("snapshot_not_restorable", "Snapshot has no Run worktree")
    })?;
    let(status,worktree):(String,String)=database.connect()?.query_row("SELECT r.status,w.path FROM agent_runs r JOIN worktrees w ON w.agent_run_id=r.id WHERE r.id=?1 AND w.state='active'",[run_id],|row|Ok((row.get(0)?,row.get(1)?))).optional()?.ok_or_else(||CommandError::new("worktree_not_found","Snapshot worktree is no longer active"))?;
    if matches!(status.as_str(), "preparing" | "running" | "waiting") {
        return Err(CommandError::new(
            "run_active",
            "Pause the agent before rolling its worktree back",
        ));
    }
    save_snapshot(database, paths, git, run_id, "snapshot", "Before rollback")?;
    let patch_path: String = database.connect()?.query_row(
        "SELECT patch_path FROM workspace_snapshots WHERE id=?1",
        [id],
        |row| row.get(0),
    )?;
    let patch = fs::read(patch_path).map_err(io_error)?;
    git.restore_snapshot(Path::new(&worktree), &target.base_revision, &patch)?;
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    timeline::append(
        &transaction,
        EventRefs {
            project_id: &target.project_id,
            task_id: target.task_id.as_deref(),
            run_id: target.run_id.as_deref(),
            provider_id: None,
        },
        "snapshot.restored",
        serde_json::json!({"snapshotId":id}),
    )?;
    transaction.commit()?;
    Ok(target)
}
fn list_snapshots(
    database: &Database,
    project_id: &str,
) -> Result<Vec<WorkspaceSnapshot>, CommandError> {
    let connection = database.connect()?;
    let mut statement=connection.prepare("SELECT id,project_id,task_id,agent_run_id,kind,label,base_revision,created_at FROM workspace_snapshots WHERE project_id=?1 ORDER BY created_at DESC LIMIT 200")?;
    statement
        .query_map([project_id], snapshot_row)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}
fn get_snapshot(database: &Database, id: &str) -> Result<WorkspaceSnapshot, CommandError> {
    database.connect()?.query_row("SELECT id,project_id,task_id,agent_run_id,kind,label,base_revision,created_at FROM workspace_snapshots WHERE id=?1",[id],snapshot_row).optional()?.ok_or_else(||CommandError::new("snapshot_not_found","Snapshot was not found"))
}
fn snapshot_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceSnapshot> {
    Ok(WorkspaceSnapshot {
        id: row.get(0)?,
        project_id: row.get(1)?,
        task_id: row.get(2)?,
        run_id: row.get(3)?,
        kind: row.get(4)?,
        label: row.get(5)?,
        base_revision: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn enqueue_merge(database: &Database, input: QueueInput) -> Result<MergeQueueItem, CommandError> {
    let (task_id, project_id) =
        review::queue_candidate(database, &input.attempt_id, &input.fingerprint)?;
    let id = Uuid::new_v4().to_string();
    database.connect()?.execute("INSERT INTO merge_queue(id,project_id,task_id,review_attempt_id,review_fingerprint,created_at) VALUES(?1,?2,?3,?4,?5,strftime('%Y-%m-%dT%H:%M:%fZ','now')) ON CONFLICT(review_attempt_id) DO NOTHING",params![id,project_id,task_id,input.attempt_id,input.fingerprint])?;
    let queued_id = database.connect()?.query_row(
        "SELECT id FROM merge_queue WHERE review_attempt_id=?1",
        [input.attempt_id],
        |row| row.get::<_, String>(0),
    )?;
    get_queue(database, &queued_id)
}
fn process_merge(
    database: &Database,
    paths: &RuntimePaths,
    git: &GitService,
    project_id: &str,
) -> Result<MergeQueueItem, CommandError> {
    let item = claim_next_merge(database, project_id)?.ok_or_else(|| {
        CommandError::new(
            "merge_queue_empty",
            "No approved review is waiting to merge",
        )
    })?;
    let result = review::merge(
        review::MergeInput {
            attempt_id: item.attempt_id.clone(),
            fingerprint: item.fingerprint.clone(),
        },
        database,
        paths,
        git,
    );
    match result {
        Ok(revision) => {
            database.connect()?.execute("UPDATE merge_queue SET status='succeeded',result_revision=?1,completed_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2",params![revision,item.id])?;
        }
        Err(error) => {
            database.connect()?.execute("UPDATE merge_queue SET status='failed',error_code=?1,error_message=?2,completed_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?3",params![error.code,error.message,item.id])?;
        }
    }
    get_queue(database, &item.id)
}

fn claim_next_merge(
    database: &Database,
    project_id: &str,
) -> Result<Option<MergeQueueItem>, CommandError> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let item = transaction.query_row("SELECT id,project_id,task_id,review_attempt_id,review_fingerprint,status,result_revision,error_message,created_at FROM merge_queue WHERE project_id=?1 AND status='queued' ORDER BY created_at,id LIMIT 1",[project_id],queue_row).optional()?;
    if let Some(item) = &item {
        transaction.execute(
            "UPDATE merge_queue SET status='running' WHERE id=?1 AND status='queued'",
            [&item.id],
        )?;
    }
    transaction.commit()?;
    Ok(item)
}
fn list_queue(database: &Database, project_id: &str) -> Result<Vec<MergeQueueItem>, CommandError> {
    let connection = database.connect()?;
    let mut statement=connection.prepare("SELECT id,project_id,task_id,review_attempt_id,review_fingerprint,status,result_revision,error_message,created_at FROM merge_queue WHERE project_id=?1 ORDER BY created_at")?;
    statement
        .query_map([project_id], queue_row)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}
fn get_queue(database: &Database, id: &str) -> Result<MergeQueueItem, CommandError> {
    database.connect()?.query_row("SELECT id,project_id,task_id,review_attempt_id,review_fingerprint,status,result_revision,error_message,created_at FROM merge_queue WHERE id=?1",[id],queue_row).map_err(Into::into)
}
fn queue_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MergeQueueItem> {
    Ok(MergeQueueItem {
        id: row.get(0)?,
        project_id: row.get(1)?,
        task_id: row.get(2)?,
        attempt_id: row.get(3)?,
        fingerprint: row.get(4)?,
        status: row.get(5)?,
        result_revision: row.get(6)?,
        error: row.get(7)?,
        created_at: row.get(8)?,
    })
}

fn set_limit(database: &Database, input: LimitInput) -> Result<(), CommandError> {
    let value = limit_i64(input.unit_limit)?;
    let (sql, event_project) = match input.scope.as_str() {
        "project" => (
            "UPDATE projects SET unit_limit=?1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2",
            input.id.clone(),
        ),
        "task" => {
            let project = database.connect()?.query_row(
                "SELECT project_id FROM tasks WHERE id=?1",
                [&input.id],
                |row| row.get(0),
            )?;
            (
                "UPDATE tasks SET unit_limit=?1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2",
                project,
            )
        }
        "agent" => {
            let project=database.connect()?.query_row("SELECT t.project_id FROM agent_runs r JOIN tasks t ON t.id=r.task_id WHERE r.id=?1",[&input.id],|row|row.get(0))?;
            (
                "UPDATE agent_runs SET unit_limit=?1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2",
                project,
            )
        }
        _ => {
            return Err(CommandError::new(
                "invalid_limit_scope",
                "Limit scope must be project, task, or agent",
            ));
        }
    };
    if database.connect()?.execute(sql, params![value, input.id])? == 0 {
        return Err(CommandError::new(
            "entity_not_found",
            "Usage limit target was not found",
        ));
    }
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    timeline::append(
        &transaction,
        EventRefs {
            project_id: &event_project,
            ..Default::default()
        },
        "budget.configured",
        serde_json::json!({"scope":input.scope,"id":input.id,"limit":value}),
    )?;
    transaction.commit()?;
    Ok(())
}
fn dashboard(database: &Database, project_id: &str) -> Result<Dashboard, CommandError> {
    let connection = database.connect()?;
    let count = |sql: &str| {
        connection
            .query_row(sql, [project_id], |row| row.get::<_, i64>(0))
            .map(|value| value.max(0) as u64)
    };
    let recent_activity = {
        let mut statement = connection.prepare("SELECT event_type,task_id,created_at FROM timeline_events WHERE project_id=?1 ORDER BY created_at DESC,id DESC LIMIT 8")?;
        statement
            .query_map([project_id], |row| {
                Ok(DashboardActivity {
                    event_type: row.get(0)?,
                    task_id: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    Ok(Dashboard {
        active_agents: count(
            "SELECT COUNT(*) FROM agent_runs r JOIN tasks t ON t.id=r.task_id WHERE t.project_id=?1 AND r.status IN('queued','preparing','running','waiting')",
        )?,
        pending_tasks: count(
            "SELECT COUNT(*) FROM tasks WHERE project_id=?1 AND archived_at IS NULL",
        )?,
        blocked_tasks: count(
            "SELECT COUNT(*) FROM tasks WHERE project_id=?1 AND status IN('waiting','failed')",
        )?,
        reviews: count(
            "SELECT COUNT(*) FROM tasks WHERE project_id=?1 AND status IN('review','approved')",
        )?,
        failures: count(
            "SELECT COUNT(*) FROM agent_runs r JOIN tasks t ON t.id=r.task_id WHERE t.project_id=?1 AND r.status='failed'",
        )?,
        queued_merges: count(
            "SELECT COUNT(*) FROM merge_queue WHERE project_id=?1 AND status IN('queued','running')",
        )?,
        snapshots: count("SELECT COUNT(*) FROM workspace_snapshots WHERE project_id=?1")?,
        bookmarks: count("SELECT COUNT(*) FROM bookmarks WHERE project_id=?1")?,
        total_reported_units: count(
            "SELECT COALESCE(SUM(COALESCE(r.reported_input_units,0)+COALESCE(r.reported_output_units,0)),0) FROM agent_runs r JOIN tasks t ON t.id=r.task_id WHERE t.project_id=?1",
        )?,
        attention_items: count(
            "SELECT (SELECT COUNT(*) FROM tasks WHERE project_id=?1 AND status IN('waiting','failed'))+(SELECT COUNT(*) FROM approval_requests WHERE project_id=?1 AND status='pending')",
        )?,
        recent_activity,
    })
}
fn io_error(error: std::io::Error) -> CommandError {
    CommandError::new("filesystem_error", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;
    #[test]
    fn saves_reusable_configuration_and_searches_persistent_logs() {
        let root = tempdir().unwrap();
        let repository = root.path().join("repo");
        fs::create_dir(&repository).unwrap();
        for args in [
            ["init", "-q"].as_slice(),
            ["config", "user.email", "test@example.com"].as_slice(),
            ["config", "user.name", "Test"].as_slice(),
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
        fs::write(repository.join("README.md"), "base").unwrap();
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
        let database = Database::initialize(&root.path().join("db")).unwrap();
        let connection = database.connect().unwrap();
        connection.execute("INSERT INTO projects(id,name,path,created_at,updated_at) VALUES('p','Workspace',?1,'now','now')",[repository.to_string_lossy()]).unwrap();
        connection.execute("INSERT INTO provider_accounts(id,provider_type,display_name,config_scope_path,status,created_at,updated_at) VALUES('provider','generic','Codex','/tmp/provider','active','now','now')",[]).unwrap();
        connection.execute("INSERT INTO tasks(id,project_id,title,description,status,base_branch,base_revision,created_at,updated_at) VALUES('task','p','Fix search','Find needle','working','main','abc','now','now')",[]).unwrap();
        let log = root.path().join("agent.log");
        fs::write(&log, format!("é{}needle output", "x".repeat(300))).unwrap();
        connection.execute("INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,status,raw_log_path,created_at,updated_at) VALUES('run','task','provider','Inspect','failed',?1,'now','now')",[log.to_string_lossy()]).unwrap();
        drop(connection);
        let template = save_template(
            &database,
            AgentTemplate {
                id: String::new(),
                project_id: "p".into(),
                name: "Reviewer".into(),
                provider_id: Some("provider".into()),
                role: "reviewer".into(),
                instruction: "Review".into(),
                environment_files: vec![],
                unit_limit: Some(100),
            },
        )
        .unwrap();
        assert_eq!(template.name, "Reviewer");
        let other = root.path().join("other");
        fs::create_dir(&other).unwrap();
        database.connect().unwrap().execute("INSERT INTO projects(id,name,path,created_at,updated_at) VALUES('other','Other',?1,'now','now')",[other.to_string_lossy()]).unwrap();
        assert_eq!(
            save_template(
                &database,
                AgentTemplate {
                    project_id: "other".into(),
                    name: "Hijack".into(),
                    ..template.clone()
                }
            )
            .unwrap_err()
            .code,
            "project_mismatch"
        );
        let profile = save_profile(
            &database,
            EnvironmentProfile {
                id: String::new(),
                project_id: "p".into(),
                name: "Checks".into(),
                environment_files: vec![],
                validation_commands: vec!["true".into()],
            },
        )
        .unwrap();
        assert_eq!(
            save_profile(
                &database,
                EnvironmentProfile {
                    project_id: "other".into(),
                    name: "Hijack".into(),
                    ..profile
                }
            )
            .unwrap_err()
            .code,
            "project_mismatch"
        );
        let results = search(
            &database,
            &GitService::default(),
            SearchInput {
                project_id: Some("p".into()),
                query: "needle".into(),
            },
        )
        .unwrap();
        assert!(
            results
                .iter()
                .any(|result| result.kind == "log" && result.run_id.as_deref() == Some("run"))
        );
        assert!(
            results
                .iter()
                .find(|result| result.kind == "log")
                .unwrap()
                .detail
                .chars()
                .count()
                <= 166
        );
        let commits = search(
            &database,
            &GitService::default(),
            SearchInput {
                project_id: Some("p".into()),
                query: "base".into(),
            },
        )
        .unwrap();
        assert!(commits.iter().any(|result| result.kind == "commit"));

        let connection = database.connect().unwrap();
        connection.execute("INSERT INTO review_records(id,task_id,created_at,updated_at) VALUES('record','task','now','now')", []).unwrap();
        for index in 1..=2 {
            connection.execute("INSERT INTO review_attempts(id,review_record_id,attempt_number,base_revision,input_fingerprint,combined_diff_path,created_at) VALUES(?1,'record',?2,'abc',?3,'/tmp/diff','now')", params![format!("attempt-{index}"),index,format!("fingerprint-{index}")]).unwrap();
            connection.execute("INSERT INTO merge_queue(id,project_id,task_id,review_attempt_id,review_fingerprint,created_at) VALUES(?1,'p','task',?2,?3,'now')",params![format!("queue-{index}"),format!("attempt-{index}"),format!("fingerprint-{index}")]).unwrap();
        }
        drop(connection);
        let claims = [database.clone(), database.clone()].map(|database| {
            std::thread::spawn(move || claim_next_merge(&database, "p").unwrap().unwrap().id)
        });
        let mut ids = claims
            .into_iter()
            .map(|claim| claim.join().unwrap())
            .collect::<Vec<_>>();
        ids.sort();
        assert_eq!(ids, vec!["queue-1", "queue-2"]);
    }
}
