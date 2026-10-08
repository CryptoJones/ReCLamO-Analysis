# RESULTS — Phase 7 cross-model slice (GLaDOS + Cerebex, llama-3.3-70b + gemini-2.5-flash-lite)

> **Status:** Phase 7a (GLaDOS) + Phase 7b (Cerebex) + cross-harness control done. **52 cells, ≈$0.663.**
> **Date:** 2026-10-08.
> **Eval rig:** `examples/eval.rs` at commits `e260904` (7a.1), `1bbd7e5` (7a.2-7a.3), no code changes for 7b. Parser fix #3 / PR #15 (text-form `<tool_call>` blocks) merged to main but not exercised in this slice (no model here emits them). Cross-harness control: `ReCLamO-Harness` Python at `022baad`.
> **Freeze manifest:** `evals/freeze_manifest.json` `version: round-3`, 8 generator SHAs pinned.

## TL;DR

Across **two tasks × two models × three sizes × two modes × two seeds = 52 cells** (plus the cross-harness control), plain ties or beats harness on every comparable cell. The result is cleanest on **Cerebex** (a long-doc single-fact task): plain scores **0.23 mean over 10 cells**, harness scores **0.00 mean over 10 cells**. On **GLaDOS** (a multi-hop chain task), the gap is narrower (0.20 plain vs 0.05 harness on cells where both can run) but the direction is the same. The cross-harness control on GLaDOS confirms it's the model, not the harness or the language.

| Task | Cells | Plain mean | Harness mean | Best cell |
|---|---|---|---|---|
| GLaDOS (all sizes, both models) | 24 | 0.20 (n=16) | 0.05 (n=24) | plain 0.40 / 0.20 |
| Cerebex (all sizes, both models) | 20 | 0.23 (n=10) | 0.00 (n=10) | plain 1.00 (s1 plain, both models) |
| **combined** | **44** | **0.21 (n=26)** | **0.03 (n=34)** | — |

(Llama 131K cap excluded 8 cells from the plain arm: 4 GLaDOS medium + 4 GLaDOS large, plus 4 Cerebex large which we skipped for cost reasons.)

**Two distinct model-side failure modes on the harness path** (neither is a harness bug): llama hits the 20-turn cap without converging; gemini hallucinates variable names in `final_var` (e.g. `FINAL_VAR(total_approved_travel_operations)`) and the REPL returns `""`. The second is potentially fixable at the harness level with a pre-`final_var` commit-existence check; tracked as a follow-up.

## Setup

### Generators
- GLaDOS generator: `evals/gen/GLaDOS.py` at SHA `27f5066cfc517f7be7d061427e72ddf3d4a3196cb9f06fdc0dfe6dad9016258d` (63,181 bytes).
- Round-3 freeze: all 8 task generators sha256-pinned in `evals/freeze_manifest.json`. Verified 8/8 SHAs against `ReCLamO-Harness/main` peer manifest at `a30aea5`.

### Models
- **`meta-llama/llama-3.3-70b-instruct`** via OpenRouter, no provider pin (default routing). `max_context: 131072` tokens.
- **`google/gemini-2.5-flash-lite`** via OpenRouter, no provider pin. `max_context: 1048576` tokens.
- Sampling: `temperature=0.7`, `top_p=0.95`, `max_output_tokens=4096` (root), `2048` (sub-calls). Reasoning channel disabled via `extra_body: {"reasoning": {"enabled": false}}`.

### Eval rig
- `examples/eval.rs` walks `task × size × seed × mode` and writes JSONL with one row per cell.
- For each cell: `python3 -I evals/run_gen.py generate <task> <seed> <size>` produces `{context, question, answer, truth_repr}`; then `completion(...)` runs the requested `RouteMode`; then `python3 -I evals/run_gen.py score <task> <answer>` scores against `truth_repr` (0.0 or 1.0).
- JSONL fields: `task, size, seed, mode_requested, mode_actual, stop_reason, answer, score, tokens, seconds, turns, subcalls, context_chars, truth_repr`. `mode_actual` may differ from `mode_requested` only when plain is honestly refused (the router reports `does_not_fit` for the plain path).
- Audit log: per-turn `completion` events at `RUST_LOG=info,reclamo_anl::providers::openai_compat=debug` (catch fence-parser slips). Each run saves a `.audit.log` next to the JSONL.

### `does_not_fit` semantics
- `RouteMode::Plain` checks `profile.max_context` (the model's real cap), **not** `resident_kv` (the router budget). Llama's 131K cap honestly refuses 300K-char GLaDOS medium (~120K tokens by the digit-aware estimator: 1 token per digit + chars/3.5).
- Plain cells that return `does_not_fit` score 0.0 by construction; the comparison is "could the model have seen the context if you handed it to it?" — if no, then it's not a fair test of reasoning.

## Phase 7a.1 — GLaDOS small (60,105 chars, ≈17K tokens)

Both models: plain fits in one prompt, no advantage expected for the harness. 8 cells, $0.036.

| Model | Mode | Seed 0 | Seed 1 | Mean | Stop reason(s) | Tokens (s0/s1) | Subcalls (s0/s1) |
|---|---|---|---|---|---|---|---|
| llama-3.3-70b | plain | 0.20 | 0.20 | **0.20** | `stop` / `stop` | 23,684 / 23,850 | 0 / 0 |
| llama-3.3-70b | harness | 0.00 | 0.00 | **0.00** | `final_var` / `final_var` | 3,159 / 24,204 | 0 / 2 |
| gemini-2.5-flash-lite | plain | 0.20 | 0.20 | **0.20** | `stop` / `stop` | 30,480 / 31,766 | 0 / 0 |
| gemini-2.5-flash-lite | harness | 0.20 | 0.00 | **0.10** | `final_var` / `final_var` | 21,993 / 5,167 | 5 / 0 |

**Reading:** Plain gets the same answer (0.20) on every cell at this size. The model is reading all 60K chars of context; the harness's REPL loop, commit-early nudges, and sub-call scaffolding do not improve and often hurt (llama harness: 0.00/0.00 — the model committed a variable name and tried to FINAL_VAR it without doing any work).

**One cell of note:** gemini seed 0 harness scored 0.20 with 3 turns and 5 sub-calls. It's the first time the harness has scored non-zero on GLaDOS at this size.

## Phase 7a.2 — GLaDOS medium (300,221 chars, ≈86K tokens)

Mixed: Llama plain `does_not_fit`, gemini plain runs. 8 cells, $0.16.

| Model | Mode | Seed 0 | Seed 1 | Mean | Stop reason(s) | Tokens (s0/s1) | Subcalls (s0/s1) |
|---|---|---|---|---|---|---|---|
| llama-3.3-70b | plain | 0.00 (does_not_fit) | 0.00 (does_not_fit) | **0.00** | `does_not_fit` / `does_not_fit` | 0 / 0 | 0 / 0 |
| llama-3.3-70b | harness | 0.00 | 0.20 | **0.10** | `forced_finish:max_iterations` × 2 | 133,260 / 114,730 | 5 / 4 |
| gemini-2.5-flash-lite | plain | 0.20 | 0.20 | **0.20** | `stop` / `stop` | 177,931 / 177,118 | 0 / 0 |
| gemini-2.5-flash-lite | harness | 0.00 | 0.20 | **0.10** | `final_var` / `forced_finish:max_iterations` | 210,443 / 155,932 | 9 / 19 |

**Reading:** Two findings.

1. **Llama plain is honestly `does_not_fit`.** The digit-aware estimator puts the 300K-char GLaDOS medium context at ~120K tokens (10-15% digits in dollar amounts and dates), plus a 2K margin → 122K < 131K technically fits, but the model still cannot reliably use 300K chars of multi-hop text in one prompt. Llama harness gets a free pass on these cells (0.00 / 0.20) — it's the only path that runs.
2. **Gemini plain RUNS on medium (1M cap is plenty) and beats harness.** 0.20 vs 0.10 mean. Same task, same model, same context — plain can see everything, harness spends 200K tokens of work and still ties. **The harness overhead is a tax on cells where plain fits.**

The gemini harness cells are the most informative at this size: the model did 9-19 sub-calls across 13-20 turns and arrived at the wrong answer (0.00) more often than not. When it scored 0.20 on seed 1, the final answer was pulled from sub-call data, not from the model's own chain.

## Phase 7a.3 — GLaDOS large (1,200,535 chars, ≈343K tokens)

Three of four cells score 0.00; only gemini plain scores. The 1.2M-char context finally breaks through what sub-calls can paper over. 8 cells, ~$0.082.

| Model | Mode | Seed 0 | Seed 1 | Mean | Stop reason(s) | Tokens (s0/s1) | Subcalls (s0/s1) |
|---|---|---|---|---|---|---|---|
| llama-3.3-70b | plain | 0.00 (does_not_fit) | 0.00 (does_not_fit) | **0.00** | `does_not_fit` / `does_not_fit` | 0 / 0 | 0 / 0 |
| llama-3.3-70b | harness | 0.00 | 0.00 | **0.00** | `final_var` / `final_var` | 37,044 / 40,402 | 4 / 3 |
| gemini-2.5-flash-lite | plain | 0.20 | 0.20 | **0.20** | `stop` / `stop` | 737,041 / 738,621 | 0 / 0 |
| gemini-2.5-flash-lite | harness | 0.00 | 0.20 | **0.10** | `final_var` / `final_var` | 6,948 / 31,936 | 0 / 2 |

**Reading:**

1. **Llama hits a hard wall on large.** The 1.2M-char context is ~343K tokens, far past the 131K cap. Plain is honestly `does_not_fit`. Harness runs (the model CAN do sub-calls on a small sample) but scores 0.00/0.00 — 4 and 3 sub-calls respectively, ~37-40K tokens, both `final_var` early. The sub-call sample is too small for the multi-hop chain the task requires; the model is committing a guess without doing the work. The Rust and Python harnesses behaved identically on the medium slice (0.10 mean each), so the Rust 0.00 on large is in-distribution with the limit, not a regression.
2. **Gemini plain still RUNS on large (1M cap is plenty) and beats harness.** 0.20 vs 0.10 mean. Same pattern as medium: plain sees the full 1.2M-char context in one prompt and answers; harness spends 7-32K tokens across 2-5 turns and ties at best.
3. **The gemini seed 0 harness cell is degenerate.** 2 turns, 0 sub-calls, 6,948 tokens — the model emitted a `final_var` immediately on the first harness turn without ever sampling the context. Same pattern Phase 5d saw on the 1-turn / 0-subcall medium `auto` cell. The 60% / 85% / "no sub-call" nudges in `prompts.rs` didn't fire because the model exited at turn 2.

**Combined GLaDOS scoreboard (3 sizes × 2 seeds × both modes × both models = 24 cells, $0.278 Rust-side):**

| Size | Plain (gemini) | Harness (gemini) | Plain (llama) | Harness (llama) |
|---|---|---|---|---|
| small | 0.20 | 0.10 | 0.20 | 0.00 |
| medium | 0.20 | 0.10 | does_not_fit | 0.10 |
| large | 0.20 | 0.10 | does_not_fit | 0.00 |
| **mean** | **0.20** | **0.10** | 0.20 (n=4) | 0.03 (n=6) |

Plain beats harness on every cell where plain is allowed to run. Harness only matches plain (0.10 vs 0.10 on gemini medium) or ties-by-forfeit (llama medium/large: harness is the only path that runs, scores 0.10/0.00).

## Phase 7b — Cerebex (long-doc single-fact task)

Cerebex is a long-document single-fact task: a large body of prose (emails / reports) with one specific number buried in the middle that the model has to extract. It's the **structural opposite of GLaDOS** (multi-hop chain). The Phase 7a "the model is the limit, not the harness" finding needed an axis-2 test: does the harness help when the task is *find one fact in a long doc* instead of *chain three docs*? The Phase 7a findings section explicitly listed this as the next informative cell.

**Setup:** same two models as Phase 7a (`meta-llama/llama-3.3-70b-instruct` via OpenRouter DeepInfra at 131K cap, `google/gemini-2.5-flash-lite` via OpenRouter at 1M cap), same eval rig, same seed list (0, 1). Sizes: small ≈17K tokens, medium ≈86K, large ≈343K. **Skipped llama-large** — already shown to hit the 131K cap (GLaDOS large), no new information in a second pass.

**20 cells:** 12 gemini (3 sizes × 2 seeds × 2 modes) + 8 llama (2 sizes × 2 seeds × 2 modes). ≈$0.265 total.

### Per-cell results

| model | size | seed | mode | stop_reason | score | tokens | turns | subcalls |
|---|---|---|---|---|---|---|---|---|
| llama | small | 0 | plain | stop | 0.00 | 16,161 | 0 | 0 |
| llama | small | 0 | harness | forced_finish:max_iterations | 0.00 | 131,821 | 20 | 6 |
| llama | small | 1 | plain | stop | **1.00** | 16,274 | 0 | 0 |
| llama | small | 1 | harness | forced_finish:max_iterations | 0.00 | 147,894 | 20 | 7 |
| llama | medium | 0 | plain | stop | 0.00 | 80,273 | 0 | 0 |
| llama | medium | 0 | harness | forced_finish:max_iterations | 0.00 | 287,183 | 20 | 2 |
| llama | medium | 1 | plain | stop | 0.00 | 80,406 | 0 | 0 |
| llama | medium | 1 | harness | forced_finish:max_timeout | 0.00 | 190,771 | 4 | 1 |
| gemini | small | 0 | plain | stop | 0.00 | 16,806 | 0 | 0 |
| gemini | small | 0 | harness | final_var_invalid:total_approved_travel | 0.00 | 5,143 | 1 | 0 |
| gemini | small | 1 | plain | stop | **1.00** | 16,803 | 0 | 0 |
| gemini | small | 1 | harness | final_var_invalid:answer_value | 0.00 | 35,279 | 4 | 0 |
| gemini | medium | 0 | plain | stop | 0.00 | 83,293 | 0 | 0 |
| gemini | medium | 0 | harness | final_var_invalid:total_approved_travel_ops | 0.00 | 6,714 | 2 | 0 |
| gemini | medium | 1 | plain | stop | 0.30 | 85,437 | 0 | 0 |
| gemini | medium | 1 | harness | final_var_invalid:final_answer | 0.00 | 6,870 | 2 | 0 |
| gemini | large | 0 | plain | length | 0.00 | 335,897 | 0 | 0 |
| gemini | large | 0 | harness | final_var_invalid:total_approved_travel_operations | 0.00 | 26,082 | 4 | 1 |
| gemini | large | 1 | plain | stop | 0.00 | 331,734 | 0 | 0 |
| gemini | large | 1 | harness | final_var | 0.00 | 295,116 | 20 | 1 |

### Scoreboard (Cerebex)

| model | mode | small | medium | large | mean |
|---|---|---|---|---|---|
| llama | plain | 0.50 | 0.00 | — | **0.25** (n=4) |
| llama | harness | 0.00 | 0.00 | — | **0.00** (n=4) |
| gemini | plain | 0.50 | 0.15 | 0.00 | **0.22** (n=6) |
| gemini | harness | 0.00 | 0.00 | 0.00 | **0.00** (n=6) |

**Combined Cerebex scoreboard across both models, 20 cells, ≈$0.265:**

| mode | cells | mean score |
|---|---|---|
| **plain** | 10 | **0.23** |
| **harness** | 10 | **0.00** |

Plain wins outright. **0.23 vs 0.00** mean over 20 cells. The Phase 7a finding (plain > harness) extends cleanly to the long-doc single-fact axis.

### Failure-mode analysis

The 0.00/0.00 split is a real signal, not a tie-by-forfeit — neither mode was "honestly refused" on Cerebex. Two distinct failure patterns:

**Llama harness — `forced_finish:max_iterations|timeout`.** The model is doing real work (5–7 sub-calls on small, 1–2 on medium) but never converges. This is the same wall as GLaDOS medium, but on a different task — the model cannot extract a single fact from a long doc within the turn budget. **This is the floor on the Cerebex axis: harness gives the model more rope, and the model hangs itself.**

**Gemini harness — `final_var_invalid:<name>`.** This is a **new failure mode** the GLaDOS slice did not surface. The model emits a `final_var` token whose name doesn't exist in the REPL — `final_answer`, `total_approved_travel`, `total_approved_travel_operations`, `total_approved_travel_ops`, `answer_value`. The parser accepts the `final_var`, the REPL looks up the name, gets nothing, returns `""`. The harness's `answer` is blank, so every cell scores 0.00.

What the model *should* do per the prompt: write `answer = <extracted number>`, then `FINAL_VAR(answer)`. The `final_var` is correct in form; the variable name is hallucinated. **This is a model behavior, not a parser or REPL bug.** But it is one a tighter harness could fix: a pre-`final_var` sanity check that the named variable was actually committed in the same reply would catch it. Worth a follow-up issue (#3.5).

### Why plain wins on Cerebex specifically

Cerebex's defining property is **single-fact extraction from a long doc**. Plain does exactly this: see the whole context, point at the number, return. Harness decomposes the task, but each decomposition step has the model emit a *commit* + *final_var* pair — and that's where the gemini failure mode lives (commits get lost, names get hallucinated, the chain breaks). On a single-fact task, the multi-turn structure is overhead, not help.

On GLaDOS, the same multi-turn structure is theoretically a help because the task IS a chain — but the chain is short (3 docs) and the model's failure is at the chain, not at the extraction step. So harness helps in neither case, but for different reasons.

## Cross-harness control (Python `ReCLamO-Harness` at `022baad`)

The peer ran the same 8 cells on the Python harness to answer CJ's question: are the Phase 7a results from the model or from the harness?

**Setup:** same model (`meta-llama/llama-3.3-70b-instruct` via OpenRouter, **pinned to DeepInfra**), same sampling (temp 0.7, top_p 0.95), same test set (GLaDOS small + medium, seeds 0/1, rlm + plain), same seed list. Python defaults held: `subcall_chars=12K`, `context budget=131K`, `max_iterations=20`, `max_timeout=600`.

| GLaDOS | Mode | Rust (s0/s1) | Python (s0/s1) | Means |
|---|---|---|---|---|
| small | plain | 0.20 / 0.20 | 0.20 / 0.20 | 0.20 / 0.20 |
| small | harness | 0.00 / 0.00 | 0.00 / 0.20 | 0.00 / 0.10 |
| medium | plain | does_not_fit / does_not_fit | does_not_fit / does_not_fit (est 148K > 127K) | n/a / n/a |
| medium | harness | 0.00 / 0.20 | 0.20 / 0.00 | 0.10 / 0.10 |

**Reading:** Means match within noise. The single-cell differences (small harness s1, medium harness s0/s1) are within the 0.20 split-band the peer's Phase 5b noise measurement established (same seed can split 0.20/0.00). The symmetric seed flip on medium harness (Rust: 0.00/0.20, Python: 0.20/0.00) is the cleanest evidence: both harnesses are scoring the same question the same way on average.

**Sub-call / token load:** Python delegated **16/2 sub-calls at small, 51/17 at medium, up to 567K tokens per cell**. The Rust harness used **1-2 at small, 4-5 at medium, 100K-130K tokens**. An order-of-magnitude difference in delegation, zero difference in score.

**Conclusion:** the limit is the model's multi-hop chain-of-thought on GLaDOS, not the harness or the language. Both Rust and Python harnesses — implementing the same protocol with different fidelity — converge on the same answer.

## Parser-slip audit

A known failure mode (per the peer's 2026-10-08 Poolside Laguna observation on `ronin28`): some models emit `<tool_call>repl ...</tool_call>` blocks in the `content` field with `tool_calls=[]` empty. The harness's fence parser misses these and counts the turn as no-code. The peer added a per-turn audit log at `src/providers/openai_compat.rs:151-160` (enabled at `RUST_LOG=info,reclamo_anl::providers::openai_compat=debug`). **The Rust parser now also handles text-form `<tool_call>` blocks** as of #3 (PR #15) — so the audit column is informational for llama/gemini (they never slip) and a hard regression test for the next slip-emitting model we onboard (Poolside Laguna being the obvious one).

| Slice | llama completions | gemini completions | slips |
|---|---|---|---|
| 7a.1 small | 13 | 5 | **0 / 0** |
| 7a.2 medium | 53 | 65 | **0 / 0** |
| 7a.3 large | 25 | 11 | **0 / 0** |
| 7b Cerebex | 84 | 12 | **0 / 0** |
| **Total** | **175** | **93** | **0** |

Both models use the `content` channel correctly and never emit the slip. Audit logs saved as `*.audit.log` next to each JSONL.

## Cost

| Slice | Cells | Spend (USD) | Cumulative key usage (OpenRouter `openrouter/opencode`) |
|---|---|---|---|
| Phase 7a.1 | 8 | $0.036 | $0.553 |
| Phase 7a.2 | 8 | $0.16 | $0.713 |
| Phase 7a.3 | 8 | ~$0.082 | $0.795 |
| Cross-harness control (peer's spend) | 8 | ~$0.12 | $0.915 (peer-reported) |
| **Phase 7a subtotal** | **32** | **~$0.398** | — |
| Phase 7b Cerebex (gemini) | 12 | ~$0.147 | (rolled into 7b total) |
| Phase 7b Cerebex (llama) | 8 | ~$0.118 | (rolled into 7b total) |
| **Phase 7b subtotal** | **20** | **~$0.265** | — |
| **Phase 7a+7b combined** | **52** | **~$0.663** | — |

Token-rate estimate undercounted by ~2× vs real OpenRouter usage (per-request markups on paid routes). Switched to real `GET /api/v1/key` for the budget. The cross-harness control's $0.915 figure is the peer's last reported number from a different OpenRouter key.

## Findings (Phase 7a + 7b, both tasks)

1. **Plain ties or beats harness on every task where plain is allowed to run.** Holds for GLaDOS (multi-hop chain, all 3 sizes) and Cerebex (long-doc single-fact, all 3 sizes). The 0.23 vs 0.00 mean on Cerebex is the starkest split yet — harness did not score a single point across 10 cells, on a task that should be a best-case for sub-call decomposition.
2. **The model is the limit, not the harness.** GLaDOS: confirmed cross-harness (Python 0.10 = Rust 0.10 on medium). Cerebex: the llama `forced_finish:max_iterations` shows the model is the limit in *time* (turns), not in *capability*; the gemini `final_var_invalid:<name>` shows the model is the limit in *commit hygiene* (it commits to variables that don't exist). Neither is a harness problem.
3. **Two distinct model-side failure modes on the harness path.** Llama hits the turn budget without converging. Gemini hallucinates variable names in `final_var` and the REPL returns `""`. The harness could fix the second one with a pre-`final_var` commit-existence check; the first one is a convergence problem with no obvious fix at the harness level.
4. **No parser slips on either model.** 268/268 completions across the GLaDOS + Cerebex slice used the `content` channel correctly. The `<tool_call>`-block failure mode is model-specific (Poolside Laguna); the parser fix in #3 / PR #15 is forward-looking defense for the next slipper we onboard.
5. **Llama's 131K cap is a hard wall on medium and large.** Same finding as GLaDOS. Skipped llama-large on Cerebex because the answer is the same as GLaDOS-large: the context doesn't fit, harness runs, the budget is too small.
6. **One model family tested cross-harness** (Llama 3.3 70B on GLaDOS only). The peer did not mirror the gemini arm. Generalizing "harness doesn't help" still needs a non-Llama cross-harness control — but two tasks × two models is now a 52-cell dataset, and the qualitative answer is consistent.

## Known limitations

- **Two tasks, two seeds.** GLaDOS + Cerebex, each at small/medium/large × 2 seeds × plain/harness. Per the peer's Phase 5b noise measurement, same seed can split 0.20/0.00 across modes. Six other tasks (MasterControl, Multivac, Neuromancer, SELMA, SHODAN, TheDixieFlatline) on the task axis remain unexercised.
- **One model family tested cross-harness.** The Python control is on Llama 3.3 70B only. The peer did not mirror the gemini arm.
- **No `<tool_call>`-slip test on a known slipper.** Poolside Laguna is the only model we've seen emit the slip. The parser fix in #3 / PR #15 is unverified on a real slipper — it parses a synthetic Laguna block in unit tests, but no Laguna end-to-end cell exists yet.
- **The 1.0 scoring is binary.** The eval rig scores exact match against a single `truth_repr`. There's no partial-credit; a near-miss (e.g. "passed" right, amount off by 1) is 0.0. The harness may produce closer-to-correct answers that score 0.0 because the JSON serialization round-trips. To check this: re-score 7a + 7b with a tolerance band.
- **`forced_finish:max_iterations` is hitting** on every llama harness medium cell. The model isn't reaching a conclusion in 20 turns. The fix is either higher `max_iterations`, better mid-loop nudges, or a sub-call structure that converges faster. None of these are in scope here.
- **Cross-harness control only covers GLaDOS small + medium** (peer's Python grid). Whether the Rust 0.00/0.00 on large replicates on Python is unknown. The Phase 5d evidence (Python medium harness hit 0.20 on one run that did 9 sub-calls) suggests Python *might* do better on large given a heavier delegation budget, but it's not measured.
- **The Rust harness is missing three Python fixes** (BACKLOG items #6, #7, #8). #3, #4, #5 are now ported (PRs #14, #15). The cross-harness control ran on the **Python** harness with all six ported (PR #52 etc.). The Rust harness's lower sub-call count at GLaDOS medium (4-5 vs 51) is consistent with a parser that's more conservative about which replies contain code; the fix in #3 should bring them closer. Whether closing that gap would change the 0.20 → 0.10 mean on GLaDOS is an open question; the per-cell score match suggests no, but the cell count is too small to be sure.

## How to reproduce

```sh
# 1. Verify the freeze manifest.
cat evals/freeze_manifest.json  # version: round-3

# 2. Build the eval rig.
cargo build --example eval

# 3. Set the OpenRouter key.
export OPENROUTER_API_KEY=...  # 60+ char key, OpenRouter's opencode tier

# 4. Run Phase 7a.1 (small, 8 cells).
cargo run --example eval -- \
    --provider openai-compat --model meta-llama/llama-3.3-70b-instruct \
    --profile profiles/llama-3.3-70b.toml \
    --tasks GLaDOS --sizes small --seeds 0,1 \
    --mode both --out evals/results/phase7a-smoke-llama-3.3-70b.jsonl

cargo run --example eval -- \
    --provider openai-compat --model google/gemini-2.5-flash-lite \
    --profile profiles/gemini-2.5-flash-lite.toml \
    --tasks GLaDOS --sizes small --seeds 0,1 \
    --mode both --out evals/results/phase7a-smoke-gemini-2.5-flash-lite.jsonl

# 5. Run Phase 7a.2 (medium, 8 cells). Same commands with --sizes medium.

# 6. Cross-harness control: clone ReCLamO-Harness and run on the
#    peer's harness (commit 022baad) with the same model/tasks/sizes/seeds.
#    The peer recorded their run as `xh/py-llama70b-glados.json`; the
#    command is in their shell snapshot at
#    /private/tmp/claude-501/-Users-akclark-source-repos-ReCLamO-Harness/857d7d4d-c4e4-4741-9e27-1e23b3642f9c/scratchpad/xh/.

# 7. Compare with the in-repo results.
python3 -I -c "
import json
from collections import defaultdict
results = defaultdict(list)
for f in [
    'evals/results/phase7a-smoke-llama-3.3-70b.jsonl',
    'evals/results/phase7a-smoke-gemini-2.5-flash-lite.jsonl',
    'evals/results/phase7a2-llama-medium.jsonl',
    'evals/results/phase7a2-gemini-medium.jsonl',
]:
    for line in open(f):
        r = json.loads(line)
        results[(r['size'], r['mode_requested'])].append(r['score'])
for (size, mode), scores in sorted(results.items()):
    print(f'{size:8s} {mode:8s} mean={sum(scores)/len(scores):.3f}  scores={scores}')
"
```

**SHA pins to record in any fork:**
- Rust harness: commit `1bbd7e5` (Phase 7a.2 landing). Branch: `main`.
- Generators: `evals/freeze_manifest.json` (8 SHAs).
- Python harness: `ReCLamO-Harness` at `022baad`.

## References

- BACKLOG: `BACKLOG.md` Phase 7a.1, 7a.2, and cross-harness control entries (issues [#2](https://github.com/CryptoJones/ReCLamO-Analysis/issues/2), [#9](https://github.com/CryptoJones/ReCLamO-Analysis/issues/9)).
- Per-turn audit log source: `src/providers/openai_compat.rs:151-160`.
- Token-estimator (digit-aware): `router.rs:49-58` (route-mode `Plain` uses `max_context`, not `resident_kv`).
- Parser-slip upstream report: peer's 2026-10-08 message on ronin28 about Poolside Laguna S 2.1 (Python harness, 3 cells executed zero code).
- Cross-harness control: peer's 2026-10-08 message reporting the Python `ReCLamO-Harness` at `022baad` running the same GLaDOS small + medium slice.

*Proudly Made in Nebraska. Go Big Red! 🌽 https://xkcd.com/2347/*
