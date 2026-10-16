# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests for corpus/verify.py. Run: cd corpus && python3.11 -m unittest test_verify.py"""

import copy
import hashlib
import unittest

import verify

H1 = "1" * 64
H2 = "2" * 64
COMMIT = "a" * 40


def base() -> dict:
    """A minimal manifest that passes every offline check."""
    m = {
        "schema": 1,
        "never": ["quakec-1.06"],
        "blocked_urls": ["https://github.com/maddes-b/QuakeC-releases"],
        "source": [
            {
                "id": "src",
                "url": "https://github.com/o/r",
                "commit": COMMIT,
                "checksums": {"LICENSE": H1} | {f"m{i}.map": f"{i:064x}" for i in range(20)},
            }
        ],
        "codebase": [
            {
                "id": "quakec-1.06",
                "kind": "id",
                "license": "NONE",
                "use": "never",
                "reason": "no license",
            },
        ],
        "map": [],
    }
    for cid in ("id-1.01", "rerelease"):
        m["codebase"].append(_cb(cid, "id"))
    for i in range(10):
        m["codebase"].append(_cb(f"mod{i}", "mod"))
    for i in range(20):
        m["map"].append(
            {
                "id": f"map{i}",
                "source": "src",
                "path": f"m{i}.map",
                "license": "BSD-3-Clause",
                "use": "vendor",
                "evidence": ["LICENSE"],
            }
        )
    return m


def _cb(cid: str, kind: str) -> dict:
    return {
        "id": cid,
        "kind": kind,
        "source": "src",
        "path": ".",
        "license": "GPL-2.0-or-later",
        "use": "vendor",
        "evidence": ["LICENSE"],
    }


def cb(m: dict, cid: str) -> dict:
    return next(e for e in m["codebase"] if e["id"] == cid)


class RealManifest(unittest.TestCase):
    def test_manifest_passes(self):
        self.assertEqual(verify.check(verify.load()), [])

    def test_repo_tracks_no_corpus_files(self):
        m = verify.load()
        errs = verify.check_repo(
            m, verify.tracked_files(), lambda f: (verify.REPO / f).read_bytes()
        )
        self.assertEqual(errs, [])

    def test_quakec_106_is_never(self):
        m = verify.load()
        self.assertIn("quakec-1.06", m["never"])
        entry = cb(m, "quakec-1.06")
        self.assertEqual(entry["use"], "never")
        self.assertNotIn("source", entry)

    def test_every_usable_entry_has_pinned_evidence(self):
        m = verify.load()
        for table in ("codebase", "map"):
            for e in m[table]:
                if e["use"] != "never":
                    self.assertTrue(e["evidence"], e["id"])


class Check(unittest.TestCase):
    def errs(self, m: dict) -> str:
        return "\n".join(verify.check(m))

    def test_base_passes(self):
        self.assertEqual(verify.check(base()), [])

    def test_schema(self):
        m = base()
        m["schema"] = 2
        self.assertIn("schema must be 1", self.errs(m))

    def test_source_fields(self):
        m = base()
        s = m["source"][0]
        s["url"] = "http://example.com/x"
        s["commit"] = "main"
        s["checksums"]["LICENSE"] = "abc"
        out = self.errs(m)
        self.assertIn("url must be", out)
        self.assertIn("full 40-character SHA", out)
        self.assertIn("LICENSE: bad sha256", out)

    def test_source_without_checksums(self):
        m = base()
        m["source"].append({"id": "empty", "url": "https://github.com/o/e", "commit": COMMIT})
        self.assertIn("source empty: no pinned checksums", self.errs(m))

    def test_duplicate_and_bad_ids(self):
        m = base()
        m["source"].append(copy.deepcopy(m["source"][0]))
        m["codebase"].append(_cb("mod0", "mod"))
        m["codebase"].append(_cb("Bad_ID", "mod"))
        out = self.errs(m)
        self.assertIn("source src: duplicate id", out)
        self.assertIn("codebase mod0: duplicate id", out)
        self.assertIn("codebase Bad_ID: bad id", out)

    def test_blocked_url(self):
        m = base()
        m["source"].append(
            {
                "id": "q106",
                "url": "https://github.com/maddes-b/QuakeC-releases",
                "commit": COMMIT,
                "checksums": {"x": H1},
            }
        )
        self.assertIn("is blocked", self.errs(m))

    def test_license_required(self):
        m = base()
        cb(m, "mod0")["license"] = ""
        cb(m, "mod1")["license"] = "GPL v2"
        out = self.errs(m)
        self.assertIn("codebase mod0: license must be", out)
        self.assertIn("codebase mod1: license must be", out)

    def test_use_and_kind(self):
        m = base()
        cb(m, "mod0")["use"] = "bundle"
        cb(m, "mod1")["kind"] = "map"
        out = self.errs(m)
        self.assertIn("mod0: use must be one of", out)
        self.assertIn("mod1: kind must be one of", out)

    def test_vendor_needs_compatible_license(self):
        for lic in ("GPL-2.0-only", "LicenseRef-no-commercial", "NONE"):
            m = base()
            cb(m, "mod0")["license"] = lic
            self.assertIn("cannot be vendor", self.errs(m), lic)
            cb(m, "mod0")["use"] = "fetch-at-test"
            self.assertEqual(verify.check(m), [], lic)

    def test_evidence_required_and_pinned(self):
        m = base()
        cb(m, "mod0")["evidence"] = []
        cb(m, "mod1")["evidence"] = ["README.md"]
        cb(m, "mod2")["evidence"] = ["nosuch:LICENSE"]
        out = self.errs(m)
        self.assertIn("mod0: no license evidence", out)
        self.assertIn("mod1: evidence README.md has no pinned checksum", out)
        self.assertIn("mod2: evidence nosuch:LICENSE: unknown source", out)

    def test_evidence_in_other_source(self):
        m = base()
        m["source"].append(
            {
                "id": "other",
                "url": "https://gitlab.com/o/x",
                "commit": COMMIT,
                "checksums": {"COPYING": H2},
            }
        )
        cb(m, "mod0")["evidence"] = ["other:COPYING"]
        self.assertEqual(verify.check(m), [])

    def test_unknown_source_and_missing_path(self):
        m = base()
        cb(m, "mod0")["source"] = "nosuch"
        del cb(m, "mod1")["path"]
        out = self.errs(m)
        self.assertIn("mod0: unknown source 'nosuch'", out)
        self.assertIn("mod1: missing path", out)

    def test_map_must_be_pinned_map_file(self):
        m = base()
        m["map"][0]["path"] = "m0.bsp"
        m["map"][1]["path"] = "unpinned.map"
        out = self.errs(m)
        self.assertIn("map map0: path must be a .map file", out)
        self.assertIn("map map1: unpinned.map has no pinned checksum", out)

    def test_never_entries(self):
        m = base()
        e = cb(m, "quakec-1.06")
        e["source"] = "src"
        del e["reason"]
        out = self.errs(m)
        self.assertIn("must not name a source", out)
        self.assertIn("needs a reason", out)

    def test_never_list_consistency(self):
        m = base()
        m["never"] = ["quakec-1.06", "ghost"]
        cb(m, "mod0").update(use="never", reason="r")
        del cb(m, "mod0")["source"], cb(m, "mod0")["path"]
        out = self.errs(m)
        self.assertIn("never list: ghost has no entry", out)
        self.assertIn("mod0: use = never but missing from the never list", out)

    def test_quakec_106_must_be_never(self):
        m = base()
        m["never"] = []
        m["codebase"] = [e for e in m["codebase"] if e["id"] != "quakec-1.06"]
        self.assertIn("must include quakec-1.06", self.errs(m))

    def test_required_codebases_and_minimums(self):
        m = base()
        m["codebase"] = [e for e in m["codebase"] if e["id"] not in ("rerelease", "mod9")]
        m["map"].pop()
        out = self.errs(m)
        self.assertIn("missing required codebase rerelease", out)
        self.assertIn("9 usable mods; need at least 10", out)
        self.assertIn("19 usable maps; need at least 20", out)

    def test_never_entries_do_not_count(self):
        m = base()
        cb(m, "id-1.01").update(use="never", reason="r")
        del cb(m, "id-1.01")["source"], cb(m, "id-1.01")["path"]
        m["never"].append("id-1.01")
        self.assertIn("missing required codebase id-1.01", self.errs(m))


class CheckRepo(unittest.TestCase):
    def run_check(self, files: dict, fixture_dirs=()):
        m = base()
        m["fixture_dirs"] = list(fixture_dirs)
        return "\n".join(verify.check_repo(m, files, files.__getitem__))

    def test_clean(self):
        self.assertEqual(self.run_check({"README.md": b"x", "corpus/manifest.toml": b"y"}), "")

    def test_pak_never(self):
        self.assertIn(
            "pak files", self.run_check({"tests/fixtures/pak0.pak": b"PACK"}, ["tests/fixtures"])
        )

    def test_content_outside_fixture_dirs(self):
        out = self.run_check({"src/defs.qc": b"float x;"})
        self.assertIn("src/defs.qc: Quake content outside", out)
        self.assertEqual(
            self.run_check({"tests/fixtures/a.qc": b"float x;"}, ["tests/fixtures"]), ""
        )

    def test_copy_of_corpus_file(self):
        data = b"// a corpus map"
        m = base()
        m["source"][0]["checksums"]["m0.map"] = hashlib.sha256(data).hexdigest()
        m["fixture_dirs"] = ["tests/fixtures"]
        errs = verify.check_repo(m, ["tests/fixtures/x.map"], lambda f: data)
        self.assertEqual(errs, ["tests/fixtures/x.map: is a copy of corpus file src:m0.map"])

    def test_license_text_is_not_flagged(self):
        data = b"GNU GENERAL PUBLIC LICENSE"
        m = base()
        m["source"][0]["checksums"]["LICENSE"] = hashlib.sha256(data).hexdigest()
        self.assertEqual(verify.check_repo(m, ["LICENSE"], lambda f: data), [])


class Online(unittest.TestCase):
    def test_raw_urls(self):
        self.assertEqual(
            verify.raw_url("https://github.com/o/r", COMMIT, "a b/c.qc"),
            f"https://raw.githubusercontent.com/o/r/{COMMIT}/a%20b/c.qc",
        )
        self.assertEqual(
            verify.raw_url("https://gitlab.com/o/r", COMMIT, "qcsrc/Makefile"),
            f"https://gitlab.com/api/v4/projects/o%2Fr/repository/files/qcsrc%2FMakefile/raw?ref={COMMIT}",
        )
        with self.assertRaises(ValueError):
            verify.raw_url("https://example.com/o/r", COMMIT, "x")

    def manifest(self, data: bytes) -> dict:
        return {
            "source": [
                {
                    "id": "s",
                    "url": "https://github.com/o/r",
                    "commit": COMMIT,
                    "checksums": {"LICENSE": hashlib.sha256(data).hexdigest()},
                }
            ]
        }

    def test_match(self):
        self.assertEqual(verify.check_online(self.manifest(b"ok"), lambda u: b"ok"), [])

    def test_mismatch(self):
        errs = verify.check_online(self.manifest(b"ok"), lambda u: b"changed")
        self.assertEqual(len(errs), 1)
        self.assertIn("source s: LICENSE: sha256", errs[0])

    def test_fetch_failure(self):
        def boom(url: str) -> bytes:
            raise OSError("404 Not Found")

        errs = verify.check_online(self.manifest(b"ok"), boom)
        self.assertEqual(errs, ["source s: LICENSE: fetch failed: 404 Not Found"])


if __name__ == "__main__":
    unittest.main()
