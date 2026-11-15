# qs-map-vis

The visibility compiler: from the portal file `qs-map-bsp` writes, each leaf's potentially
visible set, compressed into the BSP's visibility lump, and each leaf's ambient sound levels, as
id's `vis` finds them.

- **Layer:** L4 bridges (`layers.toml`).
- **Spec:** [`docs/spec/vis.md`](../../docs/spec/vis.md), VIS-1 to VIS-7.
- **Portal files:** id's `PRT1`, numbered by leaf, and ericw-tools' `PRT2`, numbered by cluster
  of leaves, which `qs-map-bsp`'s extended dialect writes for a map with detail (VIS-7).
- **Modes:** fast (`-fast`: each portal sees what a flood through facing portals reaches) and
  full (the portal flow at test levels 0 to 4, 2 by default), on every thread the machine has,
  with the same result on any of them (VIS-4.7).

id's vis is not a portable upstream until the license review rules on it (`docs/porting.md`,
P4-22), so its flow is written from the spec, which was written from vis's source at the pinned
commit, read and run as an oracle (`cargo xtask oracles maps`). Reading `PRT2` and expanding its
clusters to leaves are ported from ericw-tools' `common/prtfile.cc` and `vis/vis.cc` (ADR-0025).
