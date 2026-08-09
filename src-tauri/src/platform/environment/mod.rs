use std::{
    collections::HashSet,
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::Serialize;

use crate::contracts::CommandError;

#[derive(Clone)]
pub struct RuntimePaths {
    pub data_dir: PathBuf,
}

#[derive(Clone, Default)]
pub struct PortLeases(Arc<Mutex<HashSet<u16>>>);

impl PortLeases {
    pub fn acquire(&self) -> Result<u16, CommandError> {
        // ponytail: a released ephemeral port can be claimed externally; keep a listener reservation if collisions appear in practice.
        for _ in 0..20 {
            let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(io_error)?;
            let port = listener.local_addr().map_err(io_error)?.port();
            drop(listener);
            let mut ports = self.0.lock().unwrap_or_else(|p| p.into_inner());
            if ports.insert(port) {
                return Ok(port);
            }
        }
        Err(CommandError::new(
            "port_unavailable",
            "Could not reserve a local port",
        ))
    }
    pub fn release(&self, port: u16) {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&port);
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentPreview {
    pub files: Vec<String>,
    pub port: Option<u16>,
}

pub fn validate_files(
    project: &Path,
    files: &[String],
) -> Result<Vec<(String, PathBuf)>, CommandError> {
    let root = project.canonicalize().map_err(io_error)?;
    let mut output = Vec::new();
    for relative in files {
        let candidate = root.join(relative);
        let canonical = candidate.canonicalize().map_err(|_| {
            CommandError::new(
                "invalid_environment_file",
                format!("{relative} does not exist"),
            )
        })?;
        if !canonical.starts_with(&root) || !canonical.is_file() {
            return Err(CommandError::new(
                "invalid_environment_file",
                format!("{relative} leaves the project or is not a file"),
            ));
        }
        let normalized = canonical
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        output.push((normalized, canonical));
    }
    output.sort_by(|a, b| a.0.cmp(&b.0));
    output.dedup_by(|a, b| a.0 == b.0);
    Ok(output)
}

pub fn copy_files(
    project: &Path,
    worktree: &Path,
    files: &[String],
) -> Result<EnvironmentPreview, CommandError> {
    let approved = validate_files(project, files)?;
    for (relative, source) in &approved {
        let destination = worktree.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        fs::copy(source, &destination).map_err(io_error)?;
    }
    Ok(EnvironmentPreview {
        files: approved.into_iter().map(|p| p.0).collect(),
        port: None,
    })
}

pub fn copy_directory(source: &Path, destination: &Path) -> Result<(), CommandError> {
    if !source.exists() {
        fs::create_dir_all(destination).map_err(io_error)?;
        return Ok(());
    }
    fs::create_dir_all(destination).map_err(io_error)?;
    for entry in fs::read_dir(source).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let target = destination.join(entry.file_name());
        if entry.file_type().map_err(io_error)?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else if entry.file_type().map_err(io_error)?.is_file() {
            fs::copy(entry.path(), target).map_err(io_error)?;
        }
    }
    Ok(())
}

fn io_error(error: std::io::Error) -> CommandError {
    CommandError::new("filesystem_error", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    #[test]
    fn copies_only_explicit_files_and_leases_unique_ports() {
        let root = tempdir().unwrap();
        let work = tempdir().unwrap();
        fs::write(root.path().join(".env.example"), "A=1").unwrap();
        fs::write(root.path().join("secret"), "no").unwrap();
        let preview = copy_files(root.path(), work.path(), &[".env.example".into()]).unwrap();
        assert_eq!(preview.files, vec![".env.example"]);
        assert!(!work.path().join("secret").exists());
        let leases = PortLeases::default();
        let a = leases.acquire().unwrap();
        let b = leases.acquire().unwrap();
        assert_ne!(a, b);
        leases.release(a);
        leases.release(a);
    }

    #[test]
    fn rejects_environment_files_outside_the_project() {
        let root = tempdir().unwrap();
        let project = root.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::write(root.path().join("secret"), "no").unwrap();
        let error = validate_files(&project, &["../secret".into()]).unwrap_err();
        assert_eq!(error.code, "invalid_environment_file");
    }
}
