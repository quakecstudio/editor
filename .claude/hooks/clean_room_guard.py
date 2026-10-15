#!/usr/bin/env python3
"""PreToolUse hook for the implementer subagent: keep clean-room implementers away from GPL source.

Clean-room implementation work (docs/clean-room.md, from #9) may only use docs/spec/. This hook
blocks tool calls that would read, fetch or clone the GPL implementations we re-implement:
id's engine and tools, our oracles repo, and the GPL source ports and compilers. QuakeC game
code (the 1.01 and rerelease corpora) is test input, not an implementation, and is allowed.

Oracle binaries (downloaded from the oracles repo's releases into target/oracle-bin/) may be
run: comparing outputs is what the differential tests do.

Exit 0 lets the call through; exit 2 blocks it and tells the agent why (stderr).
"""

from __future__ import annotations

import json
import re
import sys

BLOCKED = [
    # id's engine and tools repositories (not quake-rerelease-qc, which is QuakeC corpus)
    (re.compile(r"id-software/(quake|quake-tools|quake-2|qw)(\.git)?(?=[/\s\"'#?]|$)", re.I),
     "id Software's GPL engine or tools source"),
    # our own GPL oracle and preview host repositories
    # (release downloads are allowed: running an oracle binary to compare outputs is the point)
    (re.compile(r"quakecstudio/(oracles|preview-host)(\.git)?(?=[/\s\"'#?]|$)(?!/releases/download/)", re.I),
     "the GPL oracles or preview host repository"),
    (re.compile(r"(^|[\s\"']|(?<!quakecstudio)/)oracles/", re.I), "a checkout of the GPL oracles repository"),
    # GPL source ports and compilers hosted on forges
    (re.compile(r"(github\.com|gitlab\.com|sourceforge\.net|codeberg\.org)/[^\s\"']*"
                r"(fteqw|fteqcc|gmqcc|ericw-tools|quakespasm|vkquake|ironwail|darkplaces|tyrutils)", re.I),
     "a GPL engine, compiler or map tool's source"),
    (re.compile(r"fteqw\.org/[^\s\"']*(svn|trunk|source|\.c\b)", re.I), "FTEQW's source"),
    # the engine and qcc source files the specs are written from
    (re.compile(r"(^|[\s\"'/])(pr_exec|pr_edict|pr_cmds|pr_comp|pr_lex|pr_fix|qcc|progdefs|progs)\.(c|h)\b", re.I),
     "an id engine or qcc source file"),
]


def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for v in value.values():
            yield from strings(v)
    elif isinstance(value, list):
        for v in value:
            yield from strings(v)


def verdict(event: dict) -> str | None:
    for text in strings(event.get("tool_input", {})):
        for pattern, what in BLOCKED:
            if pattern.search(text):
                return what
    return None


def main() -> int:
    try:
        event = json.load(sys.stdin)
    except json.JSONDecodeError:
        return 0
    what = verdict(event)
    if what is None:
        return 0
    print(
        f"Blocked by the clean-room guard: this {event.get('tool_name', 'tool')} call would reach {what}. "
        "Clean-room implementers work only from docs/spec/. If the spec is missing or unclear, "
        "stop and report what is missing so a spec issue can be filed; do not look at the GPL source.",
        file=sys.stderr,
    )
    return 2


if __name__ == "__main__":
    sys.exit(main())
