# SPDX-License-Identifier: GPL-3.0-or-later
"""Summarise the spike's benchmark CSVs as Markdown tables.

usage: python3 spikes/summarize.py FILE.csv... > summary.md

Each CSV is one run of one prototype (see bench.sh). Runs of the same host and framework label
are pooled. Percentiles are nearest-rank over frame CPU times, as in docs/perf-budgets.md, and
the verdict applies the spike's threshold: p99 at most 8.33 ms over every measured phase.
"""

import csv
import sys
from collections import defaultdict

BUDGET_NS = 8_333_333
PHASES = ["type", "scroll", "multicursor", "ime", "diff", "all"]


def percentile(sorted_values, pct):
    """Nearest-rank percentile of an already sorted list."""
    if not sorted_values:
        return None
    rank = max(1, -(-pct * len(sorted_values) // 100))
    return sorted_values[rank - 1]


def ms(ns):
    return f"{ns / 1e6:.2f}"


def load(paths):
    """Returns {(host, label): {phase: [frame_ns...]}} and the run count per key."""
    pooled = defaultdict(lambda: defaultdict(list))
    runs = defaultdict(int)
    for path in paths:
        with open(path, newline="") as f:
            rows = list(csv.DictReader(f))
        if not rows:
            continue
        key = (rows[0]["host"], rows[0]["framework"])
        runs[key] += 1
        for row in rows:
            if row["phase"] == "warmup":
                continue
            value = int(row["frame_ns"])
            pooled[key][row["phase"]].append(value)
            pooled[key]["all"].append(value)
    return pooled, runs


def render(pooled, runs):
    out = []
    out.append(
        "| Host | Prototype | Runs | Phase | Frames | p50 ms | p95 ms | p99 ms | max ms "
        "| Over 8.33 ms |"
    )
    out.append("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |")
    verdicts = []
    for key in sorted(pooled):
        host, label = key
        for phase in PHASES:
            values = sorted(pooled[key].get(phase, []))
            if not values:
                continue
            over = sum(1 for v in values if v > BUDGET_NS)
            p99 = percentile(values, 99)
            name = f"**{phase}**" if phase == "all" else phase
            out.append(
                f"| {host} | {label} | {runs[key]} | {name} | {len(values)} | "
                f"{ms(percentile(values, 50))} | {ms(percentile(values, 95))} | {ms(p99)} | "
                f"{ms(values[-1])} | {over} |"
            )
            if phase == "all":
                verdict = "pass" if p99 <= BUDGET_NS else "fail"
                verdicts.append(f"| {host} | {label} | {ms(p99)} | {verdict} |")
    out.append("")
    out.append("| Host | Prototype | p99 ms, all measured phases | Threshold 8.33 ms |")
    out.append("| --- | --- | --- | --- |")
    out.extend(verdicts)
    return "\n".join(out) + "\n"


def main(argv):
    if len(argv) < 2:
        sys.stderr.write(__doc__)
        return 2
    pooled, runs = load(argv[1:])
    sys.stdout.write(render(pooled, runs))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
