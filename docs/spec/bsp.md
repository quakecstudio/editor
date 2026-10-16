# BSP29, BSP2 and .lit

Prefix: `BSP` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the compiled map: the `.bsp` file of version 29 as id's engine loads it, the two
BSP2 variants later engines added, and the `.lit` file that gives a map coloured light.
BSP29 is described from `WinQuake/bspfile.h` and `WinQuake/model.c` in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`, with the server's use of the result from
`WinQuake/sv_main.c` and `WinQuake/world.c`. GLQuake's loader in `WinQuake/gl_model.c` makes
the same checks with the same messages, except that it accepts larger face extents
(BSP-9.15). Implemented by `crates/qs-bsp`
([#143](https://github.com/quakecstudio/editor/issues/143), P5-02), whose tests cite
every statement.

id never defined BSP2 or `.lit`. They are described from the programs that read and write
them, cited for description only; neither is a registered upstream for porting
([`../porting.md`](../porting.md#upstreams)) and no code is ported from them:

| Short name | Repository and commit | Files read |
| --- | --- | --- |
| QS | [sezero/quakespasm](https://github.com/sezero/quakespasm)@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5` (QuakeSpasm, GPL-2.0-or-later) | `Quake/bspfile.h`; `Quake/gl_model.c`, `Mod_LoadBrushModel`, `Mod_LoadLighting` and the `Mod_Load*` lump readers |
| ericw-tools | [ericwa/ericw-tools](https://github.com/ericwa/ericw-tools)@`36eec1da2a194467e6baac2f444c2dd04b57d266` (GPL-2.0-or-later) | `include/common/bspfile_q1.hh`; `common/bspfile.cc`, `bspver_bsp2`, `bspver_bsp2rmq`; `include/common/litfile.hh`, `common/litfile.cc` (the writer) |

All integers are two's-complement little-endian values read through `LittleLong` (32-bit) or
`LittleShort` (16-bit), and all floats are 32-bit IEEE 754 little-endian values read through
`LittleFloat`. An offset is a byte offset from the start of the file unless a statement says
otherwise.

## BSP-1 Header and lumps

**Source:** `WinQuake/bspfile.h`, `dheader_t`, `lump_t`, `BSPVERSION`, `LUMP_*`,
`HEADER_LUMPS`; `WinQuake/model.c`, `Mod_LoadModel`, `Mod_LoadBrushModel`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

A BSP is a header, a directory of 15 lumps, and the lumps. `Mod_LoadModel` picks the loader
from the first four bytes (SPR-1): a file that is neither `IDPO` nor `IDSP` goes to
`Mod_LoadBrushModel`, which checks the version and then loads each lump into the model.

| Offset | Size | Type | Field |
| --- | --- | --- | --- |
| 0 | 4 | `int` | `version` |
| 4 + 8 * n | 4 | `int` | `lumps[n].fileofs` |
| 8 + 8 * n | 4 | `int` | `lumps[n].filelen` |

| n | Lump | Element | Size | Section |
| --- | --- | --- | --- | --- |
| 0 | `LUMP_ENTITIES` | text | 1 | BSP-2 |
| 1 | `LUMP_PLANES` | `dplane_t` | 20 | BSP-3 |
| 2 | `LUMP_TEXTURES` | `dmiptexlump_t` | variable | BSP-4 |
| 3 | `LUMP_VERTEXES` | `dvertex_t` | 12 | BSP-5 |
| 4 | `LUMP_VISIBILITY` | bytes | 1 | BSP-6 |
| 5 | `LUMP_NODES` | `dnode_t` | 24 | BSP-7 |
| 6 | `LUMP_TEXINFO` | `texinfo_t` | 40 | BSP-8 |
| 7 | `LUMP_FACES` | `dface_t` | 20 | BSP-9 |
| 8 | `LUMP_LIGHTING` | bytes | 1 | BSP-10 |
| 9 | `LUMP_CLIPNODES` | `dclipnode_t` | 8 | BSP-11 |
| 10 | `LUMP_LEAFS` | `dleaf_t` | 28 | BSP-12 |
| 11 | `LUMP_MARKSURFACES` | `unsigned short` | 2 | BSP-13 |
| 12 | `LUMP_EDGES` | `dedge_t` | 4 | BSP-14 |
| 13 | `LUMP_SURFEDGES` | `int` | 4 | BSP-14 |
| 14 | `LUMP_MODELS` | `dmodel_t` | 64 | BSP-15 |

### Behaviour

- **BSP-1.1** A BSP starts with a 124-byte header: a signed 32-bit `version`, then 15
  directory entries of a signed 32-bit `fileofs` and a signed 32-bit `filelen`, in the order
  of the lump table above.
- **BSP-1.2** `version` must be 29; any other value fails with the error
  `Mod_LoadBrushModel: <name> has wrong version number (<version> should be 29)` (fatal in
  id's engine). Because the loader is chosen by the magic of SPR-1.2 and MDL-1.2, any file
  that is not a sprite or an alias model is checked as a BSP.
- **BSP-1.3** Lump `n` is the `filelen` bytes at `fileofs`. Lumps may appear in the file in
  any order and need not be adjacent; the bytes between and after them are ignored.
- **BSP-1.4** Each lump of fixed-size elements (planes, vertexes, nodes, texinfo, faces,
  clipnodes, leafs, marksurfaces, edges, surfedges and models) must have a `filelen` that is a
  whole multiple of its element size; otherwise loading fails with the error
  `MOD_LoadBmodel: funny lump size in <name>` (fatal in id's engine). The element count is
  `filelen` divided by the element size, and an empty lump has none. The entities,
  textures, visibility and lighting lumps have no such check.
- **BSP-1.5** id's loader reads the lumps in this order, so of two faults the one in the
  earlier lump is reported: vertexes, edges, surfedges, textures, lighting, planes, texinfo,
  faces, marksurfaces, visibility, leafs, nodes, clipnodes, entities, models.
- **BSP-1.6** A file shorter than the header, or a lump whose `fileofs` or `filelen` is
  negative or whose bytes run past the end of the file, fails to load. id's code reads the
  header and lumps from the loaded buffer without checking them, which is undefined; this
  spec fixes the answer as an error.
- **BSP-1.7** **Withdrawn** 2026-10-16: it made every visibility offset name a byte of the
  lump, but `qbsp` gives leaf 0 the offset 0 even when the visibility lump is empty, which
  id's loader never reads; replaced by BSP-1.8 and BSP-6.6.
- **BSP-1.8** An index stored in one lump that refers to another (a plane, texinfo, edge,
  surfedge, face, node, leaf, clipnode or vertex number, or an offset into the visibility or
  lighting lump) must name an element that exists; one that does not fails to load, except
  for the visibility offsets of BSP-6.6. An index is checked where the loader uses it: an
  edge's vertices and a surfedge's edge when a face uses them, and a model's first node or
  clipnode for hulls 0 to 2, since hull 3 is not used (BSP-11.3). id's code checks only the
  cases BSP-8.3 and BSP-13.2 name and uses every other index unchecked, which is undefined;
  this spec fixes the answer as an error.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BSP_VERSION` | 29 | `WinQuake/bspfile.h`, `BSPVERSION` |
| `BSP_HEADER_LUMPS` | 15 | `WinQuake/bspfile.h`, `HEADER_LUMPS` |
| `BSP_HEADER_SIZE` | 124 | `WinQuake/bspfile.h`, `sizeof(dheader_t)` |
| `BSP_LUMP_ENTITIES` to `BSP_LUMP_MODELS` | 0 to 14 | `WinQuake/bspfile.h`, `LUMP_ENTITIES` to `LUMP_MODELS`, in the order of the table |

### Test vectors

See [`vectors/bsp/`](vectors/bsp/README.md): `base` loads (BSP-1.1, BSP-1.3); variant
`version-30` gives the version error and `funny-vertexes` the lump-size error (BSP-1.2,
BSP-1.4).

## BSP-2 Entities

**Source:** `WinQuake/model.c`, `Mod_LoadEntities`; `WinQuake/sv_main.c`, `SV_SpawnServer`;
`WinQuake/pr_edict.c`, `ED_LoadFromFile` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

The entity lump is the map's entities as text, in the syntax of MAP-2 without brushes. The
server spawns them as EDICT-8 describes.

### Behaviour

- **BSP-2.1** The entities lump is text: a sequence of entities, each `{`, then `"key"
  "value"` pairs, then `}`, tokenised by `COM_Parse` and applied by EDICT-8.
- **BSP-2.2** The text ends at its first NUL byte or at the end of the lump, whichever comes
  first. id's engine copies exactly `filelen` bytes and parses until a NUL; id's `qbsp`
  writes a NUL after the text and counts it in `filelen` (`qutils/QBSP/MAP.C`,
  `WriteEntitiesToString`: `entdatasize = end - buf + 1`), so a lump without one is read
  past its end, which is undefined; this spec fixes the answer as the end of the lump.
- **BSP-2.3** An empty entities lump loads; the map then has no entities.
- **BSP-2.4** The first entity is the world (EDICT-4); a brush entity names its model with a
  `model` key of the form `*<n>` (BSP-15.3).

## BSP-3 Planes

**Source:** `WinQuake/bspfile.h`, `dplane_t`, `PLANE_*`; `WinQuake/model.c`,
`Mod_LoadPlanes` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

| Offset | Size | Type | Field |
| --- | --- | --- | --- |
| 0 | 12 | `float[3]` | `normal` |
| 12 | 4 | `float` | `dist` |
| 16 | 4 | `int` | `type` |

### Behaviour

- **BSP-3.1** A plane is the 20-byte `dplane_t`: the set of points `p` with
  `dot(normal, p) = dist`. Its front is the side `normal` points to.
- **BSP-3.2** `type` is 0, 1 or 2 (`PLANE_X`, `PLANE_Y`, `PLANE_Z`) for a plane whose normal
  is along that axis, and 3, 4 or 5 (`PLANE_ANYX` to `PLANE_ANYZ`) for one whose normal is
  closest to that axis. The loader copies `type` without checking it or the normal, so any
  value loads.
- **BSP-3.3** On load each plane gets `signbits`: bit `j` is set when `normal[j] < 0`.
  `-0.0` is not less than 0, so it sets no bit.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BSP_PLANE_SIZE` | 20 | `WinQuake/bspfile.h`, `sizeof(dplane_t)` |
| `BSP_PLANE_X`, `BSP_PLANE_Y`, `BSP_PLANE_Z` | 0, 1, 2 | `WinQuake/bspfile.h`, `PLANE_X`, `PLANE_Y`, `PLANE_Z` |
| `BSP_PLANE_ANYX`, `BSP_PLANE_ANYY`, `BSP_PLANE_ANYZ` | 3, 4, 5 | `WinQuake/bspfile.h`, `PLANE_ANYX`, `PLANE_ANYY`, `PLANE_ANYZ` |

## BSP-4 Textures

**Source:** `WinQuake/bspfile.h`, `dmiptexlump_t`, `miptex_t`, `MIPLEVELS`; `WinQuake/model.c`,
`Mod_LoadTextures` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

The textures lump holds the map's textures, copied by `qbsp` from the WAD2 files the map
names (MAP-6.3). Each texture is the same `miptex_t` a WAD2 texture lump holds (WAD-4.3).

| Structure | Offset | Size | Type | Field |
| --- | --- | --- | --- | --- |
| `dmiptexlump_t` | 0 | 4 | `int` | `nummiptex` |
| `dmiptexlump_t` | 4 + 4 * i | 4 | `int` | `dataofs[i]`, from the start of the lump |
| `miptex_t` | 0 | 16 | `char[16]` | `name` |
| `miptex_t` | 16 | 4 | `unsigned` | `width` |
| `miptex_t` | 20 | 4 | `unsigned` | `height` |
| `miptex_t` | 24 | 16 | `unsigned[4]` | `offsets`, from the start of the `miptex_t` |

### Behaviour

- **BSP-4.1** An empty textures lump loads with no textures; every texinfo then uses the
  engine's checkerboard texture with flags 0 (BSP-8.4).
- **BSP-4.2** Otherwise the lump starts with a signed 32-bit `nummiptex` and `nummiptex`
  signed 32-bit offsets, each from the start of the lump. An offset of -1 is a missing
  texture: that slot is empty and a texinfo naming it uses the checkerboard (BSP-8.4).
- **BSP-4.3** At each other offset is a 40-byte `miptex_t`: a 16-byte name, a 32-bit
  `width` and `height`, and four 32-bit offsets from the start of the `miptex_t`. The name is
  the bytes before the first NUL, or all 16.
- **BSP-4.4** `width` and `height` must each be a multiple of 16; otherwise loading fails
  with the error `Texture <name> is not 16 aligned` (fatal in id's engine).
- **BSP-4.5** The engine copies `width * height / 64 * 85` bytes from right after the
  40-byte header, the four mip levels of `width * height`, a quarter, a sixteenth and a
  sixty-fourth of that, and finds each level at its `offsets[j]`. A texture whose levels do
  not lie in those bytes, or whose bytes run past the lump, fails to load: id's code copies
  them unchecked, which is undefined, and this spec fixes the answer as an error. `qbsp`
  writes the levels in order right after the header (offsets 40, `40 + w*h`, and so on).
- **BSP-4.6** A negative `nummiptex`, or one whose offset table runs past the lump, fails to
  load, as does an offset other than -1 that points outside the lump (BSP-1.8).
- **BSP-4.7** A texture whose name starts with `sky` (compared exactly, case included) is a
  sky: the loader hands it to the renderer's `R_InitSky`.
- **BSP-4.8** A texture whose name starts with `+` is a frame of an animation. The second
  character, upper-cased if it is a letter, is `0` to `9` for frame 0 to 9 of the primary
  animation, or `A` to `J` for frame 0 to 9 of the alternate animation; anything else fails
  with the error `Bad animating texture <name>` (fatal in id's engine). Frames belong to the
  same animation when their names match from the third character on, case included.
- **BSP-4.9** An animation of `n` primary frames needs every frame from 0 to `n - 1`, where
  `n` is one more than the highest frame present, and likewise for the alternate frames; a
  gap fails with the error `Missing frame <i> of <name>`, where `<name>` is the first texture
  of that animation in lump order (fatal in id's engine). Each frame's next frame is the
  following one, wrapping to frame 0; each frame lasts `ANIM_CYCLE` (2) units of the
  animation's total of `2 * n`. When an animation has both kinds, every primary frame's
  alternate is alternate frame 0 and every alternate frame's alternate is primary frame 0.
- **BSP-4.10** When two textures claim the same frame of one animation, the later one in
  lump order takes the frame and the earlier one is not part of the cycle.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BSP_MIPTEX_HEADER_SIZE` | 40 | `WinQuake/bspfile.h`, `sizeof(miptex_t)` |
| `BSP_MIPLEVELS` | 4 | `WinQuake/bspfile.h`, `MIPLEVELS` |
| `BSP_ANIM_CYCLE` | 2 | `WinQuake/model.c`, `ANIM_CYCLE` in `Mod_LoadTextures` |

### Test vectors

See [`vectors/bsp/`](vectors/bsp/README.md): `not-aligned` (BSP-4.4), `missing-texture`
(BSP-4.2), `no-textures` (BSP-4.1), `anim`, `anim-missing` and `anim-bad` (BSP-4.8,
BSP-4.9), `sky` (BSP-4.7).

## BSP-5 Vertexes

**Source:** `WinQuake/bspfile.h`, `dvertex_t`; `WinQuake/model.c`, `Mod_LoadVertexes`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

### Behaviour

- **BSP-5.1** A vertex is 12 bytes: three floats, x, y and z, in map units.

## BSP-6 Visibility

**Source:** `WinQuake/model.c`, `Mod_LoadVisibility`, `Mod_DecompressVis`, `Mod_LeafPVS`,
`Mod_LoadBrushModel`; `WinQuake/world.c`, `SV_FindTouchedLeafs`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

The visibility lump holds, for each leaf, the set of leaves that can be seen from it (its
potentially visible set), run-length compressed. The server sends an entity to a client only
when one of the entity's leaves is in the set of the client's leaf.

### Behaviour

- **BSP-6.1** The visibility lump is raw bytes. A leaf's `visofs` (BSP-12.1) is the offset of
  its set in the lump, or -1 for none.
- **BSP-6.2** A set decompresses to a row of `(visleafs + 7) >> 3` bytes, where `visleafs` is
  model 0's `visleafs` (BSP-15.1). Bit `k` of the row (byte `k >> 3`, bit `k & 7`) is leaf
  `k + 1`: leaf 0 has no bit.
- **BSP-6.3** Decompression reads a byte: if it is not 0 it is copied to the row; if it is 0
  the next byte is a count and that many 0 bytes are written. It stops as soon as the row is
  full, checked after each copied byte or run, so a run may end past the row; only the row's
  bytes count. A run of count 0 writes nothing.
- **BSP-6.4** Leaf 0, and any leaf whose `visofs` is -1, sees every leaf: its row is all
  `0xFF`.
- **BSP-6.5** A set whose compressed bytes run past the end of the lump before the row is
  full fails to decompress. id's code reads past the lump, which is undefined; this spec
  fixes the answer as an error for that leaf's set.
- **BSP-6.6** Leaf 0's `visofs` is never read (BSP-6.4), so any value loads. When the
  visibility lump is empty, `Mod_LoadVisibility` keeps no data (`loadmodel->visdata = NULL`)
  and `Mod_LoadLeafs` sets a leaf's set to `loadmodel->visdata + p`, so a `visofs` of 0 gives
  no set, which `Mod_DecompressVis` reads as every leaf visible, as -1 does. Any other
  `visofs` but -1 into an empty lump fails to load (BSP-1.8). `qbsp`'s `BeginBSPFile` sets
  only leaf 0's `contents` in its zeroed leaf array, so leaf 0's `visofs` is 0, also in a
  BSP that `vis` has not processed.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BSP_MAX_MAP_LEAFS` | 8192 | `WinQuake/bspfile.h`, `MAX_MAP_LEAFS`; the size of `Mod_DecompressVis`'s row buffer is `MAX_MAP_LEAFS/8` |

### Test vectors

See [`vectors/bsp/`](vectors/bsp/README.md): `base` (leaf 1's row is `01`) and `vis`
(BSP-6.2 to BSP-6.4).

## BSP-7 Nodes

**Source:** `WinQuake/bspfile.h`, `dnode_t`; `WinQuake/model.c`, `Mod_LoadNodes`,
`Mod_SetParent`, `Mod_MakeHull0` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

| Offset | Size | Type | Field |
| --- | --- | --- | --- |
| 0 | 4 | `int` | `planenum` |
| 4 | 4 | `short[2]` | `children` |
| 8 | 6 | `short[3]` | `mins` |
| 14 | 6 | `short[3]` | `maxs` |
| 20 | 2 | `unsigned short` | `firstface` |
| 22 | 2 | `unsigned short` | `numfaces` |

### Behaviour

- **BSP-7.1** A node is the 24-byte `dnode_t`. It splits space by plane `planenum`:
  `children[0]` is the front and `children[1]` the back.
- **BSP-7.2** A child `c` of 0 or more is node `c`; a negative child is leaf `-1 - c`, so
  -1 is leaf 0.
- **BSP-7.3** `mins` and `maxs` are the node's bounding box in whole map units, for culling.
- **BSP-7.4** `firstface` and `numfaces` are the faces (BSP-9) that lie on the node's plane,
  read as unsigned 16-bit values.
- **BSP-7.5** The nodes of the drawing hull are also collision hull 0: each node becomes a
  clipnode with the same plane, a child node keeping its number and a child leaf replaced by
  that leaf's `contents` (BSP-12.2).
- **BSP-7.6** A BSP with no nodes fails to load. id's code reads node 0 regardless
  (`Mod_SetParent`), which is undefined; this spec fixes the answer as an error.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BSP_NODE_SIZE` | 24 | `WinQuake/bspfile.h`, `sizeof(dnode_t)` |

## BSP-8 Texinfo

**Source:** `WinQuake/bspfile.h`, `texinfo_t`, `TEX_SPECIAL`; `WinQuake/model.c`,
`Mod_LoadTexinfo` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

| Offset | Size | Type | Field |
| --- | --- | --- | --- |
| 0 | 16 | `float[4]` | `vecs[0]`: s axis x, y, z and offset |
| 16 | 16 | `float[4]` | `vecs[1]`: t axis x, y, z and offset |
| 32 | 4 | `int` | `miptex` |
| 36 | 4 | `int` | `flags` |

### Behaviour

- **BSP-8.1** A texinfo is the 40-byte `texinfo_t`. A point `p` on a face using it has the
  texture coordinates `s = dot(vecs[0].xyz, p) + vecs[0][3]` and
  `t = dot(vecs[1].xyz, p) + vecs[1][3]`, in texels.
- **BSP-8.2** `miptex` is an index into the textures lump's slots (BSP-4.2).
- **BSP-8.3** When the textures lump is not empty, a `miptex` of `nummiptex` or more fails
  with the error `miptex >= loadmodel->numtextures` (fatal in id's engine). A negative
  `miptex` is not checked and indexes before the array, which is undefined; this spec fixes
  the answer as an error.
- **BSP-8.4** When the textures lump is empty, or the slot is a missing texture, the
  texinfo uses the engine's checkerboard texture and its flags become 0, whatever the file
  says.
- **BSP-8.5** `flags` bit 0 (`TEX_SPECIAL`, 1) marks a texture with no lightmap and no size
  limit: sky and liquids. `qbsp` sets it for textures whose names start with `*` or with
  `sky` in any case (MAP-4.5). Other bits are kept and not interpreted.
- **BSP-8.6** On load each texinfo gets a mip bias from the mean length `m` of its two axes:
  4 when `m < 0.32`, 3 when `m < 0.49`, 2 when `m < 0.99`, otherwise 1. The lengths and their
  mean are single-precision floats, compared with the constants as doubles.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BSP_TEXINFO_SIZE` | 40 | `WinQuake/bspfile.h`, `sizeof(texinfo_t)` |
| `BSP_TEX_SPECIAL` | 1 | `WinQuake/bspfile.h`, `TEX_SPECIAL` |

### Test vectors

See [`vectors/bsp/`](vectors/bsp/README.md): `bad-miptex` (BSP-8.3), `missing-texture` and
`no-textures` (BSP-8.4), `mipadjust` (BSP-8.6).

## BSP-9 Faces

**Source:** `WinQuake/bspfile.h`, `dface_t`, `MAXLIGHTMAPS`; `WinQuake/model.h` and
`WinQuake/gl_model.h`, `msurface_t`; `WinQuake/model.c`, `Mod_LoadFaces`,
`CalcSurfaceExtents`; `WinQuake/gl_model.c`, `CalcSurfaceExtents`; `WinQuake/gl_rsurf.c`,
`R_BuildLightMap`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

| Offset | Size | Type | Field |
| --- | --- | --- | --- |
| 0 | 2 | `short` | `planenum` |
| 2 | 2 | `short` | `side` |
| 4 | 4 | `int` | `firstedge` |
| 8 | 2 | `short` | `numedges` |
| 10 | 2 | `short` | `texinfo` |
| 12 | 4 | `byte[4]` | `styles` |
| 16 | 4 | `int` | `lightofs` |

### Behaviour

- **BSP-9.1** A face is the 20-byte `dface_t`: a convex polygon on plane `planenum`, facing
  the plane's front when `side` is 0 and its back for any other value.
- **BSP-9.2** The face's vertices are the `numedges` surfedges from `firstedge` (BSP-14.2),
  in order.
- **BSP-9.3** **Withdrawn** 2026-10-16: it said the texture coordinates are computed in
  single precision, but id's original x86 builds compute them on the x87 with exact products
  and one rounding to single precision, which gives 247 faces of LibreQuake's maps another
  `texturemins` or extent; replaced by BSP-9.10, BSP-9.11 and BSP-9.12.
- **BSP-9.4** When the texinfo's flags (after BSP-8.4) do not have `TEX_SPECIAL`, an extent
  over 256 fails with the error `Bad surface extents` (fatal in id's engine); 256 loads.
- **BSP-9.5** A face whose texture name starts with `sky` is drawn as sky; one whose name
  starts with `*` is a liquid, drawn warped, and its extents become 16384 and its
  `texturemins` -8192 after the check of BSP-9.4.
- **BSP-9.6** `styles` are up to four light styles; the first 255 ends the list. `lightofs`
  is -1 for a face with no light samples, otherwise the offset of its samples in the
  lighting lump.
- **BSP-9.7** A face's lightmap is `smax = (extents[0] >> 4) + 1` samples wide and
  `tmax = (extents[1] >> 4) + 1` high, one sample per 16 texels, stored row by row; there is
  one lightmap of `smax * tmax` samples for each style before the first 255, one after
  another from `lightofs`.
- **BSP-9.8** `planenum`, `numedges` and `texinfo` are signed 16-bit and `firstedge` signed
  32-bit; a negative value, or one that names no element, fails to load (BSP-1.8).
- **BSP-9.9** **Withdrawn** 2026-10-16: it said the products and the difference that follow
  the conversion wrap as 32-bit x86 integers and that such a face fails BSP-9.4 unless its
  texinfo has `TEX_SPECIAL`, but id's loader stores `texturemins` and `extents` in 16-bit
  fields and checks the stored value, so an extent of 65536 + 16 loads, and a face whose
  bounds both convert to the indefinite value has extent 0 and loads; replaced by BSP-9.13
  and BSP-9.14.
- **BSP-9.10** The face's texture extents are computed from its vertices' texture
  coordinates (BSP-8.1, BSP-9.11). For each axis the running minimum starts at 999999 and the
  maximum at -99999, and each vertex's coordinate in turn may replace them (BSP-9.12); then
  `bmin = floor(min / 16)` and `bmax = ceil(max / 16)`, `texturemins = bmin * 16` and
  `extents = (bmax - bmin) * 16`.
- **BSP-9.11** A vertex `(x, y, z)`'s texture coordinate on an axis `(s0, s1, s2, offset)`
  is computed as id's x86 builds of WinQuake and GLQuake compute it: the three products, each
  exact, are added in the order `((x * s0 + z * s2) + y * s1) + offset`, each sum rounded to
  53 bits (double precision, round to nearest even), and the result is rounded once to single
  precision. It is not single-precision arithmetic, and the order is not C's. Once WinQuake
  has drawn a frame it adds at 64 bits instead, and id's DOS and Linux builds add in C's
  order (see the evidence below); these answers differ only when a sum lies so close to a
  midpoint of two singles that the order or the precision of the additions decides how it
  rounds (vectors `x87-order` and `x87-pc64`), which no face of LibreQuake's maps does. This
  spec takes 53 bits, the precision GLQuake always uses.
- **BSP-9.12** A coordinate replaces the running minimum unless it is greater than or equal
  to it, and the running maximum unless the maximum is greater than or equal to it, as
  WinQuake's and GLQuake's x86 code compares: a NaN coordinate replaces both, and the next
  coordinate replaces a NaN minimum or maximum. A bound that is still NaN after the last
  vertex gives the integer indefinite value of BSP-9.13.
- **BSP-9.13** `CalcSurfaceExtents` stores `floor(min / 16)` and `ceil(max / 16)` in `int`
  variables, which is undefined in C when the value is NaN or outside the `int` range. This
  spec takes the answer of the original x86 build, whose conversion gives the integer
  indefinite value 0x80000000 (-2147483648) there; BSP-9.14 then keeps the low 16 bits of
  what BSP-9.10 computes from it.
- **BSP-9.14** `texturemins` and `extents` are `short` fields of `msurface_t`
  (`WinQuake/model.h` and `WinQuake/gl_model.h`: `short texturemins[2];` and
  `short extents[2];`), so `CalcSurfaceExtents` stores the low 16 bits of each value
  BSP-9.10 computes, read as a signed two's-complement value: a value from -32768 to 32767
  is kept, an extent of 65552 is stored as 16 and one of 32784 as -32752, and a
  `texturemins` of 32768 as -32768. The check of BSP-9.4 and the lightmap size of BSP-9.7
  use the stored value, so a stored extent of 256 or less, negative ones included, passes
  BSP-9.4. The conversion to `short` is implementation-defined in C; this spec takes the
  answer of id's x86 builds, which compute, store and compare the 16-bit value (see the
  evidence below).
- **BSP-9.15** GLQuake's `CalcSurfaceExtents` (`WinQuake/gl_model.c`) checks
  `s->extents[i] > 512 /* 256 */` where WinQuake's checks `s->extents[i] > 256` (BSP-9.4):
  when the texinfo's flags do not have `TEX_SPECIAL`, a stored extent (BSP-9.14) over 512
  fails with `Bad surface extents` and 512 loads, so a face with an extent from 272 to 512
  loads in GLQuake and fails in WinQuake. `crates/qs-bsp` applies the limit of the loader its
  caller names, WinQuake's or GLQuake's, and has no default.

### Evidence for BSP-9.11 and BSP-9.12

`CalcSurfaceExtents` declares `val` a `float` and writes the sum as one C expression, which
leaves the precision of the evaluation to the compiler. The answer is taken from id's own x86
executables, disassembled with `llvm-objdump` from the archives id released (fetched
2026-10-16 from `https://ftp.idtech.space/idstuff/`; not committed):

| Build | Archive, file and SHA-256 | Compiler |
| --- | --- | --- |
| WinQuake 1.00 | `quake/wq100.zip`, `WINQUAKE.EXE` `e0f12ec4dc4026d7aaa949f96b237fdc773e1546fb99d44be707f01c1f28b3b9` | Microsoft Visual C++ |
| GLQuake, March 1997 | `unsup/glq3_28.zip`, `GLQUAKE.EXE` `1d0ef75e8ee8de9a760dfbbd5862fbaa7b4d1e2740c726e29dd37bd5208adb3e` | Microsoft Visual C++ |
| Quake 1.08 for DOS | `quake/quake108.zip`, `quake.exe` `e0a1936400ca56e94d44d585097bd9e490035fcfb6e7d806d7e879f423928642` | DJGPP (gcc) |
| Quake for Linux 1.0 | `unsup/unix/quake.x11-1.0-i386-unknown-linux2.0.tar.gz`, `quake.x11` `b89c9b5fbdedc86e719429b02fb363cbc2785f8e984e79ab6493375c2985cfee` | gcc (symbols kept) |

`CalcSurfaceExtents` is found by its reference to `Bad surface extents`, and in `quake.x11`
by its symbol. WinQuake and GLQuake compute `val` with the same instructions (WinQuake from
`0x417b22`, GLQuake from `0x42c372`; `edi` points at `vecs[j][1]` and `ebx` at the vertex):

```
flds   -0x4(%edi)      ; s0
fmuls  (%ebx)          ; * x
flds   0x4(%edi)       ; s2
fmuls  0x8(%ebx)       ; * z
flds   (%edi)          ; s1
fmuls  0x4(%ebx)       ; * y
fxch   %st(1)
faddp  %st, %st(2)     ; x*s0 + z*s2
faddp  %st, %st(1)     ; + y*s1
fadds  0x8(%edi)       ; + offset
fcoms  0x20(%esp,%ebp) ; compare with mins[j]
fstps  0x10(%esp)      ; store val, rounded to single
fnstsw %ax
testb  $0x1, %ah       ; C0: less than, or unordered
je     ...             ; if set, mins[j] = val
flds   0x28(%esp,%ebp) ; maxs[j]
fcomps 0x10(%esp)      ; compare with val
fnstsw %ax
testb  $0x1, %ah       ; C0: maxs[j] less than val, or unordered
```

- **Exact products, one rounding to single.** The products and sums stay in x87 registers,
  at the precision the control word sets, and `val` is rounded to single precision only when
  `fstps` stores it. A product of two singles has at most 48 significant bits, so it is exact
  at 53 or 64 bits. `floor` and `ceil` are given `mins` and `maxs` reloaded from memory,
  already single.
- **53 bits.** Both executables' C run-time start-up calls `_controlfp(0x10000, 0x30000)`
  (`WINQUAKE.EXE` from `0x458bef`, reached through the floating-point initialisation pointer
  that `0x459260` calls; `GLQUAKE.EXE` from `0x43a6af`), which is `_PC_53` under the mask
  `_MCW_PC` in the table of Microsoft's
  [`_control87`, `_controlfp`](https://learn.microsoft.com/en-us/cpp/c-runtime-library/reference/control87-controlfp-control87-2)
  ("`_PC_53` (53 bits)" 0x00010000, "`_MCW_PC` (Precision control)" 0x00030000). In the
  Windows builds' source, `fldcw` appears only in `WinQuake/sys_wina.s` and in the software
  renderer's `r_drawa.s` and `d_polysa.s` (`sys_dosa.s` is DOS's). `Sys_SetFPCW` computes
  `full_cw` ("round mode, 64-bit precision") and `single_cw` ("chop mode, single precision")
  without loading either; `Sys_PushFPCW_SetHigh` and `Sys_PopFPCW` put back what they found;
  and the only callers of `Sys_LowFPPrecision` and `Sys_HighFPPrecision` are in the software
  renderer's `R_RenderView` (`WinQuake/r_main.c`), which loads `single_cw` before it draws
  and `full_cw` after. GLQuake's renderer (`WinQuake/gl_rmain.c`) calls neither, so GLQuake
  loads every map at 53 bits (unless an OpenGL driver changes the control word, which is
  outside id's code); WinQuake loads at 53 bits until it has drawn its first frame
  and at 64 bits after. The start-up precision of the DOS and Linux builds was not checked.
- **The order.** Visual C++ adds `x * s0 + z * s2` first; the gcc builds add in C's order
  (`quake.x11` from `0x8066a71`, `quake.exe` from `0x1be78`).
- **The comparisons.** `fcom` sets C0 when the first operand is less than the second or the
  two are unordered, and Visual C++ tests C0 alone, so a NaN replaces the bound (BSP-9.12).
  The gcc builds test C3, C2 and C0 together and follow C, where a NaN replaces nothing.

**Checked:** on 2026-10-16 id's `Mod_LoadBrushModel` (`WinQuake/model.c` from the pinned
commit, in the harness the vectors' README describes) was built with gcc 14.4 for x86-64 and
run in an emulated `linux/amd64` container on Apple silicon, with `val` computed by the
instruction sequences above in inline x87 assembly under control word 0x027F (53 bits) or
0x037F (64 bits), and with C's or Visual C++'s comparisons. A probe first showed that the
emulated x87 honours the precision field: `(1 + 2^-60) - 1` gives 2^-60 at 64 bits and 0 at
53 and 24 bits, and `1 / 3` at 24 bits differs from the double. Over the 23 BSPs of
LibreQuake's `lite` release, WinQuake's and the gcc builds' sequences at 53 and at 64 bits
give every coordinate the same value; single precision gives 8,585 coordinate evaluations
another value and 247 faces of ten maps another `texturemins` or extent. `crates/qs-bsp`
printed every face as the WinQuake sequence at 53 bits with its comparisons did, on all 23
maps and the three vectors `x87-order`, `x87-pc64` and `nan-vertex`.

### Evidence for BSP-9.14

`CalcSurfaceExtents` ends:

```c
		s->texturemins[i] = bmins[i] * 16;
		s->extents[i] = (bmaxs[i] - bmins[i]) * 16;
		if ( !(tex->flags & TEX_SPECIAL) && s->extents[i] > 256)
			Sys_Error ("Bad surface extents");
```

`WINQUAKE.EXE` (the executable in the table above) does this in 16-bit registers, from
`0x417bc9`, after the calls to `floor` and `ceil` and the conversions to `int`, with `%esi`
pointing at `extents[i]`:

```
movw   0x30(%esp,%edi), %cx  ; low 16 bits of bmins[i]
movl   0x14(%esp), %ebx      ; tex
movl   %eax, 0x18(%esp,%edi) ; bmaxs[i], from ceil
movw   %cx, %ax
movl   0x28(%ebx), %edx      ; tex->flags
shlw   $0x4, %ax             ; * 16
movw   %ax, -0x4(%esi)       ; texturemins[i]
movl   0x18(%esp,%edi), %eax
subw   %cx, %ax              ; bmaxs[i] - bmins[i], 16 bits
shlw   $0x4, %ax             ; * 16
testb  $0x1, %dl             ; TEX_SPECIAL
movw   %ax, (%esi)           ; extents[i]
jne    ...
cmpw   $0x100, %ax           ; the 16-bit extent against 256
jle    ...                   ; signed: not greater, no error
```

`GLQUAKE.EXE` has the same sequence from `0x42c419`, comparing with `cmpw $0x200`: GLQuake's
`WinQuake/gl_model.c` checks `s->extents[i] > 512 /* 256 */`. The low 16 bits of a product or
difference do not depend on the bits above them, so computing in 32 bits and keeping the low
16 bits, as BSP-9.10 and BSP-9.14 state it, gives the same values.

**Checked:** on 2026-10-16 the vectors `extents-65552`, `extents-32784` and
`texturemins-32768` were loaded by id's `Mod_LoadBrushModel`, in `WinQuake/model.c` copied
unchanged from the pinned commit and built for arm64 with Apple clang and for x86-64 with gcc
14.4 (in an emulated `linux/amd64` container), in the harness the vectors' README describes;
both builds gave the results the README records. The same x86-64 build, given the base with
the s axis offset -1e30 and `lightofs` -1, failed with `Bad surface extents`, and with
texinfo flags 1 loaded with `texturemins` 0 0 and extents 31088 64 (BSP-9.13: the minimum's
indefinite value, and a maximum still -99999).

### Evidence for BSP-9.15

`WinQuake/model.c` and `WinQuake/gl_model.c` at the pinned commit differ in
`CalcSurfaceExtents` only in the check:

```c
		if ( !(tex->flags & TEX_SPECIAL) && s->extents[i] > 256)          /* model.c */
		if ( !(tex->flags & TEX_SPECIAL) && s->extents[i] > 512 /* 256 */ ) /* gl_model.c */
```

The released executables agree: `WINQUAKE.EXE` compares the stored 16-bit extent with
`cmpw $0x100, %ax` and `GLQUAKE.EXE` with `cmpw $0x200, %ax` at `0x42c447`, each followed by
a signed `jle` past the call to `Sys_Error` (the sequences under "Evidence for BSP-9.14").
Which limit a map must keep to is a property of the engine that loads it, so `qs-bsp` takes
it from its caller rather than choosing one engine for everyone.

GLQuake loads such a face, but its renderer then builds the face's lightmap in
`WinQuake/gl_rsurf.c`'s `unsigned blocklights[18*18];`, and `R_BuildLightMap` writes
`smax * tmax` entries there (`size = smax*tmax;` ... `blocklights[i] = 255*256;`), more
than its 324 for a face of more than 324 samples, such as one of 512 by 512 texels (33 by 33
samples): the load succeeds and the drawing runs past the array. This section describes the
load only.

**Checked:** on 2026-10-16 id's `Mod_LoadBrushModel` in `WinQuake/gl_model.c`, copied
unchanged from the pinned commit with `gl_model.h`, `bspfile.h`, `modelgen.h` and
`spritegn.h`, was built in the harness the vectors' README describes, with the GL calls it
makes (`GL_LoadTexture`, `GL_SubdivideSurface`, `Cvar_RegisterVariable`) stubbed, for arm64
with Apple clang and for x86-64 with gcc 14.4 in an emulated `linux/amd64` container, beside
the WinQuake build of `WinQuake/model.c`. Both builds of each loader gave the results the
README records for `extents-256`, `extents-272`, `extents-272-special`, `extents-512`,
`extents-528` and `extents-528-special`.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BSP_FACE_SIZE` | 20 | `WinQuake/bspfile.h`, `sizeof(dface_t)` |
| `BSP_MAXLIGHTMAPS` | 4 | `WinQuake/bspfile.h`, `MAXLIGHTMAPS` |
| `BSP_MAX_SURFACE_EXTENT` | 256 | `WinQuake/model.c`, `CalcSurfaceExtents`: `s->extents[i] > 256` |
| `BSP_GL_MAX_SURFACE_EXTENT` | 512 | `WinQuake/gl_model.c`, `CalcSurfaceExtents`: `s->extents[i] > 512 /* 256 */` |
| `BSP_LIGHTMAP_SCALE` | 16 | `WinQuake/model.c`, `CalcSurfaceExtents`; `WinQuake/gl_rsurf.c`, `R_BuildLightMap`: `(surf->extents[0]>>4)+1` |

### Test vectors

See [`vectors/bsp/`](vectors/bsp/README.md): `base`, `texture-offset`, `extents-256`,
`extents-272`, `extents-272-special` (BSP-9.4, BSP-9.10), `sky` and `turb` (BSP-9.5),
`nolight-back` (BSP-9.1, BSP-9.6), `x87-order` and `x87-pc64` (BSP-9.11), `nan-vertex`
(BSP-9.12), `extents-65552`, `extents-32784` and `texturemins-32768` (BSP-9.14),
`extents-512`, `extents-528` and `extents-528-special` (BSP-9.15, with `extents-256`,
`extents-272` and `extents-272-special` as GLQuake loads them).

## BSP-10 Lighting

**Source:** `WinQuake/model.c`, `Mod_LoadLighting`; `WinQuake/gl_rsurf.c`, `R_BuildLightMap`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

### Behaviour

- **BSP-10.1** The lighting lump is one unsigned byte per lightmap sample, 0 darkest; faces
  index it by `lightofs` (BSP-9.6, BSP-9.7). An empty lump means the map has no light data
  and is drawn fully bright.
- **BSP-10.2** A face's samples (`smax * tmax` times its number of styles from `lightofs`)
  must lie in the lump; otherwise it fails to load (BSP-1.8).

## BSP-11 Clipnodes and hulls

**Source:** `WinQuake/bspfile.h`, `dclipnode_t`, `MAX_MAP_HULLS`, `CONTENTS_*`;
`WinQuake/model.c`, `Mod_LoadClipnodes`, `Mod_LoadBrushModel`; `WinQuake/world.c`,
`SV_HullForEntity` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

| Offset | Size | Type | Field |
| --- | --- | --- | --- |
| 0 | 4 | `int` | `planenum` |
| 4 | 4 | `short[2]` | `children` |

### Behaviour

- **BSP-11.1** A clipnode is the 8-byte `dclipnode_t`: plane `planenum` and a front and back
  child. A child of 0 or more is that clipnode; a negative child is a contents value
  (BSP-12.2) ending the walk.
- **BSP-11.2** The clipnodes lump holds collision hulls 1 and 2 of every model; a model's
  `headnode[1]` and `headnode[2]` (BSP-15.1) are their first clipnodes. Hull 0 is the nodes
  (BSP-7.5).
- **BSP-11.3** Hull 1 is the hull for an entity of size (-16, -16, -24) to (16, 16, 32), the
  player, and hull 2 for (-32, -32, -24) to (32, 32, 64). A `SOLID_BSP` entity is collided
  with hull 0 when the other entity's x size is under 3, hull 1 when it is at most 32, and
  hull 2 otherwise. Hull 3 is not used.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BSP_CLIPNODE_SIZE` | 8 | `WinQuake/bspfile.h`, `sizeof(dclipnode_t)` |
| `BSP_MAX_MAP_HULLS` | 4 | `WinQuake/bspfile.h`, `MAX_MAP_HULLS` |
| `BSP_HULL1_MINS`, `BSP_HULL1_MAXS` | (-16, -16, -24), (16, 16, 32) | `WinQuake/model.c`, `Mod_LoadClipnodes` |
| `BSP_HULL2_MINS`, `BSP_HULL2_MAXS` | (-32, -32, -24), (32, 32, 64) | `WinQuake/model.c`, `Mod_LoadClipnodes` |

## BSP-12 Leaves

**Source:** `WinQuake/bspfile.h`, `dleaf_t`, `CONTENTS_*`, `AMBIENT_*`, `NUM_AMBIENTS`;
`WinQuake/model.c`, `Mod_LoadLeafs`; `WinQuake/common.c`, `ShortNoSwap`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

| Offset | Size | Type | Field |
| --- | --- | --- | --- |
| 0 | 4 | `int` | `contents` |
| 4 | 4 | `int` | `visofs` |
| 8 | 6 | `short[3]` | `mins` |
| 14 | 6 | `short[3]` | `maxs` |
| 20 | 2 | `unsigned short` | `firstmarksurface` |
| 22 | 2 | `unsigned short` | `nummarksurfaces` |
| 24 | 4 | `byte[4]` | `ambient_level` |

### Behaviour

- **BSP-12.1** A leaf is the 28-byte `dleaf_t`: a convex region of space with one contents
  value, its visibility offset (BSP-6.1), its bounding box in whole map units, the range of
  marksurfaces (BSP-13) listing the faces that can be seen in it, and four ambient sound
  levels.
- **BSP-12.2** `contents` is one of the `CONTENTS_` values: -1 empty, -2 solid, -3 water, -4
  slime, -5 lava, -6 sky. The loader copies it without checking.
- **BSP-12.3** Leaf 0 is the shared solid leaf that every solid region points to; `qbsp`
  gives it `CONTENTS_SOLID` and no faces (`qutils/QBSP/WRITEBSP.C`, `BeginBSPFile`: "leaf 0
  is common solid with no faces"), and it has no visibility bit (BSP-6.2).
- **BSP-12.4** `ambient_level` is the volume, 0 to 255, of the water, sky, slime and lava
  ambient sounds, in that order.
- **BSP-12.5** `firstmarksurface` and `nummarksurfaces` are read as unsigned 16-bit values.
  id's loader reads them through `LittleShort`, which returns a signed `short`, so a value
  of 32768 or more indexes before the array, which is undefined; this spec reads them as the
  file declares them, as QS does.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BSP_LEAF_SIZE` | 28 | `WinQuake/bspfile.h`, `sizeof(dleaf_t)` |
| `BSP_CONTENTS_EMPTY` | -1 | `WinQuake/bspfile.h`, `CONTENTS_EMPTY` |
| `BSP_CONTENTS_SOLID` | -2 | `WinQuake/bspfile.h`, `CONTENTS_SOLID` |
| `BSP_CONTENTS_WATER` | -3 | `WinQuake/bspfile.h`, `CONTENTS_WATER` |
| `BSP_CONTENTS_SLIME` | -4 | `WinQuake/bspfile.h`, `CONTENTS_SLIME` |
| `BSP_CONTENTS_LAVA` | -5 | `WinQuake/bspfile.h`, `CONTENTS_LAVA` |
| `BSP_CONTENTS_SKY` | -6 | `WinQuake/bspfile.h`, `CONTENTS_SKY` |
| `BSP_NUM_AMBIENTS` | 4 | `WinQuake/bspfile.h`, `NUM_AMBIENTS`, `AMBIENT_WATER` 0 to `AMBIENT_LAVA` 3 |

## BSP-13 Marksurfaces

**Source:** `WinQuake/model.c`, `Mod_LoadMarksurfaces`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

### Behaviour

- **BSP-13.1** The marksurfaces lump is a list of 16-bit face numbers, read as unsigned
  (as BSP-12.5); a leaf's faces are its `nummarksurfaces` entries from `firstmarksurface`.
- **BSP-13.2** A face number that is not below the number of faces fails with the error
  `Mod_ParseMarksurfaces: bad surface number` (fatal in id's engine).

## BSP-14 Edges and surfedges

**Source:** `WinQuake/bspfile.h`, `dedge_t`; `WinQuake/model.c`, `Mod_LoadEdges`,
`Mod_LoadSurfedges`, `CalcSurfaceExtents` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

### Behaviour

- **BSP-14.1** An edge is 4 bytes: two unsigned 16-bit vertex numbers, `v[0]` and `v[1]`.
  Edge 0 is never used, because its number cannot be negated; `qbsp` writes it as a
  placeholder.
- **BSP-14.2** A surfedge is a signed 32-bit edge number. A value `e` of 0 or more uses edge
  `e` from `v[0]` to `v[1]`, so the face's vertex is `v[0]`; a negative value uses edge `-e`
  backwards, so the vertex is its `v[1]`.
- **BSP-14.3** A surfedge of -2147483648 has no positive counterpart and fails to load. id's
  code negates it, which is undefined; this spec fixes the answer as an error.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BSP_EDGE_SIZE` | 4 | `WinQuake/bspfile.h`, `sizeof(dedge_t)` |
| `BSP_SURFEDGE_SIZE` | 4 | `WinQuake/model.c`, `Mod_LoadSurfedges`: `int` elements |

## BSP-15 Models

**Source:** `WinQuake/bspfile.h`, `dmodel_t`; `WinQuake/model.c`, `Mod_LoadSubmodels`,
`Mod_LoadBrushModel`, `RadiusFromBounds`; `WinQuake/sv_main.c`, `SV_SpawnServer`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

| Offset | Size | Type | Field |
| --- | --- | --- | --- |
| 0 | 12 | `float[3]` | `mins` |
| 12 | 12 | `float[3]` | `maxs` |
| 24 | 12 | `float[3]` | `origin` |
| 36 | 16 | `int[4]` | `headnode` |
| 52 | 4 | `int` | `visleafs` |
| 56 | 4 | `int` | `firstface` |
| 60 | 4 | `int` | `numfaces` |

### Behaviour

- **BSP-15.1** A model is the 64-byte `dmodel_t`: its bounds, an origin, the first node of
  hull 0 and first clipnode of hulls 1 to 3 in `headnode`, the number of leaves with
  visibility (not counting leaf 0), and the range of faces drawn for it.
- **BSP-15.2** Model 0 is the world. Its `visleafs` sets the width of every visibility row
  (BSP-6.2).
- **BSP-15.3** Model `n` above 0 is the brush model named `*<n>`, which a brush entity
  uses through its `model` key. The server precaches the world as model 1 and `*<n>` as
  model `n + 1`.
- **BSP-15.4** On load each model's `mins` are lowered and `maxs` raised by 1 on every axis,
  and its radius is the length of the vector whose components are the larger absolute value
  of `mins[j]` and `maxs[j]`, after that change.
- **BSP-15.5** `origin` is read and not used by id's engine.
- **BSP-15.6** A BSP with no models fails to load. id's loader then leaves the world without
  bounds or hull heads; this spec fixes the answer as an error.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BSP_MODEL_SIZE` | 64 | `WinQuake/bspfile.h`, `sizeof(dmodel_t)` |

### Test vectors

See [`vectors/bsp/`](vectors/bsp/README.md): `base` has two models (BSP-15.1 to BSP-15.4).

## BSP-16 Compiler limits

**Source:** `WinQuake/bspfile.h`, `MAX_MAP_*`, `MAX_KEY`, `MAX_VALUE`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

### Behaviour

- **BSP-16.1** The `MAX_MAP_` limits below are the sizes of `qbsp`'s and the other tools'
  arrays; id's engine checks none of them when it loads a BSP.
- **BSP-16.2** A BSP29 file cannot refer past what its fields hold: a node child names node
  0 to 32767 or leaf 0 to 32767 (BSP-7.2), a clipnode child names clipnode 0 to 32767
  (BSP-11.1), an edge names vertex 0 to 65535 (BSP-14.1), a face names plane and texinfo 0
  to 32767 (BSP-9.8), and a node's faces and a marksurface name face 0 to 65535 (BSP-7.4,
  BSP-13.1).

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BSP_MAX_MAP_MODELS` | 256 | `WinQuake/bspfile.h`, `MAX_MAP_MODELS` |
| `BSP_MAX_MAP_BRUSHES` | 4096 | `WinQuake/bspfile.h`, `MAX_MAP_BRUSHES` |
| `BSP_MAX_MAP_ENTITIES` | 1024 | `WinQuake/bspfile.h`, `MAX_MAP_ENTITIES` |
| `BSP_MAX_MAP_ENTSTRING` | 65536 | `WinQuake/bspfile.h`, `MAX_MAP_ENTSTRING` |
| `BSP_MAX_MAP_PLANES` | 32767 | `WinQuake/bspfile.h`, `MAX_MAP_PLANES` |
| `BSP_MAX_MAP_NODES` | 32767 | `WinQuake/bspfile.h`, `MAX_MAP_NODES` |
| `BSP_MAX_MAP_CLIPNODES` | 32767 | `WinQuake/bspfile.h`, `MAX_MAP_CLIPNODES` |
| `BSP_MAX_MAP_VERTS` | 65535 | `WinQuake/bspfile.h`, `MAX_MAP_VERTS` |
| `BSP_MAX_MAP_FACES` | 65535 | `WinQuake/bspfile.h`, `MAX_MAP_FACES` |
| `BSP_MAX_MAP_MARKSURFACES` | 65535 | `WinQuake/bspfile.h`, `MAX_MAP_MARKSURFACES` |
| `BSP_MAX_MAP_TEXINFO` | 4096 | `WinQuake/bspfile.h`, `MAX_MAP_TEXINFO` |
| `BSP_MAX_MAP_EDGES` | 256000 | `WinQuake/bspfile.h`, `MAX_MAP_EDGES` |
| `BSP_MAX_MAP_SURFEDGES` | 512000 | `WinQuake/bspfile.h`, `MAX_MAP_SURFEDGES` |
| `BSP_MAX_MAP_TEXTURES` | 512 | `WinQuake/bspfile.h`, `MAX_MAP_TEXTURES` |
| `BSP_MAX_MAP_MIPTEX` | 0x200000 | `WinQuake/bspfile.h`, `MAX_MAP_MIPTEX` |
| `BSP_MAX_MAP_LIGHTING` | 0x100000 | `WinQuake/bspfile.h`, `MAX_MAP_LIGHTING` |
| `BSP_MAX_MAP_VISIBILITY` | 0x100000 | `WinQuake/bspfile.h`, `MAX_MAP_VISIBILITY` |
| `BSP_MAX_KEY` | 32 | `WinQuake/bspfile.h`, `MAX_KEY` |
| `BSP_MAX_VALUE` | 1024 | `WinQuake/bspfile.h`, `MAX_VALUE` |

## BSP-17 BSP2

**Source:** QS `Quake/bspfile.h`, `BSP2VERSION_2PSB`, `BSP2VERSION_BSP2`, `dl1node_t`,
`dl2node_t`, `dlclipnode_t`, `dledge_t`, `dlface_t`, `dl1leaf_t`, `dl2leaf_t`; QS
`Quake/gl_model.c`, `Mod_LoadBrushModel`, `Mod_LoadEdges`, `Mod_LoadFaces`,
`Mod_LoadMarksurfaces`, `Mod_LoadLeafs`, `Mod_LoadNodes`, `Mod_LoadClipnodes`;
ericw-tools `include/common/bspfile_q1.hh`, `BSP2VERSION`, `BSP2RMQVERSION`.

BSP2 widens the 16-bit fields of BSP29 to 32 bits so a map can exceed BSP-16's limits. It
has two variants, both read by QS and written by ericw-tools: `2PSB`, the first, from the
RMQ project, which QS calls "first iteration", and `BSP2`, which QS calls "sanitised
revision" and which also makes node and leaf bounds floats. id's engine rejects both by
BSP-1.2.

| Lump | BSP29 | `2PSB` | `BSP2` |
| --- | --- | --- | --- |
| Nodes | `dnode_t`, 24 | `dl1node_t`, 32: `int` children, `short` bounds, `unsigned int` faces | `dl2node_t`, 44: `int` children, `float` bounds, `unsigned int` faces |
| Clipnodes | `dclipnode_t`, 8 | `dlclipnode_t`, 12: `int planenum`, `int children[2]` | as `2PSB` |
| Edges | `dedge_t`, 4 | `dledge_t`, 8: `unsigned int v[2]` | as `2PSB` |
| Faces | `dface_t`, 20 | `dlface_t`, 28: `int` `planenum`, `side`, `firstedge`, `numedges`, `texinfo`, then `byte styles[4]`, `int lightofs` | as `2PSB` |
| Leaves | `dleaf_t`, 28 | `dl1leaf_t`, 32: `short` bounds, `unsigned int` marksurfaces | `dl2leaf_t`, 44: `float` bounds, `unsigned int` marksurfaces |
| Marksurfaces | `unsigned short`, 2 | `unsigned int`, 4 | as `2PSB` |

### Behaviour

- **BSP-17.1** A `2PSB` file's version field is the bytes `2PSB` (0x32 0x50 0x53 0x42, the
  32-bit value 0x42535032) and a `BSP2` file's is the bytes `BSP2` (0x42 0x53 0x50 0x32, the
  value 0x32505342). The header and directory are otherwise those of BSP-1.1.
- **BSP-17.2** The entities, planes, textures, vertexes, visibility, texinfo, lighting,
  surfedges and models lumps are those of BSP29; the other six use the structures in the
  table above, with the same fields in the same order, widened.
- **BSP-17.3** In both variants a node child or clipnode child is a signed 32-bit value read
  as BSP-7.2 and BSP-11.1, and edge vertex numbers, marksurface entries and a leaf's
  marksurface range are unsigned 32-bit.
- **BSP-17.4** A `2PSB` node or leaf keeps 16-bit integer bounds; a `BSP2` node or leaf has
  float bounds, `mins[3]` then `maxs[3]`.
- **BSP-17.5** Every rule of BSP-1 to BSP-15 that does not concern a field's width applies
  to BSP2 unchanged.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BSP_2PSB_VERSION` | 0x42535032 (`2PSB`) | QS `Quake/bspfile.h`, `BSP2VERSION_2PSB`; ericw-tools `BSP2RMQVERSION` |
| `BSP_BSP2_VERSION` | 0x32505342 (`BSP2`) | QS `Quake/bspfile.h`, `BSP2VERSION_BSP2`; ericw-tools `BSP2VERSION` |
| `BSP_2PSB_NODE_SIZE` | 32 | QS `sizeof(dl1node_t)` |
| `BSP_BSP2_NODE_SIZE` | 44 | QS `sizeof(dl2node_t)` |
| `BSP_2PSB_LEAF_SIZE` | 32 | QS `sizeof(dl1leaf_t)` |
| `BSP_BSP2_LEAF_SIZE` | 44 | QS `sizeof(dl2leaf_t)` |
| `BSP_BSP2_CLIPNODE_SIZE` | 12 | QS `sizeof(dlclipnode_t)` |
| `BSP_BSP2_EDGE_SIZE` | 8 | QS `sizeof(dledge_t)` |
| `BSP_BSP2_FACE_SIZE` | 28 | QS `sizeof(dlface_t)` |
| `BSP_BSP2_MARKSURFACE_SIZE` | 4 | QS `Mod_LoadMarksurfaces`: `unsigned int` elements |

## BSP-18 .lit files

**Source:** QS `Quake/gl_model.c`, `Mod_LoadLighting`; ericw-tools
`include/common/litfile.hh`, `litheader_t`, `LIT_VERSION`; `common/litfile.cc`.

A `.lit` file gives a map coloured light without changing the BSP: it sits beside the BSP
with the same name (`maps/e1m1.lit` for `maps/e1m1.bsp`) and holds an RGB triple for every
byte of the lighting lump. The format comes from LordHavoc's engines (QS's comment reads
"LordHavoc: check for a .lit file").

| Offset | Size | Type | Field |
| --- | --- | --- | --- |
| 0 | 4 | `char[4]` | `ident`: `QLIT` |
| 4 | 4 | `int` | `version`: 1 |
| 8 | 3 * `filelen` | `byte[3]` | red, green and blue for each lighting byte |

### Behaviour

- **BSP-18.1** A `.lit` file is the 4 bytes `QLIT`, a signed 32-bit version 1, and then 3
  bytes, red, green and blue, for each byte of the BSP's lighting lump, in the same order,
  so `lightofs` (BSP-9.6) indexes it after multiplying by 3.
- **BSP-18.2** QS uses a `.lit` only when its size is exactly `8 + 3 * filelen` of the
  BSP's lighting lump; otherwise it prints `Outdated .lit file (<name> should be <n> bytes,
  not <m>)` and uses the BSP's own light.
- **BSP-18.3** A file that does not start with `QLIT` is ignored (`Corrupt .lit file (old
  version?), ignoring`), and one with another version is ignored (`Unknown .lit file version
  (<version>)`). ericw-tools also writes a version 0x00010001 holding one packed 32-bit
  `E5BGR9` HDR sample per lighting byte; QS does not read it, and this spec does not describe
  it further.
- **BSP-18.4** QS ignores a `.lit` found in a game directory of lower priority than the BSP
  (`ignored <name> from a gamedir with lower priority`), so a mod's BSP never takes the
  base game's `.lit`.
- **BSP-18.5** Without a usable `.lit`, each lighting byte stands for the grey triple of
  that value.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BSP_LIT_MAGIC` | `QLIT` | QS `Mod_LoadLighting`; ericw-tools `litheader_t::v1_t::ident` |
| `BSP_LIT_VERSION` | 1 | ericw-tools `LIT_VERSION` |
| `BSP_LIT_HEADER_SIZE` | 8 | QS `Mod_LoadLighting`: `data + 8` |

## BSP-19 .ent files

**Source:** QS `Quake/gl_model.c`, `Mod_LoadEntities`, `external_ents`; QS `Quake/common.c`,
`COM_StripExtension`; QS `Quake/crc.c`, `CRC_Block`.

A shipped map has no `.map` source, so its entities are edited in the BSP's entity lump or in
a `.ent` file beside it, which engines descended from QS read in place of the lump. id's
engine reads only the lump.

### Behaviour

- **BSP-19.1** When its `external_ents` cvar is set (its default is `"1"`), QS looks for the
  entity text in `<map>@<crc>.ent` and then in `<map>.ent`, where `<map>` is the BSP's name
  with its extension removed by `COM_StripExtension` (the last `.` after the last `/` or `\`,
  and not the name's first byte), and uses the first it finds instead of the entity lump.
  `<crc>` is `CRC_Block` (the CRC of PROGS-4) of the lump's first `filelen - 1` bytes, which
  leaves out the NUL `qbsp` writes (BSP-2.2), formatted with `%04x`; it is 0 for an empty
  lump.
- **BSP-19.2** QS ignores a `.ent` file found in a game directory of lower priority than the
  BSP's (`ignored <name> from a gamedir with lower priority`), as it does a `.lit`
  (BSP-18.4).
- **BSP-19.3** This project writes edited entities in either place. A BSP is rewritten with
  the new text and a NUL as its entity lump and every other lump's bytes unchanged, the
  lumps in the order of their offsets in the original file, each at an offset that is a
  multiple of 4; a `.ent` file is the text alone.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BSP_ENT_EXTENSION` | `.ent` | QS `Mod_LoadEntities` |
