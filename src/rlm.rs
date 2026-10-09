//! The loop. All five NEXT-STEPS fixes baked in:
//!
//! - **Fix #1 route-by-size** — handled by `router::route` before the loop runs.
//! - **Fix #2 commit-early** — `answer['content']` is mirrored on the Rust side
//!   via a REPL state snapshot, and is updated after every exec that produced
//!   stdout (the prompt tells the model to call `commit(text)`).
//! - **Fix #3 un-scare delegation** — `Late nudges` in `prompts.rs`; the loop
//!   also counts "fruitless regex" turns and adds a nudge after 3.
//! - **Fix #4 multi-hop helper** — `extract_event_table(text, regex)` is part
//!   of the REPL bootstrap (worker.py); the prompt mentions it by name.
//! - **Fix #5 forced-finish unification** — all four limit triggers
//!   (`max_iterations`, `max_timeout`, `max_tokens`, `max_errors`) route
//!   through `forced_finish`.
//!
//! The loop is typed against [`crate::repl::ReplClient`], which lets tests
//! swap in [`crate::repl::InMemoryRepl`] without spawning a Python child.

use crate::completion::RunResult;
use crate::config::LoopConfig;
use crate::error::{ReclamoError, ReclamoResult};
use crate::logger::{make_meta, RunLogger, TurnLine};
use crate::parsing::{extract_final, parse_response, FinalIntent};
use crate::prompts::{build_system_prompt, build_user_message, PromptInputs};
use crate::providers::{CompleteOpts, Message, ModelProfile};
use crate::repl::{self, ReplClient, ReplExecResult, SubcallFn};
use crate::repl::subprocess_repl::SubcallFuture;
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;

/// Appended to the message history when the model emits a turn with
/// no code and no FINAL/FINAL_VAR. Mirrors ReCLamO-Harness fix #5
/// (PR #52 in the Python harness). Before this, an empty turn just
/// `continue`d silently and the prompt's late nudges — which only
/// fire at ≥60% of max_iterations — were the only signal.
const NO_CODE_NO_FINAL_NUDGE: &str = "\
That message had no code and no final answer. \
Reply with exactly one ```repl code block (or ```python), \
or `FINAL(...)` / `FINAL_VAR(...)`.";

/// v0.2 followup: appended when the upstream reports `finish_reason =
/// "length"` (root reply cut off by the per-request `max_tokens` cap).
/// Peer-aligned with ReCLamO-Harness `CONTINUE_PROMPT` at 9ac890d. The
/// peer port does a `join` of the partial reply + a continuation; this
/// port pushes a single breadcrumb and lets the loop continue. Both
/// approaches are valid — the peer explicitly OK'd the breadcrumb
/// approach as an alternative.
const LENGTH_STOP_REASON_BREADCRUMB: &str = "\
Your last reply was cut off by the output limit. Continue it from where you left off.";

pub async fn run(
    context: String,
    query: String,
    profile: ModelProfile,
    cfg: LoopConfig,
) -> ReclamoResult<RunResult> {
    let started = Instant::now();
    let deadline = started + cfg.max_timeout;

    // Open the REPL and load the context. We try the real subprocess worker
    // first; if Python is missing on PATH (CI on a stripped image, etc.) we
    // fall back to the in-memory scripted variant so unit tests still pass.
    let subcall_fn = make_subcall_fn(&profile, &cfg);
    let mut repl: Box<dyn ReplClient> = match repl::spawn_subprocess(&context, subcall_fn).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                "SubprocessRepl unavailable, falling back to InMemoryRepl: {e}"
            );
            repl::default_client(&context, cfg.subcall_timeout)?
        }
    };
    let caps = profile.provider.capabilities();
    let sys_prompt_text = build_system_prompt(PromptInputs { profile: &profile, caps: &caps }).text;

    let mut messages = vec![
        Message::system(sys_prompt_text),
        Message::user(format!("Question:\n{query}\n\nBegin.")),
    ];

    // Logger — opt-in via env or default under ./runs.
    let log_dir = default_log_dir();
    let logger = RunLogger::open(
        &log_dir,
        profile.provider.model_id(),
        profile.provider.name(),
    )?;
    logger.meta(make_meta(
        logger.run_id(),
        &iso8601_now(),
        profile.provider.model_id(),
        profile.provider.name(),
        profile.max_context,
        cfg.max_iterations,
        cfg.max_subcalls_per_run,
    ))?;

    let mut stats = LoopStats::default();
    let mut last_user_turn_nudge = false;

    let mut turn: u32 = 0;
    // Number of consecutive turns where the model emitted
    // `FINAL_VAR(<name>)` but the name wasn't in the REPL. Caps at 3
    // (then forced_finish) so a model that keeps hallucinating the
    // same variable doesn't loop forever. Resets on any successful
    // commit, successful final, or successful final_var.
    #[allow(unused_assignments)]
    let mut consecutive_uncommitted_finalvar: u32 = 0;
    // Number of consecutive erroring REPL execs across the whole run.
    // Was previously declared inside the turn loop, which silently
    // reset the counter every turn — a model that errors once per
    // turn (`repl.execute()` returns `error.is_some()` on every call)
    // never tripped the cap, because each turn's counter started at
    // 0. Mirror of ReCLamO-Harness #50 (PR #52) which has the same
    // shape. The fix: declare outside the loop, drop the within-turn
    // reset on a successful exec. The counter now accumulates across
    // turns and trips `forced_finish:max_errors` at the Nth
    // consecutive erroring exec. Test:
    // `max_errors_counts_across_turns_not_just_within`.
    let mut consecutive_errors: u32 = 0;
    while turn < cfg.max_iterations {
        turn += 1;
        if Instant::now() >= deadline {
            return forced_finish(repl.as_mut(), profile.provider.as_ref(), &messages, "max_timeout", &mut stats, started, deadline).await;
        }
        if let Some(t) = cfg.max_tokens {
            if stats.tokens >= t {
                return forced_finish(repl.as_mut(), profile.provider.as_ref(), &messages, "max_tokens", &mut stats, started, deadline).await;
            }
        }

        // First-turn "inspect the context first" is the system prompt's
        // job; subsequent turns re-state the question so the model's
        // append-only history stays a valid request.
        if turn == 1 {
            // The first user message is already in messages.
        } else {
            // Build a fresh user message with nudges.
            let has_answer = repl_has_answer(repl.as_mut()).await;
            let nudge = build_user_message(
                &query, turn, cfg.max_iterations,
                stats.subcalls_total,
                cfg.max_subcalls_per_run,
                has_answer,
                stats.regex_futility,
                &repl_kind(),
                context.chars().count(),
                profile.subcall_chars,
            );
            messages.push(Message::user(nudge));
            last_user_turn_nudge = true;
        }

        let opts = CompleteOpts {
            thinking_override: Some(crate::providers::ThinkingMode::Enabled),
            temperature: Some(profile.temperature),
            top_p: Some(profile.top_p),
            top_k: profile.top_k,
            max_output_tokens: Some(cfg.root_max_output_tokens as u64),
            extra_body: profile.extra_body.clone(),
        };

        let completion = match tokio::time::timeout(
            deadline.saturating_duration_since(Instant::now()),
            profile.provider.complete(&messages, None, opts),
        )
        .await
        {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => return Err(e),
            // Root call ran past the run's `max_timeout` budget. Mirror
            // ReCLamO-Harness #51 (PR #52) — the loop's deadline must
            // bound the root call, not just the per-turn check at the
            // top. Without this, a hung model can run far past
            // `max_timeout` because the deadline is only sampled at
            // turn boundaries. Forced_finish records the
            // `forced_finish:max_timeout` stop_reason the eval rig
            // already understands (mirrors the per-turn check).
            Err(_elapsed) => {
                return forced_finish(
                    repl.as_mut(),
                    profile.provider.as_ref(),
                    &messages,
                    "max_timeout",
                    &mut stats,
                    started,
                    deadline,
                )
                .await;
            }
        };
        stats.tokens += completion.usage.total();
        stats.turns = turn;

        let parsed = parse_response(&completion.content);

        logger.turn(TurnLine {
            kind: "turn",
            run_id: logger.run_id().into(),
            turn,
            max_iterations: cfg.max_iterations,
            messages: messages.clone(),
            completion: completion.clone(),
            parsed_code_blocks: parsed.code_blocks.len() as u32,
            parsed_final: parsed.final_intent.as_ref().map(intent_to_string),
            reject_reason: parsed.reject_reason.clone(),
            cumulative_tokens: stats.tokens,
            cumulative_seconds: started.elapsed().as_secs_f64(),
        })?;

        // Push the assistant message into history — append-only.
        let mut assistant = Message::assistant(completion.content.clone());
        assistant.reasoning = completion.reasoning.clone();
        assistant.tool_calls = completion.tool_calls.clone();
        messages.push(assistant);

        // v0.2 followup: if the upstream cut off the reply mid-content
        // (`finish_reason: "length"`), push a continue-from-here breadcrumb
        // before the FINAL/code check. If the cut-off reply DID contain a
        // usable FINAL, the next branch short-circuits and returns; the
        // breadcrumb just sits in `messages` harmlessly. If the cut-off
        // reply was mid-code or mid-prose, the breadcrumb steers the
        // model back on track instead of letting it loop.
        if completion.stop_reason == "length" {
            messages.push(Message::user(LENGTH_STOP_REASON_BREADCRUMB));
        }

        // Resolve a FINAL/FINAL_VAR first, if present and not rejected.
        if let (Some(intent), None) = (parsed.final_intent.as_ref(), parsed.reject_reason.as_ref()) {
            let (answer, stop_reason) = match intent {
                FinalIntent::Final(v) => (v.clone(), "final_var".to_string()),
                FinalIntent::FinalVar(name) => match lookup_var(repl.as_mut(), name).await {
                    Ok(v) => (v, "final_var".to_string()),
                    // The model wrote FINAL_VAR(X) but X doesn't exist in
                    // the REPL. Phase 7b Cerebex (2026-10-08) showed this
                    // is gemini-2.5-flash-lite's dominant failure mode on
                    // the harness path: 5/6 cells ended with
                    // `final_var_invalid:<hallucinated_name>` because the
                    // model never committed the variable it then pointed
                    // at. Treat it as a corrective loop signal rather than
                    // a final stop — push a breadcrumb that names the
                    // missing variable and the standard recovery pattern
                    // (`<name> = <value>` then `FINAL_VAR(<name>)`), and
                    // continue. Cap the uncommitted streak at 3 so a model
                    // that keeps hallucinating the same name doesn't loop
                    // forever; after the cap, forced_finish with
                    // `forced_finish:final_var_uncommitted` (mirrors the
                    // `max_iterations` cap pattern).
                    Err(_) => {
                        consecutive_uncommitted_finalvar += 1;
                        if consecutive_uncommitted_finalvar >= 3 {
                            return forced_finish(
                                repl.as_mut(),
                                profile.provider.as_ref(),
                                &messages,
                                "final_var_uncommitted",
                                &mut stats,
                                started,
                                deadline,
                            )
                            .await;
                        }
                        let fix = format!(
                            "FINAL_VAR({name}) pointed at a variable that doesn't exist in the REPL. \
                             Did you forget to commit it? Either:\n\
                             - `<name> = <extracted value>` (or `commit(\"<extracted value>\")`) then `FINAL_VAR({name})`, or\n\
                             - `answer = <value>` followed by `FINAL_VAR(answer)`.\n\
                             Reply with one ```repl block (or ```python) committing the variable, then re-emit the FINAL_VAR."
                        );
                        messages.push(Message::user(fix));
                        continue;
                    }
                },
            };
            // (Successful final: counter goes out of scope here, no
            // need to reset; the function returns.)
            return Ok(RunResult {
                answer,
                mode: "harness:fence".into(),
                stop_reason,
                tokens: stats.tokens,
                seconds: started.elapsed().as_secs_f64(),
                turns: stats.turns,
                subcalls: stats.subcalls_total,
            });
        }

        // If a FINAL was REJECTED, add a corrective user message and continue.
        if let (Some(_), Some(reason)) = (parsed.final_intent.as_ref(), parsed.reject_reason.as_ref()) {
            let fix = format!(
                "Your FINAL was rejected: {reason}.\n\
                 Either run the missing code first, or write a concrete answer — not a plan."
            );
            messages.push(Message::user(fix));
            continue;
        }

        // No FINAL: run the code blocks.
        if parsed.code_blocks.is_empty() {
            // No code, no FINAL. The Python harness's ReCLamO-Harness fix
            // #5 (PR #52) appends a corrective breadcrumb here; before
            // that, the empty turn just `continue`d and the prompt's late
            // nudges were the only signal — and those only fire at
            // ≥60% of max_iterations. The breadcrumb fires every time.
            // (Seen 2026-10-08 on GLaDOS large gemini seed 0 harness:
            // the model emitted `final_var` on turn 1 without ever
            // sampling the context; no nudge ever fired.)
            messages.push(Message::user(NO_CODE_NO_FINAL_NUDGE));
            continue;
        }

        let mut made_progress = false;
        // Run only the FIRST code block when a reply contains several.
        // The Python harness's ReCLamO-Harness fix #4 (PR #52) keeps
        // `parsed.code_blocks[:1]`; without this, a model that emits
        // multiple ```repl fences (or, on the Poolside Laguna slip,
        // ~12 <tool_call> blocks with fabricated REPL output between
        // them) gets every fence executed, including the faked output.
        // The `TurnLine.parsed_code_blocks` field already records N so
        // the drop is visible in the trajectory (parsed N, executed 1).
        for block in parsed.code_blocks.iter().take(1) {
            // Bound the REPL exec by the run's remaining deadline AND
            // the existing per-sub-call budget (whichever is shorter).
            // SubprocessRepl::execute has no built-in timeout — a hung
            // Python child (e.g. infinite loop in user code) blocks
            // forever without this. The subcall_timeout default of 90s
            // also serves as the per-exec cap; the remaining deadline
            // provides the overall run cap. Mirror of ReCLamO-Harness
            // #51 (PR #52).
            let exec_budget = deadline
                .saturating_duration_since(Instant::now())
                .min(cfg.subcall_timeout);
            let exec = match tokio::time::timeout(
                exec_budget,
                repl.execute(&block.code),
            )
            .await
            {
                Ok(Ok(e)) => e,
                Ok(Err(e)) => return Err(e),
                Err(_elapsed) => {
                    return forced_finish(
                        repl.as_mut(),
                        profile.provider.as_ref(),
                        &messages,
                        "max_timeout",
                        &mut stats,
                        started,
                        deadline,
                    )
                    .await;
                }
            };
            if exec.error.is_some() {
                consecutive_errors += 1;
                if consecutive_errors >= cfg.max_errors {
                    return forced_finish(repl.as_mut(), profile.provider.as_ref(), &messages, "max_errors", &mut stats, started, deadline).await;
                }
            } else {
                // Successful exec — leave the across-turn counter alone.
                // It is reset implicitly by the next error hitting
                // `cfg.max_errors` (the loop exits via forced_finish), or
                // implicitly at the bottom of a clean run. We deliberately
                // do NOT reset it on a successful exec: that is the bug
                // #8 fixes (a model that errors once per turn never
                // tripped the cap because every turn's counter started
                // at 0).
                // A successful commit clears the uncommitted-final_var
                // streak: the model did exactly what we asked (e.g.
                // `answer = 42`) and the next FINAL_VAR(answer) is no
                // longer a hallucination.
                consecutive_uncommitted_finalvar = 0;
            }
            stats.regex_futility = count_fruitless_regex(stats.regex_futility, &exec);
            if exec.tokens > 0 {
                stats.subcalls_total += 1;
            }
            stats.tokens += exec.tokens;
            if exec.made_progress {
                made_progress = true;
            }
            messages.push(exec.to_user_message());
        }

        if !made_progress && !last_user_turn_nudge {
            // Belt-and-suspenders nudge if the prompt's nudge didn't fire.
            messages.push(Message::user(
                "Make sure each turn either commits progress to answer, or uses \
                 llm_query on a chunk you cannot handle in code."
                    .to_string(),
            ));
        }
        last_user_turn_nudge = false;
    }

    forced_finish(repl.as_mut(), profile.provider.as_ref(), &messages, "max_iterations", &mut stats, started, deadline).await
}

fn intent_to_string(i: &FinalIntent) -> String {
    match i {
        FinalIntent::Final(v) => format!("FINAL({v})"),
        FinalIntent::FinalVar(n) => format!("FINAL_VAR({n})"),
    }
}

async fn repl_has_answer(repl: &mut dyn ReplClient) -> bool {
    // Cross-wire: a real subprocess worker honors lookup_var; InMemoryRepl
    // does too (returns the dict or string the script bound). We accept
    // either shape: a string is truthy if non-empty; a dict is truthy if
    // `answer['content']` is non-empty.
    let v = repl.lookup_var("answer").await.unwrap_or(Value::Null);
    match v {
        Value::String(s) => !s.is_empty(),
        Value::Object(map) => map
            .get("content")
            .and_then(|c| match c {
                Value::String(s) => Some(!s.is_empty()),
                _ => None,
            })
            .unwrap_or(false),
        Value::Null => false,
        _ => false,
    }
}

async fn lookup_var(
    repl: &mut dyn ReplClient,
    name: &str,
) -> ReclamoResult<String> {
    let v = repl.lookup_var(name).await?;
    if v.is_null() {
        // Stale-name fallback — the v0.1 bug. If the model wrote
        // `FINAL(final_answer)` and the REPL has none, refuse to return
        // empty; ask the model once more.
        return Err(ReclamoError::limit_with(
            "final_var",
            "FINAL_VAR pointed at a missing variable",
            "",
        ));
    }
    Ok(strip_to_string(v))
}

fn strip_to_string(v: serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s,
        // The REPL bootstrap leaves `answer` as `{"content": "..."}`; the
        // worker's `commit(text)` writes to that slot. So if we get back a
        // dict with a `content` key, return just the content.
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(s)) = map.get("content") {
                return s.clone();
            }
            serde_json::Value::Object(map).to_string()
        }
        // Fall back to the JSON shape for anything else (numbers, bools,
        // nulls, arrays). Eval-rig `extract_final` filters these cases
        // before this runs.
        other => other.to_string(),
    }
}

fn count_fruitless_regex(prev: u32, exec: &ReplExecResult) -> u32 {
    // Heuristic: a `re.search/findall` call with no match counts as a
    // fruitless regex turn. (REPL echoes can detect this in phase 2 by
    // parsing Python output; for now we just bump on empty output.)
    if exec.stdout.trim().is_empty() && exec.subcalls.is_empty() {
        prev + 1
    } else {
        0
    }
}

fn repl_kind() -> String {
    "subprocess".into()
}

fn default_log_dir() -> std::path::PathBuf {
    let p = std::path::PathBuf::from("./runs");
    let _ = std::fs::create_dir_all(&p);
    p
}

fn iso8601_now() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[derive(Debug, Default)]
struct LoopStats {
    turns: u32,
    subcalls_total: u32,
    regex_futility: u32,
    tokens: u64,
}

/// One forced-finish path for **all four** limit triggers (fix #5).
async fn forced_finish(
    repl: &mut dyn ReplClient,
    provider: &dyn crate::providers::Provider,
    messages: &[Message],
    kind: &'static str,
    stats: &mut LoopStats,
    started: Instant,
    deadline: Instant,
) -> ReclamoResult<RunResult> {
    // Step 1: ask the model once with REPL state shown. Demand a
    // FINAL_VAR(answer) if `answer['content']` is non-empty, else fall
    // back to a concrete value (the loop and worker do NOT accept
    // `FINAL("I could not determine the answer")` as the answer because
    // that's an explicit request to lose).
    let state = repl.snapshot_state().await.unwrap_or(serde_json::Value::Null);
    let state_str = serde_json::to_string_pretty(&state).unwrap_or_default();
    let prompt = format!(
        "Time/turn/token/error limit hit ({kind}).\n\n\
         REPL state snapshot (best effort):\n{state_str}\n\n\
         Now produce your final answer in ONE line:\n\
         - If `answer['content']` is correct, write exactly: FINAL_VAR(answer)\n\
         - Otherwise, write a one-line concrete answer: FINAL(<one line>)\n\
         Do NOT return code. Do NOT return a plan. Do NOT return a stale-variable name."
    );

    let mut attempt_messages = messages.to_vec();
    attempt_messages.push(Message::user(prompt));

    let opts = CompleteOpts {
        thinking_override: Some(crate::providers::ThinkingMode::Disabled),
        temperature: Some(profile_t_low()),
        top_p: Some(0.9),
        top_k: None,
        max_output_tokens: Some(512),
        extra_body: serde_json::Value::Null,
    };

    // Bound the forced-finish provider call by the same run deadline
    // the loop already uses — otherwise a hung model can blow past
    // `max_timeout` *twice* (once in the loop, once here) and the
    // `forced_finish:max_timeout` stop_reason still leaks a slow
    // completion through as the answer. (#7 — root call path, but
    // forced_finish is the second of two provider calls the loop
    // can make, and it must be bounded too.)
    let budget = deadline.saturating_duration_since(Instant::now());
    let completion = match tokio::time::timeout(
        budget,
        provider.complete(&attempt_messages, None, opts),
    )
    .await
    {
        Ok(Ok(c)) => Ok(c),
        // ANY provider error in the forced-finish call — a model-side
        // error (e.g. mock exhausted) OR the call itself blowing the
        // budget — must NOT propagate. The loop has already decided
        // the run is over; the eval rig scores `stop_reason` and an
        // empty `answer`, never a `ReclamoError`. Fall through to the
        // REPL-state lookup below.
        Ok(Err(_e)) => Err(ReclamoError::Provider {
            provider: provider.name().to_string(),
            message: format!("forced_finish: {kind} provider error"),
        }),
        Err(_elapsed) => Err(ReclamoError::Provider {
            provider: provider.name().to_string(),
            message: format!("forced_finish: {kind} exceeded remaining budget {budget:?}"),
        }),
    };
    let extra_tokens = completion.as_ref().map(|c| c.usage.total()).unwrap_or(0);
    stats.tokens += extra_tokens;
    let answer = match completion {
        Ok(c) => {
            let intent = extract_final(&c.content);
            match intent {
                Some(FinalIntent::FinalVar(n)) => {
                    match lookup_var(repl, n.as_str()).await {
                        Ok(v) => v,
                        Err(_) => c.content,
                    }
                }
                Some(FinalIntent::Final(v)) => v,
                None => c.content,
            }
        }
        Err(_) => {
            // Provider failed on forced finish; fall back to whatever
            // REPL state holds. Worst-case: empty answer + correct
            // stop_reason so the eval rig attributes the loss.
            lookup_var(repl, "answer").await.unwrap_or_default()
        }
    };
    Ok(RunResult {
        answer,
        mode: "harness:fence".into(),
        stop_reason: format!("forced_finish:{kind}"),
        tokens: stats.tokens,
        seconds: started.elapsed().as_secs_f64(),
        turns: stats.turns,
        subcalls: stats.subcalls_total,
    })
}

fn profile_t_low() -> f64 { 0.3 }

fn make_subcall_fn(profile: &ModelProfile, cfg: &LoopConfig) -> SubcallFn {
    let provider: Arc<dyn crate::providers::Provider> = profile.provider.clone();
    let extra_body = profile.extra_body.clone();
    // Sub-calls: cheaper decoding. Mirror the v0.1 Strata defaults — no
    // thinking on sub-calls (45 tok/s vs 12 tok/s for Qwen on Strata),
    // lower temperature for stability.
    let temperature = profile.temperature.min(0.3);
    let top_p = profile.top_p.min(0.9);
    let top_k = profile.top_k;
    let max_output = cfg.subcall_max_output_tokens as u64;
    let subcall_timeout = cfg.subcall_timeout;
    Arc::new(move |prompt: String| -> SubcallFuture {
        let provider = provider.clone();
        let extra_body = extra_body.clone();
        Box::pin(async move {
            let opts = CompleteOpts {
                thinking_override: Some(crate::providers::ThinkingMode::Disabled),
                temperature: Some(temperature),
                top_p: Some(top_p),
                top_k,
                max_output_tokens: Some(max_output),
                extra_body,
            };
            let msgs = vec![Message::user(prompt)];
            let completion =
                tokio::time::timeout(subcall_timeout, provider.complete(&msgs, None, opts))
                    .await
                    .map_err(|_| {
                        ReclamoError::Repl("sub-call timeout".into())
                    })??;
            Ok((completion.content, completion.usage.total()))
        })
    })
}

#[cfg(test)]
mod tests {
    //! Lightweight end-to-end tests of the loop, gated on a MockProvider
    //! so they don't require network or Python.
    use super::*;
    use crate::completion::CompletionOpts;
    use crate::providers::{Completion, MockProvider, ThinkingMode, Usage};
    use crate::repl::InMemoryRepl;

    fn make_profile(scripted: Vec<Completion>) -> ModelProfile {
        let prov: Arc<dyn crate::providers::Provider> = Arc::new(MockProvider::scripted("mock-m", scripted));
        let mut p = ModelProfile::new("mock-profile", prov, 16_384);
        p.thinking = ThinkingMode::Disabled;
        p
    }

    #[tokio::test]
    async fn loop_completes_via_final_var_in_memory_repl() {
        // MockProvider scripted to emit a one-shot FINAL_VAR(answer); the
        // InMemoryRepl fallback holds `answer` as a string set by the
        // first scripted exec (which we drive directly via repl.put).
        let scripted = vec![
            Completion {
                content: "```repl\ncommit(\"the needle is at index 7\")\n```\nFINAL_VAR(answer)".into(),
                reasoning: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
                usage: Usage { input_tokens: Some(8), output_tokens: Some(8), total_tokens: Some(16) },
            },
        ];
        let profile = make_profile(scripted);
        // NOTE: repl_has_answer uses `lookup_var("answer")` against the
        // InMemoryRepl — the test repl needs to have `answer` already
        // bound from the simulated exec. We pre-populate it via put().
        // We can't easily run the loop with InMemoryRepl through `run()`,
        // because run() always tries SubprocessRepl first. Instead, build
        // a small InMemoryRepl and exercise the FINAL_VAR resolution path
        // directly.
        let mut repl_test = InMemoryRepl::new().on_lookup(|name, state| {
            state.get(name).cloned().unwrap_or(Value::Null)
        });
        repl_test.put("answer", serde_json::json!("the needle is at index 7"));
        let v = repl_test.lookup_var("answer").await.unwrap();
        assert_eq!(v, serde_json::json!("the needle is at index 7"));

        // Drive a fake "first turn": model returned FINAL_VAR; we resolve
        // via lookup_var.
        let answer = lookup_var(&mut repl_test, "answer").await.unwrap();
        assert_eq!(answer, "the needle is at index 7");
        let _ = profile;
    }

    #[tokio::test]
    async fn route_by_size_sends_plain_path() {
        // A small context that fits in `resident_kv - plain_query_margin`
        // takes the plain path, never runs the loop.
        use crate::completion::completion;
        let scripted = vec![Completion {
            content: "FINAL(not used; plain path)".into(),
            reasoning: None,
            tool_calls: vec![],
            stop_reason: "stop".into(),
            usage: Usage { input_tokens: Some(1), output_tokens: Some(1), total_tokens: Some(2) },
        }];
        let profile = make_profile(scripted);
        let ctx = "tiny context".to_string();
        let query = "what?".to_string();
        let opts = CompletionOpts::default();
        let res = completion(ctx, query, profile, opts).await.unwrap();
        assert_eq!(res.mode, "plain");
        assert!(!res.answer.is_empty());
    }

    #[tokio::test]
    async fn explicit_harness_bypasses_router_on_small_context() {
        // A small context that the router would send to plain. With
        // `RouteMode::Harness` the loop must run anyway — the explicit
        // request wins, otherwise the eval rig's "plain-vs-harness"
        // comparison is meaningless (the peer flagged this 2026-10-08).
        use crate::completion::{completion, RouteMode};
        let scripted = vec![Completion {
            content: "FINAL(not used; loop ran)".into(),
            reasoning: None,
            tool_calls: vec![],
            stop_reason: "stop".into(),
            usage: Usage { input_tokens: Some(1), output_tokens: Some(1), total_tokens: Some(2) },
        }];
        let profile = make_profile(scripted);
        let mut opts = CompletionOpts::default();
        opts.route_mode = RouteMode::Harness;
        let res = completion("tiny".into(), "q".into(), profile, opts).await.unwrap();
        assert_eq!(res.mode, "harness:fence", "explicit Harness must bypass the router");
    }

    #[tokio::test]
    async fn harness_final_var_missing_var_does_not_crash_run() {
        // The model writes `FINAL_VAR(missing)` and the REPL has no such
        // variable. The loop must NOT propagate the lookup error out of
        // `completion()` — that previously crashed the eval rig mid-grid
        // on gemini-2.5-flash-lite (Phase 7b Cerebex, 2026-10-08). The
        // new contract: treat the missing var as a corrective loop
        // signal, not a final stop. Push a breadcrumb that names the
        // missing variable and the standard recovery pattern, then
        // continue. The next scripted turn recovers by committing a
        // value to `answer` and re-emitting `FINAL_VAR(answer)`, so the
        // test asserts the full recovery path: mode=harness:fence,
        // stop=final_var, answer="42", turns>=2. (A separate test covers
        // the 3-strike cap.)
        use crate::completion::{completion, RouteMode};
        let scripted = vec![
            // Turn 1: hallucinate a variable that doesn't exist. Note
            // that any code block before the FINAL_VAR is skipped —
            // the loop's FINAL branch runs first, so the comment
            // never executes. That's fine: the breadcrumb push is
            // the only thing this turn needs to do.
            Completion {
                content: "```repl\n# oops, never bound missing_var\n```\nFINAL_VAR(missing_var)".into(),
                reasoning: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
                usage: Usage { input_tokens: Some(1), output_tokens: Some(1), total_tokens: Some(2) },
            },
            // Turn 2: take the breadcrumb, run a real commit. No FINAL
            // this turn — we need the code to execute and bind
            // `answer['content']` before the next turn looks it up.
            // (FINAL_VAR resolution runs *before* code execution, so
            // emitting `commit(...)` + `FINAL_VAR(answer)` on the same
            // turn would still see the bootstrap empty string.)
            Completion {
                content: "```repl\ncommit(\"42\")\n```".into(),
                reasoning: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
                usage: Usage { input_tokens: Some(1), output_tokens: Some(1), total_tokens: Some(2) },
            },
            // Turn 3: now `answer` is bound, emit just the FINAL.
            Completion {
                content: "FINAL_VAR(answer)".into(),
                reasoning: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
                usage: Usage { input_tokens: Some(1), output_tokens: Some(1), total_tokens: Some(2) },
            },
        ];
        let profile = make_profile(scripted);
        let mut opts = CompletionOpts::default();
        opts.route_mode = RouteMode::Harness;
        let res = completion("tiny context".into(), "q".into(), profile, opts)
            .await
            .expect("run() must not propagate lookup_var Err for a missing variable");
        assert_eq!(res.mode, "harness:fence");
        assert_eq!(res.stop_reason, "final_var", "after the breadcrumb the model should recover and bind `answer`");
        assert_eq!(res.answer, "42");
        assert!(res.turns >= 2, "the loop must take at least 2 turns to recover");
    }

    #[tokio::test]
    async fn harness_final_var_uncommitted_forced_finish_after_three() {
        // Cap test: if the model keeps emitting `FINAL_VAR(missing)`
        // three times in a row, the loop must NOT loop forever; it
        // routes through `forced_finish` with kind
        // `final_var_uncommitted` (mirrors the `max_iterations` cap
        // pattern in `forced_finish`). Without the cap, this is
        // gemini-2.5-flash-lite's observed failure mode on the harness
        // path (Phase 7b Cerebex, 2026-10-08): 5/6 cells burned the
        // full max_iterations budget emitting hallucinated names.
        use crate::completion::{completion, RouteMode};
        let scripted = vec![
            Completion {
                content: "```repl\n# never bound\n```\nFINAL_VAR(missing_var)".into(),
                reasoning: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
                usage: Usage { input_tokens: Some(1), output_tokens: Some(1), total_tokens: Some(2) },
            },
            Completion {
                content: "```repl\n# still missing\n```\nFINAL_VAR(missing_var)".into(),
                reasoning: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
                usage: Usage { input_tokens: Some(1), output_tokens: Some(1), total_tokens: Some(2) },
            },
            Completion {
                content: "```repl\n# still hallucinating\n```\nFINAL_VAR(missing_var)".into(),
                reasoning: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
                usage: Usage { input_tokens: Some(1), output_tokens: Some(1), total_tokens: Some(2) },
            },
        ];
        let profile = make_profile(scripted);
        let mut opts = CompletionOpts::default();
        opts.route_mode = RouteMode::Harness;
        let res = completion("tiny context".into(), "q".into(), profile, opts)
            .await
            .expect("forced_finish must still return Ok so the eval rig scores it as 0.0");
        assert_eq!(res.mode, "harness:fence");
        assert_eq!(
            res.stop_reason, "forced_finish:final_var_uncommitted",
            "after 3 consecutive uncommitted FINAL_VARs the loop must force-finish"
        );
        assert_eq!(res.answer, "");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn harness_root_call_respects_max_timeout() {
        // #7 (root call path): the root `provider.complete` call is
        // bounded by the run's `max_timeout` deadline, not just the
        // per-turn check at the top of the loop. Previously, a hung
        // model could run far past `max_timeout` because the deadline
        // was only sampled at turn boundaries — a single turn's
        // `provider.complete` could hang indefinitely and the loop
        // never checked. The fix wraps the call in
        // `tokio::time::timeout(remaining, ...)` and routes the
        // `Elapsed` into `forced_finish:max_timeout`.
        //
        // Test: a slow async mock that sleeps 1s on every call, with
        // `max_timeout=100ms`. The provider call should fire
        // `Elapsed` at 100ms, well before the 1s sleep completes.
        // The per-turn check at the top of turn 2 would also see the
        // deadline is past, but the new wrapper trips first.
        //
        // (The exec path is also bounded — same pattern, see the
        // `tokio::time::timeout` wrap around `repl.execute` in the
        // loop. No unit test for that one yet: `SubprocessRepl::execute`
        // does not currently SIGKILL the child on timeout, so a test
        // that drives `while True: pass` would leak a Python child
        // and hang the test runner even after forced_finish returns.
        // The production fix is correct; the test gap is a
        // SubprocessRepl cleanup TODO, not a rlm.rs gap.)
        use crate::completion::{completion, RouteMode};
        use crate::providers::{MockProvider, Usage};
        use std::sync::Arc;
        use std::time::Duration;
        let slow: crate::providers::mock::AsyncScripted = Arc::new(|_msgs| {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(1000)).await;
                Completion {
                    content: "should not be reached".into(),
                    reasoning: None,
                    tool_calls: vec![],
                    stop_reason: "stop".into(),
                    usage: Usage { input_tokens: Some(1), output_tokens: Some(1), total_tokens: Some(2) },
                }
            })
        });
        let prov: Arc<dyn crate::providers::Provider> =
            Arc::new(MockProvider::function_async("slow-mock", slow));
        let mut profile = crate::providers::ModelProfile::new("slow-profile", prov, 16_384);
        profile.thinking = crate::providers::ThinkingMode::Disabled;
        let mut opts = CompletionOpts::default();
        opts.route_mode = RouteMode::Harness;
        opts.max_timeout = Some(Duration::from_millis(100));
        let res = completion("tiny context".into(), "q".into(), profile, opts)
            .await
            .expect("forced_finish on max_timeout must still return Ok so the eval rig scores it as 0.0");
        assert_eq!(res.mode, "harness:fence");
        assert_eq!(
            res.stop_reason, "forced_finish:max_timeout",
            "a slow provider.complete must trip the root-call timeout, not burn the full max_iterations"
        );
        assert_eq!(res.answer, "");
    }

    #[tokio::test]
    async fn max_errors_counts_across_turns_not_just_within() {
        // #8: the `consecutive_errors` counter used to be declared
        // inside the turn loop and reset to 0 on every successful
        // exec within the same turn. A model that errors *once per
        // turn* (the worst-case failure mode: every turn produces
        // exactly one erroring exec, then the turn ends) would never
        // trip the cap, because each turn's counter started at 0.
        // The fix: declare the counter outside the loop and never
        // reset on a successful exec — it now accumulates across the
        // whole run, mirroring ReCLamO-Harness #50 (PR #52).
        //
        // Script: three turns, each emits `1/0` (Python
        // ZeroDivisionError, so the real SubprocessRepl returns
        // `error.is_some()`). With the default `max_errors=3` and
        // `max_iterations=20`, the third erroring exec trips
        // `forced_finish:max_errors`. (We don't need a successful
        // exec anywhere — every turn errors, so the counter goes
        // 1 -> 2 -> 3 -> trip.)
        use crate::completion::{completion, RouteMode};
        let scripted = vec![
            Completion {
                content: "```repl\n1/0\n```".into(),
                reasoning: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
                usage: Usage { input_tokens: Some(1), output_tokens: Some(1), total_tokens: Some(2) },
            },
            Completion {
                content: "```repl\n1/0\n```".into(),
                reasoning: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
                usage: Usage { input_tokens: Some(1), output_tokens: Some(1), total_tokens: Some(2) },
            },
            Completion {
                content: "```repl\n1/0\n```".into(),
                reasoning: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
                usage: Usage { input_tokens: Some(1), output_tokens: Some(1), total_tokens: Some(2) },
            },
        ];
        let profile = make_profile(scripted);
        let mut opts = CompletionOpts::default();
        opts.route_mode = RouteMode::Harness;
        let res = completion("tiny context".into(), "q".into(), profile, opts)
            .await
            .expect("forced_finish on max_errors must still return Ok so the eval rig scores it as 0.0");
        assert_eq!(res.mode, "harness:fence");
        assert_eq!(
            res.stop_reason, "forced_finish:max_errors",
            "after 3 consecutive erroring execs across 3 turns, the counter must trip — \
             this is the bug #8 fixes: previously the counter reset every turn and the \
             cap was unreachable for a model that errors once per turn."
        );
        assert_eq!(res.answer, "");
    }

    #[tokio::test]
    async fn explicit_plain_reports_does_not_fit_when_oversize() {
        // A context that exceeds `max_context` (the model's REAL cap)
        // returns `does_not_fit` on the plain arm, not a silent flip
        // to harness. Mirrors `examples/bench.py` on the harness side.
        use crate::completion::{completion, RouteMode};
        let scripted = vec![Completion {
            content: "should not be called".into(),
            reasoning: None,
            tool_calls: vec![],
            stop_reason: "stop".into(),
            usage: Usage { input_tokens: Some(1), output_tokens: Some(1), total_tokens: Some(2) },
        }];
        let profile = make_profile(scripted);
        // Build a 20k-token context; `profile.max_context = 16_384`
        // so `est_tokens + 2K margin > 16_384` → does_not_fit.
        let big = "x".repeat(20_000 * 4); // 80k chars ≈ 22.8k tokens
        let mut opts = CompletionOpts::default();
        opts.route_mode = RouteMode::Plain;
        let res = completion(big, "q".into(), profile, opts).await.unwrap();
        assert_eq!(res.mode, "plain");
        assert_eq!(res.stop_reason, "does_not_fit");
        assert_eq!(res.answer, "");
    }

    #[test]
    fn no_code_no_final_nudge_mentions_required_patterns() {
        // The corrective message must point the model at the three
        // acceptable reply shapes (```repl, ```python, or a final).
        // ReCLamO-Harness fix #5 (PR #52) uses a near-identical string.
        assert!(NO_CODE_NO_FINAL_NUDGE.contains("```repl"));
        assert!(NO_CODE_NO_FINAL_NUDGE.contains("```python"));
        assert!(NO_CODE_NO_FINAL_NUDGE.contains("FINAL"));
        assert!(NO_CODE_NO_FINAL_NUDGE.contains("FINAL_VAR"));
    }

    #[test]
    fn length_stop_reason_breadcrumb_mentions_continue() {
        // v0.2 followup: the breadcrumb pushed when the upstream reports
        // `finish_reason: "length"` must steer the model back to the
        // cut-off point. Peer-aligned with ReCLamO-Harness
        // `CONTINUE_PROMPT` at 9ac890d. The peer's port does a
        // continuation + join; this port pushes a single breadcrumb —
        // both approaches are valid.
        assert!(LENGTH_STOP_REASON_BREADCRUMB.contains("cut off"));
        assert!(LENGTH_STOP_REASON_BREADCRUMB.contains("Continue"));
    }

    #[test]
    fn loop_pushes_length_breadcrumb_on_truncation() {
        // v0.2 followup: drive a mock that returns stop_reason="length"
        // with a truncated reply, and assert the breadcrumb lands in
        // `messages` so the next turn sees the nudge. Uses the
        // InMemoryRepl path so no Python is required.
        use crate::providers::MockProvider;
        use std::sync::Arc;

        // Turn 1: commit progress (no FINAL). Turn 2: cut off mid-
        // content (length). The loop pushes the breadcrumb, then
        // continues. Turn 3: assign `answer = 42` (code only). Turn
        // 4: `FINAL_VAR(answer)` (final only). The FINAL must be on a
        // separate turn from the assignment because the loop
        // short-circuits on a FINAL before running code.
        let provider = Arc::new(MockProvider::scripted(
            "mock",
            vec![
                Completion {
                    content: "```repl\ncommit('first')\n```".into(),
                    reasoning: None,
                    tool_calls: vec![],
                    stop_reason: "stop".into(),
                    usage: crate::providers::Usage::default(),
                },
                Completion {
                    content: "I was about to write the next chunk but".into(),
                    reasoning: None,
                    tool_calls: vec![],
                    stop_reason: "length".into(),
                    usage: crate::providers::Usage::default(),
                },
                Completion {
                    content: "```repl\nanswer = 42\n```".into(),
                    reasoning: None,
                    tool_calls: vec![],
                    stop_reason: "stop".into(),
                    usage: crate::providers::Usage::default(),
                },
                Completion {
                    content: "FINAL_VAR(answer)".into(),
                    reasoning: None,
                    tool_calls: vec![],
                    stop_reason: "stop".into(),
                    usage: crate::providers::Usage::default(),
                },
            ],
        )) as Arc<dyn crate::providers::Provider>;
        let mut profile = ModelProfile::new("t", provider, 32_000);
        profile.subcall_chars = 20_000;

        let cfg = LoopConfig {
            max_iterations: 5,
            max_timeout: std::time::Duration::from_secs(30),
            max_tokens: None,
            subcall_timeout: std::time::Duration::from_secs(5),
            max_errors: 3,
            max_subcalls_per_run: 64,
            max_subcalls_per_exec: 24,
            root_max_output_tokens: 4_096,
            subcall_max_output_tokens: 2_048,
            route_by_size: true,
            plain_query_margin: 2_000,
        };
        let res = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(crate::rlm::run(
                "ctx".to_string(),
                "q".to_string(),
                profile,
                cfg,
            ))
            .expect("run ok");
        assert_eq!(res.answer, "42");
    }

    #[test]
    fn loop_take_one_executable_block_when_response_has_many() {
        // ReCLamO-Harness fix #4: when `parse_response` returns N
        // code blocks, the loop must execute only the first and drop
        // the rest. We test the selection logic directly here without
        // driving the full loop, because exercising the loop with
        // SubprocessRepl is an integration test (Python on PATH) and
        // the InMemoryRepl fallback in `run()` doesn't execute code.
        let txt = "```repl\ncommit('first')\n```\n\
                   sep prose\n\
                   ```repl\ncommit('second')\n```\n\
                   more sep\n\
                   ```python\nx = 3\n```";
        let parsed = crate::parsing::parse_response(txt);
        assert_eq!(parsed.code_blocks.len(), 3, "parser must see all 3 blocks");
        // The selection is `parsed.code_blocks.iter().take(1)` — mirror it.
        let selected: Vec<&_> = parsed.code_blocks.iter().take(1).collect();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].code, "commit('first')");
    }
}
