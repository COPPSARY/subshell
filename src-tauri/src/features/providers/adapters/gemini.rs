use super::{ParsedOutput, ProviderAdapter, ProviderCapabilities, parse_output};

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
    fn parse_output(&self, output: &[u8]) -> ParsedOutput {
        parse_output(output, &["please set an auth method", "api key not valid"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_gemini_auth_fixture() {
        assert!(
            GEMINI
                .parse_output(b"Please set an Auth method")
                .auth_required
        );
    }
}
