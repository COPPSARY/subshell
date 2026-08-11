use std::path::PathBuf;

use tauri::Manager;

use crate::{
    features::{
        agent_api, attention,
        context::{self, ContextDrafts},
        context_sharing, health,
        preview::{self, PreviewService},
        projects, providers, review,
        runs::{self, RunService},
        tasks, timeline,
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
            let preview_service = PreviewService::new(
                database.clone(),
                paths.clone(),
                git.clone(),
                processes.clone(),
                ports.clone(),
            );
            let run_service = RunService::new(
                database.clone(),
                paths.clone(),
                git.clone(),
                drafts.clone(),
                processes.clone(),
                ports,
            );
            run_service
                .recover_and_dispatch()
                .map_err(|error| std::io::Error::other(error.message))?;
            app.manage(run_service);
            app.manage(preview_service);
            app.manage(database);
            app.manage(paths);
            app.manage(git);
            app.manage(drafts);
            app.manage(processes);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health::health_status,
            projects::projects_open,
            projects::projects_list,
            projects::projects_restore,
            projects::projects_status,
            projects::projects_files,
            providers::providers_create_generic,
            providers::providers_update_generic,
            providers::providers_remove,
            providers::providers_list,
            providers::providers_detect,
            preview::preview_prepare,
            preview::preview_get,
            preview::preview_start,
            preview::preview_stop,
            preview::preview_restart,
            preview::preview_close,
            preview::preview_read_log,
            review::review_get,
            review::review_approve,
            review::review_send_back,
            review::review_merge,
            tasks::tasks_create,
            tasks::tasks_list,
            tasks::tasks_list_archived,
            tasks::tasks_get,
            tasks::tasks_update_status,
            context::context_sources,
            context::context_preview,
            context_sharing::context_share_preview,
            context_sharing::context_share_deliver,
            runs::runs_environment_preview,
            runs::runs_start,
            runs::runs_enqueue,
            runs::runs_list,
            runs::runs_plan_get,
            runs::runs_plan_approve,
            runs::runs_plan_reject,
            runs::runs_read_output,
            runs::runs_write_input,
            runs::runs_resize,
            runs::runs_stop,
            runs::runs_mark_complete,
            runs::runs_resume,
            runs::runs_diff,
            timeline::timeline_list,
            attention::attention_list,
            attention::attention_acknowledge,
            attention::attention_claim_notification,
            agent_api::workspace_snapshot,
            agent_api::workspace_request_action,
            agent_api::workspace_report_activity,
            agent_api::workspace_submit_plan,
            agent_api::workspace_decide_action,
            agent_api::workspace_list_approvals,
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
