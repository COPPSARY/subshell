use std::path::PathBuf;

use tauri::Manager;

use crate::{
    features::{
        context::{self, ContextDrafts},
        health, projects, providers,
        runs::{self, RunService},
        tasks,
    },
    platform::{
        database::Database,
        environment::{PortLeases, RuntimePaths},
        git::GitService,
        process::ProcessSupervisor,
    },
};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = data_dir(app)?;
            let database = Database::initialize(&data_dir.join("subshell.sqlite3"))?;
            let paths = RuntimePaths { data_dir };
            let git = GitService::default();
            let drafts = ContextDrafts::default();
            let processes = ProcessSupervisor::default();
            let ports = PortLeases::default();
            app.manage(RunService::new(
                database.clone(),
                paths.clone(),
                git.clone(),
                drafts.clone(),
                processes,
                ports,
            ));
            app.manage(database);
            app.manage(paths);
            app.manage(git);
            app.manage(drafts);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health::health_status,
            projects::projects_open,
            projects::projects_list,
            projects::projects_restore,
            projects::projects_status,
            providers::providers_create_generic,
            providers::providers_update_generic,
            providers::providers_remove,
            providers::providers_list,
            tasks::tasks_create,
            tasks::tasks_list,
            tasks::tasks_get,
            context::context_sources,
            context::context_preview,
            runs::runs_environment_preview,
            runs::runs_start,
            runs::runs_list,
            runs::runs_read_output,
            runs::runs_write_input,
            runs::runs_resize,
            runs::runs_stop,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run SubShell");
}

fn data_dir(app: &tauri::App) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = std::env::var_os("SUBSHELL_DATA_DIR") {
        return Ok(path.into());
    }
    Ok(app.path().app_data_dir()?)
}
