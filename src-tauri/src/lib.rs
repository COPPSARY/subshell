mod app;
mod contracts;
mod features;
mod platform;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    app::run();
}

pub fn run_control_adapter() -> Result<bool, String> {
    features::agent_api::run_adapter()
}
