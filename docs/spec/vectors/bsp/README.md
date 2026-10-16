# BSP29 test vectors

Vectors for [`../../bsp.md`](../../bsp.md). [`base.txt`](base.txt) is a hand-built BSP29
stored as an `xxd` hex dump: `xxd -r base.txt > base.bsp` rebuilds it byte for byte (1020
bytes, SHA-256 `fc9a1cabdaf7fb0e6086e76684197a84faeb08c3e38bcc03cec0522ce99bc36c`). The
other vectors are the base with one change each, listed below, so a test builds them from
the base with a factory.

**Checked:** on 2026-10-16 every vector was loaded by id's `Mod_LoadBrushModel` and the
functions it calls, in `WinQuake/model.c` copied unchanged from
id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d` and built with the system C
compiler (Apple clang, arm64) against `bspfile.h`, `model.h`, `modelgen.h` and `spritegn.h`
from the same commit. The harness stubs only the engine around the loader: the hunk and
cache as `calloc`-like buffers, `Sys_Error` printing its message and stopping,
`COM_LoadStackFile` returning the file, `R_InitSky` printing the texture name, and the
little-endian swaps as no-ops. It printed every loaded field; the results below are its
output. `.lit` has no vectors. QS's loader was run on every vector, and on the BSP2 ones, for the FitzQuake lineage's rules (BSP-20), as the last section says.

`x87-order`, `x87-pc64` and `nan-vertex` (2026-10-16) were loaded by the same harness built
for x86-64 with `val` computed by WinQuake's x87 instructions and compared as WinQuake does,
the run described under "Evidence for BSP-9.11 and BSP-9.12" in [`../../bsp.md`](../../bsp.md);
each row also gives the result the other evaluations give. The other vectors' texture
coordinates are exact in single precision, so every evaluation gives them the same result.

`extents-65552`, `extents-32784` and `texturemins-32768` (2026-10-16) were loaded by the
harness above with `WinQuake/model.c` unchanged, built for arm64 with Apple clang and for
x86-64 with gcc 14.4 in an emulated `linux/amd64` container; both builds gave the results
below, which are those of `WINQUAKE.EXE`'s 16-bit code (the evidence for BSP-9.14 in
[`../../bsp.md`](../../bsp.md)).

The rows marked GLQuake (2026-10-16) also give the result of GLQuake's loader:
`Mod_LoadBrushModel` in `WinQuake/gl_model.c`, copied unchanged from the pinned commit with
`gl_model.h`, in the same harness with `GL_LoadTexture`, `GL_SubdivideSurface`,
`GL_MakeAliasModelDisplayLists` and `Cvar_RegisterVariable` stubbed, built for arm64 with
Apple clang and for x86-64 with gcc 14.4; both builds, and both builds of `WinQuake/model.c`,
gave the results below (BSP-9.15). Every other vector's face extents are 256 or less, or its
texinfo has `TEX_SPECIAL`, so GLQuake's limit does not change its result.

The rows for BSP-9.16 (2026-10-16) also give what id's renderers do with the loaded face, and
the lightmap `qs-bsp` gives it. WinQuake: `D_CacheSurface` and `D_SCAlloc` from
`WinQuake/d_surf.c` with `WinQuake/r_surf.c`, caching the face at mip levels 0 to 3 as the
software renderer does when it draws it. GLQuake: `GL_BuildLightmaps`, `AllocBlock`,
`GL_CreateSurfaceLightmap` and `R_BuildLightMap` from `WinQuake/gl_rsurf.c`, as GLQuake runs
them when a map starts. Both were built from the pinned commit unchanged with id's own
headers, each beside its loader, with stubs for the rest of the engine, for arm64 with Apple
clang and for x86-64 with gcc 14.4; all four builds gave the results below. Given `base`, the
WinQuake build caches surfaces 64, 32, 16 and 8 bytes wide, and the GLQuake build puts the
5 by 5 lightmap at column 0, row 0 of lightmap texture 0, its texels 255 - 2 * sample.

The rows for BSP-12.6 (2026-10-16) give the face flags of both loaders: `WinQuake/model.c`
and `WinQuake/gl_model.c`, unchanged from the pinned commit in the harness above, each built for
arm64 with Apple clang and for x86-64 with gcc 14.4; all four builds gave the results below. Both
loaders give `base`'s face, which only the empty leaf 1 lists, flags 0.

## base

One square face on the plane z = 0, in a world of one node and two leaves, plus a second
model for a door. Statements: BSP-1.1, BSP-1.3, BSP-2.1, BSP-3.1, BSP-4.3, BSP-5.1,
BSP-6.2, BSP-7.1, BSP-7.2, BSP-8.1, BSP-9.1, BSP-9.2, BSP-9.6, BSP-9.7, BSP-9.10, BSP-11.1,
BSP-12.1, BSP-13.1, BSP-14.1, BSP-14.2, BSP-15.1 to BSP-15.4.

| Lump | `fileofs` | `filelen` | Contents |
| --- | --- | --- | --- |
| entities | 124 | 92 | `{ "classname" "worldspawn" "wad" "gfx/base.wad" }` and `{ "classname" "func_door" "model" "*1" }`, one pair per line, then a NUL |
| planes | 216 | 20 | normal (0, 0, 1), dist 0, type 2 |
| textures | 236 | 388 | 1 texture at offset 8: `wall`, 16 by 16, mip offsets 40, 296, 360, 376, every pixel 7 |
| vertexes | 624 | 48 | (0, 0, 0), (64, 0, 0), (64, 64, 0), (0, 64, 0) |
| visibility | 672 | 1 | `01` |
| nodes | 676 | 24 | plane 0, children -2 and -1 (leaf 1, leaf 0), bounds (0, 0, 0) to (64, 64, 0), faces 0 + 1 |
| texinfo | 700 | 40 | s axis (1, 0, 0) offset 0, t axis (0, 1, 0) offset 0, miptex 0, flags 0 |
| faces | 740 | 20 | plane 0, side 0, edges 0 + 4, texinfo 0, styles 0 255 255 255, lightofs 0 |
| lighting | 760 | 25 | bytes 0 to 24 |
| clipnodes | 788 | 8 | plane 0, children -1 and -2 |
| leafs | 796 | 56 | leaf 0: contents -2, visofs -1, no marksurfaces; leaf 1: contents -1, visofs 0, marksurfaces 0 + 1; both bounds (0, 0, 0) to (64, 64, 0) and ambient 0 1 2 3 |
| marksurfaces | 852 | 2 | face 0 |
| edges | 856 | 20 | (0, 0) unused, (0, 1), (1, 2), (2, 3), (3, 0) |
| surfedges | 876 | 16 | 1, 2, 3, -4 |
| models | 892 | 128 | model 0: (0, 0, 0) to (64, 64, 0), headnodes 0 0 0 0, visleafs 1, faces 0 + 1; model 1: (0, 0, 0) to (64, 64, 8), visleafs 0, faces 0 + 1 |

id's loader gives:

| Value | Result |
| --- | --- |
| Face 0 | `texturemins` 0 0, extents 64 64 (a 5 by 5 lightmap at lighting offset 0) |
| Texinfo 0 | texture `wall`, flags 0, mip bias 1 |
| Plane 0 | `signbits` 0 |
| Hulls | hull 1 (-16, -16, -24) to (16, 16, 32), hull 2 (-32, -32, -24) to (32, 32, 64), each over clipnodes 0 to 0 |
| Model 0 (the world) | bounds (-1, -1, -1) to (65, 65, 1), radius 91.9293, 1 visible leaf |
| Model 1, named `*1` | bounds (-1, -1, -1) to (65, 65, 9), radius 92.3634 |
| Leaf 0 row | `ff` |
| Leaf 1 row | `01` |

## Variants

Each row changes the base as described; "loads" means the loader finished, and the values
given are those that differ from the base. A variant whose face needs more light samples
than the base's 25 gives the lighting lump's size, so that the samples lie in it (BSP-10.2);
id's loader copies the lighting lump without reading it, so the size does not change the
result it records.

| Name | Change | Result | Statements |
| --- | --- | --- | --- |
| `version-30` | `version` 30 | Error `Mod_LoadBrushModel: <name> has wrong version number (30 should be 29)` | BSP-1.2 |
| `funny-vertexes` | Vertexes lump of 49 bytes (four vertexes and one more byte) | Error `MOD_LoadBmodel: funny lump size in <name>` | BSP-1.4 |
| `not-aligned` | Texture `wall` 24 by 16 | Error `Texture wall is not 16 aligned` | BSP-4.4 |
| `missing-texture` | Texture slot offset -1; texinfo flags 1 | Loads; slot 0 empty; texinfo 0 uses the checkerboard with flags 0 | BSP-4.2, BSP-8.4 |
| `no-textures` | Empty textures lump | Loads; no textures; texinfo 0 uses the checkerboard with flags 0 | BSP-4.1, BSP-8.4 |
| `extents-256` | Square of side 256, lighting of 289 bytes | Loads; extents 256 256 (GLQuake the same) | BSP-9.4, BSP-9.15 |
| `extents-272` | Square of side 272, lighting of 324 bytes | Error `Bad surface extents` (GLQuake loads; extents 272 272) | BSP-9.4, BSP-9.15 |
| `extents-272-special` | Square of side 272, lighting of 324 bytes, texinfo flags 1 | Loads; extents 272 272 (GLQuake the same) | BSP-9.4, BSP-9.15 |
| `extents-512` | Square of side 512, lighting of 1089 bytes | Error `Bad surface extents` (GLQuake loads; extents 512 512, a 33 by 33 lightmap) | BSP-9.4, BSP-9.15 |
| `extents-528` | Square of side 528, lighting of 1156 bytes | Error `Bad surface extents` (GLQuake the same) | BSP-9.4, BSP-9.15 |
| `extents-528-special` | Square of side 528, lighting of 1156 bytes, texinfo flags 1 | Loads; extents 528 528 (GLQuake the same) | BSP-9.4, BSP-9.15 |
| `texture-offset` | Texinfo offsets -8 (s) and 24 (t), lighting of 36 bytes | Loads; `texturemins` -16 16, extents 80 80 | BSP-8.1, BSP-9.10 |
| `mipadjust` | Texinfo axes (0.5, 0, 0) and (0, 0.5, 0) | Loads; mip bias 2; extents 32 32 | BSP-8.6, BSP-9.10 |
| `bad-miptex` | Texinfo miptex 1 | Error `miptex >= loadmodel->numtextures` | BSP-8.3 |
| `bad-marksurface` | Marksurface 1 | Error `Mod_ParseMarksurfaces: bad surface number` | BSP-13.2 |
| `anim` | Textures `+0lava`, `+1lava`, `+Alava`, `wall` | Loads; `+0lava` total 4, 0 to 2, next `+1lava`, alternate `+Alava`; `+1lava` 2 to 4, next `+0lava`, alternate `+Alava`; `+Alava` total 2, 0 to 2, next itself, alternate `+0lava`; `wall` not animated | BSP-4.8, BSP-4.9 |
| `anim-missing` | Textures `+0slime`, `+2slime` | Error `Missing frame 1 of +0slime` | BSP-4.9 |
| `anim-bad` | Texture `+xfoo` | Error `Bad animating texture +xfoo` | BSP-4.8 |
| `sky` | Texture `sky1` | Loads; `R_InitSky` called for `sky1`; face flags sky and tiled (0x24) | BSP-4.7, BSP-9.5 |
| `turb` | Texture `*water`, texinfo flags 1 | Loads; face flags turbulent and tiled (0x30), `texturemins` -8192 -8192, extents 16384 16384 | BSP-9.5 |
| `nolight-back` | Face side 1, styles 0 1 255 255, lightofs -1 | Loads; face drawn from its plane's back (flag 0x2), no samples | BSP-9.1, BSP-9.6 |
| `planes` | Second plane: normal (-0.6, 0, -0.8), dist 5, type 9 | Loads; plane 1 `signbits` 5, type 9 kept | BSP-3.2, BSP-3.3 |
| `vis` | Model 0 `visleafs` 20; 21 leaves, leaf 2 with visofs 4 and leaves 3 to 20 with visofs -1; visibility bytes `05 00 01 80 00 05` | Loads; rows of 3 bytes: leaf 0 `ff ff ff`, leaf 1 `05 00 80`, leaf 2 `00 00 00` (its run of 5 ends past the row) | BSP-6.2 to BSP-6.4, BSP-15.2 |
| `no-entities` | Empty entities lump | Loads; no entity text | BSP-2.3 |
| `x87-order` | Vertexes (1, 0, 1), (1, 1, 1), (1, 1, 1), (1, 1, 1); s axis (2048, 2^-42, -(1024 - 2^-14)) offset 0 | Loads; `texturemins` 1024 0, extents 16 16 (single precision, or C's order of the additions at 53 bits, gives extents 0 16) | BSP-9.11 |
| `x87-pc64` | Vertexes as `x87-order`; s axis (0, 2^-14, 2^-50) offset 1024 | Loads; `texturemins` 1024 0, extents 0 16 (additions at 64 bits give extents 16 16) | BSP-9.11 |
| `nan-vertex` | Vertex 0 (NaN, 0, 0), the float bits `7fc00000` | Loads; `texturemins` 0 0, extents 0 0 (C's comparisons give `texturemins` 64 0, extents 0 64) | BSP-9.12 |
| `extents-65552` | s axis (1024.25, 0, 0) offset 0 | Loads; extents 16 64: the extent 65552 keeps its low 16 bits | BSP-9.4, BSP-9.14 |
| `extents-32784` | s axis (512.25, 0, 0) offset 0 | Loads; extents -32752 64: the extent 32784 keeps its low 16 bits, a negative value that passes the check. WinQuake drawing it: error `D_SCAlloc: bad cache width -32752`. GLQuake starting the map: `smax` -2046, `tmax` 5, error `AllocBlock: full`. `qs-bsp`: lightmap 0 by 0 | BSP-9.4, BSP-9.14, BSP-9.16 |
| `extents-65520` | s axis (1023.75, 0, 0) offset 0 | Loads; extents -16 64. WinQuake drawing it: error `D_SCAlloc: bad cache width -16`. GLQuake: `smax` 0, `tmax` 5, a block at column 127, row 0 of texture 0 and nothing stored in it. `qs-bsp`: lightmap 0 by 0 | BSP-9.16 |
| `extents-32784-t` | t axis (0, 512.25, 0) offset 0 | Loads; extents 64 -32752. WinQuake drawing it: error `D_SCAlloc: bad cache size -2096128`. GLQuake: `smax` 5, `tmax` -2046, a block at column 0, row 0 of texture 0, whose five columns' height becomes -2046, and nothing stored in it. `qs-bsp`: lightmap 0 by 0 | BSP-9.16 |
| `extents-65504-st` | s axis (1023.5, 0, 0) and t axis (0, 1023.5, 0), offsets 0 | Loads; extents -32 -32. WinQuake drawing it: error `D_SCAlloc: bad cache width -32`. GLQuake: `smax` and `tmax` -1, texture 0 with no position set, 1 sample added and nothing stored. `qs-bsp`: lightmap 0 by 0 | BSP-9.16 |
| `extents-32784-st` | s axis (512.25, 0, 0) and t axis (0, 512.25, 0), offsets 0 | Loads; extents -32752 -32752. WinQuake drawing it: error `D_SCAlloc: bad cache width -32752`. GLQuake not run: it would add 2046 * 2046 samples into `blocklights[18*18]`, undefined in C. `qs-bsp`: lightmap 0 by 0 | BSP-9.16 |
| `texturemins-32768` | s axis (1, 0, 0) offset 32768 | Loads; `texturemins` -32768 0 | BSP-9.14 |
| `water-leaf` | Leaf 1 contents -3 (water) | Loads; WinQuake: face flags 0. GLQuake: face flags 0x80 (`SURF_UNDERWATER`) | BSP-12.6 |
| `water-leaf-turb` | Leaf 1 contents -3, texture `*water`, texinfo flags 1 | Loads; WinQuake: face flags 0x30. GLQuake: face flags 0xb0 | BSP-12.6 |
| `solid-leaf-face` | Leaf 0 (contents -2) lists marksurface 0 too: marksurfaces 0 + 1 | Loads; WinQuake: face flags 0. GLQuake: face flags 0x80, though the empty leaf 1 lists the face too | BSP-12.6 |
| `contents-unknown` | Leaf 1 contents -7, no `CONTENTS_` value | Loads; WinQuake: face flags 0. GLQuake: face flags 0x80 | BSP-12.6 |

## The FitzQuake lineage (BSP-20)

**Checked:** on 2026-10-16 every vector in this file was loaded by QuakeSpasm's
`Mod_ForName`, `Mod_LoadBrushModel` and the functions they call, in `Quake/gl_model.c` copied
unchanged with every header of `Quake/` from
sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5` (the tarball
`https://codeload.github.com/sezero/quakespasm/tar.gz/c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`,
SHA-256 `139d85c096ebec6b62425584be570daa5830323ff6eac6cd7ed94400bec3deba`). It was built with
`gcc -O1 -w $(sdl2-config --cflags)` (gcc 14.4.0, Debian's `libsdl2-dev` 2.32.4 for the
headers) for x86-64 and for arm64 in `linux/amd64` and `linux/arm64` containers. The harness
stubs only the engine around the loader: the hunk as a zeroed buffer, `Sys_Error` and
`Host_Error` printing their message and stopping, the console printing to standard error,
`COM_LoadStackFile` returning the file and every other file lookup (`.lit`, `.ent`, `.vis`,
WADs, external images) finding nothing, the cvars at 0, `isDedicated` false, the texture
manager and `Sky_LoadTexture` recording without uploading, `q_strncasecmp` and
`q_strcasecmp` as the C library's, and `VectorLength` as in QuakeSpasm's `Quake/mathlib.c`.
It printed every texture (name, size, sky, animation, and a hash of the `width * height`
bytes kept), texinfo (flags and texture), face (`texturemins`, extents, flags), leaf,
node, clipnode, hull 0 clipnode, model (bounds, radius, `visleafs`, faces, head nodes) and
visibility row. `qs-bsp` read with `BspLoader::default()` printed the same as the x86-64 build
for every vector and the 23 BSPs of LibreQuake's `lite` release (267,924 lines), except:

- `version-30`: QuakeSpasm loads it as a Half-Life BSP and `qs-bsp` fails it (BSP-20.2).
- `bad-marksurface`: both fail; QuakeSpasm's message is
  `Mod_LoadMarksurfaces: bad surface number` and `qs-bsp`'s is id's (BSP-20.18).

The arm64 build printed the same as the x86-64 build for every input but `no-edges`, where its
conversion of an out-of-range `floor` saturates (BSP-20.8). The GLQuake and WinQuake columns of
the new rows come from the `WinQuake/model.c` and `WinQuake/gl_model.c` builds above;
`double-sum` was also loaded by the x87 harness of BSP-9.11, whose 53-bit and 64-bit
evaluations give extents 32 16 (single precision gives 16 16).

| Name | Change | Result with the FitzQuake lineage's loader | GLQuake and WinQuake | Statements |
| --- | --- | --- | --- | --- |
| `base` | None | Loads as the base, with level 0's 256 pixels of `wall` kept | Loads | BSP-20.1, BSP-20.5 |
| `version-30` | As above | Loads in QuakeSpasm as Half-Life's format; `qs-bsp` fails with BSP-1.2's error | Error | BSP-20.2 |
| `texture-zero` | Texture `wall` 0 by 16 | Loads; slot 0 empty; texinfo 0 flags 2 (`TEX_MISSING`), the checkerboard; face flags 0x100 | Load; the texture has no pixels; face flags 0 | BSP-20.3, BSP-20.7, BSP-20.11 |
| `not-aligned` | As above | Loads; `wall` 24 by 16, its 384 bytes kept | Error `Texture wall is not 16 aligned` | BSP-20.4, BSP-20.5 |
| `texture-8x8` | Texture `wall` 8 by 8 | Loads; `wall` 8 by 8, its 64 bytes kept | Error `Texture wall is not 16 aligned` | BSP-20.4, BSP-20.5 |
| `sky-upper` | Texture `SKY1` | Loads; a sky; face flags 0x24 | Load; not a sky; face flags 0 | BSP-20.6, BSP-20.11 |
| `bad-miptex` | As above | Loads; texinfo 0 the checkerboard, flags 0 | Error `miptex >= loadmodel->numtextures` | BSP-20.7 |
| `miptex-past` | Texinfo miptex 2 | Loads; texinfo 0 the checkerboard, flags 2; face flags 0x100 | Error `miptex >= loadmodel->numtextures` | BSP-20.7, BSP-20.11 |
| `missing-texture` | As above | Loads; texinfo 0 flags 3, `r_notexture_mip2`; face flags 0x100 | Load; flags 0 | BSP-20.7, BSP-20.11 |
| `no-textures` | As above | Loads; texinfo 0 the checkerboard, flags 0 | Load; flags 0 | BSP-20.7 |
| `far-offset` | s axis offset 2000000 | Loads; `texturemins` -31616 0, extents 64 64 | Error `Bad surface extents` | BSP-20.8 |
| `no-edges` | Face 0 `numedges` 0 | Loads; `texturemins` 0 0, extents 0 0 (the arm64 build: -16 -16, 16 16) | Error `Bad surface extents` | BSP-20.8 |
| `nan-vertex` | As above | Loads; `texturemins` 64 0, extents 0 64 | Load; `texturemins` 0 0, extents 0 0 | BSP-20.8 |
| `double-sum` | Vertexes (0, 0, 0) and three of (1, 1, 1); s axis (16, 2^-20, 2^-26) offset 0 | Loads; extents 32 16 | Load; extents 32 16 | BSP-20.9 |
| `x87-order` | As above | Loads; `texturemins` 1024 0, extents 0 16 | Load; extents 16 16 | BSP-20.9 |
| `extents-1984` | Square of side 1984, lighting of 15625 bytes | Loads; extents 1984 1984 | Error `Bad surface extents` | BSP-20.10 |
| `extents-2016` | Square of side 2016, lighting of 16129 bytes | Error `Bad surface extents` | Error `Bad surface extents` | BSP-20.10 |
| `turb` | As above | Loads; face flags 0x2030; `texturemins` 0 0, extents 64 64 | Load; flags 0x30; extents 16384 16384 | BSP-20.11 |
| `turb-lit` | Texture `*water`, texinfo flags 0 | Loads; face flags 0x2010; extents 64 64 | Load; flags 0x30; extents 16384 16384 | BSP-20.11 |
| `turb-lava` | Texture `*lava1`, texinfo flags 1 | Loads; face flags 0x430 | Load; flags 0x30 | BSP-20.11 |
| `turb-slime` | Texture `!slime0`, texinfo flags 1 | Loads; face flags 0x830 | Load; flags 0 | BSP-20.11 |
| `turb-tele` | Texture `*teleport`, texinfo flags 1 | Loads; face flags 0x1030 | Load; flags 0x30 | BSP-20.11 |
| `fence` | Texture `{grate` | Loads; face flags 0x200 | Load; flags 0 | BSP-20.11 |
| `texinfo-flag-2` | Texinfo flags 2 | Loads; texinfo 0 `wall`, flags 2; face flags 0x100 | Load; flags 0 | BSP-20.11 |
| `missing-unlit` | Texture slot offset -1; face 0 `lightofs` -1 | Loads; texinfo 0 flags 2; face flags 0x120 | Load; flags 0 | BSP-20.11 |
| `water-leaf`, `water-leaf-turb`, `solid-leaf-face`, `contents-unknown` | As above | Load; face flags 0, 0x2030, 0 and 0 | GLQuake 0x80, 0xb0, 0x80, 0x80 | BSP-20.12 |
| `leafs-32768` | 32768 leaves, those after leaf 1 solid with no faces | Error `Mod_LoadLeafs: 32768 leafs exceeds limit of 32767.` | Load | BSP-20.13 |
| `nodes-32769` | 32769 nodes; node 0 children 32768 and -2; the others -1 and -1 | Loads; node 0's front is node 32768 | `qs-bsp` fails (leaf 32767 does not exist, BSP-1.8); id's not run | BSP-20.15 |
| `node-bad-leaf` | Node 0 children -3 and -1 | Loads; node 0's front is leaf 0 | `qs-bsp` fails (BSP-1.8); id's not run | BSP-20.15 |
| `bsp2-child-0` | The base as `BSP2`; node 0 children 0 and -1 | Loads; node 0's front is leaf 0 | id's read no BSP2; `qs-bsp` loads with node 0 as its front | BSP-20.16 |
| `bsp2-bad-leaf` | The base as `BSP2`; node 0 children -5 and -1 | Loads; node 0's front is leaf 0 | id's read no BSP2; `qs-bsp` fails (BSP-1.8) | BSP-20.16 |
| `clipnode-past` | Clipnode 0 children 5 and -2 | Loads; clipnode 0 children -65531 and -2 | id's loaders load it; `qs-bsp` fails (BSP-1.8) | BSP-20.17 |
