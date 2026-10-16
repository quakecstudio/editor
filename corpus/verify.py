#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Check corpus/manifest.toml and the repository against the corpus policy.

Offline (default): the manifest's shape, that every entry records a license
with pinned evidence, that only GPL-3.0-or-later compatible items are marked
`vendor`, that QuakeC 1.06 and id's game data are listed as never fetched, that
every test asset archive is pinned and carries its source's license files, and
that no corpus file or game data is tracked in git.

--online: also downloads every pinned file at its commit and compares SHA-256,
which checks the URLs, the commits and the checksums.

Needs Python 3.11 or later (tomllib). cargo xtask corpus verify replaces this
script in P1-14.
"""

from __future__ import annotations

import argparse
import hashlib
import re
import subprocess
import sys
import tomllib
import urllib.parse
import urllib.request
from collections.abc import Callable, Iterable
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
MANIFEST = HERE / "manifest.toml"

USES = ("vendor", "fetch-at-test", "never")
KINDS = ("id", "mod")
# Licenses whose code may be combined with GPL-3.0-or-later code and so may be
# copied into this repository as test fixtures.
GPL3_COMPATIBLE = frozenset(
    {
        "GPL-2.0-or-later",
        "GPL-3.0-only",
        "GPL-3.0-or-later",
        "BSD-2-Clause",
        "BSD-3-Clause",
        "ISC",
        "MIT",
        "Apache-2.0",
        "Zlib",
        "CC0-1.0",
    }
)
# Quake content: tracked files with these extensions are corpus or game data.
CONTENT_EXTS = frozenset(
    {".qc", ".qh", ".src", ".map", ".bsp", ".pak", ".pk3", ".dat", ".wad", ".lmp", ".mdl", ".spr"}
)
REQUIRED_IDS = ("id-1.01", "rerelease")
# The never entry for id's pak0/pak1, which no tool fetches and CI never uses.
ID_GAME_DATA = "id1-paks"
MIN_MODS = 10
MIN_MAPS = 20

URL_RE = re.compile(r"^https://(github\.com|gitlab\.com)/[^/]+/[^/]+$")
ID_RE = re.compile(r"^[a-z0-9][a-z0-9.-]*$")
SHA1_RE = re.compile(r"^[0-9a-f]{40}$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
RELEASE_RE = re.compile(r"^(https://github\.com/[^/]+/[^/]+)/releases/download/[^/]+/[^/]+\.zip$")
LICENSE_RE = re.compile(r"^(NONE|LicenseRef-[A-Za-z0-9.-]+|[A-Za-z0-9.+-]+)$")


def load(path: Path = MANIFEST) -> dict:
    with path.open("rb") as f:
        return tomllib.load(f)


def _entries(m: dict) -> list[tuple[str, dict]]:
    return [(t, e) for t in ("codebase", "map", "asset") for e in m.get(t, [])]


def check(m: dict) -> list[str]:
    """Offline checks of the manifest itself; returns a list of errors."""
    errs: list[str] = []
    if m.get("schema") != 1:
        errs.append("schema must be 1")
    sources = _check_sources(m, errs)
    seen: set[str] = set()
    for table, e in _entries(m):
        where = f"{table} {e.get('id', '')}"
        if e.get("id", "") in seen:
            errs.append(f"{where}: duplicate id")
        seen.add(e.get("id", ""))
        _check_entry(table, e, where, errs)
        if table == "asset":
            if e.get("use") != "never":
                _check_asset(e, where, sources, errs)
        elif e.get("use") in ("vendor", "fetch-at-test"):
            _check_pins(table, e, where, sources, errs)
    _check_policy(m, errs)
    return errs


def _check_sources(m: dict, errs: list[str]) -> dict[str, dict]:
    sources: dict[str, dict] = {}
    blocked = [u.rstrip("/").lower() for u in m.get("blocked_urls", [])]
    for s in m.get("source", []):
        sid = s.get("id", "")
        if not ID_RE.match(sid):
            errs.append(f"source {sid!r}: bad id")
        if sid in sources:
            errs.append(f"source {sid}: duplicate id")
        sources[sid] = s
        url = s.get("url", "")
        if not URL_RE.match(url):
            errs.append(
                f"source {sid}: url must be https://github.com/o/r or https://gitlab.com/o/r"
            )
        if any(url.rstrip("/").lower().startswith(b) for b in blocked):
            errs.append(f"source {sid}: {url} is blocked")
        if not SHA1_RE.match(s.get("commit", "")):
            errs.append(f"source {sid}: commit must be a full 40-character SHA")
        sums = s.get("checksums", {})
        if not sums:
            errs.append(f"source {sid}: no pinned checksums")
        errs.extend(
            f"source {sid}: {p}: bad sha256" for p, h in sums.items() if not SHA256_RE.match(h)
        )
    return sources


def _check_entry(table: str, e: dict, where: str, errs: list[str]) -> None:
    """Fields every codebase or map needs, and what each `use` requires."""
    if not ID_RE.match(e.get("id", "")):
        errs.append(f"{where}: bad id")
    lic = e.get("license", "")
    if not LICENSE_RE.match(lic):
        errs.append(f"{where}: license must be an SPDX id, LicenseRef-* or NONE")
    use = e.get("use")
    if use not in USES:
        errs.append(f"{where}: use must be one of {', '.join(USES)}")
    if table == "codebase" and e.get("kind") not in KINDS:
        errs.append(f"{where}: kind must be one of {', '.join(KINDS)}")
    if use == "never":
        if "source" in e or "path" in e:
            errs.append(f"{where}: a never entry must not name a source or path")
        if not e.get("reason"):
            errs.append(f"{where}: a never entry needs a reason")
    elif use == "vendor" and lic not in GPL3_COMPATIBLE:
        errs.append(f"{where}: {lic} is not GPL-3.0-or-later compatible, so use cannot be vendor")
    if table == "asset" and use == "vendor":
        errs.append(f"{where}: assets are binary game data, so use must be fetch-at-test or never")


def _check_pins(table: str, e: dict, where: str, sources: dict, errs: list[str]) -> None:
    """A fetched entry names a pinned source, and its evidence (and map) is checksummed."""
    src = sources.get(e.get("source", ""))
    if src is None:
        errs.append(f"{where}: unknown source {e.get('source')!r}")
        return
    path = e.get("path", "")
    if not path:
        errs.append(f"{where}: missing path")
    if table == "map":
        if not path.endswith(".map"):
            errs.append(f"{where}: path must be a .map file")
        elif path not in src.get("checksums", {}):
            errs.append(f"{where}: {path} has no pinned checksum in source {src.get('id')}")
    evidence = e.get("evidence", [])
    if not evidence:
        errs.append(f"{where}: no license evidence")
    for ev in evidence:
        sid, _, p = ev.rpartition(":")
        esrc = sources.get(sid) if sid else src
        if esrc is None:
            errs.append(f"{where}: evidence {ev}: unknown source")
        elif p not in esrc.get("checksums", {}):
            errs.append(f"{where}: evidence {ev} has no pinned checksum")


def safe_member(path: str) -> bool:
    """A relative archive path with no empty, `.`, `..`, drive or backslash parts."""
    return (
        bool(path)
        and "\\" not in path
        and ":" not in path
        and all(p not in ("", ".", "..") for p in path.split("/"))
    )


def _check_asset(e: dict, where: str, sources: dict, errs: list[str]) -> None:
    """A test asset is a pinned release archive of a pinned source. Every member that is
    extracted is pinned too, and the source's license evidence is among them, which ties
    the archive to the license files read at the pinned commit."""
    src = sources.get(e.get("source", ""))
    if src is None:
        errs.append(f"{where}: unknown source {e.get('source')!r}")
        return
    match = RELEASE_RE.match(e.get("url", ""))
    if match is None or match.group(1) != src.get("url"):
        errs.append(f"{where}: url must be a .zip release download of {src.get('url')}")
    if not SHA256_RE.match(e.get("sha256", "")):
        errs.append(f"{where}: bad sha256")
    if not safe_member(e.get("game_dir", "")):
        errs.append(f"{where}: game_dir must be a relative directory such as id1")
    strip = e.get("strip", "")
    members = e.get("members", {})
    if not members:
        errs.append(f"{where}: no pinned members")
    for p, h in members.items():
        if not safe_member(p) or not p.startswith(strip) or p == strip:
            errs.append(f"{where}: member {p!r} must be a safe path under {strip!r}")
        if not SHA256_RE.match(h):
            errs.append(f"{where}: member {p}: bad sha256")
    evidence = e.get("evidence", [])
    if not evidence:
        errs.append(f"{where}: no license evidence")
    for ev in evidence:
        want = src.get("checksums", {}).get(ev)
        if want is None:
            errs.append(f"{where}: evidence {ev} has no pinned checksum")
        elif want not in members.values():
            errs.append(f"{where}: evidence {ev} is not among the archive's pinned members")
    for name, stand_in in e.get("stand_ins", {}).items():
        if not (safe_member(name) and safe_member(stand_in)):
            errs.append(f"{where}: stand-in {name!r} = {stand_in!r} must be relative paths")


def _check_policy(m: dict, errs: list[str]) -> None:
    """The never list matches the never entries, and the corpus meets its minimums."""
    never = set(m.get("never", []))
    never_entries = {e.get("id") for _, e in _entries(m) if e.get("use") == "never"}
    errs.extend(
        f"never list: {i} has no entry with use = never" for i in sorted(never - never_entries)
    )
    errs.extend(
        f"{i}: use = never but missing from the never list" for i in sorted(never_entries - never)
    )
    if "quakec-1.06" not in never:
        errs.append("never list must include quakec-1.06 (LIC-5)")
    if ID_GAME_DATA not in never:
        errs.append(f"never list must include {ID_GAME_DATA} (LIC-4)")
    usable = {e.get("id") for e in m.get("codebase", []) if e.get("use") != "never"}
    errs.extend(f"missing required codebase {r}" for r in REQUIRED_IDS if r not in usable)
    assets = [e for e in m.get("asset", []) if e.get("use") != "never"]
    errs.extend(
        f"asset {e.get('id')}: covers unknown codebase {c}"
        for e in assets
        for c in e.get("covers", [])
        if c not in usable
    )
    if not any("rerelease" in e.get("covers", []) for e in assets):
        errs.append("no test asset covers the rerelease codebase's precaches (QA-4)")
    mods = [e for e in m.get("codebase", []) if e.get("kind") == "mod" and e.get("use") != "never"]
    if len(mods) < MIN_MODS:
        errs.append(f"{len(mods)} usable mods; need at least {MIN_MODS}")
    maps = [e for e in m.get("map", []) if e.get("use") != "never"]
    if len(maps) < MIN_MAPS:
        errs.append(f"{len(maps)} usable maps; need at least {MIN_MAPS}")


def tracked_files(repo: Path = REPO) -> list[str]:
    out = subprocess.run(
        ["git", "-C", str(repo), "ls-files", "-z"], capture_output=True, check=True
    ).stdout
    return [p for p in out.decode().split("\0") if p]


def check_repo(m: dict, files: Iterable[str], read: Callable[[str], bytes]) -> list[str]:
    """Nothing from the corpus, and no game data, is tracked in the repository."""
    errs: list[str] = []
    fixture_dirs = [d.rstrip("/") + "/" for d in m.get("fixture_dirs", [])]
    pinned_content = {
        h: f"{s['id']}:{p}"
        for s in m.get("source", [])
        for p, h in s.get("checksums", {}).items()
        if Path(p).suffix.lower() in CONTENT_EXTS
    }
    for f in files:
        ext = Path(f).suffix.lower()
        if ext == ".pak":
            errs.append(f"{f}: pak files (game data) are never committed")
            continue
        if ext in CONTENT_EXTS:
            if not any(f.startswith(d) for d in fixture_dirs):
                errs.append(f"{f}: Quake content outside the manifest's fixture_dirs")
            digest = hashlib.sha256(read(f)).hexdigest()
            if digest in pinned_content:
                errs.append(f"{f}: is a copy of corpus file {pinned_content[digest]}")
    return errs


def raw_url(url: str, commit: str, path: str) -> str:
    parsed = urllib.parse.urlparse(url)
    project = parsed.path.strip("/")
    if parsed.netloc == "github.com":
        return f"https://raw.githubusercontent.com/{project}/{commit}/{urllib.parse.quote(path)}"
    if parsed.netloc == "gitlab.com":
        return (
            "https://gitlab.com/api/v4/projects/"
            + urllib.parse.quote(project, safe="")
            + "/repository/files/"
            + urllib.parse.quote(path, safe="")
            + f"/raw?ref={commit}"
        )
    raise ValueError(f"unsupported host in {url}")


def fetch(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "quakecstudio-corpus-verify"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


def check_online(m: dict, get: Callable[[str], bytes] = fetch) -> list[str]:
    """Download every pinned file at its commit and compare SHA-256."""
    errs: list[str] = []
    for s in m.get("source", []):
        for p, want in s.get("checksums", {}).items():
            url = raw_url(s["url"], s["commit"], p)
            try:
                got = hashlib.sha256(get(url)).hexdigest()
            # Any failure (HTTP status, DNS, timeout) is one more error to report, not a crash.
            except Exception as exc:
                errs.append(f"source {s['id']}: {p}: fetch failed: {exc}")
                continue
            if got != want:
                errs.append(f"source {s['id']}: {p}: sha256 {got}, manifest says {want}")
    return errs


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--manifest", type=Path, default=MANIFEST)
    ap.add_argument("--online", action="store_true", help="download pinned files and check SHA-256")
    args = ap.parse_args(argv)

    m = load(args.manifest)
    errs = check(m)
    errs += check_repo(m, tracked_files(), lambda f: (REPO / f).read_bytes())
    if args.online:
        errs += check_online(m)
    for e in errs:
        print(f"error: {e}", file=sys.stderr)
    n_files = sum(len(s.get("checksums", {})) for s in m.get("source", []))
    mode = "online" if args.online else "offline"
    print(
        f"corpus {mode}: {len(m.get('codebase', []))} codebases, {len(m.get('map', []))} maps, "
        f"{len(m.get('asset', []))} assets, {len(m.get('source', []))} sources, {n_files} pinned files, {len(errs)} errors"
    )
    return 1 if errs else 0


if __name__ == "__main__":
    sys.exit(main())
