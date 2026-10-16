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
