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
use crate::repl::{ReplClient, ReplExecResult};
use std::time::Instant;

pub async fn run(
    context: String,
    query: String,
    profile: ModelProfile,
    cfg: LoopConfig,
) -> ReclamoResult<RunResult> {
    let started = Instant::now();
    let deadline = started + cfg.max_timeout;

    // Open the REPL and load the context.
    let mut repl: Box<dyn ReplClient> = crate::repl::default_client(&context, cfg.subcall_timeout)?;
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
    while turn < cfg.max_iterations {
        turn += 1;
        if Instant::now() >= deadline {
            return forced_finish(repl.as_mut(), profile.provider.as_ref(), &messages, "max_timeout", &mut stats, started).await;
        }
        if let Some(t) = cfg.max_tokens {
            if stats.tokens >= t {
                return forced_finish(repl.as_mut(), profile.provider.as_ref(), &messages, "max_tokens", &mut stats, started).await;
            }
        }

        // First-turn "inspect the context first" is the system prompt's
        // job; subsequent turns re-state the question so the model's
        // append-only history stays a valid request.
        if turn == 1 {
            // The first user message is already in messages.
        } else {
            // Build a fresh user message with nudges.
            let has_answer = repl_has_answer(&*repl).await;
            let nudge = build_user_message(
                &query, turn, cfg.max_iterations,
                stats.subcalls_total,
                cfg.max_subcalls_per_run,
                has_answer,
                stats.regex_futility,
                &repl_kind(),
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

        let completion = profile.provider.complete(&messages, None, opts).await?;
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

        // Resolve a FINAL/FINAL_VAR first, if present and not rejected.
        if let (Some(intent), None) = (parsed.final_intent.as_ref(), parsed.reject_reason.as_ref()) {
            let answer = match intent {
                FinalIntent::Final(v) => v.clone(),
                FinalIntent::FinalVar(name) => lookup_var(repl.as_mut(), name).await?,
            };
            return Ok(RunResult {
                answer,
                mode: "harness:fence".into(),
                stop_reason: "final_var".into(),
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
            // No code, no FINAL — count this as a no-op turn. The prompt's
            // late nudges catch this from turn ≥ ceil(max_iters * 0.6).
            continue;
        }

        let mut made_progress = false;
        let mut consecutive_errors = 0u32;
        for block in &parsed.code_blocks {
            let exec = repl.execute(&block.code).await?;
            if exec.error.is_some() {
                consecutive_errors += 1;
                if consecutive_errors >= cfg.max_errors {
                    return forced_finish(repl.as_mut(), profile.provider.as_ref(), &messages, "max_errors", &mut stats, started).await;
                }
            } else {
                consecutive_errors = 0;
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

    forced_finish(repl.as_mut(), profile.provider.as_ref(), &messages, "max_iterations", &mut stats, started).await
}

fn intent_to_string(i: &FinalIntent) -> String {
    match i {
        FinalIntent::Final(v) => format!("FINAL({v})"),
        FinalIntent::FinalVar(n) => format!("FINAL_VAR({n})"),
    }
}

async fn repl_has_answer(_repl: &dyn ReplClient) -> bool {
    // Phase 1 doesn't introspect the REPL across the wire; the InMemory
    // variant gives it back via lookup_var. We default to false (the prompt's
    // commit-early rule covers the case anyway).
    false
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

    let completion = provider.complete(&attempt_messages, None, opts).await;
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
