# qs-render

The wgpu renderer (P5-05, MAP-17): a BSP's world surfaces with their textures and lightmaps,
drawn as GLQuake draws them, inside egui's render pass through a paint callback (ADR-0003), so
the map view and the preview host share one renderer.

- **Layer:** L7 UI. It depends on `qs-bsp` and `qs-mdl` (L0), `egui-wgpu` and `epaint`; its examples also
  on `qs-vfs`, `qc-progs` and `qs-sim`.
- **Spec sections:** REND-1 to REND-4 of [`render.md`](../../docs/spec/render.md).
- **Ported from:** id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`,
  `WinQuake/gl_rsurf.c` (`BuildSurfaceDisplayList`, `GL_CreateSurfaceLightmap`,
  `R_BuildLightMap`, `R_DrawBrushModel`), `WinQuake/gl_rmain.c` (`R_RotateForEntity`); `R_AnimateLight`'s value for a style with no light string is from
  `WinQuake/gl_rlight.c`.
- **Shipped:** yes.

## Public shape

- `MapMesh::build(bsp, palette)`: the world model as vertices, a fan of triangles per face,
  batches by texture, the textures through `Palette`, and every face's light in one lightmap
  atlas (REND-1.1 to REND-1.6, REND-1.8).
- `MeshBuilder`: the same for several models of several BSPs, each `Placement` at its
  entity's origin and angles, such as a map's doors and lifts and its item boxes (REND-1.9).
- `AliasModel::read(name, bytes, palette)`: an `.mdl`'s skins, poses and triangles;
  `draw(entity, time, out)` appends an entity's shaded triangles for its frame and skin at a
  time (REND-4). `light_point(bsp, point)` is `R_LightPoint`, the light it is shaded by.
- `MapRenderer::new(device, queue, (format, depth), mesh, skins)`: the mesh and the alias
  skins on the GPU, and the pipelines; `set_view`, `set_models` and `draw`.
- `MapCallback`: an `egui_wgpu` paint callback that draws the `MapRenderer` kept in egui's
  callback resources with its view and a frame's alias models.
- `Camera`: a free camera in Quake's axes and its clip-space matrix.

## What it is not, yet

Lightstyles do not animate (every style is drawn at the value of a style with no light
string, REND-1.4); dynamic lights, sprites, particles, the view model, player colours and
`.lit` files are not drawn; BSP2 batching limits and picking are not done. These are the rest
of P5-05. Each model has its own matrix (`MapRenderer::set_placements`), so the viewer draws
doors and lifts where `qs-sim`'s server frames move them.

## Trying it

`examples/view.rs` draws a map from a Quake install, read through the game filesystem, with
the brush entities its `progs.dat` spawns through `qs-sim`:

```bash
cargo run --release -p qs-render --example view -- -basedir <game root> [-game <dir>] [map]
```

W, A, S and D move, Space and C rise and sink, Shift is fast, and dragging looks around.
