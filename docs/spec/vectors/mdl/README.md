# MDL test vectors

Vectors for [`../../mdl.md`](../../mdl.md). [`base.txt`](base.txt) is a hand-built alias
model stored as an `xxd` hex dump: `xxd -r base.txt > base.mdl` rebuilds it byte for byte
(284 bytes, SHA-256 `1e5dee493851af8ab4df9e3ed2c39e8544f59dec0f6096e8f9a26e865667db2f`). The
other vectors are the base with one change each, listed below, so a test builds them from
the base with a factory.

**Checked:** on 2026-10-16 every vector was loaded by id's `Mod_LoadAliasModel` and the
functions it calls, in `WinQuake/model.c` copied unchanged from
id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`, in the harness described in
[`../bsp/README.md`](../bsp/README.md) with `MAXALIASVERTS` 2000, `MAX_LBM_HEIGHT` 480 and
`ALIAS_BASE_SIZE_RATIO` `1.0 / 11.0` from `r_local.h` and `d_iface.h` and `r_pixbytes` 1. The
results below are its output. GLQuake's loader was read, not run.

## base

Statements: MDL-1.1, MDL-1.2, MDL-1.6, MDL-1.7, MDL-2.1, MDL-2.2, MDL-3.1, MDL-4.1, MDL-5.1,
MDL-5.2, MDL-5.4.

| Part | Contents |
| --- | --- |
| Header | `IDPO`, version 6, scale (1, 1, 1), scale origin (-8, -8, -8), bounding radius 10, eye position (0, 0, 22), 1 skin of 4 by 2, 3 vertices, 1 triangle, 2 frames, synctype 1, flags 8, size 11 |
| Skin 0 | type 0, pixels 1 to 8 |
| Texture coordinates | (onseam 0, s 0, t 0), (0x20, 1, 2), (0, 3, 1) |
| Triangle 0 | facesfront 1, vertices 0 1 2 |
| Frame 0 | type 0, bounds (0, 0, 0) to (16, 16, 16), name `stand1`, vertices (0, 0, 0) normal 0, (1, 2, 3) normal 1, (2, 4, 6) normal 2 |
| Frame 1 | type 1, a group of 2 with bounds (1, 1, 1) to (9, 9, 9) and intervals 0.1 and 0.2; frame `run1` with bounds (0, 0, 0) to (9, 9, 9) and every vertex (5, 0, 0) normal 7; frame `run2` with bounds (1, 1, 1) to (9, 9, 9) and every vertex (6, 0, 0) normal 7 |

id's loader gives: flags 8, synctype 1, 2 frames, bounds (-16, -16, -16) to (16, 16, 16),
size 1; texture coordinates kept as `s << 16` and `t << 16` (vertex 1: 65536 and 131072);
frame 1 is a group named `run2` with the group's bounds (1, 1, 1) to (9, 9, 9).

## Variants

| Name | Change | Result | Statements |
| --- | --- | --- | --- |
| `version-5` | `version` 5 | Error `<name> has wrong version number (5 should be 6)` | MDL-1.3 |
| `skin-480` | Skin 4 by 480 | Loads | MDL-1.4 |
| `skin-481` | Skin 4 by 481 | Error `model <name> has a skin taller than 480` | MDL-1.4 |
| `no-verts` | `numverts` 0, `numtris` 0 | Error `model <name> has no vertices` | MDL-1.4 |
| `verts-2000` | 2000 vertices | Loads | MDL-1.4 |
| `verts-2001` | 2001 vertices | Error `model <name> has too many vertices` | MDL-1.4 |
| `no-tris` | `numtris` 0 | Error `model <name> has no triangles` | MDL-1.4 |
| `skinwidth-6` | Skin width 6, `numskins` 0 | Error `Mod_LoadAliasModel: skinwidth not multiple of 4` | MDL-1.4 |
| `no-skins` | `numskins` 0 | Error `Mod_LoadAliasModel: Invalid # of skins: 0` | MDL-1.4 |
| `no-frames` | `numframes` 0 | Error `Mod_LoadAliasModel: Invalid # of frames: 0` | MDL-1.4 |
| `order` | Skin 6 by 481, `numverts` 0, `numtris` 0 | Error `model <name> has a skin taller than 480` | MDL-1.4 |
| `order-2` | Skin width 6, `numskins` 0, `numframes` 0 | Error `Mod_LoadAliasModel: skinwidth not multiple of 4` | MDL-1.4 |
| `skin-group` | Two skins: type 1, a group of 2 with intervals 0.5 and 1.0, pixels all 11 then all 12; type 9, a group of 1 with interval 0.25, pixels all 13 | Loads; both entries are groups | MDL-2.1, MDL-2.3 |
| `skin-group-zero` | Skin type 1, a group of 2 with intervals 0.5 and 0 | Error `Mod_LoadAliasSkinGroup: interval<=0` | MDL-2.3 |
| `skin-group-nan` | Skin type 1, a group of 1 with a NaN interval | Loads | MDL-2.3 |
| `frame-group-zero` | One frame entry of type 7: a group of 1 with interval 0 | Error `Mod_LoadAliasGroup: interval<=0` | MDL-5.1, MDL-5.5 |
| `frame-group-nan` | One frame entry of type 7: a group of 1 with a NaN interval | Loads; the entry is a group | MDL-5.1, MDL-5.5 |
