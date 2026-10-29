# Rendering

Prefix: `REND` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes how a BSP's world surfaces are drawn, as implemented by GLQuake's `gl_rsurf.c` in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`. Implemented by `qs-render`.

## REND-1 World surfaces

**Source:** `WinQuake/gl_rsurf.c`, `BuildSurfaceDisplayList`, `GL_CreateSurfaceLightmap`,
`R_BuildLightMap`, `R_BlendLightmaps` and `GL_BuildLightmaps`; `WinQuake/gl_rlight.c`,
`R_AnimateLight` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

GLQuake turns each face of the world model into a polygon with two sets of texture
coordinates, one for its texture and one for its place in a lightmap texture, and draws the
texture darkened by the lightmap. The map view and the preview host draw the world this way.

### Behaviour

- **REND-1.1** A face's polygon is its edges' vertices in order: for a surfedge `lindex > 0` the
  first vertex of edge `lindex`, otherwise the second vertex of edge `-lindex`
  (`BuildSurfaceDisplayList`). The renderer draws it as a triangle fan from its first vertex.
- **REND-1.2** A vertex's texture coordinates are `s = DotProduct (vec, vecs[0]) + vecs[0][3]`
  divided by the texture's width, and `t` the same with `vecs[1]` and the height
  (`BuildSurfaceDisplayList`). A face whose texinfo names no texture is not drawn.
- **REND-1.3** A vertex's lightmap coordinates are `(s - texturemins[0] + light_s*16 + 8) /
  (BLOCK_WIDTH*16)` and `(t - texturemins[1] + light_t*16 + 8) / (BLOCK_HEIGHT*16)`, where
  `light_s` and `light_t` are the face's texel position in its lightmap texture
  (`BuildSurfaceDisplayList`). The face takes `smax = (extents[0]>>4)+1` by
  `tmax = (extents[1]>>4)+1` texels (`GL_CreateSurfaceLightmap`). GLQuake packs faces into
  128 by 128 blocks; this renderer packs them into one texture, whose width and height take
  the place of `BLOCK_WIDTH` and `BLOCK_HEIGHT`, which keeps the mapping from a face's texels
  to its surface.
- **REND-1.4** A lightmap texel is `t = (sum of sample * scale) >> 7`, capped at 255, summed over
  the face's styles until one is 255, at most four, `scale` being the style's value
  (`R_BuildLightMap`). A style is 256 while no light string is set for it, and otherwise
  `(letter - 'a') * 22` for the letter its string has at the time, so `m`, "normal light", is
  264 (`gl_rlight.c`, `R_AnimateLight`). This renderer draws a map before any light string
  is set, so every style is 256, until lightstyles animate (P3-26).
- **REND-1.5** A face with no samples (`lightofs` of -1) has every texel 0, so it is drawn black;
  in a map with no lighting lump every texel is 255 (`R_BuildLightMap`: "set to full bright if
  no light data").
- **REND-1.6** A sky or turbulent face (`SURF_DRAWSKY`, `SURF_DRAWTURB`) has no lightmap
  (`GL_CreateSurfaceLightmap` returns first) and is drawn at its texture's colour. This
  renderer draws both as plain textured faces; the sky's scroll and the water's warp are
  P3-26.
- **REND-1.7** The surface's colour is its texture's colour times `t / 255`: GLQuake stores
  `255 - t` in its default luminance lightmaps (`GL_BuildLightmaps`: `gl_lightmap_format =
  GL_LUMINANCE`) and blends them with `glBlendFunc (GL_ZERO, GL_ONE_MINUS_SRC_COLOR)`
  (`R_BlendLightmaps`).
- **REND-1.8** A texture's pixels are indices into the palette, `gfx/palette.lmp`: 256 colours
  of three bytes, red, green and blue. A palette of any other size is refused.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `REND_LIGHT_SHIFT` | 7 | `WinQuake/gl_rsurf.c`, `t >>= 7` in `R_BuildLightMap` |
| `REND_UNSET_STYLE` | 256 | `WinQuake/gl_rlight.c`, `d_lightstylevalue[j] = 256` in `R_AnimateLight` for a style with no string |
| `REND_LIGHTMAP_TEXEL` | 16 | `WinQuake/gl_rsurf.c`, `light_s*16` and `(extents[0]>>4)+1` |
