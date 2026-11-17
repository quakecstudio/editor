# qs-map-light

The light compiler: each face's lightmaps from the map's lights, written into the BSP's
lighting lump with each face's styles and offset, and coloured light in a `.lit` file beside
the BSP.

- **Layer:** L4 bridges (`layers.toml`).
- **Spec:** [`docs/spec/light.md`](../../docs/spec/light.md), LIGHT-1 to LIGHT-20, and
  [`docs/spec/bsp.md`](../../docs/spec/bsp.md), BSP-18 for the `.lit` file.
- **Dialects** (LIGHT-1.11), `qs-map-bsp`'s two:
  - **id:** id's `light` (LIGHT-1 to LIGHT-6), its lightmaps held to it byte for byte. The
    modules `entities`, `face`, `trace` and `lump`, written from the spec, which was written
    from `light`'s source at the pinned commit.
  - **extended**, the default: ericw-tools' `light` (LIGHT-8 to LIGHT-20, ADR-0027): sunlight
    with its penumbra, the sky dome, surface lights, minimum light, phong shading, dirt and
    bounce, the formulas of `delay`, and shadows from brush models, fences and glass. The
    `extended` module, ported from ericw-tools' `light/` at the commit `docs/porting.md` pins
    (ADR-0025). What it does not port is LIGHT-8.7.
- **Modes:** fast (one sample per luxel) and extra (`-extra`, four samples per luxel
  averaged), faces lit on every thread the machine has and written in face order, so the
  result is the one a single thread gives (LIGHT-1.5).
- **Beside the BSP:** `Light::run_with` takes what ericw-tools' qbsp writes for its `light`,
  `<map>.texinfo.json` and `<map>.content.json`, read by `Sidecars` (LIGHT-9.8, LIGHT-9.9),
  and the game's palette for the colour of glass and of bounced light.

`cargo xtask light-diff` (P7-01) is the differential. Every corpus map is lit by id's `light` and
by our id dialect in both modes, and the lightmaps are compared luxel by luxel. The extended
dialect is compared with ericw-tools' `light` on the same maps, compiled by ericw-tools' qbsp:
ericw-tools asks Embree what each ray meets, so the lightmaps are held to it within a level on
average and three luxels in a hundred past the tolerance (LIGHT-11.9).
