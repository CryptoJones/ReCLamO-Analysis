//! System-prompt builder.
//!
//! One entry: [`build_system_prompt`]. Capability-driven — no model name
//! anywhere in the prompt; instead, the loop interpolates values from
//! [`ModelProfile`] and [`CapabilitySet`].
//!
//! ## Prompt version: v0.2
//!
//! The current template (`src/prompts/system_prompt.template`) ports upstream
//! rlm's `RLM_SYSTEM_PROMPT` + `ORCHESTRATOR_ADDENDUM` (rlm/utils/prompts.py
//! on `rlm main`):
//!
//! - **Commit-early** — the prompt tells the model to populate
//!   `answer["content"]` from the first turn and update it on every useful
//!   intermediate result.
//! - **Orchestrator role** — the upstream ORCHESTRATOR_ADDENDUM is appended
//!   (six paragraphs verbatim): act as an orchestrator, not a solver; plan
//!   the decomposition explicitly before delegating; say when NOT to
//!   delegate; keep your own context clean (delegate long reads to sub-LLMs);
//!   two-axis sub-call budget (~100K chars/prompt capacity, ~20 prompts/batch
//!   fan-out); reserve your own tokens for high-level decisions.
//! - **No delegation deterrent** — the v0.1 "use sub-calls for judgement, not
//!   for line-by-line scanning" / "DELEGATION RULES" section (the Qwen3-Coder
//!   patch from ReCLamO-Harness) is gone. The paper's main prompt (App. C.1
//!   1a) says sub-LLMs are "strongly encouraged to use as much as possible…
//!   don't be afraid to put a lot of context into them." v0.1 had the
//!   deterrent and `llm_query` fired in only 5/40 runs.
//! - **One big sub-call allowed** — the WORKFLOW section no longer nudges
//!   against passing a slice larger than the per-call chunk size when it fits
//!   the sub-model's window.
//! - **Multi-hop helper** — REPL bootstrap exposes a generic
//!   `extract_event_table(text, regex)` helper, callable from any model.
//!
//! Fix 1 (route by size) and Fix 5 (forced-finish unification) are at
//! `router.rs` and `rlm.rs`, not here.

use crate::providers::{CapabilitySet, ModelProfile};

/// System-prompt template version. Bump on every meaningful template change
/// so the eval rig can attribute eval deltas to prompt edits. The eval
/// runner reads this string and stamps it into each cell's `prompt_version`
/// field (see `examples/eval.rs`).
pub const PROMPT_VERSION: &str = "v0.2";

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
/// - The first-turn "inspect the context first" safeguard, plus the v0.2
///   followup per-call + context-size prompt pattern
///   (peer-aligned with ReCLamO-Harness `build_user_message` at 9ac890d).
/// - Late nudges at 60 % and 85 % of `max_iterations`.
/// - A regex-futility nudge if `regex_futility_turns` ≥ N.
///
/// `context_chars` is the number of chars in the user's `context`. When
/// `context_chars <= subcall_chars`, the init message appends "the whole
/// `context` fits in a single call" and the per-context "you have not used
/// a single sub-call yet" nudge is suppressed for the rest of the run
/// (decomposing the context is not the right move when one sub-call
/// already sees it all).
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
    context_chars: usize,
    subcall_chars: u32,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    if turn == 1 {
        let subcall_fmt = thousands_sep(subcall_chars as u64);
        let context_fmt = thousands_sep(context_chars as u64);
        // v0.2 followup: print both the per-call char budget and the
        // context size, then tell the model not to over-decompose.
        let mut s = format!(
            "Question:\n{query}\n\n\
             Turn 1/{max_iterations}: First, inspect `context`. \n\
             Then form a plan and begin working. After each useful intermediate result, \n\
             call `commit(text)` to update `answer['content']`.\n\n\
             Each call can read about {subcall_fmt} characters, so don't be afraid to put a lot of context into one call. \
             Analyze your data and see if it is sufficient to just fit it in a few sub-LLM calls."
        );
        if context_chars as u64 <= subcall_chars as u64 {
            // Whole context fits in one sub-call — say so explicitly. The
            // model is supposed to *consider* using one call, not auto-do
            // it, so the rest of the prompt (orchestrator addendum) still
            // encourages a plan first.
            s.push_str(&format!(
                " The whole `context` ({context_fmt} characters) fits in a single call."
            ));
        }
        parts.push(s);
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

    let context_fits = context_chars as u64 <= subcall_chars as u64;
    if regex_futility_turns >= 3 {
        parts.push(format!(
            "You have run regex for {regex_futility_turns} turns without a useful match. \n\
             Consider switching strategies: use a chunked `llm_query` over sample slices of \n\
             `context`, or build an event table with the `extract_event_table` helper, then \n\
             apply corrections in code."
        ));
    } else if max_subcalls_used == 0 && turn >= 4 && !context_fits {
        // v0.2 followup: suppress the "no sub-call yet" nudge when the
        // context already fits in one sub-call — the right answer is to
        // *not* decompose, not to be nagged into decomposing.
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
        let m = build_user_message("find the bug", 1, 20, 0, 64, false, 0, "", 60_000, 20_000);
        assert!(m.contains("Turn 1/20"));
        assert!(m.contains("inspect"));
    }

    #[test]
    fn user_message_late_nudge_at_85_percent() {
        let m = build_user_message("q", 17, 20, 0, 64, true, 0, "", 60_000, 20_000);
        assert!(m.contains("85 %"));
        assert!(m.contains("FINAL_VAR(answer)"));
    }

    #[test]
    fn user_message_nudges_after_fruitless_regex() {
        let m = build_user_message("q", 5, 20, 0, 64, true, 4, "", 60_000, 20_000);
        assert!(m.contains("regex for 4 turns"));
        assert!(m.contains("extract_event_table"));
    }

    #[test]
    fn user_message_prompts_delegation_when_zero_subcalls_at_turn_4() {
        // 60,000-char context does not fit in 20,000-char sub-calls →
        // "no sub-call yet" nudge is appropriate.
        let m = build_user_message("q", 4, 20, 0, 64, false, 0, "", 60_000, 20_000);
        assert!(m.contains("not used a single sub-call"));
    }

    #[test]
    fn user_message_silenced_when_delegation_already_used() {
        let m = build_user_message("q", 4, 20, 5, 64, false, 0, "", 60_000, 20_000);
        assert!(!m.contains("not used a single sub-call"));
    }

    #[test]
    fn user_message_warns_docker() {
        let m = build_user_message("q", 2, 20, 0, 64, false, 0, "docker", 60_000, 20_000);
        assert!(m.contains("network none"));
    }

    // --- v0.2 followup tests: per-call size + "fits in one call" pattern ---

    fn args() -> (usize, u32) {
        (60_000, 20_000)
    }

    #[test]
    fn user_message_init_prints_per_call_chars() {
        // v0.2 followup: turn 1 must state the per-call char budget so the
        // model can size its sub-calls. (Peer-aligned with the
        // "Each call can read about N characters" pattern.)
        let (context_chars, subcall_chars) = args();
        let m = build_user_message("q", 1, 30, 0, 256, false, 0, "", context_chars, subcall_chars);
        assert!(m.contains("Each call can read about 20,000 characters"));
        assert!(m.contains("don't be afraid to put a lot of context into one call"));
    }

    #[test]
    fn user_message_init_appends_fits_in_one_call_when_context_fits() {
        // context 8,000 chars, subcall budget 20,000 → fits.
        let m = build_user_message("q", 1, 30, 0, 256, false, 0, "", 8_000, 20_000);
        assert!(m.contains("The whole `context` (8,000 characters) fits in a single call"));
    }

    #[test]
    fn user_message_init_omits_fits_note_when_context_oversize() {
        // context 60,000 chars, subcall budget 20,000 → does NOT fit.
        let m = build_user_message("q", 1, 30, 0, 256, false, 0, "", 60_000, 20_000);
        assert!(!m.contains("fits in a single call"));
    }

    #[test]
    fn user_message_no_subcall_nudge_suppressed_when_context_fits() {
        // v0.2 followup: a model that has not used a sub-call by turn 4
        // should NOT be nagged when the context already fits in one call.
        // That would push it to decompose a context it can already see.
        let m = build_user_message("q", 4, 30, 0, 256, false, 0, "", 8_000, 20_000);
        assert!(!m.contains("not used a single sub-call"));
    }

    #[test]
    fn user_message_no_subcall_nudge_fires_when_context_oversize() {
        // Same shape, but context does not fit → nudge fires as before.
        let m = build_user_message("q", 4, 30, 0, 256, false, 0, "", 60_000, 20_000);
        assert!(m.contains("not used a single sub-call"));
    }

    #[test]
    fn prompt_version_constant_is_v0_2() {
        assert_eq!(PROMPT_VERSION, "v0.2");
    }

    #[test]
    fn prompt_default_subcall_chars_interpolates_20_000() {
        // With the v0.2 default the prompt must mention 20,000 — not 12,000.
        let p = profile(8192, 20_000);
        let caps = CapabilitySet::openai_compat();
        let s = build_system_prompt(PromptInputs { profile: &p, caps: &caps });
        assert!(s.text.contains("20,000"), "prompt did not interpolate 20,000 default; got:\n{}", s.text);
        assert!(!s.text.contains("12,000"));
    }

    #[test]
    fn prompt_contains_orchestrator_addendum() {
        let p = profile(8192, 20_000);
        let caps = CapabilitySet::openai_compat();
        let s = build_system_prompt(PromptInputs { profile: &p, caps: &caps });
        // Section marker must be present.
        assert!(s.text.contains("ORCHESTRATOR ROLE"));
        // Every paragraph of the upstream addendum must be present, in order,
        // at least by its first sentence.
        assert!(s.text.contains("act as an orchestrator, not a solver"));
        assert!(s.text.contains("pause and plan"));
        assert!(s.text.contains("Your own context window is small"));
        assert!(s.text.contains("Sub-LLMs have no REPL"));
        assert!(s.text.contains("Sub-call budget is finite on two independent axes"));
        assert!(s.text.contains("Reserve your own tokens for high-level decisions"));
    }

    #[test]
    fn prompt_does_not_contain_v0_1_delegation_deterrent() {
        let p = profile(8192, 20_000);
        let caps = CapabilitySet::openai_compat();
        let s = build_system_prompt(PromptInputs { profile: &p, caps: &caps });
        // The Qwen3-Coder-only "use sub-calls for judgement, not for line-by-line
        // scanning" / "DELEGATION RULES" patch is gone in v0.2.
        assert!(!s.text.contains("DELEGATION RULES"));
        assert!(!s.text.contains("use them for judgement, not for line-by-line scanning"));
        assert!(!s.text.contains("Use them for judgement, not for line-by-line scanning"));
        assert!(!s.text.contains("Spend them on judgement"));
    }

    #[test]
    fn prompt_workflow_allows_one_big_subcall() {
        let p = profile(8192, 20_000);
        let caps = CapabilitySet::openai_compat();
        let s = build_system_prompt(PromptInputs { profile: &p, caps: &caps });
        // v0.1 said "state the per-call chunk size" which nudged against
        // fat-prompt sub-calls. v0.2 explicitly allows a larger slice.
        assert!(s.text.contains("larger slice in a single sub-call"));
    }
}
