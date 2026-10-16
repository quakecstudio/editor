# qs-pak

Reads PAK archives, the files Quake keeps its game data in. `Pak::read` loads an archive held
in memory and `Pak::read_from` one in a seekable file, reading only the header and directory;
both are id's `COM_LoadPackFile` with the bounds checks id's engine leaves out, and reject
malformed input with a typed `PakError`, never a panic. `Pak::find` is the archive half of
`COM_FindFile`, and `entry_data` and `read_entry` return an entry's bytes. Ported from
`WinQuake/common.c` at the commit pinned in [`docs/porting.md`](../../docs/porting.md); the
directory CRC is `qc-progs`'s port of `crc.c`.

- **Layer:** L0 formats. It depends on `qc-progs` for the CRC; `qs-vfs` builds on it.
- **Spec sections:** PAK-1 to PAK-5 in [`pak.md`](../../docs/spec/pak.md). PAK-1.4 (an
  archive that cannot be opened) is the filesystem's and is tested in `qs-vfs`.
- **Shipped:** yes.

## Invariants

Property tests in `src/tests.rs` and the fuzz target `fuzz/fuzz_targets/qs_pak.rs`
(`cargo xtask fuzz qs-pak`):

- Arbitrary bytes never panic either loader, and the two loaders agree (PAK-1, PAK-2).
- Every entry reads the same from memory and through a seekable file (PAK-3.2).
- An archive built from any names and data reads back with the same names and data, in order.
