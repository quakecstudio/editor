# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests for corpus/assets.py. Run: cd corpus && python3.11 -m unittest test_assets.py

Everything runs offline against small archives built here; nothing is downloaded.
"""

import contextlib
import hashlib
import io
import os
import struct
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

import assets
import verify

COMMIT = "c" * 40


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def make_pak(files: dict[str, bytes]) -> bytes:
    """A PACK file: header, file data, then the directory."""
    body = b""
    entries = b""
    pos = 12
    for name, data in files.items():
        entries += struct.pack("<56sii", name.encode(), pos + len(body), len(data))
        body += data
    return struct.pack("<4sii", b"PACK", 12 + len(body), len(entries)) + body + entries


SCENARIO = {
    "gfx/palette.lmp": b"p" * 768,
    "gfx/colormap.lmp": b"c",
    "gfx.wad": b"WAD2",
    "maps/start.bsp": b"bsp",
    "progs/player.mdl": b"IDPO",
    "sound/boss1/death.wav": b"RIFF-boss1",
}
PAK0 = make_pak(SCENARIO)
PAK1 = make_pak({"sound/player/slmbrn2.wav": b"RIFF-slime"})
LICENSE = b"BSD 3-Clause"


def make_zip(files: dict[str, bytes]) -> bytes:
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        for name, data in files.items():
            z.writestr(name, data)
    return buf.getvalue()


ARCHIVE = make_zip(
    {
        "lite/id1/pak0.pak": PAK0,
        "lite/id1/pak1.pak": PAK1,
        "lite/id1/docs/COPYING": LICENSE,
        "lite/id1/music/track02.ogg": b"not extracted",
    }
)


def entry() -> dict:
    return {
        "id": "lq",
        "url": "https://github.com/o/r/releases/download/v1/lite.zip",
        "sha256": sha(ARCHIVE),
        "strip": "lite/",
        "game_dir": "id1",
        "members": {
            "lite/id1/pak0.pak": sha(PAK0),
            "lite/id1/pak1.pak": sha(PAK1),
            "lite/id1/docs/COPYING": sha(LICENSE),
        },
        "stand_ins": {
            "sound/boss2/death.wav": "sound/boss1/death.wav",
            "sound/player/slimbrn2.wav": "sound/player/slmbrn2.wav",
        },
        "covers": ["rr"],
    }


class Downloads:
    """A stand-in for assets.download that serves fixed bytes and counts calls."""

    def __init__(self, data: bytes = ARCHIVE):
        self.data = data
        self.calls = 0

    def __call__(self, url: str, out: Path) -> None:
        self.calls += 1
        out.write_bytes(self.data)


class TempDir(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.dest = Path(tmp.name)


class Fetch(TempDir):
    def test_fetch_extracts_pinned_members_only(self):
        get = Downloads()
        out = assets.extract(entry(), assets.fetch_archive(entry(), self.dest, get), self.dest)
        self.assertEqual(out, self.dest / "lq")
        self.assertEqual((out / "id1/pak0.pak").read_bytes(), PAK0)
        self.assertEqual((out / "id1/docs/COPYING").read_bytes(), LICENSE)
        self.assertFalse((out / "id1/music").exists())
        self.assertFalse((out / "lite").exists())

    def test_cached_archive_is_not_downloaded_again(self):
        get = Downloads()
        assets.fetch_archive(entry(), self.dest, get)
        assets.fetch_archive(entry(), self.dest, get)
        self.assertEqual(get.calls, 1)

    def test_corrupt_cache_is_downloaded_again(self):
        get = Downloads()
        archive = assets.fetch_archive(entry(), self.dest, get)
        archive.write_bytes(b"truncated")
        assets.fetch_archive(entry(), self.dest, get)
        self.assertEqual(get.calls, 2)
        self.assertEqual(archive.read_bytes(), ARCHIVE)

    def test_archive_checksum_mismatch(self):
        with self.assertRaisesRegex(assets.AssetError, "manifest says"):
            assets.fetch_archive(entry(), self.dest, Downloads(b"something else"))
        self.assertEqual(list((self.dest / "downloads").iterdir()), [])

    def test_download_failure(self):
        def boom(url: str, out: Path) -> None:
            out.write_bytes(b"partial")
            raise OSError("404 Not Found")

        with self.assertRaisesRegex(assets.AssetError, "failed: 404 Not Found"):
            assets.fetch_archive(entry(), self.dest, boom)
        self.assertEqual(list((self.dest / "downloads").iterdir()), [])

    def test_member_checksum_mismatch(self):
        e = entry()
        e["members"]["lite/id1/pak0.pak"] = "0" * 64
        archive = assets.fetch_archive(e, self.dest, Downloads())
        with self.assertRaisesRegex(assets.AssetError, "lite/id1/pak0.pak: sha256"):
            assets.extract(e, archive, self.dest)

    def test_missing_member(self):
        e = entry()
        e["members"]["lite/id1/pak2.pak"] = "0" * 64
        archive = assets.fetch_archive(e, self.dest, Downloads())
        with self.assertRaisesRegex(assets.AssetError, "no member lite/id1/pak2.pak"):
            assets.extract(e, archive, self.dest)

    def test_unsafe_member_is_refused(self):
        e = entry()
        e["members"] = {"lite/../../evil": sha(b"x")}
        archive = assets.fetch_archive(e, self.dest, Downloads())
        with self.assertRaisesRegex(assets.AssetError, "not a safe path"):
            assets.extract(e, archive, self.dest)

    def test_not_a_zip(self):
        bad = self.dest / "bad.zip"
        bad.write_bytes(b"not a zip")
        with self.assertRaisesRegex(assets.AssetError, "bad.zip"):
            assets.extract(entry(), bad, self.dest)

    def test_cmd_fetch_installs_stand_ins(self):
        m = {"asset": [entry(), {"id": "id1-paks", "use": "never"}]}
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(assets.cmd_fetch(m, self.dest, Downloads()), [])
        game = self.dest / "lq/id1"
        self.assertEqual((game / "sound/boss2/death.wav").read_bytes(), b"RIFF-boss1")
        self.assertEqual((game / "sound/player/slimbrn2.wav").read_bytes(), b"RIFF-slime")
        self.assertFalse((self.dest / "id1-paks").exists())

    def test_cmd_fetch_reports_missing_scenario_files(self):
        pak = make_pak({"maps/start.bsp": b"bsp"})
        archive = make_zip({"a/id1/pak0.pak": pak})
        e = entry() | {
            "strip": "a/",
            "sha256": sha(archive),
            "members": {"a/id1/pak0.pak": sha(pak)},
            "stand_ins": {},
        }
        with contextlib.redirect_stdout(io.StringIO()):
            errs = assets.cmd_fetch({"asset": [e]}, self.dest, Downloads(archive))
        out = "\n".join(errs)
        self.assertIn("asset lq: missing gfx/palette.lmp", out)
        self.assertIn("asset lq: no model (progs/*.mdl)", out)
        self.assertIn("asset lq: no sound (sound/*.wav)", out)

    def test_cmd_fetch_stand_in_must_exist(self):
        e = entry()
        e["stand_ins"] = {"sound/x.wav": "sound/nosuch.wav"}
        with contextlib.redirect_stdout(io.StringIO()):
            errs = assets.cmd_fetch({"asset": [e]}, self.dest, Downloads())
        self.assertEqual(
            errs, ["asset lq: stand-in sound/nosuch.wav for sound/x.wav is not in the paks"]
        )


class Pak(TempDir):
    def write(self, data: bytes, name: str = "pak0.pak") -> Path:
        p = self.dest / name
        p.write_bytes(data)
        return p

    def test_index(self):
        idx = assets.pak_index(self.write(make_pak({"a.wav": b"12345", "b/c.mdl": b"x"})))
        self.assertEqual(idx, {"a.wav": (12, 5), "b/c.mdl": (17, 1)})

    def test_later_pak_wins(self):
        self.write(make_pak({"a": b"old", "b": b"b"}), "pak0.pak")
        p1 = self.write(make_pak({"a": b"new"}), "pak1.pak")
        self.write(make_pak({"z": b"skipped: pak2 is missing"}), "pak3.pak")
        files = assets.pak_files(self.dest)
        self.assertEqual(set(files), {"a", "b"})
        self.assertEqual(files["a"][0], p1)

    def test_malformed(self):
        cases = {
            b"PAC": "truncated pak header",
            b"WAD2" + bytes(8): "not a PACK file",
            struct.pack("<4sii", b"PACK", 12, 64): "directory out of range",
            struct.pack("<4sii", b"PACK", 12, 10) + bytes(10): "directory out of range",
            struct.pack("<4sii", b"PACK", 12, 64) + struct.pack("<56sii", b"a", 0, 999): "entry 0",
        }
        for data, msg in cases.items():
            with self.assertRaisesRegex(assets.AssetError, msg):
                assets.pak_index(self.write(data))

    def test_scenario(self):
        self.assertEqual(assets.check_scenario(set(SCENARIO)), [])
        names = set(SCENARIO) - {"gfx.wad", "maps/start.bsp"}
        self.assertEqual(
            assets.check_scenario(names),
            ["missing gfx.wad", "missing maps/start.bsp", "no map (maps/*.bsp)"],
        )


QC = """
// precache_model ("progs/commented.mdl");
/* precache_sound ("misc/commented.wav");
   still a comment */
void() worldspawn =
{
	precache_model ("progs/player.mdl");
	PRECACHE_SOUND("boss1/death.wav");  // a trailing comment
	precache_sound2 ("boss2/death.wav");
	precache_model2 ("progs/missing.mdl");
	precache_model (self.mdl);
	bprint ("precache_sound (\\"fake.wav\\") in a string");
	precache_sounds ();
};
"""


class Precache(unittest.TestCase):
    def calls(self):
        return assets.precache_calls("w.qc", QC)

    def test_parse(self):
        got = [(c.kind, c.name, c.line) for c in self.calls()]
        self.assertEqual(
            got,
            [
                ("model", "progs/player.mdl", 7),
                ("sound", "boss1/death.wav", 8),
                ("sound", "boss2/death.wav", 9),
                ("model", "progs/missing.mdl", 10),
                ("model", None, 11),
            ],
        )

    def test_paths(self):
        self.assertEqual(
            [c.path() for c in self.calls()],
            [
                "progs/player.mdl",
                "sound/boss1/death.wav",
                "sound/boss2/death.wav",
                "progs/missing.mdl",
                None,
            ],
        )

    def test_coverage(self):
        names = {"progs/player.mdl", "sound/boss1/death.wav"}
        stand_ins = {
            "sound/boss2/death.wav": "sound/boss1/death.wav",
            "progs/missing.mdl": "progs/nosuch.mdl",
        }
        missing, standing_in, dynamic = assets.coverage(self.calls(), names, stand_ins)
        self.assertEqual(missing, ["progs/missing.mdl"])
        self.assertEqual(standing_in, ["sound/boss2/death.wav"])
        self.assertEqual([c.arg for c in dynamic], ["self.mdl"])


def make_tarball(files: dict[str, bytes]) -> bytes:
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tar:
        for name, data in files.items():
            info = tarfile.TarInfo(f"r-{COMMIT}/{name}")
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
    return buf.getvalue()


DEFS = b"void(string s) precache_model = #20;\n"


def manifest(defs_sum: str = sha(DEFS)) -> dict:
    return {
        "source": [
            {
                "id": "s",
                "url": "https://github.com/o/r",
                "commit": COMMIT,
                "checksums": {"qc/defs.qc": defs_sum, "COPYING": "0" * 64},
            }
        ],
        "codebase": [{"id": "rr", "source": "s", "path": "qc", "use": "vendor"}],
        "asset": [entry()],
    }


TARBALL = make_tarball(
    {
        "COPYING": b"GPL",
        "qc/defs.qc": DEFS,
        "qc/world.qc": QC.encode(),
        "qc/progs.src": b"defs.qc",
        "qcother/x.qc": b'precache_model ("progs/elsewhere.mdl");',
    }
)


class Sources(TempDir):
    def test_reads_codebase_qc_files_at_pinned_commit(self):
        urls = []

        def get(url):
            urls.append(url)
            return TARBALL

        files = assets.codebase_sources(manifest(), "rr", get)
        self.assertEqual(urls, [f"https://github.com/o/r/archive/{COMMIT}.tar.gz"])
        self.assertEqual(sorted(files), ["qc/defs.qc", "qc/world.qc"])

    def test_cmd_codebase_writes_the_tree(self):
        out = self.dest / "cb"
        with contextlib.redirect_stdout(io.StringIO()):
            errs = assets.cmd_codebase(manifest(), "rr", out, lambda u: TARBALL)
        self.assertEqual(errs, [])
        written = sorted(str(p.relative_to(out)) for p in out.rglob("*") if p.is_file())
        self.assertEqual(written, ["defs.qc", "progs.src", "world.qc"])
        self.assertEqual((out / "defs.qc").read_bytes(), DEFS)

    def test_cmd_codebase_errors(self):
        errs = assets.cmd_codebase(manifest(), "nosuch", self.dest, lambda u: TARBALL)
        self.assertEqual(errs, ["codebase nosuch: no usable codebase nosuch"])
        evil = make_tarball({"qc/defs.qc": DEFS, "qc/../../evil.qc": b"x"})
        errs = assets.cmd_codebase(manifest(), "rr", self.dest / "cb", lambda u: evil)
        self.assertEqual(len(errs), 1)
        self.assertIn("is outside", errs[0])

    def test_pinned_checksum_mismatch(self):
        with self.assertRaisesRegex(assets.AssetError, "s:qc/defs.qc: sha256"):
            assets.codebase_sources(manifest("1" * 64), "rr", lambda u: TARBALL)

    def test_unknown_or_never_codebase(self):
        m = manifest()
        with self.assertRaisesRegex(assets.AssetError, "no usable codebase nosuch"):
            assets.codebase_sources(m, "nosuch", lambda u: TARBALL)
        m["codebase"][0]["use"] = "never"
        with self.assertRaisesRegex(assets.AssetError, "no usable codebase rr"):
            assets.codebase_sources(m, "rr", lambda u: TARBALL)

    def test_download_failure(self):
        def boom(url):
            raise OSError("timed out")

        with self.assertRaisesRegex(assets.AssetError, "failed: timed out"):
            assets.codebase_sources(manifest(), "rr", boom)

    def test_cmd_precache(self):
        m = manifest()
        with contextlib.redirect_stdout(io.StringIO()):
            assets.cmd_fetch(m, self.dest, Downloads())
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            errs = assets.cmd_precache(m, self.dest, lambda u: TARBALL)
        self.assertEqual(errs, ["asset lq: rr precaches progs/missing.mdl, which is missing"])
        self.assertIn("1 by stand-in, 1 missing", out.getvalue())
        self.assertIn("stand-in: sound/boss2/death.wav -> sound/boss1/death.wav", out.getvalue())
        self.assertIn("not a literal: qc/world.qc:11 precache_model(self.mdl)", out.getvalue())

    def test_cmd_precache_before_fetch(self):
        errs = assets.cmd_precache(manifest(), self.dest, lambda u: TARBALL)
        self.assertEqual(len(errs), 1)
        self.assertIn("run fetch first", errs[0])


class Dest(unittest.TestCase):
    def test_env_override(self):
        with mock.patch.dict(os.environ, {"QS_TEST_ASSETS": "/x/assets"}):
            self.assertEqual(assets.default_dest(), Path("/x/assets"))

    def test_xdg_cache(self):
        env = {"XDG_CACHE_HOME": "/c"}
        with mock.patch.dict(os.environ, env, clear=True):
            self.assertEqual(assets.default_dest(), Path("/c/quakecstudio/test-assets"))

    def test_outside_the_source_tree(self):
        # Only the overrides are removed: Windows finds the home directory
        # through USERPROFILE, so clearing the whole environment breaks Path.home().
        with mock.patch.dict(os.environ):
            os.environ.pop("QS_TEST_ASSETS", None)
            os.environ.pop("XDG_CACHE_HOME", None)
            self.assertFalse(assets.default_dest().is_relative_to(verify.REPO))


if __name__ == "__main__":
    unittest.main()
