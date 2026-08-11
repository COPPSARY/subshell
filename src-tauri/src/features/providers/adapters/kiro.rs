use super::{ProviderAdapter, ProviderCapabilities};

pub struct Kiro;
pub static KIRO: Kiro = Kiro;

impl ProviderAdapter for Kiro {
    fn key(&self) -> &'static str {
        "kiro"
    }
    fn display_name(&self) -> &'static str {
        "Kiro CLI"
    }
    fn executable(&self) -> &'static str {
        "kiro-cli"
    }
    fn launch_arguments(&self) -> &'static [&'static str] {
        &["chat", "{prompt}"]
    }
    fn resume_arguments(&self) -> &'static [&'static str] {
        &["chat", "--resume"]
    }
    fn config_root_env_var(&self) -> Option<&'static str> {
        Some("KIRO_HOME")
    }
    fn secret_env_var(&self) -> Option<&'static str> {
        Some("KIRO_API_KEY")
    }
    fn full_access_flag(&self) -> &'static str {
        "--trust-all-tools"
    }
    fn auth_probe_arguments(&self) -> &'static [&'static str] {
        &["whoami"]
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_skills: false,
            reports_usage: false,
            interactive_input: true,
        }
    }
}
