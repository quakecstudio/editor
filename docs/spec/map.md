# .map files

Prefix: `MAP` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the `.map` file a level editor saves and a BSP compiler reads: its tokens, its
entities and key-value pairs, its brushes in the standard (QuakeEd) and Valve 220 forms, and
the conventions of the world entity. Implemented by `crates/qs-map`
([#143](https://github.com/quakecstudio/editor/issues/143), P5-02), whose tests cite
every statement.

id's engine never reads a `.map`; its format is what id's map compiler, `qbsp`, accepts.
`qbsp` is in `qutils/` of id-Software/Quake-Tools, which is not a portable upstream until the
license review rules on it ([`../porting.md`](../porting.md#upstreams), P4-22). It may be
read and run as an oracle, so the standard form is described from it, and the Valve 220 form,
which id's tools do not read, from ericw-tools. Both are cited for description only and no
code is ported from either:

| Short name | Repository and commit | Files read |
| --- | --- | --- |
| qbsp | [id-Software/Quake-Tools](https://github.com/id-Software/Quake-Tools)@`c0d1b91c74eb654365ac7755bc837e497caaca73` | `qutils/QBSP/MAP.C`, `GetToken`, `ParseEpair`, `ParseBrush`, `TextureAxisFromPlane`, `FindMiptex`, `FindTexinfo`, `ParseEntity`, `LoadMapFile`, `GetVectorForKey`, `WriteEntitiesToString`; `qutils/QBSP/QBSP.C`, `ProcessEntity`, `UpdateEntLump`; `qutils/QBSP/WRITEBSP.C`, `WriteMiptex`, `AddAnimatingTextures`, `LoadLump`, `TEX_InitFromWad`, `CleanupName`; `qutils/QBSP/BSP5.H`, `ON_EPSILON` |
| ericw-tools | [ericwa/ericw-tools](https://github.com/ericwa/ericw-tools)@`36eec1da2a194467e6baac2f444c2dd04b57d266` (GPL-2.0-or-later) | `common/parser.cc`, `parser_t::parse_token`; `common/mapfile.cc`, `brush_side_t::parse_plane_def`, `parse_texture_def`, `parse_valve_220`, `parse_quake_ed`, `set_texinfo`, `brush_t::parse_brush_face`, `map_entity_t::parse`, `parse_brush`, `parse_entity_dict`; `common/entdata.cc`, `entdict_t::set`; `qbsp/map.cc`, the texture loading that reads `_wad` and `wad` |
| TrenchBroom | [TrenchBroom/TrenchBroom](https://github.com/TrenchBroom/TrenchBroom)@`8f329f11e65e3d21b355a42bcae199cb3e4e0812` (GPL-3.0-or-later) | `lib/TbMdlLib/include/mdl/EntityProperties.h`, the `_tb_` keys and `func_group`; `lib/TbMdlLib/src/EntityProperties.cpp`, `isLayer`, `isGroup`; `lib/TbMdlLib/src/MapReader.cpp`, `extractContainerInfo` and the layer and group readers |

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
- **MAP-1.9** The line number in qbsp's errors (`Line <n> is incomplete`, `Token too large
  on line <n>`) counts only the LFs `GetToken` skips between tokens (`if (*script_p++ ==
  '\n') ... scriptline++;`). The LF that ends a `//` comment is consumed with the comment
  (`while (*script_p++ != '\n')`) and a LF inside a quoted token is copied into it, and
  neither is counted, so after a comment qbsp's line numbers fall behind the file's.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MAP_MAX_TOKEN` | 128 | qbsp `MAP.C`, `MAXTOKEN`; a token holds `MAXTOKEN - 1` bytes |

### Test vectors

See [`vectors/map/`](vectors/map/README.md): `crlf-highbyte` (MAP-1.2), `eof-in-quote`
and `escape` (MAP-1.4), `token-127` and `token-128` (MAP-1.6), `comment-mid-line` (MAP-1.3,
MAP-1.7). The line numbers of MAP-1.9 are tested in `crates/qs-map`.

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
`qutils/QBSP/WRITEBSP.C`, `WriteMiptex`, `AddAnimatingTextures`, `LoadLump`,
`TEX_InitFromWad`, `CleanupName`; ericw-tools `qbsp/map.cc` (texture loading);
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
- **MAP-6.4** **Withdrawn** 2026-10-16: it said qbsp adds every frame of a `+` animation
  that the WAD holds, but `AddAnimatingTextures` compares the name as the map spells it with
  the WAD's upper-cased names; replaced by MAP-6.8.
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
- **MAP-6.8** qbsp's textures lump has a slot for each texture name the map uses (MAP-4.7)
  and then, from `AddAnimatingTextures`, for each name that starts with `+`, each frame `+0`
  to `+9` and `+A` to `+J` of its animation (the name with its second byte replaced) that
  equals a lump name of the WAD byte for byte. `TEX_InitFromWad` upper-cases the WAD's lump
  names (`CleanupName`), so a frame is added only when the rest of the map's name is upper
  case: `+0LAVA` finds a lump `+1lava`, `+0lava` finds none. `LoadLump` upper-cases the name
  it looks for, so each slot's lookup ignores case; a texture the WAD lacks gets the slot
  offset -1 (BSP-4.2) and `WARNING: texture <name> not found`.

### Test vectors

See [`vectors/map/`](vectors/map/README.md): `standard` (MAP-6.1, MAP-6.2). The lump names of
MAP-6.8 were read from `qutils/QBSP/WRITEBSP.C`, `CleanupName`, `TEX_InitFromWad`,
`LoadLump`, `AddAnimatingTextures` and `WriteMiptex`, and are tested in `crates/qs-map`.

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

## MAP-8 Entities against the progs

**Source:** EDICT-8.2 and EDICT-8.5, which say what id's engine does with each entity and
key when the map loads (`WinQuake/pr_edict.c`, `ED_ParseEdict`, `ED_ParseEpair`,
`ED_LoadFromFile`); the progs is the project's server QuakeC as `qc-sema` reads it.

The engine reads a map's entities against the progs, not against a list of classes: a key
works when it names a field, and an entity spawns when its `classname` names a function. So
the editor checks a map the same way, and says what the engine will do. These are the
editor's diagnostics; none of them stops the engine but MAP-8.4's function and field values.

### Behaviour

- **MAP-8.1** An entity with no `classname`, or whose `classname` names no function of the
  progs, is reported: the engine frees it (EDICT-8.5). The function of the closest name, at
  most two edits away, is suggested.
- **MAP-8.2** A key the engine looks up as no field of the progs is reported, with the field
  of the closest name, at most two edits away, as the fix: the engine ignores it with
  `'<key>' is not a field` (EDICT-8.2). The key is looked up as the engine does: trailing
  spaces removed, `angle` as `angles` and `light` as `light_lev`; a key starting with `_` is
  never reported. Among fields at the same distance, one the entity's class reads comes
  first. `wad` and `mapversion` are not reported either: qbsp and the map editors write them
  for the compilers (MAP-6.3).
- **MAP-8.3** A value its field's type cannot read is reported: a float that is not a number,
  a vector that is not three numbers separated by spaces (EDICT-8.3), and an entity that is
  not a whole number. The engine reads them with `atof` and `atoi` all the same, as the
  number their text starts with, or 0.
- **MAP-8.4** A function field's value that names no function, and a field field's value that
  names no field, are reported: the engine stops loading the map with `ED_ParseEdict: parse
  error` (EDICT-8.2).
- **MAP-8.5** **Withdrawn** 2026-11-12: it took a key starting `target` (but `targetname`), or
  `killtarget`, as one naming entities to fire; P5-13 reads those keys from the code; replaced
  by MAP-8.8 and MAP-8.9.
- **MAP-8.6** **Withdrawn** 2026-11-12: it counted a `targetname` as fired by the keys MAP-8.5
  named; replaced by MAP-8.9.
- **MAP-8.7** Renaming a field in the code offers to rename the key in the open maps; once
  confirmed, every pair with the old key takes the new one, its value and formatting kept.
- **MAP-8.8** A key names entities to fire when the code compares its field with `targetname`:
  passes it to `find` as the value to match, with `targetname` the field to match it against,
  as id's `SUB_UseTargets` does (`t = find (t, targetname, self.target);`), whatever entity it
  is loaded from; or compares it with a `targetname` by `==` or `!=`. In id 1.01's and the
  rerelease's code these keys are `target` and `killtarget`; a mod's own keys count once its
  code compares them. A comparison of `targetname` with anything but a field names no key.
- **MAP-8.9** A key of MAP-8.8 whose value no entity's `targetname` holds is reported: what it
  fires does not exist. A `targetname` that no such key names is reported: nothing in the map
  fires it.

## MAP-9 Rooms: TrenchBroom's layers and groups

**Source:** TrenchBroom `lib/TbMdlLib/include/mdl/EntityProperties.h`,
`lib/TbMdlLib/src/EntityProperties.cpp` (`isLayer`, `isGroup`) and
`lib/TbMdlLib/src/MapReader.cpp` (`extractContainerInfo`, the layer and group readers), for
description only; nothing is ported.

A mapper names parts of a map with TrenchBroom's layers and groups, which TrenchBroom saves as
`func_group` entities. The editor and the agent call each one a room, so a prompt can name
one ("the armory") and the map tools can find it (MAP-16).

### Behaviour

- **MAP-9.1** A layer is an entity whose `classname` is `func_group`
  (`LayerClassname = "func_group"`) and whose `_tb_type` is `_tb_layer`; a group is one whose
  `_tb_type` is `_tb_group` (`isLayer`, `isGroup`). Its name is its `_tb_name` and its id its
  `_tb_id`; TrenchBroom skips one whose name or id is blank (`Skipping layer entity: missing
  name`, `missing id`) or whose id is not a whole number above 0 (`!rawId || *rawId <= 0u`),
  and it is no room here either. The world entity is no room.
- **MAP-9.2** An entity is in the layer its `_tb_layer` names by id, else in the group its
  `_tb_group` names (`extractContainerInfo` reads `_tb_layer` first); a value that is blank or
  not a whole number from 0 (`rawParentLayerId && *rawParentLayerId >= 0`) names none.
- **MAP-9.3** A room's bounds are the box around every brush polygon (MAP-3.4) of the room's own
  entity, of the brush entities in it and of the rooms in it, however deep; a room with no
  brush has none. A container with no name passes its members and brushes to the nearest named
  room around it. Not TrenchBroom's: it keeps no bounds.
- **MAP-9.4** A room's own entity is in the room; another entity is in the nearest named room
  at or around the container it names (MAP-9.2). An entity that names none, or one that leads
  to no room, is in the smallest room whose bounds hold its `origin` (MAP-2.7), edges included,
  the one inside the other of two with the same box: an entity placed where a room is, with no
  TrenchBroom key, is in it. An entity in a room is
  in every room around that one too, so a filter or a count by a room's name takes the rooms
  inside it.
- **MAP-9.5** A group whose `_tb_linked_group_id` is not blank is linked to every other group
  with the same value (manual, Linked Groups: "All groups with this linked group ID will be
  mutually linked"). Its `_tb_transformation` is sixteen numbers, the matrix row by row
  (`NodeSerializer::groupProperties` writes `transformation[0][0], transformation[1][0],
  transformation[2][0], transformation[3][0], // row 0` and so on), so a translation is the
  fourth, eighth and twelfth numbers; it is written only when it is not the identity, and a
  group without it has the identity. TrenchBroom reads it only for a linked group
  (`if (!linkId.empty())` in `MapReader`), and one that is not sixteen numbers is a
  `MalformedTransformationIssue`; here such a group is read as the identity and is never
  updated (MAP-9.7).
- **MAP-9.6** A group's contents are its own entity's brushes, in file order, then the entities
  that name it in `_tb_group`, in file order (MAP-9.2). The contents of two linked groups
  correspond by position: TrenchBroom saves link ids only on groups and pairs the other objects
  of linked groups by their place among the group's children (`visitNodesPerPosition`).
- **MAP-9.7** When the contents of a linked group change, every other group linked to it gets a
  copy of its contents transformed by that group's transformation times the inverse of the
  changed group's (`updateLinkedGroups`: `targetGroupNode->group().transformation() *
  *invertedSourceTransformation`); a changed group whose transformation has no inverse updates
  none ("Group transformation is not invertible"). Copying moves an entity's `origin` and each
  brush's points, and points its `angle`, or the yaw of its `angles`, the way the
  transformation turns or mirrors that direction; a mirror also swaps the order of two of each
  face's points, so the plane keeps facing out. Each linked set an edit changed is copied from
  its changed group; an edit that changed two groups of one set differently is not copied, as
  copying either would undo the other. Here a linked group with a group inside it is not
  updated.
- **MAP-9.8** `_tb_protected_properties` lists keys separated by `;` (`kdl::str_split(...,
  ";")`). A copied entity keeps, for every key protected in it or in the entity it replaces,
  the value the replaced entity had, and none when it had none, and takes the replaced
  entity's list (`preserveEntityProperties`).
- **MAP-9.9** A layer's `_tb_layer_hidden`, `_tb_layer_locked` and
  `_tb_layer_omit_from_export` are set when their value is `1` (`LayerHiddenValue = "1"`,
  `LayerLockedValue = "1"`, `LayerOmitFromExportValue = "1"`) and are absent otherwise
  (`NodeSerializer` removes them). An exported map leaves out a layer marked so (manual:
  "Layers marked "Omit From Export" will not be present in the exported map"), with every
  entity in it: a layer's own entity, the entities naming it, and the entities of a group in
  it, however deep (MAP-9.2).

## MAP-10 Shapes

**Source:** our design, after the block tool of Valve's Hammer and J.A.C.K., whose shapes take
sides, a wall width, an arc and a start angle; the step a player climbs is `SV_WalkMove`'s
`STEPSIZE` (PHYS-8.3). Implemented by `qs_map::shapes`.

### Behaviour

- **MAP-10.1** A shape fills a box given by two corners, in any order. A box with no volume, or
  a corner that is not a number, is refused. Every brush of a shape is made from its faces'
  planes and is valid (MAP-3.4); a shape whose brush would not be is refused, never written.
- **MAP-10.2** A round shape of `n` sides lies around the ellipse the box's footprint holds. With
  corners on the ellipse, corner `i` is at angle `2πi/n` from +x; with edges on it, the radii are
  divided by `cos(π/n)` and corner `i` is at `π/n + 2πi/n`, so each edge touches the ellipse at
  its middle and a 4-sided shape is the box's footprint. Cylinders and cones take either; an
  arch's corners are always on its ellipse.
- **MAP-10.3** A round shape has 3 to 128 sides; others are refused.
- **MAP-10.4** A cylinder is one upright prism from the box's bottom to its top. A hollow one of
  wall `t` is one prism per side, each from the outer polygon's edge to the edge of the polygon
  of the same sides and mode on the ellipse whose radii are `t` less; neighbours share their
  sides, and a solid cylinder on the box shrunk by `t` on each side fits its hole exactly. A wall
  is above 0 and thinner than the smaller radius; others are refused.
- **MAP-10.5** A cone is the cylinder's base and a point at the middle of the box's top.
- **MAP-10.6** An arch lies in the box's x-z face and runs through it along y: its curve is the
  half ellipse of radii half the box's width and its height, centred at the middle of its bottom,
  from the start angle over the arc, cut in `n` segments, each a prism of wall `t`. The arc is
  above 0 and the start at least 0, and the arch ends by 180 degrees, so it stays in the box;
  others are refused. With spandrels filled, each segment below the box's top also has the prism
  between its outer edge and the top.
- **MAP-10.7** A UV spheroid is one brush whose faces are its bands of latitude, 2 to 64 of them,
  cut in `n` slices, with at most 1280 faces, slices times bands; an icospheroid is one brush of
  an icosahedron's twenty triangles, each cut in four up to 3 times (1280 faces), pushed out onto
  the box's ellipsoid.
- **MAP-10.8** A rock is the convex hull of 4 to 64 points spread over the box's ellipsoid by a
  spiral, each from 0.7 to 1 of the way out, as a 64-bit seed decides: one seed, one rock.
- **MAP-10.9** A scalable curve has 12, 24, 48 or 96 sides; others are refused. Its box is put on
  the grid, at least one grid step each way, and its corners are those of the ellipse the box
  holds, each rounded to the grid, so every corner lies on the grid at every size. A solid curve
  is one prism, the convex hull of its corners; a hollow one of wall `t` is one prism per side,
  the hull of that side's outer and inner corners, the inner ellipse's radii `t` less.
- **MAP-10.10** Stairs climb the box one way, each step a box from the floor to its top: as many
  steps as fit both the box's length at the step's depth and its height at the step's height,
  at least one and at most 256, the same for spiral stairs; others are refused. The default step is 16 high, under `STEPSIZE` (18), and 32 deep, the player box's
  width, so a player walks up them.
- **MAP-10.11** Spiral stairs are steps of the step's height, `n` to a turn around the box's
  middle, each the slice between the core's circle and the ellipse; with no core each slice
  reaches the middle.

## MAP-11 Moving brushes

**Source:** our design, after TrenchBroom's texture lock (its manual: "When texture lock is on,
the textures stay fixed in the world as you move or rotate the brushes"), for moves only.
Implemented by `qs_map::Brush::translated`.

### Behaviour

- **MAP-11.1** A brush moved by `d` has each face's three points moved by `d`; its texture and
  texture definition are kept but for MAP-11.2.
- **MAP-11.2** With texture lock on, each face's s and t shifts are each less the dot product of
  `d` with that face's texture axis as MAP-4.2 to MAP-4.4 and MAP-5.3 scale it, so every point of
  the moved face has the texture coordinate its point had before the move: the texture stays
  fixed in the world. With texture lock off the texture definitions are unchanged and the texture
  moves with the brush. Each face is rewritten in place, keeping the text before it. A brush
  with a Valve 220 face of scale 0, an error (MAP-5.4), cannot be written and is refused. id's
  qbsp reads a standard face's shifts as whole numbers (MAP-4.1), so there a locked shift that
  is not whole is cut to one; ericw-tools, which reads them as doubles, keeps it.

## MAP-12 Texture alignment

**Source:** our design, after the face inspector of TrenchBroom and the texture application
tool of Hammer and J.A.C.K., whose shift, rotation and scale fields set those of the selected
faces. Implemented by `qs_map::TexDef::aligned`.

### Behaviour

- **MAP-12.1** Aligning a face sets the parts of its texture definition given, of shift, rotation
  and scale, and keeps the others. In the standard form (MAP-4.1) each part replaces its field.
- **MAP-12.2** In the Valve 220 form (MAP-5.1) the shift is each axis's fourth value and the scale
  replaces the scale. A new rotation `r`, the kept rotation being `r0`, turns both axes by
  `r - r0` degrees about their normal, the unit `s x t`, from `s` toward `t`, keeping their
  lengths and the angle between them; axes with no normal, parallel or zero, are kept.

## MAP-13 UV editing

**Source:** our design, after TrenchBroom's UV editor (its manual: the face's texture can be
fitted, justified, flipped and reset, and "alignment transfer" copies one face's texture onto
another so it wraps around the corner between them) and Hammer's justify buttons. Implemented
by `qs_map::Face::uv_edited`, `qs_map::Face::aligned_from` and `qs_map::Brush::face_polygons`.

A face's texel along axis `i` at a point `p` is `dot(p, axis_i) / scale_i + shift_i`, where
`axis_i` is the rotated base axis of MAP-4.2 and MAP-4.3 in the standard form and the given
axis in the Valve 220 form, and a standard scale of 0 is 1 (MAP-4.4, MAP-5.3). The face's
extent along an axis is the least and greatest texel of its polygon's corners before the
shift.

### Behaviour

- **MAP-13.1** Each face of a brush has the polygon MAP-3.4 gives it on the brush's solid, in
  the order of the faces; a face with no plane (MAP-3.6), or nothing on the solid, has none. A
  face with no plane takes no edit and no transfer; a face with no polygon has no extent, so
  fitting, justifying and snapping leave it as it is.
- **MAP-13.2** Fitting a texture of width `w` and height `h` sets each axis's scale so that the
  face's extent is `w` along s and `h` along t, keeping the scale's sign, and its shift so that
  the extent starts on a texture edge. An axis along which the face has no extent, or for a
  texture of no size, is kept.
- **MAP-13.3** Justifying changes only the shift: left puts the least s on a texture edge,
  right the greatest s, top the least t and bottom the greatest t; centre puts the centre of
  the extent on the texture's centre along both axes.
- **MAP-13.4** Flipping an axis negates its scale and changes its shift so that the centre of
  the face's extent keeps its texel: the texture is mirrored about it.
- **MAP-13.5** Resetting gives a standard face no shift, no rotation and a scale of 1, and a
  Valve 220 face the paraxial axes of its plane (MAP-4.2) with no shift, rotation or scaling.
- **MAP-13.6** Snapping moves each axis's shift by the least amount that puts one of the face's
  corners on a texture edge; of two corners as near, the first in the polygon's order.
- **MAP-13.7** Fitting, justifying and snapping leave each shift they set in `[0, size)` for
  the texture's size along that axis, as whole copies of the texture are the same alignment.
- **MAP-13.8** Carrying a source face's alignment onto a target face keeps the target's form
  and gives every point of the line the two planes share the texel it has on the source. A
  Valve 220 target takes the source's axes turned about that line by the angle between the
  normals, with the source's rotation and scale. A standard target keeps the axes of its own
  plane (MAP-4.2) and takes the rotation and the scales, in the source's proportion, that
  make the texels change along the line as the source's do; a rotation within 0.000001 of a
  whole number of degrees is that number, so the exact sines of MAP-4.3 apply. A source whose
  axes both lie across the line, as a Valve 220 source's may, gives no rotation and its own
  scales.
- **MAP-13.9** Between parallel planes there is no shared line: a standard source's definition
  is copied onto a standard target, which projects it the same way (MAP-4.2 gives a plane and
  its opposite the same axes); a Valve 220 target takes the source's axes, shifts, rotation
  and scale; and a standard target of a Valve 220 source agrees with it along the line through
  the target plane's point nearest the origin along its paraxial s axis.
- **MAP-13.10** Every edit is written as MAP-7.3 writes numbers, so it is read back unchanged
  in either form. id's qbsp reads a standard face's shifts and rotation as whole numbers
  (MAP-4.1), so there a fraction is cut off; ericw-tools keeps it.
- **MAP-13.11** Adjusting adds a given amount to each shift and to the rotation and multiplies
  each scale by a given factor, as a drag in the UV view does; a standard scale of 0 counts as
  1 (MAP-4.4), and a Valve 220 face's new rotation turns its axes (MAP-12.2). A factor of 0,
  or one that is not finite, would leave no texture and is refused.

## MAP-14 External map instances

**Source:** ericw-tools `qbsp/map.cc` at `36eec1da2a194467e6baac2f444c2dd04b57d266`,
`ProcessExternalMapEntity`, `LoadExternalMap`, `ScaleMapFace`, `RotateMapFace`,
`TranslateMapFace`; `common/mathlib.cc`, `RotateAboutX`, `RotateAboutY`, `RotateAboutZ`;
`common/entdata.cc`, `entdict_t::get_vector`, `get_float`. Implemented by
`qs_map::MapFile::with_external_maps`.

ericw-tools' qbsp lets a point entity stand for another `.map`: before anything else is
compiled, the entity takes that map's brushes, placed by its keys. id's qbsp has no such
entity.

### Behaviour

- **MAP-14.1** An entity whose classname is `misc_external_map`, in any case, is an instance. It
  names the file in `_external_map` and the classname it becomes in `_external_map_classname`;
  ericw-tools asserts both are given and the entity has no brush. This spec fixes the answer: an
  instance without either key, or with brushes, is an error.
- **MAP-14.2** The file is read as a `.map`: its first entity must be `worldspawn`, in any case,
  and every entity's brushes are taken, but for an entity whose `_omitbrushes` reads as a
  non-zero integer, and a brush with no face. A file that cannot be read, does not start with
  its world or gives no brush is an error.
- **MAP-14.3** The instance's `origin`, `_external_map_angles` and `_external_map_scale` are
  read as `sscanf`'s `"%f %f %f"` reads them: up to three floats, a missing one 0.
- **MAP-14.4** The angles are pitch, yaw and roll in degrees. When all three are within 0.0001
  of 0, the yaw is `_external_map_angle`, read as `atof`, 0 when not given.
- **MAP-14.5** A scale of three numbers scales each axis by its own. Fewer than three gives one
  scale for all: 1 when the first is 0 or missing, else the first. A scale of 0 along an axis
  leaves no brush, an inverse of infinity in ericw-tools; this spec fixes the answer: it is an
  error.
- **MAP-14.6** Each point of each face is scaled, then turned by `Rz(yaw) Ry(pitch) Rx(roll)`,
  each a right-handed rotation about its axis, then moved by the origin. Each texture axis
  (MAP-4.4, MAP-5.3) is divided by the scale along each component, then turned the same way,
  and its offset is less the dot product of the turned axis with the origin, so every point of
  a face keeps its texel. ericw-tools keeps texture axes in single precision; this spec keeps
  them in double, so the two can differ in the last bits.
- **MAP-14.7** The instance's classname becomes `_external_map_classname` and its origin
  `0 0 0`; its other keys are kept, and the placed brushes are added after its pairs. Each
  placed face is written in the Valve 220 form (MAP-5.1) with the placed axes, no rotation and a
  scale of 1; a face with no plane keeps its definition, as both compilers drop it (MAP-3.6).
