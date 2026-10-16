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
output. BSP2 and `.lit` have no vectors: QS was read, not run.

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
| `extents-256` | Square of side 256, lighting of 289 bytes | Loads; extents 256 256 | BSP-9.4 |
| `extents-272` | Square of side 272, lighting of 324 bytes | Error `Bad surface extents` | BSP-9.4 |
| `extents-272-special` | Square of side 272, lighting of 324 bytes, texinfo flags 1 | Loads; extents 272 272 | BSP-9.4 |
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
| `extents-32784` | s axis (512.25, 0, 0) offset 0 | Loads; extents -32752 64: the extent 32784 keeps its low 16 bits, a negative value that passes the check | BSP-9.4, BSP-9.14 |
| `texturemins-32768` | s axis (1, 0, 0) offset 32768 | Loads; `texturemins` -32768 0 | BSP-9.14 |
