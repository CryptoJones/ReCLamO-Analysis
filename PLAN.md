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

### Provider abstraction (`src/reclamo/providers/`)

One interface, two implementations:

```python
class Provider(Protocol):
    name: str
    def complete(self, messages, *, tools=None, **opts) -> Completion: ...
    def capabilities(self) -> "CapabilitySet": ...
```

- `AnthropicProvider` — Messages API. Native thinking, tool use, prompt caching via `cache_control`.
- `OpenAICompatProvider` — universal; works against Strata, Ollama, vLLM, LM Studio, OpenRouter, OpenAI native. A pluggable `thinking_extension` handles the cross-vendor differences (`chat_template_kwargs` for Qwen, `reasoning_effort` for OpenAI, `extra_body` passthrough for everything else).
- Capability detection (`CapabilitySet`): `supports_thinking`, `supports_tool_use`, `supports_json_mode`, `supports_prefix_cache`, `known_reasoning_format`. Loops and prompts branch on capabilities, not on model name.

### Reasoning adapter (`src/reclamo/providers/reasoning.py`)

One function, all formats:

```python
def extract_reasoning(raw: dict, fmt: ReasoningFormat) -> tuple[str, str | None]:
    """Returns (clean_visible_content, reasoning_or_none)."""

ReasoningFormat ∈ {`think_tags`, `reasoning_field`, `native_blocks`, `none`}

Handles:
- `<think>…</think>` strip (Strata/Qwen/etc.)
- `reasoning_content` field fallback when content is empty (Strata, OpenRouter)
- Anthropic native `thinking` blocks
- Plain text (no-op)

The rest of the codebase never sees the difference.

### Capability profile (one dataclass, drives prompt + loop)

```python
@dataclass
class ModelProfile:
    name: str                            # human-readable
    provider: Provider
    max_context: int                     # effective window
    resident_kv: int                     # for compaction threshold (Strata = 32K, Anthropic = depends)
    subcall_chars: int = 12_000          # prompt's "batch ~N chars per call"
    presence_penalty: float | None = None  # only for repetitive-loops models
    thinking: ThinkingMode = "auto"      # on / off / auto (per-call)
```

Each provider ships a default profile. The prompt knows `profile.subcall_chars`; the loop knows `profile.max_context` and `profile.resident_kv`; the reasoning adapter is fed `provider.capabilities().known_reasoning_format`.

### The loop — mostly v0.1, with five folds-in

The v0.1 loop is fine; the bugs are pinned at the prompt/edge-handling layer. So the v2 loop is the v0.1 loop with five one-spot patches:

1. **Route by size.** Before `RLM.completion()` starts, measure `len(context_tokens)`. If it fits `profile.max_context - query_margin`, return a plain single-shot completion from the same provider with the same prompt minus the REPL section. One file: `src/reclamo/router.py`.
2. **Commit early.** REPL bootstrap sets `answer["content"] = ""` and a tiny helper `commit(text: str)` that updates it. The first turn's instructions say "After each useful intermediate result, `commit(...)` it into `answer`. Don't wait." Smell test: the prompt and the answer-tracking are visible from turn 1.
3. **Late nudges.** At ~60 % and ~85 % of `max_iterations`, the system prompt injects one final nudge: "If you're not making progress, `commit` your best answer. If `answer['content']` looks right, `FINAL_VAR(answer)`."
4. **Un-scare delegation.** Replace the v0.1 "expensive / one-at-a-time / hard cap" warnings with the real budget: "You have `max_subcalls_per_run` sub-calls available (default 64). Use them for judgement, not for line-by-line scanning." Add a regex-futility detector that nudges after N turns where the regex produced no matches and didn't progress the answer.
5. **Forced-finish unification.** All four termination triggers (`max_iterations`, `max_timeout`, `max_tokens`, `max_errors`) go through one forced-finish path: ask the model once with a snapshot of `answer` and `vars`, demand a `FINAL_VAR(answer)` (if `answer["content"]` is non-empty) or a fallback `final_answer = "I could not determine the answer"`. Never return code verbatim. Never return a stale variable named `final_answer / answer_text / result / final` — look those up, take the value, drop the name.

### Eval rig — copy the discipline

- Generators + answer keys frozen, sha256-pinned; manifest recorded with each run.
- Three modes per cell: `plain`, `harness:fence`, `harness:tools`.
- Per-row table: score, mean/sd, tokens, seconds, turns, sub-calls, stop_reason, trajectory path, model id, provider.
- Aggregations: per (model, task family, size); per cell; per row.
- Cross-mode comparisons: harness vs plain on cells where plain fits; harness-only on the rest.

The eval rig is **separate** from the harness package — `evals/` directory, not under `src/reclamo/`.

## Phases (this is also the BACKLOG seed)

| # | Phase | What ships | Notes |
|---|---|---|---|
| 1 | Scaffold | `pyproject.toml`, `src/reclamo/{config,providers/{base,anthropic,openai_compat},providers/reasoning,client,parsing,prompts,router,logger}.py`, `reclamo --version` | Make sure the package even builds. No REPL yet. |
| 2 | Loop + REPL | `src/reclamo/{rlm,repl/worker,repl/subprocess_repl}.py`, `reclamo run` | MVP. Routes-by-size (fix 1) included. |
| 3 | Five fixes | Edit prompts/loop to fold in fixes 2–5, no public API change | Smoke eval on three cells before/after. |
| 4 | Eval rig | `evals/{run_eval.py, freeze_manifest.py}`, README on how to pin generators | Replay of v0.1 frozen eval must produce the same numbers. |
| 5 | First model | Run eval against **Anthropic** (sonnet family) on a small/medium slice | Sanity-check the abstraction. |
| 6 | Second model | OpenAI-compatible (one of Ollama local; OpenRouter free; vLLM if reachable) | Catches provider-interface bugs. |
| 7 | Full eval | All 8 tasks × 3 sizes × 2 seeds × 2 providers, both modes | Write up. |
| 8 | Write the postcard | `RESULTS.md` with the table | This is the artifact CJ cares about. |

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

## File layout (planned)

```
src/reclamo/
  __init__.py
  cli.py                 # `reclamo run`, `reclamo ping`, `reclamo version`
  config.py              # dataclass config, TOML profiles (kept v0.1-compatible shape)
  client.py              # v0.1 default-key resolver; not model-specific
  providers/
    base.py              # Provider Protocol, CapabilitySet, ModelProfile
    anthropic.py
    openai_compat.py
    reasoning.py         # the one normalize function
    registry.py          # `--provider anthropic|openai-compat` → class
  parsing.py             # finds ```repl / ```python / ```tool blocks; detects answer
  prompts.py             # ONE prompt builder fed capability flags, not model name
  repl/
    worker.py            # vendored from v0.1, no Qwen assumptions
    subprocess_repl.py
  rlm.py                 # the loop, all 5 fixes folded in
  router.py              # plain-vs-harness routing
  logger.py              # JSONL trajectory schema (kept close to v0.1)
  errors.py              # typed limits with `partial_answer`

evals/
  freeze_manifest.py     # sha256-pin generators + answer keys
  run_eval.py            # cells × modes × seeds matrix; writes RESULTS.md
  cells/                 # frozen generators (re-exported from v0.1 frozen eval)

examples/
  needle.py
  oolong_lite.py
  longdoc_qa.py

tests/
  test_providers.py      # reasoning-format normalizer unit tests
  test_parsing.py
  test_router.py
  test_prompts.py        # capability-driven prompt diff
  test_rlm.py            # with MockProvider end to end

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
