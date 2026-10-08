//! Provider abstraction.
//!
//! One trait, three implementations:
//!
//! - [`AnthropicProvider`] — Anthropic Messages API.
//! - [`OpenAICompatProvider`] — Strata / Ollama / OpenRouter / vLLM /
//!   LM Studio / OpenAI native. A pluggable `thinking_extension` for
//!   vendor-specific reasoning controls.
//! - [`MockProvider`] — deterministic scripted responses for tests.
//!
//! Loops and prompts branch on [`CapabilitySet`], not on model name.

pub mod anthropic;
pub mod base;
pub mod mock;
pub mod openai_compat;
pub mod reasoning;

pub use anthropic::AnthropicProvider;
pub use base::{
    CapabilitySet, Completion, CompleteOpts, Message, ModelProfile, Provider, ReasoningFormat, Role,
    ThinkingMode, ToolCall, Usage,
};
pub use mock::MockProvider;
pub use openai_compat::OpenAICompatProvider;
pub use reasoning::extract_think_tags;

/// Convenience alias for shared ownership.
pub type SharedProvider = std::sync::Arc<dyn Provider>;
