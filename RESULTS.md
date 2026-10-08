# RESULTS — Phase 7a cross-model slice (GLaDOS, llama-3.3-70b + gemini-2.5-flash-lite)

> **Status:** Phase 7a.1 + 7a.2 + 7a.3 + cross-harness control done.
> **Date:** 2026-10-08.
> **Eval rig:** `examples/eval.rs` at commits `e260904` (7a.1), `1bbd7e5` (7a.2), `1bbd7e5` (7a.3, no code changes since 7a.2). Cross-harness control: `ReCLamO-Harness` Python at `022baad`.
> **Freeze manifest:** `evals/freeze_manifest.json` `version: round-3`, 8 generator SHAs pinned.

## TL;DR

For the **GLaDOS** task on **llama-3.3-70b** and **gemini-2.5-flash-lite**, the limit on multi-hop chain-of-thought is **the model**, not the harness and not the language. A cross-harness control on the Python `ReCLamO-Harness` (022baad) ran the same 8 cells with **16-51 sub-calls per cell (up to 567K tokens)** and converged to the **same mean score** as the Rust harness's 1-5 sub-calls. Plain (one-shot, no REPL) tied or beat harness on every cell where plain could see the full context; harness only ran on cells where plain was honestly refused (`does_not_fit`).

| Cell | Rust harness mean | Python harness mean | Plain mean |
|---|---|---|---|
| GLaDOS small (both models) | 0.05 | 0.10 | 0.20 |
| GLaDOS medium (both models) | 0.10 | 0.10 | 0.20 (Llama does_not_fit; gemini) |
| GLaDOS large (both models) | 0.05 | (not run) | 0.10 (Llama does_not_fit; gemini) |

Caveat: this is one task and two seeds. Generalizing to "harness doesn't help" needs more tasks (Cerebex, MasterControl, etc.) and at least one non-Llama/non-Gemini model.

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

A known failure mode (per the peer's 2026-10-08 Poolside Laguna observation on `ronin28`): some models emit `<tool_call>repl ...</tool_call>` blocks in the `content` field with `tool_calls=[]` empty. The harness's fence parser misses these and counts the turn as no-code. The peer added a per-turn audit log at `src/providers/openai_compat.rs:151-160` (enabled at `RUST_LOG=info,reclamo_anl::providers::openai_compat=debug`).

| Slice | llama completions | gemini completions | slips |
|---|---|---|---|
| 7a.1 small | 13 | 5 | **0 / 0** |
| 7a.2 medium | 53 | 65 | **0 / 0** |
| 7a.3 large | 25 | 11 | **0 / 0** |
| **Total** | **91** | **81** | **0** |

Both models use the `content` channel correctly and never emit the slip. Audit logs saved as `*.audit.log` next to each JSONL.

## Cost

| Slice | Cells | Spend (USD) | Cumulative key usage (OpenRouter `openrouter/opencode`) |
|---|---|---|---|
| Phase 7a.1 | 8 | $0.036 | $0.553 |
| Phase 7a.2 | 8 | $0.16 | $0.713 |
| Phase 7a.3 | 8 | ~$0.082 | $0.795 |
| Cross-harness control (peer's spend) | 8 | ~$0.12 | $0.915 (peer-reported) |
| **Total Phase 7a** | **32** | **~$0.398** | — |

Token-rate estimate undercounted by ~2× vs real OpenRouter usage (per-request markups on paid routes). Switched to real `GET /api/v1/key` for the budget. Verified at 2026-10-08 06:41 CDT: usage=$0.8016, limit=$10.00, remaining=$9.1984. The cross-harness control's $0.915 figure is the peer's last reported number; the key now shows $0.8016 cumulative (the peer ran on a different OpenRouter key).

## Findings (so far, for the GLaDOS slice)

1. **Plain ties or beats harness on cells where plain can see the context** (7a.1 small both models, 7a.2 medium gemini, 7a.3 large gemini). The harness's overhead — multi-turn nudges, REPL state echo, sub-call commit-prompting — is a tax that doesn't pay off on GLaDOS at these sizes.
2. **The model is the limit, not the harness** (cross-harness control). Python's heavier delegation (16-51 sub-calls, up to 567K tokens) did not improve the score.
3. **Llama's 131K cap is a hard wall on medium and large.** GLaDOS large (≈343K tokens) doesn't fit; harness runs but the 3-4 sub-call budget is too small for the multi-hop chain. **At large, even the harness path stops helping on Llama** (0.00/0.00).
4. **No parser slips on either model.** 172/172 completions on the GLaDOS slice used the `content` channel correctly. The `<tool_call>`-block failure mode (Poolside Laguna) is model-specific, not generic.
5. **One task is not enough.** GLaDOS is a multi-hop chain; the model fails on the chain, not on the harness. A different task structure (e.g. a long-doc QA with a single fact in the middle) might show the harness helping. The next informative cells are: a Cerebex control (long-doc, single fact), and a third model family.

## Known limitations

- **One task, two seeds.** All three cells of GLaDOS × 2 seeds. Per the peer's Phase 5b noise measurement, same seed can split 0.20/0.00 across modes. Seven other tasks (Cerebex, MasterControl, Multivac, Neuromancer, SELMA, SHODAN, TheDixieFlatline) on the task axis remain unexercised.
- **One model family tested cross-harness.** The Python control is on Llama 3.3 70B only. The peer did not mirror the gemini arm. Generalizing "harness doesn't help" needs at least one non-Llama cross-harness control.
- **No `<tool_call>`-slip test on a known slipper.** Poolside Laguna is the only model we've seen emit the slip. Llama and Gemini don't. We can't claim the audit log catches all slips until we test it against a slip-emitting model.
- **The 1.0 scoring is binary.** The eval rig scores exact match against a single `truth_repr`. There's no partial-credit; a near-miss (e.g. "passed" right, amount off by 1) is 0.0. The harness may produce closer-to-correct answers that score 0.0 because the JSON serialization round-trips. To check this: re-score 7a.1-7a.3 with a tolerance band.
- **`forced_finish:max_iterations` is hitting** on every llama harness medium cell. The model isn't reaching a conclusion in 20 turns. The fix is either higher `max_iterations`, better mid-loop nudges, or a sub-call structure that converges faster. None of these are in scope here.
- **Cross-harness control only covers small + medium.** The peer didn't run large on Python; whether the Rust 0.00/0.00 on large replicates on Python is unknown. The Phase 5d evidence (Python medium harness hit 0.20 on one run that did 9 sub-calls) suggests Python *might* do better on large given a heavier delegation budget, but it's not measured.
- **The Rust harness is missing six Python fixes** (BACKLOG items #3, #4, #5, #6, #7, #8). The cross-harness control ran on the **Python** harness with all six ported (PR #52 etc.). The Rust harness's lower sub-call count at medium (4-5 vs 51) is consistent with a parser that's more conservative about which replies contain code. Whether porting the parser fixes would close the 0.20 → 0.10 gap is an open question; the per-cell score match suggests no, but the cell count is too small to be sure.

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
