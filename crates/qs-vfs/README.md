# qs-vfs

The game filesystem: the search path of game directories and PAK archives id's engine builds
from a base directory and its command line, and file lookup on it. `Vfs::new` is
`COM_InitFilesystem` followed by `COM_CheckRegistered`, `Vfs::find` is `COM_FindFile`,
`Vfs::load` is `COM_LoadFile`, and `Vfs::load_wad` finds and checks `gfx.wad` as
`W_LoadWadFile` does. `Vfs::files` lists every file of every element, so an install can be
walked. The host filesystem is behind the `HostFs` trait: `StdFs` is the real one, and the
tests use a tree held in memory. Ported from `WinQuake/common.c` and `wad.c` at the commit
pinned in [`docs/porting.md`](../../docs/porting.md).

QuakeC Studio's answers where id's code leaves the result to the host or overflows a buffer
are spec statements: names in directories match ASCII case-insensitively on every OS
(FS-3.7), loose lookups never leave the install (FS-3.8), and there is no limit on path
lengths or the number of archives (FS-3.9, FS-3.10). `-cachedir` is not implemented (FS-2.5).

- **Layer:** L0 formats. It depends on `qs-pak` and `qs-wad`. Finding the user's install is
  [#43](https://github.com/quakecstudio/editor/issues/43).
- **Spec sections:** FS-1 to FS-5 in [`filesystem.md`](../../docs/spec/filesystem.md),
  PAK-1.4 in [`pak.md`](../../docs/spec/pak.md) and WAD-1.4 in
  [`wad2.md`](../../docs/spec/wad2.md).
- **Shipped:** yes.

## Walking an install

The `walk` example builds the search path from the engine's arguments, then lists and reads
every file on it and prints a digest. It also decodes every `.spr`, `.mdl` and MD5 model with
`qs-mdl`, every `.wav` with `qs-wav`, and every `.bsp` with `qs-bsp`, with its entity text
read and written back by `qs-map`, and prints a `failed:` line for each file it cannot read
or decode. A BSP that a lookup reaches has its `.lit` chosen as QuakeSpasm's loader chooses
it (BSP-18); a `.lit` QuakeSpasm ignores is a `warning:` line, since the map still loads:

```bash
cargo run --release -p qs-vfs --example walk -- -basedir <install> [-game <dir>]
```

On LibreQuake's `lite` release (the `librequake-lite` test asset), 2026-10-16:

```
registered: true, modified: true
archive <asset>/id1/pak1.pak: 60 entries, directory CRC 47719, modified true
archive <asset>/id1/pak0.pak: 353 entries, directory CRC 34223, modified true
directory <asset>/id1
gfx.wad: 149 lumps, conchars at Ok(23996)
decoded 3 sprites, 226 sounds, 0 MD5 meshes, 0 MD5 animations, 81 alias models, 23 BSPs, 8 .lit files (0 from a game directory of lower priority, ignored)
read 424 files, 99848012 bytes, digest ae8822040b6eab21
```
