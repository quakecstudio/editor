# ADR-0008: Launch engines from a linked run directory inside the project

- **Status:** Proposed
- **Date:** 2026-10-15
- **Decided by:** pending
- **Issue:** [#22](https://github.com/quakecstudio/editor/issues/22)
- **Requirements:** PLAT-6, LIC-4

## Question

Where do the compiled `progs.dat` and maps go, and how is each engine launched against the user's
Steam or GOG install, for the original game and the rerelease, without modifying or copying it?

## Context

PLAT-6 asks for build output and engine launch against the user's install "without modifying or
copying it", and LIC-4 and PLAT-8 say game data is never bundled and always read where it is. Modders
traditionally put their mod folder inside the Quake folder (`Quake/mymod/progs.dat`) and launch with
`-game mymod`; the IDE must not do that, because the install belongs to Steam or GOG (which can
verify or delete it) and on macOS is a folder the player assembled by hand. The answer is needed by
the engine adapters (P3-06 [#94](https://github.com/quakecstudio/editor/issues/94), which depends on
this issue; P3-07 FTEQW, P3-08 restart-loop engines), by finding the game data (P1-19
[#43](https://github.com/quakecstudio/editor/issues/43), which reports the `id1` and `rerelease`
paths), by new-project templates (P4-17) and by mod export (P4-28).

### How the engines find files

Read from each engine's source at its default branch on 2026-10-15 (`COM_InitFilesystem` and the
`sys_sdl_*.c` files):

| Engine | Game root | Mod directory | Separate write directory |
| --- | --- | --- | --- |
| QuakeSpasm 0.97 | One `-basedir` | `-game NAME`, which must be a single directory name under the basedir ("gamedir should be a single directory name, not a path") | `-userdir PATH` (new in 0.97.0) adds `PATH/NAME` as a search path and write target, but only in builds made with `DO_USERDIRS`, and never on Windows (`sys_sdl_win.c`: "userdirs not really necessary for windows guys") |
| Ironwail | Several `-basedir` flags: the first must hold `id1/pak0.pak`, each further one is added as another root | `-game NAME`, a single name searched in every root | A per-user preferences directory on every OS |
| vkQuake | One `-basedir`; finds Steam, GOG and Epic installs itself | `-game NAME`, a single name | `-userdir` on macOS and Linux; on Windows `%APPDATA%\vkQuake` for store installs or with `-multiuser` |
| FTEQW | `-basedir` | `-game NAME` | `-homedir PATH`, searched on top of the basedir and used for writes |
| Our preview host (a QuakeSpasm fork, ADR-0004 proposed) | as QuakeSpasm | as QuakeSpasm | whatever the fork adds |
| KEX rerelease | not launched: a compile target only (ENG-8) | - | - |

The store layouts: the original game is `<install>/id1/` (`pak0.pak`, `pak1.pak`); the 2021
rerelease is `<install>/rerelease/`, holding its own `id1/` and `QuakeEX.kpf` (localization and
interface art, which QuakeSpasm reads from the basedir or, with `DO_USERDIRS`, the userdir). Ironwail
and vkQuake look for exactly `<install>/rerelease`.

So no flag works across all engines and operating systems: only Ironwail and FTEQW can put a mod
folder outside the game root on Windows. What every engine accepts is a basedir with an `id1` and a
mod folder side by side.

### Links on each OS

macOS and Linux symbolic links need no privileges. On Windows, a symbolic link needs Developer Mode
or administrator rights, but a directory junction does not: Microsoft documents that a junction
"can also link directories located on different local volumes on the same computer". Junctions
cannot point at files or at network drives. Hard links can point at files only on the same volume.
The `junction` crate (MIT, 2.1.0) creates them, or the IDE can call the Windows API itself in
`qs-paths`.

**Not verified here.** The acceptance criterion asks for a prototype launch with Steam and GOG
installs on all three operating systems, for `id1` and the rerelease, with recorded launches. That
needs the installs and the three machines; an agent has neither and must not fetch game data. The
engine behaviour above is read from source, not observed. The launches go on the issue before this
ADR is accepted.

## Options

| Option | Consequences |
| --- | --- |
| A. A linked run directory in the project (recommended) | The compiler writes to `<project>/build/<mod>/`. For each game flavour the IDE keeps `<project>/build/run/<flavour>/` with a link `id1` to the install's `id1` (or `rerelease/id1`) and a link `<mod>` to `../../<mod>`, and launches every engine with `-basedir <project>/build/run/<flavour> -game <mod>`. One recipe for every engine and OS; nothing is written into the install; engine writes (config, saves, screenshots) land in the project's mod folder. Costs: link creation and repair code in `qs-paths`, and the rerelease's `QuakeEX.kpf` is a file, which a Windows junction cannot link (see Decision) |
| B. Each engine's own flags | Ironwail gets two `-basedir`s, FTEQW `-basedir` plus `-homedir`, QuakeSpasm and vkQuake `-userdir` on macOS and Linux, and our preview host a new flag. No links, but Windows has no answer for stock QuakeSpasm or vkQuake, so A is needed there anyway, and four recipes have to be kept working as engines change |
| C. Write the mod into the install (`<install>/<mod>/`) | What modders do by hand and what every engine supports, but it modifies the install, which PLAT-6 forbids; Steam's "verify files" or an uninstall can delete the work. Rejected |
| D. Copy the game data into the project | Works everywhere, but copies `pak0.pak`/`pak1.pak` into a folder the user may commit or share, against LIC-4 and PLAT-8. Rejected |

## Decision

Recommendation: **option A, a linked run directory inside the project, used for every engine.**
It is the only recipe every engine accepts on every OS without touching or copying the install. It
keeps the user's mod in the project, where the build, the git checkpoints and mod export already
look. Concretely:

- **Build output:** `<project>/build/<mod>/`, holding `progs.dat`, `.lno`, compiled `maps/` and the
  project's assets as the engine expects them. `<mod>` is the project's mod name (a single directory
  name, as every engine requires). `build/` is ignored by version control in the new-project
  templates.
- **Run roots:** `<project>/build/run/original/` and `<project>/build/run/rerelease/`, created and
  repaired before every launch. Each holds `id1`, a link to the install's `id1` or `rerelease/id1`,
  and `<mod>`, a link to `../../<mod>`. The links are symbolic links on macOS and Linux and junctions
  on Windows.
- **Launch:** `-basedir <run root> -game <mod>` for the preview host, QuakeSpasm, Ironwail, vkQuake
  and FTEQW. In Ironwail and vkQuake an explicit `-basedir` also turns off their own store
  detection, so they cannot pick the install instead (read from `COM_InitBaseDir` and
  `COM_InitFilesystem`).
- **`QuakeEX.kpf`:** on macOS and Linux a symbolic link in the rerelease run root. On Windows a
  junction cannot link a file, so the preview host fork gains one patch: look for `QuakeEX.kpf` next
  to the real `id1` it was given (a `-kpf PATH` option), listed in its `PATCHES.md`. Stock QuakeSpasm
  on Windows then runs the rerelease without localized strings, which the restart-loop adapter
  reports as a known limitation.
- **Never:** the IDE never writes inside the install, never copies a file out of it, and refuses to
  follow a run-root link when exporting a mod (P4-28 already refuses id's data).

## Consequences

If accepted as recommended:

- `plan/backlog/p3.yaml`: P3-06 gains "creates and repairs the run roots (`build/run/original`,
  `build/run/rerelease`) and launches with `-basedir` and `-game`"; P3-02 gains the `-kpf` patch in
  its `PATCHES.md`; P3-08's acceptance notes the Windows rerelease localization limit for stock
  engines.
- `crates/qs-paths` (P0-01, ADR-0011) or the install crate (P1-19): one function that makes a
  directory link per OS, with tests for symbolic links, junctions, a broken link and a missing
  install. No new dependency is needed if it calls the Windows API through the crates the workspace
  already allows; otherwise the `junction` crate (MIT) is added under the conventions.
- P4-17 templates ignore `build/`; P4-28 export reads `build/<mod>/` and never follows the run-root
  links.
- The acceptance criterion's recorded launches (Steam and GOG, three OSes, `id1` and rerelease) are
  run by a person before acceptance, or become the acceptance test of P3-06 if Justin prefers to
  accept on the source reading.

## Links

- Issue: [#22](https://github.com/quakecstudio/editor/issues/22); pull request: this ADR's draft PR.
- Related: [ADR-0011](0011-app-directories.md) (application directories), ADR-0004 proposed in [#221](https://github.com/quakecstudio/editor/pull/221) (preview host base).
- Design document: [Platform support](../design/design-doc.md#platform-support), [Source and licensing](../design/design-doc.md#source-and-licensing).
- Engine sources read: [sezero/quakespasm](https://github.com/sezero/quakespasm) (`Quake/common.c` `COM_InitFilesystem`, `COM_AddGameDirectory`, `LOC_Init`; `Quake/sys_sdl_unix.c`, `Quake/sys_sdl_win.c`; `Quakespasm.txt` 0.97.0 notes), [andrei-drexler/ironwail](https://github.com/andrei-drexler/ironwail) (`Quake/common.c` `COM_InitBaseDir`), [Novum/vkQuake](https://github.com/Novum/vkQuake) (`Quake/common.c` `COM_InitFilesystem`, readme), [fte-team/fteqw](https://github.com/fte-team/fteqw) (`engine/common/fs.c`, `-homedir`).
- Microsoft, [Hard links and junctions](https://learn.microsoft.com/en-us/windows/win32/fileio/hard-links-and-junctions).
- [junction crate](https://crates.io/crates/junction).
