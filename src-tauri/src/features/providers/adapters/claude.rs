use super::{ParsedOutput, ProviderAdapter, ProviderCapabilities, parse_output};

pub struct Claude;
pub static CLAUDE: Claude = Claude;

impl ProviderAdapter for Claude {
    fn key(&self) -> &'static str {
        "claude"
    }
    fn display_name(&self) -> &'static str {
        "Claude Code"
    }
    fn executable(&self) -> &'static str {
        "claude"
    }
    fn launch_arguments(&self) -> &'static [&'static str] {
        &["--session-id", "{sessionId}", "{prompt}"]
    }
    fn resume_arguments(&self) -> &'static [&'static str] {
        &["--resume", "{sessionId}"]
    }
    fn config_root_env_var(&self) -> Option<&'static str> {
        Some("CLAUDE_CONFIG_DIR")
    }
    fn secret_env_var(&self) -> Option<&'static str> {
        Some("ANTHROPIC_API_KEY")
    }
    fn full_access_flag(&self) -> &'static str {
        "--dangerously-skip-permissions"
    }
    fn auth_probe_arguments(&self) -> &'static [&'static str] {
        &["auth", "status"]
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_skills: false,
            reports_usage: false,
            interactive_input: true,
        }
    }
    fn parse_output(&self, output: &[u8]) -> ParsedOutput {
        parse_output(output, &["invalid api key", "please run /login"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_claude_auth_fixture() {
        assert!(CLAUDE.parse_output(b"Invalid API key").auth_required);
    }
}
