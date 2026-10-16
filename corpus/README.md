# Conformance corpus

[`manifest.toml`](manifest.toml) lists the QuakeC codebases and maps the differential and conformance tests run against (FND-9, MAP-19, OPEN-4): where each one comes from, the commit it is pinned to, its license and what we may do with it.

Nothing in the manifest is committed to this repository. Tools fetch each entry at its pinned commit and check it against the recorded SHA-256 (LIC-7). `cargo xtask corpus fetch|build|verify` ([P1-14](../plan/backlog/p1.yaml)) does the fetching; until it exists, [`verify.py`](verify.py) checks the manifest.

## Policy

- **QuakeC 1.06 is never fetched, built, vendored or bundled.** id released it with no license (LIC-5). It is in the manifest's `never` list, it has no source, and `blocked_urls` names the repository that carries it so no entry can point there. id's QuakeC 1.01 (GPL, in Quake-Tools) and the rerelease QuakeC stand in for it.
- **Quake game data (pak0/pak1) never enters the repository.** `verify.py` fails on any tracked `.pak` file.
- **Corpus files are fetched, not committed.** `verify.py` fails on any tracked Quake content file (`.qc`, `.src`, `.map`, `.bsp`, `.wad` and so on) outside the directories listed in the manifest's `fixture_dirs`, and on any tracked file that is a byte-for-byte copy of a pinned corpus file. `fixture_dirs` is empty: a later issue that adds test fixtures lists its directory there in the same PR.
- **Licenses come from the item's own files.** Every entry names the files its license was read from (`evidence`), and each of those files is pinned by SHA-256, so a license change upstream fails `verify.py --online`. When an item ships only the GPL text with no "or any later version" grant from its authors, it is recorded as `-only`: the conservative reading, which never claims more rights than the files give.

## Fields

`[[source]]` is a repository at a pinned commit:

| Field | Meaning |
| --- | --- |
| `id` | Short name the entries refer to |
| `url` | `https://github.com/owner/repo` or `https://gitlab.com/owner/repo` |
| `commit` | Full 40-character commit SHA |
| `[source.checksums]` | SHA-256 of each pinned file in that commit: license evidence and every map |

`[[codebase]]` and `[[map]]` are the corpus entries:

| Field | Meaning |
| --- | --- |
| `id` | Unique across codebases and maps |
| `name`, `kind` | Codebases only. `kind` is `id` (id Software's code) or `mod` |
| `source`, `path` | Where it lives: a directory for a codebase, a `.map` file for a map (whose SHA-256 is in the source's checksums) |
| `license` | SPDX identifier, `LicenseRef-*` for a custom notice, or `NONE` |
| `use` | See below |
| `evidence` | Files the license was read from: `path` in the entry's source, or `source-id:path` in another |
| `reason` | Required for `use = "never"` |
| `notes` | What the evidence says, in a sentence |

`use` is what the license permits:

| `use` | Meaning |
| --- | --- |
| `vendor` | GPL-3.0-or-later compatible. Fetched like everything else; small excerpts may be committed as test fixtures with the notice kept |
| `fetch-at-test` | Fetched by checksum for local and CI tests and never committed. For licenses that cannot be combined with GPL-3.0-or-later, such as GPL-2.0-only or no-commercial-use notices |
| `never` | Never fetched or committed |

## The corpus

**id Software:** QuakeC 1.01 from Quake-Tools, and the six rerelease codebases (base campaign, Scourge of Armagon, Dissolution of Eternity, Dimension of the Machine, Capture the Flag, Dawn of the Machine).

**Mods (14):** Copper, Alkaline, Arcane Dimensions, progs_dump, LibreQuake, Re:Mobilize, Conduit, Quake Reignited, Nuclide, FreeCS, Nazi Zombies: Portable, CRMod 7, Starts with a Bang and Xonotic. Together they cover vanilla QuakeC, the FTEQCC dialect with CSQC (Nuclide, FreeCS, Copper, Arcane Dimensions) and GMQCC (Xonotic).

**Maps (26):** 20 LibreQuake maps (BSD-3-Clause), four ericw-tools test maps (GPL-2.0-or-later) and two progs_dump maps (no commercial use, so `fetch-at-test`).

**Excluded:** QuakeC 1.06 (no license) and Quoth, whose release has no QuakeC source and whose readme only permits redistributing the unmodified zip. The ericw-tools test maps derived from id's Quake and Quake II map sources (`E1M1-edited-ents.map`, `base1-test.map`, `base1leak.map`) are left out because those files state no license of their own.

## Checking

Needs Python 3.11 or later (for `tomllib`):

```bash
python3.11 corpus/verify.py            # offline: manifest rules and the repository scan
python3.11 corpus/verify.py --online   # also downloads every pinned file and compares SHA-256
(cd corpus && python3.11 -m unittest test_verify.py)
```
