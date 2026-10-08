# PLAN — ReCLamO v2 (model-agnostic)

The opus agent in `../ReCLamO-Harness/` shipped v0.1: an Apache-2.0 RLM
harness tuned for Qwen3.8 on pluto Flash-Next. Self-authored results were
perfect. On the independent roundtable-authored eval (#22) the harness **lost**
to plain when context fit, and only the harness answered when it didn't. The
NEXT-STEPS analysis ranks the five fixable failure modes by expected payoff.
CJ asked for a parallel build that works on any model, not just Qwen on pluto.

This repo is that parallel build.

## Evidence I'm acting on

Source: `~/.local/share/reclamo-archive/2026-10-07/` and the merged-but-stable
v0.1 at `../ReCLamO-Harness/` (sha `458e2f7`, latest PR #35 still does not
land the five NEXT-STEPS fixes).

| Cell | v0.1 (replayed from `#22`) |
|---|---|
| small × 8 tasks × 2 seeds | harness **5/16** exact (0.31), plain **9/16** (0.56) |
| medium × 8 × 2 | harness 5/16 (0.31), plain n/a (doesn't fit) |
| large × 8 × 1 | harness 1/8 (0.13), plain n/a |
| 20-turn cap hit | 15/40 (37.5 %); only 2 of those 15 were correct |
| `llm_query` used at all | 5/40 runs (over-cautious sub-call prompt) |
| Forced-finish bugs | 2 (fixed PR #34, but bug class — error cap — not yet closed) |
| Generator defects found | 4 (MasterControl regex, Multivac two-rng, SELMA hidden step, etc.) — fixed PR #35 |
| Tool-calling protocol | helps long-doc-QA (9/9 vs 6/9 fence). Neutral on oolong. (PR #30) |

## Goals (measurable)

A model-agnostic ReCLamO that:

1. Runs against **≥ 3 model families** with no per-model forks. Today: only Qwen on pluto works end to end.
2. Implements the five NEXT-STEPS fixes end to end (no half-measures that need another round).
3. Reverses the small-context regression so harness ≥ plain where plain fits.
4. Doesn't regress on medium/large (only-harness cells).
5. Reproduces the eval discipline (frozen generators, sha256-pinned, plain-vs-harness, JSONL traces, per-row stop reasons, manifest per run).

## Non-goals (YAGNI)

- Replacing v0.1's Qwen-tuned *defaults*. The Qwen profile still exists; the model-agnostic layer sits on top.
- Cloud sandboxes (modal/e2b/daytona).
- Training / fine-tuning.
- A web visualizer.
- Replicating every PR in v0.1 by a clean-room rewrite. Where v0.1 already works (subprocess REPL, JSONL logger, eval rig pattern) we copy the design and re-implement; we don't paper over it.

## Architecture

### Provider abstraction (`src/providers/`)

One trait, three implementations:

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;
    async fn complete(&self, messages: &[Message], tools: Option<&[serde_json::Value]>, opts: CompleteOpts)
        -> Result<Completion, ReclamoError>;
    fn capabilities(&self) -> CapabilitySet;
    fn model_id(&self) -> &str;
}
```

- `AnthropicProvider` — Messages API direct via reqwest. Native thinking
  blocks, tool use, prompt caching via `cache_control`.
- `OpenAICompatProvider` — universal; works against Strata, Ollama, vLLM,
  LM Studio, OpenRouter, OpenAI native. The body builder is hand-rolled
  JSON so vendor-specific fields (`chat_template_kwargs` for Qwen,
  `reasoning_effort` for OpenAI o-series, `extra_body` passthrough for
  everything else) pass through cleanly without fighting a typed SDK.
- `MockProvider` — scripted responses for tests, capability override.
- Capability detection (`CapabilitySet`): `supports_thinking`,
  `supports_tool_use`, `supports_json_mode`, `supports_prefix_cache`,
  `known_reasoning_format: ReasoningFormat`. Loops and prompts branch on
  capabilities, not on model name.

### Reasoning adapter (`src/providers/reasoning.rs`)

One function, all formats:

```rust
pub fn extract_think_tags(text: &str) -> (String, Option<String>);
/* plus the Anthropic provider decodes native `thinking` blocks from content[],
   plus the OpenAI-compat provider decodes `reasoning_content` on the way in */
```

`ReasoningFormat ∈ { ThinkTags, ReasoningField, NativeBlocks, None }`.

Handles:
- `<think>…</think>` strip (Strata / Qwen / OpenAI o-series / DeepSeek)
- `reasoning_content` field fallback when content is empty (Strata,
  OpenRouter, DeepSeek R1)
- Anthropic native `thinking` blocks
- Plain text (no-op)

The rest of the codebase never sees the difference.

### Capability profile (one struct, drives prompt + loop)

```rust
pub struct ModelProfile {
    pub name: String,
    pub provider: Arc<dyn Provider>,
    pub max_context: u64,
    pub resident_kv: u64,                  // for compaction threshold
    pub subcall_chars: u32,                // prompt's "batch ~N chars per call"
    pub presence_penalty: Option<f64>,
    pub thinking: ThinkingMode,
    // ... sampling defaults ...
}
```

Each profile ships a default profile. The prompt knows `profile.subcall_chars`;
the loop knows `profile.max_context` and `profile.resident_kv`; the reasoning
adapter is fed `provider.capabilities().known_reasoning_format`.

### The loop — v0.1 with five folds-in

The v0.1 loop is fine; the bugs are pinned at the prompt/edge-handling layer.
The Rust loop is the v0.1 design with five one-spot patches:

1. **Route by size.** Before `rlm::run` starts, measure
   `len(context_chars)/3.5 + len(query_chars)/3.5`. If it fits
   `profile.resident_kv - plain_query_margin`, return a plain single-shot
   completion from the same provider with the same prompt minus the REPL
   section. One file: `src/router.rs`.
2. **Commit early.** REPL bootstrap sets `answer["content"] = ""` and
   exposes `commit(text)` and `SHOW_VARS()` helpers. The system prompt
   tells the model "after each useful intermediate result, `commit(...)` it
   into `answer`. Don't wait." — visible from turn 1.
3. **Late nudges.** At ~60 % and ~85 % of `max_iterations`, the user
   message injects one final nudge: "If you're not making progress,
   `commit` your best answer. If `answer['content']` looks right,
   `FINAL_VAR(answer)`."
4. **Un-scare delegation.** Replace the v0.1 "expensive / one-at-a-time
   / hard cap" warnings with the real budget: "You have
   `max_subcalls_per_run` sub-calls available (default 64). Use them
   for judgement, not for line-by-line scanning." A regex-futility
   detector in `rlm::count_fruitless_regex` nudges after 3 empty turns.
5. **Forced-finish unification.** All four termination triggers
   (`max_iterations`, `max_timeout`, `max_tokens`, `max_errors`) go
   through one `rlm::forced_finish` path: ask the model once with a
   snapshot of the REPL state, demand a `FINAL_VAR(answer)` (if
   `answer["content"]` is non-empty) or a fallback `FINAL(<one line>)`.
   Never return code verbatim. Never return a stale variable name —
   look it up, take the value, drop the name.

### REPL split

The REPL is a Python subprocess (vendored `worker.py` via `include_str!`)
because Python stdlib gives us regex / json / csv for free and matches
v0.1's worker exactly. The orchestrator is Rust.

- Phase 1 ships `InMemoryRepl` only (scripted closures, no Python process).
  Test surfaces: `MockProvider` round-trips, parser, prompts — without
  requiring `python3` on PATH.
- Phase 2 wires `SubprocessRepl::spawn` into `repl::default_client` so the
  loop sees a real worker.

### Eval rig — copy the discipline

- Generators + answer keys frozen, sha256-pinned; manifest recorded with each run.
- Three modes per cell: `plain`, `harness:fence`, `harness:tools`.
- Per-row table: score, mean/sd, tokens, seconds, turns, sub-calls, stop_reason, trajectory path, model id, provider.
- Aggregations: per (model, task family, size); per cell; per row.
- Cross-mode comparisons: harness vs plain on cells where plain fits; harness-only on the rest.

The eval rig is **separate** from the crate — `evals/` directory, not under `src/`.

## Phases (this is also the BACKLOG seed)

| # | Phase | What ships | Status |
|---|---|---|---|
| 1 | Scaffold | `Cargo.toml`, `src/{config,parsing,prompts,router,logger,cli}.rs`, `src/providers/{base,anthropic,openai_compat,mock,reasoning}.rs`, `src/repl/{mod,in_memory_repl,subprocess_repl,worker.py}`, `reclamo-anl --version` | **SHIPPED 2026-10-08** (55/55 unit tests green) |
| 2 | Loop + REPL | Wire `SubprocessRepl` into `default_client`; let the model emit `llm_query`/`llm_query_batched`; end-to-end against a small needle task | next |
| 3 | Five fixes | Verify the five fixes on a smoke eval; tune the prompt wording if any fail the smoke test | after 2 |
| 4 | Eval rig | `evals/{freeze_manifest,run_eval}.rs` (or `.py` for the visualizer path); JSONL trajectory reader | after 3 |
| 5 | First model | Anthropic Sonnet family on a small/medium slice | after 4 |
| 6 | Second model | OpenAI-compatible (Ollama local / OpenRouter free / vLLM) | after 5 |
| 7 | Full eval | All 8 tasks × 3 sizes × 2 seeds × 2 providers, both modes | after 6 |
| 8 | Write up | `RESULTS.md` with the table | after 7 |

After every phase: OMI note "phase N complete" so the next session doesn't re-derive.

## Success criterion ("better than v0.1")

Same frozen generators, same seeds 0 and 1, two providers (Qwen via OpenAI-compat as a sanity; Anthropic as the cross-family one). Pass if **all** of:

| Target | v0.1 baseline | Target |
|---|---|---|
| Small harness vs plain on cells where plain fits | harness 0.31, plain 0.56 (harness loses) | harness ≥ plain (regression reversed) |
| Medium exact-match rate | 5/16 (0.31) | ≥ 6/16 |
| Large exact-match rate | 1/8 (0.13) | ≥ 1/8 |
| 20-turn cap rate | 15/40 (37.5 %) | ≤ 8/40 (20 %) |
| `llm_query` use rate | 5/40 (12.5 %) | ≥ 12/40 (30 %) without regressing accuracy |
| Cross-model portability | n/a | ≥ 2 distinct families green on small/medium |
| Eval reproducibility | sha-pinned + manifest | sha-pinned + manifest + same numbers reproduced twice |

A phase fails if its smoke eval regresses vs the previous phase.

## Repo state

- **Private**, direct commits to `main` (per OMI operational rules).
- `BACKLOG.md` seeded from this plan; will mirror the GitHub Issues tab once the repo is published.
- Per the project's tag-line rule: README banner carries the Nebraska line.

## File layout (live)

```
src/
  cli.rs                 # `reclamo-anl run`, `reclamo-anl ping`, `reclamo-anl version`
  config.rs              # TOML profiles + LoopConfig defaults
  completion.rs          # top-level entry — routes by size, then plain or loop
  parsing.rs             # finds ```repl / ```python / ``` blocks; detects FINAL/FINAL_VAR
  prompts.rs             # system prompt builder; reads prompts/system_prompt.template
  prompts/system_prompt.template
  router.rs              # plain-vs-harness routing (fix #1)
  logger.rs              # JSONL trajectory schema (kept close to v0.1)
  error.rs               # typed ReclamoError::LimitHit / Provider / Repl / Io / ...
  rlm.rs                 # the loop, all 5 fixes folded in
  providers/
    mod.rs
    base.rs              # Provider trait, CapabilitySet, ModelProfile, types
    anthropic.rs         # Messages API direct via reqwest
    openai_compat.rs     # universal OpenAI-shape, hand-rolled JSON
    mock.rs              # scripted responses for tests
    reasoning.rs         # extract_think_tags  (the one normalize function)
  repl/
    mod.rs               # ReplClient trait + Box blanket impl
    in_memory_repl.rs    # Phase-1 default (no Python process needed)
    subprocess_repl.rs   # Phase-2 client; vendored worker.py
    worker.py            # vendored from v0.1, no Qwen assumptions

evals/
  freeze_manifest.rs     # sha256-pin generators + answer keys
  run_eval.rs            # cells × modes × seeds matrix; writes RESULTS.md
  cells/                 # frozen generators (re-exported from v0.1 frozen eval)

examples/
  needle.rs
  oolong_lite.rs
  longdoc_qa.rs

profiles/
  qwen-pluto.toml        # example profile using Strata via OpenAICompat
  anthropic.toml         # example profile using AnthropicProvider
```

## Process

1. Get the user's plan OK by writing this. (Now in the file.)
2. Each phase = one commit on `main` (private repo) + one OMI note + one entry tick in BACKLOG.md.
3. The two-spot failures v0.1 hit (Python 3.11, validator-only-on-3.12) — run CI on 3.11 and 3.12.
4. No PR for this (private repo). If CJ asks to publish, I'll cut a public mirror and follow the public-repo branch+PR rule from there.

*Proudly Made in Nebraska. Go Big Red! 🌽 <https://xkcd.com/2347/>*
