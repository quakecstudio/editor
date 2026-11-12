# Lighting (light)

Prefix: `LIGHT` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes what id's `light` does: reading the light entities from a BSP's entity lump, finding
each face's sample points, lighting them, and writing the lightmaps into the BSP's lighting lump
(BSP-10) with each face's styles and offset (BSP-9.6); and, from ericw-tools, coloured light and
the `.lit` file (BSP-18). It is the contract for our `qmap-light` (P6-06) and the light half of
the map compiler differential (P6-07).

`light` is in `qutils/` of id-Software/Quake-Tools, which is not a portable upstream until the
license review rules on it ([`../porting.md`](../porting.md#upstreams), P4-22); ericw-tools is
GPL-2.0-or-later and cited for description only. Both may be run as oracles
(`cargo xtask oracles maps`); no code is ported from either:

| Short name | Repository and commit | Files read |
| --- | --- | --- |
| light | [id-Software/Quake-Tools](https://github.com/id-Software/Quake-Tools)@`c0d1b91c74eb654365ac7755bc837e497caaca73` | `qutils/LIGHT/LIGHT.C`, `LIGHT.H`, `LTFACE.C`, `TRACE.C`, `ENTITIES.C`, `ENTITIES.H` |
| ericw-tools | [ericwa/ericw-tools](https://github.com/ericwa/ericw-tools)@`36eec1da2a194467e6baac2f444c2dd04b57d266` | `light/entities.cc`, the `color` setting; `include/common/qvec.hh`, `normalize_color_format`; `light/ltface.cc`, `Light_ClampMin` and its callers; `light/write.cc`, `LightFace_ScaleAndClamp`, the sample output |

`light` is built without `DOUBLEVEC_T`, as `qbsp` is, so `vec_t` is `float`.

## LIGHT-1 The run and its modes

**Source:** light `LIGHT.C`, `main`, `LightWorld`, `LightThread`.

### Behaviour

- **LIGHT-1.1** `light [-threads n] [-extra] [-dist f] [-range f] file` reads `file` with its
  extension replaced by `.bsp`, lights every face of every model, and writes the lighting lump,
  each face's `styles` and `lightofs`, and the entity lump back. Any other option starting with
  `-` stops the run with `Unknown option "<option>"`.
- **LIGHT-1.2** `-dist` sets `scaledist` (default 1) and `-range` sets `rangescale` (default
  0.5); `scalecos` is 0.5 and has no option.
- **LIGHT-1.3** The fast mode, the default, takes one sample per 16 texels of each face (LIGHT-3.3).
  The full mode, `-extra`, takes four, every 8 texels, and averages each two by two (LIGHT-5.2).
- **LIGHT-1.4** Our `qmap-light` offers the same two modes: fast is LIGHT-1.3's default, full
  its `-extra`.
- **LIGHT-1.5** Faces are lit in face order and their lightmaps written one after another, each
  starting at a multiple of 4 bytes from the start of the lump. A lump past
  `MAX_MAP_LIGHTING` stops the run with `GetFileSpace: overrun`.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `LIGHT_SCALEDIST` | 1.0 | `LIGHT.C`, `scaledist` |
| `LIGHT_SCALECOS` | 0.5 | `LIGHT.C`, `scalecos` |
| `LIGHT_RANGESCALE` | 0.5 | `LIGHT.C`, `rangescale` |
| `LIGHT_ON_EPSILON` | 0.1 | `LIGHT.H`, `ON_EPSILON` |

## LIGHT-2 Light entities

**Source:** light `ENTITIES.C`, `LoadEntities`, `LightStyleForTargetname`, `MatchTargets`;
`ENTITIES.H`, `DEFAULTLIGHTLEVEL`.

### Behaviour

- **LIGHT-2.1** Any entity with a non-zero `light` value is a light. The value is read with
  `atof` from the key `_light` or any key whose first five bytes are `light` (`light`, and also
  `lightning`); the last such key in the entity wins, and it is stored as an `int`, truncated.
- **LIGHT-2.2** An entity whose classname starts with `light` and has no light value, or a value
  of 0, gets 300.
- **LIGHT-2.3** `origin` must be three numbers (`LoadEntities: not 3 values for origin`); `angle`
  is the spotlight's cone (LIGHT-4.4); `style`, read with `atof` into an `int`, must be 0 to 254
  (`Bad light style <n> (must be 0-254)`).
- **LIGHT-2.4** An entity of class `light` (exactly) with a `targetname` and no `style`, or style
  0, is a switchable light: it gets style 32 plus the number of distinct targetnames given styles
  before it, in entity order, the same targetname getting the same style, and the key `style` is
  written to it. More than 32 distinct targetnames overrun `light`'s table, which is undefined in
  C; this spec fixes the answer: the 33rd is refused.
- **LIGHT-2.5** An entity with a `target` points at the first entity whose `targetname` equals
  it; with none it prints `WARNING: entity at (x,y,z) (<class>) has unmatched target`. When the
  target has a non-zero style, the entity gets that style and the key `style` is written to it.
- **LIGHT-2.6** A light that targets an entity is a spotlight, aimed from its origin at the
  target's origin.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `LIGHT_DEFAULT_LEVEL` | 300 | `ENTITIES.H`, `DEFAULTLIGHTLEVEL` |
| `LIGHT_FIRST_SWITCHED_STYLE` | 32 | `ENTITIES.C`, `LightStyleForTargetname` |
| `LIGHT_MAX_SWITCHED` | 32 | `ENTITIES.C`, `lighttargets[32]` |

## LIGHT-3 Sample points

**Source:** light `LTFACE.C`, `CalcFaceVectors`, `CalcFaceExtents`, `CalcPoints`, `CastRay`.

### Behaviour

- **LIGHT-3.1** A face with `TEX_SPECIAL` in its texinfo's flags is not lit: its `lightofs` is -1
  and every style 255.
- **LIGHT-3.2** A face's extents are its vertices' texture coordinates `s` and `t` (BSP-8.1); its
  lightmap is `floor(min / 16)` to `ceil(max / 16)` on each axis, `size = max - min` luxels plus
  one. A size over 17 stops the run with `Bad surface extents`.
- **LIGHT-3.3** In the fast mode each luxel's sample is the point of the face's plane at texture
  coordinates `16 * (texmin + s)`, `16 * (texmin + t)`; in the full mode samples are every 8
  texels from half a luxel before the first, twice as many each way.
- **LIGHT-3.4** A sample point from which the face's middle cannot be reached by a line through
  empty space (LIGHT-4.1) is tried again, up to five more times, each time 8 texels nearer the
  middle on `t` then `s` alternately, stopping at the middle. When the sixth try fails too, the
  point used is the last one moved 8 units toward the middle in space.
- **LIGHT-3.5** Sample points lie one unit in front of the face's plane: the texture origin is
  moved along the texture normal until it is.

## LIGHT-4 Lighting a sample

**Source:** light `LTFACE.C`, `SingleLightFace`, `CastRay`; `TRACE.C`, `TestLine`,
`MakeTnodes`.

### Behaviour

- **LIGHT-4.1** A light reaches a sample when the line between them crosses no solid leaf of the
  world model's tree (model 0 only): only `CONTENTS_SOLID` blocks it, so light passes through sky
  and liquid. At each node, a line whose two ends are both more than `-ON_EPSILON` from the plane
  goes to the front child alone; otherwise one whose ends are both less than `ON_EPSILON` goes to
  the back child alone; otherwise it is split at the plane.
- **LIGHT-4.2** A light whose origin is not in front of the face's plane, or is farther from it
  than its light value (`scaledist` times the distance), lights nothing on the face.
- **LIGHT-4.3** A reached sample at distance `d` (times `scaledist`) gets
  `(light - d) * ((1 - scalecos) + scalecos * cos)`, `cos` the cosine between the face's normal
  and the direction to the light; a negative amount adds nothing.
- **LIGHT-4.4** A spotlight lights only samples within half its cone of its aim: the cone is
  `angle` degrees, 40 when `angle` is 0.
- **LIGHT-4.5** Each face has up to four lightmaps, one per style, in the order its lights first
  give a sample more than 1; a light of a fifth style prints
  `WARNING: Too many light styles on a face` and adds nothing.
- **LIGHT-4.6** `light` has a minimum light per face (`minlights`, `FixMinlight`) that nothing
  sets, so it never applies.

## LIGHT-5 Writing a lightmap

**Source:** light `LTFACE.C`, `LightFace`; `LIGHT.C`, `GetFileSpace`.

### Behaviour

- **LIGHT-5.1** A face no light reaches keeps `lightofs` -1 and styles 255. Otherwise its styles
  are its lightmaps' styles, 255 for unused slots, and its lightmaps are written one after
  another, each `size` bytes, row by row.
- **LIGHT-5.2** Each byte is the sample's light times `rangescale`, clamped to 255 and converted to
  a byte, truncating. In the full mode a luxel is the mean of its four samples first.
- **LIGHT-5.3** A negative byte stops the run with `light < 0`; none arises, every amount added
  being positive.

## LIGHT-6 Coloured light

**Source:** ericw-tools `light/entities.cc`, the light setting `color` (default
`255.0, 255.0, 255.0`); `include/common/qvec.hh`, `normalize_color_format`; `light/ltface.cc`,
`Light_ClampMin` (`color[i] * (light / 255.0f)`); `light/write.cc`, `LightFace_ScaleAndClamp`
and the sample output.

id's `light` has no colour. ericw-tools reads a light's colour from `_color` and writes a `.lit`
file beside the BSP (BSP-18) as well as the grey lump.

### Behaviour

- **LIGHT-6.1** A light's `_color` is three numbers; when all three are 0 to 1 they are scaled by
  255, otherwise taken as they are. A light with no `_color` is white, 255 255 255.
- **LIGHT-6.2** A light adds to each sample its colour times its amount (LIGHT-4.3) divided by
  255, per channel.
- **LIGHT-6.3** A sample's colour is clamped at 0, scaled by `rangescale`, and, when its largest
  channel is over 255, scaled down so that channel is 255.
- **LIGHT-6.4** The `.lit` file holds the three channels of each sample, as bytes, in the order of
  the lighting lump (BSP-18.1), and the grey lump's byte is the largest of the three ("This must be
  max(), see LightNormalize in MarkV 1036").
- **LIGHT-6.5** Our `qmap-light` writes a `.lit` when any light has a colour other than white, and
  the grey lump by LIGHT-6.4, so a map with white light only lights as id's `light` does.
