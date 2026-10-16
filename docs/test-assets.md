# Test assets

The headless preview host runs scripted engine scenarios in CI on all three OSes (QA-4). Those scenarios need game data: a map, models, textures and sounds. This page records which open assets CI uses, why their licenses allow it, and the rule that keeps id's game data out (LIC-4).

## The rule

**id's `pak0.pak` and `pak1.pak` never enter CI or this repository.** Quake's game data is commercial. Nothing fetches it, nothing commits it, and no CI job reads it:

- [`corpus/manifest.toml`](../corpus/manifest.toml) lists it as the `id1-paks` asset with `use = "never"`, and `cargo xtask corpus verify` fails if that entry or its place in the `never` list is removed.
- `cargo xtask corpus verify` fails on any tracked `.pak` file.
- `cargo xtask assets fetch` downloads only the archives the manifest pins, keeps an archive only if its SHA-256 matches, and extracts only the members the manifest pins by SHA-256. id's files cannot come through it.
- On a developer's machine the preview host reads the user's own Steam or GOG install in place ([P0-22](../plan/backlog/p0.yaml)); that never happens in CI.

## Chosen assets

| Asset | Release | License | Use |
| --- | --- | --- | --- |
| `librequake-lite` | [LibreQuake v0.09-beta](https://github.com/lavenderdotpet/LibreQuake/releases/tag/v0.09-beta), `lite.zip` (56 MB, SHA-256 `428e736b...da83`) | BSD-3-Clause; GPL-2.0-or-later for its progs and `pop.lmp` | `fetch-at-test` |

[LibreQuake](https://github.com/lavenderdotpet/LibreQuake) is a free replacement for Quake's `id1` game data. It uses id's file names, so QuakeC written for Quake, including the rerelease, finds its models and sounds under the names it expects. It is already in the conformance corpus (its QuakeC and 20 of its map sources), so its license evidence is pinned there.

Only the two pak files and the license documents are extracted. `pak0.pak` holds:

- 10 playable maps (`start`, `dev`, `lq_e0m1` to `lq_e0m8`) and 13 brush models (`maps/b_*.bsp`), with their textures inside the BSPs;
- 81 models and 3 sprites under `progs/`;
- 221 sounds under `sound/`;
- `gfx.wad` and 66 `gfx/` images, including `palette.lmp`, `colormap.lmp` and `pop.lmp`;
- LibreQuake's own `progs.dat` and `qwprogs.dat`, which our scenarios replace with progs we compile.

The `lite` build was chosen over `full` (116 MB) because it already covers what the rerelease precaches (below) at half the download. `server.zip` has more maps but no models or sounds. The archive's music tracks are not extracted.

### License review

Read from the release archive on 2026-10-15. Each license file is pinned twice: in the manifest's `librequake` source at commit `681890f` and as a member of the archive, and the two SHA-256 values are equal, so the release carries the same license files as the pinned commit (which is 139 commits after the `v0.09-beta` tag).

| Content | License | Evidence |
| --- | --- | --- |
| Models, textures, sounds | BSD-3-Clause | `docs/COPYING` (BSD 3-Clause text, "Contributors to the LibreQuake project"); `docs/README-IMPORTANT-LICENCE-INFO`: "bsd3 ... is what all the models textures and sounds are under" |
| `progs.dat`, `qwprogs.dat`, `pop.lmp` | GPL-2.0-or-later | `docs/README-IMPORTANT-LICENCE-INFO`: "gpl2 ... just applies to the quakec, progs.dat and pop.lmp"; `docs/misc-docs/COPYING` (GPL v2 text); `docs/misc-docs/pop.lmp.md` quotes id's header granting version 2 or any later version; the QuakeC's own `defs.qc` header does the same (corpus entry `librequake`) |
| Content derived from Freedoom | BSD-3-Clause | `docs/freedoom-docs/COPYING.adoc` |
| Individual contributions | BSD-3-Clause unless stated; a few marked public domain | `docs/CREDITS`, whose template lists each contributor's license as "bsd3 (same as lq) or cc0" |

Both licenses are compatible with GPL-3.0-or-later (ADR-0002). We still never commit or bundle these files: they are fetched by checksum for local and CI tests only, which needs nothing beyond the licenses' own terms. If a later release ever ships them to users, the BSD notice and `CREDITS` go with them.

## What a headless scenario gets

`cargo xtask assets fetch` puts the assets in a cache outside the source tree: `--dest <dir>`, else `$QS_TEST_ASSETS` if set, else `quakecstudio/test-assets` under `$XDG_CACHE_HOME` or `~/.cache`; it refuses a directory inside the source tree. The layout is

```
<cache>/downloads/<sha256>.zip      the release archive, kept so later runs work offline
<cache>/downloads/<source>-<commit>.tar.gz   a repository archive cargo xtask corpus fetch keeps
<cache>/librequake-lite/id1/        pak0.pak, pak1.pak, docs/, and the stand-ins below
```

so an engine starts with `-basedir <cache>/librequake-lite`. After extracting, `fetch` fails unless the paks contain `gfx/palette.lmp`, `gfx/colormap.lmp`, `gfx.wad` and `maps/start.bsp`, plus at least one map (`maps/*.bsp`), model (`progs/*.mdl`) and sound (`sound/*.wav`).

## Rerelease precache coverage

`cargo xtask assets precache` reads the rerelease QuakeC (corpus codebase `rerelease`, the base campaign) at its pinned commit through the same download cache as `cargo xtask corpus fetch`, checks the pinned `defs.qc` checksum, lists every `precache_model`, `precache_model2`, `precache_sound` and `precache_sound2` call, and fails if a precached file is neither in the paks nor given a stand-in. Calls inside comments and strings are skipped.

Recorded result, 2026-10-16 (the same as the Python script it replaced recorded on 2026-10-15):

```
asset librequake-lite covers rerelease: 33 .qc files, 373 precache calls, 318 files, 313 present, 5 by stand-in, 0 missing
  stand-in: sound/boss2/death.wav -> sound/boss1/death.wav
  stand-in: sound/boss2/idle.wav -> sound/boss1/out1.wav
  stand-in: sound/boss2/pop2.wav -> sound/player/udeath.wav
  stand-in: sound/boss2/sight.wav -> sound/boss1/sight1.wav
  stand-in: sound/player/slimbrn2.wav -> sound/player/slmbrn2.wav
  not a literal: quakec/misc.qc:274 precache_model(self.mdl)
```

| Precached by the rerelease | Stand-in | Why |
| --- | --- | --- |
| `sound/boss2/death.wav`, `idle.wav`, `sight.wav` | Chthon's `boss1/death.wav`, `out1.wav`, `sight1.wav` | LibreQuake has no Shub-Niggurath sounds; the nearest boss's sounds play the same roles |
| `sound/boss2/pop2.wav` | `sound/player/udeath.wav` | Shub's gib pop; the gib sound is the closest match |
| `sound/player/slimbrn2.wav` | `sound/player/slmbrn2.wav` | LibreQuake ships this sound under a misspelled name |

`fetch` writes each stand-in into `id1/` as a loose copy under the precached name, so the engine finds it. The one non-literal call is `misc_explobox`, which defaults `self.mdl` to `maps/b_explob.bsp`; that file is in `pak0.pak`.

The check covers the base campaign. The mission packs and the other rerelease codebases precache their own models and sounds, which have no free equivalents yet; scenarios for them run locally against the user's install.

## Commands

```bash
cargo xtask assets fetch       # download, checksum, extract, check the scenario minimum
cargo xtask assets precache    # list the rerelease's precaches and check coverage
cargo test -p xtask corpus     # offline tests
```

CI runs `fetch` and `precache` on macOS, Windows and Linux and caches `<cache>/downloads` keyed on the SHA-256 of `corpus/manifest.toml` ([P0-03](../plan/backlog/p0.yaml)). Both commands work offline once the cache holds the archives.

## Adding or changing an asset

1. Read the release's license files and record them in this page's review. The asset's `source` must be in the manifest with those files pinned as evidence.
2. Add an `[[asset]]` entry: the release `url` and `sha256`, the `members` to extract with their SHA-256 (the evidence files among them), `strip`, `game_dir`, `covers` and any `stand_ins`. `corpus/README.md` explains the fields.
3. Run `cargo xtask assets fetch`, `cargo xtask assets precache` and `cargo xtask corpus verify`, and paste the new `precache` output above.
