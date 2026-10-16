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
  and `world.c` at the commit pinned in [`docs/porting.md`](../../docs/porting.md). The caller
  names a `BspLoader`: `WinQuake` fails a face extent over 256 as `model.c` does, `GlQuake`
  one over 512 as `gl_model.c` does (BSP-9.15) and also gives every face a leaf that is not
  empty lists `SURF_UNDERWATER` (BSP-12.6). BSP2 follows QuakeSpasm's loader, cited for
  description only.
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
`-ffp-contract=off`, loaded the 23 BSPs and 81 alias models of LibreQuake's `lite` release
(the `librequake-lite` test asset), and printed every plane, texture and animation link,
texinfo, face extent and flag, model bound and radius, hull 0 clipnode and visibility row.
`qs-mdl` printed the same for all 81 models. The same day the BSP loader was built for x86-64
with `CalcSurfaceExtents` computing each texture coordinate with WinQuake's own x87
instructions at 53 bits and comparing as WinQuake does (BSP-9.11, BSP-9.12; the run is under
"Evidence for BSP-9.11 and BSP-9.12" in `docs/spec/bsp.md`), and `qs-bsp` printed the same
for all 23 maps; single-precision arithmetic would give 247 of their faces another
`texturemins` or extent. `texturemins` and extents are kept in 16 bits, as id's `short` fields
hold them (BSP-9.14); the vectors `extents-65552`, `extents-32784` and `texturemins-32768`
pin that against the arm64 and x86-64 builds, and `extents-512` and its neighbours pin
GLQuake's extent limit against `WinQuake/gl_model.c` built the same two ways (BSP-9.15). A
face with a negative stored extent has no lightmap (BSP-9.16): the vectors for it were run
through id's own renderer code, WinQuake's surface cache and GLQuake's lightmap builder,
neither of which draws light from such a face. The vectors `water-leaf` and its neighbours pin
GLQuake's `SURF_UNDERWATER` against both loaders built the same two ways (BSP-12.6). The harness is not committed, as id's C never
is.
