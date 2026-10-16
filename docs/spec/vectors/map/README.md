# .map test vectors

Vectors for [`../../map.md`](../../map.md). Each `.txt` file here is a `.map` file, stored
with a `.txt` name so the repository's license check classifies it as text. One more vector,
`crlf-highbyte`, needs bytes that the repository's line-ending rules would change, so it is
given as hex below.

**Checked:** on 2026-10-16 every vector was read by id's qbsp parser: `qutils/QBSP/MAP.C`
and `qutils/COMMON/MATHLIB.C` copied unchanged from
id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73` (CR bytes removed, as
the files use CRLF line endings), built with the system C compiler (Apple clang, arm64) and
`qutils/COMMON/BSPFILE.H` and `MATHLIB.H` from the same commit, with stubs for `Error`
(prints its message and stops), `LoadFile`, `copystring`, `qprintf` and `Q_strncasecmp`. The
harness ran `LoadMapFile`, printed every entity's pairs and every face's plane and texinfo,
then ran `WriteEntitiesToString` and printed the entity lump. qbsp is used here as an oracle
only ([`../../../porting.md`](../../../porting.md#upstreams)). ericw-tools was read, not
run: the `valve220` results come from its code.

| File | qbsp's result | Statements |
| --- | --- | --- |
| [`standard.txt`](standard.txt) | Loads: see [standard](#standard) | MAP-1.3, MAP-2.1 to MAP-2.7, MAP-3.1 to MAP-3.5, MAP-4.1 to MAP-4.5, MAP-4.7, MAP-6.1, MAP-6.2 |
| [`valve220.txt`](valve220.txt) | Error `parsing brush`; ericw-tools reads it: see [valve220](#valve220) | MAP-5.1 to MAP-5.3, MAP-5.5 |
| [`no-open-brace.txt`](no-open-brace.txt) | Error `ParseEntity: { not found` | MAP-2.1 |
| [`eof-in-entity.txt`](eof-in-entity.txt) | Error `ParseEntity: EOF without closing brace` | MAP-2.2 |
| [`eof-in-quote.txt`](eof-in-quote.txt) | Error `EOF inside quoted token` | MAP-1.4 |
| [`value-on-next-line.txt`](value-on-next-line.txt) | Error `Line 2 is incomplete` | MAP-1.7, MAP-2.3 |
| [`comment-mid-line.txt`](comment-mid-line.txt) | A `//` comment between a key and its value: error `Line 2 is incomplete` | MAP-1.3, MAP-1.7 |
| [`key-30.txt`](key-30.txt) | A 30-byte key: loads | MAP-2.4 |
| [`key-31.txt`](key-31.txt) | A 31-byte key: error `ParseEpar: token too long` | MAP-2.4 |
| [`token-127.txt`](token-127.txt) | A 127-byte value: parses; writing the entity lump then overflows `line[128]` and the build's stack check stops the program | MAP-1.6, MAP-2.10 |
| [`token-128.txt`](token-128.txt) | A 128-byte value: error `Token too large on line 2` | MAP-1.6 |
| [`escape.txt`](escape.txt) | Loads; the value of `message` is the 4 bytes `a\nb` with a backslash, and is written to the lump that way | MAP-1.4 |
| [`point-split.txt`](point-split.txt) | The third point of a face on its own line: loads; plane normal (-1, 0, 0), dist 0 | MAP-3.2 |
| [`float-points.txt`](float-points.txt) | Points `( 0.5 0 0 ) ( 0.5 1.9 0 ) ( 0.5 0 1 )`, shift `0.7 -1.5`: loads as points (0, 0, 0), (0, 1, 0), (0, 0, 1), plane normal (-1, 0, 0) dist 0, texinfo offsets 0 and -1 | MAP-3.3, MAP-4.1 |
| [`no-normal.txt`](no-normal.txt) | Collinear points: `WARNING: brush plane with no normal`, then the rest of the brush is read as keys and fails with `Line 4 is incomplete` | MAP-3.6 |
| `crlf-highbyte` | The bytes `7b 0d 0a 22 63 6c 61 73 73 6e 61 6d 65 22 c2 a0 22 77 6f 72 6c 64 73 70 61 77 6e 22 0d 0a 7d 0d 0a`: CRLF line ends, and the key and value separated by the bytes 0xC2 0xA0. Loads: one entity, `classname` `worldspawn` | MAP-1.2 |

## standard

Three entities: the world with four keys and one six-sided brush, a light with a duplicated
`light` key, and a door with one brush whose seventh face repeats its first.

qbsp prints `WARNING: brush with duplicate plane` once (the door's seventh face) and reads 3
entities, 2 brushes, 5 texture names (`wall`, `floor`, `+0lava`, `*water`, `sky1`, in that
order) and 7 texinfo.

World brush faces, in file order:

| Face | Plane normal, dist | Texture | Flags | s axis, offset | t axis, offset |
| --- | --- | --- | --- | --- | --- |
| `wall 0 0 0 1 1` | (-1, 0, 0), 64 | `wall` | 0 | (0, 1, 0), 0 | (0, 0, -1), 0 |
| `wall 0 0 0 1 1` | (0, -1, 0), 64 | `wall` | 0 | (1, 0, 0), 0 | (0, 0, -1), 0 |
| `floor 16 -8 90 0.5 2` | (0, 0, -1), 16 | `floor` | 0 | (0, 2, 0), 16 | (0.5, 0, 0), -8 |
| `+0lava 0 0 45 1 1` | (0, 0, 1), 16 | `+0lava` | 0 | (0.707107, 0.707107, 0), 0 | (0.707107, -0.707107, 0), 0 |
| `*water 0 0 0 1 1` | (0, 1, 0), 64 | `*water` | 1 | (1, 0, 0), 0 | (0, 0, -1), 0 |
| `sky1 0 0 0 0 0` | (1, 0, 0), 64 | `sky1` | 1 | (0, 1, 0), 0 | (0, 0, -1), 0 |

The door's six faces use texinfo 0 and 1 (the same as the world's two `wall` walls) and a new
texinfo 6 for its top and bottom, s axis (1, 0, 0) and t axis (0, -1, 0); its planes are
(-1, 0, 0) 0, (0, -1, 0) 0, (0, 0, -1) 0, (1, 0, 0) 8, (0, 1, 0) 8 and (0, 0, 1) 8. The light's
origin is (0, 0, 8).

The entity lump qbsp writes, 199 bytes with its NUL (the door has no `model` key because the
harness does not run `ProcessEntity`):

```
{
"sounds" "2"
"message" "The Test Map"
"wad" "gfx/base.wad"
"classname" "worldspawn"
}
{
"light" "300"
"light" "200"
"origin" "0 0 8"
"classname" "light"
}
{
"angle" "-1"
"classname" "func_door"
}
```

## valve220

The world's brush of `standard` in the Valve 220 form, with `"mapversion" "220"`. By
ericw-tools' `parse_valve_220` and `set_texinfo`, each face's texinfo axes are the bracketed
axes divided by the scale and its offsets the bracketed shifts; the third face,
`wall [ 1 0 0 16 ] [ 0 -1 0 -8 ] 0 0.5 2`, gives s axis (2, 0, 0) offset 16 and t axis
(0, -0.5, 0) offset -8.
