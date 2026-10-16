# .map files

Prefix: `MAP` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the `.map` file a level editor saves and a BSP compiler reads: its tokens, its
entities and key-value pairs, its brushes in the standard (QuakeEd) and Valve 220 forms, and
the conventions of the world entity. Implemented by `crates/qs-map`
([#143](https://github.com/quakecstudio/editor/issues/143), P5-02); every statement in this
file is in [`coverage-baseline.tsv`](coverage-baseline.tsv) until that crate cites it.

id's engine never reads a `.map`; its format is what id's map compiler, `qbsp`, accepts.
`qbsp` is in `qutils/` of id-Software/Quake-Tools, which is not a portable upstream until the
license review rules on it ([`../porting.md`](../porting.md#upstreams), P4-22). It may be
read and run as an oracle, so the standard form is described from it, and the Valve 220 form,
which id's tools do not read, from ericw-tools. Both are cited for description only and no
code is ported from either:

| Short name | Repository and commit | Files read |
| --- | --- | --- |
| qbsp | [id-Software/Quake-Tools](https://github.com/id-Software/Quake-Tools)@`c0d1b91c74eb654365ac7755bc837e497caaca73` | `qutils/QBSP/MAP.C`, `GetToken`, `ParseEpair`, `ParseBrush`, `TextureAxisFromPlane`, `FindMiptex`, `FindTexinfo`, `ParseEntity`, `LoadMapFile`, `GetVectorForKey`, `WriteEntitiesToString`; `qutils/QBSP/QBSP.C`, `ProcessEntity`, `UpdateEntLump`; `qutils/QBSP/WRITEBSP.C`, `WriteMiptex`; `qutils/QBSP/BSP5.H`, `ON_EPSILON` |
| ericw-tools | [ericwa/ericw-tools](https://github.com/ericwa/ericw-tools)@`36eec1da2a194467e6baac2f444c2dd04b57d266` (GPL-2.0-or-later) | `common/parser.cc`, `parser_t::parse_token`; `common/mapfile.cc`, `brush_side_t::parse_plane_def`, `parse_texture_def`, `parse_valve_220`, `parse_quake_ed`, `set_texinfo`, `brush_t::parse_brush_face`, `map_entity_t::parse`, `parse_brush`, `parse_entity_dict`; `common/entdata.cc`, `entdict_t::set`; `qbsp/map.cc`, the texture loading that reads `_wad` and `wad` |

The engine's side of the same entities, once `qbsp` has written them to the BSP's entity
lump, is EDICT-8 and BSP-2.

## MAP-1 Tokens

**Source:** qbsp `qutils/QBSP/MAP.C`, `GetToken`, `StartTokenParsing`; ericw-tools
`common/parser.cc`, `parser_t::parse_token`.

A `.map` is text read one token at a time. Some tokens must be on the same line as the token
before them; `GetToken` is called with `crossline` false for those and fails when a line
ends first.

### Behaviour

- **MAP-1.1** A file is read up to its first NUL byte or its end, whichever comes first.
- **MAP-1.2** Tokens are separated by whitespace: every byte from 0x01 to 0x20, and every
  byte from 0x80 to 0xFF. qbsp compares a `char`, signed on x86, with 32, so the high bytes
  count as whitespace there; this spec follows the x86 build. A CR before a LF is therefore
  whitespace, and lines are counted by LF.
- **MAP-1.3** `//` starts a comment that runs to the end of the line. Comments separate
  tokens and may stand on their own lines or follow the last token of a line.
- **MAP-1.4** A `"` starts a quoted token that runs to the next `"`; the quotes are not part
  of it. It may be empty and may hold any byte but `"` and NUL, including a LF. A backslash
  is an ordinary byte: qbsp keeps `\n` as the two bytes `\` and `n`, and the engine decodes it
  when it spawns the entity (EXAMPLE-1). A file that ends inside a quoted token fails with the
  error `EOF inside quoted token`.
- **MAP-1.5** Any other token is a run of bytes up to the next whitespace. `(`, `)`, `{`, `}`
  and `"` do not end it, so `(0` is one token.
- **MAP-1.6** A token holds at most 127 bytes; a longer one fails with the error `Token too
  large on line <n>`.
- **MAP-1.7** Where a token must be on the current line (MAP-2.3, MAP-3.2, MAP-4.1), a LF, a
  comment or the end of the file before it fails with the error `Line <n> is incomplete`.
- **MAP-1.8** ericw-tools tokenises the same way except that it also takes `;` as the start
  of a comment, and inside a quoted token keeps a backslash and the byte after it as they
  are, so `\"` does not end the token unless a CR or LF follows it; it warns about escapes
  other than `\n`, `\'`, `\r`, `\t`, `\\`, `\b`, `\x` and `\0` to `\9`. A file that qbsp
  reads gives the same tokens in ericw-tools unless it holds `;` outside quotes or `\"`
  inside them.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MAP_MAX_TOKEN` | 128 | qbsp `MAP.C`, `MAXTOKEN`; a token holds `MAXTOKEN - 1` bytes |

### Test vectors

See [`vectors/map/`](vectors/map/README.md): `crlf-highbyte` (MAP-1.2), `eof-in-quote`
and `escape` (MAP-1.4), `token-127` and `token-128` (MAP-1.6), `comment-mid-line` (MAP-1.3,
MAP-1.7).

## MAP-2 Entities and key-value pairs

**Source:** qbsp `qutils/QBSP/MAP.C`, `ParseEntity`, `ParseEpair`, `LoadMapFile`,
`ValueForKey`, `GetVectorForKey`, `WriteEntitiesToString`; ericw-tools `common/mapfile.cc`,
`map_entity_t::parse`, `parse_entity_dict`; `common/entdata.cc`, `entdict_t::set`.

A `.map` is a list of entities. Each is a `{`, its key-value pairs and brushes in any order,
and a `}`:

```
{
"classname" "light"
"origin" "0 0 8"
}
```

### Behaviour

- **MAP-2.1** A file is zero or more entities. Each starts with the token `{`; any other
  token where an entity should start fails with the error `ParseEntity: { not found`. The
  end of the file between entities ends the list.
- **MAP-2.2** Inside an entity, the token `}` ends it, `{` starts a brush (MAP-3), and any
  other token is a key followed by its value. The end of the file inside an entity fails
  with the error `ParseEntity: EOF without closing brace`.
- **MAP-2.3** A value must be on the same line as its key (MAP-1.7). Keys and values are
  tokens, normally quoted; an unquoted word is accepted as either.
- **MAP-2.4** A key of 31 bytes or more, or a value of 1023 bytes or more, fails with the
  error `ParseEpar: token too long`. A value is in practice limited to 127 bytes by MAP-1.6.
- **MAP-2.5** An entity may have the same key more than once. qbsp keeps every pair and
  looks a key up by its last occurrence; ericw-tools keeps one pair per key, at the position
  of the first occurrence, holding the last value.
- **MAP-2.6** qbsp writes the entity lump with each entity's pairs in the reverse of their
  order in the `.map`, one `"key" "value"` per line, between `{` and `}` lines, and skips an
  entity with no pairs. Since the engine applies pairs in lump order with a later key
  overwriting an earlier one (EDICT-8.2), the game sees the first occurrence of a duplicated
  key when the map is compiled by qbsp.
- **MAP-2.7** The `origin` key, when present, is read as three numbers separated by
  whitespace, each as C `sscanf`'s `%lf`; missing or unreadable components are 0.
- **MAP-2.8** A map holds at most 1024 entities; the 1025th fails with the error
  `num_entities == MAX_MAP_ENTITIES`.
- **MAP-2.9** ericw-tools also removes whitespace from both ends of each key.
- **MAP-2.10** qbsp formats each pair as `"key" "value"` and a LF into a 128-byte buffer when
  it writes the entity lump (`WriteEntitiesToString`, `char line[128]`), so a pair whose key
  and value together exceed 121 bytes overflows it, which is undefined (a build with stack
  checks stops there). This spec fixes the answer: such a pair is reported as one id's qbsp
  cannot compile. ericw-tools has no such limit.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MAP_MAX_KEY` | 32 | `WinQuake/bspfile.h`, `MAX_KEY`; a key holds at most `MAX_KEY - 2` bytes in qbsp's `ParseEpair` |
| `MAP_MAX_VALUE` | 1024 | `WinQuake/bspfile.h`, `MAX_VALUE` |
| `MAP_MAX_ENTITIES` | 1024 | `WinQuake/bspfile.h`, `MAX_MAP_ENTITIES` |

### Test vectors

See [`vectors/map/`](vectors/map/README.md): `standard` (MAP-2.1 to MAP-2.7),
`no-open-brace`, `eof-in-entity`, `value-on-next-line`, `key-30` and `key-31` (MAP-2.1 to
MAP-2.4), `token-127` (MAP-2.10).

## MAP-3 Brushes and planes

**Source:** qbsp `qutils/QBSP/MAP.C`, `ParseBrush`; `qutils/QBSP/BSP5.H`, `ON_EPSILON`;
ericw-tools `common/mapfile.cc`, `brush_side_t::parse_plane_def`, `brush_t::parse_brush_face`,
`map_entity_t::parse_brush`.

A brush is a convex solid: the intersection of the back half-spaces of its faces' planes.
Each face is one line: three points and a texture definition.

```
{
( -64 -64 -16 ) ( -64 -63 -16 ) ( -64 -64 -15 ) wall 0 0 0 1 1
...
}
```

### Behaviour

- **MAP-3.1** A brush is `{`, then faces, then `}`. Each face starts with `(` (any other
  token fails with the error `parsing brush`) and the end of the file ends the brush.
- **MAP-3.2** A face is three points, each `(`, three numbers and `)`. The three numbers and
  the `)` must be on the line of their `(`, but each point's `(` may start a new line. The
  texture definition follows on the line of the third point's `)` (MAP-4, MAP-5).
- **MAP-3.3** qbsp reads each point coordinate with C `atoi`, so `0.5` is 0 and a token
  that does not start with a number is 0; ericw-tools reads them as doubles.
- **MAP-3.4** The face's plane has the normal `normalize((p0 - p1) x (p2 - p1))` and the
  distance `dot(p1, normal)`. The brush is the region on the back of every face's plane, so
  each normal points out of the brush.
- **MAP-3.5** A face whose three points all lie within 0.05 units of an earlier face's plane
  is a duplicate: qbsp prints `WARNING: brush with duplicate plane` and drops it. The test
  ignores the plane's orientation, so a face on the same plane facing the other way is also
  dropped. ericw-tools drops it too.
- **MAP-3.6** A face whose points are collinear has no normal. qbsp prints `WARNING: brush
  plane with no normal` and stops reading the brush there, so the rest of the brush is then
  read as entity keys, which normally fails; ericw-tools warns and drops only that face.
  This spec's reader keeps the face and reports it.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MAP_ON_EPSILON` | 0.05 | qbsp `BSP5.H`, `ON_EPSILON` |
| `MAP_MAX_BRUSHES` | 4096 | `WinQuake/bspfile.h`, `MAX_MAP_BRUSHES`; qbsp's `mapbrushes` array, which `ParseBrush` fills without a check |

### Test vectors

See [`vectors/map/`](vectors/map/README.md): `standard` (MAP-3.1 to MAP-3.5),
`point-split` (MAP-3.2), `float-points` (MAP-3.3), `no-normal` (MAP-3.6).

## MAP-4 Standard texture definitions

**Source:** qbsp `qutils/QBSP/MAP.C`, `ParseBrush`, `TextureAxisFromPlane`, `baseaxis`,
`FindMiptex`, `FindTexinfo`; ericw-tools `common/mapfile.cc`, `brush_side_t::parse_quake_ed`,
`set_texinfo`.

The standard form, from id's QuakeEd, gives a texture name, a shift, a rotation and a scale;
the compiler derives the texture axes from the plane.

| Token | Field | qbsp reads it with |
| --- | --- | --- |
| 1 | texture name | as is |
| 2, 3 | `shift[0]`, `shift[1]` | `atoi` |
| 4 | `rotate`, in degrees | `atoi` |
| 5, 6 | `scale[0]`, `scale[1]` | `atof` |

### Behaviour

- **MAP-4.1** After the points come six tokens on the same line: the texture name, the s and
  t shift, the rotation and the s and t scale. qbsp reads the shifts and rotation as integers
  (`atoi`) and the scales as floats; ericw-tools reads all five as doubles.
- **MAP-4.2** The base axes come from the plane's normal: the first of the six rows below
  whose axis has the largest dot product with the normal, compared with `>` so the earliest
  wins a tie (a normal with no positive dot product takes the floor row):

  | Row | Axis | s axis | t axis |
  | --- | --- | --- | --- |
  | floor | (0, 0, 1) | (1, 0, 0) | (0, -1, 0) |
  | ceiling | (0, 0, -1) | (1, 0, 0) | (0, -1, 0) |
  | west wall | (1, 0, 0) | (0, 1, 0) | (0, 0, -1) |
  | east wall | (-1, 0, 0) | (0, 1, 0) | (0, 0, -1) |
  | south wall | (0, 1, 0) | (1, 0, 0) | (0, 0, -1) |
  | north wall | (0, -1, 0) | (1, 0, 0) | (0, 0, -1) |

- **MAP-4.3** The axes are rotated by `rotate` degrees in the plane of the two non-zero axis
  components: with `sv` the index of the s axis's non-zero component and `tv` that of the t
  axis's, each axis `a` becomes `a[sv] = cos * a[sv] - sin * a[tv]` and
  `a[tv] = sin * a[sv] + cos * a[tv]` (using the old values). For 0, 90, 180 and 270 the sine
  and cosine are exactly 0 or 1 or -1; otherwise they are `sin` and `cos` of
  `rotate / 180 * pi`.
- **MAP-4.4** A scale of 0 is taken as 1. The texinfo's axes (BSP-8.1) are the rotated axes
  divided by their scale, and its offsets are the two shifts, unscaled.
- **MAP-4.5** qbsp sets `TEX_SPECIAL` (BSP-8.5) on the texinfo when the texture name starts
  with `*` or with `sky` in any case.
- **MAP-4.6** The texture name names a texture in the WAD2 files of MAP-6.3. qbsp copies it
  into a 16-byte slot, so a name of 16 bytes or more overflows it, which is undefined; this
  spec fixes the answer: a name holds at most 15 bytes, as a WAD2 lump name (WAD-2).
- **MAP-4.7** qbsp gives each distinct texture name a slot in first-use order, and each
  distinct texinfo (texture, flags and the eight axis values compared exactly) one texinfo
  in first-use order. A map uses at most 4096 of each; the next fails with the error
  `nummiptex == MAX_MAP_TEXINFO` or `numtexinfo == MAX_MAP_TEXINFO`.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MAP_MAX_TEXINFO` | 4096 | `WinQuake/bspfile.h`, `MAX_MAP_TEXINFO`; qbsp `FindMiptex`, `FindTexinfo` |
| `MAP_MAX_TEXTURE_NAME` | 15 | qbsp `MAP.C`, `miptex[MAX_MAP_TEXINFO][16]` less the NUL |

### Test vectors

See [`vectors/map/`](vectors/map/README.md): `standard` gives each face's texinfo
(MAP-4.1 to MAP-4.5, MAP-4.7); `float-points` (MAP-4.1).

## MAP-5 Valve 220 texture definitions

**Source:** ericw-tools `common/mapfile.cc`, `brush_side_t::parse_texture_def`,
`parse_valve_220`, `set_texinfo(const texdef_valve_t &)`; qbsp `qutils/QBSP/MAP.C`,
`ParseBrush`.

The Valve 220 form, from Valve's Hammer editor, stores the texture axes themselves, so a
texture keeps its alignment when the face is moved or rotated.

```
( -64 -64 -16 ) ( -63 -64 -16 ) ( -64 -63 -16 ) wall [ 1 0 0 16 ] [ 0 -1 0 -8 ] 0 0.5 2
```

### Behaviour

- **MAP-5.1** A Valve 220 face has, after the texture name, `[`, the s axis's x, y and z,
  the s shift, `]`, then `[`, the t axis and t shift, `]`, then the rotation and the s and t
  scale, all on the line of the third point; numbers are read as doubles. Any other token
  where a bracket belongs fails (`couldn't parse Valve220 texture info`).
- **MAP-5.2** ericw-tools decides the form face by face: a face whose token after the
  texture name is `[` is Valve 220, any other is standard. The worldspawn key `mapversion`
  with the value `220`, which editors write for this form, is not read.
- **MAP-5.3** The texinfo's axes are the given axes divided by their scale, and its offsets
  are the given shifts, unscaled. The rotation is not applied: it is kept for editors.
- **MAP-5.4** ericw-tools divides by a Valve 220 scale of 0, which gives an infinite or NaN
  axis; this spec fixes the answer: a scale of 0 in a Valve 220 face is an error.
- **MAP-5.5** id's qbsp does not read the Valve 220 form: it takes `[` as the first shift and
  then fails with the error `parsing brush`.

### Test vectors

See [`vectors/map/`](vectors/map/README.md): `valve220` (MAP-5.1 to MAP-5.3, MAP-5.5).

## MAP-6 The world and brush entities

**Source:** qbsp `qutils/QBSP/QBSP.C`, `ProcessEntity`, `UpdateEntLump`;
`qutils/QBSP/WRITEBSP.C`, `WriteMiptex`; ericw-tools `qbsp/map.cc` (texture loading);
`WinQuake/sv_main.c`, `SV_SpawnServer`, `SV_SendServerinfo`; `WinQuake/cl_parse.c`,
`CL_ParseServerInfo`, `svc_cdtrack` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

### Behaviour

- **MAP-6.1** The first entity is the world. Its `classname` is `worldspawn` by convention;
  neither qbsp nor the engine checks it: qbsp builds the first entity's brushes into model 0
  whatever its keys, and the engine spawns the first entity into edict 0 and calls the
  function its `classname` names (EDICT-4.2, EDICT-8.6).
- **MAP-6.2** Every later entity with at least one brush is a brush entity. qbsp numbers
  them in file order from 1 and sets each one's `model` key to `*<n>` (BSP-15.3), replacing
  any value the `.map` gave. An entity with no brushes keeps its keys as written.
- **MAP-6.3** qbsp reads its textures from the world's `_wad` key or, when that is missing or
  empty, its `wad` key: one path, relative to the game directory. With neither it prints
  `WARNING: no wadfile specified` and writes an empty textures lump. ericw-tools reads the
  same keys as a list of paths separated by `;`.
- **MAP-6.4** A texture qbsp cannot find in the WAD gets the slot offset -1 (BSP-4.2) with
  `WARNING: texture <name> not found`. For each texture whose name starts with `+`, qbsp also
  adds every other frame of its animation (`+0` to `+9`, `+A` to `+J`) that the WAD holds
  (BSP-4.8).
- **MAP-6.5** The world's `message` is the level's title: the server sends it to each client
  when it connects. id's server passes it to `sprintf` as the format string (`sprintf
  (message, pr_strings+sv.edicts->v.message)`), so a `%` in it is undefined; this spec fixes
  the answer as a map diagnostic: a `message` containing `%` is reported.
- **MAP-6.6** The world's `sounds` is the CD track the client plays, sent as one byte for both
  the track and the loop track (`MSG_WriteByte` takes the float converted to an `int` and
  stores its low byte). A value outside 0 to 255 is not portable; a map check reports it.
- **MAP-6.7** Every other key of the world, and every key of other entities, is the game's:
  the engine applies it to a field of the QuakeC entity by name (EDICT-8.2), and a key that
  starts with `_` is ignored by the engine and left for the tools.

### Test vectors

See [`vectors/map/`](vectors/map/README.md): `standard` (MAP-6.1, MAP-6.2).

## MAP-7 Text both compilers read

**Source:** MAP-1 to MAP-6.

A `.map` written for id's qbsp also suits ericw-tools; the reverse is not true. These rules
describe the text that both read the same way, which is what a writer emits by default.

### Behaviour

- **MAP-7.1** Every key and value is quoted and holds no `"`, no NUL, no LF and no `\"`; a key
  holds at most 30 bytes, and a key and its value together at most 121 (MAP-2.10).
- **MAP-7.2** Each pair, each face and each `{` and `}` is on a line of its own, tokens are
  separated by single spaces, and lines end in LF.
- **MAP-7.3** Brushes use the standard form with integer point coordinates, integer shifts
  and an integer rotation; a texture name holds at most 15 bytes and no whitespace.
- **MAP-7.4** A file with a Valve 220 face, a fractional coordinate, shift or rotation, a `;`
  outside quotes or a `\"` inside them is read by ericw-tools but not as intended by id's
  qbsp, and is written only when the map already uses that feature.
