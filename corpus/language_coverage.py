#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Check that docs/spec/language.md covers every construct id's QuakeC 1.01 uses.

Fetches the id-1.01 codebase from corpus/manifest.toml at its pinned commit (or reads
a local directory with --source-dir), splits every .qc file into tokens the way id's
qcc does (Quake-Tools qcc/pr_lex.c, PR_Lex), and lists every keyword, operator and
other punctuation, `$` command and kind of literal it finds. Each must appear in the
coverage table of the spec, a row `| `<construct>` | QC-n[, QC-m] |`, and every
section a row names must exist as a `## QC-n` heading. Exits 1 and names what is
missing otherwise.

Needs Python 3.11 or later (tomllib). Verification for P1-06 (#30).
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import Counter
from collections.abc import Callable, Iterator
from pathlib import Path

import assets
import verify

REPO = Path(__file__).resolve().parent.parent
SPEC = REPO / "docs" / "spec" / "language.md"
CODEBASE = "id-1.01"

# Names qcc compares a token against to decide what it is parsing: PR_ParseType in
# pr_lex.c (the types) and PR_ParseStatement in pr_comp.c (the statements).
KEYWORDS = frozenset(
    {"float", "vector", "entity", "string", "void", "local", "return", "if", "else", "while", "do"}
)
# pr_punctuation in pr_lex.c, in its order: the first entry that matches wins.
PUNCTUATION = (
    "&&", "||", "<=", ">=", "==", "!=", ";", ",", "!", "*", "/", "(", ")", "-", "+",
    "=", "[", "]", "{", "}", "...", ".", "<", ">", "#", "&", "|",
)  # fmt: skip
# The `$` commands PR_LexGrab in pr_lex.c knows; any other `$name` is a frame macro.
DOLLAR_COMMANDS = frozenset({"frame", "cd", "origin", "base", "flags", "scale", "skin"})

STRING = "string literal"
VECTOR = "vector literal"
FLOAT = "float literal"
NEGATIVE = "negative float literal"
FRAME_REF = "frame macro reference"
COMPONENT = "vector component name"
LINE_COMMENT = "// comment"
BLOCK_COMMENT = "/* comment */"

ROW_RE = re.compile(r"^\|\s*`(?P<key>[^`]+)`\s*\|\s*(?P<ids>QC-\d+(?:,\s*QC-\d+)*)\s*\|")
HEADING_RE = re.compile(r"^## (QC-\d+) ", re.MULTILINE)


class LexError(Exception):
    pass


def _is_name_start(c: str) -> bool:
    return c.isascii() and (c.isalpha() or c == "_")


def _is_name_char(c: str) -> bool:
    return c.isascii() and (c.isalnum() or c == "_")


def constructs(text: str) -> Iterator[tuple[str, int]]:
    """The constructs in one file, as (key, line), in source order."""
    i, line, n = 0, 1, len(text)
    macros: set[str] = set()

    def rest_of_line(j: int) -> tuple[list[str], int]:
        """PR_SimpleGetToken until the end of the line: words split at blanks, `,` and `;`."""
        end = text.find("\n", j)
        end = n if end < 0 else end
        words = [w for w in re.split(r"[\x00- ,;]+", text[j:end]) if w]
        return words, end

    while i < n:
        c = text[i]
        if c == "\n":
            line += 1
            i += 1
        elif c <= " ":
            i += 1
        elif text.startswith("//", i):
            yield LINE_COMMENT, line
            end = text.find("\n", i)
            i = n if end < 0 else end
        elif text.startswith("/*", i):
            yield BLOCK_COMMENT, line
            end = text.find("*/", i + 2)
            end = n if end < 0 else end + 2
            line += text.count("\n", i, end)
            i = end
        elif c == '"':
            yield STRING, line
            i += 1
            while True:
                if i >= n:
                    raise LexError(f"line {line}: EOF inside quote")
                ch = text[i]
                if ch == "\n":
                    raise LexError(f"line {line}: newline inside quote")
                if ch == "\\":
                    esc = text[i + 1 : i + 2]
                    if esc not in ("n", '"'):
                        raise LexError(f"line {line}: unknown escape char {esc!r}")
                    yield f"escape \\{esc}", line
                    i += 2
                    continue
                i += 1
                if ch == '"':
                    break
        elif c == "'":
            yield VECTOR, line
            end = text.find("'", i + 1)
            if end < 0:
                raise LexError(f"line {line}: bad vector")
            line += text.count("\n", i, end)
            i = end + 1
        elif c.isdigit() or (c == "-" and text[i + 1 : i + 2].isdigit()):
            yield (NEGATIVE if c == "-" else FLOAT), line
            i += 1
            while i < n and (text[i].isdigit() or text[i] == "."):
                i += 1
        elif _is_name_start(c):
            j = i
            while j < n and _is_name_char(text[j]):
                j += 1
            word = text[i:j]
            if word in KEYWORDS:
                yield word, line
            elif re.search(r"_[xyz]$", word):
                yield COMPONENT, line
            i = j
        elif c == "$":
            words, end = rest_of_line(i + 1)
            if not words:
                raise LexError(f"line {line}: hanging $")
            command = words[0]
            if command == "frame":
                macros.update(words[1:])
                yield "$frame", line
                i = end
            elif command in DOLLAR_COMMANDS or command not in macros:
                # A command qcc does not know is still listed, so the spec must say
                # what happens to it.
                yield f"${command}", line
                i = end
            else:
                yield FRAME_REF, line
                i += 1 + len(command)
        else:
            p = next((p for p in PUNCTUATION if text.startswith(p, i)), None)
            if p is None:
                raise LexError(f"line {line}: unknown punctuation {c!r}")
            yield p, line
            i += len(p)


def spec_table(spec: str) -> tuple[dict[str, list[str]], set[str]]:
    """The coverage rows (construct to section IDs) and the section headings of the spec."""
    rows: dict[str, list[str]] = {}
    for raw in spec.splitlines():
        mo = ROW_RE.match(raw)
        if mo:
            # A `|` in a Markdown table cell is written `\|`.
            key = mo.group("key").replace("\\|", "|")
            rows[key] = [s.strip() for s in mo.group("ids").split(",")]
    return rows, set(HEADING_RE.findall(spec))


def check(sources: dict[str, str], spec: str) -> tuple[Counter[str], list[str]]:
    """Counts each construct in `sources` and lists what the spec does not cover."""
    counts: Counter[str] = Counter()
    where: dict[str, str] = {}
    errs: list[str] = []
    for path, text in sorted(sources.items()):
        try:
            for key, line in constructs(text):
                counts[key] += 1
                where.setdefault(key, f"{path}:{line}")
        except LexError as exc:
            errs.append(f"{path}: {exc}")
    rows, headings = spec_table(spec)
    for key in sorted(counts):
        if key not in rows:
            errs.append(f"`{key}` (first at {where[key]}) has no row in the coverage table")
            continue
        for sid in rows[key]:
            if sid not in headings:
                errs.append(f"`{key}` maps to {sid}, which is not a section of the spec")
    return counts, errs


def local_sources(directory: Path) -> dict[str, str]:
    return {
        p.name: p.read_bytes().decode("latin-1")
        for p in sorted(directory.iterdir())
        if p.suffix.lower() == ".qc"
    }


def main(argv: list[str] | None = None, get: Callable[[str], bytes] = verify.fetch) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--spec", type=Path, default=SPEC, help="the spec to check")
    ap.add_argument(
        "--source-dir", type=Path, help="read the .qc files here instead of fetching them"
    )
    args = ap.parse_args(argv)
    try:
        if args.source_dir:
            sources = local_sources(args.source_dir)
        else:
            sources = assets.codebase_sources(verify.load(), CODEBASE, get)
    except (OSError, assets.AssetError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    if not sources:
        print("error: no .qc files found", file=sys.stderr)
        return 1
    counts, errs = check(sources, args.spec.read_text(encoding="utf-8"))
    rows, _ = spec_table(args.spec.read_text(encoding="utf-8"))
    print(f"{CODEBASE}: {len(sources)} .qc files, {len(counts)} distinct constructs")
    for key in sorted(counts):
        print(f"  {key:<24} {counts[key]:>6}  {', '.join(rows.get(key, ['MISSING']))}")
    for e in errs:
        print(f"error: {e}", file=sys.stderr)
    return 1 if errs else 0


if __name__ == "__main__":
    sys.exit(main())
