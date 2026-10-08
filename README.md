<p align="center"><em>Proudly Made in Nebraska. Go Big Red! 🌽 <a href="https://xkcd.com/2347/">https://xkcd.com/2347/</a></em></p>

# ReCLamO-Analysis

A model-agnostic RLM harness. Companion to
[`CryptoJones/ReCLamO-Harness`](../ReCLamO-Harness) (Qwen-tuned v0.1).

## Why this repo exists

v0.1 works against Qwen3.8 on pluto Flash-Next and solves its self-authored
tasks perfectly. On the independent roundtable-eval (#22, frozen at
`../ReCLamO-Harness` sha `e17580f`), it loses to plain on small contexts and
solves only 5/16 medium and 1/8 large. The fix list lives in `../ReCLamO-Harness`
`NEXT-STEPS.md`. This repo builds the model-agnostic version that **also**
takes the fixes end to end.

See [`PLAN.md`](PLAN.md) for the goals, architecture, and success criteria.

## Status

Pre-implementation. Phase 1 (scaffold) in progress.

## Layout

- `PLAN.md` — what I'm building and why.
- `BACKLOG.md` — backlog (mirrors the GitHub Issues tab once any are filed).
- `src/reclamo/` — the package.
- `evals/` — frozen-generators rig and runner.
- `examples/` — needle, oolong-lite, long-doc-QA.
- `tests/` — unit tests, capability-driven, against a MockProvider.

## Quickstart (not yet usable — phase 2+)

```sh
uv sync
uv run reclamo --version
uv run reclamo run --profile anthropic --context file.txt -q "Find X."
uv run evals/run_eval.py --provider anthropic --task needle --seeds 0,1
```

## Roadmap

Tracked in [`BACKLOG.md`](BACKLOG.md); summary in [`PLAN.md`](PLAN.md#phases-this-is-also-the-backlog-seed).

## License

Apache-2.0. See `LICENSE` (will be added in phase 1).

## Credits

Design inspired by `alexzhang13/rlm` and `alexzhang13/rlm-minimal`
(both MIT); `NOTICE` and `THIRD_PARTY_LICENSES.md` to be added in phase 1.

*Proudly Made in Nebraska. Go Big Red! 🌽 <https://xkcd.com/2347/>*
