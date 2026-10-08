# BACKLOG

Mirrors the GitHub Issues tab once any issues are filed. Pre-issue seed below
comes from `PLAN.md#phases-this-is-also-the-backlog-seed`.

When filing an issue: add a matching `- [ ]` line here and link the issue
number. When an item ships: tick the box or move to `Done`.

## Open

- [ ] **Phase 1 — Scaffold.** `pyproject.toml`, `src/reclamo/{config,client,providers/*,parsing,prompts,router,logger}.py`, `reclamo --version`. *Acceptance:* package builds on Python 3.11 and 3.12; `--version` works; `MockProvider` unit-tested against the reasoning-format adapter.
- [ ] **Phase 2 — Loop + REPL.** `repl/worker.py`, `repl/subprocess_repl.py`, `rlm.py`, `reclamo run`. Includes route-by-size (NEXT-STEPS fix 1). *Acceptance:* on the frozen `needle 1M` task the harness matches v0.1's `3/3 found` at the same prompt; on a 200-line context that fits in `max_context`, plain-fallback path is taken and produces the same answer.
- [ ] **Phase 3 — Five fixes folded in.** Prompts/loop patched for: commit-early + late nudges (2), delegation un-scare (3), multi-hop helper (4), forced-finish unification (5). *Acceptance:* 20-turn-cap rate on the same eval seed-set falls from `15/40 (37.5 %)` to `≤ 8/40 (20 %)`, with no accuracy regression. `llm_query` use rate rises from `5/40 (12.5 %)` to `≥ 12/40 (30 %)` without accuracy regression. `max_errors` triggers the same forced-finish path as turn/time caps.
- [ ] **Phase 4 — Eval rig.** `evals/{freeze_manifest.py,run_eval.py}`. *Acceptance:* replaying v0.1's frozen eval (#22, sha `e17580f`) produces the same per-cell numbers twice in a row (sha-pinned generators + manifest).
- [ ] **Phase 5 — First cross-model run.** Anthropic Sonnet family on small/medium slice. *Acceptance:* the run completes without provider-interface errors; the cross-family cells (`small × 8 × 1`) produce scores within the v0.1 baseline envelope (±0.1 on harness; -0.05 acceptable on plain). If outside envelope, the next phase is "debug the provider abstraction" not "iterate cells."
- [ ] **Phase 6 — Second cross-model run.** OpenAI-compatible provider (Ollama local / OpenRouter free / vLLM). *Acceptance:* same as phase 5.
- [ ] **Phase 7 — Full eval.** All 8 tasks × 3 sizes × 2 seeds × 2 providers, both `plain` and `harness:fence` modes. *Acceptance:* all targets in `PLAN.md#success-criterion-better-than-v01` met.
- [ ] **Phase 8 — Write up.** `RESULTS.md` with the comparison table. *Acceptance:* a stranger can re-run `evals/run_eval.py` and get the same numbers from `RESULTS.md` (sha-pinned).

## Done

*(none yet)*

*Proudly Made in Nebraska. Go Big Red! 🌽 <https://xkcd.com/2347/>*
