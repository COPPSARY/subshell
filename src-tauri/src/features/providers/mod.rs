#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::{
    contracts::{CommandError, Page},
    platform::{database::Database, environment::RuntimePaths},
};

const KNOWN_PROVIDERS: [(&str, &str, &[&str], &[&str]); 3] = [
    (
        "claude",
        "Claude Code",
        &["--session-id", "{sessionId}", "{prompt}"],
        &["--resume", "{sessionId}"],
    ),
    ("codex", "Codex", &["{prompt}"], &["resume", "--last"]),
    ("gemini", "Gemini CLI", &["-i", "{prompt}"], &[]),
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenericProfile {
    #[serde(default)]
    pub id: String,
    pub display_name: String,
    pub executable_path: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub resume_arguments: Vec<String>,
    pub prompt_mode: String,
    pub config_root_env_var: Option<String>,
    pub config_source_path: Option<String>,
    #[serde(default)]
    pub inherit_user_home: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedProvider {
    pub key: &'static str,
    pub display_name: &'static str,
    pub executable_path: String,
    pub arguments: Vec<String>,
    pub resume_arguments: Vec<String>,
    pub prompt_mode: &'static str,
    pub is_configured: bool,
}

pub struct ResolvedProvider {
    pub id: String,
    pub config_source_path: Option<String>,
    pub config_root_env_var: Option<String>,
    pub inherit_user_home: bool,
    executable_path: String,
    arguments: Vec<String>,
    resume_arguments: Vec<String>,
    prompt_mode: String,
}

impl ResolvedProvider {
    pub fn new_session_id(&self) -> Option<String> {
        self.arguments
            .iter()
            .any(|argument| argument.contains("{sessionId}"))
            .then(|| Uuid::new_v4().to_string())
    }

    pub fn launch_command(
        &self,
        prompt: &str,
        config_root: &Path,
        session_id: Option<&str>,
        full_access: bool,
    ) -> Result<(String, Vec<String>, bool), CommandError> {
        let mut arguments =
            render_arguments(&self.arguments, Some(prompt), config_root, session_id)?;
        self.add_full_access_flag(&mut arguments, full_access)?;
        Ok((
            self.executable_path.clone(),
            arguments,
            self.prompt_mode == "stdin",
        ))
    }

    pub fn resume_command(
        &self,
        config_root: &Path,
        session_id: Option<&str>,
        full_access: bool,
    ) -> Result<(String, Vec<String>, bool), CommandError> {
        if self.resume_arguments.is_empty() {
            return Err(CommandError::new(
                "resume_unsupported",
                "This CLI profile does not support session resume",
            ));
        }
        let mut arguments =
            render_arguments(&self.resume_arguments, None, config_root, session_id)?;
        self.add_full_access_flag(&mut arguments, full_access)?;
        Ok((self.executable_path.clone(), arguments, false))
    }

    pub fn can_resume(&self) -> bool {
        !self.resume_arguments.is_empty()
    }

    pub fn ensure_full_access_supported(&self) -> Result<(), CommandError> {
        self.full_access_flag().map(|_| ())
    }

    fn add_full_access_flag(
        &self,
        arguments: &mut Vec<String>,
        full_access: bool,
    ) -> Result<(), CommandError> {
        if full_access {
            let flag = self.full_access_flag()?;
            if !arguments.iter().any(|argument| argument == flag) {
                arguments.insert(0, flag.into());
            }
        }
        Ok(())
    }

    fn full_access_flag(&self) -> Result<&'static str, CommandError> {
        let executable = Path::new(&self.executable_path)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match executable.as_str() {
            "claude" => Ok("--dangerously-skip-permissions"),
            "codex" => Ok("--dangerously-bypass-approvals-and-sandbox"),
            "gemini" => Ok("--approval-mode=yolo"),
            _ => Err(CommandError::new(
                "full_access_unsupported",
                "Full permissions are supported only for detected Claude Code, Codex, and Gemini CLI profiles",
            )),
        }
    }
}

fn render_arguments(
    template: &[String],
    prompt: Option<&str>,
    config_root: &Path,
    session_id: Option<&str>,
) -> Result<Vec<String>, CommandError> {
    if template
        .iter()
        .any(|argument| argument.contains("{sessionId}"))
        && session_id.is_none()
    {
        return Err(CommandError::new(
            "provider_session_missing",
            "The provider session identifier is unavailable",
        ));
    }
    let root = config_root.to_string_lossy();
    Ok(template
        .iter()
        .map(|argument| {
            argument
                .replace("{prompt}", prompt.unwrap_or_default())
                .replace("{configRoot}", &root)
                .replace("{sessionId}", session_id.unwrap_or_default())
        })
        .collect())
}

#[derive(Deserialize)]
pub struct ProfileId {
    pub id: String,
}

#[tauri::command]
pub fn providers_create_generic(
    mut input: GenericProfile,
    database: State<Database>,
    paths: State<RuntimePaths>,
) -> Result<GenericProfile, CommandError> {
    input.id = Uuid::new_v4().to_string();
    save(&input, &database, &paths)?;
    Ok(input)
}
#[tauri::command]
pub fn providers_update_generic(
    input: GenericProfile,
    database: State<Database>,
    paths: State<RuntimePaths>,
) -> Result<GenericProfile, CommandError> {
    save(&input, &database, &paths)?;
    Ok(input)
}
#[tauri::command]
pub fn providers_remove(input: ProfileId, database: State<Database>) -> Result<(), CommandError> {
    database.connect()?.execute("UPDATE provider_accounts SET status='revoked',removed_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",[input.id])?;
    Ok(())
}
#[tauri::command]
pub fn providers_list(database: State<Database>) -> Result<Page<GenericProfile>, CommandError> {
    Ok(Page::first(list(&database)?))
}

#[tauri::command]
pub fn providers_detect(database: State<Database>) -> Result<Vec<DetectedProvider>, CommandError> {
    detect(&database)
}

pub(crate) fn save(
    profile: &GenericProfile,
    database: &Database,
    paths: &RuntimePaths,
) -> Result<(), CommandError> {
    validate(profile)?;
    let connection = database.connect()?;
    let scope = profile.config_source_path.clone().unwrap_or_else(|| {
        paths
            .data_dir
            .join("provider-profiles")
            .join(&profile.id)
            .to_string_lossy()
            .into_owned()
    });
    fs::create_dir_all(&scope).map_err(io_error)?;
    connection.execute("INSERT INTO provider_accounts(id,provider_type,display_name,config_scope_path,status,created_at,updated_at,removed_at) VALUES(?1,'generic',?2,?3,'active',strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'),NULL) ON CONFLICT(id) DO UPDATE SET display_name=excluded.display_name,config_scope_path=excluded.config_scope_path,status='active',updated_at=excluded.updated_at,removed_at=NULL",params![profile.id,profile.display_name,scope])?;
    connection.execute("INSERT INTO generic_provider_profiles(provider_account_id,executable_path,arguments_json,resume_arguments_json,prompt_mode,config_root_env_var,inherit_user_home) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(provider_account_id) DO UPDATE SET executable_path=excluded.executable_path,arguments_json=excluded.arguments_json,resume_arguments_json=excluded.resume_arguments_json,prompt_mode=excluded.prompt_mode,config_root_env_var=excluded.config_root_env_var,inherit_user_home=excluded.inherit_user_home",params![profile.id,profile.executable_path,serde_json::to_string(&profile.arguments).unwrap(),serde_json::to_string(&profile.resume_arguments).unwrap(),profile.prompt_mode,profile.config_root_env_var,profile.inherit_user_home])?;
    Ok(())
}

fn validate(profile: &GenericProfile) -> Result<(), CommandError> {
    if profile.id.is_empty() || profile.display_name.trim().is_empty() {
        return Err(CommandError::new(
            "invalid_profile",
            "Profile name is required",
        ));
    }
    let executable = Path::new(&profile.executable_path);
    if !executable.is_absolute() || !executable.is_file() {
        return Err(CommandError::new(
            "invalid_executable",
            "Choose an absolute executable file",
        ));
    }
    #[cfg(unix)]
    if std::fs::metadata(executable)
        .map_err(io_error)?
        .permissions()
        .mode()
        & 0o111
        == 0
    {
        return Err(CommandError::new(
            "invalid_executable",
            "The selected file is not executable",
        ));
    }
    let prompt_count = profile
        .arguments
        .iter()
        .filter(|arg| arg.as_str() == "{prompt}")
        .count();
    if (profile.prompt_mode == "argument" && prompt_count != 1)
        || (profile.prompt_mode == "stdin" && prompt_count != 0)
    {
        return Err(CommandError::new(
            "invalid_arguments",
            "Argument mode requires one {prompt} token; stdin mode forbids it",
        ));
    }
    if !matches!(profile.prompt_mode.as_str(), "argument" | "stdin") {
        return Err(CommandError::new(
            "invalid_prompt_mode",
            "Prompt mode must be argument or stdin",
        ));
    }
    for arg in &profile.arguments {
        let clean = arg
            .replace("{prompt}", "")
            .replace("{configRoot}", "")
            .replace("{sessionId}", "");
        if clean.contains('{') || clean.contains('}') {
            return Err(CommandError::new(
                "invalid_arguments",
                "Only {prompt}, {configRoot}, and {sessionId} placeholders are supported",
            ));
        }
    }
    for arg in &profile.resume_arguments {
        let clean = arg.replace("{sessionId}", "").replace("{configRoot}", "");
        if clean.contains('{') || clean.contains('}') || arg.contains("{prompt}") {
            return Err(CommandError::new(
                "invalid_arguments",
                "Resume arguments support only {sessionId} and {configRoot}",
            ));
        }
    }
    if profile
        .resume_arguments
        .iter()
        .any(|argument| argument.contains("{sessionId}"))
        && !profile
            .arguments
            .iter()
            .any(|argument| argument.contains("{sessionId}"))
    {
        return Err(CommandError::new(
            "invalid_arguments",
            "A resumable session ID must also be assigned at launch",
        ));
    }
    if let Some(name) = &profile.config_root_env_var
        && (name.contains('=') || name.contains('\0') || name.trim().is_empty())
    {
        return Err(CommandError::new(
            "invalid_environment_variable",
            "Config environment variable is invalid",
        ));
    }
    if let Some(source) = &profile.config_source_path
        && !Path::new(source).is_dir()
    {
        return Err(CommandError::new(
            "invalid_config_source",
            "Config template must be a directory",
        ));
    }
    Ok(())
}

pub fn get(database: &Database, id: &str) -> Result<GenericProfile, CommandError> {
    list(database)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| CommandError::new("provider_not_found", "Provider profile was not found"))
}

pub fn resolve(database: &Database, id: &str) -> Result<ResolvedProvider, CommandError> {
    let profile = get(database, id)?;
    Ok(ResolvedProvider {
        id: profile.id,
        executable_path: profile.executable_path,
        arguments: profile.arguments,
        resume_arguments: profile.resume_arguments,
        prompt_mode: profile.prompt_mode,
        config_root_env_var: profile.config_root_env_var,
        config_source_path: profile.config_source_path,
        inherit_user_home: profile.inherit_user_home,
    })
}
pub(crate) fn list(database: &Database) -> Result<Vec<GenericProfile>, CommandError> {
    let connection = database.connect()?;
    let mut statement=connection.prepare("SELECT a.id,a.display_name,g.executable_path,g.arguments_json,g.resume_arguments_json,g.prompt_mode,g.config_root_env_var,a.config_scope_path,g.inherit_user_home FROM provider_accounts a JOIN generic_provider_profiles g ON g.provider_account_id=a.id WHERE a.removed_at IS NULL ORDER BY a.display_name")?;
    statement
        .query_map([], |row| {
            Ok(GenericProfile {
                id: row.get(0)?,
                display_name: row.get(1)?,
                executable_path: row.get(2)?,
                arguments: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
                resume_arguments: serde_json::from_str(&row.get::<_, String>(4)?)
                    .unwrap_or_default(),
                prompt_mode: row.get(5)?,
                config_root_env_var: row.get(6)?,
                config_source_path: row.get(7)?,
                inherit_user_home: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn detect(database: &Database) -> Result<Vec<DetectedProvider>, CommandError> {
    let configured = list(database)?;
    Ok(KNOWN_PROVIDERS
        .into_iter()
        .filter_map(|(key, display_name, arguments, resume_arguments)| {
            let executable = find_executable(key)?;
            let executable_path = executable.to_string_lossy().into_owned();
            Some(DetectedProvider {
                key,
                display_name,
                is_configured: configured
                    .iter()
                    .any(|profile| profile.executable_path == executable_path),
                executable_path,
                arguments: arguments
                    .iter()
                    .map(|argument| (*argument).into())
                    .collect(),
                resume_arguments: resume_arguments
                    .iter()
                    .map(|argument| (*argument).into())
                    .collect(),
                prompt_mode: "argument",
            })
        })
        .collect())
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let mut directories = env::var_os("PATH")
        .map(|path| env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        directories.extend([
            home.join(".local/bin"),
            home.join(".cargo/bin"),
            home.join(".npm-global/bin"),
            home.join(".bun/bin"),
        ]);
    }
    directories.sort();
    directories.dedup();
    for directory in directories {
        let candidate = directory.join(name);
        if is_executable(&candidate) {
            return candidate.canonicalize().ok();
        }
        #[cfg(windows)]
        for extension in env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
        {
            let candidate = directory.join(format!("{name}{extension}"));
            if is_executable(&candidate) {
                return candidate.canonicalize().ok();
            }
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    return fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0);
    #[cfg(not(unix))]
    true
}
fn io_error(error: std::io::Error) -> CommandError {
    CommandError::new("filesystem_error", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    #[test]
    fn prompt_is_a_single_argv_token_not_a_shell() {
        let dir = tempdir().unwrap();
        let exe = std::env::current_exe().unwrap();
        let p = GenericProfile {
            id: "p".into(),
            display_name: "P".into(),
            executable_path: exe.to_string_lossy().into(),
            arguments: vec!["--prompt".into(), "{prompt}".into()],
            resume_arguments: vec![],
            prompt_mode: "argument".into(),
            config_root_env_var: None,
            config_source_path: None,
            inherit_user_home: false,
        };
        validate(&p).unwrap();
        let resolved = ResolvedProvider {
            id: p.id,
            executable_path: p.executable_path,
            arguments: p.arguments,
            resume_arguments: p.resume_arguments,
            prompt_mode: p.prompt_mode,
            config_root_env_var: None,
            config_source_path: None,
            inherit_user_home: false,
        };
        let (_, args, _) = resolved
            .launch_command("hello; touch /tmp/nope", dir.path(), None, false)
            .unwrap();
        assert_eq!(args[1], "hello; touch /tmp/nope");
    }

    #[cfg(unix)]
    #[test]
    fn finds_an_installed_cli_without_launching_it() {
        assert!(find_executable("sh").is_some());
        assert!(find_executable("subshell-not-a-real-cli").is_none());
    }

    #[test]
    fn detected_provider_recipes_start_interactive_sessions() {
        assert_eq!(
            KNOWN_PROVIDERS[0].2,
            ["--session-id", "{sessionId}", "{prompt}"]
        );
        assert_eq!(KNOWN_PROVIDERS[0].3, ["--resume", "{sessionId}"]);
        assert_eq!(KNOWN_PROVIDERS[1].2, ["{prompt}"]);
        assert_eq!(KNOWN_PROVIDERS[1].3, ["resume", "--last"]);
        assert_eq!(KNOWN_PROVIDERS[2].2, ["-i", "{prompt}"]);
    }

    #[test]
    fn session_templates_resume_without_replaying_the_prompt() {
        let root = tempdir().unwrap();
        let resolved = ResolvedProvider {
            id: "claude".into(),
            executable_path: "/usr/bin/claude".into(),
            arguments: vec![
                "--session-id".into(),
                "{sessionId}".into(),
                "{prompt}".into(),
            ],
            resume_arguments: vec!["--resume".into(), "{sessionId}".into()],
            prompt_mode: "argument".into(),
            config_root_env_var: None,
            config_source_path: None,
            inherit_user_home: true,
        };
        let session_id = resolved.new_session_id().unwrap();
        let (_, launch, _) = resolved
            .launch_command("full context", root.path(), Some(&session_id), false)
            .unwrap();
        let (_, resume, _) = resolved
            .resume_command(root.path(), Some(&session_id), false)
            .unwrap();
        assert_eq!(launch, ["--session-id", &session_id, "full context"]);
        assert_eq!(resume, ["--resume", &session_id]);
        assert!(!resume.iter().any(|argument| argument == "full context"));
    }

    #[test]
    fn full_access_uses_each_supported_cli_native_flag() {
        let root = tempdir().unwrap();
        for (executable, expected) in [
            ("/usr/bin/claude", "--dangerously-skip-permissions"),
            (
                "/usr/bin/codex",
                "--dangerously-bypass-approvals-and-sandbox",
            ),
            ("/usr/bin/gemini", "--approval-mode=yolo"),
        ] {
            let resolved = ResolvedProvider {
                id: executable.into(),
                executable_path: executable.into(),
                arguments: vec!["{prompt}".into()],
                resume_arguments: vec!["resume".into()],
                prompt_mode: "argument".into(),
                config_root_env_var: None,
                config_source_path: None,
                inherit_user_home: true,
            };
            let (_, launch, _) = resolved
                .launch_command("work", root.path(), None, true)
                .unwrap();
            let (_, resume, _) = resolved.resume_command(root.path(), None, true).unwrap();
            assert_eq!(launch.first().map(String::as_str), Some(expected));
            assert_eq!(resume.first().map(String::as_str), Some(expected));
        }
    }
}
