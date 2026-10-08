//! Provider trait and shared types.
//!
//! A [`Provider`] is one adapter per upstream. Loops and prompts branch on
//! [`CapabilitySet`], not on model name. Reasoning format is a capability.
//!
//! A [`ModelProfile`] is one model binding a [`Provider`] to the parameters
//! that affect prompt and loop behavior (window size, sub-call char budget,
//! sampling, presence penalty).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// How the upstream represents the model's reasoning tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningFormat {
    /// `<think>…</think>` in content; the upstream may or may not strip it.
    ThinkTags,
    /// A separate `reasoning_content` field on the response; the visible
    /// `content` may be empty (Strata, OpenRouter, some vLLM builds).
    ReasoningField,
    /// Anthropic `thinking` blocks in `content[]`.
    NativeBlocks,
    /// No reasoning available.
    None,
}

/// How the loop drives thinking across the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingMode {
    /// Every call asks for thinking.
    Enabled,
    /// Never asks. Default for `OpenAICompatProvider` on sub-calls when the
    /// upstream won't strip thinking.
    Disabled,
    /// Root asks for thinking; sub-calls don't. Matches the v0.1 Strata
    /// behavior (Qwen-on-Strata is much faster without thinking on
    /// 45-tok/s decode).
    Adaptive,
}

/// What a provider/model can do. Drives every prompt and loop decision.
#[derive(Debug, Clone, Copy)]
pub struct CapabilitySet {
    /// Whether `ThinkingMode::Enabled` does anything useful.
    pub supports_thinking: bool,
    /// Whether the provider can emit `ToolCall`s.
    pub supports_tool_use: bool,
    /// Whether the provider can force JSON output.
    pub supports_json_mode: bool,
    /// Whether the upstream rewards keeping histories append-only
    /// (Strata, OpenRouter with cache, Anthropic prompt cache).
    pub supports_prefix_cache: bool,
    /// How reasoning tokens come back.
    pub known_reasoning_format: ReasoningFormat,
}

impl CapabilitySet {
    /// Build a Qwen-on-Strata-style capability set.
    pub const fn qwen_strata() -> Self {
        Self {
            supports_thinking: true,
            supports_tool_use: false,
            supports_json_mode: false,
            supports_prefix_cache: true,
            known_reasoning_format: ReasoningFormat::ReasoningField,
        }
    }

    /// Anthropic Messages API.
    pub const fn anthropic() -> Self {
        Self {
            supports_thinking: true,
            supports_tool_use: true,
            supports_json_mode: false,
            supports_prefix_cache: true,
            known_reasoning_format: ReasoningFormat::NativeBlocks,
        }
    }

    /// Generic OpenAI-compatible endpoint (Ollama, OpenAI native, etc.).
    pub const fn openai_compat() -> Self {
        Self {
            supports_thinking: true,
            supports_tool_use: true,
            supports_json_mode: true,
            supports_prefix_cache: false,
            known_reasoning_format: ReasoningFormat::ReasoningField,
        }
    }
}

/// Chat role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// One chat message. Providers translate to their wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Role.
    pub role: Role,
    /// Visible content (no `<think>` tags, no block structures).
    pub content: String,
    /// Model reasoning for this turn (only meaningful for `Assistant`).
    /// Kept separate so the loop never re-feeds it into history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// Tool calls the assistant emitted (only `Assistant`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// `tool_call_id` for tool-result messages (only `Tool`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    /// System message.
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: Role::System, content: content.into(), reasoning: None, tool_calls: vec![], tool_call_id: None }
    }
    /// User message.
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: Role::User, content: content.into(), reasoning: None, tool_calls: vec![], tool_call_id: None }
    }
    /// Assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            reasoning: None,
            tool_calls: vec![],
            tool_call_id: None,
        }
    }
}

/// One tool-use call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Stable id from the provider (Anthropic: long; OpenAI: `call_xxx`).
    pub id: String,
    /// Name (e.g. `execute_python`, `final_answer`).
    pub name: String,
    /// Raw JSON arguments as the provider returned them.
    pub arguments: serde_json::Value,
}

/// Token accounting. `None` if upstream does not report.
#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct Usage {
    /// Input tokens.
    pub input_tokens: Option<u64>,
    /// Output tokens.
    pub output_tokens: Option<u64>,
    /// Total tokens (may be `None` even when split is reported).
    pub total_tokens: Option<u64>,
}

impl Usage {
    /// Add another usage to this one in place.
    pub fn add(&mut self, other: Usage) {
        fn sum(a: Option<u64>, b: Option<u64>) -> Option<u64> {
            match (a, b) {
                (Some(a), Some(b)) => Some(a + b),
                _ => None,
            }
        }
        self.input_tokens = sum(self.input_tokens, other.input_tokens);
        self.output_tokens = sum(self.output_tokens, other.output_tokens);
        self.total_tokens = sum(self.total_tokens, other.total_tokens);
    }

    /// Best-effort total.
    pub fn total(&self) -> u64 {
        self.total_tokens.unwrap_or_else(|| {
            self.input_tokens.unwrap_or(0) + self.output_tokens.unwrap_or(0)
        })
    }
}

/// One provider completion, normalized.
#[derive(Debug, Clone, Serialize)]
pub struct Completion {
    /// Visible content (no `<think>` tags, no block structure).
    pub content: String,
    /// Model reasoning text, if any. **Never re-enters history.**
    pub reasoning: Option<String>,
    /// Tool calls, if any.
    pub tool_calls: Vec<ToolCall>,
    /// Provider-reported stop reason (`stop`, `max_tokens`, `tool_use`, ...).
    pub stop_reason: String,
    /// Token accounting.
    pub usage: Usage,
}

/// Provider-level options the loop can vary per call (e.g. sub-call vs root).
#[derive(Debug, Clone, Default)]
pub struct CompleteOpts {
    /// Override the profile's thinking policy for this single call.
    pub thinking_override: Option<ThinkingMode>,
    /// Sampling temperature override.
    pub temperature: Option<f64>,
    /// `top_p` override.
    pub top_p: Option<f64>,
    /// `top_k` override.
    pub top_k: Option<u64>,
    /// Max output tokens override.
    pub max_output_tokens: Option<u64>,
    /// Any extra body parameters (vendor-specific, e.g. `chat_template_kwargs`).
    pub extra_body: serde_json::Value,
}

/// Provider trait — one adapter per upstream.
///
/// Implemented via `async_trait` so the registry can return
/// `Box<dyn Provider>` (a single binary dispatch point).
#[async_trait]
pub trait Provider: Send + Sync {
    /// Stable identifier (`"anthropic"`, `"openai-compat"`, `"mock"`).
    fn name(&self) -> &'static str;

    /// Issue one chat completion.
    async fn complete(
        &self,
        messages: &[Message],
        tools: Option<&[serde_json::Value]>,
        opts: CompleteOpts,
    ) -> Result<Completion, crate::error::ReclamoError>;

    /// Capability set this provider+model exposes. The loop and prompt branch
    /// on this — **never** on the model name.
    fn capabilities(&self) -> CapabilitySet;

    /// Model id (used in logs and eval rigs).
    fn model_id(&self) -> &str;
}

/// One model binding a [`Provider`] to loop-relevant parameters.
#[derive(Clone)]
pub struct ModelProfile {
    /// Human-readable label.
    pub name: String,
    /// Underlying provider.
    pub provider: std::sync::Arc<dyn Provider>,
    /// Advertised context window.
    pub max_context: u64,
    /// KV the upstream keeps resident (used for compaction threshold; for
    /// Anthropic it equals `max_context`; for Strata it's 32K of 262K).
    pub resident_kv: u64,
    /// Suggested char budget per `llm_query` sub-call (the prompt mentions
    /// this exact number).
    pub subcall_chars: u32,
    /// If the model tends to repeat `llm_query` outputs (Qwen-on-Strata did).
    pub presence_penalty: Option<f64>,
    /// Thinking mode for the loop.
    pub thinking: ThinkingMode,
    /// Sampling temperature.
    pub temperature: f64,
    /// `top_p`.
    pub top_p: f64,
    /// `top_k`.
    pub top_k: Option<u64>,
    /// Max output tokens per call (per-role set in `LoopConfig`).
    pub max_output_tokens: Option<u64>,
    /// Any vendor-specific extra body for every call.
    pub extra_body: serde_json::Value,
}

impl ModelProfile {
    /// Convenience constructor.
    pub fn new(name: impl Into<String>, provider: std::sync::Arc<dyn Provider>, max_context: u64) -> Self {
        Self {
            name: name.into(),
            provider,
            max_context,
            resident_kv: max_context,
            subcall_chars: 12_000,
            presence_penalty: None,
            thinking: ThinkingMode::Adaptive,
            temperature: 0.7,
            top_p: 0.95,
            top_k: None,
            max_output_tokens: None,
            extra_body: serde_json::Value::Object(Default::default()),
        }
    }

    /// Effective context budget we let the prompt see (minus room for the
    /// answer and the system instructions).
    pub fn effective_context(&self) -> u64 {
        // Reserve ~2K for system + answer bookkeeping.
        self.resident_kv.saturating_sub(2_000)
    }
}
