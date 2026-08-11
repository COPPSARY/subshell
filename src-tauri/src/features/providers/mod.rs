pub mod adapters;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tauri::{State, ipc::Channel};
use uuid::Uuid;

use crate::{
    contracts::{CommandError, Page},
    platform::{
        database::Database,
        environment::RuntimePaths,
        keychain::{SecretStore, SystemSecretStore},
        process::{ProcessNotice, ProcessSpec, ProcessSupervisor},
    },
};

const DEFAULT_PROVIDER_SETTING: &str = "default_provider_account_id";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenericProfile {
    #[serde(default)]
    pub id: String,
    pub display_name: String,
    #[serde(default = "generic_provider_type")]
    pub provider_type: String,
    #[serde(default = "active_status")]
    pub status: String,
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
    pub config_root_env_var: Option<&'static str>,
    pub auth_probe_arguments: Vec<String>,
    pub capabilities: adapters::ProviderCapabilities,
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
    adapter: Option<&'static dyn adapters::ProviderAdapter>,
}

impl ResolvedProvider {
    pub fn secret_environment_key(&self) -> Option<&'static str> {
        self.adapter.and_then(|adapter| adapter.secret_env_var())
    }

    pub fn parse_output(&self, output: &[u8]) -> adapters::ParsedOutput {
        self.adapter
            .map_or_else(Default::default, |adapter| adapter.parse_output(output))
    }

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

    pub fn runtime_config_root(&self, fallback: PathBuf) -> PathBuf {
        if self.adapter.is_some_and(|adapter| adapter.key() == "codex")
            && !self.inherit_user_home
            && let Some(scope) = &self.config_source_path
        {
            return PathBuf::from(scope);
        }
        fallback
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
        self.adapter
            .map(|adapter| adapter.full_access_flag())
            .ok_or_else(|| {
                CommandError::new(
                    "full_access_unsupported",
                    "This provider profile does not support full permissions",
                )
            })
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSecretInput {
    pub id: String,
    pub secret: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexLoginInput {
    pub id: String,
    pub method: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProviderAuthEvent {
    Output {
        text: String,
    },
    Finished {
        success: bool,
        account: Option<Box<GenericProfile>>,
        message: String,
    },
}

#[tauri::command]
pub fn providers_create_generic(
    mut input: GenericProfile,
    database: State<Database>,
    paths: State<RuntimePaths>,
) -> Result<GenericProfile, CommandError> {
    input.id = Uuid::new_v4().to_string();
    save(&input, &database, &paths)?;
    get(&database, &input.id)
}
#[tauri::command]
pub fn providers_update_generic(
    input: GenericProfile,
    database: State<Database>,
    paths: State<RuntimePaths>,
) -> Result<GenericProfile, CommandError> {
    save(&input, &database, &paths)?;
    get(&database, &input.id)
}
#[tauri::command]
pub fn providers_remove(
    input: ProfileId,
    database: State<Database>,
    secrets: State<SystemSecretStore>,
) -> Result<(), CommandError> {
    let profile = get(&database, &input.id)?;
    if profile.provider_type == "codex" && !profile.inherit_user_home {
        logout_codex(&database, &input.id)?;
    }
    remove(&database, &*secrets, &input.id)
}

#[tauri::command]
pub fn providers_reauthenticate(
    input: ProviderSecretInput,
    database: State<Database>,
    secrets: State<SystemSecretStore>,
) -> Result<GenericProfile, CommandError> {
    reauthenticate(&database, &*secrets, &input.id, &input.secret)?;
    get(&database, &input.id)
}
#[tauri::command]
pub fn providers_list(database: State<Database>) -> Result<Page<GenericProfile>, CommandError> {
    Ok(Page::first(list(&database)?))
}

#[tauri::command]
pub fn providers_detect(database: State<Database>) -> Result<Vec<DetectedProvider>, CommandError> {
    detect(&database)
}

#[tauri::command]
pub fn providers_default(database: State<Database>) -> Result<Option<String>, CommandError> {
    default_provider(&database)
}

#[tauri::command]
pub fn providers_set_default(
    input: ProfileId,
    database: State<Database>,
) -> Result<String, CommandError> {
    set_default(&database, &input.id)
}

#[tauri::command]
pub fn providers_codex_login(
    input: CodexLoginInput,
    on_event: Channel<ProviderAuthEvent>,
    database: State<Database>,
    paths: State<RuntimePaths>,
    processes: State<ProcessSupervisor>,
) -> Result<(), CommandError> {
    start_codex_login(
        &database,
        &paths,
        &processes,
        &input.id,
        &input.method,
        Arc::new(move |event| {
            let _ = on_event.send(event);
        }),
    )
}

#[tauri::command]
pub fn providers_codex_logout(
    input: ProfileId,
    database: State<Database>,
) -> Result<GenericProfile, CommandError> {
    logout_codex(&database, &input.id)
}

#[tauri::command]
pub fn providers_codex_login_stop(
    input: ProfileId,
    processes: State<ProcessSupervisor>,
) -> Result<(), CommandError> {
    processes.stop(&auth_process_id(&input.id))
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
    let initial_status = if profile.provider_type == "codex" && !profile.inherit_user_home {
        "needs_reauth"
    } else {
        "active"
    };
    connection.execute("INSERT INTO provider_accounts(id,provider_type,display_name,config_scope_path,status,created_at,updated_at,removed_at) VALUES(?1,?2,?3,?4,?5,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'),NULL) ON CONFLICT(id) DO UPDATE SET provider_type=excluded.provider_type,display_name=excluded.display_name,config_scope_path=excluded.config_scope_path,updated_at=excluded.updated_at,removed_at=NULL",params![profile.id,profile.provider_type,profile.display_name,scope,initial_status])?;
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
    if profile.status != "active" {
        return Err(CommandError::new(
            "provider_reauth_required",
            "Provider account requires reauthentication before starting a run",
        ));
    }
    let adapter = adapters::by_key(&profile.provider_type);
    Ok(ResolvedProvider {
        id: profile.id,
        executable_path: profile.executable_path,
        arguments: profile.arguments,
        resume_arguments: profile.resume_arguments,
        prompt_mode: profile.prompt_mode,
        config_root_env_var: profile.config_root_env_var,
        config_source_path: profile.config_source_path,
        inherit_user_home: profile.inherit_user_home,
        adapter,
    })
}
pub(crate) fn list(database: &Database) -> Result<Vec<GenericProfile>, CommandError> {
    let connection = database.connect()?;
    let mut statement=connection.prepare("SELECT a.id,a.display_name,a.provider_type,a.status,g.executable_path,g.arguments_json,g.resume_arguments_json,g.prompt_mode,g.config_root_env_var,a.config_scope_path,g.inherit_user_home FROM provider_accounts a JOIN generic_provider_profiles g ON g.provider_account_id=a.id WHERE a.removed_at IS NULL ORDER BY CASE WHEN a.status='active' AND a.id=(SELECT value FROM app_settings WHERE key='default_provider_account_id') THEN 0 WHEN a.status='active' THEN 1 ELSE 2 END,a.display_name")?;
    statement
        .query_map([], |row| {
            Ok(GenericProfile {
                id: row.get(0)?,
                display_name: row.get(1)?,
                provider_type: row.get(2)?,
                status: row.get(3)?,
                executable_path: row.get(4)?,
                arguments: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default(),
                resume_arguments: serde_json::from_str(&row.get::<_, String>(6)?)
                    .unwrap_or_default(),
                prompt_mode: row.get(7)?,
                config_root_env_var: row.get(8)?,
                config_source_path: row.get(9)?,
                inherit_user_home: row.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn reauthenticate(
    database: &Database,
    secrets: &dyn SecretStore,
    account_id: &str,
    secret: &str,
) -> Result<(), CommandError> {
    if secret.is_empty() {
        return Err(CommandError::new(
            "invalid_secret",
            "Credential is required",
        ));
    }
    let connection = database.connect()?;
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM provider_accounts WHERE id=?1 AND removed_at IS NULL)",
        [account_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(CommandError::new(
            "provider_not_found",
            "Provider account was not found",
        ));
    }
    secrets.set(account_id, secret.as_bytes())?;
    connection.execute(
        "UPDATE provider_accounts SET status='active',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
        [account_id],
    )?;
    Ok(())
}

fn remove(
    database: &Database,
    secrets: &dyn SecretStore,
    account_id: &str,
) -> Result<(), CommandError> {
    let connection = database.connect()?;
    let active: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_runs WHERE provider_account_id=?1 AND status IN('queued','preparing','running','waiting'))",
        [account_id],
        |row| row.get(0),
    )?;
    if active {
        return Err(CommandError::new(
            "provider_account_in_use",
            "Stop active runs before removing this provider account",
        ));
    }
    secrets.delete(account_id)?;
    let changed = connection.execute("UPDATE provider_accounts SET status='revoked',removed_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1 AND removed_at IS NULL",[account_id])?;
    if changed == 0 {
        return Err(CommandError::new(
            "provider_not_found",
            "Provider account was not found",
        ));
    }
    connection.execute(
        "DELETE FROM app_settings WHERE key=?1 AND value=?2",
        params![DEFAULT_PROVIDER_SETTING, account_id],
    )?;
    Ok(())
}

fn default_provider(database: &Database) -> Result<Option<String>, CommandError> {
    let configured = database
        .connect()?
        .query_row(
            "SELECT settings.value FROM app_settings settings WHERE settings.key=?1 AND EXISTS(SELECT 1 FROM provider_accounts accounts WHERE accounts.id=settings.value AND accounts.status='active' AND accounts.removed_at IS NULL)",
            [DEFAULT_PROVIDER_SETTING],
            |row| row.get(0),
        )
        .optional()?;
    if configured.is_some() {
        return Ok(configured);
    }
    Ok(list(database)?
        .into_iter()
        .find(|profile| profile.status == "active")
        .map(|profile| profile.id))
}

fn set_default(database: &Database, account_id: &str) -> Result<String, CommandError> {
    let active = database.connect()?.query_row(
        "SELECT EXISTS(SELECT 1 FROM provider_accounts WHERE id=?1 AND status='active' AND removed_at IS NULL)",
        [account_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !active {
        return Err(CommandError::new(
            "provider_not_ready",
            "Link or reauthenticate this account before using it for new goals",
        ));
    }
    database.connect()?.execute(
        "INSERT INTO app_settings(key,value,updated_at) VALUES(?1,?2,strftime('%Y-%m-%dT%H:%M:%fZ','now')) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",
        params![DEFAULT_PROVIDER_SETTING, account_id],
    )?;
    Ok(account_id.to_owned())
}

fn start_codex_login(
    database: &Database,
    paths: &RuntimePaths,
    processes: &ProcessSupervisor,
    account_id: &str,
    method: &str,
    sink: Arc<dyn Fn(ProviderAuthEvent) + Send + Sync>,
) -> Result<(), CommandError> {
    ensure_account_idle(database, account_id)?;
    let profile = get(database, account_id)?;
    if profile.provider_type != "codex" || profile.inherit_user_home {
        return Err(CommandError::new(
            "codex_account_required",
            "Account linking currently supports isolated Codex profiles only",
        ));
    }
    let arguments = match method {
        "browser" => codex_auth_arguments(&["login"]),
        "device" => codex_auth_arguments(&["login", "--device-auth"]),
        _ => {
            return Err(CommandError::new(
                "invalid_login_method",
                "Choose browser or device-code login",
            ));
        }
    };
    let process_id = auth_process_id(account_id);
    if processes.is_active(&process_id) {
        return Err(CommandError::new(
            "provider_login_active",
            "This Codex account is already signing in",
        ));
    }
    let scope = PathBuf::from(profile.config_source_path.as_deref().ok_or_else(|| {
        CommandError::new(
            "provider_config_missing",
            "Codex account storage is unavailable",
        )
    })?);
    ensure_codex_keyring_config(&scope)?;
    let log_path = paths
        .data_dir
        .join("provider-auth")
        .join(format!("{account_id}.log"));
    if log_path.exists() {
        fs::remove_file(&log_path).map_err(io_error)?;
    }
    let executable = profile.executable_path.clone();
    let environment = codex_auth_environment(&scope);
    let database = database.clone();
    let verification_executable = executable.clone();
    let verification_environment = environment.clone();
    let verification_scope = scope.clone();
    let completion_log = log_path.clone();
    let completion_account_id = account_id.to_string();
    processes.launch(
        process_id,
        ProcessSpec {
            executable,
            arguments,
            cwd: scope,
            environment,
            log_path,
            stdin: None,
            redactions: Vec::new(),
        },
        Arc::new(move |notice| match notice {
            ProcessNotice::Output { bytes, .. } => sink(ProviderAuthEvent::Output {
                text: String::from_utf8_lossy(&bytes).into_owned(),
            }),
            ProcessNotice::Exited { success, .. } => {
                let authenticated = success
                    && codex_login_is_valid(
                        &verification_executable,
                        &verification_scope,
                        &verification_environment,
                    );
                let message = if authenticated {
                    "Codex account linked".to_string()
                } else {
                    "Codex sign-in did not complete. Start a fresh login and try again.".to_string()
                };
                let account = update_auth_status(&database, &completion_account_id, authenticated)
                    .and_then(|()| get(&database, &completion_account_id))
                    .ok();
                let _ = fs::remove_file(&completion_log);
                sink(ProviderAuthEvent::Finished {
                    success: authenticated,
                    account: account.map(Box::new),
                    message,
                });
            }
        }),
    )?;
    Ok(())
}

fn logout_codex(database: &Database, account_id: &str) -> Result<GenericProfile, CommandError> {
    ensure_account_idle(database, account_id)?;
    let profile = get(database, account_id)?;
    if profile.provider_type != "codex" || profile.inherit_user_home {
        return Err(CommandError::new(
            "codex_account_required",
            "Account linking currently supports isolated Codex profiles only",
        ));
    }
    let scope = PathBuf::from(profile.config_source_path.as_deref().ok_or_else(|| {
        CommandError::new(
            "provider_config_missing",
            "Codex account storage is unavailable",
        )
    })?);
    ensure_codex_keyring_config(&scope)?;
    let status = codex_command(&profile.executable_path, &scope, &["logout"])?;
    if !status.success() {
        return Err(CommandError::new(
            "provider_logout_failed",
            "Codex could not clear this account from the OS keychain",
        ));
    }
    update_auth_status(database, account_id, false)?;
    get(database, account_id)
}

fn ensure_account_idle(database: &Database, account_id: &str) -> Result<(), CommandError> {
    let active: bool = database.connect()?.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_runs WHERE provider_account_id=?1 AND status IN('queued','preparing','running','waiting'))",
        [account_id],
        |row| row.get(0),
    )?;
    if active {
        return Err(CommandError::new(
            "provider_account_in_use",
            "Stop active runs before changing this Codex login",
        ));
    }
    Ok(())
}

fn update_auth_status(
    database: &Database,
    account_id: &str,
    authenticated: bool,
) -> Result<(), CommandError> {
    database.connect()?.execute(
        "UPDATE provider_accounts SET status=?1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2 AND removed_at IS NULL",
        params![if authenticated { "active" } else { "needs_reauth" }, account_id],
    )?;
    Ok(())
}

fn ensure_codex_keyring_config(scope: &Path) -> Result<(), CommandError> {
    fs::create_dir_all(scope).map_err(io_error)?;
    let path = scope.join("config.toml");
    let existing = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(io_error(error)),
    };
    let setting = "cli_auth_credentials_store = \"keyring\"";
    let mut in_root = true;
    let mut replaced = false;
    let mut lines = Vec::new();
    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_root = false;
        }
        if in_root
            && trimmed
                .split_once('=')
                .is_some_and(|(key, _)| key.trim() == "cli_auth_credentials_store")
        {
            lines.push(setting.to_string());
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }
    let content = if replaced {
        format!("{}\n", lines.join("\n"))
    } else if existing.is_empty() {
        format!("{setting}\n")
    } else {
        format!("{setting}\n{existing}")
    };
    fs::write(path, content).map_err(io_error)
}

fn codex_auth_arguments(arguments: &[&str]) -> Vec<String> {
    let mut values = vec!["-c".into(), "cli_auth_credentials_store=\"keyring\"".into()];
    values.extend(arguments.iter().map(|argument| (*argument).into()));
    values
}

fn codex_auth_environment(scope: &Path) -> Vec<(String, String)> {
    let mut values = [
        "PATH",
        "LANG",
        "LC_ALL",
        "TERM",
        "COLORTERM",
        "HOME",
        "USER",
        "LOGNAME",
        "SHELL",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "BROWSER",
        "SYSTEMROOT",
        "WINDIR",
        "PATHEXT",
        "APPDATA",
        "LOCALAPPDATA",
        "USERPROFILE",
    ]
    .into_iter()
    .filter_map(|key| env::var(key).ok().map(|value| (key.into(), value)))
    .collect::<Vec<_>>();
    values.push(("CODEX_HOME".into(), scope.to_string_lossy().into_owned()));
    values.push(("NO_COLOR".into(), "1".into()));
    values
}

fn codex_command(
    executable: &str,
    scope: &Path,
    arguments: &[&str],
) -> Result<std::process::ExitStatus, CommandError> {
    let mut command = Command::new(executable);
    command
        .args(codex_auth_arguments(arguments))
        .current_dir(scope)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, value) in codex_auth_environment(scope) {
        command.env(key, value);
    }
    command.status().map_err(io_error)
}

fn codex_login_is_valid(executable: &str, scope: &Path, environment: &[(String, String)]) -> bool {
    let mut command = Command::new(executable);
    command
        .args(codex_auth_arguments(&["login", "status"]))
        .current_dir(scope)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, value) in environment {
        command.env(key, value);
    }
    command.status().is_ok_and(|status| status.success())
}

fn auth_process_id(account_id: &str) -> String {
    format!("provider-auth:{account_id}")
}

fn generic_provider_type() -> String {
    "generic".into()
}

fn active_status() -> String {
    "active".into()
}

fn detect(database: &Database) -> Result<Vec<DetectedProvider>, CommandError> {
    let configured = list(database)?;
    Ok(adapters::ADAPTERS
        .iter()
        .filter_map(|adapter| {
            let executable = find_executable(adapter.executable())?;
            let executable_path = executable.to_string_lossy().into_owned();
            Some(DetectedProvider {
                key: adapter.key(),
                display_name: adapter.display_name(),
                is_configured: configured
                    .iter()
                    .any(|profile| profile.executable_path == executable_path),
                executable_path,
                arguments: adapter
                    .launch_arguments()
                    .iter()
                    .map(|argument| (*argument).into())
                    .collect(),
                resume_arguments: adapter
                    .resume_arguments()
                    .iter()
                    .map(|argument| (*argument).into())
                    .collect(),
                prompt_mode: "argument",
                config_root_env_var: adapter.config_root_env_var(),
                auth_probe_arguments: adapter
                    .auth_probe_arguments()
                    .iter()
                    .map(|argument| (*argument).into())
                    .collect(),
                capabilities: adapter.capabilities(),
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
    use super::adapters::ProviderAdapter;
    use super::*;
    use crate::platform::keychain::MemorySecretStore;
    use tempfile::tempdir;
    #[test]
    fn prompt_is_a_single_argv_token_not_a_shell() {
        let dir = tempdir().unwrap();
        let exe = std::env::current_exe().unwrap();
        let p = GenericProfile {
            id: "p".into(),
            display_name: "P".into(),
            provider_type: "generic".into(),
            status: "active".into(),
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
            adapter: None,
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
            adapters::CLAUDE.launch_arguments(),
            ["--session-id", "{sessionId}", "{prompt}"]
        );
        assert_eq!(adapters::CODEX.resume_arguments(), ["resume", "--last"]);
        assert_eq!(adapters::KIRO.launch_arguments(), ["chat", "{prompt}"]);
        assert_eq!(adapters::GEMINI.launch_arguments(), ["-i", "{prompt}"]);
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
            adapter: adapters::by_key("claude"),
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
        for (provider_type, executable, expected) in [
            (
                "claude",
                "/usr/bin/claude",
                "--dangerously-skip-permissions",
            ),
            (
                "codex",
                "/usr/bin/codex",
                "--dangerously-bypass-approvals-and-sandbox",
            ),
            ("kiro", "/usr/bin/kiro-cli", "--trust-all-tools"),
            ("gemini", "/usr/bin/gemini", "--approval-mode=yolo"),
        ] {
            let adapter = adapters::by_key(provider_type).unwrap();
            let resolved = ResolvedProvider {
                id: executable.into(),
                executable_path: executable.into(),
                arguments: vec!["{prompt}".into()],
                resume_arguments: vec!["resume".into()],
                prompt_mode: "argument".into(),
                config_root_env_var: None,
                config_source_path: None,
                inherit_user_home: true,
                adapter: Some(adapter),
            };
            let (_, launch, _) = resolved
                .launch_command("work", root.path(), None, true)
                .unwrap();
            let (_, resume, _) = resolved.resume_command(root.path(), None, true).unwrap();
            assert_eq!(launch.first().map(String::as_str), Some(expected));
            assert_eq!(resume.first().map(String::as_str), Some(expected));
        }
    }

    #[test]
    fn account_secrets_stay_out_of_sqlite_and_live_runs_block_removal() {
        let root = tempdir().unwrap();
        let database_path = root.path().join("db.sqlite3");
        let database = Database::initialize(&database_path).unwrap();
        let paths = RuntimePaths {
            data_dir: root.path().join("data"),
        };
        let profile = GenericProfile {
            id: "account".into(),
            display_name: "Claude account".into(),
            provider_type: "claude".into(),
            status: "active".into(),
            executable_path: std::env::current_exe().unwrap().to_string_lossy().into(),
            arguments: vec!["{prompt}".into()],
            resume_arguments: vec![],
            prompt_mode: "argument".into(),
            config_root_env_var: Some("CLAUDE_CONFIG_DIR".into()),
            config_source_path: None,
            inherit_user_home: false,
        };
        save(&profile, &database, &paths).unwrap();
        database
            .connect()
            .unwrap()
            .execute(
                "UPDATE provider_accounts SET status='needs_reauth' WHERE id='account'",
                [],
            )
            .unwrap();
        let secrets = MemorySecretStore::default();
        reauthenticate(&database, &secrets, "account", "secret-marker").unwrap();
        assert_eq!(
            secrets.get("account").unwrap(),
            Some(b"secret-marker".to_vec())
        );
        assert_eq!(get(&database, "account").unwrap().status, "active");
        assert!(
            !String::from_utf8_lossy(&fs::read(&database_path).unwrap()).contains("secret-marker")
        );

        let connection = database.connect().unwrap();
        connection.execute("INSERT INTO projects(id,name,path,created_at,updated_at) VALUES('project','Project','/tmp/project','now','now')", []).unwrap();
        connection.execute("INSERT INTO tasks(id,project_id,title,status,base_branch,base_revision,created_at,updated_at) VALUES('task','project','Task','working','main','abc','now','now')", []).unwrap();
        connection.execute("INSERT INTO agent_runs(id,task_id,provider_account_id,instruction,status,created_at,updated_at) VALUES('run','task','account','work','running','now','now')", []).unwrap();
        let error = remove(&database, &secrets, "account").unwrap_err();
        assert_eq!(error.code, "provider_account_in_use");
        assert!(secrets.get("account").unwrap().is_some());
        connection
            .execute(
                "UPDATE agent_runs SET status='succeeded' WHERE id='run'",
                [],
            )
            .unwrap();
        drop(connection);
        remove(&database, &secrets, "account").unwrap();
        assert_eq!(secrets.get("account").unwrap(), None);
        assert_eq!(
            get(&database, "account").unwrap_err().code,
            "provider_not_found"
        );
    }

    #[test]
    fn uses_the_selected_active_provider_for_new_goals() {
        let root = tempdir().unwrap();
        let database = Database::initialize(&root.path().join("db.sqlite3")).unwrap();
        let paths = RuntimePaths {
            data_dir: root.path().join("data"),
        };
        for id in ["first", "second"] {
            save(
                &GenericProfile {
                    id: id.into(),
                    display_name: id.into(),
                    provider_type: "generic".into(),
                    status: "active".into(),
                    executable_path: std::env::current_exe()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    arguments: vec!["{prompt}".into()],
                    resume_arguments: vec![],
                    prompt_mode: "argument".into(),
                    config_root_env_var: None,
                    config_source_path: None,
                    inherit_user_home: false,
                },
                &database,
                &paths,
            )
            .unwrap();
        }

        assert_eq!(set_default(&database, "second").unwrap(), "second");
        assert_eq!(
            default_provider(&database).unwrap().as_deref(),
            Some("second")
        );
        assert_eq!(list(&database).unwrap()[0].id, "second");

        database
            .connect()
            .unwrap()
            .execute(
                "UPDATE provider_accounts SET status='needs_reauth' WHERE id='second'",
                [],
            )
            .unwrap();
        assert_eq!(
            default_provider(&database).unwrap().as_deref(),
            Some("first")
        );
        assert_eq!(
            set_default(&database, "second").unwrap_err().code,
            "provider_not_ready"
        );
    }

    #[cfg(unix)]
    #[test]
    fn links_separate_codex_homes_through_the_device_flow() {
        use std::{os::unix::fs::PermissionsExt, sync::mpsc, time::Duration};

        let root = tempdir().unwrap();
        let database = Database::initialize(&root.path().join("db.sqlite3")).unwrap();
        let paths = RuntimePaths {
            data_dir: root.path().join("data"),
        };
        let executable = root.path().join("codex");
        fs::write(
            &executable,
            "#!/bin/sh\ncase \"$*\" in\n  *\"login status\"*) exit 0 ;;\n  *\"login --device-auth\"*) echo 'Open https://auth.openai.com/codex/device and enter TEST-CODE'; exit 0 ;;\n  *logout*) exit 0 ;;\n  *) exit 1 ;;\nesac\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        let profile = GenericProfile {
            id: "codex-work".into(),
            display_name: "Work email".into(),
            provider_type: "codex".into(),
            status: "active".into(),
            executable_path: executable.to_string_lossy().into_owned(),
            arguments: vec!["{prompt}".into()],
            resume_arguments: vec!["resume".into(), "--last".into()],
            prompt_mode: "argument".into(),
            config_root_env_var: Some("CODEX_HOME".into()),
            config_source_path: None,
            inherit_user_home: false,
        };
        save(&profile, &database, &paths).unwrap();
        let saved = get(&database, &profile.id).unwrap();
        assert_eq!(saved.status, "needs_reauth");
        assert_eq!(
            resolve(&database, &profile.id).err().unwrap().code,
            "provider_reauth_required"
        );

        let scope = PathBuf::from(saved.config_source_path.as_ref().unwrap());
        fs::write(
            scope.join("config.toml"),
            "model = \"test\"\n[history]\npersistence = \"none\"\n",
        )
        .unwrap();
        let (sender, receiver) = mpsc::channel();
        start_codex_login(
            &database,
            &paths,
            &ProcessSupervisor::default(),
            &profile.id,
            "device",
            Arc::new(move |event| {
                sender.send(event).unwrap();
            }),
        )
        .unwrap();
        let mut output = String::new();
        let linked = loop {
            match receiver.recv_timeout(Duration::from_secs(5)).unwrap() {
                ProviderAuthEvent::Output { text } => output.push_str(&text),
                ProviderAuthEvent::Finished {
                    success, account, ..
                } => {
                    assert!(success);
                    break account.unwrap();
                }
            }
        };
        assert!(output.contains("https://auth.openai.com/codex/device"));
        assert_eq!(linked.status, "active");
        let config = fs::read_to_string(scope.join("config.toml")).unwrap();
        assert!(config.starts_with("cli_auth_credentials_store = \"keyring\"\n"));
        assert!(config.contains("[history]"));
        assert_eq!(
            resolve(&database, &profile.id)
                .unwrap()
                .runtime_config_root(root.path().join("run-config")),
            scope
        );

        assert_eq!(
            logout_codex(&database, &profile.id).unwrap().status,
            "needs_reauth"
        );
    }
}
