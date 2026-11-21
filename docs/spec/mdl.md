# MDL alias models

Prefix: `MDL` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the `.mdl` alias model (monsters, weapons, items and the player) and how the engine
loads it, as implemented by `WinQuake/modelgen.h` and `WinQuake/model.c` in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`, with the renderers' use of the fields from
`WinQuake/r_alias.c` and `WinQuake/gl_mesh.c`. GLQuake's loader in `WinQuake/gl_model.c`
differs in a few checks, which MDL-1.8 lists. Implemented by `crates/qs-mdl`
([#143](https://github.com/quakecstudio/editor/issues/143), P5-02), whose tests cite
every statement.

All integers are 32-bit two's-complement signed little-endian values read through
`LittleLong`, all floats 32-bit IEEE 754 little-endian values read through `LittleFloat`,
and a `trivertx_t` is four unsigned bytes.

## MDL-1 Header

**Source:** `WinQuake/modelgen.h`, `mdl_t`, `IDPOLYHEADER`, `ALIAS_VERSION`, `synctype_t`;
`WinQuake/model.c`, `Mod_LoadModel`, `Mod_LoadAliasModel`; `WinQuake/r_local.h`,
`MAXALIASVERTS`, `ALIAS_BASE_SIZE_RATIO`; `WinQuake/d_iface.h`, `MAX_LBM_HEIGHT`;
`WinQuake/model.h`, `EF_*` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

| Offset | Size | Type | Field | Meaning |
| --- | --- | --- | --- | --- |
| 0 | 4 | `int` | `ident` | `IDPO` |
| 4 | 4 | `int` | `version` | 6 |
| 8 | 12 | `float[3]` | `scale` | Size of one vertex step on each axis |
| 20 | 12 | `float[3]` | `scale_origin` | Position of vertex (0, 0, 0) |
| 32 | 4 | `float` | `boundingradius` | Not used by the engine |
| 36 | 12 | `float[3]` | `eyeposition` | Not used by the engine |
| 48 | 4 | `int` | `numskins` | Skin entries |
| 52 | 4 | `int` | `skinwidth` | Skin width in pixels |
| 56 | 4 | `int` | `skinheight` | Skin height in pixels |
| 60 | 4 | `int` | `numverts` | Vertices per frame |
| 64 | 4 | `int` | `numtris` | Triangles |
| 68 | 4 | `int` | `numframes` | Frame entries |
| 72 | 4 | `synctype_t` | `synctype` | `ST_SYNC` 0 or `ST_RAND` 1 |
| 76 | 4 | `int` | `flags` | `EF_` effect bits |
| 80 | 4 | `float` | `size` | Average triangle size, for the software renderer |

### Behaviour

- **MDL-1.1** An alias model starts with the 84-byte header above.
- **MDL-1.2** Its first four bytes are `IDPO` (0x49 0x44 0x50 0x4F). `Mod_LoadModel` sends a
  file that starts with them to the alias loader and any other to another loader (SPR-1.2,
  BSP-1.2); the alias reader rejects a file without them.
- **MDL-1.3** `version` must be 6; any other value fails with the error
  `<name> has wrong version number (<version> should be 6)` (fatal in id's engine).
- **MDL-1.4** Then, in this order, each failing with its error (fatal in id's engine):
  `skinheight` over 480 (`model <name> has a skin taller than 480`); `numverts` of 0 or less
  (`model <name> has no vertices`); `numverts` over 2000 (`model <name> has too many
  vertices`); `numtris` of 0 or less (`model <name> has no triangles`); `skinwidth` not a
  multiple of 4, tested as `skinwidth & 3` (`Mod_LoadAliasModel: skinwidth not multiple of
  4`); `numskins` under 1 (`Mod_LoadAliasModel: Invalid # of skins: <numskins>`). The skins,
  texture coordinates and triangles are then read, and only after them `numframes` under 1
  fails (`Mod_LoadAliasModel: Invalid # of frames: <numframes>`).
- **MDL-1.5** A negative `skinwidth` or `skinheight`, a skin size `skinwidth * skinheight`
  that does not fit in a signed 32-bit integer, or a file shorter than the header fails to
  load. id's code reads the header unchecked and uses these values as sizes, which is
  undefined; this spec fixes the answer as an error. (`-4 & 3` is 0, so the test of MDL-1.4
  alone passes a negative width.)
- **MDL-1.6** `scale`, `scale_origin`, `boundingradius`, `eyeposition`, `synctype`, `flags`
  and `size` are not checked. The loader keeps `size` multiplied by `1.0 / 11.0`.
- **MDL-1.7** Every alias model's bounds are (-16, -16, -16) to (16, 16, 16), whatever its
  frames hold (`Mod_LoadAliasModel`: "FIXME: do this right").
- **MDL-1.8** GLQuake's loader (`WinQuake/gl_model.c`, `Mod_LoadAliasModel`,
  `Mod_LoadAllSkins`) checks `numframes` before the skins, rejects more than 1024 vertices
  and more than 32 skins (`Mod_LoadAliasModel: Invalid # of skins: <numskins>`), and does not
  check `skinwidth` or the intervals of MDL-2.3 and MDL-5.5. It keeps triangles and frame
  poses in fixed arrays of 2048 and 256 without checking either count. A model is loadable by
  GLQuake-derived engines only within those four limits; a reader reports a model that
  exceeds one.
- **MDL-1.9** `flags` holds the `EF_` bits that make the client draw an effect behind the
  entity or spin it: 1 rocket trail, 2 grenade trail, 4 gib trail, 8 rotate, 16 green split
  trail, 32 small blood trail, 64 orange split trail and rotate, 128 purple trail.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MDL_MAGIC` | `IDPO` | `WinQuake/modelgen.h`, `IDPOLYHEADER`, "little-endian "IDPO"" |
| `MDL_VERSION` | 6 | `WinQuake/modelgen.h`, `ALIAS_VERSION` |
| `MDL_HEADER_SIZE` | 84 | `WinQuake/modelgen.h`, `sizeof(mdl_t)` |
| `MDL_MAX_SKIN_HEIGHT` | 480 | `WinQuake/d_iface.h`, `MAX_LBM_HEIGHT` |
| `MDL_MAX_VERTS` | 2000 | `WinQuake/r_local.h`, `MAXALIASVERTS` |
| `MDL_SIZE_RATIO` | `1.0 / 11.0` | `WinQuake/r_local.h`, `ALIAS_BASE_SIZE_RATIO` |
| `MDL_GL_MAX_VERTS` | 1024 | `WinQuake/gl_model.h`, `MAXALIASVERTS` |
| `MDL_GL_MAX_SKINS` | 32 | `WinQuake/gl_model.h`, `MAX_SKINS` |
| `MDL_GL_MAX_TRIS` | 2048 | `WinQuake/gl_model.h`, `MAXALIASTRIS` |
| `MDL_GL_MAX_POSES` | 256 | `WinQuake/gl_model.h`, `MAXALIASFRAMES` |
| `MDL_ST_SYNC`, `MDL_ST_RAND` | 0, 1 | `WinQuake/modelgen.h`, `ST_SYNC`, `ST_RAND` |
| `MDL_EF_ROCKET` | 1 | `WinQuake/model.h`, `EF_ROCKET` |
| `MDL_EF_GRENADE` | 2 | `WinQuake/model.h`, `EF_GRENADE` |
| `MDL_EF_GIB` | 4 | `WinQuake/model.h`, `EF_GIB` |
| `MDL_EF_ROTATE` | 8 | `WinQuake/model.h`, `EF_ROTATE` |
| `MDL_EF_TRACER` | 16 | `WinQuake/model.h`, `EF_TRACER` |
| `MDL_EF_ZOMGIB` | 32 | `WinQuake/model.h`, `EF_ZOMGIB` |
| `MDL_EF_TRACER2` | 64 | `WinQuake/model.h`, `EF_TRACER2` |
| `MDL_EF_TRACER3` | 128 | `WinQuake/model.h`, `EF_TRACER3` |

### Test vectors

See [`vectors/mdl/`](vectors/mdl/README.md): `base` loads (MDL-1.1, MDL-1.6, MDL-1.7);
`version-5`, `skin-480`, `skin-481`, `no-verts`, `verts-2000`, `verts-2001`, `no-tris`,
`skinwidth-6`, `no-skins`, `no-frames`, `order` and `order-2` (MDL-1.3, MDL-1.4).

## MDL-2 Skins

**Source:** `WinQuake/modelgen.h`, `daliasskintype_t`, `daliasskingroup_t`,
`daliasskininterval_t`, `aliasskintype_t`; `WinQuake/model.c`, `Mod_LoadAliasModel`,
`Mod_LoadAliasSkin`, `Mod_LoadAliasSkinGroup` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

A skin is a palette-indexed image wrapped onto the model. A group is a set of skins the
renderer cycles through by itself, each interval being the time at which that skin ends.

### Behaviour

- **MDL-2.1** After the header come `numskins` skin entries, each starting with a 32-bit
  type. A type of 0 (`ALIAS_SKIN_SINGLE`) is followed by one skin; any other value, not only
  1 (`ALIAS_SKIN_GROUP`), is followed by a skin group.
- **MDL-2.2** A skin is `skinwidth * skinheight` bytes of palette indices, row by row from
  the top left.
- **MDL-2.3** A skin group is a 32-bit count, then that many float intervals, then that many
  skins. Every interval must be greater than 0; one that is 0 or less fails with the error
  `Mod_LoadAliasSkinGroup: interval<=0` (fatal in id's engine). The test is `<= 0`, so a NaN
  interval loads.
- **MDL-2.4** A skin group's count must be at least 1. id's code allocates and reads with a
  count of 0 or less without checking it, which is undefined; this spec fixes the answer as
  an error.
- **MDL-2.5** A skin, interval or count that runs past the end of the file fails to load.
  id's code reads it unchecked, which is undefined; this spec fixes the answer as an error.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MDL_SKIN_SINGLE` | 0 | `WinQuake/modelgen.h`, `ALIAS_SKIN_SINGLE` |
| `MDL_SKIN_GROUP` | 1 | `WinQuake/modelgen.h`, `ALIAS_SKIN_GROUP` |

### Test vectors

See [`vectors/mdl/`](vectors/mdl/README.md): `skin-group`, `skin-group-zero` and
`skin-group-nan` (MDL-2.1 to MDL-2.3).

## MDL-3 Texture coordinates

**Source:** `WinQuake/modelgen.h`, `stvert_t`, `ALIAS_ONSEAM`; `WinQuake/model.c`,
`Mod_LoadAliasModel`; `WinQuake/gl_mesh.c`, `BuildTris`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

| Offset | Size | Type | Field |
| --- | --- | --- | --- |
| 0 | 4 | `int` | `onseam` |
| 4 | 4 | `int` | `s` |
| 8 | 4 | `int` | `t` |

### Behaviour

- **MDL-3.1** After the skins come `numverts` 12-byte `stvert_t` entries, one per vertex:
  `onseam`, then `s` and `t`, the vertex's position on the skin in pixels from the top left.
- **MDL-3.2** A skin holds the model's front on its left half and its back on its right
  half. A vertex with a non-zero `onseam` lies on the seam between them; when a triangle
  that does not face front (MDL-4.2) uses it, its `s` is moved right by `skinwidth / 2`.
  `modelgen.h` names the flag `ALIAS_ONSEAM` (0x20).
- **MDL-3.3** The software loader keeps `s` and `t` as 16.16 fixed point (`s << 16`), which
  is undefined in C for a value outside -32768 to 32767; this spec keeps them as the 32-bit
  values read.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MDL_STVERT_SIZE` | 12 | `WinQuake/modelgen.h`, `sizeof(stvert_t)` |
| `MDL_ALIAS_ONSEAM` | 0x20 | `WinQuake/modelgen.h`, `ALIAS_ONSEAM` |

### Test vectors

See [`vectors/mdl/`](vectors/mdl/README.md): `base` (MDL-3.1).

## MDL-4 Triangles

**Source:** `WinQuake/modelgen.h`, `dtriangle_t`, `DT_FACES_FRONT`; `WinQuake/model.c`,
`Mod_LoadAliasModel`; `WinQuake/gl_mesh.c`, `BuildTris`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

| Offset | Size | Type | Field |
| --- | --- | --- | --- |
| 0 | 4 | `int` | `facesfront` |
| 4 | 12 | `int[3]` | `vertindex` |

### Behaviour

- **MDL-4.1** After the texture coordinates come `numtris` 16-byte `dtriangle_t` entries:
  `facesfront`, then three vertex numbers, which index both the texture coordinates and
  each frame's vertices.
- **MDL-4.2** A `facesfront` of 0 marks a triangle on the back half of the skin; any other
  value faces front. `modelgen.h` defines `DT_FACES_FRONT` as 0x10.
- **MDL-4.3** A vertex number below 0 or not below `numverts` fails to load. id's code does
  not check them and the renderer indexes with them, which is undefined; this spec fixes the
  answer as an error.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MDL_TRIANGLE_SIZE` | 16 | `WinQuake/modelgen.h`, `sizeof(dtriangle_t)` |
| `MDL_DT_FACES_FRONT` | 0x10 | `WinQuake/modelgen.h`, `DT_FACES_FRONT` |

## MDL-5 Frames and frame groups

**Source:** `WinQuake/modelgen.h`, `daliasframetype_t`, `daliasframe_t`, `daliasgroup_t`,
`daliasinterval_t`, `trivertx_t`, `aliasframetype_t`; `WinQuake/model.c`,
`Mod_LoadAliasModel`, `Mod_LoadAliasFrame`, `Mod_LoadAliasGroup`; `WinQuake/r_alias.c`,
`R_AliasSetUpTransform`, `r_avertexnormals`, `NUMVERTEXNORMALS`; `WinQuake/gl_model.c`,
`Mod_LoadAliasGroup` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

| Structure | Offset | Size | Type | Field |
| --- | --- | --- | --- | --- |
| `daliasframetype_t` | 0 | 4 | `int` | `type`: `ALIAS_SINGLE` 0, `ALIAS_GROUP` 1 |
| `daliasframe_t` | 0 | 4 | `trivertx_t` | `bboxmin` |
| `daliasframe_t` | 4 | 4 | `trivertx_t` | `bboxmax` |
| `daliasframe_t` | 8 | 16 | `char[16]` | `name` |
| `daliasgroup_t` | 0 | 4 | `int` | `numframes` |
| `daliasgroup_t` | 4 | 4 | `trivertx_t` | `bboxmin` |
| `daliasgroup_t` | 8 | 4 | `trivertx_t` | `bboxmax` |
| `daliasinterval_t` | 0 | 4 | `float` | `interval` |
| `trivertx_t` | 0 | 3 | `byte[3]` | `v`: x, y, z |
| `trivertx_t` | 3 | 1 | `byte` | `lightnormalindex` |

### Behaviour

- **MDL-5.1** After the triangles come `numframes` frame entries, each starting with a 32-bit
  type. A type of 0 (`ALIAS_SINGLE`) is followed by one frame; any other value is followed by
  a frame group.
- **MDL-5.2** A frame is a 24-byte `daliasframe_t` (its bounding box as two `trivertx_t`,
  whose `lightnormalindex` is unused, and a 16-byte name) followed by `numverts`
  `trivertx_t`, one per vertex. The name is the bytes before the first NUL, or all 16; id's
  code copies it with `strcpy`, which reads past a name with no NUL, undefined behaviour this
  spec settles that way.
- **MDL-5.3** A vertex's position in the model is `v[i] * scale[i] + scale_origin[i]` on
  each axis, in single precision, and its normal is entry `lightnormalindex` of the 162
  precomputed normals in `r_avertexnormals`. An index of 162 or more reads past that table
  in id's renderer, which is undefined; this spec fixes the answer: the model loads and the
  index is reported.
- **MDL-5.4** A frame group is a 12-byte `daliasgroup_t` (a 32-bit count and the group's
  bounding box), then that many float intervals, then that many frames as in MDL-5.2. The
  entry takes the group's bounding box, and its name is the name of the group's last frame,
  because each frame of the group is copied into the same name buffer.
- **MDL-5.5** Every interval of a frame group must be greater than 0; one that is 0 or less
  fails with the error `Mod_LoadAliasGroup: interval<=0` (fatal in id's engine). The test is
  `<= 0.0`, so a NaN interval loads. GLQuake does not check the intervals and keeps only the
  first.
- **MDL-5.6** A frame group's count must be at least 1, as MDL-2.4, and a frame, interval or
  count that runs past the end of the file fails to load, as MDL-2.5.
- **MDL-5.7** Bytes after the last frame entry are ignored.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MDL_FRAME_SINGLE` | 0 | `WinQuake/modelgen.h`, `ALIAS_SINGLE` |
| `MDL_FRAME_GROUP` | 1 | `WinQuake/modelgen.h`, `ALIAS_GROUP` |
| `MDL_FRAME_HEADER_SIZE` | 24 | `WinQuake/modelgen.h`, `sizeof(daliasframe_t)` |
| `MDL_GROUP_HEADER_SIZE` | 12 | `WinQuake/modelgen.h`, `sizeof(daliasgroup_t)` |
| `MDL_NUMVERTEXNORMALS` | 162 | `WinQuake/r_alias.c`, `NUMVERTEXNORMALS` |

### Test vectors

See [`vectors/mdl/`](vectors/mdl/README.md): `base` (MDL-5.1, MDL-5.2, MDL-5.4),
`frame-group-zero` and `frame-group-nan` (MDL-5.1, MDL-5.5).

## MDL-6 Writing a model

**Source:** `qutils/MODELGEN/MODELGEN.C`, `WriteModelFile`, `WriteFrame`, `WriteGroupBBox`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`), the tool id's models
were written by. Implemented by `crates/qs-mdl`'s `mdl::write`
([#1081](https://github.com/quakecstudio/editor/issues/1081), L-47).

### Behaviour

- **MDL-6.1** `WriteModelFile` writes the header, then each skin entry (its type, then its
  pixels), then the texture coordinates, then the triangles, then each frame entry (its type,
  then a frame, or a group's `daliasgroup_t`, its intervals and its frames): the order and the
  layout the loader reads (MDL-1 to MDL-5). So a model read whole and written with nothing
  changed is the file it was read from, byte for byte, the bytes after its last frame entry
  (MDL-5.7) with it.
- **MDL-6.2** The header's `numskins`, `numverts`, `numtris` and `numframes` are the counts of
  what is written (`modeltemp.numskins = LittleLong (model.numskins)` and the like), so they
  follow the parts held when one is added or taken away; `numverts` is the count of texture
  coordinates. The header's other fields are written as held.
- **MDL-6.3** `WriteFrame` names a frame with `strcpy (aframe.name, frames[framenum].name)`
  into a `daliasframe_t` on its stack, which sets the name and its NUL and leaves the bytes
  after as they were. id's files hold such bytes: `progs/armor.mdl`'s frame is `armor`, a NUL,
  0xE5, 0x3F, then NULs, and 1746 frames of the models in `id1/pak0.pak` have a byte that is
  not NUL after their name's. The engine reads the name as a C string (MDL-5.2). A frame
  keeps its 16 bytes as read; a name that is set is at most 15 bytes with no NUL among them,
  and the rest of the 16 are NUL.
- **MDL-6.4** Not from modelgen, which writes whatever its script built: a model the loader
  would refuse is not written. A skin that is not `skinwidth * skinheight` bytes, a frame
  without one vertex for each texture coordinate, a group without one interval for each of
  its skins or frames, and a count past a 32-bit field are refused; so is whatever
  `Mod_LoadAliasModel` would fail on (MDL-1.4, MDL-1.5, MDL-2.3, MDL-2.4, MDL-4.3, MDL-5.5,
  MDL-5.6), with the loader's own error.

### Test vectors

The `base` vector of [`vectors/mdl/`](vectors/mdl/README.md), read and written, is itself
(MDL-6.1). **Checked:** on 2026-11-21 every `.mdl` of LibreQuake's `pak0.pak` and `pak1.pak`
(the test game) and of the original game's `id1/pak0.pak`, 160 models, was read and written
back byte for byte by `QS_MDL_PAKS=<paks> cargo nextest run -p qs-mdl a_games_models`; 9 of
LibreQuake's hold bytes after their last frame entry.

## MDL-7 Making a model from positions

**Source:** `qutils/MODELGEN/MODELGEN.C`, `ClearModel`, `GrabFrame`, `WriteModelFile`,
`WriteFrame`, `WriteGroupBBox`; `qutils/COMMON/MATHLIB.C`, `CrossProduct`, `VectorNormalize`
(id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`). modelgen's `vec_t` is
a `float` (`MATHLIB.H`). Implemented by `crates/qs-mdl`'s `mdl::make`
([#1081](https://github.com/quakecstudio/editor/issues/1081), L-47).

How modelgen makes the frames it is given, from triangle files, is MDL-8; a model made
from a mesh with texture coordinates of its own has those.

### Behaviour

- **MDL-7.1** The box the frames fill, `framesmins` to `framesmaxs`, starts as the origin
  alone (`ClearModel`: "`VectorCopy (vec3_origin, framesmins); VectorCopy (vec3_origin,
  framesmaxs);`"), and `GrabFrame` grows it to every vertex of every frame. So it holds the
  origin whatever the frames are.
- **MDL-7.2** `WriteModelFile` sets, on each axis, `scale` to `(framesmaxs[i] -
  framesmins[i]) / 255.9`, the difference of two floats divided in double by the double
  `255.9` and stored in a float, and `scale_origin` to `framesmins[i]`. `boundingradius` is the
  square root of the sum of the squares of, on each axis, `framesmins[i]` when `fabs
  (framesmins[i]) > fabs (framesmaxs[i])` and `framesmaxs[i]` otherwise. `eyeposition`,
  `flags` and `synctype` are as given; the skins' size is the header's.
- **MDL-7.3** `WriteFrame` packs a vertex with "`v = (pframe[j].v[k] - model.scale_origin[k])
  / model.scale[k]; tarray[j].v[k] = v;`": a float stored in a byte, its whole part. A
  frame's `bboxmin` and `bboxmax` are the least and the greatest packed byte on each axis,
  started from 255 and 0. Ours, where C's are undefined or unset: on an axis the box has no
  extent on, the scale is 0 and the division is not a number, and the byte is 0; a value
  outside 0 to 255 is the nearer end; and the `lightnormalindex` byte of the two bounds,
  which modelgen leaves as its stack had it, is 0.
- **MDL-7.4** `GrabFrame` takes a triangle's normal as `CrossProduct (v0 - v1, v2 - v1)` of
  its three corners, made a unit vector by `VectorNormalize`, which sums the squares and
  takes the root in a `double` and divides each part by it; a vector of no length is left.
  A vertex's normal is the sum of its triangles' normals divided by how many there are, made
  a unit vector, and its `lightnormalindex` is the entry of the normal table (MDL-5.3) with
  the greatest dot product, the first of equals ("`if (dot > maxdot)`"). A vertex that is a
  corner of no triangle stops modelgen ("`Vertex with no non-degenerate triangles
  attached`") and is refused. So a triangle's corners go around it clockwise as seen from
  outside the model.
- **MDL-7.5** `size` is `totsize / model.numtris`: `GrabFrame` adds to `totsize`, for each
  triangle of the first frame, half the length of that cross product before it is made a
  unit vector, the triangle's area.
- **MDL-7.6** A group's intervals are the times its frames end at: `WriteModelFile` writes
  "`totinterval += frames[groupframe+1+j].interval; temp.interval = LittleFloat
  (totinterval);`" for each. Its bounds (`WriteGroupBBox`) are the least and the greatest
  packed byte over all its frames.
- **MDL-7.7** Ours: a frame without a position for each texture coordinate, a name MDL-6.3
  does not take and a model MDL-6.4 refuses are not made.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MDL_SCALE_STEPS` | 255.9 | `qutils/MODELGEN/MODELGEN.C`, `WriteModelFile` |

### Test vectors

**Checked:** on 2026-11-21, for the first frame of every model of the original game's
`id1/pak0.pak`, each vertex's normal was computed as MDL-7.4 says from the positions the
file stores and compared with the entry the file stores: of 5824 vertices, 5819 are within
45 degrees of it and none is opposite. The positions are those the file packed, so the
match is not exact; it settles which way a triangle's corners go.

## MDL-8 Making a model from triangles

**Source:** `qutils/MODELGEN/MODELGEN.C`, `Cmd_Base`, `SetSkinValues`, `Cmd_Skin`,
`Cmd_SkinGroupStart`, `GrabFrame`, `Cmd_Origin`, `Cmd_Eyeposition`, `Cmd_ScaleUp`,
`WriteModelFile`; `qutils/COMMON/MATHLIB.C`, `VectorCompare`, `Q_rint`; `MATHLIB.H`,
`EQUAL_EPSILON` (id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`).
Implemented by `crates/qs-mdl`'s `mdl::grab`
([#1081](https://github.com/quakecstudio/editor/issues/1081), L-47).

modelgen makes a model from a script naming triangle files: `$base` one, each `$frame`
another with the same triangles in the same order, their corners moved. What it reads those
files with (`TRILIB.C`, Alias's `.tri`; `LBMLIB.C`, a 320 by 200 `.lbm` picture) is not
here: a triangle is its three corners and a picture its pixels. MDL-7 says how the frames
grabbed become a file.

### Behaviour

- **MDL-8.1** `Cmd_Base` makes the model's vertices from the base's triangles in order:
  each corner is the first vertex `VectorCompare` finds equal to it, "`if
  (fabs(v1[i]-v2[i]) > EQUAL_EPSILON) return false;`" on each axis, else a new vertex at the
  corner. A triangle two of whose own corners compare equal is degenerate; its corners are
  vertices all the same.
- **MDL-8.2** `WriteModelFile` writes the triangles that are not degenerate, in the base's
  order, each with its three vertices and `facesfront` as `SetSkinValues` left it, 1 or 0,
  and the header's `numtris` is their count ("`model.numtris - degeneratetris`").
- **MDL-8.3** `SetSkinValues` takes the box of the base's vertices, its low side `floor`ed
  and its high side `ceil`ed, and from it `width = maxs[0] - mins[0]` and `height = maxs[2]
  - mins[2]` as `int`s. "`scale = 8; if (width*scale >= 150) scale = 150.0 / width; if
  (height*scale >= 190) scale = 190.0 / height;`", then "`iwidth = ceil(width*scale) + 4;
  iheight = ceil(height*scale) + 4;`". The skin is `(iwidth*2 + 3) & ~3` wide, "a multiple
  of 4", and `iheight` high.
- **MDL-8.4** A triangle whose normal, `CrossProduct (v0 - v1, v2 - v1)` of its vertices in
  the base, has "`normal[1] > 0`" is on the back, `facesfront` 0, laid from `basex = iwidth
  + 2`; any other is on the front, `facesfront` 1, laid from `basex = 2`; `basey` is 2.
  Each of its vertices gains a seam bit, 1 for a front triangle and 2 for a back one, and
  takes "`s = Q_rint((pbasevert[0] - mins[0]) * scale + basex); t = Q_rint((maxs[2] -
  pbasevert[2]) * scale + basey);`" when the triangle is on the front or the vertex has no
  front bit yet ("we want the front s value for seam vertices"). `Q_rint` is "`floor (in +
  0.5)`". A vertex with both bits is written with `onseam` `ALIAS_ONSEAM`, any other with 0.
  Degenerate triangles are laid as the rest are.
- **MDL-8.5** `Cmd_Skin` takes a skin from the top left of its picture: `skinheight` rows of
  `skinwidth` pixels. A skin group is not made: `Cmd_SkinGroupStart` gives the group's entry
  a type and a count but no data, and `WriteModelFile` writes every entry as "`SafeWrite
  (modelouthandle, skins[curskin].pdata, model.skinwidth * model.skinheight);`", so what
  modelgen writes for a group is no skin group (MDL-2.1). Ours: a picture smaller than the
  skin, where modelgen reads past it, is refused.
- **MDL-8.6** `GrabFrame` puts each vertex where the corners of the frame's triangles that
  are not degenerate in the base put it, the last one read: `v[0] = -y * scale_up +
  adjust[0]`, `v[1] = x * scale_up + adjust[1]`, `v[2] = z * scale_up + adjust[2]` of a
  corner `(x, y, z)` ("rotate the vertices so the model faces down the positive x axis").
  `scale_up` is `$scale`, 1 when not given. `Cmd_Origin` reads `$origin a b c` as
  "`adjust[1] = -atof (token); ... adjust[0] = atof (token); ... adjust[2] = -atof
  (token);`", and `Cmd_Eyeposition` reads `$eyeposition a b c` as `eyeposition[1] = a`,
  `eyeposition[0] = -b`, `eyeposition[2] = c`; the header's is `model.eyeposition[i] +
  adjust[i]`.
- **MDL-8.7** A vertex's normals are those of the frame's triangles it is a corner of, each
  the triangle's own corners' `CrossProduct (v0 - v1, v2 - v1)` as the file has them, not
  scaled, made a unit vector and then turned as the vertices are ("`normal[0] = -normal[1];
  normal[1] = ftemp;`"), averaged and matched as MDL-7.4 says. modelgen keeps 20 for a
  vertex (`float normals[20][3]`) and writes past them for more: ours, a vertex that is a
  corner of more than 20 triangles is refused, as is one all of whose triangles are
  degenerate, which modelgen stops at (MDL-7.4).
- **MDL-8.8** The header's `size` is `totsize / model.numtris`, where `numtris` counts the
  degenerate triangles: for the first frame the script grabs, each triangle that is not
  degenerate adds half the length of the cross product of its two edges each multiplied by
  `scale_up`. `totsize` is a `float` ("`float totsize, averagesize;`"): the square root is
  taken and added in double, and each sum is stored back in single. A frame of a group is
  shown for the interval the script gives after its name, else "`frames[framecount].interval
  = 0.1;`"; an interval that is not above 0 stops modelgen ("`if
  (frames[framecount].interval <= 0.0) Error ("Non-positive interval %s %f", ...`"), as does
  a group with no frame ("`Error ("Empty group\n");`") and a frame whose file has another
  number of triangles than the base's ("number of triangles doesn't match"), and each is
  refused.
- **MDL-8.9** Ours: a base with no triangle, whose box modelgen leaves at `9999999`, is
  refused, and so is a base of more vertices than a model has (MDL-1.4), where modelgen
  writes past its `MAXVERTS`, as soon as it has that many.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MDL_GRAB_EQUAL_EPSILON` | 0.001 | `qutils/COMMON/MATHLIB.H`, `EQUAL_EPSILON` |
| `MDL_GRAB_SKIN_SCALE` | 8 | `qutils/MODELGEN/MODELGEN.C`, `SetSkinValues` |
| `MDL_GRAB_SKIN_WIDTH` | 150 | `SetSkinValues` |
| `MDL_GRAB_SKIN_HEIGHT` | 190 | `SetSkinValues` |
| `MDL_GRAB_SKIN_MARGIN` | 2 | `SetSkinValues`, `basex`, `basey` and half of the `+ 4` |
| `MDL_GRAB_MAX_NORMALS` | 20 | `qutils/MODELGEN/MODELGEN.C`, `vertexnormals` |
| `MDL_GRAB_INTERVAL` | 0.1 | `GrabFrame` |

### Test vectors

A pyramid: a foot with corners `a (-8, -8, 0)`, `b (8, -8, 0)`, `c (8, 8, 0)`, `e (-8, 8,
0)` and a tip `t (0, 0, 16)`, as the triangles `abt`, `bct`, `cet`, `eat`, `acb`, `aec`,
with a second frame whose tip is `(2, 1, 20.5)`. id's modelgen, built from the pinned commit
and run on it on 2026-11-21, wrote a skin of 264 by 132; vertices `a, b, t, c, e` at `(s,
t)` (2, 130), (130, 130), (66, 2), (130, 130), (2, 130), all but the tip on the seam; the
triangles `abt` on the back and the other five on the front; the first frame's vertices
packed as (255, 0, 0), (255, 255, 0), (127, 127, 199), (0, 255, 0), (0, 0, 0) with normals
17, 118, 84, 139, 42, and the second frame's tip as (111, 159, 255); and a `size` of
138.07224.

**Checked:** `cargo xtask oracles models` builds id's modelgen unchanged from the pinned
commit and gives it and `Grab::build` the same scripts, each made from its number (2 to 19
triangles over a pool of 4 to 15 points, one to three frame entries of which some are groups,
with and without `$origin`, `$scale`, `$eyeposition`, `$flags` and `$sync`, and now and then
a triangle of no area). On 2026-11-21, of 1000 scripts modelgen wrote a model for 769 and
ours was the same file for each, but for the bytes modelgen leaves as its stack had them (a
frame's name after its NUL, and the `lightnormalindex` of a frame's and a group's bounds);
the other 231 both refused. With `Q_rint`'s half made a quarter in ours, every script
modelgen took differed. The scripts have since gained groups with no frame and frames shown
for no time or less, which both refuse.

## MDL-9 Reading a mesh

**Sources:** the `.obj` format's description, "Object Files (.obj)", appendix B1 of
Wavefront's Advanced Visualizer manual, as transcribed at
<http://www.martinreddy.net/gfx/3d/OBJ.spec> (fetched 2026-11-21, SHA-256
`60e398c4ce8fa91dcba2cf414914da03a030e1379a82620e9d3a7b0ea813263f`); the `.iqe` format's
description, [`specs/iqe/iqe.txt`](../../specs/iqe/iqe.txt), and the exporter the format's
own kit ships, `blender-5.0/iqm_export.py` of
lsalzman/iqm@`1077b9c195a7f76f9b26266562f6e36bb4d5dac9` (fetched 2026-11-21), for what the
description leaves unsaid. Nothing here is ported: the readers are written from the
descriptions. Implemented by `crates/qs-mdl`'s `mesh`, `mesh::obj` and `mesh::iqe`
([#1081](https://github.com/quakecstudio/editor/issues/1081), L-47).

A reader gives a mesh: triangles whose corners each name a position and, where the file has
one, a texture coordinate; and one or more poses, each a place for every position. In a
mesh a triangle's corners go clockwise as seen from outside (MDL-7.4), a texture coordinate
is `u` across the skin from its left and `v` down it from its top, and the axes are Quake's:
x forward, y left, z up.

### Behaviour

- **MDL-9.1** An `.obj` file is read for its polygonal geometry: "`v x y z w`", "Specifies a
  geometric vertex and its x y z coordinates"; "`vt u v w`", where "u is the value for the
  horizontal direction of the texture" and "v is the value for the vertical direction of the
  texture. The default is 0."; and "`f v1/vt1/vn1 v2/vt2/vn2 v3/vt3/vn3 . . .`", "A minimum
  of three vertices are required." Vertices of each kind are "numbered separately, starting
  with 1", and "A reference number of -1 indicates the vertex immediately above the
  element." A face of more than three corners is a fan from its first. "If vertices are
  ordered counterclockwise around the face, both the face and the normal will point toward
  the viewer", so each triangle's corners are turned around. The description does not say
  which end of the picture `v` is measured from, nor which axis is up. Ours: `v` is up from
  the picture's foot, so `1 - v` down from its top; and y is up and -z forward unless the
  caller says z is up, a position `(x, y, z)` being Quake's `(-z, -x, y)`. A `#` starts a
  comment; every other statement is passed over. A vertex without three numbers, a face of
  fewer than three corners and a reference to a vertex the file does not have above the face
  are refused, with the line. The mesh has one pose, `base`.
- **MDL-9.2** An `.iqe` file "must" start with "`# Inter-Quake Export`", and "Any line
  beginning with # is a comment". `vp X Y Z W` is a vertex's position, `vt U V` its texture
  coordinate ("If U or V are unspecified, they default to 0") and `vb Ai Aw Bi Bw ...` the
  joints it is blended to and their weights. "All vertex attributes are read into separate
  vertex arrays", which "may be interleaved in any order, or not interleaved at all", so the
  Nth of each is the Nth vertex's; and "Each vertex array must either be absent, or the same
  length as all other vertex arrays", so the mesh has texture coordinates, and its vertices
  their blends, when the file has one for every vertex. `fa A B C...` names vertices "starting from the first vertex in the
  file at index 0", `fm` "from the first vertex defined after the last "mesh" command", and
  for both "Negative indexes are relative to the last defined vertex which is at -1" and
  more than three are "a convex, planar polygon", read as a fan. A face may come before the
  vertices it names: its indexes are looked up once the file is read, one below 0 counting
  back from the vertices above the face. "If there is no triangle
  data, then all vertex data between the preceding "mesh" command and the next is
  interpreted as mesh local triangle data, where each group of 3 vertexes defines a
  triangle." `joint NAME PARENT` adds a joint, with no parent "If PARENT is less than 0 or
  unspecified"; poses before any `animation` are the base poses, "the Nth pose in the list
  corresponds to the Nth specified joint", and after one they are its current frame's, a
  `frame` starting the next. A pose is applied as "output = (input*scale)*rotation +
  translation": `pq Tx Ty Tz Qx Qy Qz Qw Sx Sy Sz`, where "If Qw is unspecified, it is
  generated such that it is a negative value", and `pm`, a translation, the three rows of a
  rotation matrix and a scale. Everything after a `comment` line is left. The description
  does not say which way a triangle's corners go or which end `v` is measured from; the
  kit's exporter writes "`# flip V axis of texture space`" as `1.0 - uv[1]` and "`# Quake
  winding is reversed`", so ours: corners are clockwise and `v` is down from the top, as a
  mesh's are, and z is up unless the caller says y is. Ours as well: a name in double quotes
  is one word, as the exporter writes one; a quaternion is made a unit one; a `pa` pose,
  whose three rotations the description gives no order for, is refused, as are an argument
  that is no number, a face that is not three vertices of the file, a joint whose parent is
  not above it and a base pose that has no size on an axis. An alias model has no skeleton,
  so each frame of each animation is a pose of the mesh: a joint's place is its pose in its
  parent's, a joint without a pose in a frame keeps its base pose, and a vertex is the sum,
  by its weights over their total, of where each joint it is blended to takes it from where
  the base poses had it; a vertex with no blend stays. A frame is named by its animation's
  letters, digits and underscores and its number from 1 (`run1`, `run2`), the name cut so
  that both fit a frame's 15 bytes, `anim` for one with no name. An animation with no name
  has "an unused dummy name": one whose frames would be named as an earlier animation's
  are has a letter after its name, `b` to `h`, and a file with more such than the letters
  tell apart is refused. A file with no animation has one pose, `base`.
- **MDL-9.3** Ours: a mesh every corner of which has a texture coordinate is a model whose
  vertices are the pairs of a position and a texture coordinate its corners have, in the
  order they come. A vertex is at the skin's pixel `floor(u * skinwidth)`, `floor(v *
  skinheight)`, kept on the skin; no vertex is on the seam and every triangle has
  `facesfront` `DT_FACES_FRONT`. The skin's width is a multiple of 4 from 4 to 1024 and its
  height from 1 to 480 (MDL-1.4, which does not bound the width: 1024 is ours), checked
  before a skin of that size is made; a mesh of more vertices than a model has (MDL-1.4)
  is refused as soon as it has that many. The skin is the caller's picture, or one colour of the size
  the caller gives, its first pixel colour 0: GLQuake fills what is joined to a skin's first
  pixel and of its colour with its `filledcolor`, 0 (REND-4.1), which is all of a skin of one
  colour, and fills nothing when the first pixel is that colour already. A triangle two of whose corners are one position is left out; a mesh
  with no triangle left or no pose, and a corner past the mesh's positions or texture
  coordinates, named by its triangle's place in the mesh, are refused. Each pose is a frame made as MDL-7 says.
- **MDL-9.4** Ours: a mesh without a texture coordinate for every corner is laid out as
  modelgen lays a base (MDL-8.1, MDL-8.3, MDL-8.4) from its first pose, turned so that the
  layout is of the mesh as it stands: what faces -x is on the back, the skin's right half,
  and the rest on the front, seen from +x with +y to the right. Its skin is one colour, its
  first pixel colour 0 as MDL-9.3 says, of the size the layout gives; a picture is refused, since where the layout puts a triangle
  is not known before it is made.

### Test vectors

The arm of `crates/qs-mdl/src/mesh/iqe/tests.rs`: a joint at the origin and a joint one unit
along x under it, with a vertex at `(2, 0, 0)` blended to the second. In a frame whose
second joint is `pq 1 0 0 0 0 0.70710678 0.70710678`, a quarter turn about z, the vertex is
at `(1, 1, 0)`; one blended half to each joint is at `(1.5, 0.5, 0)`.

## MDL-10 Reading a mesh as FTEQW's converter does

**Source:** `iqm/iqm.cpp` and `iqm/util.h` of
fte-team/fteqw@`f937b9d88f71fc4429db5fe56c6a98d922711b2e`, FTE's fork of Lee Salzman's IQM
exporter ([ADR-0030](../adr/0030-fteqw-iqm-porting-upstream.md)): `blendcombo::addweight`,
`blendcombo::finalize`, `buildmd5verts`, `makerelativebasepose`, `loadmd5mesh`,
`loadmd5anim`, the `smd` namespace and `loadsmd`, the `fbx` namespace and `loadfbx`;
`Quat::fromangles`, `Quat::fromdegrees`. Implemented by `crates/qs-mdl`'s `mesh::rig`,
`mesh::md5`, `mesh::smd` and `mesh::fbx`
([#1081](https://github.com/quakecstudio/editor/issues/1081), L-47). The converter's own
output is an `.iqm` file; what is ported is how it reads each format into vertices, joints
and frames, which become a mesh of poses as MDL-9.2 says an `.iqe`'s do.

### Behaviour

- **MDL-10.1** A vertex's blends are kept as `blendcombo` keeps them: "`if(weight <= 1e-3)
  return;`" leaves a weight of a thousandth or less out, the four heaviest are kept,
  heaviest first, one given earlier before another as heavy, and `finalize` divides them by
  their sum. A pose is its scale, then its rotation, then its move, in its joint's parent's
  space, and a mesh of poses is made of the joints, the blends and the frames as MDL-9.2
  says. Ours: a joint may come before its parent in a file and follows it all the same,
  and one whose parents come round to itself, or whose parent is no joint, stands as a root.
- **MDL-10.2** An `.md5mesh` and its `.md5anim` files, read as MD5-1 to MD5-3 say, are a
  mesh as `buildmd5verts` and `loadmd5anim` make one. A vertex is "`pos +=
  (j.orient.transform(w.pos) + j.pos)*w.bias;`" over its weights, by the mesh's joints,
  which the file gives in the model's space; its texture coordinate is the file's; it is
  blended to its weights' joints by their biases (MDL-10.1). A triangle's corners are kept
  in the file's order. `makerelativebasepose` makes each joint's pose its parent's:
  "`child.pos = (-parent.orient).transform(child.pos - parent.pos); child.orient =
  (-parent.orient)*child.orient;`". A frame's pose of a joint is the animation's base frame
  with the components the joint's flags name put in from the frame, in turn from the
  joint's first ("`if(h.flags&1) j.pos.x = *jdata++;`" and so on, then
  "`j.orient.restorew();`"), in its parent's space. Ours: an animation of another number
  of joints than the mesh, which the converter reads past, is refused, and so is a file
  that says it has more than 65536 vertices, triangles, weights or frames, before anything
  is made of that many.
- **MDL-10.3** An `.smd` file is read line by line; a line that is empty or starts with
  `#`, `;` or `//` is passed over, and a file whose `version` is not 1 is refused. `nodes`
  has a line for each joint, its number, its name and its parent's number, to `end`: a
  number over 255, a line with no name and a second line for one number are left out.
  `skeleton` has `time N` lines, each starting frame `N`, and lines of a joint's number, a
  position and three angles, the joint's pose in its parent's space, turned about x, then
  y, then z (`Quat::fromangles`, in radians); a frame the file skips, and a joint a frame
  gives no pose, have the first frame's. `triangles` has, for each triangle, a line naming
  its material and a line for each of three vertices: its joint, its position, its normal,
  its texture coordinate and, after a count, the joints it is linked to each with a
  weight; the section ends at a vertex's line of fewer than nine numbers. "`tc->y = 1 -
  tc->y;`": `v` is measured up the picture. A vertex's links to its own joint are summed,
  with what the links' weights lack of 1 ("`if(tweight < 1) pweight += 1 - tweight;`"), and
  it is blended to its own joint by that and to the others by theirs (MDL-10.1). The three
  vertices are kept last first, which turns the triangle around. `vertexanimation` is
  passed over. A file with triangles is the model, its base pose the first frame of its
  own skeleton and its other frames no poses of the mesh ("`eframes.setsize(firstframe);`");
  a file without is an animation of the same joints. Ours: a joint with no pose in a
  file's first frame, which the converter leaves unset, keeps the model's base pose; a
  `time` past 65535 starts no frame; a file with no triangles is no model.
- **MDL-10.4** A text `.fbx` file is read when its first line holds "`FBX 7`". Its tokens:
  spaces and commas are passed over, a `;` starts a comment to the end of its line, `{`
  and `}` open and close a block, text in double quotes is one token, a number is one,
  `*` and a count come before an array, any other word is a property's name, with the
  colon after it, and the end of a line is a token. A number is read as `strtod` reads one, and 0 with
  one byte taken where there is none. Of the file's top properties `Objects`
  and `Connections` are read. An object is `Geometry` (its `Vertices`, its
  `PolygonVertexIndex`, whose last corner of a polygon is written `-(vertex + 1)`, and of
  `LayerElementUV` its `UV` and `UVIndex`); `Model` of the kind `Mesh` or `LimbNode`, with,
  from the `P` lines of its `Properties70`, `Lcl Translation`, `Lcl Rotation`, `Lcl
  Scaling`, `PreRotation` and, for a mesh, `GeometricTranslation`; `Deformer` of the kind
  `Skin` or `Cluster`, a cluster with its `Indexes` and `Weights`; `AnimationCurve` with
  its `KeyValueFloat`; `AnimationCurveNode` with `d|X`, `d|Y` and `d|Z`;
  `AnimationLayer`; and `AnimationStack`, named after `AnimStack::`. A `C` line joins two
  objects (`"OO"`): a geometry to its mesh model, a limb to its parent limb, a skin to its
  geometry, a cluster to its skin, a limb to its cluster, a curve node to its layer and a
  layer to its stack; or an object to a property (`"OP"`): a curve to `d|X`, `d|Y` or
  `d|Z` of a curve node, and a curve node to `Lcl Translation`, `Lcl Rotation` or `Lcl
  Scaling` of a limb. The limbs are the joints, in the file's order, each posed by its
  scaling, its rotation after its pre-rotation ("`eposes[i].orient = prequat *
  eposes[i].orient;`"), and its translation; angles are degrees, about x, then y, then z
  (`Quat::fromdegrees`). The mesh has a vertex for each corner of each polygon, at its
  geometry's vertex moved by the mesh model's geometric translation, scaled, turned by its
  rotation and then its pre-rotation, and moved by its translation; with a texture
  coordinate, `v` measured up the picture ("`1-uvs[idx+1]`"), by the corner's `UVIndex`,
  else by its vertex when the file has a coordinate for each vertex, else by its place. A
  cluster whose limb's number is at most 255 blends the vertices its `Indexes` name to its
  limb by its `Weights` (MDL-10.1). A triangle of corners `a b c` is kept as `b a c`
  ("`etriangle(i+1, i, i+2)`"). An animation stack is an animation of the frames of its
  first layer's curves, which are of one length, else it is none: a frame's pose of a limb
  is the limb's own with, for each curve node joined to it, the property the node is joined
  to given the node's three values, each a curve's value at the frame where the curve
  changes and the curve's one value or the node's own where it does not ("`if(c->varies())
  curves[i] = c; else if(c->vals.length()) val[i] = c->vals[0];`"). What is read is turned
  a quarter about x, "`erotate *= Quat(M_PI/2, Vec3(1, 0, 0));`": a position `(x, y, z)`
  is `(x, -z, y)`, which stands a file whose y is up on z.
- **MDL-10.5** Where ours differs from the converter. A polygon of more than three corners
  is a fan from its first corner, each triangle kept as MDL-10.4 says, where the converter
  takes each three corners in turn for a triangle whatever the polygons are (Justin,
  2026-11-21: programs write quads unless told not to, and the converter's reading of one
  is not its faces). The binary form of `.fbx`, which the converter does not read either,
  and a file of another version are refused, where the converter reads nothing and says
  nothing. A geometry's normals and colours, materials, and a cluster's transforms are not
  read: a model has none of them. A file whose mesh model is turned and whose geometry
  has no normals, at which the converter fails, is read. A name is kept whole, where the
  converter keeps 63 bytes of it. A caller that says the file's z is up has it left
  unturned. The converter's turn keeps a file's x, where an `.obj`'s y-up turn (MDL-9.1)
  takes its -z for forward: the same model faces a quarter apart from the two.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `BLEND_MIN` | 0.001 | `iqm/iqm.cpp`, `blendcombo::addweight` |
| `BLEND_MAX` | 4 | `blendcombo`, `weights[4]` |
| `SMD_VERSION` | 1 | `loadsmd` |
| `SMD_MAX_JOINT` | 255 | `smd::readnodes` |
| `FBX_VERSION` | `FBX 7` | `fbx::checkversion` |
| `FBX_MAX_LIMB` | 255 | `fbx::clusternode::process` |
| `MD5_MAX_COUNT` | 65536 | Ours |
| `SMD_MAX_TIME` | 65535 | Ours |

### Test vectors

`crates/qs-mdl/src/mesh/fixtures/` holds one model in each format, each file named with
`.txt` after its own ending, written by `scripts/model-formats/samples.py`: two joints, a root at the origin and an arm 8 along x
under it; five vertices, three of the root's, one of the arm's at `(16, 0, 0)` and one of
both in halves at `(8, 2, 0)`; five triangles; and an animation of three frames in which
the arm turns 0, 45 and 90 degrees about z and the root rises 0, 1 and 2. The `hard-` files
are the same model with, in md5, the arm's joint turned a quarter in the base pose; in smd,
the base pose turned, the animation's second time left out and the arm given no pose in its
last frame; and in fbx, a pre-rotation of the arm and a mesh model moved, turned and scaled.

**Checked:** on 2026-11-21 the converter was built from `iqm/iqm.cpp` at the pinned commit
on its own (`c++ -O1 -std=gnu++11 iqm.cpp`) and run on each of the six
(`iqmtool out.iqm arm.md5mesh wave.md5anim`, and so for smd and fbx).
`scripts/model-formats/iqm.py` posed each `.iqm` it wrote, frame by frame, into the
`*.fteqw.txt` files beside the fixtures: each triangle's three corners, each a position and
a texture coordinate. The readers' tests hold each mesh to its file: as many frames, each
of as many triangles, each triangle one of the converter's with its corners in the same
order, within 0.05, the converter's weights being bytes. The greatest difference was 0.004
for five of the six and 0.041 for the scaled fbx. In the last frame the arm's vertex is at
`(8, 8, 2)` and the vertex of both at `(7, 1, 2)`.
