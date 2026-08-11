mod claude;
mod codex;
mod gemini;
mod kiro;

use serde::Serialize;

pub use claude::CLAUDE;
pub use codex::CODEX;
pub use gemini::GEMINI;
pub use kiro::KIRO;

pub static ADAPTERS: [&dyn ProviderAdapter; 4] = [&CLAUDE, &CODEX, &KIRO, &GEMINI];

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    pub native_skills: bool,
    pub reports_usage: bool,
    pub interactive_input: bool,
}

pub trait ProviderAdapter: Sync {
    fn key(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn executable(&self) -> &'static str;
    fn launch_arguments(&self) -> &'static [&'static str];
    fn resume_arguments(&self) -> &'static [&'static str];
    fn config_root_env_var(&self) -> Option<&'static str>;
    fn secret_env_var(&self) -> Option<&'static str>;
    fn full_access_flag(&self) -> &'static str;
    fn auth_probe_arguments(&self) -> &'static [&'static str];
    fn capabilities(&self) -> ProviderCapabilities;
}

pub fn by_key(key: &str) -> Option<&'static dyn ProviderAdapter> {
    ADAPTERS
        .iter()
        .copied()
        .find(|adapter| adapter.key() == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_native_adapter_passes_the_shared_contract() {
        for adapter in ADAPTERS {
            assert!(!adapter.key().is_empty());
            assert!(!adapter.display_name().is_empty());
            assert!(!adapter.executable().is_empty());
            assert!(adapter.launch_arguments().contains(&"{prompt}"));
            assert!(!adapter.full_access_flag().is_empty());
            assert!(!adapter.auth_probe_arguments().is_empty());
            assert!(adapter.capabilities().interactive_input);
        }
    }
}
