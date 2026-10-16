# qs-bsp

Reads compiled maps: BSP29 as id's engine loads it, the `2PSB` and `BSP2` variants, and `.lit`
coloured light; and writes a BSP's entities back, for shipped maps with no `.map` source.

- **`Bsp::read`** is id's `Mod_LoadBrushModel` and the loaders it calls, in id's order, with
  the bounds and index checks id's engine leaves out. It rejects malformed input with a typed
  `BspError`, whose `message` is id's, never a panic. It keeps what the loader computes:
  plane sign bits, texture animations, texinfo mip bias and checkerboard fallback, face
  extents, flags and lightmap sizes, and model bounds and radii. `Bsp::leaf_pvs` is
  `Mod_LeafPVS` with `Mod_DecompressVis`, `Bsp::hull0` is `Mod_MakeHull0`, and
  `hull_for_size` is the hull choice of `SV_HullForEntity`. Ported from `WinQuake/model.c`
  and `world.c` at the commit pinned in [`docs/porting.md`](../../docs/porting.md). BSP2
  follows QuakeSpasm's loader, cited for description only.
- **`lit`**: `Lit::read` checks a `.lit` file as QuakeSpasm does and gives each lighting
  byte's colour; `grey` is the colour without one.
- **`ent`** and **`replace_entities`**: `ent_file_names` gives the `.ent` names QuakeSpasm
  tries for a map, and `replace_entities` writes a BSP with new entity text and every other
  lump unchanged. With `qs-map`, which reads and edits the entity text without losing a byte,
  they are the two ways to edit a shipped map's entities (MAP-22).

- **Layer:** L0 formats. It depends on `qc-progs` for id's CRC. `qs-vfs`'s `walk` example loads
  every BSP and `.lit` of an install with it.
- **Spec sections:** BSP-1 to BSP-19 in [`bsp.md`](../../docs/spec/bsp.md).
- **Shipped:** yes.

## Invariants

Property tests in `src/tests.rs` and the fuzz target `fuzz/fuzz_targets/qs_bsp.rs`
(`cargo xtask fuzz qs-bsp`), which writes the input over the spec's `base` vector so every
lump is reached:

- Arbitrary bytes never panic the reader, its accessors, the `.lit` reader or the entity
  writer (BSP-1 to BSP-19).
- Every face with light samples has them inside the lighting lump (BSP-10.2).
- A BSP written with new entities reads back with them (BSP-19.3).

## Differential result

The factory in `src/fixtures.rs` builds the spec's `base` vector byte for byte, and every
vector in [`vectors/bsp/`](../../docs/spec/vectors/bsp/README.md) gives the result its README
records for id's loader.

On 2026-10-16 id's `Mod_LoadBrushModel` and `Mod_LoadAliasModel`, in `WinQuake/model.c`
copied unchanged from the pinned commit and built with Apple clang on arm64 with
`-ffp-contract=off` (single precision without fused multiply-adds, as BSP-9.3 states), loaded
the 23 BSPs and 81 alias models of LibreQuake's `lite` release (the `librequake-lite` test
asset), and printed every plane, texture and animation link, texinfo, face extent and flag,
model bound and radius, hull 0 clipnode and visibility row. `qs-bsp` and `qs-mdl` printed the
same for all 104 files. Built with clang's default `-ffp-contract=on`, which fuses
multiply-adds on arm64, id's code gives 131 faces of nine maps an extent one 16-texel step
apart from single precision; the harness is not committed, as id's C never is.
