#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    match subshell_lib::run_control_adapter() {
        Ok(true) => return,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
        Ok(false) => {}
    }
    subshell_lib::run();
}
