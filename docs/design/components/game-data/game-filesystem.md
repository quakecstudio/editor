# Game filesystem

**Crates:** `qs-vfs`, `qs-install`, `qs-paths` (L0). **Specs:** FS-1 to FS-6; ADR-0008, ADR-0011.

## Context and design philosophy

Three crates decide where files are: where the game's files are inside an install, where the
user's installs are on the machine, and where QuakeC Studio keeps its own files.

| Crate | Answers | Spec / decision | Source |
| --- | --- | --- | --- |
| `qs-vfs` | The search path id's engine builds from a base directory and its command line, and file lookup on it | [`filesystem.md`](../../../spec/filesystem.md) FS-1 to FS-5; PAK-1.4, WAD-1.4 | Ported from `WinQuake/common.c` (`COM_InitFilesystem`, `COM_AddGameDirectory`, `COM_FindFile`, `COM_LoadFile`, `COM_CheckRegistered` and helpers) and `wad.c` `W_LoadWadFile` (`crates/qs-vfs/src/lib.rs:5-6`) |
| `qs-install` | Where Steam, GOG and Heroic put Quake; whether a folder is an install; which editions it holds; directory links for run roots | FS-6; [ADR-0008](../../../adr/0008-game-directories.md) | Written from web sources and RFC 1321; nothing ported (FS-6 "Source: none in id's GPL code") |
| `qs-paths` | QuakeC Studio's config, data, cache and logs directories per OS; the folders the stores keep their files in | [ADR-0011](../../../adr/0011-app-directories.md); CONV-16 | Written from ADR-0011; no engine spec (`crates/qs-paths/README.md`) |

The principle across all three, from ADR-0008 and plan requirements LIC-4, PLAT-6 and
PLAT-8: **game data is read where it is and never copied or written.** The only write any of
these crates makes is `qs-install::link_dir`, which creates or repairs a directory link
outside the install and never removes a real file or folder (`crates/qs-install/src/link.rs:70-82`).

Dependency direction: `qs-install` depends on `qs-vfs` (host filesystem trait, `FS_GAMENAME`),
`qs-pak` (to read an unrecognised archive) and `qs-paths` (store folders); `qs-vfs` depends on
`qs-pak` and `qs-wad`; `qs-paths` depends on nothing. `qs-log` also depends on `qs-paths`.

## Components

### Search path (`qs-vfs`)

`Vfs::new(host, args, default_basedir)` runs `COM_InitFilesystem` then `COM_CheckRegistered`
over the engine's arguments (`crates/qs-vfs/src/lib.rs:269-297`); the result is an ordered list
of `Element`s (directories and loaded `Archive`s), the game directory, and the modified,
proghack and registered flags. `find` is `COM_FindFile`, `read` and `load` are `COM_LoadFile`,
`load_wad` finds and checks `gfx.wad`, and `files` lists every file of every element.

All host access goes through the `HostFs` trait (`open`, `read_dir`; `crates/qs-vfs/src/host.rs:36`),
with `StdFs` the real implementation and an in-memory `MemFs` in tests. This makes every
lookup rule testable on every CI host.

Where id's code leaves the answer to the host or overflows a buffer, the spec fixes
QuakeC Studio's answer as a deliberate divergence, and `qs-vfs` implements it:

| Statement | Answer |
| --- | --- |
| FS-2.5 | `-cachedir` is not implemented |
| FS-3.7 | Names match ASCII case-insensitively on every OS: exact match first, then the first case-insensitive match in name order (`host.rs:105-141`) |
| FS-3.8 | A loose lookup that is absolute, has a drive letter or a `..` component never matches a directory (`lib.rs:498-511`) |
| FS-3.9 | No path-length limit |
| FS-3.10 | No limit on the number of archives |

Directory symlinks are reported as `EntryKind::Other` so walks never loop; file symlinks are
files (`host.rs:13-21`, `:71-79`).

### Install discovery (`qs-install`)

A `Finder` takes the platform, `StoreDirs`, an `InstallFs` and a `Registry`, all injected
(`crates/qs-install/src/lib.rs:286-331`); `find_installs()` and `examine_folder()` bind them
to the real host. Store adapters (`stores.rs`) read Steam's `libraryfolders.vdf` and
`appmanifest_*.acf` (a minimal KeyValues parser, `vdf.rs`, depth-limited to 32), GOG's
registry keys and `goggame-<id>.info`, and Heroic's `installed.json` (`serde_json`). On
Windows the registry is read by spawning `reg query` (`registry.rs:26-29`).

An `Install` has a source, a root, and optional original and rerelease `GameBase`s (FS-6.1,
FS-6.6). `Install::validate` identifies each `pak0.pak`/`pak1.pak` by published size and MD5
(`known.rs`, `FS_KNOWN_FILES`, checked row for row against `filesystem.md` by
`tests.rs:856-897`), reports editions and missing files (FS-6.2 to FS-6.5). MD5 is a
hand-written RFC 1321 implementation (`md5.rs`) to avoid a dependency.

`link_dir(target, link)` makes the run-root links ADR-0008 prescribes: a symbolic link on
macOS and Linux, a junction on Windows (the `junction` crate). It creates the link's parent
folders, repairs a link that is broken or points elsewhere, and refuses with
`LinkError::Occupied` when something real is in the way.

### Application directories (`qs-paths`)

`resolve(platform, env)` is a pure function of the platform and an environment lookup, and
`current()` binds it to the process (`crates/qs-paths/src/lib.rs:135-149`). It implements
ADR-0011's table: native directories per OS; on Linux an empty or relative `XDG_*` is
ignored, while a missing, empty or relative `HOME`, `APPDATA` or `LOCALAPPDATA` is a
`PathsError`. Resolving never creates a directory. `store_dirs(platform, env)` gives the
folders where Steam, GOG and Heroic keep their files; a missing, empty or relative variable
drops that folder rather than failing (`stores.rs:17-20`). Registry lookups are left to
`qs-install` (`stores.rs:7-9`).

## Error model

- `qs-vfs` returns `VfsError` with id's messages where id has one: `CouldntLoadPackfile`,
  `CorruptedDataFile`, `ModifiedShareware`, `WadNotFound`, plus `Archive`, `Wad` and `Io`
  wrapping the lower crates' errors.
- `qs-install` reports what an install lacks as data (editions found, missing files) rather
  than failing; `link_dir` returns `LinkError`, `Occupied` when something real is in the way.
- `qs-paths` returns `PathsError` for a missing, empty or relative `HOME`, `APPDATA` or
  `LOCALAPPDATA`; `store_dirs` drops a folder instead of failing.

## Verification

- `qs-vfs`: statement-cited tests over `MemFs`, plus real-filesystem tests in a temp tree
  with symlinks on unix (`crates/qs-vfs/src/tests.rs:704-772`); the `walk` example decodes
  every file of an install with the asset crates.
- `qs-install`: every store layout on every OS over `MemFs`/`MemRegistry`; real-filesystem
  link tests per OS in CI; the `find` example for manual runs.
- `qs-paths`: inline tests of every platform's rules on every host; `crates/qs-tests/tests/it/paths.rs`
  checks `current()` on the real environment.
- Real installs: P1-30 (#277) ran `find` and `walk` on Justin's GOG installs on macOS
  (original and Quake Enhanced, six game directories) on 2026-10-16 with no failures; the
  issue comment records that Windows and Linux runs were still to do.
- L0 readiness at `110ce43`: `qs-vfs` 98.9% mutation, `qs-install` 93.8%, `qs-paths` 100%
  (`docs/readiness/p1-r0.md:61-64`).

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Search path source | Port `common.c` | Write from engine documentation | Core principle: model what the engine executes (`CLAUDE.md`) |
| Case matching | ASCII case-insensitive on every OS | Follow the host | FS-3.7: fixes the DOS/Windows reference builds' answer so results do not differ by platform |
| Path confinement | Loose lookups never leave the install | id's unchecked join | FS-3.8: QuakeC Studio confines reads to the install |
| Host access | `HostFs` trait, injected | Direct `std::fs` | [inferred] every rule testable in memory on every host |
| Launch layout | Linked run directory inside the project (`build/run/<flavour>/`) | Per-engine flags; write into install; copy game data | ADR-0008: the only recipe every engine accepts on every OS without touching or copying the install |
| Link helper location | `qs-install::link_dir` | `qs-paths` | ADR-0008 allowed either; [inferred] it sits with install discovery, which the run roots link to |
| Windows junctions | `junction` crate (MIT) | Windows API through allowed crates | ADR-0008 consequence; README: no `unsafe` needed |
| App directories | Native per OS via `qs-paths` only | XDG everywhere; `dirs`/`directories` crates | ADR-0011: desktop-app expectations, roaming config vs local data on Windows, every platform tested on every host |
| MD5 | Hand-written RFC 1321 | A hash crate | `crates/qs-install/src/md5.rs:5-6`: avoids a dependency |
| Registry | Spawn `reg query` | A registry crate | [inferred] no `unsafe` and no dependency |

## Open questions

### Resolved
1. Game directory strategy: ADR-0008 (P0-22, #22). App directories: ADR-0011.
2. GOG's Quake Enhanced original game in `dos/`: FS-6.6 (P1-54, #342).

### Deferred
1. **Layering of host I/O.** `qs-install` is mapped to L0 "formats" but spawns `reg`
   (`registry.rs:26-29`), reads the real host and writes links (`link.rs:82-141`). L0's
   description in `.claude/rules/layering.md` covers formats, the paths module, logging and the
   game filesystem; its crate column (`layering.md:12-15`) does not list `qs-install`, though
   `layers.toml:24` maps it to L0.
2. **`filesystem.md` prose is stale.** `docs/spec/filesystem.md:278-280` and `:328-330` say none
   of FS-5/FS-6 was checked against a real install; P1-30 (#277) closed after the macOS GOG
   runs. Steam installs and Windows/Linux `find` runs are not recorded; ADR-0008's
   consequences move the acceptance launches on real installs to P3-06 (#94), which also
   creates and repairs the run roots with `link_dir` before every launch.
3. `Missing`'s doc cites FS-6.4 (`crates/qs-install/src/lib.rs:144`); the statement for missing
   files is FS-6.5, which the tests cite.
4. `-game` values with `..` are followed (`host.rs:114-117`), as FS-1.8 requires, while loose
   lookups reject `..` (FS-3.8). Callers that pass user input as `-game` must confine it
   themselves; ADR-0008 passes a single directory name.
5. `com_file_extension` takes the text after the first `.` (`lib.rs:657-665`, FS-2.3), so
   `-path ./x/pak0.pak` is a directory. Faithful to id; surprising to users.
6. Every `find` lists directories through `read_dir` per component with no cache, and every
   archive read reopens the archive (`lib.rs:533-536`, pinned by `tests.rs:695-701`).
   [inferred] acceptable until a caller needs it faster.
7. `for number in 0_u32..` relies on `break` (`lib.rs:375`); a 2^32nd archive would overflow
   (theoretical).
8. `link_dir` checks then acts between `symlink_metadata` and create (`link.rs:87-107`); a
   concurrent change can race. Its non-Windows branch uses `std::os::unix`, so a target that
   is neither would not compile (`link.rs:111-124`).
9. `finding_on_this_machine_does_not_fail` runs against the developer's real stores and
   registry (`crates/qs-install/src/tests.rs:516-518`); non-hermetic.
10. Many `qs-install` tests and three `qs-vfs` tests carry no spec citation
    (`qs-vfs/src/tests.rs:614`, `:648`, `:694`); `qs-paths` tests cite nothing, as it has no
    spec (ADR-0011 only).
11. Duplicate in-memory `MemFs` fixtures in `qs-vfs/src/fixtures.rs:27` and
    `qs-install/src/fixtures.rs:27`.
12. `qs-vfs/README.md:40-48` sample output omits the `game directory:` line `walk.rs:181` prints.
13. `qs-install` has no in-tree consumer yet. Planned: run roots and launch via `link_dir`
    (P3-06, #94), first-run guide to the user's Quake files (P3-21, #205), mod export that never
    follows run-root links and refuses id's data (P4-28, #207), templates ignoring `build/`
    (P4-17, #128).

## References

- ADRs: [ADR-0008](../../../adr/0008-game-directories.md), [ADR-0011](../../../adr/0011-app-directories.md)
- Spec: [`filesystem.md`](../../../spec/filesystem.md); store layouts in [`docs/game-data.md`](../../../game-data.md)
- Plan: P1-18 (#42), P1-19 (#43), P1-30 (#277), P1-54 (#342); open P3-06 (#94), P3-21 (#205), P4-17 (#128), P4-28 (#207)
