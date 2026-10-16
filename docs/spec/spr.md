# SPR sprites

Prefix: `SPR` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the `.spr` sprite file and how the engine loads it, as implemented by
`WinQuake/spritegn.h` and `WinQuake/model.c` in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`. Sprites are the flat, camera-facing models of
explosions, bubbles and lights (`progs/s_explod.spr`, `progs/s_bubble.spr`,
`progs/flame.spr` and others). Implemented by `crates/qs-mdl`
([#235](https://github.com/quakecstudio/editor/issues/235)).

All integers in a sprite are 32-bit two's-complement signed little-endian values read
through `LittleLong`, and all floats are 32-bit IEEE 754 little-endian values read through
`LittleFloat`.

## SPR-1 Header

**Source:** `WinQuake/spritegn.h`, `dsprite_t`, `IDSPRITEHEADER`, `SPRITE_VERSION`;
`WinQuake/model.c`, `Mod_LoadModel` and `Mod_LoadSpriteModel`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

`Mod_LoadModel` loads the whole file and picks the loader from its first four bytes:
`IDPO` is an alias model, `IDSP` a sprite and anything else a brush model.
`Mod_LoadSpriteModel` then reads the header in place.

| Offset | Size | Type | Field | Meaning |
| --- | --- | --- | --- | --- |
| 0 | 4 | `int` | `ident` | `IDSP` |
| 4 | 4 | `int` | `version` | 1 |
| 8 | 4 | `int` | `type` | Orientation, the `SPR_` type constants |
| 12 | 4 | `float` | `boundingradius` | Not read by the engine |
| 16 | 4 | `int` | `width` | Largest frame width (`maxwidth`) |
| 20 | 4 | `int` | `height` | Largest frame height (`maxheight`) |
| 24 | 4 | `int` | `numframes` | Number of frame entries |
| 28 | 4 | `float` | `beamlength` | Not used by the renderer |
| 32 | 4 | `synctype_t` | `synctype` | `ST_SYNC` 0 or `ST_RAND` 1 |

### Behaviour

- **SPR-1.1** A sprite starts with a 36-byte header: `ident`, `version`, `type` as signed
  32-bit integers, `boundingradius` as a float, `width`, `height` and `numframes` as signed
  32-bit integers, `beamlength` as a float and `synctype` as a signed 32-bit integer.
- **SPR-1.2** A sprite's first four bytes are `IDSP` (0x49 0x44 0x53 0x50). A file that does
  not start with them is not a sprite: the engine loads it as another kind of model, and the
  sprite reader rejects it.
- **SPR-1.3** `version` must be 1; any other value fails with the error
  `<name> has wrong version number (<version> should be 1)` (fatal in id's engine).
- **SPR-1.4** `numframes` must be at least 1; 0 or a negative value fails with the error
  `Mod_LoadSpriteModel: Invalid # of frames: <numframes>` (fatal in id's engine).
- **SPR-1.5** `type`, `boundingradius`, `width`, `height`, `beamlength` and `synctype` are not
  checked when the sprite loads. A `type` other than 0 to 4 loads and fails only when the
  sprite is drawn (`R_DrawSprite: Bad sprite type <type>` in `WinQuake/r_sprite.c`,
  `R_DrawSprite`); the frames are not checked against `width` and `height`.
- **SPR-1.6** A file shorter than the 36-byte header fails to load. id's code reads the
  header from the loaded buffer without checking its length, which is undefined; this spec
  fixes the answer as an error.
- **SPR-1.7** The model's bounds are `mins = (-width / 2, -width / 2, -height / 2)` and
  `maxs = (width / 2, width / 2, height / 2)` with C integer division, which truncates
  toward zero, so a width of 33 gives -16 and 16. The negation comes before the division and
  wraps as on x86, so a width of -2147483648 gives -1073741824 for both.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `SPR_MAGIC` | `IDSP` | `WinQuake/spritegn.h`, `IDSPRITEHEADER`, "little-endian "IDSP"" |
| `SPR_VERSION` | 1 | `WinQuake/spritegn.h`, `SPRITE_VERSION` |
| `SPR_HEADER_SIZE` | 36 | `WinQuake/spritegn.h`, `sizeof(dsprite_t)` |
| `SPR_VP_PARALLEL_UPRIGHT` | 0 | `WinQuake/spritegn.h`, `SPR_VP_PARALLEL_UPRIGHT` |
| `SPR_FACING_UPRIGHT` | 1 | `WinQuake/spritegn.h`, `SPR_FACING_UPRIGHT` |
| `SPR_VP_PARALLEL` | 2 | `WinQuake/spritegn.h`, `SPR_VP_PARALLEL` |
| `SPR_ORIENTED` | 3 | `WinQuake/spritegn.h`, `SPR_ORIENTED` |
| `SPR_VP_PARALLEL_ORIENTED` | 4 | `WinQuake/spritegn.h`, `SPR_VP_PARALLEL_ORIENTED` |
| `SPR_ST_SYNC` | 0 | `WinQuake/spritegn.h`, `ST_SYNC` |
| `SPR_ST_RAND` | 1 | `WinQuake/spritegn.h`, `ST_RAND` |

### Test vectors

See the table in SPR-2.

## SPR-2 Frames and frame groups

**Source:** `WinQuake/spritegn.h`, `dspriteframe_t`, `dspritegroup_t`, `dspriteinterval_t`,
`dspriteframetype_t`; `WinQuake/model.c`, `Mod_LoadSpriteModel`, `Mod_LoadSpriteFrame` and
`Mod_LoadSpriteGroup` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

The frames follow the header with no directory, each one's size given by its own fields, so
the loader walks them in order. A group is an animation the renderer plays by itself:
`R_GetSpriteframe` in `WinQuake/r_sprite.c` treats each interval as the time at which that
frame ends, takes the time modulo the last interval and shows the first frame whose interval
is greater.

| Structure | Offset | Size | Type | Field |
| --- | --- | --- | --- | --- |
| `dspriteframetype_t` | 0 | 4 | `int` | `type`: `SPR_SINGLE` 0, `SPR_GROUP` 1 |
| `dspriteframe_t` | 0 | 8 | `int[2]` | `origin`: x, then y |
| `dspriteframe_t` | 8 | 4 | `int` | `width` |
| `dspriteframe_t` | 12 | 4 | `int` | `height` |
| `dspritegroup_t` | 0 | 4 | `int` | `numframes` |
| `dspriteinterval_t` | 0 | 4 | `float` | `interval` |

### Behaviour

- **SPR-2.1** After the header come `numframes` frame entries, each starting with a 32-bit
  frame type. A type of 0 (`SPR_SINGLE`) is followed by one frame; any other value, not
  only 1 (`SPR_GROUP`), is followed by a frame group.
- **SPR-2.2** A frame is a 16-byte `dspriteframe_t` (`origin[0]`, `origin[1]`, `width`,
  `height`) followed by `width * height` bytes of palette indices, row by row from the top
  left. The next entry starts right after the pixels.
- **SPR-2.3** A frame's extents are `up = origin[1]`, `down = origin[1] - height`,
  `left = origin[0]` and `right = width + origin[0]`, computed in 32-bit integers that wrap
  as on x86.
- **SPR-2.4** A frame with a negative `width` or `height`, whose `width * height` does not
  fit in a 32-bit signed integer, or whose header or pixels run past the end of the file,
  fails to load. id's code copies `width * height` bytes without checking them, which is
  undefined; this spec fixes the answer as an error.
- **SPR-2.5** A frame group is a 32-bit `numframes`, then `numframes` float intervals, then
  `numframes` frames as in SPR-2.2, with no frame type before each.
- **SPR-2.6** Every interval must be greater than 0; one that is 0 or less fails with the
  error `Mod_LoadSpriteGroup: interval<=0` (fatal in id's engine). The test is
  `interval <= 0.0`, so a NaN interval loads.
- **SPR-2.7** A frame group's `numframes` must be at least 1. id's code loads a group of 0
  frames, and `R_GetSpriteframe` then reads the interval before the group's array, which is
  undefined; a negative count fails in id's `Hunk_AllocName`. This spec fixes both as an
  error.
- **SPR-2.8** An interval or frame of a group that runs past the end of the file fails to
  load, as in SPR-2.4.
- **SPR-2.9** Bytes after the last frame entry are ignored.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `SPR_SINGLE` | 0 | `WinQuake/spritegn.h`, `SPR_SINGLE` |
| `SPR_GROUP` | 1 | `WinQuake/spritegn.h`, `SPR_GROUP` |
| `SPR_FRAME_HEADER_SIZE` | 16 | `WinQuake/spritegn.h`, `sizeof(dspriteframe_t)` |

### Test vectors

Checked on 2026-10-16 against id's code: `Mod_LoadSpriteModel`, `Mod_LoadSpriteFrame` and
`Mod_LoadSpriteGroup` copied unchanged from the pinned commit with the `spritegn.h` types,
built with the system C compiler (Apple clang 21, arm64), with `Hunk_AllocName` as `calloc`, `r_pixbytes` 1 and
`Sys_Error` printing its message and stopping. Each vector is a sprite built by a script;
the result lists each frame entry as its extents and a group as its intervals.

| Input | Result | Statements |
| --- | --- | --- |
| Version 1, type 2, width 33, height 16, one single frame: origin (-16, 8), 2 by 3 pixels | Loads; bounds (-16, -16, -8) to (16, 16, 8); frame up 8, down 5, left -16, right -14 | SPR-1.1, SPR-1.7, SPR-2.2, SPR-2.3 |
| The same with version 2 | Error `s.spr has wrong version number (2 should be 1)` | SPR-1.3 |
| The same with `numframes` 0 | Error `Mod_LoadSpriteModel: Invalid # of frames: 0` | SPR-1.4 |
| Type 9, then one entry of frame type 7: a group of 2 frames, intervals 0.1 and 0.2, frames 1 by 1 | Loads; one group with intervals 0.1, 0.2 and two frames | SPR-1.5, SPR-2.1, SPR-2.5 |
| A group of 2 frames with intervals 0.1 and 0 | Error `Mod_LoadSpriteGroup: interval<=0` | SPR-2.6 |
| A group with a NaN interval | Loads | SPR-2.6 |
| Two single frames, then 5 bytes of padding | Loads; two frames | SPR-2.9 |

`qs-mdl` gives the same result for every row (its unit tests check each one), and the same
build and `qs-mdl` loaded the three sprites in LibreQuake's `lite` `id1` (the
`librequake-lite` test asset, extracted with `qs-vfs`: `progs/s_bubble.spr`,
`s_explod.spr` and `s_light.spr`) with identical headers, bounds, extents and pixels.
