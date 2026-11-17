# Lighting (light)

Prefix: `LIGHT` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes what id's `light` does: reading the light entities from a BSP's entity lump, finding
each face's sample points, lighting them, and writing the lightmaps into the BSP's lighting lump
(BSP-10) with each face's styles and offset (BSP-9.6); and what ericw-tools' `light` does, which
our `qs-map-light` does in the extended dialect: coloured light and the `.lit` file (BSP-18),
sunlight, the sky dome, surface lights, minimum light, phong shading, dirt and bounce
(ADR-0027). It is the contract for our `qs-map-light` (P6-06, L-22, L-23) and the light half of
the map compiler differential (P6-07, P7-01).

The id dialect (LIGHT-1 to LIGHT-5) is written from id's `light` and holds its lightmaps to it
byte for byte. The extended dialect (LIGHT-8 to LIGHT-20) is ported from ericw-tools' `light/`
under ADR-0025, at the commit [`../porting.md`](../porting.md#upstreams) pins. Both tools are
run as oracles (`cargo xtask oracles maps`):

| Short name | Repository and commit | Files read |
| --- | --- | --- |
| light | [id-Software/Quake-Tools](https://github.com/id-Software/Quake-Tools)@`c0d1b91c74eb654365ac7755bc837e497caaca73` | `qutils/LIGHT/LIGHT.C`, `LIGHT.H`, `LTFACE.C`, `TRACE.C`, `ENTITIES.C`, `ENTITIES.H` |
| ericw-tools | [ericwa/ericw-tools](https://github.com/ericwa/ericw-tools)@`36eec1da2a194467e6baac2f444c2dd04b57d266` | `light/light.cc`, `entities.cc`, `ltface.cc`, `phong.cc`, `bounce.cc`, `write.cc`, `trace.cc`, `trace_embree.cc`; `common/settings.cc`, `entdata.cc`, `bsputils.cc`, `mathlib.cc`, `imglib.cc`; `include/common/qvec.hh`, `polylib.hh`; `qbsp/map.cc`, `writebsp.cc` for the files its qbsp writes beside a BSP |

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
- **LIGHT-1.4** Our `qs-map-light` offers the same two modes: fast is LIGHT-1.3's default, full
  its `-extra`.
- **LIGHT-1.5** Faces are lit in face order and their lightmaps written one after another, each
  starting at a multiple of 4 bytes from the start of the lump. A lump past
  `MAX_MAP_LIGHTING` stops the run with `GetFileSpace: overrun`.
- **LIGHT-1.6** `light` writes the entity lump back as `WriteEntitiesToString` does: each
  entity with keys between `{` and `}` lines, one `"key" "value"` line each, its keys in the
  reverse of the order they were read (`LoadEntities` puts each new pair first, and writes from
  the first), then a NUL that the lump's length counts. A key `SetKeyValue` sets replaces the
  value of the key of that name read last, or is added before every other. The entity with no
  keys is not written. Text past `MAX_MAP_ENTSTRING` (65536) once an entity is written stops the
  run with `Entity text too long`; more than `MAX_MAP_ENTITIES` (1024) entities stop it with
  `LoadEntities: MAX_MAP_ENTITIES`.
- **LIGHT-1.7** **Withdrawn** 2026-11-13: it said everything but the fixed tables is the same
  in both dialects, which stopped being true when the extended dialect began leaving the faces
  `light` stops on unlit; replaced by LIGHT-1.9 and LIGHT-1.10.
- **LIGHT-1.8** `light` lays its lightmaps into `dlightdata`, which still holds the input's
  lighting lump, and writes each lump with `AddLump`, which writes `(len+3)&~3` bytes from the
  lump's array. So the bytes skipped to bring a face's lightmaps to a multiple of 4, and the
  padding after the lighting and entity lumps, are the bytes the input's lump held at those
  offsets, and zero past its end; every other lump's padding is zero. The file is laid out as
  VIS-5.6 states for vis.
- **LIGHT-1.9** **Withdrawn** 2026-11-17: it said the extended dialect is id's `light` without
  its fixed tables, which stopped being true when the extended dialect became ericw-tools'
  `light` (ADR-0027); replaced by LIGHT-1.11 and LIGHT-8.1.
- **LIGHT-1.10** **Withdrawn** 2026-11-17: it said the extended dialect leaves a face the id
  dialect stops on unlit with a warning; the extended dialect now places samples as
  ericw-tools does; replaced by LIGHT-8.3.
- **LIGHT-1.11** Our `qs-map-light` has `qs-map-bsp`'s two dialects (QBSP-14). The id dialect is
  id's `light` (LIGHT-1 to LIGHT-5) with coloured light (LIGHT-6) and with `light`'s fixed
  tables: LIGHT-1.5's `MAX_MAP_LIGHTING`, LIGHT-1.6's `MAX_MAP_ENTITIES` and
  `MAX_MAP_ENTSTRING`, and LIGHT-2.8's line. The extended dialect, the default, is ericw-tools'
  `light` (LIGHT-8), limited only by the file's fields, a lightmap whose offset does not fit
  `lightofs` stopping the run.

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
- **LIGHT-2.7** The entity lump is read up to its first NUL as `COMMON/CMDLIB.C`,
  `COM_Parse`, reads tokens: bytes up to a space, and bytes over 127 (negative in a signed
  `char`), are white space; `//` starts a comment to the end of the line; a `"` starts a string
  that runs to the next `"`; each of `{`, `}`, `(`, `)`, `'` and `:` is a token alone; any other
  run of bytes ends before white space or one of those six. A string with no closing `"` reads
  past the text's end, which is undefined in C; this spec fixes the answer: it runs to the end.
  An entity must start with `{` (`LoadEntities: found <token> when expecting {`); a key or value
  missing at the end stops the run with `LoadEntities: EOF without closing brace`, and a value
  that starts with `}` with `LoadEntities: closing brace without data`. Keys and values longer
  than `light`'s buffers (`char key[64]`, `MAX_KEY` 32 and `MAX_VALUE` 1024 in an `epair_t`, and
  the `classname`, `target` and `targetname` fields) overrun them, which is undefined; this
  spec fixes the answer: they are kept whole.
- **LIGHT-2.8** `WriteEntitiesToString` formats each pair into `char line[128]`, so a key and
  value of more than 121 bytes together overflow it, which is undefined (a build with stack
  checks stops there, as on LibreQuake's maps, whose `wad` and `_credits` values are longer).
  This spec fixes the answer as MAP-2.10 does for qbsp: the id dialect refuses such a pair, and
  the extended dialect writes it whole.

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
- **LIGHT-3.6** `CalcFaceVectors` takes the texture normal, the cross product of the texinfo's
  t and s axes, normalized; a texture normal at right angles to the face, its dot product with
  the face's normal exactly 0, stops the run with `Texture axis perpendicular to face`. It is
  flipped to point the face's way, and each texture axis is projected back into the face along
  it. `VectorNormalize` and `VectorLength` sum the squares in `double`, and `VectorMA` works in
  `double` (its scale is a `double`), each result stored as `float`.

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
- **LIGHT-4.7** A light of a style the face has no lightmap for yet starts one in the next slot
  by clearing its first `(texsize[0]+1) * (texsize[1]+1)` samples, one per luxel. That is every
  sample in the fast mode, but only the first quarter in the full mode, which has four per
  luxel; and a light that gives no sample more than 1 leaves what it added in the slot. So in
  the full mode the next light to start a lightmap in that slot adds to what the earlier one left
  in its last three quarters.
- **LIGHT-4.8** `TestLine` keeps the far parts of a split line on a stack of 64
  (`tracestack_t tracestack[64]`); a line split more than 64 times overruns it, which is
  undefined; this spec fixes the answer: the stack has no limit.
- **LIGHT-4.9** `ON_EPSILON` (`LIGHT.H`) is a `double`, so each `float` distance of LIGHT-4.1 is
  widened to be compared with it: a distance of exactly `0.1f`, a little over 0.1, is beyond
  it.

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
- **LIGHT-6.5** In the id dialect our `qs-map-light` writes a `.lit` when any light has a colour
  other than white, and the grey lump by LIGHT-6.4, so a map with white light only lights as
  id's `light` does. The extended dialect's rule is LIGHT-18.6.

## LIGHT-7 Light previews

**Source:** this project's design (P7-09, MAP-10): the map editor draws each light as `light`
lights the map, read by the same reader (LIGHT-2), so what the modder sees is what compiles.

### Behaviour

- **LIGHT-7.1** A light's preview radius is its light value (LIGHT-2.1, LIGHT-2.2) over
  `scaledist` (LIGHT-1.2): a face farther than it lights nothing (LIGHT-4.2), and at it the
  amount (LIGHT-4.3) is 0.
- **LIGHT-7.2** A light's preview colour is its `_color` as LIGHT-6.1 reads it, each channel
  over 255; white for a light with none.
- **LIGHT-7.3** A spotlight's preview has its cone: the aim, the unit direction from its origin
  to its target's (LIGHT-2.6), and the whole cone's angle, its `angle` in degrees, 40 when it is
  0 (LIGHT-4.4).
- **LIGHT-7.4** The map view draws the previews of the selected lights, or of every light with
  **Every light** in its **Show** list, from the map as the editor holds it in each frame, so an edit of a light's key shows in the frame
  it is made: a sphere of three circles at the radius in the light's colour, and for a
  spotlight the cone's edge out to the radius.

## LIGHT-8 The extended dialect's run

**Source:** ericw-tools `light/light.cc`, `light_main`, `LightWorld`; `light/ltface.cc`,
`Face_IsLightmapped`, `DirectLightFace`, `IndirectLightFace`, `PostProcessLightFace`;
`light/entities.cc`, `LoadEntities`, `WriteEntitiesToString`; `common/entdata.cc`,
`EntData_Write`.

### Behaviour

- **LIGHT-8.1** In the extended dialect `qs-map-light` lights a BSP as ericw-tools' `light`
  does with no options: the world's settings and each light's are keys of the map (LIGHT-9,
  LIGHT-10), shadows are cast by triangles (LIGHT-11), and the lightmaps are written as
  LIGHT-18 says. Besides the BSP it takes what ericw-tools' qbsp writes beside it, each
  texinfo's flags (LIGHT-9.8) and each leaf's contents (LIGHT-9.9), and the game's palette
  (LIGHT-11.6); without them the BSP alone is lit.
- **LIGHT-8.2** The fast mode takes one sample a luxel. The full mode is ericw-tools' `-extra`:
  two samples a luxel each way, from a quarter of a luxel before its middle, averaged as
  LIGHT-18.4 says.
- **LIGHT-8.3** A face has a lightmap when it has at least three edges, a model, a texinfo
  without `TEX_SPECIAL`, and a texture named neither `trigger` nor `skip`, whatever its case.
  A face whose two texture axes and normal do not span space (`Bad texture axes on face`) is
  left unlit; a face of any size is lit, where id's `light` stops past 17 luxels (LIGHT-3.2).
- **LIGHT-8.4** Every face is lit directly first: its dirt (LIGHT-19), then each light and sun
  whose light is over 0, then the local minimum lights (LIGHT-17.3). Then each bounce
  (LIGHT-20) is added to every face. Last, for each face, the minimum light (LIGHT-17.1), for
  a brush model the light at its middle (LIGHT-17.4), and each light and sun whose light is
  under 0, which take light away. A model with `_lightignore`, or a face with the flag
  `light_ignore`, takes only the minimum light.
- **LIGHT-8.5** The entity lump is read as LIGHT-2.7 reads its tokens, each key once, a key
  read again replacing the first's value where it stands, and with the white space about a
  key taken off. Its first entity must be the worldspawn (`WorldEnt() failed to get
  worldspawn`). A key `lightmap_scale` is renamed `_lightmap_scale`.
- **LIGHT-8.6** The entity lump is written back with each entity between `{` and `}` lines and
  its keys in the order read, a key that was set keeping its place and a new one going last,
  then a NUL. In each value a `\b` is taken out and the bytes from it to the next `\b` have
  their high bit set.
- **LIGHT-8.7** Not ported, each a divergence from ericw-tools: `_lightmap_scale` and
  `_world_units_per_luxel` (a lightmap is 16 texels to the luxel); projected textures
  (`_project_texture`); radiosity surface lights (`_surface_radiosity`, `_surflight_radiosity`);
  `_suntexture`; `_minlight_mottle`; `_dirtmode` 1, whose rays are drawn at random on each
  thread; the shadows of brush models with no faces; and the command line's options.
- **LIGHT-8.8** `qs-map-light` offers a quick light for playing a map at once (MAP-13): the
  extended dialect's lighting without dirt (LIGHT-19) and bounce (LIGHT-20), the two that
  cost the most; everything else is as the final light's (MAP-14). ericw-tools has no such
  mode.

## LIGHT-9 Settings

**Source:** ericw-tools `common/settings.cc`, `setting_container::find_setting`,
`set_setting` and each setting's `parse`; `light/light.cc`, `worldspawn_keys::worldspawn_keys`,
`modelinfo_t::modelinfo_t`, `FixupGlobalSettings`, `FindModelInfo`; `common/bspfile_common.cc`,
`surfflags_t::from_json`, `LoadExtendedTexinfoFlags`, `LoadExtendedContentFlags`;
`include/common/qvec.hh`, `normalize_color_format`.

### Behaviour

- **LIGHT-9.1** A key names a setting by its name with every leading underscore taken off, so
  `_sunlight`, `sunlight` and `__sunlight` are one setting; some settings have several names.
- **LIGHT-9.2** A value is read token by token, tokens parted by white space. A number is the
  start of a token that converts as `strtod` converts it; a token with no such start leaves
  the setting as it was. A whole number is the token's decimal digits after an optional sign,
  cut to 32 bits. A flag is on unless its first token is `0` or `-1`, also with no token. A
  vector is three numbers; a mangle is up to three, the rest 0. A string is the first token.
- **LIGHT-9.3** A setting remembers who gave its value: nothing, another setting, or a key of
  the map, each outweighing the one before. A value from a lighter source does not replace
  one from a heavier, whatever the order of the keys. A setting is changed once anything but
  its default gave its value.
- **LIGHT-9.4** The worldspawn's keys set the world's settings (the table below). A key that
  names one and whose value does not read is reported, `WARNING: worldspawn key <key> has
  invalid value`, and the setting keeps its default. A number past a setting's bounds is held
  at them.
- **LIGHT-9.5** `_dirt` on the world turns on, unless the map set them, dirt on the minimum
  light (`_minlight_dirt`), on sunlight (`_sunlight_dirt` 1) and on the sky dome
  (`_sunlight2_dirt` 1). A light's or a model's `_dirt` is 1 for dirt, -1 for none, and
  anything else for what the world's `_dirt` says.
- **LIGHT-9.6** A colour is three numbers; when each is 0 to 1 they are scaled by 255,
  otherwise taken as they are.
- **LIGHT-9.7** Each brush model takes the settings of the entity whose `model` is `*<n>`
  (`Couldn't find entity for model *<n>` when none is): `_minlight`, `_maxlight`,
  `_minlight_color` or `_mincolor`, `_shadow`, `_shadowself` or `_selfshadow`,
  `_shadowworldonly`, `_switchableshadow`, `_dirt`, `_phong`, `_phong_angle`, `_alpha`,
  `_lightignore`, `_lightcolorscale`, `_object_channel_mask`, `_autominlight` and
  `_autominlight_target`. Its faces are moved by the entity's `origin`. The world always casts
  shadow, and its phong angle is the worldspawn's `_phong_angle`.
- **LIGHT-9.8** Each texinfo may carry flags, which ericw-tools' qbsp writes beside the BSP as
  `<map>.texinfo.json` from the keys of the brush entity a face came from, so that the keys of
  a `func_group` or `func_detail`, whose faces are the world's, reach `light`: `phong_angle`,
  `phong_angle_concave`, `phong_group`, `minlight`, `minlight_color`, `maxlight`,
  `lightcolorscale`, `light_alpha`, `light_twosided`, `no_dirt`, `no_shadow`, `no_bounce`,
  `no_minlight`, `light_ignore`, `object_channel_mask` and `surflight_group`. A model's own
  key, where it has the same meaning, outweighs its faces' flags.
- **LIGHT-9.9** Each leaf may carry its own contents, which ericw-tools' qbsp writes beside the
  BSP as `<map>.content.json`: the BSP's contents are only the nearest the format has, and
  call the inside of a fence or a window solid. A leaf whose contents there have `SOLID`,
  `DETAIL_WALL` or `SKY` holds no sample (LIGHT-12.7); any other may. Without the file a
  solid or sky leaf of the BSP holds none.

### Constants

The world's settings, each with its default; a second name is another key for the same.

| Key | Default | Bounds | What |
| --- | --- | --- | --- |
| `_dist` | 1 | 0 to 100 | How much a light's distance counts |
| `_range` | 0.5 | 0 to 100 | What a sample is scaled by (LIGHT-18.1) |
| `_anglescale`, `_anglesense` | 0.5 | 0 to 1 | How much the angle of incidence counts |
| `_gamma` | 1 | 0 to 100 | LIGHT-18.1 |
| `_addmin` | off | | LIGHT-17.2 |
| `light`, `_minlight` | 0 | | LIGHT-17.1 |
| `_maxlight` | 0 | | LIGHT-18.1 |
| `_minlight_color`, `_mincolor` | 255 255 255 | | LIGHT-17.1 |
| `_spotlightautofalloff` | off | | LIGHT-10.5 |
| `_compilerstyle_start`, `_compilerstyle_max` | 32, 64 | | LIGHT-10.2 |
| `_dirt`, `_dirty`, `_dirtmode`, `_dirtdepth`, `_dirtscale`, `_dirtgain`, `_dirtangle`, `_minlight_dirt` | off, 0, 128, 1, 1, 88, off | depth from 1; scale and gain 0 to 100; angle 1 to 90 | LIGHT-19 |
| `_phong`, `_phong_angle` | on, 0 | | LIGHT-13 |
| `_bounce`, `_bouncestyled`, `_bouncescale`, `_bouncecolorscale`, `_bouncelightsubdivision` | 0, off, 1, 0, 64 | bounce 0 to 100; scale 0 to 100; colour scale 0 to 1 | LIGHT-20 |
| `_sunlight`, `_sun_light`; `_sunlight_color`, `_sun_color`; `_sunlight_mangle`, `_sun_mangle`, `_sun_angle`; `_sunlight_penumbra`; `_sunlight_dirt` | 0; white; straight down; 0; 0 | penumbra 0 to 180 | LIGHT-14 |
| `_sun2`, `_sun2_color`, `_sun2_mangle` | 0, white, straight down | | LIGHT-14.2 |
| `_sunlight2`; `_sunlight2_color`, `_sunlight_color2`; `_sunlight3`; `_sunlight3_color`, `_sunlight_color3`; `_sunlight2_dirt` | 0; white; 0; white; 0 | | LIGHT-15 |

## LIGHT-10 Lights in the extended dialect

**Source:** ericw-tools `light/entities.cc`, `light_t::light_t`, `LoadEntities`,
`LightStyleForTargetname`, `CheckEntityFields`, `MatchTargets`, `SetupSpotlights`,
`JitterEntity`, `FixLightOnFace`, `EstimateVisibleBoundsAtPoint`, `SetupLights`;
`light/ltface.cc`, `GetLightValue`, `GetLightValueWithAngle`, `GetLightContrib`, `CullLight`,
`LightFace_Entity`; `common/settings.cc`, `setting_light::parse`.

### Behaviour

- **LIGHT-10.1** Every entity whose classname starts with `light` is a light. Its `light` is
  one number, the brightness; or four, a colour and the brightness; or three, a colour, the
  brightness left at 300. `_color` outweighs a colour `light` gives (LIGHT-9.3). A light with
  no `light` is 300; one with `light` 0 gives no light.
- **LIGHT-10.2** Every such entity with a `targetname` gets the style of that name in its
  `style` key, whatever style it had: the first name 32 (`_compilerstyle_start`), each new
  name the next, and a name that would be 64 (`_compilerstyle_max`) stops the run with `Too
  many unique light targetnames (max=64)`.
- **LIGHT-10.3** A light's settings are put right: `wait` of 0 or less is 1; an `_anglescale`
  outside 0 to 1 is the world's; `_falloff` counts only on a linear light; a `delay` that is
  no formula makes the light linear and puts it out. `delay` is 0 `linear`, 1 `inverse`, 2
  `inverse2`, 3 `infinite`, 4 `localmin` (LIGHT-17.3) or 5 `inverse2a`, by number or name. At
  distance `d`, with `v = _dist * wait * d`, a light of brightness `L` gives: linear, `L - v`
  and no less than 0, or with `_falloff` `f`, `L * (1 - d / f)` within `f` and nothing beyond;
  inverse, `L / (v / 128)`; inverse2, `L / (v * v / (128 * 128))`; inverse2a, the same with
  `v + 128` for `v`; infinite, `L`. A light under 0 takes away by the same formulas. A light
  with `_deviance` shares an inverse, inverse2, infinite or inverse2a brightness among its
  `_samples`.
- **LIGHT-10.4** Lights are set up in this order: surface lights are made (LIGHT-16); lights
  with `_deviance` are scattered (LIGHT-10.6); targets are matched, a light's target being the
  first entity whose `targetname` is its `target` but for case; spotlights are aimed
  (LIGHT-10.5); suns and domes are made (LIGHT-14, LIGHT-15); lights in solid are moved
  (LIGHT-10.7); and, when the BSP has no visibility, what each light sees is found
  (LIGHT-10.8).
- **LIGHT-10.5** A light with a target is a spotlight shining at the target's origin; one with
  `mangle` shines along it (LIGHT-14.2). Its cone is `angle` degrees, 40 with none or 0; its
  inner cone `_softangle`, the cone itself when that is 0 or less or wider than the cone. A
  sample outside the cone gets nothing, one inside the inner cone everything, and one between
  them in proportion, by the cosine of its angle from the aim. With `_spotlightautofalloff`
  on the world, a spotlight with a target and no `_falloff` gets one: the distance to its
  target plus that distance times the tangent of half the cone.
- **LIGHT-10.6** A light with `_deviance` over 0 gets `_samples` less one copies, 16 samples
  with none given, each moved on each axis by up to the deviance, drawn as LIGHT-14.5 draws.
  A sun is not scattered.
- **LIGHT-10.7** A light whose origin is in the world's solid (LIGHT-12.7) and that gives
  light, has not `_nonudge`, and is on the default channel, is moved to the first of the six
  points 2 units from it along x, y and z, back before forward on each, that is not in solid;
  it stays when none is.
- **LIGHT-10.8** When the BSP has no visibility, what each light can see is the box of where
  1024 rays spread over the sphere about it end, each at what it reaches or 65536 units out,
  grown on every side by a quarter of its size. A light whose box is more than 0.001 from a
  face's box lights nothing on it. With visibility in the BSP no light is culled this way.
- **LIGHT-10.10** When the BSP has visibility, each face has what its leaves see between
  them: the rows, unpacked from their runs, a 0 and a count standing for that many bytes of
  0, of the leaves that list a face of the world, or of the leaves a brush model's samples
  are in; everything when one of them is a liquid or has no row. A light on the default
  channel whose leaf is not among them lights nothing on the face, unless its leaf is solid,
  sky or a liquid. A bounce light (LIGHT-20) is passed over for a face that cannot see every
  leaf its own face is in.
- **LIGHT-10.9** A light lights a face when it is in front of the face's plane, or the face
  is curved (LIGHT-13.6) or lit from both sides, or the light has `_bleed`; and when its
  brightness at the nearest point of the sphere around the face is over 0.001. Each sample
  gets the light's colour over 255 times its brightness at the sample's distance (LIGHT-10.3)
  times `(1 - anglescale) + anglescale * cos`, `cos` the cosine between the sample's normal
  and the direction to the light, nothing when the light is behind the sample. A sample
  nearer than 0.1 is taken as 0.1 away, the light straight above. A contribution whose mean
  channel is at most 0.001 is not traced. The rest reach the sample when nothing stops the
  ray from it to the light (LIGHT-11).

## LIGHT-11 Shadows

**Source:** ericw-tools `light/trace_embree.cc`, `Embree_TraceInit`, `CreateGeometry`,
`Embree_FilterFuncN`, `PerRay_FilterFuncN`; `include/light/trace_embree.hh`; `light/trace.cc`,
`SampleTexture`; `light/light.cc`, `FindModelInfo`.

### Behaviour

- **LIGHT-11.1** Light is stopped by triangles: each shadow-casting face cut into the
  triangles of its second to last corners, each with the one before and the first, moved with
  its model. A model casts shadow when it is the world, or has `_shadow`, `_shadowself`,
  `_shadowworldonly` or `_switchableshadow` over 0, or is not on the default channel; a face
  with the flag `no_shadow` casts none. Either side of a triangle stops a ray.
- **LIGHT-11.2** A face is what its texture's name makes it: sky when the name starts with
  `sky` in any case, liquid when it starts with `*`, a fence when it starts with `{`. A sky
  face is sky, which sunlight comes from and which stops every other light. A liquid face of
  the world casts no shadow; one of another model is solid. Any other face of the world, or
  of a model with `_shadow`, is solid. The faces of a switchable shadow, of a model not on
  the default channel, of glass, of fences, and of a model that shadows only itself or only
  the world, are asked of each ray (LIGHT-11.4 to LIGHT-11.8).
- **LIGHT-11.3** A light reaches a sample when no triangle stops the ray from the sample
  toward it within its distance. Sunlight reaches a sample when the first thing the ray along
  the sun's direction reaches is sky.
- **LIGHT-11.4** A model with `_shadowworldonly` stops only the rays of the world's samples;
  one with `_shadowself` only those of its own.
- **LIGHT-11.5** An entity with `_switchableshadow` 1 gets a style of its own in
  `switchshadstyle`, by its `targetname` as LIGHT-10.2 gives styles, a new one each time it
  has none. Its faces stop no ray; a ray that crosses one carries its style, and a light of
  style 0 lights that sample in the lightmap of that style instead, so the shadow can be
  switched. A ray from a model of the same `switchshadstyle` is stopped by it. A light with
  `_switchableshadow_target` gives no light of its own and makes the model of that
  `targetname` a switchable shadow of the light's style.
- **LIGHT-11.6** Glass, a face whose alpha is under 1 (the flag `light_alpha`, else its
  model's `_alpha`), stops no ray. A ray that leaves it, meeting its back, takes its colour:
  the light is turned toward the light times the texel's colour over 255, by the glass's
  alpha. The texel is the nearest of the face's texture at the point, the texture repeating;
  its colour is the palette's. A missing texture, or no palette, gives no colour.
- **LIGHT-11.7** A fence stops a ray only where its texel is not see-through: index 255 of a
  fence's texture is. A missing texture is see-through everywhere.
- **LIGHT-11.8** A face is on the channels of the flag `object_channel_mask`, else of its
  model's `_object_channel_mask`, 1 by default; a light lights the faces whose channels its
  `_light_channel_mask` shares, and its rays are stopped by those its `_shadow_channel_mask`
  shares, which is the light mask when not given. A ray of the default mask is stopped by
  every sky and solid triangle.
- **LIGHT-11.9** ericw-tools asks Embree, whose tree of boxes and whose arithmetic are its own,
  what a ray meets; which triangle a ray that touches an edge meets is not specified by
  either. This spec fixes our answers. A ray through the edge of a triangle hits it, the
  products of the test fused into its sums as Embree works them. A ray that does not move
  along an axis lies in a plane of the grid maps are built on; Embree's slab test loses it
  at a box that ends on that plane, a distance of 0 times an infinite reciprocal. Such a ray
  is stopped by a triangle that spans its plane, and by one that ends on it only where
  another across the plane from it is hit at the same distance: it is stopped by a surface it
  crosses and passes the edge of one it only touches. A ray that moves less than a millionth
  of its length along an axis is stopped by a triangle that ends on its plane only on the
  side it moves toward.

## LIGHT-12 Sample points in the extended dialect

**Source:** ericw-tools `light/ltface.cc`, `Lightsurf_Init`, `CalcPoints`, `CalcPointNormal`,
`PositionSamplePointOnFace`, `Light_PointInAnySolid`; `common/bsputils.cc`,
`faceextents_t::faceextents_t`, `WorldToTexSpace`, `TexSpaceToWorld`, `Light_PointInSolid`;
`common/mathlib.cc`, `MakeInwardFacingEdgePlanes`, `EdgePlanes_PointInside`,
`ClosestPointOnPolyBoundary`.

### Behaviour

- **LIGHT-12.1** A face's lightmap is, as LIGHT-3.2's, `floor(min / 16)` to `ceil(max / 16)`
  of its corners' texture coordinates on each axis, each coordinate worked in `double` and
  kept as a `float`. The world point of a lightmap coordinate is found by the inverse, worked
  in `double`, of the map whose rows are the two texture axes and the face's normal.
- **LIGHT-12.2** Sample `s`, `t` of a face is wanted at lightmap coordinates
  `-0.5 + 0.5 / n + s / n` on each axis, `n` the samples a luxel takes each way: at the
  luxel's own coordinates in the fast mode.
- **LIGHT-12.3** A sample lies one unit in front of its face.
- **LIGHT-12.4** The wanted point is put on the face's plane and lifted a unit. It is on the
  face when it is in front of every edge's plane or no more than 0.05 behind one.
- **LIGHT-12.5** A point off the face goes to the face across the edge it is behind, that
  edge where it is least past the edge's ends, when that face is smoothed with this one
  (LIGHT-13.3) or lies on its plane the same way round, and is placed there by LIGHT-12.4 on,
  up to three faces away. The face across an edge is the first that walks the edge's two
  vertices the other way.
- **LIGHT-12.6** A point with no such face, when it is within a luxel of the face in the
  lightmap, snaps to the nearest point of the face's edges; farther, the sample is hidden.
- **LIGHT-12.7** A sample placed on a face takes the face's normal, or with phong
  (LIGHT-13.5) the normal there. When it is in solid it is moved by half a unit on each
  axis, each of the eight ways in turn from all down to all up, to the first that is not;
  when none is, it is hidden. A point is in solid when it is in a solid or sky leaf
  (LIGHT-9.9) of hull 0 of its own model, of the world when its model is on the default
  channel, or of a model of its channel that has `_shadow` and an alpha of 1; at a node
  whose plane it is within 0.1 of, both sides are tried.

## LIGHT-13 Phong shading

**Source:** ericw-tools `light/phong.cc`, `CalculateVertexNormals`, `MakeEdgeToFaceMap`,
`Face_EdgeIndexSmoothed`; `light/light.cc`, `modelinfo_t::getResolvedPhongAngle`;
`common/mathlib.cc`, `InterpolateNormal`.

### Behaviour

- **LIGHT-13.1** A model's phong angle is its `_phong_angle`, or 89 with `_phong` over 0;
  rounded to a whole number, to even on a half, and held to 255, it is the `phong_angle` of
  the texinfo of every face of the model.
- **LIGHT-13.2** A face is smoothed within its `phong_angle`, 89 when it has none and has a
  `phong_group`; and, where it meets another in a hollow, within its `phong_angle_concave`,
  the `phong_angle` when that is 0.
- **LIGHT-13.3** Two faces that share a vertex are smoothed together when both are smoothed,
  both or neither is `TEX_SPECIAL`, their `phong_group`s are the same, and the cosine between
  their normals is at least that of the lesser of their two angles: the concave ones when the
  middle of the second is more than 0.1 in front of the first.
- **LIGHT-13.4** A face's normal at a corner is the sum, made unit, of its own normal and of
  each face's it is smoothed with that has the corner, each weighed by that face's area times
  its angle at the corner; the face's own normal when the sum is nothing.
- **LIGHT-13.5** With phong a sample's normal is its face's corners' normals at the sample:
  those of the first triangle fanned from the first corner that holds it, by its barycentric
  coordinates.
- **LIGHT-13.6** A face is curved when its `phong_angle` is not 0 or it has a `phong_group`.
  The worldspawn's `_phong` 0 turns phong off: no normal is interpolated.

## LIGHT-14 Sunlight

**Source:** ericw-tools `light/entities.cc`, `AddSun`, `SetupSun`, `SetupSuns`;
`light/ltface.cc`, `LightFace_Sky`; `include/common/qvec.hh`, `vec_from_mangle`;
`common/mathlib.cc`, `Random`.

### Behaviour

- **LIGHT-14.1** A sun is a direction light comes from wherever the sky is seen along it. It
  is put 16384 units against the way its light goes. A sun of no light is not made.
- **LIGHT-14.2** The world's sun has the brightness `_sunlight` and the colour
  `_sunlight_color`, and shines along `_sunlight_mangle`, straight down with none; a second,
  `_sun2`, has `_sun2_color` and `_sun2_mangle`. A mangle is a yaw and a pitch in degrees: the
  direction `(cos yaw * cos pitch, sin yaw * cos pitch, sin pitch)`. A light entity with
  `_sun` and a brightness over 0 is a sun of its brightness, colour, `_anglescale`, `_dirt`
  and style, shining at its target, or along its `mangle`, or from its origin toward the
  map's origin; it gives no light of its own.
- **LIGHT-14.3** A sun with no penumbra is one sun.
- **LIGHT-14.4** A sun with a penumbra of `d` degrees, the world's `_sunlight_penumbra` or a
  sun light's `_deviance`, is 64 suns sharing its brightness: the first along its direction,
  each other with its yaw and its pitch each moved by up to `d` degrees. The draw is taken
  again while the two moves, in radians, squared and added, are over `d * d`, `d` in
  degrees, which they never are.
- **LIGHT-14.5** The moves are drawn as `rand() / RAND_MAX * 2 - 1` from the C library's
  `rand`, which no call seeds, so each sequence is a platform's: ericw-tools scatters
  otherwise on Linux, on macOS and on Windows. `qs-map-light` offers glibc's sequence, the
  default, and the BSDs', which is macOS's.
- **LIGHT-14.6** A sun takes dirt (LIGHT-19) when its dirt is 1, not when it is -1, and
  otherwise when the world's `_dirt` is on: the world's suns' is `_sunlight_dirt`, the
  dome's `_sunlight2_dirt`, a sun light's its `_dirt`.
- **LIGHT-14.7** A sun lights a face that is not turned more than 0.01 from it, or is curved
  or lit from both sides, and is on the default channel. Each sample gets the sun's colour
  over 255 times its brightness times `(1 - anglescale) + anglescale * cos`, `cos` at least
  0, with the world's `_anglescale` for the world's suns; it reaches the sample when the ray
  along the sun's direction reaches sky first (LIGHT-11.3), within 1000000 units.

## LIGHT-15 The sky dome

**Source:** ericw-tools `light/entities.cc`, `SetupSkyDome`, `SetupSkyDomes`.

### Behaviour

- **LIGHT-15.1** `_sunlight2` is a dome of 65 suns over the upper hemisphere sharing its
  brightness, of the colour `_sunlight2_color`: four rings of sixteen, at 9, 27, 45 and 63
  degrees above the horizon, each ring starting a quarter of a step on from where the last
  ended, and one straight overhead. The angles are kept and stepped in single precision.
- **LIGHT-15.2** `_sunlight3` is the same dome under the lower hemisphere, of the colour
  `_sunlight3_color`, lighting from below. A light entity with `_sunlight2` and a brightness
  over 0 is an upper dome of its brightness, colour, `_anglescale`, `_dirt` and style, and
  one with `_sunlight3` a lower; it gives no light of its own.

## LIGHT-16 Surface lights

**Source:** ericw-tools `light/entities.cc`, `MakeSurfaceLights`,
`FaceMatchesSurfaceLightTemplate`, `GL_SubdivideSurface`, `SubdividePolygon`,
`CreateSurfaceLightOnFaceSubdivision`, `CreateSurfaceLight`.

### Behaviour

- **LIGHT-16.1** A light entity with `_surface` is a template and gives no light itself. Every
  face whose texture's name is `_surface`'s, whatever its case, and whose `surflight_group`
  is the template's `_surflight_group`, gets copies of it. Faces are taken leaf by leaf in
  the order of the leaves' face lists, each once; the liquid faces of a leaf that is not
  empty are passed over.
- **LIGHT-16.2** A face is cut along each axis in turn at the multiple of 128 nearest the
  middle of its extent, when the cut leaves at least 8 units on each side, and each part cut
  again, until no cut does.
- **LIGHT-16.3** A copy of the template is put over the middle of each part, the mean of its
  fanned triangles' middles by area, 2 units along the face's normal, or `_surface_offset`,
  and moved with the face's model. With `_surface_spotlight` the copy is a spotlight along
  the normal.

## LIGHT-17 Minimum light

**Source:** ericw-tools `light/ltface.cc`, `LightFace_Min`, `LightFace_LocalMin`,
`LightFace_AutoMin`, `Light_ClampMin`, `CalcLightgridAtPoint`, `LightPoint_Entity`,
`LightPoint_Sky`, `LightPoint_ScaleAndClamp`; `light/lightgrid.cc`,
`FixPointAndCalcLightgrid`.

### Behaviour

- **LIGHT-17.1** A face's minimum light is its model's `_minlight`, else its flag `minlight`,
  else the world's, with the colour of the same; when that is not over the world's, the
  world's with the world's colour. Every sample of the face, hidden or not, has each channel
  of its style 0 lightmap raised to the colour's times the minimum over 255. A face with the
  flag `no_minlight` takes none.
- **LIGHT-17.2** With `_addmin` on the world the minimum light is added to each sample
  instead, as is a local minimum light's.
- **LIGHT-17.3** A light with `delay` 4 is a local minimum light: each sample it can see,
  whose light's mean channel is under its brightness, has each channel raised to its colour
  times its brightness over 255, in the lightmap of its style.
- **LIGHT-17.4** A brush model's face with any hidden sample has its hidden samples raised to
  the light at the model's middle, the middle of its box or the origin of the entity
  `_autominlight_target` names; with `_autominlight` 1 every sample is, and with 0 none.
  All its samples then count as seen (LIGHT-18.3).
- **LIGHT-17.5** The light at a point is, for each of up to four styles, what each light and
  sun and bounce gives it with nothing in the way, a light on the default channel only, each
  counted by `1 + (c - 1) * anglescale`, `c` the largest part of the direction to it along an
  axis, and a bounce whole; scaled as a sample's is (LIGHT-18.1). A point in the world's
  solid is moved as LIGHT-10.7 moves a light; when it cannot be, the samples are left.

## LIGHT-18 Writing lightmaps in the extended dialect

**Source:** ericw-tools `light/write.cc`, `LightFace_ScaleAndClamp`, `Lightsurf_HasColor`,
`CalculateLightmapStyles`, `FloodFillTransparent`, `IntegerDownsampleImage`,
`WriteSingleLightmap`, `SaveLightmapSurfaces`; `light/light.cc`, `SetLitNeeded`.

### Behaviour

- **LIGHT-18.1** Each sample's colour has nothing under 0; is held to twice the face's
  `_maxlight`, its model's or its flag's, else the world's, the whole colour scaled down;
  with `_lightcolorscale` under 1 is faded toward the grey of its brightest channel; is
  scaled by `_range`; and with `_gamma` `g` other than 1 each channel is
  `(c / 255) ^ (1 / g) * 255`.
- **LIGHT-18.2** A face keeps the lightmaps of the styles a light reached that have a sample
  whose mean channel is 1 or more, at most four, the brightest on average first; a face
  with none is unlit (LIGHT-5.1).
- **LIGHT-18.3** A hidden sample takes the mean of the seen samples among the eight about
  it, row by row, a sample filled counting as seen at once, until none is hidden or all are.
- **LIGHT-18.4** In the full mode a luxel is the mean of those of its four samples that were
  seen, or of all four when none was.
- **LIGHT-18.5** A luxel whose brightest channel is over 255 is scaled down to it. Each
  channel is cut to a byte; the grey byte is the brightest channel (LIGHT-6.4).
- **LIGHT-18.6** A `.lit` is written when any sample of any lightmap, kept or not, has two
  channels more than 0.001 apart once scaled.

## LIGHT-19 Dirt

**Source:** ericw-tools `light/ltface.cc`, `SetupDirt`, `LightFace_CalculateDirt`,
`Dirt_GetScaleFactor`; `include/common/qvec.hh`, `MakeTangentAndBitangentUnnormalized`.

### Behaviour

- **LIGHT-19.1** Dirt is worked out when the world's `_dirt` or `_minlight_dirt` is on, a
  sun's or the dome's dirt is over 0, or any light's `_dirt` is over 0.
- **LIGHT-19.2** Each sample casts 48 rays over a dome about its normal: 16 directions round
  it, at each of three angles from it, the middles of three equal parts of `_dirtangle`. Each
  goes `_dirtdepth` units and counts how far it got before meeting anything but sky.
- **LIGHT-19.3** A sample's occlusion is one less the mean of those distances over the
  depth. A contribution is scaled by `1 - min(1, min(1, occlusion ^ gain) * scale)`, the
  world's `_dirtgain` and `_dirtscale` or a light's own; a light's only when dirt applies to
  it (LIGHT-9.5), and between its `_dirt_off_radius` and `_dirt_on_radius`, when both are
  given, by how far along from the first to the second the sample is, nothing nearer. A face
  with the flag `no_dirt`, a model with `_dirt` -1, and a liquid face take none. Sunlight
  takes it by LIGHT-14.6, the minimum light with `_minlight_dirt`, and bounced light always.

## LIGHT-20 Bounce

**Source:** ericw-tools `light/bounce.cc`, `Face_ShouldBounce`, `MakeBounceLightsThread`,
`MakeBounceLight`, `MakeBounceLights`; `light/ltface.cc`, `LightFace_SurfaceLight`,
`GetSurfaceLighting`, `SurfaceLight_ColorAtDist`, `SurfaceLight_SphereCull`,
`LightPoint_SurfaceLight`; `light/light.cc`, `CacheTextures`; `common/imglib.cc`,
`calculate_average`; `include/common/polylib.hh`, `winding_base_t::area`, `center`, `plane`,
`remove_colinear`.

### Behaviour

- **LIGHT-20.1** `_bounce` is how many times light bounces. A face bounces light when it is
  a solid shadow caster (LIGHT-11.2), has a lightmap, an area of at least 1, and not the flag
  `no_bounce`.
- **LIGHT-20.2** On each bounce such a face gives back the light its samples took since the
  last, summed and divided by its samples, times its texture's colour (LIGHT-20.6): a light a
  unit above the middle of its corners, those on a straight edge left out, shining along its
  normal. Its colour is that light's, scaled so its brightest channel is at most 1; its
  brightness is that brightest channel times the face's area. A bounce on which no face gives
  any ends the bounces.
- **LIGHT-20.3** Only the light of style 0 bounces, unless `_bouncestyled` is on, when each
  style's bounces into its own lightmap.
- **LIGHT-20.4** A bounce light gives a sample its colour times its brightness over the
  square of `_dist` times the distance, which is taken as at least 128, times half of
  `_bouncescale`, the cosine at the light between its normal and the way to the sample, and
  the cosine at the sample between its normal and the way back; nothing when either is under
  -0.01. A sample of a face lit from both sides takes the cosine at it as 1.
- **LIGHT-20.5** A bounce light is passed over for a face when what it sees misses the face's
  box (LIGHT-10.8), or when its whole light at the far side of the sphere around the face is
  at most 0.01 on every channel; and for a sample, when what it gives is. The rest reach the
  sample when nothing stops the ray from the light to it. Bounced light takes dirt
  (LIGHT-19.3) and counts toward the next bounce.
- **LIGHT-20.6** A texture's colour is grey, 127 over 255, turned toward its own mean colour
  by `_bouncecolorscale`: the mean over its texels that are not see-through of their
  palette colours, each channel cut to a whole number, and black for a texture with no
  texels. With no palette given a texture's own colour is not known and it bounces grey. A
  face with no texture name bounces a half.
