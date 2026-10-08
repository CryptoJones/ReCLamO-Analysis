<p align="center"><em>Proudly Made in Nebraska. Go Big Red! 🌽 <a href="https://xkcd.com/2347/">https://xkcd.com/2347/</a></em></p>

# ReCLamO-Analysis

A **model-agnostic RLM harness**, written in Rust. Companion to
[`CryptoJones/ReCLamO-Harness`](../ReCLamO-Harness) (the Qwen-tuned v0.1).

## Why this repo exists

v0.1 works against Qwen3.8 on pluto Flash-Next and solves its self-authored
tasks perfectly. On the independent roundtable-eval (#22, frozen at
`../ReCLamO-Harness` sha `e17580f`), it loses to plain on small contexts and
solves only 5/16 medium and 1/8 large. The fix list lives in `../ReCLamO-Harness`
`NEXT-STEPS.md`. This repo builds the model-agnostic version that **also**
takes the fixes end to end.

The orchestrator is Rust; the REPL is the same vendored Python worker that
v0.1 used — `std`-only stdlib, no third-party deps in the child process.

See [`PLAN.md`](PLAN.md) for the goals, architecture, and success criteria.

## Status

- **Phase 1 (scaffold) — SHIPPED.** `reclamo-anl --version` works; `cargo test`
  passes (55/55 unit tests green). `MockProvider` round-trips against
  `AnthropicProvider` and `OpenAICompatProvider` request builders, including
  `chat_template_kwargs` passthrough for Qwen/Strata and `reasoning_content`
  normalization for Anthropic-style thinking.
- Phase 2 (loop + REPL + first model run) is the next milestone.

## Layout

- `PLAN.md` — what I'm building and why.
- `BACKLOG.md` — backlog (mirrors the GitHub Issues tab once any are filed).
- `src/` — the Rust crate.
  - `providers/{base,anthropic,openai_compat,mock,reasoning}.rs` — the provider
    abstraction (one trait, three adapters).
  - `rlm.rs` — the loop with the five NEXT-STEPS fixes baked in.
  - `router.rs` — plain-vs-harness routing (fix #1).
  - `repl/{in_memory_repl,subprocess_repl,worker.py}.rs/.py` — REPL stub
    (in-memory) and full subprocess variant (vendored `worker.py`).
  - `prompts.rs` + `prompts/system_prompt.template` — capability-driven
    system prompt.
  - `logger.rs` — JSONL trajectory.
  - `parsing.rs` — fenced code + `FINAL`/`FINAL_VAR` detection.
  - `error.rs` — typed limit errors with `partial_answer`.
  - `config.rs` — TOML profile parser.
  - `cli.rs` — `reclamo-anl {run, ping, version}`.
- `evals/` — frozen-generators rig (phase 4).
- `examples/` — needle, oolong-lite, long-doc-QA (phase 4+).
- `profiles/` — example profile TOMLs.

## Quickstart

Requires the Rust 1.75+ toolchain. `cargo test` runs the unit suite.

```sh
# Phase-1 sanity (works today).
cargo run -- --version
cargo test

# Phase-2 use (loop + REPL on a real model — wired up next).
cargo run -- run --profile profiles/anthropic.toml \
    --context path/to/context.txt \
    -q "Find the needle."
```

`--provider mock` skips network for the ping subcommand.

## Roadmap

Tracked in [`BACKLOG.md`](BACKLOG.md); summary in [`PLAN.md`](PLAN.md#phases-this-is-also-the-backlog-seed).

## License

Apache-2.0. See `LICENSE`.

## Credits

Design inspired by `alexzhang13/rlm` and `alexzhang13/rlm-minimal`
(both MIT); see `NOTICE` and `THIRD_PARTY_LICENSES.md`.

*Proudly Made in Nebraska. Go Big Red! 🌽 <https://xkcd.com/2347/>*
