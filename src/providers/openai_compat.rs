//! OpenAI-compatible provider.
//!
//! A single adapter that talks to anything OpenAI-shaped: Strata, Ollama,
//! vLLM, LM Studio, OpenRouter, OpenAI native. The implementation uses
//! `reqwest` directly (no `async-openai` dependency) so vendor-specific
//! fields pass through cleanly:
//!
//! - **Qwen on Strata**: `chat_template_kwargs: {enable_thinking, reasoning_effort}`.
//! - **OpenAI o-series**: `reasoning_effort: "low"|"medium"|"high"`.
//! - **DeepSeek R1**: separate `reasoning_content` field on the response.
//! - **Anything else**: pass-through `extra_body`.

use super::base::{CapabilitySet, CompleteOpts, Completion, Message, Provider, Role, ToolCall};
use super::reasoning::extract_think_tags;
use crate::error::{ReclamoError, ReclamoResult};
use async_trait::async_trait;
use serde::Deserialize;

/// OpenAI-compat provider adapter (one Strata / Ollama / OpenRouter / etc.).
#[derive(Clone)]
pub struct OpenAICompatProvider {
    base_url: String,
    model: String,
    api_key: String,
    client: reqwest::Client,
}

impl OpenAICompatProvider {
    /// Build a new adapter.
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
    ) -> ReclamoResult<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| ReclamoError::Provider { provider: "openai-compat".into(), message: e.to_string() })?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: model.into(),
            api_key: api_key.into(),
            client,
        })
    }
}

#[async_trait]
impl Provider for OpenAICompatProvider {
    fn name(&self) -> &'static str {
        "openai-compat"
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::openai_compat()
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
        let body = build_body(messages, tools, &self.model, &opts);
        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ReclamoError::Provider { provider: "openai-compat".into(), message: e.to_string() })?;

        let status = resp.status();
        // Read the body chunk-by-chunk instead of `.text()` / `.bytes()`.
        // Both call `BodyExt::collect`, which on rustls + HTTP/1.1 can
        // fail with Kind::Decode on certain Cloudflare-fronted OpenRouter
        // responses (verified 2026-10-08: nemotron-3.5-lightning:free
        // returns 200 OK + chunked + leading whitespace prelude bytes
        // that the chunked decoder chokes on; nemotron-3-super-120b:free
        // does not). Streaming through `chunk()` and concatenating
        // matches what `curl -i` shows. The fix is empirical — the
        // underlying transport bug is upstream.
        let mut raw_bytes: Vec<u8> = Vec::new();
        let mut resp = resp;
        loop {
            match resp.chunk().await {
                Ok(Some(chunk)) => raw_bytes.extend_from_slice(&chunk),
                Ok(None) => break,
                Err(e) => {
                    return Err(ReclamoError::Provider {
                        provider: "openai-compat".into(),
                        message: e.to_string(),
                    });
                }
            }
        }
        let raw = String::from_utf8_lossy(&raw_bytes).into_owned();

        if !status.is_success() {
            return Err(ReclamoError::Provider {
                provider: "openai-compat".into(),
                message: format!("HTTP {}: {}", status.as_u16(), truncate(&raw)),
            });
        }

        let parsed: ChatResponse = serde_json::from_str(&raw).map_err(|e| ReclamoError::Provider {
            provider: "openai-compat".into(),
            message: format!("JSON parse: {e}; head: {}", truncate(&raw)),
        })?;

        if let Some(err) = parsed.error {
            return Err(ReclamoError::Provider {
                provider: "openai-compat".into(),
                message: format!("upstream error: {}", err.message),
            });
        }

        let choice = parsed.choices.into_iter().next().ok_or_else(|| ReclamoError::Provider {
            provider: "openai-compat".into(),
            message: "no choice in response".into(),
        })?;

        let msg = choice.message.ok_or_else(|| ReclamoError::Provider {
            provider: "openai-compat".into(),
            message: "choice has no message".into(),
        })?;

        let raw_text = msg.content.unwrap_or_default();
        let mut reasoning = msg.reasoning_content.or(msg.reasoning);
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        for tc in msg.tool_calls.unwrap_or_default() {
            let args = serde_json::from_str(&tc.function.arguments)
                .unwrap_or(serde_json::Value::Null);
            tool_calls.push(ToolCall { id: tc.id, name: tc.function.name, arguments: args });
        }

        // Defensive <think>-tag strip (Strata sometimes leaks them).
        let (content, stripped_thinking) = extract_think_tags(&raw_text);
        if stripped_thinking.is_some() && reasoning.is_none() {
            reasoning = stripped_thinking;
        }

        // Per-turn audit log: every completion the harness sees.
        // Useful for catching fence-parser slips (some models emit
        // `<tool_call>repl ...</tool_call>` inside `content` with
        // `tool_calls` empty; the peer's 2026-10-08 fix runs only
        // the first such block and discards the rest). Off by default
        // (`RUST_LOG=info,reclamo_anl::providers::openai_compat=debug`).
        tracing::debug!(
            model = %self.model,
            finish = %choice.finish_reason.as_str(),
            tool_calls = tool_calls.len(),
            content_bytes = content.len(),
            has_tool_call_block = content.contains("tool_call") || content.contains("<tool_call>"),
            "completion"
        );

        let usage = parsed.usage.map(|u| super::base::Usage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        }).unwrap_or_default();

        Ok(Completion {
            content,
            reasoning,
            tool_calls,
            stop_reason: choice.finish_reason.as_str(),
            usage,
        })
    }
}

fn truncate(s: &str) -> String {
    if s.len() > 240 { format!("{}…", &s[..240]) } else { s.to_string() }
}

fn build_body(
    messages: &[Message],
    tools: Option<&[serde_json::Value]>,
    model: &str,
    opts: &CompleteOpts,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages.iter().map(to_wire_message).collect::<Vec<_>>(),
        "stream": false,
    });

    if let Some(t) = opts.temperature { body["temperature"] = serde_json::json!(t); }
    if let Some(p) = opts.top_p { body["top_p"] = serde_json::json!(p); }
    if let Some(k) = opts.top_k { body["top_k"] = serde_json::json!(k); }
    if let Some(m) = opts.max_output_tokens { body["max_tokens"] = serde_json::json!(m); }

    // Merge any vendor extras (presence_penalty, chat_template_kwargs,
    // reasoning_effort, frequency_penalty, ...).
    if !opts.extra_body.is_null() {
        if let (Some(target), Some(src)) = (body.as_object_mut(), opts.extra_body.as_object()) {
            for (k, v) in src {
                target.insert(k.clone(), v.clone());
            }
        }
    }

    if let Some(ts) = tools {
        if !ts.is_empty() {
            body["tools"] = serde_json::json!(ts);
            body["tool_choice"] = serde_json::json!("auto");
        }
    }

    body
}

fn to_wire_message(m: &Message) -> serde_json::Value {
    let mut v = serde_json::json!({"role": wire_role(m.role), "content": m.content});
    if let Some(r) = &m.reasoning {
        v["reasoning_content"] = serde_json::json!(r);
    }
    if !m.tool_calls.is_empty() {
        v["tool_calls"] = serde_json::json!(m.tool_calls.iter().map(|tc| {
            serde_json::json!({
                "id": tc.id,
                "type": "function",
                "function": {"name": tc.name, "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default()},
            })
        }).collect::<Vec<_>>());
    }
    if let Some(id) = &m.tool_call_id {
        v["tool_call_id"] = serde_json::json!(id);
    }
    v
}

fn wire_role(r: Role) -> &'static str {
    match r {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    /// Error envelope (OpenRouter, vLLM, etc.) — surfaces 503/429 with a
    /// `{"error": {"message": "..."}}` body instead of a 200 + `choices: []`.
    #[serde(default)]
    error: Option<WireError>,
    #[serde(default)]
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
struct WireError {
    message: String,
    // Captured so the JSON parse doesn't drop the field, but the harness
    // only needs the message today.
    #[serde(default)]
    #[allow(dead_code)]
    code: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    #[serde(default)]
    message: Option<ChatMessage>,
    finish_reason: FinishReason,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    #[serde(default)]
    content: Option<String>,
    /// Non-standard: Strata, DeepSeek R1, some OpenRouter providers echo
    /// reasoning here.
    #[serde(default, rename = "reasoning_content")]
    reasoning_content: Option<String>,
    /// Non-standard: OpenRouter's NVIDIA Nemotron echoes a top-level
    /// `reasoning` string on the assistant message.
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<WireToolCall>>,
}

#[derive(Debug, Deserialize)]
struct WireToolCall {
    id: String,
    function: WireFunction,
}

#[derive(Debug, Deserialize)]
struct WireFunction {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct ChatUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

/// `stop` / `length` / `tool_calls` / `content_filter` — keep as an enum so
/// we can serialize/inspect at the trace layer.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
struct FinishReason(String);

impl FinishReason {
    fn as_str(&self) -> String {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_think_tags_strips_leak() {
        let (v, r) = extract_think_tags("<think>reasoning</think>answer");
        assert_eq!(v, "answer");
        assert_eq!(r.as_deref(), Some("reasoning"));
    }

    #[test]
    fn build_body_merges_extra_body() {
        let msg = Message::user("hi");
        let opts = CompleteOpts {
            extra_body: json!({"chat_template_kwargs": {"enable_thinking": false}, "presence_penalty": 1.0}),
            ..Default::default()
        };
        let b = build_body(&[msg], None, "x", &opts);
        assert_eq!(b["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(b["presence_penalty"], 1.0);
    }

    #[test]
    fn build_body_keeps_first_party_params() {
        let msg = Message::user("hi");
        let opts = CompleteOpts {
            temperature: Some(0.3),
            top_p: Some(0.9),
            ..Default::default()
        };
        let b = build_body(&[msg], None, "x", &opts);
        assert_eq!(b["temperature"], 0.3);
        assert_eq!(b["top_p"], 0.9);
    }

    #[test]
    fn parse_response_with_thinking_field() {
        let raw = r#"{
            "choices": [{
                "message": {
                    "content": "answer",
                    "reasoning_content": "thinking"
                },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 5, "completion_tokens": 7, "total_tokens": 12}
        }"#;
        let parsed: ChatResponse = serde_json::from_str(raw).unwrap();
        let c = parsed.choices.into_iter().next().unwrap();
        assert_eq!(c.message.as_ref().unwrap().content.as_deref(), Some("answer"));
        assert_eq!(c.message.as_ref().unwrap().reasoning_content.as_deref(), Some("thinking"));
    }

    #[test]
    fn parse_response_with_tool_calls() {
        let raw = r#"{
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {"name": "execute_python", "arguments": "{\"code\":\"1+1\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        }"#;
        let parsed: ChatResponse = serde_json::from_str(raw).unwrap();
        let c = parsed.choices.into_iter().next().unwrap();
        let m = c.message.unwrap();
        assert!(m.content.is_none());
        let tcs = m.tool_calls.unwrap();
        assert_eq!(tcs[0].function.name, "execute_python");
    }
}
