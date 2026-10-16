# SPDX-License-Identifier: GPL-3.0-or-later
"""Records what the VM oracle wrote for each VM vector into vector-traces.json.

Run `cargo xtask oracles` first: it keeps each vector's files in
target/oracles/work/vectors/<file>/<vector>/. Then, from the repository root:

    python3 xtask/fixtures/oracles/record.py
"""

import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[3]
WORK = ROOT / "target" / "oracles" / "work" / "vectors"
OUT = pathlib.Path(__file__).resolve().parent / "vector-traces.json"
# Traces longer than this (the runaway loops and the 600-edict allocations) are left out, to
# keep the file small; their vectors are not replayed.
MAX_TRACE = 4096


def main() -> None:
    """Writes one entry per vector: its trace in hex and the oracle's standard output."""
    recorded = {}
    for vector in sorted(WORK.glob("*/*")):
        key = f"{vector.parent.name}/{vector.name}"
        trace = (vector / "trace.bin").read_bytes()
        if len(trace) > MAX_TRACE:
            continue
        recorded[key] = {
            "trace": trace.hex(),
            "stdout": (vector / "stdout.txt").read_text(),
        }
    OUT.write_text(json.dumps(recorded, indent=1, sort_keys=True) + "\n")
    print(f"recorded {len(recorded)} vectors in {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
