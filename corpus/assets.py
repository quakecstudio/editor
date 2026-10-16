#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Fetch the open test assets in corpus/manifest.toml and check what they cover.

fetch: downloads each usable [[asset]] archive into a cache outside the source
tree, checks its SHA-256, extracts only the pinned members (each checked against
its own SHA-256) and checks that the result has what a headless engine scenario
needs. A cached archive with the right SHA-256 is not downloaded again, so a
second run works offline.

precache: lists every precache_model and precache_sound call in each codebase an
asset `covers` (the rerelease QuakeC) and fails if a precached file is neither in
the asset's pak files nor given a stand-in.

Needs Python 3.11 or later (tomllib). cargo xtask assets fetch replaces this
script in P1-14. docs/test-assets.md has the license review and the policy.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import os
import re
import shutil
import struct
import sys
import tarfile
import urllib.request
import zipfile
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from pathlib import Path

import verify

USER_AGENT = "quakecstudio-test-assets"
# Files an engine needs to start a map with no id data: the palette and colormap,
# the HUD wad, and the map a scenario loads first.
SCENARIO_FILES = ("gfx/palette.lmp", "gfx/colormap.lmp", "gfx.wad", "maps/start.bsp")
# At least one of each kind of content a scenario exercises.
SCENARIO_KINDS = {"map": "maps/*.bsp", "model": "progs/*.mdl", "sound": "sound/*.wav"}
PAK_HEADER = struct.Struct("<4sii")
PAK_ENTRY = struct.Struct("<56sii")

# A precache call, or a string or comment to skip whole. Names are matched
# case-insensitively, since some compilers accept any case.
_SCAN_RE = re.compile(
    r"\bprecache_(?P<kind>model|sound)2?\s*\(\s*(?:\"(?P<name>[^\"]*)\"|(?P<expr>[^)\"]*))\)"
    r'|"(?:[^"\\\n]|\\.)*"|//[^\n]*|/\*.*?\*/',
    re.I | re.S,
)


class AssetError(Exception):
    """A download, checksum or archive problem that stops one asset."""


@dataclass(frozen=True)
class Precache:
    """One precache_model or precache_sound call; `name` is None when it is not a literal."""

    kind: str
    name: str | None
    file: str
    line: int
    arg: str

    def path(self) -> str | None:
        """The file the engine loads: models by their own path, sounds under sound/."""
        if self.name is None:
            return None
        return self.name if self.kind == "model" else f"sound/{self.name}"


def default_dest() -> Path:
    """QS_TEST_ASSETS, else quakecstudio/test-assets under the XDG cache directory."""
    if env := os.environ.get("QS_TEST_ASSETS"):
        return Path(env)
    base = os.environ.get("XDG_CACHE_HOME") or str(Path.home() / ".cache")
    return Path(base) / "quakecstudio" / "test-assets"


def download(url: str, out: Path) -> None:
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=300) as r, out.open("wb") as f:
        shutil.copyfileobj(r, f, 1 << 20)


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def usable_assets(m: dict) -> list[dict]:
    return [e for e in m.get("asset", []) if e.get("use") != "never"]


def fetch_archive(e: dict, dest: Path, get: Callable[[str, Path], None]) -> Path:
    """The asset's archive in the download cache, downloaded only if missing or wrong."""
    archive = dest / "downloads" / f"{e['sha256']}.zip"
    if archive.is_file() and sha256_file(archive) == e["sha256"]:
        return archive
    archive.parent.mkdir(parents=True, exist_ok=True)
    part = archive.with_suffix(".part")
    try:
        get(e["url"], part)
    # Any failure (HTTP status, DNS, timeout) stops this asset with one message.
    except Exception as exc:
        part.unlink(missing_ok=True)
        raise AssetError(f"download of {e['url']} failed: {exc}") from exc
    got = sha256_file(part)
    if got != e["sha256"]:
        part.unlink()
        raise AssetError(f"{e['url']}: sha256 {got}, manifest says {e['sha256']}")
    part.replace(archive)
    return archive


def extract(e: dict, archive: Path, dest: Path) -> Path:
    """Extract the pinned members, checking each SHA-256; returns the asset's directory."""
    out = dest / e["id"]
    strip = e.get("strip", "")
    try:
        zf = zipfile.ZipFile(archive)
    except zipfile.BadZipFile as exc:
        raise AssetError(f"{archive}: {exc}") from exc
    with zf:
        for name, want in e["members"].items():
            if not verify.safe_member(name) or not name.startswith(strip):
                raise AssetError(f"member {name!r} is not a safe path under {strip!r}")
            target = out / name.removeprefix(strip)
            if target.is_file() and sha256_file(target) == want:
                continue
            try:
                data = zf.read(name)
            except KeyError as exc:
                raise AssetError(f"{archive}: no member {name}") from exc
            got = hashlib.sha256(data).hexdigest()
            if got != want:
                raise AssetError(f"{name}: sha256 {got}, manifest says {want}")
            target.parent.mkdir(parents=True, exist_ok=True)
            tmp = target.with_name(target.name + ".part")
            tmp.write_bytes(data)
            tmp.replace(target)
    return out


def pak_index(path: Path) -> dict[str, tuple[int, int]]:
    """A Quake PACK file's directory: each file name with its offset and length."""
    size = path.stat().st_size
    with path.open("rb") as f:
        head = f.read(PAK_HEADER.size)
        if len(head) < PAK_HEADER.size:
            raise AssetError(f"{path}: truncated pak header")
        magic, ofs, length = PAK_HEADER.unpack(head)
        if magic != b"PACK":
            raise AssetError(f"{path}: not a PACK file")
        if ofs < 0 or length < 0 or length % PAK_ENTRY.size or ofs + length > size:
            raise AssetError(f"{path}: pak directory out of range")
        f.seek(ofs)
        table = f.read(length)
    index = {}
    for i in range(0, length, PAK_ENTRY.size):
        raw, pos, n = PAK_ENTRY.unpack_from(table, i)
        if pos < 0 or n < 0 or pos + n > size:
            raise AssetError(f"{path}: entry {i // PAK_ENTRY.size} out of range")
        index[raw.split(b"\0", 1)[0].decode("latin-1")] = (pos, n)
    return index


def pak_files(game_dir: Path) -> dict[str, tuple[Path, int, int]]:
    """Every file in the game directory's pak0.pak, pak1.pak, ... in the order the engine
    reads them, so a later pak's copy of a name wins."""
    files: dict[str, tuple[Path, int, int]] = {}
    i = 0
    while (pak := game_dir / f"pak{i}.pak").is_file():
        files.update({n: (pak, pos, ln) for n, (pos, ln) in pak_index(pak).items()})
        i += 1
    return files


def install_stand_ins(e: dict, game_dir: Path) -> None:
    """Write each stand-in as a loose file under the name the QuakeC precaches, copied
    from the pak file that has the stand-in, so the engine finds it."""
    files = pak_files(game_dir)
    for name, stand_in in e.get("stand_ins", {}).items():
        if stand_in not in files:
            raise AssetError(f"stand-in {stand_in} for {name} is not in the paks")
        pak, pos, ln = files[stand_in]
        with pak.open("rb") as f:
            f.seek(pos)
            data = f.read(ln)
        target = game_dir / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)


def check_scenario(names: set[str]) -> list[str]:
    """What a headless scenario needs is present (acceptance: a map, models, textures, sounds)."""
    errs = [f"missing {f}" for f in SCENARIO_FILES if f not in names]
    for kind, pattern in SCENARIO_KINDS.items():
        prefix, ext = pattern.split("*")
        if not any(n.startswith(prefix) and n.endswith(ext) for n in names):
            errs.append(f"no {kind} ({pattern})")
    return errs


def precache_calls(file: str, text: str) -> list[Precache]:
    """The precache calls in QuakeC source, left to right. Strings and comments are
    scanned as whole tokens, so a call inside one is not counted."""
    calls = []
    for mo in _SCAN_RE.finditer(text):
        if mo.group("kind") is None:
            continue
        line = text.count("\n", 0, mo.start()) + 1
        literal = mo.group("name")
        arg = literal if literal is not None else mo.group("expr").strip()
        calls.append(Precache(mo.group("kind").lower(), literal, file, line, arg))
    return calls


def coverage(
    calls: Iterable[Precache], names: set[str], stand_ins: dict[str, str]
) -> tuple[list[str], list[str], list[Precache]]:
    """Returns (precached paths missing from the assets, paths served by a stand-in,
    calls whose argument is not a literal)."""
    missing, standing_in, dynamic = set(), set(), []
    for c in calls:
        p = c.path()
        if p is None:
            dynamic.append(c)
        elif p not in names:
            if stand_ins.get(p) in names:
                standing_in.add(p)
            else:
                missing.add(p)
    return sorted(missing), sorted(standing_in), dynamic


def archive_url(src: dict) -> str:
    return f"{src['url']}/archive/{src['commit']}.tar.gz"


def codebase_sources(
    m: dict, codebase_id: str, get: Callable[[str], bytes] = verify.fetch
) -> dict[str, str]:
    """The .qc files of a codebase at its source's pinned commit, by path in the source.
    Every pinned checksum under the codebase's path is checked against the archive."""
    cb = next((c for c in m.get("codebase", []) if c.get("id") == codebase_id), None)
    if cb is None or cb.get("use") == "never":
        raise AssetError(f"no usable codebase {codebase_id}")
    src = next(s for s in m["source"] if s["id"] == cb["source"])
    base = "" if cb["path"] == "." else cb["path"].rstrip("/") + "/"
    try:
        blob = get(archive_url(src))
    # Any failure (HTTP status, DNS, timeout) is reported as one error.
    except Exception as exc:
        raise AssetError(f"download of {archive_url(src)} failed: {exc}") from exc
    files: dict[str, bytes] = {}
    with tarfile.open(fileobj=io.BytesIO(blob), mode="r:gz") as tar:
        for member in tar:
            _, _, rel = member.name.partition("/")
            if member.isfile() and rel.startswith(base):
                fobj = tar.extractfile(member)
                if fobj is not None:
                    files[rel] = fobj.read()
    for p, want in src.get("checksums", {}).items():
        if p.startswith(base):
            got = hashlib.sha256(files.get(p, b"")).hexdigest()
            if got != want:
                raise AssetError(f"{src['id']}:{p}: sha256 {got}, manifest says {want}")
    return {p: d.decode("latin-1") for p, d in sorted(files.items()) if p.lower().endswith(".qc")}


def cmd_fetch(m: dict, dest: Path, get: Callable[[str, Path], None] = download) -> list[str]:
    errs = []
    for e in usable_assets(m):
        try:
            out = extract(e, fetch_archive(e, dest, get), dest)
            game_dir = out / e["game_dir"]
            errs += [f"asset {e['id']}: {x}" for x in check_scenario(set(pak_files(game_dir)))]
            install_stand_ins(e, game_dir)
        except AssetError as exc:
            errs.append(f"asset {e['id']}: {exc}")
            continue
        print(f"asset {e['id']}: {len(e['members'])} files in {out}")
    return errs


def cmd_precache(m: dict, dest: Path, get: Callable[[str], bytes] = verify.fetch) -> list[str]:
    errs = []
    for e in usable_assets(m):
        game_dir = dest / e["id"] / e["game_dir"]
        if not game_dir.is_dir():
            errs.append(f"asset {e['id']}: {game_dir} does not exist; run fetch first")
            continue
        try:
            names = set(pak_files(game_dir))
        except AssetError as exc:
            errs.append(f"asset {e['id']}: {exc}")
            continue
        for cid in e.get("covers", []):
            try:
                sources = codebase_sources(m, cid, get)
            except AssetError as exc:
                errs.append(f"asset {e['id']}: {exc}")
                continue
            calls = [c for f, t in sources.items() for c in precache_calls(f, t)]
            missing, standing_in, dynamic = coverage(calls, names, e.get("stand_ins", {}))
            literal = {c.path() for c in calls} - {None}
            print(
                f"asset {e['id']} covers {cid}: {len(sources)} .qc files, {len(calls)} precache "
                f"calls, {len(literal)} files, {len(literal) - len(missing) - len(standing_in)} "
                f"present, {len(standing_in)} by stand-in, {len(missing)} missing"
            )
            for p in standing_in:
                print(f"  stand-in: {p} -> {e['stand_ins'][p]}")
            for c in dynamic:
                print(f"  not a literal: {c.file}:{c.line} precache_{c.kind}({c.arg})")
            errs += [f"asset {e['id']}: {cid} precaches {p}, which is missing" for p in missing]
    return errs


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("command", choices=("fetch", "precache"))
    ap.add_argument("--manifest", type=Path, default=verify.MANIFEST)
    ap.add_argument(
        "--dest",
        type=Path,
        help="cache directory (default: $QS_TEST_ASSETS, else $XDG_CACHE_HOME or ~/.cache, "
        "then quakecstudio/test-assets)",
    )
    args = ap.parse_args(argv)
    dest = args.dest or default_dest()

    m = verify.load(args.manifest)
    errs = verify.check(m)
    if not errs:
        errs = cmd_fetch(m, dest) if args.command == "fetch" else cmd_precache(m, dest)
    for e in errs:
        print(f"error: {e}", file=sys.stderr)
    return 1 if errs else 0


if __name__ == "__main__":
    sys.exit(main())
