use super::{ParsedOutput, ProviderAdapter, ProviderCapabilities, parse_output};

pub struct Codex;
pub static CODEX: Codex = Codex;

impl ProviderAdapter for Codex {
    fn key(&self) -> &'static str {
        "codex"
    }
    fn display_name(&self) -> &'static str {
        "Codex"
    }
    fn executable(&self) -> &'static str {
        "codex"
    }
    fn launch_arguments(&self) -> &'static [&'static str] {
        &["{prompt}"]
    }
    fn resume_arguments(&self) -> &'static [&'static str] {
        &["resume", "--last"]
    }
    fn config_root_env_var(&self) -> Option<&'static str> {
        Some("CODEX_HOME")
    }
    fn secret_env_var(&self) -> Option<&'static str> {
        Some("OPENAI_API_KEY")
    }
    fn full_access_flag(&self) -> &'static str {
        "--dangerously-bypass-approvals-and-sandbox"
    }
    fn auth_probe_arguments(&self) -> &'static [&'static str] {
        &["login", "status"]
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_skills: false,
            reports_usage: false,
            interactive_input: true,
        }
    }
    fn parse_output(&self, output: &[u8]) -> ParsedOutput {
        parse_output(output, &["not logged in", "401 unauthorized"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_codex_auth_fixture() {
        assert!(CODEX.parse_output(b"Not logged in").auth_required);
    }
}
