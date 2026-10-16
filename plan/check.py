#!/usr/bin/env python3
"""Validate the plan and generate plan/coverage.md.

Fails (exit 1) if:
  - a requirement in requirements.yaml has no issue covering it
  - an issue cites an unknown requirement, label, milestone or dependency
  - an issue lacks a title, deliverable, acceptance criteria or verification
  - an issue has no type:* label, or more than one
  - an issue depends on an issue in a later phase, or dependencies form a cycle
  - a gate issue does not depend on every other issue in its phase
  - a type:decision issue does not name its docs/adr/NNNN-slug.md file in its deliverable
    (NNNN is four digits, or 00NN when the number is taken when the ADR is written; ADR-0001)
  - a type:decision issue is not labelled needs:human
  - a label description is longer than GitHub's 100-character limit
  - a LATER issue is not linked from docs/scope.md as [KEY](https://github.com/quakecstudio/editor/issues/N)
Phase order is enforced by adding the previous phase's gate to every issue's dependencies
(LATER issues wait for the MVP gate, P4-25). Layer order is enforced by adding the lower
layer's readiness issue to every issue that builds on it (docs/testing.md).

Usage: python3 plan/check.py [--write-coverage]
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

import yaml

PLAN = Path(__file__).resolve().parent
SCOPE = PLAN.parent / "docs" / "scope.md"
ISSUE_URL = r"https://github\.com/quakecstudio/editor/issues/\d+"
ADR_FILE = re.compile(r"docs/adr/(?!0000-)(\d{4}|00NN)-[a-z0-9-]+\.md")
PHASE_ORDER = ["P0", "P1", "P2", "P3", "P4", "P5", "P6", "P7", "LATER"]


def load_requirements() -> dict[str, dict]:
    data = yaml.safe_load((PLAN / "requirements.yaml").read_text())
    reqs: dict[str, dict] = {}
    for section in data["sections"]:
        for r in section["requirements"]:
            if r["id"] in reqs:
                raise SystemExit(f"duplicate requirement id {r['id']}")
            reqs[r["id"]] = {**r, "section": section["title"]}
    return reqs


def load_backlog() -> tuple[dict, list[dict]]:
    meta = yaml.safe_load((PLAN / "backlog" / "_meta.yaml").read_text())
    issues: list[dict] = []
    for path in sorted((PLAN / "backlog").glob("*.yaml")):
        if path.name == "_meta.yaml":
            continue
        data = yaml.safe_load(path.read_text())
        groups = data.get("groups") or [data]
        for g in groups:
            for issue in g["issues"]:
                issue["milestone"] = g["milestone"]
                issue["_file"] = path.name
                issues.append(issue)
    add_gate_dependencies(issues)
    add_readiness_dependencies(issues)
    return meta, issues


LAYER_RANK = {"c-tools": -1, "L0-formats": 0, "L1-vm": 1, "L2-syntax": 2, "L3-semantics": 3,
              "L4-bridges": 4, "L5-commands": 5, "L6-agent": 6, "L7-ui": 7, "apps": 8}
BUILD_TYPES = {"type:feature", "type:test"}


def layer_of(issue: dict) -> str | None:
    found = [l for l in issue.get("labels", []) if l in LAYER_RANK]
    return found[0] if found else None


def add_readiness_dependencies(issues: list[dict]) -> None:
    """Building on a lower layer waits for that layer's test-suite readiness (docs/testing.md).

    For every feature or test issue X that depends on a feature or test issue D in a lower
    layer (or a C tool), X also depends on the readiness issue for D's layer in D's phase.
    Phase gates depend on every readiness issue in their phase.
    """
    by_key = {i["key"]: i for i in issues}
    readiness = {(i["milestone"], layer_of(i)): i["key"] for i in issues if "type:readiness" in i.get("labels", [])}
    for x in issues:
        lx = layer_of(x)
        if lx is None or not BUILD_TYPES & set(x.get("labels", [])):
            continue
        for d in list(x.get("depends", [])):
            di = by_key.get(d)
            if di is None or not BUILD_TYPES & set(di.get("labels", [])):
                continue
            ld = layer_of(di)
            if ld is None or ld == lx or LAYER_RANK[ld] > LAYER_RANK[lx]:
                continue
            r = readiness.get((di["milestone"], ld))
            if r is None:
                x.setdefault("_errors", []).append(
                    f"builds on {d} ({ld}, {di['milestone']}) but no readiness issue exists for that layer and phase; run plan/gen_readiness.py")
            elif r not in x["depends"]:
                x["depends"].append(r)
    for i in issues:
        if "type:gate" in i.get("labels", []):
            for (m, _), r in readiness.items():
                if m == i["milestone"] and r not in i.setdefault("depends", []):
                    i["depends"].append(r)


def add_gate_dependencies(issues: list[dict]) -> None:
    """Phases run in order: every non-gate issue in phase N depends on the gate of phase N-1."""
    gates = {i["milestone"]: i["key"] for i in issues if "type:gate" in i.get("labels", [])}
    for i in issues:
        m = i["milestone"]
        if m == "LATER":
            if "P4-25" not in i.setdefault("depends", []):
                i["depends"].append("P4-25")
            continue
        if "type:gate" in i.get("labels", []) or m not in PHASE_ORDER:
            continue
        n = PHASE_ORDER.index(m)
        if n == 0:
            continue
        prev = gates.get(PHASE_ORDER[n - 1])
        if prev and prev not in i.setdefault("depends", []):
            i["depends"].append(prev)


def check(reqs: dict, meta: dict, issues: list[dict]) -> list[str]:
    errors: list[str] = []
    milestones = {m["key"] for m in meta["milestones"]}
    labels = {l["name"] for group in meta["labels"].values() for l in group}
    sizes = set(meta["sizes"])
    by_key: dict[str, dict] = {}

    for group in meta["labels"].values():
        for l in group:
            if len(l.get("description", "")) > 100:
                errors.append(f"label {l['name']}: description over GitHub's 100-character limit")

    for i in issues:
        k = i.get("key", "?")
        errors.extend(f"{k}: {e}" for e in i.get("_errors", []))
        if k in by_key:
            errors.append(f"{k}: duplicate key")
        by_key[k] = i
        for field in ("title", "deliverable", "acceptance", "verify", "reqs"):
            if not i.get(field):
                errors.append(f"{k}: missing {field}")
        if i["milestone"] not in milestones:
            errors.append(f"{k}: unknown milestone {i['milestone']}")
        if i.get("size") not in sizes:
            errors.append(f"{k}: size must be one of {sorted(sizes)}")
        for l in i.get("labels", []):
            if l not in labels:
                errors.append(f"{k}: unknown label {l}")
        types = [l for l in i.get("labels", []) if l.startswith("type:")]
        if len(types) != 1:
            errors.append(f"{k}: needs exactly one type:* label, has {types}")
        for r in i.get("reqs", []):
            if r not in reqs:
                errors.append(f"{k}: unknown requirement {r}")

    phase = {k: PHASE_ORDER.index(i["milestone"]) for k, i in by_key.items()}
    for k, i in by_key.items():
        for d in i.get("depends", []):
            if d not in by_key:
                errors.append(f"{k}: unknown dependency {d}")
            elif phase[d] > phase[k]:
                errors.append(f"{k}: depends on {d} from a later phase")

    # cycles
    state: dict[str, int] = {}

    def visit(k: str, stack: list[str]) -> None:
        if state.get(k) == 2:
            return
        if state.get(k) == 1:
            errors.append("dependency cycle: " + " -> ".join(stack + [k]))
            return
        state[k] = 1
        for d in by_key.get(k, {}).get("depends", []):
            if d in by_key:
                visit(d, stack + [k])
        state[k] = 2

    for k in by_key:
        visit(k, [])

    # gates
    for k, i in by_key.items():
        if "type:gate" in i["labels"]:
            same = {o for o, oi in by_key.items() if oi["milestone"] == i["milestone"] and o != k}
            missing = same - set(i.get("depends", []))
            if missing:
                errors.append(f"{k}: gate does not depend on {sorted(missing)}")

    for k, i in by_key.items():
        if "type:decision" in i["labels"] and not any(ADR_FILE.search(d) for d in i.get("deliverable", [])):
            errors.append(f"{k}: decision issue must name a docs/adr/ file (docs/adr/NNNN-slug.md) in its deliverable")

    for k, i in by_key.items():
        if "type:decision" in i["labels"] and "needs:human" not in i["labels"]:
            errors.append(f"{k}: decisions need a person; label it needs:human")

    covered = {r for i in issues for r in i.get("reqs", [])}
    for r in reqs:
        if r not in covered:
            errors.append(f"{r}: requirement not covered by any issue ({reqs[r]['text'][:60]})")
    return errors


def check_scope(issues: list[dict], scope: Path = SCOPE) -> list[str]:
    """Every post-MVP (LATER) issue is linked from docs/scope.md (MVP-3)."""
    if not scope.is_file():
        return [f"{scope}: missing; it must link every LATER issue"]
    text = scope.read_text()
    return [f"{i['key']}: LATER issue not linked from docs/scope.md as [{i['key']}](<issue URL>)"
            for i in issues
            if i["milestone"] == "LATER"
            and not re.search(rf"\[{re.escape(i['key'])}\]\({ISSUE_URL}\)", text)]


def coverage_md(reqs: dict, meta: dict, issues: list[dict]) -> str:
    by_req: dict[str, list[str]] = {r: [] for r in reqs}
    for i in issues:
        for r in i["reqs"]:
            by_req.setdefault(r, []).append(i["key"])
    lines = ["# Requirement coverage", "", "Generated by plan/check.py. Do not edit.", ""]
    section = None
    for r, info in reqs.items():
        if info["section"] != section:
            section = info["section"]
            lines += ["", f"## {section}", "", "| ID | Kind | Requirement | Issues |", "| --- | --- | --- | --- |"]
        lines.append(f"| {r} | {info['kind']} | {info['text']} | {', '.join(by_req[r])} |")
    lines += ["", "## Issues per milestone", ""]
    for m in meta["milestones"]:
        n = sum(1 for i in issues if i["milestone"] == m["key"])
        lines.append(f"- {m['title']}: {n} issues")
    return "\n".join(lines) + "\n"


def main() -> int:
    reqs = load_requirements()
    meta, issues = load_backlog()
    errors = check(reqs, meta, issues) + check_scope(issues)
    if "--write-coverage" in sys.argv:
        (PLAN / "coverage.md").write_text(coverage_md(reqs, meta, issues))
    if errors:
        print(f"{len(errors)} problem(s):")
        for e in errors:
            print("  " + e)
        return 1
    implied = sum(1 for r in reqs.values() if r["kind"] == "implied")
    print(f"OK: {len(reqs)} requirements ({implied} implied), {len(issues)} issues, all covered")
    return 0


if __name__ == "__main__":
    sys.exit(main())
