#!/usr/bin/env python3
"""Freeze manifest for the eval rig.

Walks evals/gen/, sha256-pins each generator, and writes a JSON
manifest. Run before any eval and check the manifest into git so
future runs are pinned.

Usage:
    python3 -I evals/freeze_manifest.py > evals/freeze_manifest.json
"""

import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path


GEN_DIR = Path(__file__).resolve().parent / "gen"


def sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 16), b""):
            h.update(chunk)
    return h.hexdigest()


def main():
    gens = sorted(GEN_DIR.glob("*.py"))
    files = {}
    for g in gens:
        files[g.name] = {
            "sha256": sha256_file(g),
            "bytes": g.stat().st_size,
        }
    manifest = {
        "tool": "ReCLamO-Analysis freeze_manifest",
        "generated_at": datetime.now(tz=timezone.utc).isoformat(),
        "generator_dir": str(GEN_DIR.relative_to(GEN_DIR.parent.parent)),
        "files": files,
    }
    json.dump(manifest, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
