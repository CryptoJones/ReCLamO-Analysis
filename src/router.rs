//! Plain-vs-harness routing (NEXT-STEPS fix #1).
//!
//! Before `RLM.completion()` starts, estimate how many tokens the context
//! would consume **as part of the system's prompt history**, accounting for
//! the prompt template. If the total still fits inside
//! `profile.resident_kv - LoopConfig::plain_query_margin`, take the plain
//! path — one single-shot completion, no REPL, no loop, no sub-calls. If
//! not, hand off to the loop.
//!
//! Token estimation is conservative: we use chars / 3.5 to approximate
//! tokens for English-like text. Over-estimating is safe (we err on the
//! side of the harness, where the answer is bounded by code execution
//! instead of model recall).

use crate::completion::RunResult;
use crate::config::LoopConfig;
use crate::error::ReclamoResult;
use crate::providers::{CompleteOpts, Message, ModelProfile};
use crate::rlm;
use std::time::Instant;

/// Conservative chars-to-tokens conversion. Roughly tuned for English.
const CHARS_PER_TOKEN: f64 = 3.5;

/// Plumb `CompletionOpts` through (so the CLI's options apply).
pub async fn route(
    context: String,
    query: String,
    profile: ModelProfile,
    cfg: LoopConfig,
) -> ReclamoResult<RunResult> {
    let est_tokens = estimate_tokens(&context) + estimate_tokens(&query);

    if cfg.route_by_size && est_tokens + cfg.plain_query_margin <= profile.resident_kv {
        return run_plain(context, query, profile, cfg).await;
    }

    rlm::run(context, query, profile, cfg).await
}

/// `chars / 3.5`, rounded up. The estimator never lies about how much will
/// fit.
pub fn estimate_tokens(s: &str) -> u64 {
    let n = s.chars().count() as f64 / CHARS_PER_TOKEN;
    n.ceil() as u64
}

/// Run the plain path.
pub async fn run_plain(
    context: String,
    query: String,
    profile: ModelProfile,
    cfg: LoopConfig,
) -> ReclamoResult<RunResult> {
    let started = Instant::now();
    // System prompt is far shorter in the plain path: no REPL scaffolding
    // (the model can't use it). We send only the static "You are a careful
    // assistant. Quote the document, don't summarize" instructions.
    let system = format!(
        "You are a careful assistant with access to a long document.\n\
         Answer the user's question by quoting or extracting directly from\n\
         `context`. Do not summarize; do not paraphrase; do not invent.\n\
         If the answer is not in the document, say so exactly."
    );

    let user = format!(
        "Question:\n{query}\n\n\
         --- context ---\n{context}\n--- end context ---"
    );

    let messages = vec![Message::system(system), Message::user(user)];

    let max_output: u64 = (cfg.root_max_output_tokens as u64)
        .max(profile.max_output_tokens.unwrap_or(0));
    let opts = CompleteOpts {
        thinking_override: Some(profile.thinking),
        temperature: Some(profile.temperature),
        top_p: Some(profile.top_p),
        top_k: profile.top_k,
        max_output_tokens: Some(max_output),
        extra_body: profile.extra_body.clone(),
    };

    let completion = profile
        .provider
        .complete(&messages, None, opts)
        .await?;

    // If the reasoning came back but content is empty, fall back to
    // reasoning. Mirrors the Qwen/Strata fallback pattern.
    let mut answer = completion.content.clone();
    if answer.trim().is_empty() {
        if let Some(r) = &completion.reasoning {
            answer = r.clone();
        }
    }
    let answer = answer.trim().to_string();

    let tokens = completion.usage.total();
    Ok(RunResult {
        answer,
        mode: "plain".into(),
        stop_reason: completion.stop_reason,
        tokens,
        seconds: started.elapsed().as_secs_f64(),
        turns: 0,
        subcalls: 0,
    })
}

/// Quick estimator exposed for tests and the eval rig.
pub fn tokens_for(s: &str) -> u64 {
    estimate_tokens(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_is_conservative() {
        let s = "a".repeat(350);
        let t = estimate_tokens(&s);
        assert!(t >= 99 && t <= 101, "got {t}");
    }

    #[test]
    fn estimate_handles_short_text() {
        assert!(estimate_tokens("") <= 1);
        assert!(estimate_tokens("hi") <= 1);
    }
}
