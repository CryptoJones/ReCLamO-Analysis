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
    /// Suggested char budget per sub-call. `None` → derive from
    /// [`Profile::effective_subcall_chars`] (peer-aligned with
    /// ReCLamO-Harness `RLMConfig.effective_subcall_chars`, v0.2 followup).
    /// v0.1 profiles that pinned an explicit value still win.
    #[serde(default)]
    pub subcall_chars: Option<u32>,
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
    /// `root_max_output_tokens: None` → derive from
    /// [`Profile::effective_root_max_output_tokens`]: 8,192 with
    /// `thinking = enabled`, 4,096 otherwise (v0.2 followup, peer-aligned).
    /// An explicit value wins.
    #[serde(default)]
    pub root_max_output_tokens: Option<u32>,
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
// v0.2 followup (peer-aligned with ReCLamO-Harness @ 9ac890d):
//   600 -> 3,600 s run wall-clock; 90 -> 900 s per-sub-call; 64 -> 256 per run;
//   24 -> 64 per exec; 2,048 -> 4,096 sub-call output. Root max_tokens is
//   conditional on thinking (see `Profile::effective_root_max_output_tokens`).
fn default_max_timeout() -> u64 {
    3_600
}
fn default_subcall_timeout() -> u64 {
    900
}
fn default_max_errors() -> u32 {
    3
}
fn default_max_subcalls_per_run() -> u32 {
    256
}
fn default_max_subcalls_per_exec() -> u32 {
    64
}
fn default_subcall_max_output() -> u32 {
    4_096
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
            // None → derive via `Profile::effective_root_max_output_tokens`
            // (8,192 with thinking on, 4,096 off).
            root_max_output_tokens: None,
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
            root_max_output_tokens: self.effective_root_max_output_tokens() as u32,
            subcall_max_output_tokens: self.loop_config.subcall_max_output_tokens,
            route_by_size: self.loop_config.route_by_size,
            plain_query_margin: self.loop_config.plain_query_margin,
        }
    }

    /// Effective char budget per `llm_query` sub-call (the prompt mentions
    /// this exact number). v0.2 followup, peer-aligned with
    /// ReCLamO-Harness `RLMConfig.effective_subcall_chars` at config.py:313.
    ///
    /// Formula:
    ///   advertised = (sub_window − sub.max_output) × (1 − SUBCALL_MARGIN)
    ///               × SUBCALL_CHARS_PER_TOKEN
    /// with `SUBCALL_MARGIN = 0.15` and `SUBCALL_CHARS_PER_TOKEN = 3`.
    /// On pluto (32K window, sub max_tokens 4,096) that gives ~71,155.
    /// An explicit `subcall_chars` in the profile wins. v0.1 profiles that
    /// pinned 12,000 keep 12,000.
    pub fn effective_subcall_chars(&self) -> u32 {
        if let Some(c) = self.subcall_chars {
            return c;
        }
        const SUBCALL_MARGIN: f64 = 0.15;
        const SUBCALL_CHARS_PER_TOKEN: f64 = 3.0;
        let sub_window = self.max_context;
        let sub_max_output = self.loop_config.subcall_max_output_tokens as u64;
        let raw = sub_window.saturating_sub(sub_max_output) as f64;
        let advertised = raw * (1.0 - SUBCALL_MARGIN) * SUBCALL_CHARS_PER_TOKEN;
        // Floor at 1,000 to avoid advertising "0 chars per call" on tiny
        // windows; cap at 1,000,000 to keep the prompt from looking like a
        // phone book. Real models stay well under both bounds.
        advertised.clamp(1_000.0, 1_000_000.0) as u32
    }

    /// Effective root `max_tokens` for this profile. v0.2 followup,
    /// peer-aligned: 8,192 with `thinking = enabled` (the root can use a
    /// long CoT), 4,096 with anything else. An explicit
    /// `loop_config.root_max_output_tokens` wins.
    pub fn effective_root_max_output_tokens(&self) -> u64 {
        if let Some(t) = self.loop_config.root_max_output_tokens {
            return t as u64;
        }
        match self.thinking {
            ThinkingMode::Enabled => 8_192,
            _ => 4_096,
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
        // v0.2 followup: derive per-call char budget from the sub-model's
        // real window unless the profile pins an explicit value.
        p.subcall_chars = self.effective_subcall_chars();
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
        // `root_max_output_tokens` is `Option<u32>` on the TOML side because
        // the v0.2 followup makes it conditional on `Profile::thinking`
        // (8,192 enabled, 4,096 otherwise — see
        // `Profile::effective_root_max_output_tokens`). A bare
        // `LoopConfigToml::default()` has no profile to consult, so we
        // fall back to 4,096 (the "thinking off" default). Production
        // callers go through `Profile::loop_config()`, which DOES consult
        // `Profile::thinking`.
        let root_max_output_tokens = t.root_max_output_tokens.unwrap_or(4_096);
        Self {
            max_iterations: t.max_iterations,
            max_timeout: std::time::Duration::from_secs(t.max_timeout_secs),
            max_tokens: t.max_tokens,
            subcall_timeout: std::time::Duration::from_secs(t.subcall_timeout_secs),
            max_errors: t.max_errors,
            max_subcalls_per_run: t.max_subcalls_per_run,
            max_subcalls_per_exec: t.max_subcalls_per_exec,
            root_max_output_tokens,
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
        // v0.2 followup: subcall_chars is `Option<u32>`; not set in the
        // minimal profile, so `effective_subcall_chars()` derives from
        // the (128K window, default 4,096 sub-max) formula.
        assert_eq!(p.subcall_chars, None);
        let expected = ((128_000u64 - 4_096) as f64 * 0.85 * 3.0) as u32;
        assert_eq!(p.effective_subcall_chars(), expected);
        assert_eq!(p.thinking, ThinkingMode::Adaptive);
    }

    #[test]
    fn loop_config_defaults_match_upstream_rlm_v0_2() {
        // Covers both the v0.2 port (30 max_iterations) and the v0.2 followup
        // (bumped caps + longer budgets). Peer-aligned with ReCLamO-Harness
        // config.py at 9ac890d.
        let c: LoopConfig = LoopConfigToml::default().into();
        assert_eq!(c.max_iterations, 30);
        assert_eq!(c.max_errors, 3);
        // v0.2 followup:
        assert_eq!(c.max_subcalls_per_run, 256);
        assert_eq!(c.max_subcalls_per_exec, 64);
        assert_eq!(c.max_timeout, std::time::Duration::from_secs(3_600));
        assert_eq!(c.subcall_timeout, std::time::Duration::from_secs(900));
        assert_eq!(c.subcall_max_output_tokens, 4_096);
        // root_max_output_tokens is derived: 4,096 with thinking = Adaptive.
        assert_eq!(c.root_max_output_tokens, 4_096);
    }

    #[test]
    fn profile_effective_subcall_chars_uses_formula() {
        // 100K window, default 4,096 sub-max → ~244,683 advertised chars.
        // Verifies the v0.2 followup formula; this is the value the prompt
        // mentions in `llm_query`'s `subcall_chars=` and the
        // "Each call can read about N characters" line.
        let p = minimal_profile(100_000, None, ThinkingMode::Adaptive);
        let expected = ((100_000u64 - 4_096) as f64 * 0.85 * 3.0) as u32;
        assert_eq!(p.effective_subcall_chars(), expected);
    }

    #[test]
    fn profile_effective_subcall_chars_explicit_wins() {
        // v0.1 profiles that pinned 12,000 must keep 12,000.
        let p = minimal_profile(100_000, Some(12_000), ThinkingMode::Adaptive);
        assert_eq!(p.effective_subcall_chars(), 12_000);
    }

    #[test]
    fn profile_effective_subcall_chars_small_window_floored() {
        // 2K window, 4,096 sub-max → saturating_sub = 0 → advertised = 0;
        // clamp floors at 1,000 so the prompt still says "1,000" not "0".
        let p = minimal_profile(2_000, None, ThinkingMode::Adaptive);
        assert_eq!(p.effective_subcall_chars(), 1_000);
    }

    #[test]
    fn profile_effective_root_max_output_tokens_thinking_on() {
        let p = minimal_profile(32_000, None, ThinkingMode::Enabled);
        assert_eq!(p.effective_root_max_output_tokens(), 8_192);
    }

    #[test]
    fn profile_effective_root_max_output_tokens_thinking_off() {
        let p = minimal_profile(32_000, None, ThinkingMode::Adaptive);
        assert_eq!(p.effective_root_max_output_tokens(), 4_096);
        let p = minimal_profile(32_000, None, ThinkingMode::Disabled);
        assert_eq!(p.effective_root_max_output_tokens(), 4_096);
    }

    #[test]
    fn profile_effective_root_max_output_tokens_explicit_wins() {
        // 16,384 explicit should beat the 8,192 default.
        let mut p = minimal_profile(32_000, None, ThinkingMode::Enabled);
        p.loop_config.root_max_output_tokens = Some(16_384);
        assert_eq!(p.effective_root_max_output_tokens(), 16_384);
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
            // v0.2 followup: subcall_chars is `Option<u32>`.
            subcall_chars: Some(20_000),
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

    fn minimal_profile(
        max_context: u64,
        subcall_chars: Option<u32>,
        thinking: ThinkingMode,
    ) -> Profile {
        Profile {
            name: "t".into(),
            provider: "openai-compat".into(),
            model: "x".into(),
            base_url: None,
            api_key_env: None,
            api_key_cmd: None,
            max_context,
            resident_kv: None,
            subcall_chars,
            temperature: 0.7,
            top_p: 0.95,
            top_k: None,
            max_output_tokens: None,
            presence_penalty: None,
            thinking,
            capabilities: None,
            extra_body: serde_json::Value::Null,
            loop_config: LoopConfigToml::default(),
        }
    }
}
