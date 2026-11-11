# qs-render

The wgpu renderer (P5-05, MAP-17): a BSP's world surfaces with their textures and lightmaps,
drawn as GLQuake draws them, inside egui's render pass through a paint callback (ADR-0003), so
the map view and the preview host share one renderer.

- **Layer:** L7 UI. It depends on `qs-bsp`, `qs-mdl` and `qs-wad` (L0), `qs-client` (L4),
  `egui-wgpu`, `epaint` and `image` (its TGA reader); its examples also on `qs-vfs`,
  `qc-progs` and `qs-sim`.
- **Spec sections:** REND-1 to REND-13 of
  [`render.md`](../../docs/spec/render.md); PART-3.2 of
  [`engine/particles.md`](../../docs/spec/engine/particles.md); the drawing of HUD-1 to HUD-3
  of [`engine/hud.md`](../../docs/spec/engine/hud.md), of VIEW-4.6, and of the console's
  background and the menus' fade (CON-3.2, MENU-1.3).
- **Ported from:** id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`,
  `WinQuake/gl_rsurf.c` (`BuildSurfaceDisplayList`, `GL_CreateSurfaceLightmap`,
  `R_BuildLightMap`, `R_AddDynamicLights`, `R_DrawBrushModel`), `WinQuake/gl_rlight.c`
  (`R_LightPoint`, `R_PushDlights`, `R_MarkLights`, `R_AnimateLight`), `WinQuake/gl_rmain.c`
  (`R_DrawAliasModel`, `R_DrawViewModel`, `R_PolyBlend`, `R_RotateForEntity`),
  `WinQuake/gl_rmisc.c` (`R_TranslatePlayerSkin`), `WinQuake/r_part.c` (`R_DrawParticles`),
  `WinQuake/gl_draw.c` (`Draw_Character`, `Draw_Pic`, `Draw_Fill`, `GL_Set2D`),
  `WinQuake/sbar.c` (`Sbar_DrawPic`); from
  sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`, `Quake/r_alias.c`
  (`GL_DrawAliasFrame`, the pose blend; `GLAlias_CreateShaders`, the MD5 shading),
  `Quake/r_sprite.c` (`R_GetSpriteFrame`, `R_DrawSpriteModel`) and `Quake/gl_fog.c`
  (`Fog_ParseWorldspawn`, `Fog_Update`, `Fog_GetColor`, `Fog_GetDensity`), `Quake/gl_sky.c`
  (`Sky_NewMap`, `Sky_LoadSkyBox`) and `Quake/image.c` (`Image_LoadImage`, `Image_LoadPCX`).
  Coloured light (REND-6) is QuakeSpasm's behaviour, described in the spec; none of its code is
  ported.
- **Shipped:** yes.

## Public shape

- `MapMesh::build(bsp, palette)`: the world model as vertices, a fan of triangles per face,
  batches by texture, the textures through `Palette`, and every face's light in one lightmap
  atlas (REND-1.1 to REND-1.6, REND-1.8). `MapMesh::relight(styles)` makes the atlas again
  when a style a face reads changes (REND-5.5), and `MapRenderer::set_lightmap` writes each
  256 by 256 block's changed rectangle, as QuakeSpasm uploads its lightmaps (REND-5.7).
- `LightStyles::at(strings, time)`: `R_AnimateLight`, every style's value from the strings the
  progs set with `lightstyle` (REND-5).
- `MeshBuilder::add_with(bsp, extras, placements)`: a model lit in colour by its map's
  `.lit` file (REND-6), and its textures each through a palette of its own where `Extras`
  gives one, as Half-Life's textures carry (MAP-24).
- `MeshBuilder`: the same for several models of several BSPs, each `Placement` at its
  entity's origin and angles, such as a map's doors and lifts and its item boxes (REND-1.9).
- `AliasModel::read(name, bytes, palette)`: an `.mdl`'s skins, poses and triangles;
  `draw(entity, time, out)` appends an entity's shaded triangles for its frame and skin at a
  time (REND-4). `light_point(bsp, point)` is `R_LightPoint`, the light it is shaded by.
- `MapMesh::relight_with(styles, lights)`: the atlas again with the frame's dynamic lights
  pushed through the BSP's nodes and added to the faces they reach, each face checked first as
  QuakeSpasm's `R_MarkLights` does (REND-7, REND-7.6).
- `Md5Model::read(name, bytes, read, palette, torch)`: a rerelease MD5 model, its frames'
  vertices from the joints and its skins from the shader names, drawn as an alias model;
  `Play` draws one in place of the `.mdl` it replaces (REND-12).
- `Look`: QuakeSpasm's look by default (palette as it is, trilinear mipmaps, glowing
  fullbright texels, overbright light, the view widened to the window) or GLQuake's, which
  `MeshBuilder::new` and `MapMesh::build` take and the renderer draws (REND-13).
- `SpriteModel::read(name, bytes, palette)`: a `.spr`'s frames; `draw(entity, time, eye,
  out)` appends the quad facing the eye by the sprite's type (REND-11).
- `particle_triangles(particles, palette, eye)`: a triangle per particle (PART-3.2).
- `MapRenderer::new(device, queue, (format, depth), mesh, assets)`: the mesh, the alias
  skins, the sprite frames and the 2D pictures on the GPU, and the pipelines: the world,
  blended water (REND-9), alias models and the view model in the near depth range
  (REND-4.11), alpha-tested sprites and decals, blended particles, and the 2D layer; every
  3D fragment fogged (REND-10.4).
- `MapCallback`: an `egui_wgpu` paint callback that draws the `MapRenderer` kept in egui's
  callback resources with its `View` (the matrix, the fog, the water alpha, the 2D layer's
  size) and a frame's models, sprites, particles and 2D quads, in `R_RenderView`'s order.
- `Play`: a `qs_client::Scene` and `Hud` turned into a `MapCallback`: the models the client
  precached (`load_models`), the pictures (`Pics`, from `gfx.wad` and the `.lmp` files), the
  map's fog (`FogState`), the view model's and the players' light floors, each player's skin
  in its slot's shirt and trouser colours (REND-4.13), and the status
  bar, the scoreboards and the centre print as quads.
- `Skybox::load(name, read)`: the six `gfx/env` faces a worldspawn's `sky` key names, as TGA
  or PCX (REND-8.2); `Play::load_sky` reads the map's, and `Assets::skybox` gives it to the
  renderer, which draws it through the sky faces (REND-8.3).
- `Offscreen`: a wgpu device with no window, and egui's frames, paint callbacks included,
  drawn into a texture with a depth buffer and read back as RGBA `Pixels`; the headless
  preview host's `screenshot` and the GPU tests draw with it (P3-05).
- `Camera`: a free camera in Quake's axes and its clip-space matrix, and
  `Camera::clip_from_view` for the client's view angles.

## What it is not, yet

`gl_flashblend` (REND-7.4, off in the FitzQuake lineage) is not drawn. Each model has its own
matrix (`MapRenderer::set_placements`), and a brush entity out of the client's visible list is
not drawn.

## Trying it

The preview host, `apps/quakec-engine`, plays a map from a Quake install with this renderer
drawing the client's scene and HUD; its README has the command line.
