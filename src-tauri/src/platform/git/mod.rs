use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use serde::Serialize;

use crate::contracts::CommandError;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub is_repository: bool,
    pub branch: Option<String>,
    pub revision: Option<String>,
    pub dirty: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiff {
    pub files: Vec<String>,
    pub patch: String,
    pub truncated: bool,
}

#[derive(Clone, Debug)]
pub struct ExactGitDiff {
    pub files: Vec<String>,
    pub patch: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct GitService {
    mutation_locks: Arc<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>>,
}

impl GitService {
    pub fn status(&self, path: &Path) -> Result<GitStatus, CommandError> {
        if !path.is_dir() {
            return Err(CommandError::new(
                "invalid_path",
                "Project directory does not exist",
            ));
        }
        let inside = git(path, ["rev-parse", "--is-inside-work-tree"]);
        if !matches!(inside.as_deref(), Ok("true")) {
            return Ok(GitStatus {
                is_repository: false,
                branch: None,
                revision: None,
                dirty: false,
            });
        }
        let branch = git(path, ["symbolic-ref", "--quiet", "--short", "HEAD"]).ok();
        // An initialized repository has a branch but no revision until its first commit.
        let revision = git(path, ["rev-parse", "HEAD"]).ok();
        let dirty = !git(path, ["status", "--porcelain"])?.is_empty();
        Ok(GitStatus {
            is_repository: true,
            branch,
            revision,
            dirty,
        })
    }

    pub fn files(&self, path: &Path) -> Result<Vec<String>, CommandError> {
        let output = Command::new("git")
            .args([
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "-z",
            ])
            .current_dir(path)
            .output()
            .map_err(|error| CommandError::new("git_unavailable", error.to_string()))?;
        if !output.status.success() {
            return Err(CommandError::new(
                "git_failed",
                String::from_utf8_lossy(&output.stderr),
            ));
        }
        let mut files = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect::<Vec<_>>();
        append_local_guidance(path, &mut files);
        files.sort();
        files.dedup();
        Ok(files)
    }

    pub fn diff(&self, path: &Path, base_revision: &str) -> Result<GitDiff, CommandError> {
        const MAX_PATCH_BYTES: usize = 1024 * 1024;
        let exact = self.exact_diff(path, base_revision)?;
        let truncated = exact.patch.len() > MAX_PATCH_BYTES;
        let patch = &exact.patch[..exact.patch.len().min(MAX_PATCH_BYTES)];
        Ok(GitDiff {
            files: exact.files,
            patch: String::from_utf8_lossy(patch).into_owned(),
            truncated,
        })
    }

    pub fn exact_diff(
        &self,
        path: &Path,
        base_revision: &str,
    ) -> Result<ExactGitDiff, CommandError> {
        let names = Command::new("git")
            .args(["diff", "--name-only", "-z", base_revision, "--"])
            .current_dir(path)
            .output()
            .map_err(|error| CommandError::new("git_unavailable", error.to_string()))?;
        let tracked = Command::new("git")
            .args([
                "diff",
                "--binary",
                "--full-index",
                "--find-renames",
                "--no-ext-diff",
                "--no-color",
                base_revision,
                "--",
            ])
            .current_dir(path)
            .output()
            .map_err(|error| CommandError::new("git_unavailable", error.to_string()))?;
        if !names.status.success() || !tracked.status.success() {
            return Err(CommandError::new(
                "git_failed",
                String::from_utf8_lossy(if !names.status.success() {
                    &names.stderr
                } else {
                    &tracked.stderr
                }),
            ));
        }
        let untracked = Command::new("git")
            .args(["ls-files", "--others", "--exclude-standard", "-z"])
            .current_dir(path)
            .output()
            .map_err(|error| CommandError::new("git_unavailable", error.to_string()))?;
        if !untracked.status.success() {
            return Err(CommandError::new(
                "git_failed",
                String::from_utf8_lossy(&untracked.stderr),
            ));
        }
        let paths = |bytes: &[u8]| {
            bytes
                .split(|byte| *byte == 0)
                .filter(|part| !part.is_empty())
                .map(|part| String::from_utf8_lossy(part).into_owned())
                .collect::<Vec<_>>()
        };
        let new_files = paths(&untracked.stdout);
        let mut files = paths(&names.stdout);
        files.extend(new_files.iter().cloned());
        files.sort();
        files.dedup();

        let mut patch = tracked.stdout;
        for file in new_files {
            let output = Command::new("git")
                .args([
                    "diff",
                    "--no-index",
                    "--binary",
                    "--full-index",
                    "--no-color",
                    "--",
                ])
                .arg(null_device())
                .arg(&file)
                .current_dir(path)
                .output()
                .map_err(|error| CommandError::new("git_unavailable", error.to_string()))?;
            if !matches!(output.status.code(), Some(0 | 1)) {
                return Err(CommandError::new(
                    "git_failed",
                    String::from_utf8_lossy(&output.stderr),
                ));
            }
            patch.extend(output.stdout);
        }
        Ok(ExactGitDiff { files, patch })
    }

    pub fn create_worktree(
        &self,
        repository: &Path,
        revision: &str,
        destination: &Path,
    ) -> Result<(), CommandError> {
        self.create_worktree_cancellable(repository, revision, destination, &AtomicBool::new(false))
    }

    pub fn create_worktree_cancellable(
        &self,
        repository: &Path,
        revision: &str,
        destination: &Path,
        cancelled: &AtomicBool,
    ) -> Result<(), CommandError> {
        if cancelled.load(Ordering::SeqCst) {
            return Err(CommandError::new(
                "cancelled",
                "Worktree creation was cancelled",
            ));
        }
        let canonical = repository
            .canonicalize()
            .map_err(|error| CommandError::new("invalid_path", error.to_string()))?;
        let lock = {
            let mut locks = self
                .mutation_locks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            locks.entry(canonical).or_default().clone()
        };
        // Git's own lock is not enough to order two worktree mutations started by this app.
        let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if cancelled.load(Ordering::SeqCst) {
            return Err(CommandError::new(
                "cancelled",
                "Worktree creation was cancelled",
            ));
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(io_error)?;
        }
        let output = Command::new("git")
            .args([
                "worktree",
                "add",
                "--detach",
                "--lock",
                "--reason",
                "SubShell agent run",
            ])
            .arg(destination)
            .arg(revision)
            .current_dir(repository)
            .output()
            .map_err(|error| CommandError::new("git_unavailable", error.to_string()))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(CommandError::new(
                "worktree_failed",
                String::from_utf8_lossy(&output.stderr),
            ))
        }
    }

    pub fn create_snapshot_branch(
        &self,
        repository: &Path,
        base_revision: &str,
        destination: &Path,
        branch_name: &str,
        patch: &[u8],
        message: &str,
    ) -> Result<String, CommandError> {
        validate_branch_name(repository, branch_name)?;
        if git_status(
            repository,
            [
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch_name}"),
            ],
        )? {
            return Err(CommandError::new(
                "branch_exists",
                "The approved Run branch already exists",
            ));
        }
        self.create_worktree(repository, base_revision, destination)?;
        let result = (|| {
            apply_patch(destination, patch)?;
            git_command(
                destination,
                [
                    "-c",
                    "user.name=SubShell",
                    "-c",
                    "user.email=subshell@localhost",
                    "commit",
                    "--allow-empty",
                    "-qm",
                    message,
                ],
                None,
            )?;
            let revision = git(destination, ["rev-parse", "HEAD"])?;
            git_command(
                repository,
                [
                    "update-ref",
                    &format!("refs/heads/{branch_name}"),
                    &revision,
                    "0000000000000000000000000000000000000000",
                ],
                None,
            )?;
            Ok(revision)
        })();
        let _ = self.remove_worktree(repository, destination);
        result
    }

    pub fn prepare_integration(
        &self,
        repository: &Path,
        expected_target: &str,
        commits: &[String],
        destination: &Path,
    ) -> Result<String, CommandError> {
        self.create_worktree(repository, expected_target, destination)?;
        for revision in commits {
            if let Err(error) = git_command(destination, ["cherry-pick", revision], None) {
                let files = git(destination, ["diff", "--name-only", "--diff-filter=U"])
                    .unwrap_or_default()
                    .lines()
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                let _ = git_command(destination, ["cherry-pick", "--abort"], None);
                let _ = self.remove_worktree(repository, destination);
                let mut conflict = CommandError::new("merge_conflict", error.message);
                conflict.details = Some(serde_json::json!({ "files": files }));
                return Err(conflict);
            }
        }
        git(destination, ["rev-parse", "HEAD"])
    }

    pub fn remove_branch(&self, repository: &Path, branch_name: &str) -> Result<(), CommandError> {
        validate_branch_name(repository, branch_name)?;
        git_command(
            repository,
            ["update-ref", "-d", &format!("refs/heads/{branch_name}")],
            None,
        )?;
        Ok(())
    }

    pub fn publish_integration(
        &self,
        repository: &Path,
        target_branch: &str,
        expected_target: &str,
        integrated_revision: &str,
    ) -> Result<(), CommandError> {
        let status = self.status(repository)?;
        if status.branch.as_deref() != Some(target_branch)
            || status.revision.as_deref() != Some(expected_target)
            || status.dirty
        {
            return Err(CommandError::new(
                "target_drift",
                "The target branch or checkout changed after review",
            ));
        }
        if !git_status(
            repository,
            [
                "merge-base",
                "--is-ancestor",
                expected_target,
                integrated_revision,
            ],
        )? {
            return Err(CommandError::new(
                "invalid_integration",
                "The integrated revision is not a fast-forward of the approved target",
            ));
        }
        git_command(
            repository,
            ["merge", "--ff-only", "--quiet", integrated_revision],
            None,
        )?;
        Ok(())
    }

    pub fn remove_worktree(
        &self,
        repository: &Path,
        destination: &Path,
    ) -> Result<(), CommandError> {
        let _ = Command::new("git")
            .args(["worktree", "unlock"])
            .arg(destination)
            .current_dir(repository)
            .output();
        git_command(
            repository,
            [
                "worktree",
                "remove",
                "--force",
                &destination.to_string_lossy(),
            ],
            None,
        )?;
        Ok(())
    }
}

fn validate_branch_name(repository: &Path, branch_name: &str) -> Result<(), CommandError> {
    if branch_name.len() > 180
        || !git_status(repository, ["check-ref-format", "--branch", branch_name])?
    {
        return Err(CommandError::new(
            "invalid_branch_name",
            "The proposed branch name is not valid",
        ));
    }
    Ok(())
}

fn apply_patch(path: &Path, patch: &[u8]) -> Result<(), CommandError> {
    if patch.is_empty() {
        return Ok(());
    }
    git_command(
        path,
        ["apply", "--index", "--binary", "--whitespace=nowarn", "-"],
        Some(patch),
    )?;
    Ok(())
}

fn git_command<const N: usize>(
    path: &Path,
    args: [&str; N],
    stdin: Option<&[u8]>,
) -> Result<Vec<u8>, CommandError> {
    let mut command = Command::new("git");
    command.args(args).current_dir(path);
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| CommandError::new("git_unavailable", error.to_string()))?;
    if let Some(bytes) = stdin {
        child
            .stdin
            .take()
            .ok_or_else(|| CommandError::new("git_failed", "Git stdin was unavailable"))?
            .write_all(bytes)
            .map_err(io_error)?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| CommandError::new("git_unavailable", error.to_string()))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(CommandError::new(
            "git_failed",
            String::from_utf8_lossy(&output.stderr),
        ))
    }
}

fn git_status<const N: usize>(path: &Path, args: [&str; N]) -> Result<bool, CommandError> {
    Command::new("git")
        .args(args)
        .current_dir(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .map_err(|error| CommandError::new("git_unavailable", error.to_string()))
}

fn append_local_guidance(root: &Path, files: &mut Vec<String>) {
    for name in ["AGENTS.md", "CLAUDE.md"] {
        if root.join(name).is_file() {
            files.push(name.into());
        }
    }
    collect_markdown(root, &root.join("docs"), files, 0);
}

fn collect_markdown(root: &Path, directory: &Path, files: &mut Vec<String>, depth: usize) {
    if depth > 8 || files.len() >= 10_000 {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            collect_markdown(root, &path, files, depth + 1);
        } else if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("md"))
            && let Ok(relative) = path.strip_prefix(root)
        {
            files.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn git<const N: usize>(path: &Path, args: [&str; N]) -> Result<String, CommandError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .map_err(|error| CommandError::new("git_unavailable", error.to_string()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(CommandError::new(
            "git_failed",
            String::from_utf8_lossy(&output.stderr),
        ))
    }
}

fn io_error(error: std::io::Error) -> CommandError {
    CommandError::new("filesystem_error", error.to_string())
}

#[cfg(windows)]
fn null_device() -> &'static str {
    "NUL"
}
#[cfg(not(windows))]
fn null_device() -> &'static str {
    "/dev/null"
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn repository() -> tempfile::TempDir {
        let directory = tempdir().unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(directory.path())
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(directory.path().join("README.md"), "fixture").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "."])
                .current_dir(directory.path())
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-qm", "fixture"])
                .current_dir(directory.path())
                .status()
                .unwrap()
                .success()
        );
        directory
    }

    #[test]
    fn rereads_status_and_creates_isolated_worktree() {
        let repo = repository();
        let service = GitService::default();
        assert!(!service.status(repo.path()).unwrap().dirty);
        std::fs::write(repo.path().join("README.md"), "changed").unwrap();
        assert!(service.status(repo.path()).unwrap().dirty);
        let destination = repo.path().parent().unwrap().join("fixture-worktree");
        service
            .create_worktree(
                repo.path(),
                &service.status(repo.path()).unwrap().revision.unwrap(),
                &destination,
            )
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(destination.join("README.md")).unwrap(),
            "fixture"
        );
        std::fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    fn reports_non_repository_without_failure() {
        let directory = tempdir().unwrap();
        assert!(
            !GitService::default()
                .status(directory.path())
                .unwrap()
                .is_repository
        );
    }

    #[test]
    fn reports_an_unborn_repository_without_failing() {
        let directory = tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(directory.path())
                .status()
                .unwrap()
                .success()
        );
        let status = GitService::default().status(directory.path()).unwrap();
        assert!(status.is_repository);
        assert!(status.branch.is_some());
        assert!(status.revision.is_none());
    }

    #[test]
    fn includes_ignored_local_guidance_without_exposing_other_ignored_files() {
        let repo = repository();
        std::fs::create_dir_all(repo.path().join("docs/specs")).unwrap();
        std::fs::write(repo.path().join(".gitignore"), "docs/\n.env\nAGENTS.md\n").unwrap();
        std::fs::write(
            repo.path().join("docs/specs/03-coordination.md"),
            "local spec",
        )
        .unwrap();
        std::fs::write(repo.path().join("AGENTS.md"), "local rules").unwrap();
        std::fs::write(repo.path().join(".env"), "SECRET=value").unwrap();
        let files = GitService::default().files(repo.path()).unwrap();
        assert!(files.contains(&"docs/specs/03-coordination.md".into()));
        assert!(files.contains(&"AGENTS.md".into()));
        assert!(!files.contains(&".env".into()));
    }

    #[test]
    fn cancelled_mutation_never_creates_a_worktree() {
        let repo = repository();
        let destination = repo.path().parent().unwrap().join("cancelled-worktree");
        let cancelled = AtomicBool::new(true);
        let error = GitService::default()
            .create_worktree_cancellable(repo.path(), "HEAD", &destination, &cancelled)
            .unwrap_err();
        assert_eq!(error.code, "cancelled");
        assert!(!destination.exists());
    }

    #[test]
    fn reports_tracked_and_untracked_changes() {
        let repo = repository();
        std::fs::write(repo.path().join("README.md"), "changed").unwrap();
        std::fs::write(repo.path().join("new.txt"), "new file").unwrap();
        let diff = GitService::default().diff(repo.path(), "HEAD").unwrap();
        assert_eq!(diff.files, ["README.md", "new.txt"]);
        assert!(diff.patch.contains("changed"));
        assert!(diff.patch.contains("new file"));
    }

    #[test]
    fn exact_diff_keeps_renames_and_binary_content() {
        let repo = repository();
        assert!(
            Command::new("git")
                .args(["mv", "README.md", "renamed.md"])
                .current_dir(repo.path())
                .status()
                .unwrap()
                .success()
        );
        std::fs::write(repo.path().join("asset.bin"), [0, 1, 2, 0, 255]).unwrap();

        let diff = GitService::default()
            .exact_diff(repo.path(), "HEAD")
            .unwrap();

        assert!(diff.files.contains(&"renamed.md".into()));
        assert!(diff.files.contains(&"asset.bin".into()));
        let patch = String::from_utf8_lossy(&diff.patch);
        assert!(patch.contains("rename from README.md"));
        assert!(patch.contains("GIT binary patch"));
    }

    #[test]
    fn integrates_approved_snapshots_before_fast_forwarding_the_checkout() {
        let repo = repository();
        let service = GitService::default();
        let status = service.status(repo.path()).unwrap();
        let base = status.revision.unwrap();
        let target_branch = status.branch.unwrap();
        let parent = repo.path().parent().unwrap();
        let first_worktree = parent.join("first-run");
        let second_worktree = parent.join("second-run");
        service
            .create_worktree(repo.path(), &base, &first_worktree)
            .unwrap();
        service
            .create_worktree(repo.path(), &base, &second_worktree)
            .unwrap();
        std::fs::write(first_worktree.join("frontend.txt"), "frontend").unwrap();
        std::fs::write(second_worktree.join("backend.txt"), "backend").unwrap();
        let first = service.exact_diff(&first_worktree, &base).unwrap();
        let second = service.exact_diff(&second_worktree, &base).unwrap();
        service
            .remove_worktree(repo.path(), &first_worktree)
            .unwrap();
        service
            .remove_worktree(repo.path(), &second_worktree)
            .unwrap();

        let first_revision = service
            .create_snapshot_branch(
                repo.path(),
                &base,
                &parent.join("first-snapshot"),
                "subshell/task/frontend",
                &first.patch,
                "Frontend assignment",
            )
            .unwrap();
        let second_revision = service
            .create_snapshot_branch(
                repo.path(),
                &base,
                &parent.join("second-snapshot"),
                "subshell/task/backend",
                &second.patch,
                "Backend assignment",
            )
            .unwrap();
        let integration = parent.join("integration");
        let integrated = service
            .prepare_integration(
                repo.path(),
                &base,
                &[first_revision, second_revision],
                &integration,
            )
            .unwrap();

        assert!(!repo.path().join("frontend.txt").exists());
        service
            .publish_integration(repo.path(), &target_branch, &base, &integrated)
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.path().join("frontend.txt")).unwrap(),
            "frontend"
        );
        assert_eq!(
            std::fs::read_to_string(repo.path().join("backend.txt")).unwrap(),
            "backend"
        );
        service.remove_worktree(repo.path(), &integration).unwrap();
    }

    #[test]
    fn conflicting_snapshots_leave_the_target_unchanged() {
        let repo = repository();
        let service = GitService::default();
        let base = service.status(repo.path()).unwrap().revision.unwrap();
        let parent = repo.path().parent().unwrap();
        let patches = ["first", "second"].map(|value| {
            let worktree = parent.join(format!("{value}-conflict-run"));
            service
                .create_worktree(repo.path(), &base, &worktree)
                .unwrap();
            std::fs::write(worktree.join("README.md"), value).unwrap();
            let patch = service.exact_diff(&worktree, &base).unwrap().patch;
            service.remove_worktree(repo.path(), &worktree).unwrap();
            patch
        });
        let revisions = patches
            .iter()
            .enumerate()
            .map(|(index, patch)| {
                service
                    .create_snapshot_branch(
                        repo.path(),
                        &base,
                        &parent.join(format!("conflict-snapshot-{index}")),
                        &format!("subshell/conflict/{index}"),
                        patch,
                        "Conflicting assignment",
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();

        let error = service
            .prepare_integration(
                repo.path(),
                &base,
                &revisions,
                &parent.join("conflicting-integration"),
            )
            .unwrap_err();

        assert_eq!(error.code, "merge_conflict");
        assert_eq!(
            error.details,
            Some(serde_json::json!({ "files": ["README.md"] }))
        );
        assert_eq!(
            service.status(repo.path()).unwrap().revision.as_deref(),
            Some(base.as_str())
        );
        assert_eq!(
            std::fs::read_to_string(repo.path().join("README.md")).unwrap(),
            "fixture"
        );
    }
}
