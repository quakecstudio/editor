# MD5 models

Prefix: `MD5` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the `.md5mesh` and `.md5anim` files of the 2021 rerelease's enhanced models: the
skeletal text format of id Tech 4, as the rerelease uses it to replace the `.mdl` models of
`id1`. id's rerelease engine is not published, and id's GPL Quake source has no MD5 code, so
the format is described from the engine that reads the rerelease's files, cited for
description only; it is not a registered upstream for porting
([`../porting.md`](../porting.md#upstreams)) and no code is ported from it:

| Short name | Repository and commit | Files read |
| --- | --- | --- |
| QSS | [Shpoike/Quakespasm](https://github.com/Shpoike/Quakespasm)@`4c0c55f7830715cf8670f586c7be01ebce262d23` (QuakeSpasm-Spiked, GPL-2.0-or-later) | `Quake/gl_mesh.c`, `Mod_LoadMD5MeshModel`, `MD5Anim_Begin`, `MD5Anim_Load` and the `MD5_Parse` helpers; `Quake/gl_model.c`, `Mod_LoadModel`; `Quake/common.c`, `COM_Parse` |

QSS's own comment above its loader reads "MD5 Models, for compat with the rerelease and NOT
doom3." Where QSS's checks are missing or wrong, the statements below say what QSS does and
fix a stricter answer, so a file that loads here also loads in QSS. Implemented by
`crates/qs-mdl` ([#235](https://github.com/quakecstudio/editor/issues/235)).

## MD5-1 Tokens

**Source:** QSS `Quake/common.c`, `COM_Parse`; `Quake/gl_mesh.c`, `MD5_ParseUInt`,
`MD5_ParseSInt`, `MD5_ParseFloat`.

Both files are text. QSS loads the file with a NUL after it and reads it one token at a
time with `COM_Parse`, comparing each keyword with `strcmp` and converting each number with
`strtoull`, `strtol` or `strtod`.

### Behaviour

- **MD5-1.1** A file is read up to its first NUL byte or its end, whichever comes first.
- **MD5-1.2** Tokens are separated by whitespace: every byte from 0x01 to 0x20, and every
  byte from 0x80 to 0xFF. `COM_Parse` compares a `char`, which is signed on x86, with
  `' '`, so the high bytes count as whitespace there; this spec follows the x86 build.
- **MD5-1.3** `//` starts a comment that runs to the end of the line, and `/*` one that runs
  to the next `*/` or the end of the file. Comments separate tokens.
- **MD5-1.4** A `"` starts a quoted token that runs to the next `"` or the end of the file.
  The quotes are not part of the token, which may be empty and may hold any byte but `"`.
- **MD5-1.5** Each of `{`, `}`, `(`, `)`, `'` and `:` is a token by itself. Any other run
  of bytes is a word, which ends before whitespace or before `{`, `}`, `(`, `)` or `'` (but
  not `:`), so `abc(` is the two tokens `abc` and `(`.
- **MD5-1.6** A keyword matches a token with the same bytes, case included, quoted or not.
- **MD5-1.7** An integer is a token of an optional `+` or `-` and one or more decimal
  digits that fits the field (unsigned 64-bit for counts and indices, signed 64-bit for a
  parent joint). Anything else, and the end of the file where a number is expected, is an
  error. QSS converts the token with base 0, so it also reads `0x` hexadecimal and leading-zero
  octal, takes 0 for a token with no number and ignores what follows the number; this spec
  rejects those, which an exporter does not write.
- **MD5-1.8** A float is a token Rust's `f64` parser accepts (an optional sign, decimal
  digits with an optional point and exponent, or `inf`, `infinity` or `nan` in any case),
  rounded to the nearest `f64` and then to the nearest `f32`, as QSS's `strtod` into a
  `double` stored in a `float` does. Anything else, and the end of the file, is an error.
- **MD5-1.9** The end of the file where a name, a number or a required keyword is expected
  is an error; a list of entries (MD5-2.8, MD5-3.6) and the optional `commandline` just end
  there. QSS reads an empty token at the end and fails at the next keyword it requires, or
  reads 0 for a number; this spec fixes the answer as an error at the end of the file.

## MD5-2 The mesh file

**Source:** QSS `Quake/gl_mesh.c`, `Mod_LoadMD5MeshModel`, `MD5_BakeInfluences`;
`Quake/gl_model.c`, `Mod_LoadModel`.

```text
MD5Version 10
commandline "<text>"
numJoints <n>
numMeshes <n>
joints {
    "<name>" <parent> ( <x> <y> <z> ) ( <qx> <qy> <qz> )
}
mesh {
    shader "<name>"
    numverts <n>
    vert <index> ( <s> <t> ) <first weight> <weight count>
    numtris <n>
    tri <index> <vertex> <vertex> <vertex>
    numweights <n>
    weight <index> <joint> <bias> ( <x> <y> <z> )
}
```

### Behaviour

- **MD5-2.1** A mesh file's first four bytes are `MD5V`; `Mod_LoadModel` picks the MD5
  loader by them, so a file that starts with anything else, even whitespace, is not read as
  MD5 and the reader rejects it.
- **MD5-2.2** The header is the tokens `MD5Version` and `10`, optionally `commandline` and
  one token, then `numJoints` and its count and `numMeshes` and its count. A missing or
  different keyword fails with the error `Expected "<keyword>"`.
- **MD5-2.3** `numJoints` must be at least 1 (`<name> has no bones`) and `numMeshes` at
  least 1 (`<name> has no meshes`).
- **MD5-2.4** `joints` and `{` follow, then exactly `numJoints` joints, then `}`. A joint is
  its name, its parent joint's index, `(`, a position of three floats, `)`, `(`, the x, y and
  z of its orientation quaternion, and `)`.
- **MD5-2.5** A joint's parent must be -1 (no parent) or the index of a joint, less than
  `numJoints`; otherwise reading fails with the error `bone index out of bounds`. QSS's
  test is `parent < -1 && parent >= numjoints`, which is never true, so QSS accepts any
  parent; this spec fixes the check it meant.
- **MD5-2.6** A quaternion's w is `-sqrt(1 - (x*x + y*y + z*z))` in `f32`, with the value
  under the root taken as 0 when it is negative, so w is never positive.
- **MD5-2.7** Exactly `numMeshes` meshes follow the joints. A mesh is `mesh`, `{`, `shader`
  and one token naming its skin, `numverts` and a count, the vertices, `numtris` and a
  count, the triangles, `numweights` and a count, the weights, and `}`. Tokens after the
  last mesh are ignored.
- **MD5-2.8** Vertices, triangles and weights are lists of entries, each starting with its
  keyword (`vert`, `tri`, `weight`) and its index; a list ends at the first token that is
  not the keyword. The count need not equal the number of entries: an index never given is
  all zero, and an index given twice keeps the last entry.
- **MD5-2.9** A vertex is `vert`, its index (less than `numverts`), `(`, texture
  coordinates `s` and `t` as floats, `)`, the index of its first weight and its number of
  weights. A vertex index of `numverts` or more fails with the error
  `vertex index out of bounds`.
- **MD5-2.10** A triangle is `tri`, its index (less than `numtris`, or the error
  `triangle index out of bounds`) and three vertex indices, each less than `numverts`, or
  the error `vertex index out of bounds`. QSS's test is `t > numverts`, which accepts
  `numverts` itself and reads one vertex past the array; this spec fixes the answer as an
  error.
- **MD5-2.11** A weight is `weight`, its index (less than `numweights`, or the error
  `weight index out of bounds`), its joint's index (less than `numJoints`, or the error
  `bone index out of bounds`), its bias as a float, `(`, a position of three floats, and `)`.
- **MD5-2.12** Each vertex's weights are the `count` weights from its first: first weight
  plus count must be at most `numweights` (the error `<name>: weight index out of bounds`).
  The check is made once the mesh's weights are read, for every vertex including those never
  given.
- **MD5-2.13** A mesh's skins are images found from its shader name: skin `n` frame `f` is
  `progs/<shader>_<nn>_<ff>`, with `n` and `f` written as two or more decimal digits
  (`progs/ogre_00_00`). QSS looks for skins from 0 and frames from 0 to 3, and stops at the
  first skin with no frame 0; the image file's extension is the image loader's.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MD5_MAGIC` | `MD5V` | QSS `Quake/gl_model.c`, `Mod_LoadModel`, the case `('M'<<0)+('D'<<8)+('5'<<16)+('V'<<24)` |
| `MD5_VERSION` | `10` | QSS `Quake/gl_mesh.c`, `Mod_LoadMD5MeshModel`, `MD5EXPECT("10")` |
| `MD5_NO_PARENT` | -1 | QSS `Quake/gl_mesh.c`, `Mod_LoadMD5MeshModel`, the parent check |
| `MD5_MAX_SKIN_FRAMES` | 4 | QSS `Quake/gl_mesh.c`, `Mod_LoadMD5MeshModel`, `countof(surf->textures[0])` |

### Test vectors

See the table in MD5-3.

## MD5-3 The animation file

**Source:** QSS `Quake/gl_mesh.c`, `MD5Anim_Begin`, `MD5Anim_Load` and
`Mod_LoadMD5MeshModel`.

```text
MD5Version 10
commandline "<text>"
numFrames <n>
numJoints <n>
frameRate <fps>
numAnimatedComponents <n>
hierarchy {
    "<name>" <parent> <flags> <first component>
}
bounds {
    ( <x> <y> <z> ) ( <x> <y> <z> )
}
baseframe {
    ( <x> <y> <z> ) ( <qx> <qy> <qz> )
}
frame <index> {
    <component> ...
}
```

### Behaviour

- **MD5-3.1** The animation of `<path>.md5mesh` is `<path>.md5anim`, the same path with the
  extension replaced. A mesh with no animation file loads with no frames.
- **MD5-3.2** The header is the tokens `MD5Version` and `10`, optionally `commandline` and one
  token, `numFrames` and its count, `numJoints` and its count, `frameRate` and one token
  (the frame rate, which QSS skips without reading it as a number), then
  `numAnimatedComponents` and its count. `numFrames` must be at least 1 (`<name> has no
  poses`).
- **MD5-3.3** `hierarchy` and `{` follow, then exactly `numJoints` entries, then `}`. An
  entry is a joint name, its parent's index, its flags and the index of its first
  component. Flags other than the low six bits fail with the error
  `<name>: bone has unsupported flags`.
- **MD5-3.4** A joint's components are the ones its flags select, in the order x, y, z of
  the position then x, y, z of the quaternion, for flag bits 1, 2, 4, 8, 16 and 32; they are
  read from its first component on. Its first component plus the number of flags set must
  be at most `numAnimatedComponents`, or reading fails with the error
  `<name>: bone has bad offset`. QSS only checks that the first component is at most
  `numAnimatedComponents + 6` and reads the six spare floats it allocates, or past them;
  this spec fixes the answer as an error.
- **MD5-3.5** `bounds` and `{` follow, then any number of pairs `(` x y z `)` `(` x y z `)`,
  then `}`; then `baseframe` and `{`, any number of pairs `(` x y z `)` `(` qx qy qz `)`,
  and `}`. Their counts are not checked, and QSS reads the values and does not use them.
- **MD5-3.6** Then come frames, each `frame`, its index (less than `numFrames`, or the error
  `<name>: invalid pose index`), `{`, exactly `numAnimatedComponents` floats and `}`. The list
  ends at the first token that is not `frame`, and tokens after it are ignored. A frame never
  given has every component 0, and a frame given twice keeps the last.
- **MD5-3.7** A joint's pose in a frame is its components as MD5-3.4 selects them, with each
  component its flags do not select taken as 0 (not from `baseframe`, as id Tech 4 does),
  and the quaternion's w computed as MD5-2.6 says.
- **MD5-3.8** The end of the file after `frameRate`'s value fails reading with the error
  `Expected "numAnimatedComponents"`. QSS then keeps the frame count with no poses, which
  is undefined when the model is drawn; this spec fixes the answer as an error.
- **MD5-3.9** An animation goes with a mesh only if its `numJoints` equals the mesh's
  (`<name> has incorrect bone count`) and each hierarchy entry has the name and parent of
  the mesh's joint at the same index (`<name>: bone was renamed`,
  `<name>: bone has wrong parent`).
- **MD5-3.10** Each frame of the animation is one frame of the model, shown for 0.1 seconds.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MD5_ANIM_FLAGS` | 63 | QSS `Quake/gl_mesh.c`, `MD5Anim_Load`, `ab[j].flags & ~63` |
| `MD5_FRAME_INTERVAL` | 0.1 | QSS `Quake/gl_mesh.c`, `Mod_LoadMD5MeshModel`, `surf->frames[j].interval = 0.1` |

### Test vectors

Written from QSS's code on 2026-10-16. QSS was not built to check them, because no code
is ported from it; `qs-mdl`'s unit tests check each row.

| Input | Result | Statements |
| --- | --- | --- |
| One joint `"root" -1 ( 1 2 3 ) ( 0 0 0 )`, one mesh: shader `"ogre"`, 3 vertices, 1 triangle `0 1 2`, 1 weight | Loads; joint w -1; skin path `progs/ogre_00_00` | MD5-2.2, MD5-2.4, MD5-2.6, MD5-2.13 |
| The same with `numJoints 0` | `has no bones` | MD5-2.3 |
| The same with a triangle `0 1 3` | `vertex index out of bounds` | MD5-2.10 |
| The same with a joint parent of 1 | `bone index out of bounds` | MD5-2.5 |
| An animation of 2 frames, one joint with flags 63 from component 0, 6 components; frame 1 `1 2 3 0.6 0 0` | Frame 1: position (1, 2, 3), quaternion (0.6, 0, 0, -0.8); frame 0 all zero with w -1 | MD5-3.4, MD5-3.6, MD5-3.7 |
| The same with flags 64 | `bone has unsupported flags` | MD5-3.3 |
| The same with first component 1 | `bone has bad offset` | MD5-3.4 |
