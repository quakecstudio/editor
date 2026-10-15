#!/usr/bin/env python3
"""Report a phase's progress from the plan and GitHub, for people and for /goal.

Issues are grouped into tracks by who may work them:

  spec    clean-room:spec issues and gpl-tools issues (the only work that may read id's GPL source)
  build   every other issue an agent can finish (implementation, tests, tooling, docs)
  review  type:readiness issues (an independent reviewer, see docs/testing.md)
  human   needs:human issues (decisions, sign-offs, hardware, credentials, legal review)
  agents  spec + build + review: everything an agent can close
  all     the whole phase

The last line is the verdict a /goal condition can test:

  STATUS P1 agents: COMPLETE                      nothing open in the track
  STATUS P1 agents: IN PROGRESS, next #45 #47     open issues whose dependencies are all closed
  STATUS P1 agents: WAITING ON PEOPLE #20 #33     everything open is blocked by needs:human issues
  STATUS P1 agents: BLOCKED #61                   blocked by open work outside the track that is not
                                                  needs:human (another track or an earlier phase)

Usage:
  python3 plan/status.py --phase P1 [--track agents] [--json] [--repo quakecstudio/editor]
  python3 plan/status.py --phase P1 --states states.json   # offline: {"P1-07": "closed", ...}
"""

from __future__ import annotations

import argparse
import json
import re
import sys

from check import PHASE_ORDER, load_backlog

TRACKS = ("spec", "build", "review", "human", "agents", "all")


def track_of(issue: dict) -> str:
    labels = issue.get("labels", [])
    if "needs:human" in labels:
        return "human"
    if "type:readiness" in labels:
        return "review"
    if "clean-room:spec" in labels or "gpl-tools" in labels:
        return "spec"
    return "build"


def in_track(issue: dict, track: str) -> bool:
    t = track_of(issue)
    if track == "all":
        return True
    if track == "agents":
        return t != "human"
    return t == track


def compute(issues: list[dict], states: dict[str, dict], phase: str, track: str) -> dict:
    """states maps plan key -> {"number": int | None, "state": "open" | "closed"}."""
    by_key = {i["key"]: i for i in issues}

    def is_closed(k: str) -> bool:
        return states.get(k, {}).get("state") == "closed"

    def num(k: str) -> str:
        n = states.get(k, {}).get("number")
        return f"#{n}" if n else k

    def row(k: str) -> dict:
        i = by_key[k]
        return {"key": k, "number": states.get(k, {}).get("number"), "title": i["title"],
                "track": track_of(i)}

    mine = [i for i in issues if i["milestone"] == phase and in_track(i, track)]
    mine.sort(key=lambda i: i["key"])
    closed = [i["key"] for i in mine if is_closed(i["key"])]
    open_ = [i["key"] for i in mine if not is_closed(i["key"])]
    mine_keys = {i["key"] for i in mine}

    def open_blockers(k: str, seen: set[str]) -> set[str]:
        """Open issues outside the track that keep k from starting, followed through the track."""
        out: set[str] = set()
        for d in by_key[k].get("depends", []):
            if d not in by_key or is_closed(d) or d in seen:
                continue
            seen.add(d)
            if d in mine_keys:
                out |= open_blockers(d, seen)
            else:
                out.add(d)
        return out

    ready = [k for k in open_ if all(is_closed(d) for d in by_key[k].get("depends", []) if d in by_key)]
    waiting: dict[str, list[str]] = {}
    for k in open_:
        if k not in ready:
            waiting[k] = sorted(open_blockers(k, set()))

    blockers = sorted({b for bs in waiting.values() for b in bs})
    human_blockers = [b for b in blockers if track_of(by_key[b]) == "human"]
    other_blockers = [b for b in blockers if track_of(by_key[b]) != "human"]

    if not open_:
        verdict = "COMPLETE"
    elif ready:
        verdict = "IN PROGRESS, next " + " ".join(num(k) for k in ready)
    elif other_blockers:
        verdict = "BLOCKED " + " ".join(num(k) for k in other_blockers)
    else:
        verdict = "WAITING ON PEOPLE " + " ".join(num(k) for k in human_blockers)

    return {
        "phase": phase,
        "track": track,
        "closed": [row(k) for k in closed],
        "ready": [row(k) for k in ready],
        "waiting": [{**row(k), "blocked_by": [row(b) for b in bs]} for k, bs in waiting.items()],
        "verdict": verdict,
        "status_line": f"STATUS {phase} {track}: {verdict}",
    }


def github_states(repo: str) -> dict[str, dict]:
    from apply import MARKER, gh_list  # imported lazily so offline runs need no gh

    states: dict[str, dict] = {}
    for gi in gh_list(f"repos/{repo}/issues?state=all"):
        if "pull_request" in gi:
            continue
        m = MARKER.search(gi.get("body") or "")
        if m:
            states[m.group(1)] = {"number": gi["number"], "state": gi["state"]}
    return states


def render(report: dict, phase_title: str) -> str:
    def fmt(r: dict) -> str:
        n = f"#{r['number']}" if r["number"] else r["key"]
        return f"{n} {r['key']} [{r['track']}] {r['title']}"

    total = len(report["closed"]) + len(report["ready"]) + len(report["waiting"])
    lines = [f"{phase_title}, track {report['track']}: {len(report['closed'])}/{total} closed"]
    if report["ready"]:
        lines.append("Ready (dependencies closed):")
        lines += [f"  {fmt(r)}" for r in report["ready"]]
    if report["waiting"]:
        lines.append("Waiting:")
        for w in report["waiting"]:
            by = ", ".join(fmt(b) for b in w["blocked_by"]) or "open work in this track"
            lines.append(f"  {fmt(w)}\n      on {by}")
    lines.append(report["status_line"])
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--phase", required=True, choices=PHASE_ORDER)
    ap.add_argument("--track", default="agents", choices=TRACKS)
    ap.add_argument("--repo", default="quakecstudio/editor")
    ap.add_argument("--states", help="JSON file mapping plan key to 'open' or 'closed' (offline)")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    meta, issues = load_backlog()
    if args.states:
        raw = json.loads(open(args.states, encoding="utf-8").read())
        states = {k: {"number": None, "state": v} for k, v in raw.items()}
    else:
        states = github_states(args.repo)
    report = compute(issues, states, args.phase, args.track)
    title = {m["key"]: m["title"] for m in meta["milestones"]}[args.phase]
    print(json.dumps(report, indent=2) if args.json else render(report, title))
    return 0


if __name__ == "__main__":
    sys.exit(main())
