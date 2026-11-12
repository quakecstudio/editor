# qmap-bsp

Compiles a `.map` to a BSP, the portal file `vis` reads and the leak file, as id's `qbsp`
does: brushes loaded and grown for the clipping hulls, CSG, the BSP tree, portals, the outside
fill, face merging, T-junctions and the BSP written. `qbsp` is in `qutils/`, not a portable
upstream until the license review rules on it (P4-22), so it is described from its source in
[`qbsp.md`](../../docs/spec/qbsp.md) and run as an oracle; no code is ported from it.

- **`Compiler::compile`** takes the map's bytes and an optional WAD2 for the textures lump and
  gives a `Compiled`: the BSP, the portal file when the outside was filled, the leak file once
  an entity stands in empty space, and the warnings qbsp would have printed.
- **`Dialect::Id`** is qbsp exactly: its parser, its fixed-size tables and its errors. On the
  corpus it gives BSP and portal files byte-identical to qbsp's. Single-precision `vec_t`,
  double-precision epsilons, `Q_rint`, `VectorMA` and qbsp's linked-list orders are kept as the
  spec states them.
- **`Dialect::Extended`**, the default, reads Valve 220 maps and exact points, has no
  `MAX_MAP_` limit, warns where qbsp stops on a bad face, a leaf of mixed contents or an entity
  with no valid brush, and writes BSP29 when every value fits it, else `BSP2` (QBSP-14). Every
  other rule is the id dialect's.

- **Layer:** L4 bridges. It depends on `qs-map` to read the map, `qs-bsp` to write the BSP and
  `qs-wad` for the textures.
- **Spec sections:** QBSP-1 to QBSP-14 in [`qbsp.md`](../../docs/spec/qbsp.md); BSP-23 in
  [`bsp.md`](../../docs/spec/bsp.md) for the file it writes; MAP-1 to MAP-7 in
  [`map.md`](../../docs/spec/map.md) for what it reads.
- **Shipped:** yes.

## Differential result

A recorded local run on 2026-11-12: id's qbsp, built from `qutils/` at the commit
`qbsp.md` pins, and the id dialect each compiled 133 `.map` files, the LibreQuake maps
(`lq_*`, `lqdm*`, `b_*`, `prefab_*` and the rest) and the progs_dump test maps (`pd_*`), and
the outputs were compared with `cmp`. The 9 maps qbsp compiles gave byte-identical BSPs and
portal files, and the other 124 stopped with the error qbsp stopped with (qbsp leaves the
files it wrote before stopping, a portal file for one of them; the id dialect gives only the
error). The extended
dialect compiled all 133, 11 of them as `BSP2`. The map differential (P6-07) runs the same
comparison in CI.
