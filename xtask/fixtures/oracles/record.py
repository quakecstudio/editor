# SPDX-License-Identifier: GPL-3.0-or-later
"""Records what the VM oracle wrote, for the replay tests of the vectors and scenario tests.

Run `cargo xtask oracles` first: it keeps each vector's files in
target/oracles/work/vectors/<file>/<vector>/ and each scenario test's outcome in
target/oracles/work/scenarios/<case>/outcome.json. Then, from the repository root:

    python3 xtask/fixtures/oracles/record.py

It writes vector-traces.json and scenario-outcomes.json beside itself.
"""

from __future__ import annotations

import hashlib
import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[3]
WORK = ROOT / "target" / "oracles" / "work"
HERE = pathlib.Path(__file__).resolve().parent
# Traces longer than this (the runaway loops, the 600-edict allocations and the overflowing
# message buffers) are left out, to keep the files small; their runs are not replayed.
MAX_TRACE = 4096
MAX_SCENARIO_TRACE = 8192


def write(name: str, recorded: dict, what: str) -> None:
    """Writes `recorded` as sorted, indented JSON to `name` beside this script."""
    out = HERE / name
    out.write_text(json.dumps(recorded, indent=1, sort_keys=True) + "\n")
    print(f"recorded {len(recorded)} {what} in {out.relative_to(ROOT)}")


def vectors() -> None:
    """Writes one entry per vector: its trace in hex and the oracle's standard output."""
    recorded = {}
    for vector in sorted((WORK / "vectors").glob("*/*")):
        key = f"{vector.parent.name}/{vector.name}"
        trace = (vector / "trace.bin").read_bytes()
        if len(trace) > MAX_TRACE:
            continue
        recorded[key] = {
            "trace": trace.hex(),
            "stdout": (vector / "stdout.txt").read_text(),
        }
    write("vector-traces.json", recorded, "vectors")


def hex_or_none(data: list | None) -> str | None:
    """The bytes of a trace, listed as numbers in outcome.json, in hex."""
    return None if data is None else bytes(data).hex()


def inputs_digest(case: pathlib.Path) -> str:
    """SHA-256 of the case's progs.dat, then its scenario or init file if it has one.

    The replay test computes the same from the case, so a case changed since the recording
    fails until it is recorded again.
    """
    digest = hashlib.sha256((case / "progs.dat").read_bytes())
    for name in ("case.scn", "init.txt"):
        if (case / name).is_file():
            digest.update((case / name).read_bytes())
    return digest.hexdigest()


def scenarios() -> None:
    """Writes one entry per scenario test: its exit status, output and traces in hex.

    The repository's path in a message becomes `<root>`, which the replay puts back.
    """
    recorded = {}
    for path in sorted((WORK / "scenarios").glob("*/outcome.json")):
        outcome = json.loads(path.read_text())
        size = len(outcome["trace"] or []) + len(outcome["again"] or [])
        if size > MAX_SCENARIO_TRACE:
            continue
        recorded[path.parent.name] = {
            "inputs": inputs_digest(path.parent),
            "status": outcome["status"],
            "stdout": outcome["stdout"].replace(str(ROOT), "<root>"),
            "stderr": outcome["stderr"].replace(str(ROOT), "<root>"),
            "trace": hex_or_none(outcome["trace"]),
            "again": hex_or_none(outcome["again"]),
        }
    write("scenario-outcomes.json", recorded, "scenario tests")


if __name__ == "__main__":
    vectors()
    scenarios()
