# qs-render

The wgpu renderer (P5-05, MAP-17): a BSP's world surfaces with their textures and lightmaps,
drawn as GLQuake draws them, inside egui's render pass through a paint callback (ADR-0003), so
the map view and the preview host share one renderer.

- **Layer:** L7 UI. It depends on `qs-bsp` (L0), `egui-wgpu` and `epaint`.
- **Spec sections:** REND-1 of [`render.md`](../../docs/spec/render.md).
- **Ported from:** id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`,
  `WinQuake/gl_rsurf.c` (`BuildSurfaceDisplayList`, `GL_CreateSurfaceLightmap`,
  `R_BuildLightMap`); `R_AnimateLight`'s value for a style with no light string is from
  `WinQuake/gl_rlight.c`.
- **Shipped:** yes.

## Public shape

- `MapMesh::build(bsp, palette)`: the world model as vertices, a fan of triangles per face,
  batches by texture, the textures through `Palette`, and every face's light in one lightmap
  atlas (REND-1.1 to REND-1.6, REND-1.8).
- `MapRenderer::new(device, queue, (format, depth), mesh)`: the mesh on the GPU and its
  pipeline; `set_view` and `draw`.
- `MapCallback`: an `egui_wgpu` paint callback that draws the `MapRenderer` kept in egui's
  callback resources with its matrix.
- `Camera`: a free camera in Quake's axes and its clip-space matrix.

## What it is not, yet

Lightstyles do not animate (every style is drawn at the value of a style with no light
string, REND-1.4), the sky does not scroll and water does not warp (REND-1.6), and models,
`.lit` files, BSP2 batching limits and picking are not drawn or done. They are the rest of
P5-05 and P3-26.

## Trying it

`examples/view.rs` draws a map from a Quake install, read through the game filesystem:

```bash
cargo run --release -p qs-render --example view -- -basedir <game root> [-game <dir>] [map]
```

W, A, S and D move, Space and C rise and sink, Shift is fast, and dragging looks around.
