use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
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
        let revision = Some(git(path, ["rev-parse", "HEAD"])?);
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
        files.sort();
        Ok(files)
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
}
