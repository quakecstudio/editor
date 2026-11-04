#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Generate plan/backlog/readiness.yaml: one test-suite readiness issue per layer per phase.

Rerun after adding or relabelling feature/test issues. See docs/testing.md.
"""

from pathlib import Path

import yaml

BL = Path(__file__).resolve().parent / "backlog"
files = {p.name: yaml.safe_load(p.read_text()) for p in sorted(BL.glob("p*.yaml"))}
done = []


def groups(doc):
    return doc["groups"] if "groups" in doc else [doc]


# ---------- generate readiness issues ----------
LAYERS = ["L0-formats", "L1-vm", "L2-syntax", "L3-semantics", "L4-bridges", "L5-commands", "L6-agent", "L7-ui", "apps", "c-tools"]
SHORT = {"L0-formats": "R0", "L1-vm": "R1", "L2-syntax": "R2", "L3-semantics": "R3", "L4-bridges": "R4",
         "L5-commands": "R5", "L6-agent": "R6", "L7-ui": "R7", "apps": "RA", "c-tools": "RG"}
NAMES = {"L0-formats": "L0 formats", "L1-vm": "L1 VM", "L2-syntax": "L2 syntax", "L3-semantics": "L3 semantics",
         "L4-bridges": "L4 bridges", "L5-commands": "L5 commands", "L6-agent": "L6 agent", "L7-ui": "L7 UI",
         "apps": "apps", "c-tools": "C tools"}
BUILD_TYPES = {"type:feature", "type:test"}
# Readiness reviews per layer are for P1 and P2 only: from P3 on each phase has its gate review
# alone (Justin, 2026-11-04: focus on the MVP and move through the phases faster).
PHASES = ["P1", "P2"]

# Re-reviews: a crate's ratchets re-measured after a later change to its layer, recorded under a
# key of their own, since `cargo xtask ratchet` holds a recorded crate's minimums and equivalent
# mutants to the review that set them (xtask/src/ratchet.rs, check_change). Each names the
# phase, the layer, the crates, the work that changed them and the review it follows; `work` lists
# the issues the re-review waits on, its own defects among them.
REREVIEWS = [
    {"key": "P2-R1B", "phase": "P2", "layer": "L1-vm", "crates": "qc-builtins", "after": "P2-150",
     "follows": "P2-R1", "work": ["P2-156", "P2-159", "P2-160", "P2-168"]},
]

all_issues = [(g["milestone"], i) for d in files.values() for g in groups(d) for i in g["issues"]]
out_groups = []
seen_layers = {}
for ph in PHASES:
    rs = []
    for layer in LAYERS:
        members = [i["key"] for m, i in all_issues if m == ph and layer in i["labels"]
                   and BUILD_TYPES & set(i["labels"])]
        if not members:
            continue
        key = f"{ph}-{SHORT[layer]}"
        deps = members + ([seen_layers[layer]] if layer in seen_layers else [])
        rs.append({
            "key": key,
            "title": f"{NAMES[layer]} test-suite readiness ({ph})",
            "labels": [layer, "area:process", "type:readiness"],
            "size": "S",
            "reqs": ["TEST-1", "TEST-2", "TEST-3", "TEST-4", "TEST-5", "TEST-6", "TEST-7", "TEST-8", "TEST-9", "TEST-10", "TEST-11", "TEST-12"],
            "depends": deps + ["P0-24"],
            "deliverable": [f"docs/readiness/{ph.lower()}-{SHORT[layer].lower()}.md following docs/readiness/_template.md, covering: {', '.join(members)}",
                            "Ratchet values for every crate in the layer committed to CI"],
            "acceptance": [
                "Every threshold in ADR-0009 met for every crate in the layer, or a per-crate exception with its reason recorded in the crate README",
                "Independent adversarial review completed (TEST-9): reviewer did not write the code, added at least five tests, and every defect found is fixed with a regression test and re-reviewed",
                "Flake job: 20 consecutive passing runs on macOS, Windows and Linux",
                "Defects reported against this layer from higher layers are all closed",
            ],
            "verify": ["CI run links for coverage, mutation, fuzz, differential and flake jobs; reviewer sign-off on the issue"],
        })
        seen_layers[layer] = key
    for rr in REREVIEWS:
        if rr["phase"] != ph:
            continue
        note = rr["key"].lower()
        rs.append({
            "key": rr["key"],
            "title": f"{NAMES[rr['layer']]} {rr['crates']} ratchet re-review after {rr['after']} ({ph})",
            "labels": [rr["layer"], "area:process", "type:readiness"],
            "size": "S",
            "reqs": ["TEST-2", "TEST-6", "TEST-9"],
            "depends": [rr["follows"], rr["after"], *rr["work"]],
            "deliverable": [f"docs/readiness/{note}.md following docs/readiness/_template.md for {rr['crates']}, linking the measurement run",
                            f"ratchets.toml: [crates.{rr['crates']}] re-measured on the {rr['after']} code, its readiness key {rr['key']} and its equivalent_mutants holding only mutants cargo-mutants still produces"],
            "acceptance": [
                f"Every threshold in ADR-0009 met for {rr['crates']}, or a per-crate exception with its reason recorded in the crate README",
                "Reviewed by an agent that did not write the change under review (TEST-9)",
                "cargo xtask ratchet passes over the full ratchets.toml history with the new key",
            ],
            "verify": ["cargo xtask ratchet; the Readiness workflow run link in the note"],
        })
    if rs:
        out_groups.append({"milestone": ph, "issues": rs})
(BL / "readiness.yaml").write_text(
    "# Generated by plan/gen_readiness.py. One readiness review per layer per phase.\n"
    + yaml.safe_dump({"groups": out_groups}, sort_keys=False, width=200, allow_unicode=True))
print(sum(len(g["issues"]) for g in out_groups), "readiness issues")
