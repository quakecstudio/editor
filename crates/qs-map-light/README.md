# qs-map-light

The light compiler: each face's lightmaps from the map's light entities, as id's `light` finds
them, written into the BSP's lighting lump with each face's styles and offset, and coloured
light in a `.lit` file beside the BSP.

- **Layer:** L4 bridges (`layers.toml`).
- **Spec:** [`docs/spec/light.md`](../../docs/spec/light.md), LIGHT-1 to LIGHT-6, and
  [`docs/spec/bsp.md`](../../docs/spec/bsp.md), BSP-18 for the `.lit` file.
- **Modes:** fast (`light`'s default, one sample per luxel) and extra (`-extra`, four samples
  per luxel averaged), faces lit on every thread the machine has and written in face order, so
  the result is the one a single thread gives (LIGHT-1.5).

Nothing is ported: id's `light` is not a portable upstream until the license review rules on it
(`docs/porting.md`, P4-22), and ericw-tools is cited for its coloured light only. The code is
written from the spec, which was written from `light`'s source at the pinned commit, read and
run as an oracle (`cargo xtask oracles maps`).
