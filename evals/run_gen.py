#!/usr/bin/env python3
"""Eval rig helper for ReCLamO-Analysis.

Subcommands:
    generate <task> <seed> <size>     → JSON: {context, question, answer, meta, truth_repr}
    score    <task> <answer_text>     → reads truth JSON on stdin; prints a single float
    ping     <task>                   → prints 1.0 if self-score is 1.0, else exits 1
"""

import importlib
import json
import sys
from pathlib import Path


GEN_DIR = Path(__file__).resolve().parent / "gen"


def _load(task: str):
    sys.path.insert(0, str(GEN_DIR))
    return importlib.import_module(task)


def cmd_generate(args):
    if len(args) != 3:
        print("usage: generate <task> <seed> <size>", file=sys.stderr)
        return 2
    task, seed_s, size = args
    seed = int(seed_s)
    mod = _load(task)
    data = mod.generate(seed, size)
    out = {
        "context": data["context"],
        "question": data["question"],
        "answer": str(data["answer"]),
        "truth_repr": repr(data["answer"]),
        "meta": data.get("meta", {}),
    }
    sys.stdout.write(json.dumps(out))
    sys.stdout.write("\n")
    sys.stdout.flush()
    return 0


def cmd_score(args):
    """Usage: score <task> <answer>

    Truth comes from stdin as the JSON `truth_repr` (Python `repr()` of the
    original generator's `answer` field).
    """
    if len(args) != 2:
        print("usage: score <task> <answer>", file=sys.stderr)
        return 2
    task, answer = args
    mod = _load(task)
    truth_repr = sys.stdin.read().strip()
    # Reconstruct the truth. The `generate` JSON output used `repr()` so
    # `eval` is safe (no untrusted input) within this process boundary.
    try:
        truth = eval(truth_repr, {"__builtins__": {}}, {})  # noqa: S307
    except Exception:
        print(f"could not eval truth_repr {truth_repr!r}", file=sys.stderr)
        return 2
    s = mod.score(answer, truth)
    print(s)
    return 0


def cmd_ping(args):
    """Run `generate(0, "small")` and call `score(truth, truth)`. Most
    generators return 1.0; some (e.g. MasterControl) hit float vs int
    formatting noise and return 0.5. We only require the generator to
    round-trip cleanly."""
    if len(args) != 1:
        print("usage: ping <task>", file=sys.stderr)
        return 2
    task = args[0]
    mod = _load(task)
    truth = mod.generate(0, "small")["answer"]
    s = mod.score(str(truth), truth)
    if s > 0.0:
        return 0
    print(f"self-score {s} for {task}", file=sys.stderr)
    return 1


def main():
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    cmd = sys.argv[1]
    rest = sys.argv[2:]
    if cmd == "generate":
        return cmd_generate(rest)
    if cmd == "score":
        return cmd_score(rest)
    if cmd == "ping":
        return cmd_ping(rest)
    print(f"unknown command {cmd!r}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
