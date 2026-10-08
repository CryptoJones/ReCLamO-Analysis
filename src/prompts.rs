//! System-prompt builder.
//!
//! One entry: [`build_system_prompt`]. Capability-driven — no model name
//! anywhere in the prompt; instead, the loop interpolates values from
//! [`ModelProfile`] and [`CapabilitySet`].
//!
//! Implements NEXT-STEPS fixes 2, 3, 4 at the prompt layer:
//! - **Commit-early** — the prompt tells the model to populate
//!   `answer["content"]` from the first turn and update it on every useful
//!   intermediate result.
//! - **Un-scare delegation** — the real budget is stated; "use sub-calls
//!   for judgement, not for line-by-line scanning." (The Qwen warning about
//!   "expensive / one-at-a-time / hard cap" is gone — it was the reason
//!   `llm_query` fired in only 5/40 runs.)
//! - **Multi-hop helper** — REPL bootstrap exposes a generic
//!   `extract_event_table(text, regex)` helper, callable from any model.
//!
//! Fix 1 (route by size) and Fix 5 (forced-finish unification) are at
//! `router.rs` and `rlm.rs`, not here.

use crate::providers::{CapabilitySet, ModelProfile};

/// The built system prompt.
pub struct SystemPrompt {
    /// The full prompt string. Send as a single system message.
    pub text: String,
}

/// Parameters that change prompt contents.
pub struct PromptInputs<'a> {
    /// The model profile (window, sub-call char budget, ...).
    pub profile: &'a ModelProfile,
    /// The capability set (drives which sections appear).
    pub caps: &'a CapabilitySet,
}

/// Build the system prompt for one run.
pub fn build_system_prompt(input: PromptInputs<'_>) -> SystemPrompt {
    let subcall_chars = input.profile.subcall_chars;
    let max_subcalls_per_run = 64; // matches LoopConfig default; readable here.
    let resident_kv = input.profile.resident_kv;
    let supports_tool_use = input.caps.supports_tool_use;

    // Pre-format the chunk sizes with thousands separators so the prose
    // reads naturally (e.g. "12,000" not "12000"). The raw integer stays
    // available for the helper signature line.
    let subcall_chars_fmt = thousands_sep(subcall_chars as u64);
    let resident_kv_fmt = thousands_sep(resident_kv);

    let text = format!(
        include_str!("prompts/system_prompt.template"),
        subcall_chars = subcall_chars,
        subcall_chars_fmt = subcall_chars_fmt,
        max_subcalls_per_run = max_subcalls_per_run,
        resident_kv_fmt = resident_kv_fmt,
        supports_tool_use_str = if supports_tool_use { "yes" } else { "no" },
    );

    SystemPrompt { text }
}

/// Format `n` with comma thousands-separator (e.g. 12000 -> "12,000").
/// Rust's built-in `format!("{:,}")` doesn't exist (only Python supports
/// that spec), so we do it by hand. The number is always non-negative.
fn thousands_sep(n: u64) -> String {
    if n < 1_000 {
        return n.to_string();
    }
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(bytes.len() + bytes.len() / 3);
    let split_point = bytes.len() % 3;
    if split_point != 0 {
        out.push_str(std::str::from_utf8(&bytes[..split_point]).unwrap());
    }
    let mut i = split_point;
    while i < bytes.len() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(std::str::from_utf8(&bytes[i..i + 3]).unwrap());
        i += 3;
    }
    out
}

/// Per-turn user-message builder.
///
/// Sends:
/// - The original query.
/// - The current turn index (`Turn i/N`).
/// - The first-turn "inspect the context first" safeguard.
/// - Late nudges at 60 % and 85 % of `max_iterations`.
/// - A regex-futility nudge if `regex_futility_turns` ≥ N.
#[allow(clippy::too_many_arguments)]
pub fn build_user_message(
    query: &str,
    turn: u32,
    max_iterations: u32,
    max_subcalls_used: u32,
    max_subcalls_per_run: u32,
    has_partial_answer: bool,
    regex_futility_turns: u32,
    sandbox_kind: &str,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    if turn == 1 {
        parts.push(format!(
            "Question:\n{query}\n\n\
             Turn 1/{max_iterations}: First, inspect `context`. \n\
             Then form a plan and begin working. After each useful intermediate result, \n\
             call `commit(text)` to update `answer['content']`."
        ));
    } else {
        parts.push(format!("Question:\n{query}"));
        parts.push(format!(
            "Turn {turn}/{max_iterations}. Sub-calls used: {max_subcalls_used}/{max_subcalls_per_run}."
        ));
    }

    let pct = turn as f64 / max_iterations as f64;
    if pct >= 0.85 {
        parts.push(
            "You have used 85 % of your turn budget. If your code is not making progress, \n\
             commit your best partial answer to `answer['content']` now. If `answer` looks \n\
             correct, return `FINAL_VAR(answer)`."
                .to_string(),
        );
    } else if pct >= 0.6 {
        if has_partial_answer {
            parts.push(
                "Your `answer['content']` already holds a partial answer. Confirm whether \n\
                 it is the final value: if so, return `FINAL_VAR(answer)` now."
                    .to_string(),
            );
        } else {
            parts.push(
                "You have used 60 % of your turn budget. Keep an eye on the answer variable: \n\
                 is the work converging? Do not wait until the last turn to commit."
                    .to_string(),
            );
        }
    }

    if regex_futility_turns >= 3 {
        parts.push(format!(
            "You have run regex for {regex_futility_turns} turns without a useful match. \n\
             Consider switching strategies: use a chunked `llm_query` over sample slices of \n\
             `context`, or build an event table with the `extract_event_table` helper, then \n\
             apply corrections in code."
        ));
    } else if max_subcalls_used == 0 && turn >= 4 {
        parts.push(
            "You have not used a single sub-call yet. If the data is large enough that\n\
             line-by-line work would be slow, ask a small LLM to classify or summarize one\n\
             chunk at a time, then synthesize in code."
                .to_string(),
        );
    }

    if sandbox_kind == "docker" {
        parts.push(
            "Note: this REPL runs in a `--network none` container. LM calls still go out\n\
             over the parent's stdio link, but you cannot `requests.get(...)` or similar."
                .to_string(),
        );
    }

    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{CapabilitySet, MockProvider};
    use std::sync::Arc;

    fn profile(max_context: u64, subcall_chars: u32) -> ModelProfile {
        let prov = Arc::new(MockProvider::scripted("m", vec![])) as Arc<dyn crate::providers::Provider>;
        let mut p = ModelProfile::new("test", prov, max_context);
        p.subcall_chars = subcall_chars;
        p
    }

    #[test]
    fn prompt_starts_with_task_marker() {
        let p = profile(8192, 12_000);
        let caps = CapabilitySet::openai_compat();
        let s = build_system_prompt(PromptInputs { profile: &p, caps: &caps });
        assert!(s.text.contains("Recursive"));
        assert!(s.text.contains("12,000"));
    }

    #[test]
    fn prompt_interpolates_subcall_chars() {
        let p = profile(8192, 4_000);
        let caps = CapabilitySet::openai_compat();
        let s = build_system_prompt(PromptInputs { profile: &p, caps: &caps });
        assert!(s.text.contains("4,000"));
    }

    #[test]
    fn user_message_first_turn_says_inspect() {
        let m = build_user_message("find the bug", 1, 20, 0, 64, false, 0, "");
        assert!(m.contains("Turn 1/20"));
        assert!(m.contains("inspect"));
    }

    #[test]
    fn user_message_late_nudge_at_85_percent() {
        let m = build_user_message("q", 17, 20, 0, 64, true, 0, "");
        assert!(m.contains("85 %"));
        assert!(m.contains("FINAL_VAR(answer)"));
    }

    #[test]
    fn user_message_nudges_after_fruitless_regex() {
        let m = build_user_message("q", 5, 20, 0, 64, true, 4, "");
        assert!(m.contains("regex for 4 turns"));
        assert!(m.contains("extract_event_table"));
    }

    #[test]
    fn user_message_prompts_delegation_when_zero_subcalls_at_turn_4() {
        let m = build_user_message("q", 4, 20, 0, 64, false, 0, "");
        assert!(m.contains("not used a single sub-call"));
    }

    #[test]
    fn user_message_silenced_when_delegation_already_used() {
        let m = build_user_message("q", 4, 20, 5, 64, false, 0, "");
        assert!(!m.contains("not used a single sub-call"));
    }

    #[test]
    fn user_message_warns_docker() {
        let m = build_user_message("q", 2, 20, 0, 64, false, 0, "docker");
        assert!(m.contains("network none"));
    }
}
