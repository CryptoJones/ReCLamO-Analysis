//! Top-level entry point. `completion()` decides plain-vs-harness via the
//! router, then either calls the model once or runs the loop.
//!
//! This is the API the CLI and `evals/run_eval.py` consume.
//!
//! ```ignore
//! use reclamo_anl::{completion, ModelProfile, CompletionOpts};
//! ```

use crate::config::LoopConfig;
use crate::error::ReclamoResult;
use crate::providers::ModelProfile;
use crate::router;
use serde::{Deserialize, Serialize};

/// How `completion()` should pick the plain-vs-harness path.
///
/// **Default is `Auto`**, which lets the router decide via
/// `route_by_size + resident_kv` — that is the existing behavior.
///
/// **Explicit `Plain` / `Harness` BYPASS the router** so the eval rig
/// can compare the two arms honestly. Without this, `route_by_size`
/// silently picks one arm regardless of what was asked, and the
/// "plain-vs-harness" column is a coin-flip. The peer flagged this on
/// 2026-10-08: "your plain baseline isn't plain."
///
/// `Plain` is also allowed to report `does_not_fit` (mirroring
/// `examples/bench.py` on the harness side): if the context exceeds
/// the model's real input cap, we skip the call rather than fall
/// through to the harness. Otherwise the "plain" arm is contaminated
/// by harness runs on oversize cells.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteMode {
    /// Router decides via `route_by_size + resident_kv` (default).
    #[default]
    Auto,
    /// Force the plain path. Record `does_not_fit` if the context
    /// would overflow `max_context`.
    Plain,
    /// Force the harness (loop) path. Ignores `route_by_size`.
    Harness,
}

/// Optional knobs the loop / router can see.
#[derive(Debug, Clone, Default)]
pub struct CompletionOpts {
    /// Override `LoopConfig::max_iterations`.
    pub max_iterations: Option<u32>,
    /// Override `LoopConfig::max_subcalls_per_run`.
    pub max_subcalls_per_run: Option<u32>,
    /// Override `LoopConfig::max_subcalls_per_exec`.
    pub max_subcalls_per_exec: Option<u32>,
    /// `Some("docker")` to wrap the REPL in a `--network none` container.
    pub sandbox: Option<String>,
    /// JSONL trajectory output directory.
    pub log_dir: Option<std::path::PathBuf>,
    /// Plain-vs-harness routing override. Default `Auto` lets the
    /// router pick; set explicitly to get an honest arm comparison.
    pub route_mode: RouteMode,
}

/// What the harness returns to the caller.
#[derive(Debug, Clone, Serialize)]
pub struct RunResult {
    /// The visible answer (cleaned; no `<think>` tags, no code blocks).
    pub answer: String,
    /// `plain` if the router decided the context fit; `harness:fence` or
    /// `harness:tools` if the loop ran.
    pub mode: String,
    /// How the run ended: `final_var`, `plain`, `forced_finish`,
    /// `max_iterations`, `max_timeout`, `max_tokens`, `max_errors`.
    pub stop_reason: String,
    /// Total tokens used.
    pub tokens: u64,
    /// Wall-clock seconds.
    pub seconds: f64,
    /// Loop turns (0 for plain).
    pub turns: u32,
    /// Sub-calls issued (`llm_query`, batched or not).
    pub subcalls: u32,
}

/// Top-level call.
pub async fn completion(
    context: String,
    query: String,
    profile: ModelProfile,
    opts: CompletionOpts,
) -> ReclamoResult<RunResult> {
    let route_mode = opts.route_mode;
    let loop_cfg = LoopConfig::from_opts(opts);
    router::route(context, query, profile, loop_cfg, route_mode).await
}
