#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{fs, path::Path};

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::{
    contracts::{CommandError, Page},
    platform::{database::Database, environment::RuntimePaths},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenericProfile {
    #[serde(default)]
    pub id: String,
    pub display_name: String,
    pub executable_path: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    pub prompt_mode: String,
    pub config_root_env_var: Option<String>,
    pub config_source_path: Option<String>,
}

pub struct ResolvedProvider {
    pub id: String,
    pub config_source_path: Option<String>,
    pub config_root_env_var: Option<String>,
    executable_path: String,
    arguments: Vec<String>,
    prompt_mode: String,
}

impl ResolvedProvider {
    pub fn launch_command(&self, prompt: &str, config_root: &Path) -> (String, Vec<String>, bool) {
        let root = config_root.to_string_lossy();
        let arguments = self
            .arguments
            .iter()
            .map(|argument| {
                argument
                    .replace("{prompt}", prompt)
                    .replace("{configRoot}", &root)
            })
            .collect();
        (
            self.executable_path.clone(),
            arguments,
            self.prompt_mode == "stdin",
        )
    }
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
    connection.execute("INSERT INTO generic_provider_profiles(provider_account_id,executable_path,arguments_json,prompt_mode,config_root_env_var) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(provider_account_id) DO UPDATE SET executable_path=excluded.executable_path,arguments_json=excluded.arguments_json,prompt_mode=excluded.prompt_mode,config_root_env_var=excluded.config_root_env_var",params![profile.id,profile.executable_path,serde_json::to_string(&profile.arguments).unwrap(),profile.prompt_mode,profile.config_root_env_var])?;
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
        let clean = arg.replace("{prompt}", "").replace("{configRoot}", "");
        if clean.contains('{') || clean.contains('}') {
            return Err(CommandError::new(
                "invalid_arguments",
                "Only {prompt} and {configRoot} placeholders are supported",
            ));
        }
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
        prompt_mode: profile.prompt_mode,
        config_root_env_var: profile.config_root_env_var,
        config_source_path: profile.config_source_path,
    })
}
fn list(database: &Database) -> Result<Vec<GenericProfile>, CommandError> {
    let connection = database.connect()?;
    let mut statement=connection.prepare("SELECT a.id,a.display_name,g.executable_path,g.arguments_json,g.prompt_mode,g.config_root_env_var,a.config_scope_path FROM provider_accounts a JOIN generic_provider_profiles g ON g.provider_account_id=a.id WHERE a.removed_at IS NULL ORDER BY a.display_name")?;
    statement
        .query_map([], |row| {
            Ok(GenericProfile {
                id: row.get(0)?,
                display_name: row.get(1)?,
                executable_path: row.get(2)?,
                arguments: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
                prompt_mode: row.get(4)?,
                config_root_env_var: row.get(5)?,
                config_source_path: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
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
            prompt_mode: "argument".into(),
            config_root_env_var: None,
            config_source_path: None,
        };
        validate(&p).unwrap();
        let resolved = ResolvedProvider {
            id: p.id,
            executable_path: p.executable_path,
            arguments: p.arguments,
            prompt_mode: p.prompt_mode,
            config_root_env_var: None,
            config_source_path: None,
        };
        let (_, args, _) = resolved.launch_command("hello; touch /tmp/nope", dir.path());
        assert_eq!(args[1], "hello; touch /tmp/nope");
    }
}
