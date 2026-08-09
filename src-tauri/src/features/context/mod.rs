use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;
use uuid::Uuid;

use crate::{
    contracts::CommandError,
    features::tasks,
    platform::{database::Database, git::GitService},
};

const BUDGET: usize = 64 * 1024;
const SKILL: &str = include_str!("../../../resources/subshell-context/SKILL.md");

#[derive(Clone, Default)]
pub struct ContextDrafts(Arc<Mutex<HashMap<String, ContextManifest>>>);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcesInput {
    pub project_id: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewInput {
    pub task_id: String,
    pub instruction: String,
    #[serde(default)]
    pub selected_files: Vec<String>,
    pub pattern: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextEntry {
    pub source: String,
    pub bytes: usize,
    pub included: bool,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextManifest {
    pub entries: Vec<ContextEntry>,
    pub total_bytes: usize,
    pub budget_bytes: usize,
    pub reported_tokens: Option<usize>,
    pub was_edited: bool,
    pub sha256: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPreview {
    pub token: String,
    pub content: String,
    pub sha256: String,
    pub manifest: ContextManifest,
}

#[tauri::command]
pub fn context_sources(
    input: SourcesInput,
    database: State<Database>,
    git: State<GitService>,
) -> Result<Vec<String>, CommandError> {
    let path = project_path(&database, &input.project_id)?;
    git.files(&path)
}
#[tauri::command]
pub fn context_preview(
    input: PreviewInput,
    database: State<Database>,
    git: State<GitService>,
    drafts: State<ContextDrafts>,
) -> Result<ContextPreview, CommandError> {
    build(input, &database, &git, &drafts)
}

pub fn take(drafts: &ContextDrafts, token: &str) -> Result<ContextManifest, CommandError> {
    drafts
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(token)
        .ok_or_else(|| {
            CommandError::new(
                "context_expired",
                "Preview context again before starting runs",
            )
        })
}

pub(crate) fn build(
    input: PreviewInput,
    database: &Database,
    git: &GitService,
    drafts: &ContextDrafts,
) -> Result<ContextPreview, CommandError> {
    let task = tasks::get(database, &input.task_id)?
        .ok_or_else(|| CommandError::new("task_not_found", "Task was not found"))?;
    let root = project_path(database, &task.project_id)?;
    let available = git.files(&root)?;
    let mut content = format!(
        "# Task\n\nTitle: {}\n\n{}\n\nAssignment: {}\n\nAcceptance criteria:\n{}\n\nAllowed paths:\n{}\n\nValidation commands:\n{}\n\nDecisions:\n{}\n\n# Built-in safety skill\n\n{}\n",
        task.title,
        task.description,
        input.instruction,
        lines(&task.acceptance_criteria),
        lines(&task.allowed_paths),
        lines(&task.validation_commands),
        lines(&task.decisions),
        SKILL
    );
    let mut entries = vec![ContextEntry {
        source: "task".into(),
        bytes: content.len(),
        included: true,
        reason: None,
    }];
    if content.len() > BUDGET {
        return Err(CommandError::new(
            "context_required_too_large",
            "Task and built-in context exceed the 64 KiB budget",
        ));
    }
    let mut ordered = Vec::new();
    if available.iter().any(|f| f == "AGENTS.md") {
        ordered.push("AGENTS.md".to_string());
    }
    if let Some(pattern) = input.pattern.filter(|p| !p.trim().is_empty()) {
        for file in &available {
            if star_match(&pattern, file) && !ordered.contains(file) {
                ordered.push(file.clone());
            }
        }
    }
    for file in input.selected_files {
        if available.contains(&file) && !ordered.contains(&file) {
            ordered.push(file);
        }
    }
    for relative in ordered {
        let source_bytes = source_size(&root, &relative)?;
        if content.len() + source_bytes + relative.len() + 64 > BUDGET {
            entries.push(ContextEntry {
                source: relative,
                bytes: source_bytes,
                included: false,
                reason: Some("budget_exceeded".into()),
            });
            continue;
        }
        let Some(section) = read_section(&root, &relative)? else {
            entries.push(ContextEntry {
                source: relative,
                bytes: source_bytes,
                included: false,
                reason: Some("not_utf8".into()),
            });
            continue;
        };
        let bytes = section.len();
        if content.len() + bytes <= BUDGET {
            content.push_str(&section);
            entries.push(ContextEntry {
                source: relative,
                bytes,
                included: true,
                reason: None,
            });
        } else {
            entries.push(ContextEntry {
                source: relative,
                bytes,
                included: false,
                reason: Some("budget_exceeded".into()),
            });
        }
    }
    let sha256 = format!("{:x}", Sha256::digest(content.as_bytes()));
    let manifest = ContextManifest {
        total_bytes: content.len(),
        budget_bytes: BUDGET,
        reported_tokens: None,
        was_edited: false,
        sha256: sha256.clone(),
        entries,
    };
    let token = Uuid::new_v4().to_string();
    drafts
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(token.clone(), manifest.clone());
    Ok(ContextPreview {
        token,
        content,
        sha256,
        manifest,
    })
}

fn project_path(database: &Database, id: &str) -> Result<PathBuf, CommandError> {
    database
        .connect()?
        .query_row("SELECT path FROM projects WHERE id=?1", [id], |row| {
            row.get::<_, String>(0)
        })
        .map(PathBuf::from)
        .map_err(Into::into)
}
fn source_path(root: &Path, relative: &str) -> Result<PathBuf, CommandError> {
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|e| CommandError::new("context_source_unavailable", e.to_string()))?;
    let root = root
        .canonicalize()
        .map_err(|e| CommandError::new("invalid_path", e.to_string()))?;
    if !path.starts_with(root) || !path.is_file() {
        return Err(CommandError::new(
            "invalid_context_source",
            "Context file leaves the project",
        ));
    }
    Ok(path)
}
fn source_size(root: &Path, relative: &str) -> Result<usize, CommandError> {
    Ok(fs::metadata(source_path(root, relative)?)
        .map_err(|e| CommandError::new("context_source_unavailable", e.to_string()))?
        .len() as usize)
}
fn read_section(root: &Path, relative: &str) -> Result<Option<String>, CommandError> {
    let bytes = fs::read(source_path(root, relative)?)
        .map_err(|e| CommandError::new("context_source_unavailable", e.to_string()))?;
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    Ok(Some(format!(
        "\n<repository-file path={:?}>\n{}\n</repository-file>\n",
        relative, text
    )))
}
fn lines(values: &[String]) -> String {
    if values.is_empty() {
        "- None supplied".into()
    } else {
        values
            .iter()
            .map(|v| format!("- {v}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
fn star_match(pattern: &str, value: &str) -> bool {
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == value;
    }
    let mut rest = value;
    if !pattern.starts_with('*') {
        let Some(next) = rest.strip_prefix(parts[0]) else {
            return false;
        };
        rest = next;
    }
    for (part_index, part) in parts.iter().enumerate().skip(1) {
        if part.is_empty() {
            continue;
        }
        if part_index == parts.len() - 1 && !pattern.ends_with('*') {
            return rest.ends_with(part);
        }
        let Some(index) = rest.find(part) else {
            return false;
        };
        rest = &rest[index + part.len()..];
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wildcard_matching_is_bounded_and_predictable() {
        assert!(star_match("src/*.rs", "src/app.rs"));
        assert!(!star_match("src/*.rs", "tests/app.rs"));
    }
}
