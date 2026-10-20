# PAK archives

Prefix: `PAK` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the PAK archive format and how the engine reads it, as implemented by
`WinQuake/common.c` and `WinQuake/crc.c` in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`. Where PAK files sit on the search path is
[`filesystem.md`](filesystem.md). Implemented by `crates/qs-pak`
([#42](https://github.com/quakecstudio/editor/issues/42)).

All integers in a PAK file are 32-bit two's-complement signed little-endian values: the
engine reads them through `LittleLong` into C `int` fields. Offsets are from the start of
the file.

## PAK-1 Header

**Source:** `WinQuake/common.c`, `COM_LoadPackFile` and the `dpackheader_t` struct
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

`COM_LoadPackFile` reads the header with one unchecked `Sys_FileRead` and checks only the
four identification bytes.

| Offset | Size | Type | Field | Meaning |
| --- | --- | --- | --- | --- |
| 0 | 4 | `char[4]` | `id` | `PACK` |
| 4 | 4 | `int` | `dirofs` | Offset of the directory |
| 8 | 4 | `int` | `dirlen` | Length of the directory in bytes |

### Behaviour

- **PAK-1.1** A PAK file starts with a 12-byte header: the four bytes `PACK` (0x50 0x41
  0x43 0x4B), then `dirofs` and `dirlen` as signed 32-bit little-endian integers.
- **PAK-1.2** If the first four bytes are not exactly `PACK` (the comparison is
  case-sensitive, so `pack` fails), loading fails with the error `<path> is not a
  packfile`. In id's engine this is a fatal `Sys_Error`.
- **PAK-1.3** A file of fewer than 12 bytes fails to load. id's code does not check the
  length of the read and decodes whatever was in the uninitialised header struct, which is
  undefined; this spec fixes the answer as an error, the same as PAK-1.2, because no valid
  archive is that short.
- **PAK-1.4** A PAK path that cannot be opened is not an error of the format:
  `COM_LoadPackFile` returns `NULL` and its caller decides what that means (FS-1.5, FS-2.2).

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `PAK_MAGIC` | `PACK` | `WinQuake/common.c`, the `header.id` test in `COM_LoadPackFile` |
| `PAK_HEADER_SIZE` | 12 | `WinQuake/common.c`, `sizeof(dpackheader_t)` |

### Test vectors

See the table in PAK-4.

## PAK-2 Directory

**Source:** `WinQuake/common.c`, `COM_LoadPackFile`, the `dpackfile_t` and `packfile_t`
structs and `MAX_FILES_IN_PACK`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

The directory is an array of 64-byte entries at `dirofs`. The engine computes the number of
entries as `header.dirlen / sizeof(dpackfile_t)`, reads `dirlen` bytes into a stack array of
`MAX_FILES_IN_PACK` entries, and copies each name with `strcpy` into a `MAX_QPATH` (64-byte)
buffer. Because `sizeof` has the unsigned type `size_t`, the division is unsigned; on the
32-bit x86 reference build `size_t` is 32 bits wide.

| Offset in entry | Size | Type | Field | Meaning |
| --- | --- | --- | --- | --- |
| 0 | 56 | `char[56]` | `name` | Path inside the archive, NUL-terminated |
| 56 | 4 | `int` | `filepos` | Offset of the file's data from the start of the PAK |
| 60 | 4 | `int` | `filelen` | Length of the file's data in bytes |

### Behaviour

- **PAK-2.1** The directory is `dirlen` bytes at offset `dirofs`, holding 64-byte entries:
  a 56-byte name, then `filepos` and `filelen` as signed 32-bit little-endian integers.
- **PAK-2.2** The number of entries is `dirlen` divided by 64, rounded down. The remaining
  `dirlen % 64` bytes are not an entry; they are still read, and they count towards the CRC
  of PAK-4.1.
- **PAK-2.3** A `dirlen` from 0 to 63 gives an archive with no entries, which loads
  without error.
- **PAK-2.4** More than 2048 entries fails with the error `<path> has <n> files` (fatal in
  id's engine), where `<n>` is the computed entry count. Exactly 2048 entries loads.
- **PAK-2.5** A negative `dirlen` fails as PAK-2.4. The engine divides `dirlen` as an
  unsigned 32-bit value on the reference build, so any negative `dirlen` gives at least
  33,554,432 entries; `dirlen` = -64 gives `has 67108863 files`. (A 64-bit build gets a
  different, also failing, result; the 32-bit answer is the one fixed here.)
- **PAK-2.6** A `dirlen` from 131,073 to 131,135 (2048 whole entries plus part of
  another) fails to load. id's code computes 2048 entries, passes the limit check and then
  reads `dirlen` bytes into a 131,072-byte stack array, which is undefined; this spec fixes
  the answer as an error because the C overflows its buffer.
- **PAK-2.7** A directory that does not lie wholly inside the file (a negative `dirofs`,
  or `dirofs + dirlen` past the end) fails to load. id's code does not check the seek or
  the length of the read and parses uninitialised stack memory, which is undefined; this
  spec fixes the answer as an error.
- **PAK-2.8** An entry's name is the bytes of its 56-byte field before the first NUL.
  Bytes after that NUL are ignored.
- **PAK-2.9** An entry whose 56-byte name field contains no NUL fails the load. id's
  `strcpy` runs on into `filepos`, `filelen` and the next entries, which is undefined; this
  spec fixes the answer as an error because no tool writes such a name and no single answer
  matches the C.
- **PAK-2.10** Names are kept as raw bytes: no case folding, no conversion between `/` and
  `\`, no trimming. `MAPS/E1M1.BSP` and `maps/e1m1.bsp` are two different names. An entry
  whose first name byte is NUL has the empty name.
- **PAK-2.11** Entries keep their directory order, and two entries may have the same name;
  loading neither merges nor rejects them.
- **PAK-2.12** `filepos` and `filelen` are not checked when the archive is loaded, so a
  bad range never stops an archive from loading or its directory from being listed
  (PAK-3.4 covers reading such an entry).

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `PAK_ENTRY_SIZE` | 64 | `WinQuake/common.c`, `sizeof(dpackfile_t)` |
| `PAK_NAME_SIZE` | 56 | `WinQuake/common.c`, `dpackfile_t.name` |
| `PAK_MAX_FILES` | 2048 | `WinQuake/common.c`, `MAX_FILES_IN_PACK` used by `COM_LoadPackFile` |

### Test vectors

See the table in PAK-4.

## PAK-3 Finding and reading an entry

**Source:** `WinQuake/common.c`, `COM_FindFile` and `COM_LoadFile`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

`COM_FindFile` walks each archive's entries in order and compares names with `strcmp`. On
a match it seeks to `filepos` and reports `filelen` as the size; `COM_LoadFile` then
allocates `filelen + 1` bytes and reads `filelen` bytes without checking the result.

### Behaviour

- **PAK-3.1** Looking up a name in an archive returns the first entry, in directory order,
  whose name is byte-for-byte equal to it. The comparison is case-sensitive and does not
  normalise separators.
- **PAK-3.2** An entry's contents are the `filelen` bytes starting at offset `filepos` of
  the PAK file. Entries may overlap each other, the header or the directory; nothing
  checks it.
- **PAK-3.3** An entry with `filelen` 0 has empty contents.
- **PAK-3.4** Reading an entry whose range does not lie wholly inside the file (a negative
  `filepos` or `filelen`, or `filepos + filelen` past the end) is an error. id's code
  either fails in `Hunk_AllocName` (`Hunk_Alloc: bad size` for `filelen` below -1), writes
  before its buffer (`filelen` = -1) or returns the short read's uninitialised bytes; this
  spec fixes the answer as an error. The entry is still listed (PAK-2.12).

### Test vectors

See the table in PAK-4.

## PAK-4 The modified-game check

**Source:** `WinQuake/common.c`, `COM_LoadPackFile`, `PAK0_COUNT` and `PAK0_CRC`;
`WinQuake/crc.c`, `CRC_Init` and `CRC_ProcessByte`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

Every archive the engine loads is compared with the original `id1/pak0.pak` by its entry
count and a CRC of its directory. A mismatch sets `com_modified` ("set true if using non-id
files"), which only matters to the shareware check in FS-4.3. `crc.c` describes its CRC as
"a 16 bit, non-reflected CRC using the polynomial 0x1021 and the initial and final xor
values shown below... in other words, the CCITT standard CRC used by XMODEM", with
`CRC_INIT_VALUE` 0xffff and `CRC_XOR_VALUE` 0x0000.

### Behaviour

- **PAK-4.1** The directory CRC is CRC-16 with polynomial 0x1021, not reflected, initial
  value 0xFFFF and no final XOR, computed over all `dirlen` bytes of the directory as they
  are in the file (before byte swapping, and including any partial entry from PAK-2.2).
  The CRC of an empty directory is 0xFFFF (65535).
- **PAK-4.2** An archive whose entry count is not 339 or whose directory CRC is not 32981
  marks the game as modified. The check runs on every archive loaded, not only `pak0.pak`,
  and it never stops an archive from loading.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `PAK_PAK0_COUNT` | 339 | `WinQuake/common.c`, `PAK0_COUNT` used by `COM_LoadPackFile` |
| `PAK_PAK0_CRC` | 32981 | `WinQuake/common.c`, `PAK0_CRC` used by `COM_LoadPackFile` |
| `PAK_CRC_INIT` | 0xFFFF | `WinQuake/crc.c`, `CRC_INIT_VALUE` used by `CRC_Init` |
| `PAK_CRC_POLY` | 0x1021 | `WinQuake/crc.c`, the comment above `crctable` |

### Test vectors

Checked on 2026-10-16 against id's code: `COM_LoadPackFile` and the `dpackheader_t`,
`dpackfile_t`, `packfile_t` and `pack_t` definitions, copied unchanged from `common.c` at
the pinned commit together with `crctable`, `CRC_Init` and `CRC_ProcessByte` from `crc.c`,
built with the system C compiler (64-bit macOS) with `Sys_File*` on `stdio`, `Hunk_Alloc`
on `calloc` and `Sys_Error` printing its message. Each vector is a PAK built by a script;
entries are written in the order shown, data first and directory last.

| Input | Result | Statements |
| --- | --- | --- |
| `dirlen` 0 | Loads; 0 entries; CRC 65535; modified | PAK-2.3, PAK-4.1, PAK-4.2 |
| `a.txt` (`AB`), `b.txt` (empty), then 10 extra directory bytes, each `x` (`dirlen` 138) | 2 entries: `a.txt` at 12 length 2, `b.txt` at 14 length 0; CRC 29531 | PAK-2.1, PAK-2.2, PAK-3.3, PAK-4.1 |
| Two entries named `a.txt` (`first`, `second`) | Both listed, in that order; a lookup returns `first` | PAK-2.11, PAK-3.1 |
| Magic `pack` | Error `... is not a packfile` | PAK-1.2 |
| 2048 empty entries | Loads; 2048 entries | PAK-2.4 |
| 2049 empty entries | Error `... has 2049 files` | PAK-2.4 |
| One entry named `abc` NUL `garbage` | 1 entry named `abc` | PAK-2.8 |
| `MAPS/E1M1.BSP` (`Q`) and `maps/e1m1.bsp` (`q`) | 2 entries, names unchanged | PAK-2.10, PAK-3.1 |
| `dirlen` -64 | Error. The 64-bit build failed with `Hunk_Alloc: bad size: -72`; the 32-bit reference computes 67,108,863 entries and fails PAK-2.4 | PAK-2.5 |

The same build listed and read every entry of LibreQuake's `lite` release (the
`librequake-lite` test asset in `corpus/manifest.toml`): `id1/pak0.pak` (SHA-256
`0dd895a4...29378ff`) has 353 entries, directory CRC 34223, and `id1/pak1.pak`
(`28423c01...0e39365`) has 60 entries, CRC 47719; both are marked modified. A reader
written from this spec alone produced the same names, offsets, lengths, CRCs and content
hashes for all 413 entries. id's own paks are game data and are not used here (LIC-4).

`qs-pak`'s unit tests check the behaviour of every row of the table above, and the `qs-vfs` example
`walk` (`cargo run --release -p qs-vfs --example walk -- -basedir <install>`) reported the
same entry counts and CRCs for LibreQuake's two paks on 2026-10-16, reading all 413 entries.

## PAK-5 The 2021 rerelease's archives

**Source:** none in id's GPL code; the rerelease engine is not open source. Evidence for
the format is QuakeSpasm's documentation and loader at
sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`, the commit
[`bsp.md`](bsp.md) cites, read on 2026-10-16 and checked at that commit on 2026-10-20:
[`Quakespasm.txt`](https://raw.githubusercontent.com/sezero/quakespasm/c9c8ee9895961669c4674e8f8ca8c64cc43e38b5/Quakespasm.txt)
and [`Quake/common.c`](https://raw.githubusercontent.com/sezero/quakespasm/c9c8ee9895961669c4674e8f8ca8c64cc43e38b5/Quake/common.c).

QuakeSpasm's loader is id's `COM_LoadPackFile` with the same `MAX_FILES_IN_PACK 2048`
limit, and its documentation says "QuakeSpasm 0.94 has support for playing the 2021
re-release content: Copy the quakespasm binary to your rerelease installation and run as
you normally do." The rerelease's `pak0.pak` files therefore load under PAK-1 to PAK-3.

### Behaviour

- **PAK-5.1** The rerelease's `pak0.pak` archives (in `id1`, `hipnotic`, `rogue`, `dopa`,
  `mg1` and the other game directories of FS-5.2) are PAK archives as described by PAK-1
  to PAK-3, each with at most 2048 entries, and need no extension of the format. This
  statement rests on QuakeSpasm's evidence above; it is confirmed against a real install
  by the reader's acceptance test in
  [#42](https://github.com/quakecstudio/editor/issues/42), because no rerelease data is
  available to CI (LIC-4).
