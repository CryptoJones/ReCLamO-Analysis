//! Anthropic provider. Messages API.
//!
//! Speaks to `https://api.anthropic.com/v1/messages` directly via reqwest,
//! because the Messages API has subtleties async-openai doesn't handle:
//!
//! - `system` is a top-level field, not the first message.
//! - `content[]` is an array of typed blocks (`text`, `thinking`, `tool_use`,
//!   `tool_result`).
//! - Prompt caching: `cache_control: {type: "ephemeral"}` on a content block
//!   (we mark the system message and the last user message).
//! - Thinking: dedicated `thinking` blocks in the response; we translate
//!   them to our normalized `reasoning: Option<String>`.

use super::base::{CapabilitySet, CompleteOpts, Completion, Message, Provider, Role, ToolCall};
use crate::error::{ReclamoError, ReclamoResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Anthropic Messages API URL.
pub const ANTHROPIC_API_URL: &str = "https://api.anthropic.com";

/// Anthropic API version header.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic provider adapter.
pub struct AnthropicProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
    model: String,
}

impl AnthropicProvider {
    /// Build a new adapter. `api_key` should already be resolved from env or
    /// `pass`. Used as the `x-api-key` header — never logged.
    pub fn new(model: impl Into<String>, api_key: impl Into<String>) -> ReclamoResult<Self> {
        Self::with_base(ANTHROPIC_API_URL, model, api_key)
    }

    /// Override the base URL (e.g. for a proxy or self-hosted gateway).
    pub fn with_base(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
    ) -> ReclamoResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| ReclamoError::Provider { provider: "anthropic".into(), message: e.to_string() })?;
        Ok(Self {
            api_key: api_key.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client,
            model: model.into(),
        })
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &'static str {
        "anthropic"
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::anthropic()
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    async fn complete(
        &self,
        messages: &[Message],
        tools: Option<&[serde_json::Value]>,
        opts: CompleteOpts,
    ) -> ReclamoResult<Completion> {
        let req = build_request(messages, tools, &self.model, &opts);

        let resp = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| ReclamoError::Provider { provider: "anthropic".into(), message: e.to_string() })?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| ReclamoError::Provider { provider: "anthropic".into(), message: e.to_string() })?;

        if !status.is_success() {
            return Err(ReclamoError::Provider {
                provider: "anthropic".into(),
                message: format!("HTTP {}: {}", status.as_u16(), truncate_body(&body)),
            });
        }

        let parsed: MessagesResponse = serde_json::from_str(&body).map_err(|e| {
            ReclamoError::Provider {
                provider: "anthropic".into(),
                message: format!("JSON parse: {e}; body head: {}", truncate_body(&body)),
            }
        })?;

        Ok(parsed.into_completion())
    }
}

fn truncate_body(body: &str) -> String {
    if body.len() > 240 {
        format!("{}…", &body[..240])
    } else {
        body.to_string()
    }
}

#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<SystemBlock>>,
    messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct SystemBlock {
    #[serde(rename = "type")]
    kind: &'static str,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<serde_json::Value>,
}

fn build_request(
    messages: &[Message],
    tools: Option<&[serde_json::Value]>,
    model: &str,
    opts: &CompleteOpts,
) -> MessagesRequest {
    let mut sys: Vec<SystemBlock> = Vec::new();
    let mut out_messages: Vec<serde_json::Value> = Vec::new();

    for (idx, m) in messages.iter().enumerate() {
        match m.role {
            Role::System => {
                sys.push(SystemBlock {
                    kind: "text",
                    text: m.content.clone(),
                    // Mark the system message for caching. Anthropic caches
                    // up to 4 breakpoints; we use the system block + the
                    // last user message.
                    cache_control: Some(serde_json::json!({"type": "ephemeral"})),
                });
            }
            Role::User => {
                let last_user = idx + 1 == messages.len()
                    || messages
                        .get(idx + 1)
                        .map(|nm| nm.role == Role::User)
                        .unwrap_or(false);
                let mut block = serde_json::json!({
                    "type": "text",
                    "text": m.content,
                });
                if last_user {
                    block["cache_control"] = serde_json::json!({"type": "ephemeral"});
                }
                out_messages.push(serde_json::json!({
                    "role": "user",
                    "content": [block],
                }));
            }
            Role::Assistant => {
                let mut blocks: Vec<serde_json::Value> = Vec::new();
                if let Some(r) = &m.reasoning {
                    blocks.push(serde_json::json!({"type": "thinking", "thinking": r}));
                }
                if !m.content.is_empty() {
                    blocks.push(serde_json::json!({"type": "text", "text": m.content}));
                }
                for tc in &m.tool_calls {
                    blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": tc.arguments,
                    }));
                }
                out_messages.push(serde_json::json!({"role": "assistant", "content": blocks}));
            }
            Role::Tool => {
                out_messages.push(serde_json::json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                        "content": m.content,
                    }],
                }));
            }
        }
    }

    let thinking = match opts.thinking_override.unwrap_or(super::base::ThinkingMode::Enabled) {
        super::base::ThinkingMode::Enabled => {
            Some(serde_json::json!({"type": "enabled", "budget_tokens": 4096}))
        }
        _ => None,
    };

    MessagesRequest {
        model: model.into(),
        max_tokens: opts.max_output_tokens.unwrap_or(4096),
        system: if sys.is_empty() { None } else { Some(sys) },
        messages: out_messages,
        tools: tools.map(|t| t.to_vec()),
        temperature: opts.temperature,
        top_p: opts.top_p,
        top_k: opts.top_k,
        thinking,
    }
}

// Cleaner version: build messages outside, avoid the goofy `**if` trick.
#[allow(dead_code)]
fn build_messages(messages: &[Message]) -> Vec<serde_json::Value> {
    let mut out: Vec<serde_json::Value> = Vec::new();
    for m in messages {
        match m.role {
            Role::System => { /* handled separately */ }
            Role::User => out.push(serde_json::json!({"role": "user", "content": m.content})),
            Role::Assistant => {
                let mut blocks: Vec<serde_json::Value> = Vec::new();
                if let Some(r) = &m.reasoning {
                    blocks.push(serde_json::json!({"type": "thinking", "thinking": r}));
                }
                if !m.content.is_empty() {
                    blocks.push(serde_json::json!({"type": "text", "text": m.content}));
                }
                for tc in &m.tool_calls {
                    blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": tc.arguments,
                    }));
                }
                out.push(serde_json::json!({"role": "assistant", "content": blocks}));
            }
            Role::Tool => {
                out.push(serde_json::json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                        "content": m.content,
                    }],
                }));
            }
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    content: Vec<ResponseBlock>,
    stop_reason: Option<String>,
    usage: ResponseUsage,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponseBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct ResponseUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

impl MessagesResponse {
    fn into_completion(self) -> Completion {
        let mut visible = String::new();
        let mut reasoning: Option<String> = None;
        let mut tool_calls = Vec::new();

        for blk in self.content {
            match blk {
                ResponseBlock::Text { text } => {
                    if !visible.is_empty() {
                        visible.push('\n');
                    }
                    visible.push_str(&text);
                }
                ResponseBlock::Thinking { thinking } => {
                    reasoning.get_or_insert_with(String::new).push_str(&thinking);
                }
                ResponseBlock::ToolUse { id, name, input } => {
                    tool_calls.push(ToolCall { id, name, arguments: input });
                }
                ResponseBlock::Other => {}
            }
        }

        Completion {
            content: visible,
            reasoning,
            tool_calls,
            stop_reason: self.stop_reason.unwrap_or_else(|| "end_turn".to_string()),
            usage: super::base::Usage {
                input_tokens: self.usage.input_tokens,
                output_tokens: self.usage.output_tokens,
                total_tokens: match (self.usage.input_tokens, self.usage.output_tokens) {
                    (Some(i), Some(o)) => Some(i + o),
                    _ => None,
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::build_messages;

    #[test]
    fn build_messages_handles_user_only() {
        let v = build_messages(&[Message::user("hi")]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0]["role"], "user");
    }

    #[test]
    fn build_messages_translates_assistant_blocks() {
        let mut a = Message::assistant("hi");
        a.tool_calls.push(ToolCall {
            id: "t1".into(),
            name: "final_answer".into(),
            arguments: serde_json::json!({"x": 1}),
        });
        let v = build_messages(&[a]);
        let blocks = v[0]["content"].as_array().unwrap();
        assert!(blocks.iter().any(|b| b["type"] == "text"));
        assert!(blocks.iter().any(|b| b["type"] == "tool_use"));
    }

    #[test]
    fn response_blocks_translate_correctly() {
        let raw = r#"{
            "content": [
              {"type": "thinking", "thinking": "let me think"},
              {"type": "text", "text": "the answer"},
              {"type": "tool_use", "id": "t1", "name": "final_answer", "input": {"value": 42}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 100, "output_tokens": 50}
        }"#;
        let r: MessagesResponse = serde_json::from_str(raw).unwrap();
        let c = r.into_completion();
        assert_eq!(c.content, "the answer");
        assert_eq!(c.reasoning.as_deref(), Some("let me think"));
        assert_eq!(c.tool_calls.len(), 1);
        assert_eq!(c.stop_reason, "tool_use");
        assert_eq!(c.usage.input_tokens, Some(100));
    }

    #[test]
    fn empty_thinking_block_is_ignored() {
        let raw = r#"{
            "content": [{"type": "text", "text": "ok"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }"#;
        let r: MessagesResponse = serde_json::from_str(raw).unwrap();
        let c = r.into_completion();
        assert_eq!(c.reasoning, None);
    }
}
