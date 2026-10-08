//! Configuration and TOML profile loading.
//!
//! A profile answers: which provider, which model, sampling parameters, the
//! loop limits. Loops read `LoopConfig`; providers read `ModelProfile`. The
//! top-level `Profile` ties them together.

use crate::error::{ReclamoError, ReclamoResult};
use crate::providers::{CapabilitySet, ModelProfile, ReasoningFormat, ThinkingMode};
use serde::Deserialize;
use std::path::Path;
use std::sync::Arc;

/// Top-level TOML profile (the file at `~/.config/reclamo/profiles/foo.toml`).
#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    /// Profile display name.
    pub name: String,
    /// Provider adapter name: `anthropic`, `openai-compat`.
    pub provider: String,
    /// Model id passed to the provider.
    pub model: String,
    /// Optional provider-specific base URL.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Environment variable name holding the API key (preferred).
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Command to run to retrieve the API key (`pass foo/bar`).
    #[serde(default)]
    pub api_key_cmd: Option<String>,
    /// Advertised context window.
    pub max_context: u64,
    /// Resident KV (defaults to `max_context`).
    #[serde(default)]
    pub resident_kv: Option<u64>,
    /// Suggested char budget per sub-call.
    #[serde(default = "default_subcall_chars")]
    pub subcall_chars: u32,
    /// Sampling temperature.
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    /// `top_p`.
    #[serde(default = "default_top_p")]
    pub top_p: f64,
    /// `top_k`.
    #[serde(default)]
    pub top_k: Option<u64>,
    /// Max output tokens per call.
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
    /// presence_penalty — needed for the v0.1 Qwen/Strata repetitive-loop fix.
    #[serde(default)]
    pub presence_penalty: Option<f64>,
    /// Thinking mode (`enabled`, `disabled`, `adaptive`).
    #[serde(default = "default_thinking")]
    pub thinking: ThinkingMode,
    /// Capability overrides. Usually empty — the provider fills them in.
    #[serde(default)]
    pub capabilities: Option<ProfileCapabilities>,
    /// Vendor-specific passthrough (e.g. Qwen's `chat_template_kwargs`).
    #[serde(default)]
    pub extra_body: serde_json::Value,
    /// Loop limits. Required.
    pub loop_config: LoopConfigToml,
}

fn default_subcall_chars() -> u32 {
    20_000
}
fn default_temperature() -> f64 {
    0.7
}
fn default_top_p() -> f64 {
    0.95
}
fn default_thinking() -> ThinkingMode {
    ThinkingMode::Adaptive
}

/// TOML shape for the loop limits.
#[derive(Debug, Clone, Deserialize)]
pub struct LoopConfigToml {
    /// Hard cap on root turns.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
    /// Hard wall-clock cap, seconds.
    #[serde(default = "default_max_timeout")]
    pub max_timeout_secs: u64,
    /// Hard cap on cumulative tokens.
    #[serde(default)]
    pub max_tokens: Option<u64>,
    /// Per-call wall-clock cap (sub-calls).
    #[serde(default = "default_subcall_timeout")]
    pub subcall_timeout_secs: u64,
    /// Consecutive errors before forced finish.
    #[serde(default = "default_max_errors")]
    pub max_errors: u32,
    /// Sub-call caps.
    #[serde(default = "default_max_subcalls_per_run")]
    pub max_subcalls_per_run: u32,
    #[serde(default = "default_max_subcalls_per_exec")]
    pub max_subcalls_per_exec: u32,
    /// Per-role max output tokens: root vs sub-call.
    #[serde(default = "default_root_max_output")]
    pub root_max_output_tokens: u32,
    #[serde(default = "default_subcall_max_output")]
    pub subcall_max_output_tokens: u32,
    /// Use plain-fallback when context fits (NEXT-STEPS fix #1).
    #[serde(default = "default_true")]
    pub route_by_size: bool,
    /// Plain-fallback margin reserved for query + system prompt + answer.
    #[serde(default = "default_plain_margin")]
    pub plain_query_margin: u64,
}

fn default_max_iterations() -> u32 {
    30
}
fn default_max_timeout() -> u64 {
    600
}
fn default_subcall_timeout() -> u64 {
    90
}
fn default_max_errors() -> u32 {
    3
}
fn default_max_subcalls_per_run() -> u32 {
    64
}
fn default_max_subcalls_per_exec() -> u32 {
    24
}
fn default_root_max_output() -> u32 {
    4096
}
fn default_subcall_max_output() -> u32 {
    2048
}
fn default_true() -> bool {
    true
}
fn default_plain_margin() -> u64 {
    2_000
}

/// Capability overrides — only fill these if the defaults don't fit the
/// upstream.
#[derive(Debug, Clone, Deserialize)]
pub struct ProfileCapabilities {
    #[serde(default)]
    pub supports_thinking: Option<bool>,
    #[serde(default)]
    pub supports_tool_use: Option<bool>,
    #[serde(default)]
    pub supports_json_mode: Option<bool>,
    #[serde(default)]
    pub supports_prefix_cache: Option<bool>,
    #[serde(default)]
    pub known_reasoning_format: Option<ReasoningFormat>,
}

impl Default for LoopConfigToml {
    fn default() -> Self {
        Self {
            max_iterations: default_max_iterations(),
            max_timeout_secs: default_max_timeout(),
            max_tokens: None,
            subcall_timeout_secs: default_subcall_timeout(),
            max_errors: default_max_errors(),
            max_subcalls_per_run: default_max_subcalls_per_run(),
            max_subcalls_per_exec: default_max_subcalls_per_exec(),
            root_max_output_tokens: default_root_max_output(),
            subcall_max_output_tokens: default_subcall_max_output(),
            route_by_size: default_true(),
            plain_query_margin: default_plain_margin(),
        }
    }
}

impl Profile {
    /// Load a profile from a TOML file.
    pub fn from_path(p: &Path) -> ReclamoResult<Self> {
        let text = std::fs::read_to_string(p).map_err(|e| ReclamoError::Config(format!("read {p:?}: {e}")))?;
        let profile: Profile =
            toml::from_str(&text).map_err(|e| ReclamoError::Config(format!("parse {p:?}: {e}")))?;
        Ok(profile)
    }

    /// Resolve the API key from the configured source.
    pub fn resolve_api_key(&self) -> ReclamoResult<String> {
        if let Some(env) = &self.api_key_env {
            if let Ok(v) = std::env::var(env) {
                if !v.is_empty() {
                    return Ok(v);
                }
            }
        }
        if let Some(cmd) = &self.api_key_cmd {
            let out = std::process::Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .output()
                .map_err(|e| ReclamoError::Config(format!("api_key_cmd {cmd:?}: {e}")))?;
            if !out.status.success() {
                return Err(ReclamoError::Config(format!(
                    "api_key_cmd {:?} exited {:?}",
                    cmd,
                    out.status.code()
                )));
            }
            return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
        }
        Err(ReclamoError::Config(format!(
            "profile {:?}: no api_key_env or api_key_cmd set",
            self.name
        )))
    }

    /// Build the loop-level `LoopConfig`.
    pub fn loop_config(&self) -> LoopConfig {
        LoopConfig {
            max_iterations: self.loop_config.max_iterations,
            max_timeout: std::time::Duration::from_secs(self.loop_config.max_timeout_secs),
            max_tokens: self.loop_config.max_tokens,
            subcall_timeout: std::time::Duration::from_secs(self.loop_config.subcall_timeout_secs),
            max_errors: self.loop_config.max_errors,
            max_subcalls_per_run: self.loop_config.max_subcalls_per_run,
            max_subcalls_per_exec: self.loop_config.max_subcalls_per_exec,
            root_max_output_tokens: self.loop_config.root_max_output_tokens,
            subcall_max_output_tokens: self.loop_config.subcall_max_output_tokens,
            route_by_size: self.loop_config.route_by_size,
            plain_query_margin: self.loop_config.plain_query_margin,
        }
    }

    /// Effective capability set (profile override OR provider default).
    pub fn capability_set(&self, provider_default: CapabilitySet) -> CapabilitySet {
        let Some(c) = &self.capabilities else {
            return provider_default;
        };
        CapabilitySet {
            supports_thinking: c.supports_thinking.unwrap_or(provider_default.supports_thinking),
            supports_tool_use: c.supports_tool_use.unwrap_or(provider_default.supports_tool_use),
            supports_json_mode: c.supports_json_mode.unwrap_or(provider_default.supports_json_mode),
            supports_prefix_cache: c.supports_prefix_cache.unwrap_or(provider_default.supports_prefix_cache),
            known_reasoning_format: c.known_reasoning_format.unwrap_or(provider_default.known_reasoning_format),
        }
    }

    /// Build the `ModelProfile` for a given provider.
    ///
    /// `provider` is the `Provider` adapter (Anthropic, OpenAICompat, ...).
    /// The factory lives in `providers::registry`.
    pub fn model_profile(&self, provider: Arc<dyn crate::providers::Provider>) -> ModelProfile {
        let caps = self.capability_set(provider.capabilities());
        let _ = caps; // (capability hooks aren't read yet; future-proofing.)
        let mut p = ModelProfile::new(self.name.clone(), provider, self.max_context);
        p.resident_kv = self.resident_kv.unwrap_or(self.max_context);
        p.subcall_chars = self.subcall_chars;
        p.presence_penalty = self.presence_penalty;
        p.thinking = self.thinking;
        p.temperature = self.temperature;
        p.top_p = self.top_p;
        p.top_k = self.top_k;
        p.max_output_tokens = self.max_output_tokens;
        p.extra_body = self.extra_body.clone();
        p
    }
}

/// Loop-level limits. Concrete struct (not TOML-shaped) so the loop can pass
/// it by value.
#[derive(Debug, Clone)]
pub struct LoopConfig {
    /// Max root turns.
    pub max_iterations: u32,
    /// Wall-clock cap for the whole run.
    pub max_timeout: std::time::Duration,
    /// Cumulative token cap (across root + sub-calls).
    pub max_tokens: Option<u64>,
    /// Wall-clock cap for a single sub-call.
    pub subcall_timeout: std::time::Duration,
    /// Consecutive REPL errors before forced finish.
    pub max_errors: u32,
    /// Max sub-calls per run.
    pub max_subcalls_per_run: u32,
    /// Max sub-calls per REPL session (between root calls).
    pub max_subcalls_per_exec: u32,
    /// Per-call output token cap for root.
    pub root_max_output_tokens: u32,
    /// Per-call output token cap for sub-calls.
    pub subcall_max_output_tokens: u32,
    /// If true, route to plain when context fits.
    pub route_by_size: bool,
    /// Margin kept for query + system + answer in plain route.
    pub plain_query_margin: u64,
}

impl Default for LoopConfig {
    fn default() -> Self {
        LoopConfigToml::default().into()
    }
}

impl From<LoopConfigToml> for LoopConfig {
    fn from(t: LoopConfigToml) -> Self {
        Self {
            max_iterations: t.max_iterations,
            max_timeout: std::time::Duration::from_secs(t.max_timeout_secs),
            max_tokens: t.max_tokens,
            subcall_timeout: std::time::Duration::from_secs(t.subcall_timeout_secs),
            max_errors: t.max_errors,
            max_subcalls_per_run: t.max_subcalls_per_run,
            max_subcalls_per_exec: t.max_subcalls_per_exec,
            root_max_output_tokens: t.root_max_output_tokens,
            subcall_max_output_tokens: t.subcall_max_output_tokens,
            route_by_size: t.route_by_size,
            plain_query_margin: t.plain_query_margin,
        }
    }
}

impl LoopConfig {
    /// Build a `LoopConfig` from CLI opts (most fields are unchanged; CLI
    /// only overrides a few).
    pub fn from_opts(opts: crate::completion::CompletionOpts) -> Self {
        let mut c = LoopConfig::default();
        if let Some(v) = opts.max_iterations {
            c.max_iterations = v;
        }
        if let Some(v) = opts.max_subcalls_per_run {
            c.max_subcalls_per_run = v;
        }
        if let Some(v) = opts.max_subcalls_per_exec {
            c.max_subcalls_per_exec = v;
        }
        // `max_timeout` is also overridable so tests can bound the
        // loop with millisecond budgets (instead of the 600s default)
        // — see `harness_root_call_respects_max_timeout` and
        // `harness_exec_respects_remaining_deadline` (#7).
        if let Some(v) = opts.max_timeout {
            c.max_timeout = v;
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_profile() {
        let toml = r#"
            name = "openai-dev"
            provider = "openai-compat"
            model = "gpt-4o-mini"
            max_context = 128000

            [loop_config]
            max_iterations = 10
            max_timeout_secs = 300
        "#;
        let p: Profile = toml::from_str(toml).unwrap();
        assert_eq!(p.loop_config.max_iterations, 10);
        assert_eq!(p.subcall_chars, 20_000);
        assert_eq!(p.thinking, ThinkingMode::Adaptive);
    }

    #[test]
    fn loop_config_defaults_match_upstream_rlm_v0_2() {
        let c: LoopConfig = LoopConfigToml::default().into();
        // Upstream rlm: _DEFAULT_MAX_ITERATIONS = 30.
        assert_eq!(c.max_iterations, 30);
        assert_eq!(c.max_errors, 3);
        assert_eq!(c.max_subcalls_per_run, 64);
        assert_eq!(c.max_subcalls_per_exec, 24);
    }

    #[test]
    fn profile_default_subcall_chars_matches_repl_truncation() {
        // The Python REPL's `llm_query` truncates at `subcall_chars` by default
        // (see src/repl/worker.py). The TOML default must match, otherwise the
        // prompt tells the model one number and the runtime enforces another.
        assert_eq!(default_subcall_chars(), 20_000);
    }

    #[test]
    fn resolve_api_key_from_env() {
        std::env::set_var("RECLAMO_TEST_KEY", "secret-123");
        let p = Profile {
            name: "t".into(),
            provider: "anthropic".into(),
            model: "x".into(),
            base_url: None,
            api_key_env: Some("RECLAMO_TEST_KEY".into()),
            api_key_cmd: None,
            max_context: 100_000,
            resident_kv: None,
            subcall_chars: 20_000,
            temperature: 0.7,
            top_p: 0.95,
            top_k: None,
            max_output_tokens: None,
            presence_penalty: None,
            thinking: ThinkingMode::Adaptive,
            capabilities: None,
            extra_body: serde_json::Value::Null,
            loop_config: LoopConfigToml::default(),
        };
        assert_eq!(p.resolve_api_key().unwrap(), "secret-123");
        std::env::remove_var("RECLAMO_TEST_KEY");
    }
}
