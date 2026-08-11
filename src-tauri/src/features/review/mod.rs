use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;
use uuid::Uuid;

use crate::{
    contracts::CommandError,
    features::{
        tasks,
        timeline::{self, EventRefs},
    },
    platform::{database::Database, environment::RuntimePaths, git::GitService},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInput {
    pub task_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDecisionInput {
    pub attempt_id: String,
    pub fingerprint: String,
    #[serde(default)]
    pub feedback: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeInput {
    pub attempt_id: String,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub id: String,
    pub task_id: String,
    pub attempt_number: u32,
    pub base_revision: String,
    pub fingerprint: String,
    pub decision: String,
    pub feedback: Option<String>,
    pub combined_diff_path: String,
    pub combined_patch: String,
    pub runs: Vec<RunSnapshot>,
    pub conflicts: Vec<ConflictFlag>,
    pub validation_evidence: Vec<ValidationEvidence>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSnapshot {
    pub run_id: String,
    pub title: String,
    pub provider_name: String,
    pub instruction: String,
    pub explanation: Option<String>,
    pub files: Vec<String>,
    pub patch_path: String,
    pub patch_sha256: String,
    pub context_sha256: String,
    pub context_manifest: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationEvidence {
    pub run_id: String,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictFlag {
    pub category: String,
    pub run_ids: Vec<String>,
    pub evidence: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSnapshot {
    task_id: String,
    base_revision: String,
    runs: Vec<RunSnapshot>,
}

struct CapturedRun {
    snapshot: RunSnapshot,
    patch: Vec<u8>,
}

pub(crate) struct VerifiedReview {
    pub review: Review,
    pub project_path: PathBuf,
}

#[tauri::command]
pub fn review_get(
    input: TaskInput,
    database: State<Database>,
    paths: State<RuntimePaths>,
    git: State<GitService>,
) -> Result<Review, CommandError> {
    get_or_create(&input.task_id, &database, &paths, &git)
}

#[tauri::command]
pub fn review_approve(
    input: ReviewDecisionInput,
    database: State<Database>,
    git: State<GitService>,
) -> Result<Review, CommandError> {
    decide(input, "approved", &database, &git)
}

#[tauri::command]
pub fn review_send_back(
    input: ReviewDecisionInput,
    database: State<Database>,
    git: State<GitService>,
) -> Result<Review, CommandError> {
    if input.feedback.trim().is_empty() {
        return Err(CommandError::new(
            "feedback_required",
            "Tell the agents what must change",
        ));
    }
    decide(input, "sent_back", &database, &git)
}

#[tauri::command]
pub fn review_merge(
    input: MergeInput,
    database: State<Database>,
    paths: State<RuntimePaths>,
    git: State<GitService>,
) -> Result<String, CommandError> {
    merge(input, &database, &paths, &git)
}

pub(crate) fn get_or_create(
    task_id: &str,
    database: &Database,
    paths: &RuntimePaths,
    git: &GitService,
) -> Result<Review, CommandError> {
    let task = reviewable_task(database, task_id)?;
    let evidence = validation_evidence(database, task_id)?;
    let captured = capture_runs(database, git, &task)?;
    let fingerprint = fingerprint(&task.base_revision, &captured, &evidence)?;
    if let Some(review) = latest_review(database, task_id)?
        && (review.decision == "approved"
            || (review.decision == "pending" && review.fingerprint == fingerprint))
    {
        return Ok(review);
    }

    let attempt_number = latest_attempt_number(database, task_id)? + 1;
    let directory = paths
        .data_dir
        .join("reviews")
        .join(task_id)
        .join(format!("attempt-{attempt_number}"));
    fs::create_dir_all(&directory).map_err(io_error)?;
    let mut combined = Vec::new();
    let mut snapshots = Vec::new();
    for mut run in captured {
        let patch_path = directory.join(format!("{}.patch", run.snapshot.run_id));
        fs::write(&patch_path, &run.patch).map_err(io_error)?;
        run.snapshot.patch_path = patch_path.to_string_lossy().into_owned();
        combined.extend_from_slice(&run.patch);
        if !combined.ends_with(b"\n") {
            combined.push(b'\n');
        }
        snapshots.push(run.snapshot);
    }
    let combined_path = directory.join("combined.diff");
    fs::write(&combined_path, combined).map_err(io_error)?;
    let conflicts = conflict_flags(&snapshots);
    let stored = StoredSnapshot {
        task_id: task.id.clone(),
        base_revision: task.base_revision.clone(),
        runs: snapshots,
    };
    let id = Uuid::new_v4().to_string();
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    let record_id: String = transaction
        .query_row(
            "SELECT id FROM review_records WHERE task_id=?1",
            [task_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    transaction.execute(
        "INSERT OR IGNORE INTO review_records(id,task_id,created_at,updated_at) VALUES(?1,?2,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        params![record_id, task_id],
    )?;
    transaction.execute(
        "INSERT INTO review_attempts(id,review_record_id,attempt_number,base_revision,input_fingerprint,combined_diff_path,snapshot_json,validation_evidence_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        params![id, record_id, attempt_number, task.base_revision, fingerprint, combined_path.to_string_lossy(), json(&stored)?, json(&evidence)?],
    )?;
    for flag in conflicts {
        transaction.execute(
            "INSERT INTO conflict_flags(id,review_attempt_id,category,evidence_json,created_at) VALUES(?1,?2,?3,?4,strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            params![Uuid::new_v4().to_string(), id, flag.category, json(&flag)?],
        )?;
    }
    timeline::append(
        &transaction,
        EventRefs {
            project_id: &task.project_id,
            task_id: Some(task_id),
            ..Default::default()
        },
        "review.created",
        serde_json::json!({"attemptId": id, "attemptNumber": attempt_number, "fingerprint": fingerprint}),
    )?;
    transaction.commit()?;
    latest_review(database, task_id)?
        .ok_or_else(|| CommandError::new("review_not_found", "Review was not created"))
}

pub(crate) fn decide(
    input: ReviewDecisionInput,
    decision: &str,
    database: &Database,
    git: &GitService,
) -> Result<Review, CommandError> {
    let review = review_by_id(database, &input.attempt_id)?;
    if review.decision != "pending" {
        return Err(CommandError::new(
            "review_already_decided",
            "This review attempt is immutable",
        ));
    }
    if review.fingerprint != input.fingerprint {
        return Err(CommandError::new(
            "review_fingerprint_mismatch",
            "The visible review is not the submitted review",
        ));
    }
    let task = reviewable_task(database, &review.task_id)?;
    let evidence = validation_evidence(database, &review.task_id)?;
    let captured = capture_runs(database, git, &task)?;
    if fingerprint(&task.base_revision, &captured, &evidence)? != review.fingerprint {
        return Err(CommandError::new(
            "review_stale",
            "Agent changes changed after this review was assembled",
        ));
    }
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "UPDATE review_attempts SET decision=?1,feedback=?2,decided_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?3 AND decision='pending'",
        params![decision, input.feedback.trim(), input.attempt_id],
    )?;
    let next = if decision == "approved" {
        "approved"
    } else {
        "working"
    };
    transaction.execute(
        "UPDATE tasks SET status=?1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2",
        params![next, review.task_id],
    )?;
    timeline::append(
        &transaction,
        EventRefs {
            project_id: &task.project_id,
            task_id: Some(&task.id),
            ..Default::default()
        },
        &format!("review.{decision}"),
        serde_json::json!({"attemptId": input.attempt_id, "fingerprint": input.fingerprint, "feedback": input.feedback.trim()}),
    )?;
    transaction.commit()?;
    review_by_id(database, &input.attempt_id)
}

pub(crate) fn merge(
    input: MergeInput,
    database: &Database,
    paths: &RuntimePaths,
    git: &GitService,
) -> Result<String, CommandError> {
    let review = review_by_id(database, &input.attempt_id)?;
    if review.decision != "approved" || review.fingerprint != input.fingerprint {
        return Err(CommandError::new(
            "review_not_approved",
            "Approve this exact review before merging",
        ));
    }
    let task = tasks::get(database, &review.task_id)?
        .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
    let current = capture_runs(database, git, &task)?;
    let evidence = validation_evidence(database, &task.id)?;
    if fingerprint(&task.base_revision, &current, &evidence)? != review.fingerprint {
        return Err(CommandError::new(
            "review_stale",
            "Agent changes changed after approval; assemble a new review",
        ));
    }
    let project_path = project_path(database, &task.project_id)?;
    let target = git.status(&project_path)?;
    if target.branch.as_deref() != Some(&task.base_branch)
        || target.revision.as_deref() != Some(&task.base_revision)
        || target.dirty
    {
        return Err(CommandError::new(
            "target_drift",
            "The opened checkout changed after this Task started",
        ));
    }
    if database.connect()?.query_row(
        "SELECT EXISTS(SELECT 1 FROM merge_attempts WHERE review_attempt_id=?1 AND status='succeeded')",
        [&input.attempt_id],
        |row| row.get::<_, bool>(0),
    )? {
        return Err(CommandError::new("already_merged", "This approved review was already merged"));
    }

    let attempt_id = Uuid::new_v4().to_string();
    database.connect()?.execute(
        "INSERT INTO merge_attempts(id,review_attempt_id,target_branch,expected_target_revision,created_at) VALUES(?1,?2,?3,?4,strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        params![attempt_id, input.attempt_id, task.base_branch, task.base_revision],
    )?;
    let result = perform_merge(
        database,
        paths,
        git,
        &review,
        &task,
        &project_path,
        &attempt_id,
    );
    if let Err(error) = &result {
        database.connect()?.execute(
            "UPDATE merge_attempts SET status='failed',error_code=?1,error_message=?2,completed_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?3",
            params![error.code, error.message, attempt_id],
        )?;
    }
    result
}

pub(crate) fn queue_candidate(
    database: &Database,
    attempt_id: &str,
    fingerprint: &str,
) -> Result<(String, String), CommandError> {
    let review = review_by_id(database, attempt_id)?;
    if review.decision != "approved" || review.fingerprint != fingerprint {
        return Err(CommandError::new(
            "review_not_approved",
            "Approve this exact review before adding it to the merge queue",
        ));
    }
    let task = tasks::get(database, &review.task_id)?
        .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
    Ok((task.id, task.project_id))
}

pub(crate) fn verified_review(
    attempt_id: &str,
    expected_fingerprint: &str,
    database: &Database,
    git: &GitService,
) -> Result<VerifiedReview, CommandError> {
    let review = review_by_id(database, attempt_id)?;
    if review.fingerprint != expected_fingerprint {
        return Err(CommandError::new(
            "review_fingerprint_mismatch",
            "The visible review is not the requested preview",
        ));
    }
    let task = reviewable_task(database, &review.task_id)?;
    let evidence = validation_evidence(database, &task.id)?;
    let current = capture_runs(database, git, &task)?;
    if fingerprint(&task.base_revision, &current, &evidence)? != review.fingerprint {
        return Err(CommandError::new(
            "review_stale",
            "Agent changes changed after this review was assembled",
        ));
    }
    Ok(VerifiedReview {
        project_path: project_path(database, &task.project_id)?,
        review,
    })
}

fn perform_merge(
    database: &Database,
    paths: &RuntimePaths,
    git: &GitService,
    review: &Review,
    task: &tasks::Task,
    project_path: &Path,
    merge_attempt_id: &str,
) -> Result<String, CommandError> {
    let scratch = paths.data_dir.join("integration").join(merge_attempt_id);
    fs::create_dir_all(&scratch).map_err(io_error)?;
    let mut commits = Vec::new();
    for (position, run) in review.runs.iter().enumerate() {
        let branch = format!(
            "subshell/{}/{:02}-{}",
            short_id(&task.id),
            position + 1,
            short_id(&run.run_id)
        );
        let existing: Option<String> = database
            .connect()?
            .query_row(
                "SELECT revision FROM run_branches WHERE agent_run_id=?1 AND review_attempt_id=?2",
                params![run.run_id, review.id],
                |row| row.get(0),
            )
            .optional()?;
        let revision = if let Some(revision) = existing {
            revision
        } else {
            let patch = fs::read(&run.patch_path).map_err(io_error)?;
            if sha256(&patch) != run.patch_sha256 {
                return Err(CommandError::new(
                    "review_corrupt",
                    "A stored approved patch no longer matches its fingerprint",
                ));
            }
            let revision = git.create_snapshot_branch(
                project_path,
                &task.base_revision,
                &scratch.join(format!("snapshot-{}", run.run_id)),
                &branch,
                &patch,
                &format!("{}: {}", task.title, run.title),
            )?;
            database.connect()?.execute(
                "INSERT INTO run_branches(agent_run_id,review_attempt_id,branch_name,revision,created_at) VALUES(?1,?2,?3,?4,strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
                params![run.run_id, review.id, branch, revision],
            )?;
            revision
        };
        commits.push(revision);
    }
    let integration = scratch.join("combined");
    let integrated =
        git.prepare_integration(project_path, &task.base_revision, &commits, &integration)?;
    for command in &task.validation_commands {
        run_validation(&integration, command)?;
    }
    git.publish_integration(
        project_path,
        &task.base_branch,
        &task.base_revision,
        &integrated,
    )?;
    let _ = git.remove_worktree(project_path, &integration);
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "UPDATE merge_attempts SET status='succeeded',result_revision=?1,completed_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2",
        params![integrated, merge_attempt_id],
    )?;
    transaction.execute("UPDATE tasks SET status='archived',archived_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1", [&task.id])?;
    timeline::append(
        &transaction,
        EventRefs {
            project_id: &task.project_id,
            task_id: Some(&task.id),
            ..Default::default()
        },
        "merge.succeeded",
        serde_json::json!({"reviewAttemptId": review.id, "revision": integrated, "branches": review.runs.len()}),
    )?;
    transaction.commit()?;
    for run in &review.runs {
        let path: Option<String> = database
            .connect()?
            .query_row(
                "SELECT path FROM worktrees WHERE agent_run_id=?1",
                [&run.run_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(path) = path
            && git.remove_worktree(project_path, Path::new(&path)).is_ok()
        {
            database.connect()?.execute(
                "UPDATE worktrees SET state='merged',cleaned_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE agent_run_id=?1",
                [&run.run_id],
            )?;
        }
    }
    Ok(integrated)
}

fn capture_runs(
    database: &Database,
    git: &GitService,
    task: &tasks::Task,
) -> Result<Vec<CapturedRun>, CommandError> {
    let connection = database.connect()?;
    let live: i64 = connection.query_row("SELECT COUNT(*) FROM agent_runs WHERE task_id=?1 AND role<>'planner' AND status<>'succeeded'", [&task.id], |row| row.get(0))?;
    if live > 0 {
        return Err(CommandError::new(
            "runs_not_reviewable",
            "Every implementation Run must succeed before review",
        ));
    }
    let mut statement = connection.prepare(
        "SELECT r.id,COALESCE(r.assignment_title,'Assignment'),p.display_name,r.instruction,w.path,r.context_sha256,r.context_manifest_json FROM agent_runs r JOIN provider_accounts p ON p.id=r.provider_account_id JOIN worktrees w ON w.agent_run_id=r.id WHERE r.task_id=?1 AND r.role<>'planner' ORDER BY r.merge_order,r.created_at",
    )?;
    let rows = statement
        .query_map([&task.id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.is_empty() {
        return Err(CommandError::new(
            "runs_not_reviewable",
            "No completed implementation Runs are available",
        ));
    }
    rows.into_iter()
        .map(
            |(
                run_id,
                title,
                provider_name,
                instruction,
                worktree,
                context_sha256,
                context_manifest,
            )| {
                let diff = git.exact_diff(Path::new(&worktree), &task.base_revision)?;
                let explanation = latest_progress(database, &run_id)?;
                Ok(CapturedRun {
                    snapshot: RunSnapshot {
                        run_id,
                        title,
                        provider_name,
                        instruction,
                        explanation,
                        files: diff.files,
                        patch_sha256: sha256(&diff.patch),
                        patch_path: String::new(),
                        context_sha256,
                        context_manifest: serde_json::from_str(
                            context_manifest.as_deref().unwrap_or("{}"),
                        )
                        .unwrap_or_else(|_| serde_json::json!({})),
                    },
                    patch: diff.patch,
                })
            },
        )
        .collect()
}

fn fingerprint(
    base: &str,
    runs: &[CapturedRun],
    evidence: &[ValidationEvidence],
) -> Result<String, CommandError> {
    let value = serde_json::json!({
        "baseRevision": base,
        "runs": runs.iter().map(|run| &run.snapshot).collect::<Vec<_>>(),
        "validationEvidence": evidence,
    });
    Ok(sha256(&serde_json::to_vec(&value).map_err(json_error)?))
}

fn conflict_flags(runs: &[RunSnapshot]) -> Vec<ConflictFlag> {
    let mut flags = Vec::new();
    for (index, left) in runs.iter().enumerate() {
        for right in runs.iter().skip(index + 1) {
            let same = left
                .files
                .iter()
                .filter(|file| right.files.contains(file))
                .cloned()
                .collect::<Vec<_>>();
            if !same.is_empty() {
                flags.push(ConflictFlag {
                    category: "same_file".into(),
                    run_ids: vec![left.run_id.clone(), right.run_id.clone()],
                    evidence: same.join(", "),
                });
            }
            let related = left
                .files
                .iter()
                .flat_map(|a| {
                    right
                        .files
                        .iter()
                        .filter(move |b| a != *b && file_key(a) == file_key(b))
                        .map(move |b| format!("{a} ↔ {b}"))
                })
                .collect::<Vec<_>>();
            if !related.is_empty() {
                flags.push(ConflictFlag {
                    category: "related_file".into(),
                    run_ids: vec![left.run_id.clone(), right.run_id.clone()],
                    evidence: related.join(", "),
                });
            }
            let left_patch = fs::read_to_string(&left.patch_path).unwrap_or_default();
            let left_patch = left_patch.to_lowercase();
            let references = right
                .files
                .iter()
                .map(|file| file_key(file))
                .filter(|key| key.len() >= 4 && left_patch.contains(key.as_str()))
                .collect::<Vec<_>>();
            if !references.is_empty() {
                flags.push(ConflictFlag {
                    category: "shared_text_reference".into(),
                    run_ids: vec![left.run_id.clone(), right.run_id.clone()],
                    evidence: references.join(", "),
                });
            }
        }
    }
    flags
}

fn validation_evidence(
    database: &Database,
    task_id: &str,
) -> Result<Vec<ValidationEvidence>, CommandError> {
    let connection = database.connect()?;
    let mut statement = connection.prepare(
        "SELECT agent_run_id,json_extract(payload_json,'$.detail') FROM timeline_events WHERE task_id=?1 AND event_type='agent.reported_validation' AND agent_run_id IS NOT NULL ORDER BY sequence",
    )?;
    Ok(statement
        .query_map([task_id], |row| {
            Ok(ValidationEvidence {
                run_id: row.get(0)?,
                summary: row
                    .get::<_, Option<String>>(1)?
                    .unwrap_or_else(|| "Validation reported".into()),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn latest_progress(database: &Database, run_id: &str) -> Result<Option<String>, CommandError> {
    Ok(database
        .connect()?
        .query_row(
            "SELECT json_extract(payload_json,'$.detail') FROM timeline_events WHERE agent_run_id=?1 AND event_type='agent.reported_progress' ORDER BY sequence DESC LIMIT 1",
            [run_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn latest_review(database: &Database, task_id: &str) -> Result<Option<Review>, CommandError> {
    let connection = database.connect()?;
    let id: Option<String> = connection.query_row("SELECT a.id FROM review_attempts a JOIN review_records r ON r.id=a.review_record_id WHERE r.task_id=?1 ORDER BY a.attempt_number DESC LIMIT 1", [task_id], |row| row.get(0)).optional()?;
    id.map(|id| review_by_id(database, &id)).transpose()
}

fn latest_attempt_number(database: &Database, task_id: &str) -> Result<u32, CommandError> {
    Ok(database.connect()?.query_row("SELECT COALESCE(MAX(a.attempt_number),0) FROM review_attempts a JOIN review_records r ON r.id=a.review_record_id WHERE r.task_id=?1", [task_id], |row| row.get(0))?)
}

fn review_by_id(database: &Database, id: &str) -> Result<Review, CommandError> {
    let connection = database.connect()?;
    let row = connection.query_row(
        "SELECT a.id,r.task_id,a.attempt_number,a.base_revision,a.input_fingerprint,a.decision,a.feedback,a.combined_diff_path,a.snapshot_json,a.validation_evidence_json FROM review_attempts a JOIN review_records r ON r.id=a.review_record_id WHERE a.id=?1",
        [id],
        |row| Ok((row.get::<_, String>(0)?,row.get::<_, String>(1)?,row.get::<_, u32>(2)?,row.get::<_, String>(3)?,row.get::<_, String>(4)?,row.get::<_, String>(5)?,row.get::<_, Option<String>>(6)?,row.get::<_, String>(7)?,row.get::<_, String>(8)?,row.get::<_, String>(9)?)),
    ).optional()?.ok_or_else(|| CommandError::new("review_not_found", "Review attempt was not found"))?;
    let snapshot: StoredSnapshot = serde_json::from_str(&row.8).map_err(json_error)?;
    let evidence = serde_json::from_str(&row.9).map_err(json_error)?;
    let mut statement = connection.prepare("SELECT evidence_json FROM conflict_flags WHERE review_attempt_id=?1 ORDER BY created_at,id")?;
    let conflicts = statement
        .query_map([id], |row| row.get::<_, String>(0))?
        .map(|value| {
            serde_json::from_str(&value?).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let combined_patch = String::from_utf8_lossy(&fs::read(&row.7).map_err(io_error)?).into_owned();
    Ok(Review {
        id: row.0,
        task_id: row.1,
        attempt_number: row.2,
        base_revision: row.3,
        fingerprint: row.4,
        decision: row.5,
        feedback: row.6,
        combined_diff_path: row.7,
        combined_patch,
        runs: snapshot.runs,
        conflicts,
        validation_evidence: evidence,
    })
}

fn reviewable_task(database: &Database, task_id: &str) -> Result<tasks::Task, CommandError> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction()?;
    tasks::rollup_in_transaction(&transaction, task_id)?;
    transaction.commit()?;
    let task = tasks::get(database, task_id)?
        .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
    if !matches!(task.status.as_str(), "review" | "approved") {
        let (implementations, active, failed): (i64, i64, i64) = database.connect()?.query_row(
            "SELECT COUNT(*),COALESCE(SUM(status IN('queued','preparing','running','waiting')),0),COALESCE(SUM(status IN('failed','cancelled')),0) FROM agent_runs WHERE task_id=?1 AND role<>'planner'",
            [task_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let message = if implementations == 0 {
            "Approve the planner's proposed assignments or start an implementation agent first"
        } else if active > 0 {
            "Finish the active implementation agents before opening Review"
        } else if failed > 0 {
            "Retry failed agents or keep their changes before opening Review"
        } else {
            "Every implementation agent must finish before opening Review"
        };
        return Err(CommandError::new("task_not_reviewable", message));
    }
    Ok(task)
}

fn project_path(database: &Database, project_id: &str) -> Result<PathBuf, CommandError> {
    Ok(PathBuf::from(database.connect()?.query_row(
        "SELECT path FROM projects WHERE id=?1",
        [project_id],
        |row| row.get::<_, String>(0),
    )?))
}

fn run_validation(directory: &Path, command: &str) -> Result<(), CommandError> {
    if command.trim().is_empty() {
        return Ok(());
    }
    #[cfg(windows)]
    let output = Command::new("cmd")
        .args(["/C", command])
        .current_dir(directory)
        .output();
    #[cfg(not(windows))]
    let output = Command::new("sh")
        .args(["-lc", command])
        .current_dir(directory)
        .output();
    let output = output.map_err(io_error)?;
    if output.status.success() {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&output.stderr);
    Err(CommandError::new(
        "validation_failed",
        message.chars().take(4000).collect::<String>(),
    ))
}

fn file_key(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .replace(".test", "")
        .replace("_test", "")
        .to_lowercase()
}

fn short_id(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect()
}
fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn json<T: Serialize>(value: &T) -> Result<String, CommandError> {
    serde_json::to_string(value).map_err(json_error)
}
fn json_error(error: serde_json::Error) -> CommandError {
    CommandError::new("serialization_failed", error.to_string())
}
fn io_error(error: std::io::Error) -> CommandError {
    CommandError::new("filesystem_error", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        features::{context::ContextDrafts, runs::RunService},
        platform::{
            environment::PortLeases,
            keychain::MemorySecretStore,
            process::{ProcessSpec, ProcessSupervisor},
        },
    };
    use std::{process::Command, sync::Arc};
    use tauri::webview::InvokeRequest;

    fn invoke(
        webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
        command: &str,
        input: serde_json::Value,
    ) -> serde_json::Value {
        tauri::test::get_ipc_response(
            webview,
            InvokeRequest {
                cmd: command.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(serde_json::json!({"input": input})),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.into(),
            },
        )
        .unwrap_or_else(|error| panic!("{command} failed: {error}"))
        .deserialize()
        .unwrap()
    }

    #[test]
    fn completed_runs_reconcile_a_stale_task_before_review() {
        let root = tempfile::tempdir().unwrap();
        let database = Database::initialize(&root.path().join("db.sqlite3")).unwrap();
        let connection = database.connect().unwrap();
        connection.execute("INSERT INTO projects(id,name,path,created_at,updated_at) VALUES('project','Project','/tmp/repo','now','now')", []).unwrap();
        connection.execute("INSERT INTO provider_accounts(id,provider_type,display_name,config_scope_path,status,created_at,updated_at) VALUES('provider','generic','Codex','/tmp/provider','active','now','now')", []).unwrap();
        connection.execute("INSERT INTO tasks(id,project_id,title,status,base_branch,base_revision,created_at,updated_at) VALUES('task','project','Completed work','working','main','base','now','now')", []).unwrap();
        connection.execute("INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,role,status,merge_order,created_at,updated_at) VALUES('run','task','provider','Implement','implementer','running',0,'now','now')", []).unwrap();
        drop(connection);

        assert_eq!(
            reviewable_task(&database, "task").unwrap_err().message,
            "Finish the active implementation agents before opening Review"
        );
        database
            .connect()
            .unwrap()
            .execute(
                "UPDATE agent_runs SET status='succeeded' WHERE id='run'",
                [],
            )
            .unwrap();
        assert_eq!(reviewable_task(&database, "task").unwrap().status, "review");
    }

    #[test]
    fn conflict_flags_are_informational_and_deterministic() {
        let root = tempfile::tempdir().unwrap();
        let left_patch = root.path().join("left.patch");
        fs::write(&left_patch, "+import auth\n").unwrap();
        let runs = vec![
            RunSnapshot {
                run_id: "a".into(),
                title: "A".into(),
                provider_name: "Codex".into(),
                instruction: "A".into(),
                explanation: Some("Implemented A".into()),
                files: vec!["src/auth.ts".into()],
                patch_path: left_patch.to_string_lossy().into(),
                patch_sha256: "a".into(),
                context_sha256: "a".into(),
                context_manifest: serde_json::json!({}),
            },
            RunSnapshot {
                run_id: "b".into(),
                title: "B".into(),
                provider_name: "Claude".into(),
                instruction: "B".into(),
                explanation: Some("Implemented B".into()),
                files: vec!["tests/auth.test.ts".into()],
                patch_path: String::new(),
                patch_sha256: "b".into(),
                context_sha256: "b".into(),
                context_manifest: serde_json::json!({}),
            },
        ];
        let flags = conflict_flags(&runs);
        assert!(flags.iter().any(|flag| flag.category == "related_file"));
        assert!(
            flags
                .iter()
                .any(|flag| flag.category == "shared_text_reference")
        );
    }

    #[test]
    fn full_loop_shares_context_reviews_approvals_merges_cleans_and_reloads() {
        let root = tempfile::tempdir().unwrap();
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
        let git = GitService::default();
        let status = git.status(&repository).unwrap();
        let base = status.revision.unwrap();
        let branch = status.branch.unwrap();
        let paths = RuntimePaths {
            data_dir: root.path().join("data"),
        };
        let database = Database::initialize(&paths.data_dir.join("db.sqlite3")).unwrap();
        let connection = database.connect().unwrap();
        connection.execute("INSERT INTO projects(id,name,path,created_at,updated_at) VALUES('project','Project',?1,'now','now')", [repository.to_string_lossy()]).unwrap();
        connection.execute("INSERT INTO provider_accounts(id,provider_type,display_name,config_scope_path,status,created_at,updated_at) VALUES('provider','generic','Codex','/tmp/provider','active','now','now')", []).unwrap();
        connection.execute("INSERT INTO tasks(id,project_id,title,status,base_branch,base_revision,created_at,updated_at) VALUES('task','project','Parallel change','working',?1,?2,'now','now')", params![branch, base]).unwrap();
        let files = ["src/auth.ts", "tests/auth.test.ts"];
        let mut worktrees = Vec::new();
        for (index, file) in files.iter().enumerate() {
            let run_id = format!("run-{index}");
            let worktree = root.path().join(format!("worktree-{index}"));
            git.create_worktree(&repository, &base, &worktree).unwrap();
            fs::create_dir_all(worktree.join(Path::new(file).parent().unwrap())).unwrap();
            fs::write(worktree.join(file), file).unwrap();
            connection.execute("INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,role,assignment_title,status,merge_order,context_sha256,created_at,updated_at) VALUES(?1,'task','provider','Implement','executor',?2,'running',?3,?4,'now','now')", params![run_id, file, index as i64, format!("context-{index}")]).unwrap();
            connection.execute("INSERT INTO worktrees(id,agent_run_id,path,base_branch,base_revision,state,created_at) VALUES(?1,?2,?3,?4,?5,'active','now')", params![format!("worktree-row-{index}"), run_id, worktree.to_string_lossy(), branch, base]).unwrap();
            worktrees.push(worktree);
        }

        let share_log = root.path().join("share.log");
        let processes = ProcessSupervisor::default();
        processes
            .launch(
                "run-1".into(),
                ProcessSpec {
                    executable: "/bin/sh".into(),
                    arguments: vec!["-c".into(), "cat >/dev/null".into()],
                    cwd: worktrees[1].clone(),
                    environment: vec![("PATH".into(), std::env::var("PATH").unwrap())],
                    log_path: share_log,
                    stdin: None,
                    redactions: vec![],
                },
                Arc::new(|_| {}),
            )
            .unwrap();
        let runs = RunService::new(
            database.clone(),
            paths.clone(),
            git.clone(),
            ContextDrafts::default(),
            processes.clone(),
            PortLeases::default(),
            Arc::new(MemorySecretStore::default()),
        );
        let app = crate::app::configure(tauri::test::mock_builder())
            .manage(database.clone())
            .manage(paths.clone())
            .manage(git.clone())
            .manage(processes.clone())
            .manage(runs)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        let share = invoke(
            &webview,
            "context_share_preview",
            serde_json::json!({
                "sourceRunId": null,
                "targetRunId": "run-1",
                "kind": "summary",
                "contentReference": null,
                "summary": "Use the shared authentication contract"
            }),
        );
        let delivered = invoke(
            &webview,
            "context_share_deliver",
            serde_json::json!({
                "sourceRunId": null,
                "targetRunId": "run-1",
                "kind": "summary",
                "contentReference": null,
                "content": share["content"],
                "previewSha256": share["sha256"]
            }),
        );
        assert_eq!(delivered["deliveryStatus"], "delivered");
        processes.stop("run-1").unwrap();

        let denied = invoke(
            &webview,
            "workspace_request_action",
            serde_json::json!({
                "runId": "run-0",
                "action": "create_branch",
                "arguments": {"name":"unsafe"}
            }),
        );
        assert_eq!(
            invoke(
                &webview,
                "workspace_decide_action",
                serde_json::json!({"requestId":denied["id"],"decision":"denied"})
            )["status"],
            "denied"
        );
        let approved_branch = invoke(
            &webview,
            "workspace_request_action",
            serde_json::json!({
                "runId": "run-0",
                "action": "create_branch",
                "arguments": {"name":"approved/safe"}
            }),
        );
        assert_eq!(
            invoke(
                &webview,
                "workspace_decide_action",
                serde_json::json!({"requestId":approved_branch["id"],"decision":"approved"})
            )["executionStatus"],
            "succeeded"
        );
        connection.execute("UPDATE agent_runs SET status='succeeded',ended_at='now',updated_at='now' WHERE task_id='task'", []).unwrap();
        connection
            .execute(
                "UPDATE tasks SET status='review',updated_at='now' WHERE id='task'",
                [],
            )
            .unwrap();

        let pending = invoke(&webview, "review_get", serde_json::json!({"taskId":"task"}));
        assert_eq!(pending["runs"].as_array().unwrap().len(), 2);
        assert!(
            pending["conflicts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|flag| flag["category"] == "related_file")
        );
        let approved = invoke(
            &webview,
            "review_approve",
            serde_json::json!({
                "attemptId":pending["id"],
                "fingerprint":pending["fingerprint"],
                "feedback":""
            }),
        );
        let revision = invoke(
            &webview,
            "review_merge",
            serde_json::json!({
                "attemptId":approved["id"],
                "fingerprint":approved["fingerprint"]
            }),
        )
        .as_str()
        .unwrap()
        .to_string();

        assert_eq!(
            git.status(&repository).unwrap().revision.as_deref(),
            Some(revision.as_str())
        );
        assert_eq!(
            fs::read_to_string(repository.join(files[0])).unwrap(),
            files[0]
        );
        assert_eq!(
            fs::read_to_string(repository.join(files[1])).unwrap(),
            files[1]
        );
        assert_eq!(
            tasks::get(&database, "task").unwrap().unwrap().status,
            "archived"
        );
        assert_eq!(
            database
                .connect()
                .unwrap()
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM run_branches", [], |row| row.get(0))
                .unwrap(),
            2
        );
        assert!(worktrees.iter().all(|worktree| !worktree.exists()));
        assert_eq!(database.connect().unwrap().query_row::<i64, _, _>("SELECT COUNT(*) FROM worktrees WHERE state='merged' AND cleaned_at IS NOT NULL", [], |row| row.get(0)).unwrap(), 2);
        drop(webview);
        drop(app);
        drop(connection);
        drop(database);
        let reopened = Database::initialize(&paths.data_dir.join("db.sqlite3")).unwrap();
        let relaunched = crate::app::configure(tauri::test::mock_builder())
            .manage(reopened.clone())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let relaunched_webview =
            tauri::WebviewWindowBuilder::new(&relaunched, "main", Default::default())
                .build()
                .unwrap();
        let archived = invoke(
            &relaunched_webview,
            "tasks_list_archived",
            serde_json::json!({"projectId":"project"}),
        );
        assert_eq!(archived["items"][0]["status"], "archived");
        assert!(reopened.connect().unwrap().query_row::<i64, _, _>("SELECT COUNT(*) FROM timeline_events WHERE project_id='project' AND event_type IN('context.shared','approval.denied','approval.approved','merge.succeeded')", [], |row| row.get(0)).unwrap() >= 4);
    }
}
