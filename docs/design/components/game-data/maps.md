# Maps

**Crates:** `qs-bsp`, `qs-map` (L0). **Specs:** BSP-1 to BSP-20, MAP-1 to MAP-7; ADR-0012.

## Context and design philosophy

Two L0 crates own Quake's two map formats: the compiled BSP the engine loads, and the `.map`
source the compilers read.

| Crate | Owns | Spec | Source |
| --- | --- | --- | --- |
| `qs-bsp` | BSP29, BSP2 (2PSB and BSP2), `.lit` side files, `.ent` override names, entity-lump replacement | [`bsp.md`](../../../spec/bsp.md) BSP-1 to BSP-20 | Ported from `WinQuake/model.c` (22 functions), `gl_model.c`, `world.c` `SV_HullForEntity` (`crates/qs-bsp/src/lib.rs:5-7`); `.lit` and `.ent` described from QuakeSpasm and ericw-tools, not ported (`lit.rs:5-6`, `ent.rs:3-7`) |
| `qs-map` | A lossless `.map` document (parse, edit, write back byte for byte), diagnostics, and a behavioural model of id's qbsp `LoadMapFile` | [`map.md`](../../../spec/map.md) MAP-1 to MAP-7 | Nothing ported: `qutils/` is not a portable upstream until the license review (P4-22, #133) rules on it (`crates/qs-map/README.md:22-25`, `qbsp.rs:6-9`) |

The two meet at the entity lump: a BSP's entity text is `.map` syntax without brushes, so
`qs-map`'s `MapFile` reads and edits a shipped map's entities and `qs-bsp` writes them back
into the BSP (`replace_entities`) or names the `.ent` file QuakeSpasm would load
(`ent_file_names`). That serves plan requirement MAP-22 (entity edits to shipped maps with no
`.map` source), pinned end to end by `crates/qs-tests/tests/it/map_entities.rs`. `qs-bsp` and
`qs-map` do not depend on each other; the composition happens in the caller.

## Components

### BSP loading (`qs-bsp`)

`Bsp::read(bytes, loader)` loads lump by lump in id's order (vertexes, edges, surfedges,
textures, lighting, planes, texinfo, faces, marksurfaces, visibility, leafs, nodes,
clipnodes, entities, submodels; `crates/qs-bsp/src/lib.rs:991-1005`) and returns a `Bsp<'a>`
that borrows the entity lump, visibility, lighting and texture pixels and owns the decoded
arrays.

The loader is a parameter, per [ADR-0012](../../../adr/0012-bsp-loader-default.md):

| `BspLoader` | Rules | Status |
| --- | --- | --- |
| `FitzQuake` (`#[default]`) | QuakeSpasm at `c9c8ee9` as the FitzQuake-lineage reference; BSP-20 lists where it differs from GLQuake | the editor's default |
| `GlQuake` | id's `gl_model.c`: extent limit 512 (BSP-9.17), `SURF_UNDERWATER` (BSP-12.6) | compatibility mode |
| `WinQuake` | id's `model.c`: extent limit 256 (BSP-9.4) | compatibility mode |

Loader-specific arithmetic is written out with `#[expect]` reasons citing the statement:
x87 53-bit texture coordinates (BSP-9.11), NaN bounds (BSP-9.12), the x86 indefinite integer
(BSP-9.13), low-16 truncation (BSP-9.14), QuakeSpasm's double sum (BSP-20.9), FitzQuake's
child reinterpretation (BSP-20.15 to 20.17) (`crates/qs-bsp/src/lib.rs:795-893`).

Beyond loading, `Bsp` gives what the engine derives: face vertices and light samples, the
PVS row of a leaf (`Mod_LeafPVS`/`Mod_DecompressVis`), hull 0 (`Mod_MakeHull0`), the hull
for a size (`SV_HullForEntity`), model names and precache indexes.

`.lit` handling (`lit.rs`) is policy as well as parsing: `mod_load_lighting` makes
QuakeSpasm's choice between a found `.lit` and the BSP's grey lighting (priority, magic,
version, size) and gives the user-facing and developer messages; the id loaders never read a
`.lit` (BSP-18.6).

`replace_entities(bytes, text, loader)` re-reads the BSP, replaces the entity lump, orders
lumps by original offset and 4-aligns each (`crates/qs-bsp/src/lib.rs:2082-2127`).

### Map source (`qs-map`)

Two readers share one tokenizer (`lexer.rs`, MAP-1):

- **`MapFile`** (`document.rs`): lossless. Every node keeps its raw leading bytes so
  `to_bytes` reproduces the input exactly; edits (`Entity::set`, `remove`, `Face::set`,
  `push_entity`) write the MAP-7 form. No token length limit. Standard and Valve 220 texture
  definitions are detected per face (`document.rs:775`). Geometry is double precision.
- **`qbsp::load_map_file`** (`qbsp.rs`): a model of id's qbsp, with its 127-byte token limit,
  its errors and warnings verbatim (including the misspelt `ParseEpar`, `qbsp.rs:63`), single
  precision planes from `atoi` points, and the entity lump, model numbering and texture slots
  qbsp writes. C number conversions are in `cnum.rs` (MAP-2.7, MAP-3.3, MAP-4.1).

`MapFile::check` (`check.rs`) reports what qbsp, ericw-tools or the game would do
differently with a document, partly by running the qbsp model over `to_bytes()`.

## Error model

- `Bsp::read` returns `BspError`, checked in id's lump order so the first error is the one
  the engine reports; `.lit` reading returns `LitError`, but `mod_load_lighting` never fails a
  map: an ignored `.lit` is a message and grey light (BSP-18).
- `MapFile` parsing returns `MapError` (`Lex`, `LineIncomplete`, `NoOpenBrace`,
  `EofWithoutBrace`, `ParsingBrush`, `BadNumber`, `ValveScaleZero`); the qbsp model returns
  `QbspError` with qbsp's verbatim text, misspellings included.
- `MapFile::check` returns diagnostics, not errors.

## Verification

- Statement-cited tests; factories whose `Default` is the spec's `base` vector
  (`crates/qs-bsp/src/fixtures.rs`); the map vectors in `docs/spec/vectors/map/` with qbsp's
  recorded results (`crates/qs-map/src/qbsp/tests.rs`).
- Proptests and fuzz targets `fuzz/fuzz_targets/qs_bsp.rs` (three loaders, `Lit::read`,
  `replace_entities`) and `qs_map.rs` (reader, qbsp model, lossless write-back).
- Differential: id's `Mod_LoadBrushModel` on 23 LibreQuake BSPs and a QuakeSpasm
  `gl_model.c` run for the default loader, two named divergences (BSP-20.2, BSP-20.18)
  (`crates/qs-bsp/README.md:56-81`); the harness is not committed (`README.md:73-74`).
  `cargo xtask map-corpus` round-trips 26 corpus maps byte for byte
  (`crates/qs-map/README.md:44-49`).
- L0 readiness passed at `110ce43`: `qs-bsp` 97.0% mutation, `qs-map` 97.6%, both 100%
  citation (`docs/readiness/p1-r0.md:65-66`).

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Default BSP rules | FitzQuake lineage, QuakeSpasm `c9c8ee9` as reference, described not ported | WinQuake only; GLQuake only; no default; DarkPlaces | ADR-0012: accept and judge maps as the engines modders play on do, while still answering "would this load in id's engine" |
| Keep id loaders | `BspLoader::WinQuake` and `GlQuake` as explicit modes | Remove them | ADR-0012 consequences: an explicit choice keeps working unchanged |
| `.map` source | Model qbsp from the spec; port nothing | Port qutils qbsp | `docs/porting.md`: `qutils/` not portable until P4-22 |
| Two `.map` readers | Lossless `MapFile` plus a separate qbsp model | One reader with modes | [inferred] the editor needs byte-exact round trips with no limits, while diagnostics need qbsp's exact limits and precision |
| Duplicate keys | `Entity::value` returns the last occurrence | First occurrence | MAP-2.5: the one both compilers look up (`document.rs:515-522`) |
| Entity edits to shipped maps | `MapFile` + `replace_entities` or `.ent` names | A separate entity-lump parser | [inferred] the entity lump is `.map` syntax without brushes (`crates/qs-map/README.md:19-20`) |
| `.lit` source | QuakeSpasm and ericw-tools, described | Port | No id code reads `.lit` (BSP-18) |

## Open questions

### Resolved
1. Default loader: ADR-0012 (P1-45, #320).
2. `Face::set` refuses a NUL in a texture name, as pair quoting does: `is_map_space` covers every
   byte up to 0x20 (`crates/qs-map/src/lexer.rs:116-118`, `document.rs:177-185`).

### Deferred
1. **`DuplicateKey` message disagrees with the spec.** `check.rs:36-37` and `:82` say "qbsp
   and the game see the first, ericw-tools the last"; MAP-2.5 (`docs/spec/map.md:108-110`)
   says qbsp looks a key up by its last occurrence, as `QbspEntity::value_for_key` does
   (`qbsp.rs:196-204`). MAP-2.6 says the game sees the first. The qbsp half of the message is
   wrong.
2. **`leaf_pvs` allocates from a file count.** The PVS row size derives from model 0's
   `visleafs` (`crates/qs-bsp/src/lib.rs:1699-1721`); an `i32` near `i32::MAX` from a small file
   gives a ~268 MB `vec![0xFF; row]` per call. Bounded, not a panic; the fuzz target only checks
   PVS when the row is at most 65,536 bits (`fuzz/fuzz_targets/qs_bsp.rs:50-54`). Same class as
   the `Md5Anim::frame` finding in [archives-assets](archives-assets.md).
3. `BspError::FunnyLumpSize`'s message drops the lump index (`crates/qs-bsp/src/lib.rs:423`).
4. `length::<N>` sums only the first three elements and is called with 4-element texinfo rows
   (`crates/qs-bsp/src/lib.rs:1801-1806`, `:1117`); correct for the vector part, but the
   generic signature hides it.
5. The README lists BSP-1 to BSP-20 while the crate doc says BSP-1 to BSP-19
   (`crates/qs-bsp/src/lib.rs:10`). The README's invariant that arbitrary bytes never panic
   the `.lit` reader is covered by the fuzz target, not the in-crate proptests
   (`tests.rs:1719-1754`, `fitz_tests.rs:696-719`).
6. `MAP_MAX_BRUSHES` is defined and unused ("qbsp does not check it", `qbsp.rs:36-37`).
7. `Reader::same_line`'s `LineIncomplete { line: 0 }` fallback is unreachable (`qbsp.rs:246-268`).
8. **`MAP-` names two things.** `MAP-n` is both a spec prefix (`docs/spec/map.md`, MAP-1 to
   MAP-7) and a plan requirement prefix (`plan/requirements.yaml`, MAP-1 to MAP-24).
   `wad2.md:10-11` (`MAP-5, MAP-23`) and `bsp.md:1264-1266` (`MAP-6 asks for Quake's formats
   only`) read as broken cross-references; both resolve as plan requirements (MAP-5 texture
   browser, MAP-23 PNG import, MAP-6 read/write formats). `qs-map/README.md:20` and
   `ent.rs:3-7` cite MAP-22, also a plan requirement. A reader cannot tell which is meant.
9. The differential harness for `qs-bsp` is not committed (`crates/qs-bsp/README.md:73-74`);
   BSP-20.2's expected difference is not listed in `differential.md` DIFF-10.1, which covers
   the VM harness only.
10. Planned: BSP version 30 in `BspLoader::FitzQuake` (P5-11, #330); external WAD3 textures for
    version 30 (P5-12, #331); BSP, BSP2 and `.lit` writing (plan requirement MAP-6; the map
    compiler, MAP-8, is P5+ work).

## References

- ADR: [ADR-0012](../../../adr/0012-bsp-loader-default.md)
- Specs: [`bsp.md`](../../../spec/bsp.md), [`map.md`](../../../spec/map.md), vectors in `docs/spec/vectors/bsp/`, `docs/spec/vectors/map/`
- Plan: P5-01 (#142), P5-02 (#143), P1-40 to P1-47 (#305 to #324), P1-52 (#340); open P5-11 (#330), P5-12 (#331), P4-22 (#133)
