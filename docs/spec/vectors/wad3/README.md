# Map WAD and WAD3 vectors

Vectors for [`../../wad3.md`](../../wad3.md). None holds Valve's game data: every texture is 16
by 16 of one index, every palette a few bytes written by hand or the generated full one
(colour n is n, 255 - n, n / 2). The factories build them in tests:
`crates/qs-wad/src/map_wad/tests.rs` for the reader's statements (WAD3-1 to WAD3-3) and
`crates/qs-bsp/src/valve_tests.rs`, `wad_file` and `wad3_files`, for a BSP's external textures
(WAD3-4). Each WAD has its lumps after the header and its directory last; each lump is a
40-byte `miptex_t`, the four mip levels' 340 bytes and the palette.

## External textures (WAD3-4)

Each is a version 30 BSP (the BSP vector `version-30`, BSP-22) whose textures are external
(every offset 0, no pixels) and whose world's `wad` key is given, beside the WAD files it
names, by their place on the search path. The result is each texture QuakeSpasm uses: its name,
its pixels' count and first byte, and its palette's length.

| Name | `wad` key | WAD files | Result | Statements |
| --- | --- | --- | --- | --- |
| `wad3-full` | `\\half-life\\valve\\brick.wad` | `brick.wad`: WAD3, `BRICK` of index 9, type 67, a full 256-colour palette | `BRICK`, 256 pixels of 9, palette 770 bytes | WAD3-3.3, WAD3-3.6, WAD3-4.2 |
| `wad3-few` | `few.wad` | `few.wad`: WAD3, `few` of 3, two colours, then 2 bytes more | `few`, 256 of 3, palette 8 bytes | WAD3-3.3 |
| `wad3-past` | `past.wad` | `past.wad`: WAD3, `past` of 5, a count of 256 and 6 bytes of colours | `past`, 256 of 5, palette 40 bytes: cut to the lump's size less the mip levels, 48, and read to the end of the file, 40 | WAD3-3.4 |
| `wad3-truncated` | `trunc.wad` | `trunc.wad`: WAD3, `a` of 1 and `b` of 2, its last 20 bytes cut off | `a`, 256 of 1, palette 2 bytes; `b` the BSP's own, no pixels | WAD3-2.4 |
| `wad3-bad-id` | `badid.wad` | `badid.wad`: starts `WAD4` | `wall` the BSP's own, no pixels | WAD3-1.1 |
| `wad3-gfx` | `C:/maps/gfxonly.wad` | `gfx/gfxonly.wad`: WAD3, `wall` of 6, one colour | `wall`, 256 of 6, palette 5 bytes | WAD3-4.2 |
| `wad3-order` | `first.wad;second.wad` | `first.wad` and `second.wad`: WAD3, `wall` of 1 and of 2 | `wall` of 2: the last named is searched first | WAD3-4.2, WAD3-4.3 |
| `wad2-plain` | `plain.wad` | `plain.wad`: WAD2, `wall` of 8, type 68 | `wall`, 256 of 8, no palette | WAD3-3.2 |

**Checked:** on 2026-11-11 each BSP above, with its WAD files written beside it, was loaded by
QuakeSpasm's `Mod_Init`, `Mod_ForName` and the functions they call, in `Quake/gl_model.c` and
`Quake/wad.c` copied unchanged with every header of `Quake/` from
sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5` (the tarball of the BSP vectors'
README, SHA-256 `139d85c096ebec6b62425584be570daa5830323ff6eac6cd7ed94400bec3deba`), with
`COM_ParseEx`, `COM_Parse`, `COM_FileBase`, `COM_FileGetExtension`, `COM_AddExtension`,
`COM_SkipPath`, `COM_StripExtension`, `q_strdup`, `FS_fread`, `FS_fseek` and `FS_fclose`
copied unchanged from its `Quake/common.c`. It was built with
`clang -O1 -w -U_FORTIFY_SOURCE -D_FORTIFY_SOURCE=0 $(sdl2-config --cflags)` (Apple clang
21.0.0, arm64, SDL2's headers from Homebrew). The harness is the one the BSP vectors' README
describes for version 30, with three changes:

- `COM_FOpenFile` opens a file of the working directory, the search path, so the WADs the key
  names are found as `<name>.wad` or `gfx/<name>.wad` (WAD3-4.2).
- `Cvar_RegisterVariable` sets a cvar's value from its default string, as QuakeSpasm's does, so
  `external_textures` is 1.
- `sv.modelname` is the map's name, so the map is the world (WAD3-4.1).

Besides `memcpy` the harness defines `fread`, logging each read's destination and size,
because QuakeSpasm reads a WAD texture's pixels and palette with `FS_fread`. A texture's
pixels are what was written to `(byte *)(tx + 1)`, and its palette what was written right
after them, within the texture's own hunk allocation (`sizeof (texture_t) + tex_bytes`).

It printed, for each texture, its name, size, whether `Sky_LoadTexture` took it, its pixels'
count and first 8 bytes and its palette's bytes. `qs-bsp` read each BSP with
`Bsp::read_with_wads`, after `wads::wad_key` and `wads::wad_files` named the WADs and
`qs_wad::MapWad::read` read them, and printed the same fields. The two were identical for every
vector, leaving out QuakeSpasm's two placeholder textures as the BSP vectors' README does. The
same build also loaded the seven version 30 BSP vectors again, with `external_textures` on,
and printed what it printed before. QuakeSpasm's console said `badid.wad is not a valid WAD`
and `Missing texture wall in WAD, using BSP.` for `wad3-bad-id`.

## The reader (WAD3-1 to WAD3-3)

`crates/qs-wad/src/map_wad/tests.rs` builds each case from the factory and checks it: `WAD2`
and `WAD3` read and `WAD4`, `wad3`, `PACK` and a header of 11 bytes fail (WAD3-1.1); a negative
lump count or directory offset and no lumps (WAD3-1.2); `gfx.wad`'s reader failing a WAD3
(WAD3-1.3); the directory and its cleaned names (WAD3-2.1, WAD3-2.2); each of `W_AddWadFile`'s
fixes (WAD3-2.3); a cut directory (WAD3-2.4); the first entry of a name (WAD3-3.1); the types
and sizes that are textures (WAD3-3.2); the full and two-colour palettes (WAD3-3.3); a palette
past the lump (WAD3-3.4); a lump before the file and a header past it (WAD3-3.5); and the
header's own name (WAD3-3.6).
