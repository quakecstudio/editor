# Conformance corpus

[`manifest.toml`](manifest.toml) lists the QuakeC codebases and maps the differential and conformance tests run against (FND-9, MAP-19, OPEN-4): where each one comes from, the commit it is pinned to, its license and what we may do with it. Its `[[asset]]` entries are the open game data CI runs engine scenarios with (QA-4); [`docs/test-assets.md`](../docs/test-assets.md) has their license review and `cargo xtask assets fetch` fetches them.

Nothing in the manifest is committed to this repository. Tools fetch each entry at its pinned commit and check it against the recorded SHA-256 (LIC-7). `cargo xtask corpus fetch` does the fetching, `cargo xtask corpus build` compiles what an oracle compiler builds, and `cargo xtask corpus verify` checks the manifest ([`xtask/src/corpus.rs`](../xtask/src/corpus.rs)).

## Policy

- **QuakeC 1.06 is never fetched, built, vendored or bundled.** id released it with no license (LIC-5). It is in the manifest's `never` list, it has no source, and `blocked_urls` names the repository that carries it so no entry can point there. id's QuakeC 1.01 (GPL, in Quake-Tools) and the rerelease QuakeC stand in for it.
- **id's game data (pak0/pak1) never enters the repository or CI** (LIC-4). It is the `id1-paks` entry in the `never` list, `cargo xtask corpus verify` fails on any tracked `.pak` file, and `cargo xtask assets fetch` extracts only archive members pinned by SHA-256.
- **Corpus files are fetched, not committed.** `cargo xtask corpus verify` fails on any tracked Quake content file (`.qc`, `.src`, `.map`, `.bsp`, `.wad` and so on) outside the directories listed in the manifest's `fixture_dirs`, and on any tracked file that is a byte-for-byte copy of a pinned corpus file. An issue that adds test fixtures lists their directory in `fixture_dirs` in the same PR; `crates/qc-syntax/src/tests/` holds the parser's hand-written QuakeC sample.
- **Licenses come from the item's own files.** Every entry names the files its license was read from (`evidence`), and each of those files is pinned by SHA-256, so a license change upstream fails `cargo xtask corpus verify --online`. When an item ships only the GPL text with no "or any later version" grant from its authors, it is recorded as `-only`: the conservative reading, which never claims more rights than the files give.

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
| `project` | Codebases only, optional. A `quakec.toml` in this repository, relative to this directory, that `corpus fetch` writes into the codebase: for a codebase whose build gives its modules defines no `progs.src` states, such as Xonotic's (`projects/xonotic/quakec.toml`) |

`[[tree]]` lays codebases out together as the directory tree their sources expect, such as FreeCS, whose `progs.src` files reach into the Nuclide SDK and Rad-Therapy beside it. `corpus fetch <tree>` writes each codebase to its directory under `<cache>/tree/<id>/`.

| Field | Meaning |
| --- | --- |
| `id` | Unique across codebases, maps, assets and trees |
| `name`, `notes` | For people: what the tree is, and where its layout comes from |
| `[tree.dirs]` | The usable codebase written to each directory of the tree; no directory is inside another |

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

**Mods (15):** Copper, Alkaline, Arcane Dimensions, progs_dump, LibreQuake, Re:Mobilize, Conduit, Quake Reignited, Nuclide, FreeCS, Rad-Therapy (FreeHL, the Half-Life game FreeCS builds against), Nazi Zombies: Portable, CRMod 7, Starts with a Bang and Xonotic. Together they cover vanilla QuakeC, the FTEQCC dialect with CSQC (Nuclide, FreeCS, Copper, Arcane Dimensions) and GMQCC (Xonotic).

**Trees:** `freecs-tree`, FreeCS at `cstrike/src` in the Nuclide SDK's `src`, with Rad-Therapy at `valve/src`, as FreeCS's README builds it. It opens with no error only on a case-insensitive file system, such as macOS's default (CI runs it there): FreeCS's `shared/defs.h` includes `animations.h` and the file is `Animations.h`.

**Maps (26):** 20 LibreQuake maps (BSD-3-Clause), four ericw-tools test maps (GPL-2.0-or-later) and two progs_dump maps (no commercial use, so `fetch-at-test`).

**Excluded:** QuakeC 1.06 (no license) and Quoth, whose release has no QuakeC source and whose readme only permits redistributing the unmodified zip. The ericw-tools test maps derived from id's Quake and Quake II map sources (`E1M1-edited-ents.map`, `base1-test.map`, `base1leak.map`) are left out because those files state no license of their own.

**Test assets:** LibreQuake's `lite` release (BSD-3-Clause, GPL-2.0-or-later progs), `fetch-at-test`. id's `pak0.pak` and `pak1.pak` are `never`.

## Checking

The cache is `--dest <dir>`, else `$QS_TEST_ASSETS`, else `quakecstudio/test-assets` under `$XDG_CACHE_HOME` or `~/.cache`, and never inside the source tree. Everything downloaded stays in `<cache>/downloads/` and is checked again before use, so a second run works offline. An entry marked `never` is refused by name. A codebase is read from its source's archive at the pinned commit (from GitLab, an archive of its `path` only, so Xonotic's `qcsrc/` is a 2 MB download, not the 800 MB repository), and only the files under its `path` are written: from Quake-Tools that is `qcc/v101qc/` (its `.qc` files and `progs.src`), while id's compiler and engine sources are fetched by the oracles build (`oracles/sources.toml`).

```bash
cargo xtask corpus verify              # offline: manifest rules and the repository scan
cargo xtask corpus verify --online     # also downloads every pinned file and compares SHA-256
cargo xtask corpus fetch [<id>...]     # write codebases to <cache>/codebase/<id>, maps to <cache>/map/<id>.map,
                                       # trees to <cache>/tree/<id>
cargo xtask corpus build [<id>...]     # compile id 1.01 (id's qcc) and the rerelease (FTEQCC) into
                                       # <cache>/progs/<id>/ and check each progs.dat round-trips
cargo xtask assets fetch               # download, checksum and extract the test assets
cargo xtask assets precache            # check they cover the rerelease's precaches
python3.11 corpus/language_coverage.py --source-dir <cache>/codebase/id-1.01
                                       # check docs/spec/language.md covers every construct in id 1.01
cargo xtask lex-corpus <cache>/codebase/id-1.01  # lex it and preprocess progs.src with no errors
cargo xtask parse-corpus --vanilla --deletions <cache>/codebase/id-1.01  # parse it, check recovery
cargo xtask parse-corpus <cache>/codebase/<id>...  # every file round-trips through the tree
cargo xtask project-corpus <cache>/codebase/<id>...  # open each as a project; check its file graphs
cargo xtask project-corpus <cache>/tree/freecs-tree/cstrike/src  # FreeCS, with what it builds against
cargo xtask map-corpus <cache>/map     # every map reads and writes back byte for byte (qs-map)
cargo test -p xtask corpus            # the commands' offline tests
(cd corpus && python3.11 -m unittest test_language_coverage.py)
```
