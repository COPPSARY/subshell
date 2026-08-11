use super::{ProviderAdapter, ProviderCapabilities};

pub struct Gemini;
pub static GEMINI: Gemini = Gemini;

impl ProviderAdapter for Gemini {
    fn key(&self) -> &'static str {
        "gemini"
    }
    fn display_name(&self) -> &'static str {
        "Gemini CLI"
    }
    fn executable(&self) -> &'static str {
        "gemini"
    }
    fn launch_arguments(&self) -> &'static [&'static str] {
        &["-i", "{prompt}"]
    }
    fn resume_arguments(&self) -> &'static [&'static str] {
        &[]
    }
    fn config_root_env_var(&self) -> Option<&'static str> {
        None
    }
    fn secret_env_var(&self) -> Option<&'static str> {
        Some("GEMINI_API_KEY")
    }
    fn full_access_flag(&self) -> &'static str {
        "--approval-mode=yolo"
    }
    fn auth_probe_arguments(&self) -> &'static [&'static str] {
        &["--version"]
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_skills: false,
            reports_usage: false,
            interactive_input: true,
        }
    }
}
