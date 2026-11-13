# vis test vectors

Vectors for [`../../vis.md`](../../vis.md). Each map is a BSP and its portal file, compiled by
`qs-map-bsp` from a `.map` of boxes written for these vectors (no game data): `<map>.bsp.txt` is
the BSP as an `xxd` hex dump (`xxd -r` rebuilds it), `<map>.prt.txt` the portal file.
[`results.tsv`](results.tsv) gives, for each map and each mode (`-fast`, and `-level 0` to
`-level 4`), what id's vis wrote: the visibility lump in hex, each vis leaf's `visofs`, and each
vis leaf's four ambient levels in hex, and the flow's counts as `vis -v` prints them: `c_chains`,
`c_vistest`, `c_mighttest`, `portalcheck`, `portaltest` and `portalpass` (VIS-4.8).

| Map | Leaves | Portals | What it holds |
| --- | --- | --- | --- |
| `snake` | 7 | 6 | Three rows of cells joined by columns two cells long, so no line goes from the first row to the last; `*water` on the last cell's floor |
| `pillars` | 17 | 35 | A room with a 3 by 3 grid of pillars and a low wall |
| `window` | 7 | 10 | Two rooms joined only by a window in a thick wall, a screen behind it |

**Checked:** on 2026-11-13 every map was run through id's vis: `qutils/VIS/VIS.C`, `FLOW.C`,
`SOUNDPVS.C`, `VIS.H` and `qutils/COMMON/CMDLIB.C`, `MATHLIB.C`, `BSPFILE.C` and their headers,
copied unchanged from id-Software/Quake-Tools@`c0d1b91c74eb654365ac7755bc837e497caaca73`, built
with the system C compiler (Apple clang, arm64) with `oracles/maps/compat.h` included first, as
`cargo xtask oracles maps` builds it. vis is used here as an oracle only
([`../../../porting.md`](../../../porting.md#upstreams)). `qs-map-vis` gives every row exactly
(`every_vector_matches_id_vis`).
