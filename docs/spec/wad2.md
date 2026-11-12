# WAD2 archives

Prefix: `WAD` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the WAD2 lump archive and how the engine reads it, as implemented by
`WinQuake/wad.c` and `WinQuake/wad.h` in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`. The engine reads one WAD2 file, `gfx.wad`,
for its 2D graphics; map editors and compilers use WAD2 files of textures (plan requirements
MAP-5 and MAP-23 in [`plan/requirements.yaml`](../../plan/requirements.yaml), not sections of
[`map.md`](map.md)). Implemented by `crates/qs-wad`
([#42](https://github.com/quakecstudio/editor/issues/42)).

All integers in a WAD2 file are 32-bit two's-complement signed little-endian values read
through `LittleLong`, and offsets are from the start of the file.

## WAD-1 Header and loading

**Source:** `WinQuake/wad.c`, `W_LoadWadFile`; `WinQuake/wad.h`, `wadinfo_t`;
`WinQuake/host.c`, `Host_Init` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

`Host_Init` calls `W_LoadWadFile ("gfx.wad")`, which loads the whole file into the hunk
through the game filesystem (`COM_LoadHunkFile`) and then checks the identification
bytes.

| Offset | Size | Type | Field | Meaning |
| --- | --- | --- | --- | --- |
| 0 | 4 | `char[4]` | `identification` | `WAD2` |
| 4 | 4 | `int` | `numlumps` | Number of directory entries |
| 8 | 4 | `int` | `infotableofs` | Offset of the directory |

### Behaviour

- **WAD-1.1** A WAD2 file starts with a 12-byte header: the four bytes `WAD2` (0x57 0x41
  0x44 0x32), then `numlumps` and `infotableofs` as signed 32-bit little-endian integers.
- **WAD-1.2** If the first four bytes are not exactly `WAD2`, loading fails with the error
  `Wad file <name> doesn't have WAD2 id` (fatal in id's engine). `wad.h` comments the field
  "should be WAD2 or 2DAW", but the code accepts only `WAD2`; `WAD3` and `2DAW` fail.
- **WAD-1.3** A file of fewer than 12 bytes fails to load. id's code reads the header from
  the loaded buffer without checking its length, which is undefined; this spec fixes the
  answer as an error.
- **WAD-1.4** The engine finds `gfx.wad` through the game filesystem (FS-3), so a mod can
  replace it; if it is not found, the engine stops with the fatal error `W_LoadWadFile:
  couldn't load gfx.wad`.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `WAD_MAGIC` | `WAD2` | `WinQuake/wad.c`, the `identification` test in `W_LoadWadFile` |
| `WAD_HEADER_SIZE` | 12 | `WinQuake/wad.h`, `sizeof(wadinfo_t)` |
| `WAD_GFX_NAME` | `gfx.wad` | `WinQuake/host.c`, `Host_Init` |

### Test vectors

See the table in WAD-3.

## WAD-2 Directory

**Source:** `WinQuake/wad.c`, `W_LoadWadFile` and `W_CleanupName`; `WinQuake/wad.h`,
`lumpinfo_t` and the `TYP_` and `CMP_` constants
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

`W_LoadWadFile` walks `numlumps` 32-bit entries at `infotableofs` in place, byte-swaps
their integers, and rewrites each name with `W_CleanupName`. Its loop counter is
`unsigned`, so the comparison with the `int` `wad_numlumps` is unsigned. Nothing in the
directory is bounds-checked.

| Offset in entry | Size | Type | Field | Meaning |
| --- | --- | --- | --- | --- |
| 0 | 4 | `int` | `filepos` | Offset of the lump's data from the start of the WAD |
| 4 | 4 | `int` | `disksize` | Bytes of the lump as stored |
| 8 | 4 | `int` | `size` | Bytes of the lump uncompressed |
| 12 | 1 | `char` | `type` | Lump type (`TYP_` constants) |
| 13 | 1 | `char` | `compression` | Compression (`CMP_` constants) |
| 14 | 2 | `char[2]` | `pad1`, `pad2` | Unused |
| 16 | 16 | `char[16]` | `name` | Lump name, "must be null terminated" |

### Behaviour

- **WAD-2.1** The directory is `numlumps` 32-byte entries at offset `infotableofs`:
  `filepos`, `disksize` and `size` as signed 32-bit little-endian integers, then the
  `type` and `compression` bytes, two unused bytes and a 16-byte name.
- **WAD-2.2** `numlumps` 0 gives an archive with no lumps, which loads without error.
- **WAD-2.3** A negative `numlumps` fails to load. id's loop compares an unsigned counter
  with it, so it walks about four billion entries past the buffer, which is undefined; this
  spec fixes the answer as an error.
- **WAD-2.4** A directory that does not lie wholly inside the file (a negative
  `infotableofs`, or `infotableofs + 32 * numlumps` past the end) fails to load. id's code
  does not check it, which is undefined; this spec fixes the answer as an error.
- **WAD-2.5** Each lump name is cleaned when the archive loads (`W_CleanupName`): the bytes
  of the 16-byte field up to the first NUL are kept, ASCII `A` to `Z` become `a` to `z`,
  every other byte (including 0x80 to 0xFF) is unchanged, and the rest of the field is
  filled with NUL. The comment above `W_CleanupName` says it pads with spaces; the code pads
  with NUL. Bytes after the first NUL are discarded.
- **WAD-2.6** A 16-byte name field with no NUL keeps all 16 bytes as the name. id's
  `strcmp` on such a name reads past the field, which is undefined; this spec fixes the
  answer as a 16-byte name.
- **WAD-2.7** `type`, `compression` and `disksize` are not checked. The engine uses `type`
  only to byte-swap the width and height of `TYP_QPIC` (66) lumps, which changes nothing
  on little-endian hosts. `compression` is ignored: `CMP_LZSS` (1) is defined in `wad.h`
  but no WinQuake code decompresses a lump, so a lump's bytes are always returned as
  stored.
- **WAD-2.8** `filepos`, `disksize` and `size` are not checked when the archive loads, so
  a bad range never stops an archive from loading or its directory from being listed.
- **WAD-2.9** Lumps keep their directory order, and two lumps may have the same cleaned
  name.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `WAD_ENTRY_SIZE` | 32 | `WinQuake/wad.h`, `sizeof(lumpinfo_t)` |
| `WAD_NAME_SIZE` | 16 | `WinQuake/wad.h`, `lumpinfo_t.name`; `WinQuake/wad.c`, the loop bound in `W_CleanupName` |
| `WAD_TYP_NONE` | 0 | `WinQuake/wad.h`, `TYP_NONE` |
| `WAD_TYP_LABEL` | 1 | `WinQuake/wad.h`, `TYP_LABEL` |
| `WAD_TYP_PALETTE` | 64 | `WinQuake/wad.h`, `TYP_PALETTE` (also `TYP_LUMPY`, "64 + grab command number") |
| `WAD_TYP_QTEX` | 65 | `WinQuake/wad.h`, `TYP_QTEX` |
| `WAD_TYP_QPIC` | 66 | `WinQuake/wad.h`, `TYP_QPIC`, tested in `W_LoadWadFile` |
| `WAD_TYP_SOUND` | 67 | `WinQuake/wad.h`, `TYP_SOUND` |
| `WAD_TYP_MIPTEX` | 68 | `WinQuake/wad.h`, `TYP_MIPTEX` |
| `WAD_CMP_NONE` | 0 | `WinQuake/wad.h`, `CMP_NONE` |
| `WAD_CMP_LZSS` | 1 | `WinQuake/wad.h`, `CMP_LZSS` |

### Test vectors

See the table in WAD-3.

## WAD-3 Finding and reading a lump

**Source:** `WinQuake/wad.c`, `W_GetLumpinfo`, `W_GetLumpName` and `W_GetLumpNum`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

`W_GetLumpinfo` cleans the requested name into a 16-byte buffer with `W_CleanupName` and
returns the first entry whose cleaned name is equal by `strcmp`. `W_GetLumpName` returns a
pointer to `filepos` in the loaded file; the engine reads the lump's structure from there
and uses neither `disksize` nor `size`.

### Behaviour

- **WAD-3.1** A lookup cleans the requested name by the rule of WAD-2.5 and returns the
  first lump, in directory order, whose cleaned name equals it. Lookups therefore ignore
  ASCII case (`CoNcHaRs` finds `CONCHARS`) but not the case of bytes 0x80 to 0xFF.
- **WAD-3.2** Only the first 16 bytes of the requested name count. A requested name of 16
  or more bytes is compared as its first 16 bytes; id's code compares an unterminated
  buffer there, which is undefined, and this spec fixes the answer by WAD-2.6.
- **WAD-3.3** A name that matches no lump is the error `W_GetLumpinfo: <name> not found`
  (fatal in id's engine).
- **WAD-3.4** A lump's data starts at `filepos`. QuakeC Studio reads a lump as the
  `disksize` bytes at `filepos` (the bytes stored in the file; the engine itself never
  needs a length), and reading a lump whose range does not lie wholly inside the file is an
  error. The lump is still listed (WAD-2.8).
- **WAD-3.5** Looking up a lump by index accepts 0 to `numlumps - 1`. id's
  `W_GetLumpNum` accepts `numlumps` too (its test is `num > wad_numlumps`), which reads one
  entry past the directory; this spec fixes `numlumps` as an error. `W_GetLumpNum` has no
  caller in WinQuake (GitHub code search of `id-Software/Quake`, 2026-10-16, finds it only
  in `wad.c` and `wad.h`).

### Test vectors

Checked on 2026-10-16 against id's code: `W_CleanupName`, `W_LoadWadFile` and
`W_GetLumpinfo` and the `wad.h` types, copied unchanged from the pinned commit and built
with the system C compiler, with `COM_LoadHunkFile` returning the file's bytes and
`Sys_Error` printing its message. Each vector is a WAD2 built by a script.

| Input | Result | Statements |
| --- | --- | --- |
| `numlumps` 0 | Loads; 0 lumps | WAD-2.2 |
| Magic `WAD3` | Error `Wad file ... doesn't have WAD2 id` | WAD-1.2 |
| Lumps `CONCHARS` (type 68), `Disc` NUL `junk` (type 66), `x`, `x`, 0xC9 `t` 0xC9 | Listed as `conchars`, `disc`, `x`, `x`, 0xC9 `t` 0xC9, in that order | WAD-2.1, WAD-2.5, WAD-2.9 |
| The same; look up `conchars`, `CoNcHaRs`, `DISC` | `conchars`, `conchars`, `disc` | WAD-3.1 |
| The same; look up `x` | The first `x` (offset 23, not 24) | WAD-3.1 |
| The same; look up 0xC9 `T` 0xC9 and 0xC9 `t` 0xC9 | Both find 0xC9 `t` 0xC9 | WAD-2.5, WAD-3.1 |
| One lump `a`; look up `b` | Error `W_GetLumpinfo: b not found` | WAD-3.3 |

The same build loaded `gfx.wad` from LibreQuake's `lite` `id1/pak0.pak` (see PAK-4): 149
lumps, every one with `disksize` equal to `size`. Looking up `conchars`, `CONCHARS`,
`disc` and `backtile` found offsets 23996, 23996, 40380 and 7604, and
`ConChars_extra_long_name` was not found. A reader written from this spec alone gave the
same lumps and lookups.

`qs-wad`'s unit tests check the behaviour of every row of the table above, and the `qs-vfs` example
`walk` found the same 149 lumps and `conchars` at offset 23996 in LibreQuake's `gfx.wad` on
2026-10-16.

## WAD-4 Lump contents the engine reads

**Source:** `WinQuake/wad.h`, `qpic_t`; `WinQuake/draw.c`, `Draw_Init`, `Draw_PicFromWad`
and `Draw_Character`; `WinQuake/bspfile.h`, `miptex_t`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

`Draw_Init` takes `conchars`, `disc` and `backtile` from `gfx.wad`; `Draw_PicFromWad`
returns any other lump as a `qpic_t`. `Draw_Character` indexes `conchars` as rows of 128
bytes, 16 characters of 8 by 8 pixels per row.

### Behaviour

- **WAD-4.1** A picture lump (`TYP_QPIC`) is a `width` and a `height`, signed 32-bit
  little-endian, followed by `width * height` bytes of palette indices, row by row from
  the top left.
- **WAD-4.2** The `conchars` lump has no header: it is 128 by 128 bytes of palette
  indices, 16 rows of 16 characters of 8 by 8 pixels, character `n` at row `n >> 4` and
  column `n & 15`, with index 0 drawn as transparent.
- **WAD-4.3** A texture lump (`TYP_MIPTEX`) starts with a `miptex_t`: a 16-byte name,
  then `width`, `height` and four mip offsets as unsigned 32-bit little-endian integers,
  the offsets from the start of the lump. id's engine never reads a texture lump from a
  WAD; the same structure in a BSP file is the BSP spec's, and WAD2 texture files are read
  only by map tools and editors.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `WAD_CONCHARS_SIZE` | 128 | `WinQuake/draw.c`, the row stride in `Draw_Character` |
| `WAD_MIPLEVELS` | 4 | `WinQuake/bspfile.h`, `MIPLEVELS` |

## WAD-5 Writing a WAD2 file

**Source:** our decision, for importing textures (P6-15, MAP-23), written to the layout of
WAD-1 and WAD-2. id's `qlumpy` (`qutils/QLUMPY/QLUMPY.C` of id-Software/Quake-Tools at
`c0d1b91c74eb654365ac7755bc837e497caaca73`, read for description only, P4-22) writes the same
layout: the lumps after the header, the directory last.

### Behaviour

- **WAD-5.1** A written file is the 12-byte header (WAD-1.1), then every lump's bytes in the
  order given, from offset 12, then the directory of one 32-byte entry per lump in the same
  order; `infotableofs` is the directory's offset.
- **WAD-5.2** Each entry's `filepos` is its lump's offset, `disksize` and `size` are both the
  lump's length, `type` is the lump's type, `compression` is `WAD_CMP_NONE`, the two unused
  bytes are 0, and the name is the given name's bytes followed by NUL bytes to 16. The name is
  written as given, never lower-cased or upper-cased.
- **WAD-5.3** A name that is empty, longer than 15 bytes (so it would have no NUL), or equal
  to an earlier lump's name with ASCII case ignored, is refused: the engine lower-cases names
  (WAD-2.5) and qbsp upper-cases them, so either would find only the first of two such lumps
  (WAD-2.9).
- **WAD-5.4** A file whose size, an offset or a lump length would not fit a signed 32-bit
  integer is refused.
- **WAD-5.5** A texture lump is added under the name its `miptex_t` header holds (WAD-7.1); one
  whose header holds another name is refused, so qbsp, which finds a texture by its directory
  name, and the engine, which reads the header's, see one name.

## WAD-6 Texture names and sizes

**Source:** BSP-4.4, BSP-4.8 and BSP-20.6, which say how the engine reads a texture's name and
size; QS `Quake/gl_model.c`, `Mod_LoadTextures` ("`// ericw -- fence textures`", `if
(tx->name[0] == '{') extraflags |= TEXPREF_ALPHA;`) and `Quake/gl_texmgr.c`,
`TexMgr_LoadPalette` (`((byte *) &d_8to24table[255]) [3] = 0;`) at
sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`.

A texture written to a WAD for a map is checked before it is written, so a map never meets a
texture the engine refuses; a fault is reported, never repaired.

### Behaviour

- **WAD-6.1** A texture name is 1 to 15 bytes, each a printable ASCII byte other than a space
  and `"`, which end or quote a token in a `.map` (MAP-1), with no `//` (MAP-1.3) and no `;`
  (MAP-1.8), which start a comment.
- **WAD-6.2** A name that starts with `+` names a frame of an animation: it needs at least
  three bytes, and its second byte must be `0` to `9`, or `a` to `j` or `A` to `J`; any other
  is refused, which the engine would refuse when it loads the map (BSP-4.8).
- **WAD-6.3** A name that starts with `*` is a liquid, and one that starts with `sky` in any case
  is a sky, as QuakeSpasm and qbsp read it (BSP-20.6); each needs a byte after its prefix.
- **WAD-6.4** A name that starts with `{` is a fence texture: QuakeSpasm draws its index 255
  transparent, its palette entry 255 having alpha 0.
- **WAD-6.5** A width or height of 0, or one that is not a multiple of 16, is refused, which
  the engine would refuse when it loads the map (BSP-4.4).

## WAD-7 Encoding a texture

**Source:** our decision. The layout is BSP-4.5's, which qbsp copies from a WAD; the fullbright
range is REND-13.4's (texels above 223); the palette is REND-1.8's. id's `qlumpy`
(`qutils/QLUMPY/QUAKEGRB.C`, `GrabMip`, `AveragePixels`, `BestColor`, read for description
only) builds each smaller mip level from the mean colour of each block of the level above,
matched by squared RGB distance; we keep its blocks and means and choose colours as WAD-7.3
does.

### Behaviour

- **WAD-7.1** A texture lump is a 40-byte `miptex_t` (WAD-4.3): the name followed by NUL bytes
  to 16, the width and height, and the offsets 40, `40 + w*h`, `40 + w*h + w*h/4` and
  `40 + w*h + w*h/4 + w*h/16`; then the four levels, each row by row from the top left. Its
  directory type is `WAD_TYP_MIPTEX`.
- **WAD-7.2** A palette is 768 bytes, 256 colours of red, green and blue; any other length is
  refused.
- **WAD-7.3** A colour's nearest palette index is the one with the least distance
  `(2 + m/256) dR^2 + 4 dG^2 + (2 + (255 - m)/256) dB^2`, where `dR`, `dG` and `dB` are the
  differences and `m` the mean of the two reds; of equal distances the lower index wins.
- **WAD-7.4** An ordinary pixel is matched among indices 0 to 223. A pixel the caller marks
  fullbright is matched among 224 to 254, and 255 as well unless the texture is a fence
  texture (WAD-6.4).
- **WAD-7.5** In a fence texture a pixel whose alpha is below 128 is index 255. In any other
  texture such a pixel is refused, with the number of such pixels; alpha of 128 or more is
  opaque, and the pixel's colour is used as it is.
- **WAD-7.6** With dithering, each ordinary pixel's error (its colour less its index's colour,
  per channel) is spread, row by row from the top left, as Floyd and Steinberg's 7/16 to the
  right, 3/16 below left, 5/16 below and 1/16 below right, only to ordinary pixels, each
  channel clamped to 0 to 255 before matching. Without it each pixel is matched alone.
- **WAD-7.7** Level `k` of 1 to 3 has a pixel for each block of `2^k` by `2^k` pixels of level 0.
  A block of which at least half the pixels are index 255 in a fence texture is 255. Otherwise,
  of its other pixels, when more than half are fullbright (above 223) the pixel is the mean
  colour of those fullbright pixels matched among the fullbright indices of WAD-7.4; else it is
  the mean colour of the ordinary ones matched among 0 to 223. A mean is each channel's sum
  divided by the count, rounded down. Mip levels are not dithered.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `WAD_TEXTURE_NAME_MAX` | 15 | `WinQuake/bspfile.h`, `char name[16]` in `miptex_t`, less its NUL |
| `WAD_TEXTURE_ALIGN` | 16 | BSP-4.4 |
| `WAD_PALETTE_SIZE` | 768 | REND-1.8 |
| `WAD_FULLBRIGHT_FIRST` | 224 | REND-13.4, `Mod_CheckFullbrights` "`> 223`" |
| `WAD_TRANSPARENT` | 255 | QS `Quake/gl_texmgr.c`, `TexMgr_LoadPalette` |
| `WAD_OPAQUE_ALPHA` | 128 | our decision: the middle of the alpha range |

## WAD-8 Decoding a texture for display

**Source:** our decision, for the editor's texture browser (MAP-5). The layout read is WAD-4.3's;
the fence rule is WAD-6.4's, QuakeSpasm's `TexMgr_LoadPalette` giving entry 255 alpha 0.

### Behaviour

- **WAD-8.1** A texture is shown from its first mip level: `width * height` palette indices from
  the offset its first `miptex_t` offset names, row by row from the top left. A lump shorter than
  the 40-byte header, or than that offset plus `width * height`, is refused with the length it
  needs.
- **WAD-8.2** Each index is its palette colour, opaque, except index 255 in a fence texture
  (WAD-6.4), which is transparent: alpha 0.
- **WAD-8.3** A width or height of 0, or past 4096 (`WAD_DECODE_MAX_SIDE`, the side a GPU may
  refuse), is refused, and so are fewer pixels than `width * height`.
- **WAD-8.4** A WAD3 texture is shown in its own palette (WAD3-3.3): the colours after its
  16-bit count, each red, green and blue, the indices past the count or past the bytes black.
