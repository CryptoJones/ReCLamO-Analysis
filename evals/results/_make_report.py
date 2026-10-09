#!/usr/bin/env python3
"""Generate the Phase 7 rerun per-cell report from the round-3.1 JSONLs.

Run: python3 evals/results/_make_report.py
Writes: evals/results/phase7-rerun-seeds23-report.txt
"""

import json
from collections import defaultdict


def load(path):
    rows = []
    with open(path) as f:
        for line in f:
            rows.append(json.loads(line))
    return rows


V02_COMMIT = "139a0c5"


def section_for(name, rows):
    out = []
    out.append("=" * 80)
    out.append(name)
    out.append("=" * 80)
    out.append(f"  total cells: {len(rows)}")
    plain = [r for r in rows if r["mode_requested"] == "plain"]
    harness = [r for r in rows if r["mode_requested"] == "harness"]
    out.append(f"  plain   mean: {sum(r['score'] for r in plain)/len(plain):.3f}  (n={len(plain)})")
    out.append(f"  harness mean: {sum(r['score'] for r in harness)/len(harness):.3f}  (n={len(harness)})")
    out.append("")

    by = defaultdict(list)
    for r in rows:
        by[(r["task"], r["size"], r["mode_requested"])].append(
            (r["seed"], r["score"], r["mode_actual"], r["turns"], r["subcalls"], r["stop_reason"], r["tokens"], r["seconds"])
        )
    out.append(f"  {'task':14s} {'size':6s} {'mode':7s} {'s2':>5s} {'s3':>5s} {'mean':>5s}")
    for (task, size, mode), sscs in sorted(by.items()):
        sscs.sort()
        scs = [s for _, s, *_ in sscs]
        seeds = [sd for sd, *_ in sscs]
        s2 = f"{scs[seeds.index(2)]:.2f}" if 2 in seeds else "  - "
        s3 = f"{scs[seeds.index(3)]:.2f}" if 3 in seeds else "  - "
        m = sum(scs) / len(scs)
        out.append(f"  {task:14s} {size:6s} {mode:7s} {s2:>5s} {s3:>5s} {m:5.2f}")
    out.append("")
    out.append("  -- per-cell detail --")
    for r in sorted(rows, key=lambda x: (x["task"], x["size"], x["seed"], x["mode_requested"])):
        out.append(
            f"  {r['task']:14s} {r['size']:6s} seed={r['seed']} {r['mode_requested']:7s} | "
            f"score={r['score']:.2f} mode_actual={r['mode_actual']:13s} "
            f"turns={r['turns']:2d} sub={r['subcalls']:2d} stop={r['stop_reason']:32s} "
            f"tokens={r['tokens']:6d} secs={r['seconds']:5.1f}"
        )
    return "\n".join(out)


def main():
    llama = load("evals/results/round3.1-llama-3.3-70b-seeds23.jsonl")
    gemini = load("evals/results/round3.1-gemini-2.5-flash-lite-seeds23.jsonl")

    out = []
    out.append("=" * 80)
    out.append("Phase 7 rerun — round-3.1 scorer, fresh seeds 2 + 3, all 8 tasks")
    out.append(f"v0.2 commit: {V02_COMMIT}")
    out.append("Models: meta-llama/llama-3.3-70b-instruct (DeepInfra), google/gemini-2.5-flash-lite")
    out.append("128 cells total (8 tasks × 2 sizes × 2 seeds × 2 modes × 2 models)")
    out.append("Profile: profiles/openrouter-{llama-3.3-70b,gemini-2.5-flash-lite}.toml, resident_kv=80K, thinking=adaptive")
    out.append("Scorer: evals/gen/Multivac.py round-3.1 (sha bc174de9…, manifest round-3.1)")
    out.append("=" * 80)
    out.append("")
    out.append(section_for("LLAMA 3.3 70B (meta-llama/llama-3.3-70b-instruct)", llama))
    out.append("")
    out.append(section_for("GEMINI 2.5 FLASH LITE (google/gemini-2.5-flash-lite)", gemini))
    out.append("")
    out.append("=" * 80)
    out.append("COMBINED (both models, n=128)")
    out.append("=" * 80)
    all_rows = llama + gemini
    plain = [r for r in all_rows if r["mode_requested"] == "plain"]
    harness = [r for r in all_rows if r["mode_requested"] == "harness"]
    out.append(f"  total cells: {len(all_rows)}")
    out.append(f"  plain   mean: {sum(r['score'] for r in plain)/len(plain):.3f}  (n={len(plain)})")
    out.append(f"  harness mean: {sum(r['score'] for r in harness)/len(harness):.3f}  (n={len(harness)})")
    out.append("")
    by = defaultdict(list)
    for r in all_rows:
        by[(r["task"], r["size"], r["mode_requested"])].append(r["score"])
    out.append(f"  {'task':14s} {'size':6s} {'mode':7s} {'s2':>5s} {'s3':>5s} {'mean':>5s} {'n':>3s}")
    for (task, size, mode), scs in sorted(by.items()):
        scs.sort()
        m = sum(scs) / len(scs)
        out.append(f"  {task:14s} {size:6s} {mode:7s} {scs[0]:5.2f} {scs[1]:5.2f} {m:5.2f} {len(scs):3d}")

    text = "\n".join(out) + "\n"
    with open("evals/results/phase7-rerun-seeds23-report.txt", "w") as f:
        f.write(text)
    print(text)


if __name__ == "__main__":
    main()
