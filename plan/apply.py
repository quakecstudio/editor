#!/usr/bin/env python3
"""Create or update milestones, labels and issues on GitHub from the plan.

Idempotent: each issue body carries <!-- plan-key: KEY -->; existing issues are
found by that marker and updated instead of duplicated. Dependencies are written
as issue links once every issue exists.

Usage:
  python3 plan/apply.py --repo quakecstudio/editor --dry-run
  python3 plan/apply.py --repo quakecstudio/editor
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import time
from urllib.parse import quote

from check import PHASE_ORDER, check, load_backlog, load_requirements

MARKER = re.compile(r"<!-- plan-key: ([A-Z0-9-]+) -->")


def gh(args: list[str], payload: dict | None = None) -> object:
    """Call the GitHub API. Writes are paced and retried on secondary rate limits."""
    cmd = ["gh", "api", *args]
    if payload is not None:
        cmd += ["--input", "-"]
    writing = payload is not None
    for attempt in range(6):
        out = subprocess.run(cmd, input=json.dumps(payload) if writing else None,
                             capture_output=True, text=True)
        if out.returncode == 0:
            if writing:
                time.sleep(1.0)  # stay under GitHub's content-creation limits
            if not out.stdout.strip():
                return None
            data = json.loads(out.stdout)
            if "--paginate" in args:  # called with --slurp: a list of pages
                return [item for page in data for item in page]
            return data
        if "rate limit" in out.stderr.lower() or "HTTP 403" in out.stderr or "HTTP 429" in out.stderr:
            wait = 30 * (attempt + 1)
            print(f"rate limited; waiting {wait}s", file=sys.stderr)
            time.sleep(wait)
            continue
        raise RuntimeError(f"gh api {' '.join(args)} failed: {out.stderr.strip()}")
    raise RuntimeError(f"gh api {' '.join(args)} still rate limited after retries")


def render_body(issue: dict, reqs: dict, meta: dict, numbers: dict[str, int], titles: dict[str, str]) -> str:
    ms = {m["key"]: m for m in meta["milestones"]}[issue["milestone"]]
    lines = [
        f"**Plan key:** `{issue['key']}` · **Phase:** {ms['title']} · **Size:** {issue['size']} ({meta['sizes'][issue['size']]})",
        "",
        "**Requirements**",
        *[f"- `{r}` {reqs[r]['text']}" for r in issue["reqs"]],
        "",
        "## Deliverable",
        *[f"- {d}" for d in issue["deliverable"]],
        "",
        "## Acceptance criteria",
        *[f"- [ ] {a}" for a in issue["acceptance"]],
        "",
        "## Verification",
        *[f"- {v}" for v in issue["verify"]],
    ]
    if issue.get("depends"):
        lines += ["", "## Depends on"]
        for d in issue["depends"]:
            ref = f"#{numbers[d]}" if d in numbers else f"`{d}`"
            lines.append(f"- [ ] {ref} {titles[d]}")
    if "clean-room:impl" in issue["labels"]:
        lines += ["", "> Clean-room implementation: work only from `docs/spec/`. Do not read the matching GPL source."]
    if "clean-room:spec" in issue["labels"]:
        lines += ["", "> Clean-room spec: record every GPL file you read in `docs/spec/AUTHORS.md`. You may not implement the matching code."]
    lines += ["", "## Definition of done", *[f"- [ ] {d}" for d in meta["definition_of_done"]], "",
              f"<!-- plan-key: {issue['key']} -->"]
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", required=True)
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    reqs = load_requirements()
    meta, issues = load_backlog()
    errors = check(reqs, meta, issues)
    if errors:
        print("plan/check.py fails; fix the plan first", file=sys.stderr)
        return 1
    issues.sort(key=lambda i: (PHASE_ORDER.index(i["milestone"]), i["key"]))
    titles = {i["key"]: i["title"] for i in issues}
    repo = f"repos/{args.repo}"

    all_labels = [l for group in meta["labels"].values() for l in group]
    if args.dry_run:
        print(f"would ensure {len(all_labels)} labels, {len(meta['milestones'])} milestones, {len(issues)} issues")
        for i in issues[:3]:
            print("-" * 60, f"\n{i['title']}\n", render_body(i, reqs, meta, {}, titles))
        return 0

    existing_labels = {l["name"] for l in gh([f"{repo}/labels", "--paginate", "--slurp"])}
    for l in all_labels:
        body = {"name": l["name"], "color": l["color"], "description": l["description"]}
        if l["name"] in existing_labels:
            gh(["-X", "PATCH", f"{repo}/labels/{quote(l['name'], safe='')}"], body)
        else:
            gh(["-X", "POST", f"{repo}/labels"], body)

    ms_numbers = {m["title"]: m["number"] for m in gh([f"{repo}/milestones?state=all", "--paginate", "--slurp"])}
    ms_by_key = {}
    for m in meta["milestones"]:
        body = {"title": m["title"], "description": f"Gate: {m['gate']}"}
        if m["title"] in ms_numbers:
            gh(["-X", "PATCH", f"{repo}/milestones/{ms_numbers[m['title']]}"], body)
            ms_by_key[m["key"]] = ms_numbers[m["title"]]
        else:
            ms_by_key[m["key"]] = gh(["-X", "POST", f"{repo}/milestones"], body)["number"]

    numbers: dict[str, int] = {}
    for gi in gh([f"{repo}/issues?state=all&per_page=100", "--paginate", "--slurp"]):
        m = MARKER.search(gi.get("body") or "")
        if m:
            numbers[m.group(1)] = gi["number"]

    # pass 1: create missing issues; pass 2: write bodies with dependency links
    for i in issues:
        if i["key"] not in numbers:
            body = render_body(i, reqs, meta, numbers, titles)
            created = gh(["-X", "POST", f"{repo}/issues"],
                         {"title": i["title"], "body": body, "labels": i["labels"], "milestone": ms_by_key[i["milestone"]]})
            numbers[i["key"]] = created["number"]
            print(f"created #{created['number']} {i['key']} {i['title']}")
    for i in issues:
        body = render_body(i, reqs, meta, numbers, titles)
        gh(["-X", "PATCH", f"{repo}/issues/{numbers[i['key']]}"],
           {"title": i["title"], "body": body, "labels": i["labels"], "milestone": ms_by_key[i["milestone"]]})
    print(f"done: {len(issues)} issues in sync")
    return 0


if __name__ == "__main__":
    sys.exit(main())
