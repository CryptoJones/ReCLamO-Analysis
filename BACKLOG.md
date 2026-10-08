# BACKLOG

Mirrors the GitHub Issues tab once any issues are filed. Pre-issue seed below
comes from `PLAN.md#phases-this-is-also-the-backlog-seed`.

When filing an issue: add a matching `- [ ]` line here and link the issue
number. When an item ships: tick the box or move to `Done`.

## Open

- [ ] **Phase 3 — Five fixes folded in.** Verify prompts/loop patches for: commit-early + late nudges (2), delegation un-scare (3), multi-hop helper (4), forced-finish unification (5). *Acceptance:* 20-turn-cap rate on the smoke seed-set falls from `15/40 (37.5 %)` to `≤ 8/40 (20 %)`, with no accuracy regression. `llm_query` use rate rises from `5/40 (12.5 %)` to `≥ 12/40 (30 %)` without accuracy regression. `max_errors` triggers the same forced-finish path as turn/time caps.
- [ ] **Phase 4 — Eval rig.** `evals/{freeze_manifest,run_eval}.rs` (or `.py`, whichever CJ prefers for the visualizer path). *Acceptance:* replaying v0.1's frozen eval (#22, sha `e17580f`) produces the same per-cell numbers twice in a row (sha-pinned generators + manifest).
- [ ] **Phase 5 — First cross-model run.** Anthropic Sonnet family on small/medium slice. *Acceptance:* the run completes without provider-interface errors; the cross-family cells (`small × 8 × 1`) produce scores within the v0.1 baseline envelope (±0.1 on harness; -0.05 acceptable on plain). If outside envelope, the next phase is "debug the provider abstraction" not "iterate cells."
- [ ] **Phase 6 — Second cross-model run.** OpenAI-compatible provider (Ollama local / OpenRouter free / vLLM). *Acceptance:* same as phase 5.
- [ ] **Phase 7 — Full eval.** All 8 tasks × 3 sizes × 2 seeds × 2 providers, both `plain` and `harness:fence` modes. *Acceptance:* all targets in `PLAN.md#success-criterion-better-than-v01` met.
- [ ] **Phase 8 — Write up.** `RESULTS.md` with the comparison table. *Acceptance:* a stranger can re-run `evals/run_eval` and get the same numbers from `RESULTS.md` (sha-pinned).

## Done

- [x] **Phase 1 — Scaffold.** `Cargo.toml`, `src/{config,parsing,prompts,router,logger,rlm,cli}.rs`, `src/providers/{base,anthropic,openai_compat,mock,reasoning}.rs`, `src/repl/{mod,in_memory_repl,subprocess_repl,worker.py}`, `reclamo-anl --version`. *Acceptance (now met):* `cargo build` green; `cargo test` 55/55 passing including MockProvider round-trips for Anthropic + OpenAI-compat request builders, parsing fence detection, reasoning-format normalizer (`<think>` strip + `reasoning_content` fallback), router tokens-of-context estimator, capability-driven system prompt interpolation, and JSONL trajectory writer.
- [x] **Phase 2 — Loop + REPL.** `SubprocessRepl::spawn` takes a `SubcallFn`; demuxes `subcall_request` vs result; worker.py's `llm_query` / `llm_query_batched` round-trip sub-calls inline during exec. `rlm::run` builds the `SubcallFn` from `profile.provider` and tries `SubprocessRepl` first (falls back to `InMemoryRepl` if python3 is missing on PATH). `repl_has_answer` does a real `lookup_var`. `strip_to_string` drills into `{"content": "..."}`. CLI `run` gets `--provider`. `MockProvider::looped` for stateful mocks. `examples/needle.rs` proves commit→FINAL_VAR(answer) end-to-end. Profiles: `anthropic.toml`, `qwen-pluto.toml`, `mock.toml`. *Acceptance (now met):* `cargo test` 57 + 3 subprocess; `cargo run --example needle` returns `Nairobi` via the harness path (mode=`harness:fence`, stop=`final_var`, turns=2). Plain path takes a small context via `reclamo-anl run --profile profiles/mock.toml --provider mock --context examples/needle.txt -q "..."`.

*Proudly Made in Nebraska. Go Big Red! 🌽 <https://xkcd.com/2347/>*
