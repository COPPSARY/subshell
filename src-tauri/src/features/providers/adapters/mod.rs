mod claude;
mod codex;
mod gemini;
mod kiro;

use serde::Serialize;
use serde_json::Value;

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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReportedUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ParsedOutput {
    pub auth_required: bool,
    pub usage: Option<ReportedUsage>,
}

pub trait ProviderAdapter: Send + Sync {
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
    fn parse_output(&self, output: &[u8]) -> ParsedOutput;
}

pub fn by_key(key: &str) -> Option<&'static dyn ProviderAdapter> {
    ADAPTERS
        .iter()
        .copied()
        .find(|adapter| adapter.key() == key)
}

pub(super) fn parse_output(output: &[u8], auth_markers: &[&str]) -> ParsedOutput {
    let text = String::from_utf8_lossy(output);
    let lowercase = text.to_ascii_lowercase();
    let auth_required = auth_markers.iter().any(|marker| lowercase.contains(marker));
    let usage = text
        .lines()
        .filter_map(json_value)
        .filter_map(find_usage)
        .next_back();
    ParsedOutput {
        auth_required,
        usage,
    }
}

fn json_value(line: &str) -> Option<Value> {
    let line = line.trim();
    if !line.starts_with('{') || !line.ends_with('}') {
        return None;
    }
    serde_json::from_str(line).ok()
}

fn find_usage(value: Value) -> Option<ReportedUsage> {
    match value {
        Value::Object(object) => {
            let input_tokens = token(
                &object,
                &["input_tokens", "inputTokens", "promptTokenCount"],
            );
            let output_tokens = token(
                &object,
                &["output_tokens", "outputTokens", "candidatesTokenCount"],
            );
            if input_tokens.is_some() || output_tokens.is_some() {
                Some(ReportedUsage {
                    input_tokens,
                    output_tokens,
                })
            } else {
                object.into_values().filter_map(find_usage).next_back()
            }
        }
        Value::Array(values) => values.into_iter().filter_map(find_usage).next_back(),
        _ => None,
    }
}

fn token(object: &serde_json::Map<String, Value>, names: &[&str]) -> Option<u64> {
    names.iter().find_map(|name| object.get(*name)?.as_u64())
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
            assert!(
                adapter
                    .parse_output(b"ordinary terminal output")
                    .usage
                    .is_none()
            );
        }
    }

    #[test]
    fn reported_usage_is_exact_or_unknown() {
        let parsed = parse_output(br#"{"usage":{"input_tokens":12,"output_tokens":4}}"#, &[]);
        assert_eq!(
            parsed.usage,
            Some(ReportedUsage {
                input_tokens: Some(12),
                output_tokens: Some(4)
            })
        );
        assert_eq!(parse_output(b"about 20 tokens", &[]).usage, None);
        assert_eq!(parse_output(b"{broken json", &[]), ParsedOutput::default());
    }
}
