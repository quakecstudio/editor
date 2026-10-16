# Conformance corpus

[`manifest.toml`](manifest.toml) lists the QuakeC codebases and maps the differential and conformance tests run against (FND-9, MAP-19, OPEN-4): where each one comes from, the commit it is pinned to, its license and what we may do with it. Its `[[asset]]` entries are the open game data CI runs engine scenarios with (QA-4); [`docs/test-assets.md`](../docs/test-assets.md) has their license review and [`assets.py`](assets.py) fetches them.

Nothing in the manifest is committed to this repository. Tools fetch each entry at its pinned commit and check it against the recorded SHA-256 (LIC-7). `cargo xtask corpus fetch|build|verify` ([P1-14](../plan/backlog/p1.yaml)) does the fetching; until it exists, [`verify.py`](verify.py) checks the manifest.

## Policy

- **QuakeC 1.06 is never fetched, built, vendored or bundled.** id released it with no license (LIC-5). It is in the manifest's `never` list, it has no source, and `blocked_urls` names the repository that carries it so no entry can point there. id's QuakeC 1.01 (GPL, in Quake-Tools) and the rerelease QuakeC stand in for it.
- **id's game data (pak0/pak1) never enters the repository or CI** (LIC-4). It is the `id1-paks` entry in the `never` list, `verify.py` fails on any tracked `.pak` file, and `assets.py` extracts only archive members pinned by SHA-256.
- **Corpus files are fetched, not committed.** `verify.py` fails on any tracked Quake content file (`.qc`, `.src`, `.map`, `.bsp`, `.wad` and so on) outside the directories listed in the manifest's `fixture_dirs`, and on any tracked file that is a byte-for-byte copy of a pinned corpus file. An issue that adds test fixtures lists their directory in `fixture_dirs` in the same PR; `crates/qc-syntax/src/tests/` holds the parser's hand-written QuakeC sample.
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

`[[asset]]` is a test asset: a release archive of game data. Ids are unique across codebases, maps and assets, and `license`, `use`, `evidence`, `reason` and `notes` mean the same as above, except that an asset is never `vendor`.

| Field | Meaning |
| --- | --- |
| `source` | The `[[source]]` the release belongs to; `evidence` names files pinned there |
| `url`, `sha256` | A `.zip` from that repository's GitHub releases, and its SHA-256 |
| `members` | The archive paths to extract, each with its SHA-256. The `evidence` files must be among them, byte-identical to the pinned commit's |
| `strip`, `game_dir` | `strip` is removed from the front of each member path; `game_dir` (such as `id1`) is where the pak files land |
| `covers` | Codebases whose literal precache calls the paks must satisfy |
| `stand_ins` | A file the QuakeC precaches but the paks lack, mapped to a file in the paks that stands in for it |

## The corpus

**id Software:** QuakeC 1.01 from Quake-Tools, and the six rerelease codebases (base campaign, Scourge of Armagon, Dissolution of Eternity, Dimension of the Machine, Capture the Flag, Dawn of the Machine).

**Mods (14):** Copper, Alkaline, Arcane Dimensions, progs_dump, LibreQuake, Re:Mobilize, Conduit, Quake Reignited, Nuclide, FreeCS, Nazi Zombies: Portable, CRMod 7, Starts with a Bang and Xonotic. Together they cover vanilla QuakeC, the FTEQCC dialect with CSQC (Nuclide, FreeCS, Copper, Arcane Dimensions) and GMQCC (Xonotic).

**Maps (26):** 20 LibreQuake maps (BSD-3-Clause), four ericw-tools test maps (GPL-2.0-or-later) and two progs_dump maps (no commercial use, so `fetch-at-test`).

**Excluded:** QuakeC 1.06 (no license) and Quoth, whose release has no QuakeC source and whose readme only permits redistributing the unmodified zip. The ericw-tools test maps derived from id's Quake and Quake II map sources (`E1M1-edited-ents.map`, `base1-test.map`, `base1leak.map`) are left out because those files state no license of their own.

**Test assets:** LibreQuake's `lite` release (BSD-3-Clause, GPL-2.0-or-later progs), `fetch-at-test`. id's `pak0.pak` and `pak1.pak` are `never`.

## Checking

Needs Python 3.11 or later (for `tomllib`):

```bash
python3.11 corpus/verify.py            # offline: manifest rules and the repository scan
python3.11 corpus/verify.py --online   # also downloads every pinned file and compares SHA-256
python3.11 corpus/assets.py fetch      # download, checksum and extract the test assets
python3.11 corpus/assets.py precache   # check they cover the rerelease's precaches
python3.11 corpus/language_coverage.py # check docs/spec/language.md covers every construct in id 1.01
python3.11 corpus/assets.py codebase   # write id 1.01 to <cache>/codebase/id-1.01
cargo xtask lex-corpus <cache>/codebase/id-1.01  # lex it and preprocess progs.src with no errors
cargo xtask parse-corpus --vanilla --deletions <cache>/codebase/id-1.01  # parse it, check recovery
cargo xtask parse-corpus <cache>/codebase/<id>...  # every file round-trips through the tree
(cd corpus && python3.11 -m unittest test_verify.py test_assets.py test_language_coverage.py)
```
