# qs-map

Reads and writes `.map` files, the text a level editor saves and a BSP compiler reads.

- **`MapFile`** is the lossless document. `MapFile::parse` keeps every byte, comments and
  formatting included, so `MapFile::to_bytes` gives back the file it read, and an edit
  (`Entity::set`, `Entity::remove`, `Face::set`, new entities, brushes and faces) changes only
  the text it touches; new text is written in the form both compilers read (MAP-7). It reads
  the standard and the Valve 220 forms face by face, numbers as doubles and tokens of any
  length, and gives each face's plane and texture axes. `MapFile::check` reports what id's
  qbsp, ericw-tools or the game would do otherwise: tokens and keys too long for qbsp,
  fractions it truncates, Valve 220 faces it cannot read, duplicate planes, collinear points,
  `;` and `\"` that ericw-tools reads differently, the world's WAD, message and CD track.
- **`qbsp`** models what id's qbsp reads: `load_map_file` stops with qbsp's error and line
  number and keeps its warnings, planes and texinfo in single precision from `atoi` points,
  and `entity_lump`, `number_models` and `texture_slots` give the entity lump, the brush
  models' names and the textures lump's slots qbsp writes.

The entity lump of a BSP is the same syntax without brushes, so `MapFile` also reads and edits
a shipped map's entities, which `qs-bsp` writes back into the BSP or a `.ent` file (MAP-22).

id's qbsp is in `qutils/` of Quake-Tools, which is not a portable upstream
([`docs/porting.md`](../../docs/porting.md)), and ericw-tools is cited for description only, so
nothing here is ported: the code is written from the spec, whose vectors were checked by
running qbsp's parser.

- **Layer:** L0 formats, with no dependencies. `qs-vfs`'s `walk` example reads every BSP's
  entity text with it, and `cargo xtask map-corpus` reads the corpus's maps.
- **Spec sections:** MAP-1 to MAP-7 in [`map.md`](../../docs/spec/map.md).
- **Shipped:** yes.

## Invariants

Property tests in `src/*/tests.rs` and the fuzz target `fuzz/fuzz_targets/qs_map.rs`
(`cargo xtask fuzz qs-map`):

- Arbitrary bytes never panic the reader, its checks or the qbsp model (MAP-1 to MAP-7).
- Every file the reader accepts writes back byte for byte (MAP-1 to MAP-3).
- A face written from numbers reads back as the same numbers (MAP-7.3).
- Every face plane holds its three points and has a unit normal (MAP-3.4).

## Corpus and differential result

`cargo xtask map-corpus <cache>/map`, after `cargo xtask corpus fetch` of the 26 maps in
`corpus/manifest.toml`, reads every one and writes it back byte for byte (2026-10-16): 20
LibreQuake maps, four ericw-tools test maps and two progs_dump maps, 1 to 5335 entities per
map and 262,866 faces, 255,634 of them Valve 220. The qbsp model reads four of them, which
use the standard form and short keys; the others would stop id's qbsp, at a token over 127
bytes or a Valve 220 face (MAP-1.6, MAP-5.5).

The qbsp model gives every result the vectors of
[`vectors/map/`](../../docs/spec/vectors/map/README.md) record for id's qbsp, which was run on
them when the spec was written; its tests in `src/qbsp/tests.rs` check each one.
