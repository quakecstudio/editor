# The game filesystem

Prefix: `FS` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes how the engine turns a base directory and command-line options into a search
path of directories and PAK archives, and how it finds a file on that path, as implemented
by `WinQuake/common.c` in [id-Software/Quake](https://github.com/id-Software/Quake) at
commit `bf4ac424ce754894ac8f1dae6a3981954bc9852d`, with the 2021 rerelease's layout in
FS-5. The archive format is [`pak.md`](pak.md). Implemented by `crates/qs-vfs`
([#42](https://github.com/quakecstudio/editor/issues/42)); finding the install itself is
[#43](https://github.com/quakecstudio/editor/issues/43).

Paths below are written with `/`, which is what the engine's `sprintf` calls insert.

## FS-1 Building the search path

**Source:** `WinQuake/common.c`, `COM_InitFilesystem`, `COM_AddGameDirectory` and
`COM_CheckParm`; the default base directory from `WinQuake/sys_win.c`, `WinMain`,
`WinQuake/sys_dos.c`, `main`, and `WinQuake/sys_linux.c`, `main`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

The search path is a singly linked list searched from its head. `COM_AddGameDirectory`
pushes the directory onto the head and then pushes `pak0.pak`, `pak1.pak`, ... on top of
it, so within one game directory the archives come before the loose files, and each game
directory added later comes before everything added earlier.

### Behaviour

- **FS-1.1** Command-line options are matched against the arguments after the program
  name, exactly and case-sensitively (`Q_strcmp`), and the first occurrence wins. An
  option that takes a value (`-basedir`, `-game`, `-cachedir`) is ignored when it is the
  last argument.
- **FS-1.2** The base directory is the argument after `-basedir` if there is one;
  otherwise the host's default, which is the current working directory on Windows
  (`sys_win.c`) and DOS (`sys_dos.c`) and `.` on Linux (`sys_linux.c`).
- **FS-1.3** If the base directory ends with `/` or `\`, exactly one such character is
  removed; any others stay. `-basedir ..//base//` gives the game directory
  `..//base//id1`. An empty base directory stays empty, giving `/id1`.
- **FS-1.4** Adding a game directory `D` puts `D` at the head of the search path, then
  for N = 0, 1, 2, ... puts the archive `D/pakN.pak` (N in decimal, no padding) at the
  head, stopping at the first N whose file cannot be opened. The result, from the head, is
  `D/pakK.pak`, ..., `D/pak1.pak`, `D/pak0.pak`, `D`: a game directory's archives are
  searched before its loose files, and higher-numbered archives before lower ones.
- **FS-1.5** The archive scan stops at the first missing number: with `pak0.pak` and
  `pak2.pak` but no `pak1.pak`, `pak2.pak` is never loaded. With no `pak0.pak` the game
  directory has only its loose files. A missing archive is not an error, and neither is a
  missing game directory.
- **FS-1.6** An archive that opens but fails to load (PAK-1, PAK-2) is fatal; it is not
  skipped.
- **FS-1.7** Game directories are added in this order, each in front of the previous
  ones: `<base>/id1` always; `<base>/rogue` if `-rogue` is given; `<base>/hipnotic` if
  `-hipnotic` is given; `<base>/<value>` if `-game <value>` is given. The order does not
  depend on the order of the options on the command line, so with both `-rogue` and
  `-hipnotic`, `hipnotic` is searched before `rogue`.
- **FS-1.8** The value of `-game` is appended to `<base>/` as given: it is not checked,
  and it may contain `/` or `..`. `-game` marks the game as modified (FS-4.3). `-game id1`
  adds `id1` a second time, which changes no lookup result.
- **FS-1.9** The game directory the engine writes to (`com_gamedir`, used by
  `COM_WriteFile`) is the last directory added by FS-1.7.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `FS_GAMENAME` | `id1` | `WinQuake/quakedef.h`, `GAMENAME` used by `COM_InitFilesystem` |
| `FS_PAK_NAME` | `pak%i.pak` | `WinQuake/common.c`, the `sprintf` format in `COM_AddGameDirectory` |

### Test vectors

Checked on 2026-10-16 against id's code: `COM_CheckParm`, `COM_FileExtension` and the
whole "QUAKE FILESYSTEM" block of `common.c` (`COM_FindFile`, `COM_LoadFile`,
`COM_LoadPackFile`, `COM_AddGameDirectory`, `COM_InitFilesystem` and their data), copied
unchanged from the pinned commit and built with the system C compiler with `Sys_File*` on
`stdio`, `Hunk_Alloc` on `calloc`, `host_parms.basedir` = `.` and no cache directory. The
tree, built by a script under the base directory:

| Path | Contents |
| --- | --- |
| `id1/pak0.pak` | `a.txt` = `id1-pak0`, `progs.dat` = `pak0-progs`, `f.txt` = `pak0-f` |
| `id1/pak1.pak` | `a.txt` = `id1-pak1` |
| `id1/pak3.pak` | `d.txt` = `id1-pak3` (there is no `pak2.pak`) |
| `id1/a.txt`, `id1/b.txt`, `id1/sub/c.txt` | `id1-loose`, `id1-loose-b`, `id1-loose-c` |
| `mod/pak0.pak` | `e.txt` = `mod-pak0` |
| `mod/a.txt`, `mod/progs.dat` | `mod-loose`, `mod-progs` |
| `rogue/pak0.pak`, `hipnotic/pak0.pak` | `f.txt` = `rogue-f`, `hipnotic-f` |

| Options | Search path, head first | Lookups | Statements |
| --- | --- | --- | --- |
| none | `./id1/pak1.pak`, `./id1/pak0.pak`, `./id1` | `a.txt` = `id1-pak1`; `b.txt` = `id1-loose-b`; `d.txt` not found | FS-1.4, FS-1.5, FS-3.1 |
| `-game mod` | `./mod/pak0.pak`, `./mod`, then the three above; game directory `./mod` | `a.txt` = `mod-loose`; `e.txt` = `mod-pak0`; `progs.dat` = `mod-progs` | FS-1.7, FS-1.9, FS-3.1 |
| `-hipnotic -rogue -game mod` | `mod` (2), `hipnotic` (2), `rogue` (2), `id1` (3) | `f.txt` = `hipnotic-f` | FS-1.7 |
| `-game` as the last argument | as with no options | `a.txt` = `id1-pak1` | FS-1.1 |
| `-basedir ..//base//` | `..//base//id1/pak1.pak`, ... | `a.txt` = `id1-pak1` | FS-1.3 |

## FS-2 Options that replace or bend the search path

**Source:** `WinQuake/common.c`, `COM_InitFilesystem`, `COM_FileExtension` and
`COM_FindFile` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

### Behaviour

- **FS-2.1** `-path` discards the search path built by FS-1 and builds a new one from the
  arguments that follow it, up to the end or the first argument starting with `+` or `-`.
  Each is put at the head in turn, so the last one listed is searched first. `-path` with
  no such argument leaves the search path empty. It does not change the game directory of
  FS-1.9, and it marks the game as modified (FS-4.3).
- **FS-2.2** A `-path` argument whose extension (FS-2.3) is exactly `pak` is loaded as an
  archive; if it cannot be opened, the engine stops with the fatal error `Couldn't load
  packfile: <argument>`. Any other argument is a directory, and it is not checked to exist.
- **FS-2.3** A `-path` argument's extension is the text after the first `.` anywhere in
  it, at most 7 bytes, compared case-sensitively. So `mod/pak0.pak` is an archive, but
  `./id1/pak0.pak` (extension `/id1/pa`) and `id1/PAK1.PAK` (extension `PAK`) are
  directories.
- **FS-2.4** With `-proghack`, a lookup of exactly `progs.dat` starts at the second
  element of the search path instead of the first. No other name is affected.
- **FS-2.5** QuakeC Studio does not implement `-cachedir` (a deliberate divergence). In
  id's engine it copies loose files from the search path into a local cache directory and
  reads the copy; it is off unless asked for on every platform (`sys_win.c` sets
  `parms.cachedir = NULL`, and `sys_linux.c` has "caching is disabled by default, use
  -cachedir to enable"), and it exists to speed up installs on network drives, which is
  not a property of game data.

### Test vectors

Same build and tree as FS-1.

| Options | Search path, head first | Lookups | Statements |
| --- | --- | --- | --- |
| `-path id1 mod/pak0.pak` | `mod/pak0.pak`, `id1`; game directory still `./id1` | `a.txt` = `id1-loose`; `e.txt` = `mod-pak0`; `f.txt` not found | FS-2.1, FS-2.2 |
| `-path ./id1/pak0.pak` | the directory `./id1/pak0.pak` | `a.txt` not found | FS-2.3 |
| `-path id1/PAK1.PAK` | the directory `id1/PAK1.PAK` | `a.txt` not found | FS-2.3 |
| `-path id1 +map start mod` | `id1` | `a.txt` = `id1-loose` | FS-2.1 |
| `-path nope.pak` | none | Error `Couldn't load packfile: nope.pak` | FS-2.2 |
| `-game mod -proghack` | as `-game mod` | `progs.dat` = `mod-progs` (the archive `mod/pak0.pak` is skipped, the directory `mod` is not) | FS-2.4 |

## FS-3 Finding a file

**Source:** `WinQuake/common.c`, `COM_FindFile`, `COM_OpenFile` and `COM_LoadFile`;
`WinQuake/sys_win.c`, `Sys_FileTime` and `findhandle`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

`COM_FindFile` walks the search path from the head and stops at the first element that
has the file. For a directory it builds `<directory>/<name>` with `sprintf` into a
`MAX_OSPATH` buffer and tests it with `Sys_FileTime`, which on Windows is "can `fopen`
open it for reading".

### Behaviour

- **FS-3.1** A lookup returns the file from the first element of the search path, from
  the head, that has it. Elements after that one are not consulted, and nothing is merged
  across elements.
- **FS-3.2** An archive element has a name if one of its entries matches by PAK-3.1.
- **FS-3.3** A directory element has a name if the file `<directory>/<name>` exists and
  can be opened for reading. The name is joined with a single `/` and not otherwise
  changed.
- **FS-3.4** In the shareware game (FS-4.2), directory elements are skipped for any name
  that contains `/` or `\`; archive elements are not affected. A loose `progs.dat` is still
  found; a loose `maps/start.bsp` is not.
- **FS-3.5** A name found nowhere is not an error at this level: the lookup reports no
  file, and the caller decides (`COM_LoadFile` returns `NULL`).
- **FS-3.6** A found file's size is the entry's `filelen` for an archive, or the file's
  length for a directory. `COM_LoadFile` returns its bytes followed by one NUL byte that is
  not counted in the size, so text files can be parsed as C strings.
- **FS-3.7** Names in directory elements, the archive names of FS-1.4 and the game
  directory names of FS-1.7 are matched ASCII case-insensitively on every OS. id's code
  leaves this to the host filesystem, so the answer differs by platform; this spec fixes
  the answer of the DOS and Windows reference builds, whose filesystems ignore case, so
  that an install copied from a CD (`ID1/PAK0.PAK`) or a mod made on Windows behaves the
  same on macOS and Linux. Archive entries stay case-sensitive (PAK-3.1).
- **FS-3.8** A name that is absolute (starts with `/` or `\`, or a drive letter and `:`),
  or that has a `..` path component, never matches a directory element (a deliberate
  divergence). id's code joins it unchanged, so it can read files outside the game
  directory; QuakeC Studio confines reads to the install. Such a name still matches an
  archive entry with exactly that name (PAK-3.1).
- **FS-3.9** QuakeC Studio puts no limit on path lengths (a deliberate divergence). id's
  engine builds search-path and archive paths in `MAX_OSPATH` (128-byte) buffers with
  `strcpy` and `sprintf`, which overflow, undefined, when the joined path is longer; the
  length depends on where the player installed the game, not on the game data.
- **FS-3.10** QuakeC Studio puts no limit on the number of archives (a deliberate
  divergence). On Windows, id's engine has `MAX_HANDLES` 10 file handles, of which
  `findhandle` uses slots 1 to 9; every loaded archive holds one for the whole session, and
  running out is the fatal error `out of handles`. That is a limit of one host's file
  layer, not of the format.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `FS_MAX_OSPATH` | 128 | `WinQuake/quakedef.h`, `MAX_OSPATH` used by `COM_FindFile` and `COM_AddGameDirectory` |
| `FS_MAX_HANDLES` | 10 | `WinQuake/sys_win.c`, `MAX_HANDLES` used by `findhandle` |

### Test vectors

Same build and tree as FS-1; FS-3.7 to FS-3.10 are QuakeC Studio's answers and have no id
vector.

| Options | Lookups | Statements |
| --- | --- | --- |
| none | `a.txt` = `id1-pak1` (`pak1.pak` before `pak0.pak` before the loose `id1/a.txt`) | FS-3.1, FS-3.2 |
| none | `b.txt` = `id1-loose-b`, size 11 (the file's length; the NUL `COM_LoadFile` appends is not counted) | FS-3.6 |
| none | `sub/c.txt` = `id1-loose-c`, 11 bytes | FS-3.3, FS-3.6 |
| none | `e.txt` not found, size -1 | FS-3.5 |
| shareware (`static_registered` 0) | `a.txt` = `id1-pak1`; `sub/c.txt` not found | FS-3.4 |

## FS-4 Registered and shareware

**Source:** `WinQuake/common.c`, `COM_CheckRegistered`, `COM_Init` and the `pop` table
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

`COM_Init` runs `COM_CheckRegistered` right after `COM_InitFilesystem`. `static_registered`
starts at 1 ("only for startup check, then set"), and `COM_CheckRegistered` opens
`gfx/pop.lmp` before it sets `static_registered` to 0, so the lookup itself is not
restricted by FS-3.4.

### Behaviour

- **FS-4.1** Once the search path is built, the engine looks up `gfx/pop.lmp` by FS-3,
  with the shareware restriction of FS-3.4 off, so a loose `id1/gfx/pop.lmp` counts.
- **FS-4.2** If `gfx/pop.lmp` is not found, the game is shareware. If it is found, its
  first 256 bytes, read as 128 big-endian 16-bit values, must equal the 128 values of the
  `pop` table in `common.c`; if they do, the game is registered, and if they do not, the
  engine stops with the fatal error `Corrupted data file.`. A `gfx/pop.lmp` shorter than
  256 bytes is the same error: id's code compares whatever the short read left in its
  uninitialised buffer, which is undefined, and this spec fixes the answer as the mismatch.
- **FS-4.3** A shareware game that is marked as modified (by PAK-4.2, `-game` or `-path`)
  stops with the fatal error `You must have the registered version to use modified games`.
- **FS-4.4** After the check, FS-3.4 applies to every lookup in a shareware game and to
  none in a registered one.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `FS_POP_PATH` | `gfx/pop.lmp` | `WinQuake/common.c`, `COM_CheckRegistered` |
| `FS_POP_SIZE` | 256 | `WinQuake/common.c`, `sizeof(check)` (128 `unsigned short`) in `COM_CheckRegistered` |
| `FS_POP_TABLE` | 128 16-bit values | `WinQuake/common.c`, `pop` |

### Test vectors

Same build and tree as FS-1: the shareware row of FS-3's table was run with
`static_registered` set to 0 after `COM_InitFilesystem`. FS-4.1 to FS-4.3 need a
`gfx/pop.lmp`; LibreQuake ships one under the GPL (`corpus/manifest.toml`), and the
filesystem's tests in [#42](https://github.com/quakecstudio/editor/issues/42) check it
against these statements.

On 2026-10-16 `qs-vfs` found LibreQuake's `gfx/pop.lmp` (256 bytes in `id1/pak1.pak`)
equal to id's table and reported the game as registered and modified (by PAK-4.2), with
`-basedir` set to the `librequake-lite` asset (`cargo run --release -p qs-vfs --example
walk -- -basedir <asset>`).

## FS-5 The 2021 rerelease's layout

**Source:** none in id's GPL code; the rerelease (KEX) engine is not open source. The
layout below is from these pages, read on 2026-10-16:

- QuakeSpasm, [`Quakespasm.txt`](https://raw.githubusercontent.com/sezero/quakespasm/master/Quakespasm.txt):
  "QuakeSpasm 0.94 has support for playing the 2021 re-release content: Copy the
  quakespasm binary to your rerelease installation and run as you normally do."
- vkQuake, [Play the Quake 2021 Re-release](https://vkquake.com/blog/vkquake-quake-2021-rerelease.html):
  "On Steam, the re-release sits in a `rerelease` folder inside the main Quake directory
  ... That folder has its own `id1` with an updated `pak0.pak`, plus one folder per
  expansion", listing `hipnotic`, `rogue`, `dopa` and `mg1`, each run with `-game`.
- Quakestarter, [Pak Files](https://neogeographica.com/quakestarter_html/other_stuff/pak_files.html):
  "the “id1” folder in the rerelease only contains “pak0.pak” and does not have a
  “pak1.pak”", and, as of April 2022, Steam and GOG sell the original and the rerelease,
  while Xbox Game Pass, the Epic Games Store and the closed Bethesda store sold only the
  rerelease.
- QuakeSpasm commit [3af9ad4](https://code.idtech.space/fte/quakespasm/commit/3af9ad4e044b7d0692e24964872197d4e81d641b),
  "2021 rerelease support: read the localization file from QuakeEX.kpf", whose README
  change calls `QuakeEX.kpf` "actually a zip file" and opens it as
  `<basedir>/QuakeEX.kpf`.
- Steam guide [[2021 Re-Release] Make your own mod](https://steamcommunity.com/sharedfiles/filedetails/?id=2635180062):
  the rerelease also treats folders in `%userprofile%\saved games\Nightdive Studios\Quake\`
  as game directories.

None of this was checked against an install for this spec; the tests of
[#42](https://github.com/quakecstudio/editor/issues/42) and
[#43](https://github.com/quakecstudio/editor/issues/43) confirm it on real installs.

### Behaviour

- **FS-5.1** A rerelease install is a base directory of its own: on Steam it is the
  `rerelease` folder inside the original install, and elsewhere (GOG's Quake Enhanced, Epic,
  Game Pass) it is the game's install folder. Its `id1` holds `pak0.pak` and no `pak1.pak`.
- **FS-5.2** The rerelease's game directories (`id1`, `hipnotic`, `rogue`, `dopa`, `mg1`
  and any others it ships) are searched by FS-1 and FS-3 unchanged, with the rerelease
  folder as the base directory and the expansion chosen by `-game`, as QuakeSpasm and
  vkQuake run them.
- **FS-5.3** `QuakeEX.kpf` in the rerelease base directory is a zip archive (it holds the
  localization strings, such as `localization/loc_english.txt`, that the rerelease's
  progs and maps refer to). It is not an element of the search path: FS-3 never finds a
  file inside it.
- **FS-5.4** QuakeC Studio does not model the KEX engine's per-user game directories
  (`Saved Games\Nightdive Studios\Quake`); the rerelease engine is a compile target only
  (ENG-8), and the search path is the one id's engine builds.
