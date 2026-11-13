# Rendering

Prefix: `REND` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes how a BSP's world surfaces are drawn, as implemented by GLQuake's `gl_rsurf.c` in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`. Implemented by `qs-render`.

## REND-1 World surfaces

**Source:** `WinQuake/gl_rsurf.c`, `BuildSurfaceDisplayList`, `GL_CreateSurfaceLightmap`,
`R_BuildLightMap`, `R_BlendLightmaps`, `GL_BuildLightmaps` and `R_DrawBrushModel`;
`WinQuake/gl_rmain.c`, `R_RotateForEntity`; `WinQuake/gl_rlight.c`, `R_AnimateLight` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

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
- **REND-1.4** **Withdrawn** 2026-10-30: it held every style at 256 until lightstyles animate,
  which they now do; replaced by REND-1.13 and REND-5. A lightmap texel is `t = (sum of sample * scale) >> 7`, capped at 255, summed over
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
- **REND-1.9** A brush entity's model (a submodel `*n` of the map, or model 0 of a BSP of its
  own, such as `maps/b_bh25.bsp`) is drawn with its own faces moved to the entity: `R_DrawBrushModel`
  negates `angles[0]`, calls `R_RotateForEntity` and negates it back, and `R_RotateForEntity` is
  `glTranslatef (e->origin)`, `glRotatef (e->angles[1], 0, 0, 1)`,
  `glRotatef (-e->angles[0], 0, 1, 0)`, `glRotatef (e->angles[2], 1, 0, 0)`. A corner is
  therefore turned by roll about x, then by `+angles[0]` about y, then by yaw about z, then
  moved by the origin. Its texture and lightmap coordinates are its own, unmoved.
- **REND-1.10** Only the front of a face is drawn: `R_SetupGL` sets `glCullFace (GL_FRONT)` and
  enables `GL_CULL_FACE` when `gl_cull` is set, as it is by default, with GL's
  counter-clockwise front face, which keeps a polygon whose corners run clockwise on screen, a
  Quake face seen from the side its plane faces. The world walk and `R_DrawBrushModel` test
  the side too (`SURF_PLANEBACK`), except that the walk does not for a `SURF_UNDERWATER`
  surface, which the culling still removes. So a brush entity's face flush against the world's
  back to back, such as a door's underside on a floor, is never drawn.
- **REND-1.11** The world is drawn before the entities (`R_DrawWorld`, then
  `R_DrawEntitiesOnList` in `R_RenderScene`), with `glDepthFunc (GL_LEQUAL)`
  (`R_Clear`): of a brush entity's face and a world face at the same depth, such as a lowered
  lift's top flush with the floor, the entity's is seen.
- **REND-1.12** GLQuake draws such a pair as it rasterizes them, so where their depths differ
  by rounding they flicker. QuakeSpasm (`sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`,
  `Quake/r_brush.c`, `R_DrawBrushModel`) has the option `gl_zfix`, off by default
  (`Quake/gl_rmain.c`, `cvar_t gl_zfix = {"gl_zfix", "0", CVAR_NONE}`), which before
  `R_RotateForEntity` moves the entity's origin by `-DIST_EPSILON` on each axis and after it
  moves it back, `DIST_EPSILON` being `(0.03125)` (`Quake/quakedef.h`). The renderer offers the
  same; the viewer turns it on unless asked for GLQuake's drawing, since it removes a visible
  flicker the map's author did not intend.
- **REND-1.13** A lightmap texel is `t = (sum of sample * scale) >> 7`, capped at 255, summed
  over the face's styles until one is 255, at most four, `scale` being the style's value at
  the time (`R_BuildLightMap`; REND-5 gives the values and the arithmetic).

## REND-2 Moving textures

**Source:** `WinQuake/gl_rsurf.c`, `R_TextureAnimation`; `WinQuake/gl_warp.c`,
`SubdividePolygon`, `EmitWaterPolys`, `EmitSkyPolys`, `R_DrawSkyChain` and `R_InitSky`;
`WinQuake/gl_warp_sin.h` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

Some textures change over time: animated textures step through their frames, water and other
turbulent surfaces ripple, and the sky's two layers scroll.

### Behaviour

- **REND-2.1** A texture with frames (`anim_total` above 0) is drawn as the frame for
  `reletive = (int)(cl.time*10) % anim_total`: from the base, `anim_next` is followed while
  `anim_min > reletive || anim_max <= reletive` (`R_TextureAnimation`). The world is drawn
  with frame 0, so its `alternate_anims` are not used.
- **REND-2.2** A turbulent face's texture coordinates are the raw `s = DotProduct (verts,
  vecs[0])` and `t` the same with `vecs[1]`, without the offset `vecs[n][3]`
  (`SubdividePolygon`), and are drawn as `(s + turbsin[(int)((t*0.125+realtime) * TURBSCALE) &
  255]) / 64` and `(t + turbsin[(int)((s*0.125+realtime) * TURBSCALE) & 255]) / 64`
  (`EmitWaterPolys`), where `turbsin[i]` is `8 sin(2πi/256)` and `TURBSCALE` is
  `256 / (2π)`, so the index is the angle `t*0.125 + realtime` in radians.
- **REND-2.3** The sky texture is 256 by 128, cut into two 128 by 128 layers that each repeat:
  its right half is the solid layer and its left half the alpha layer, where colour 0 is
  transparent and takes the solid layer's average colour, "to avoid a fringe on the top level"
  when the layers are filtered (`R_InitSky`). Each layer's
  coordinates for a point `v` seen from the eye `r_origin` are: `dir = v - r_origin` with
  `dir[2]` tripled, `length = 6*63 / |dir|`, and `s = (speedscale + dir[0]*length) / 128`,
  `t = (speedscale + dir[1]*length) / 128`, where `speedscale` is `realtime*8` for the solid
  layer and `realtime*16` for the alpha layer, less `(int)speedscale & ~127`. The alpha layer
  is blended over the solid one (`EmitSkyPolys`, `R_DrawSkyChain`).
- **REND-2.4** **Withdrawn** 2026-11-02: it said GLQuake cuts the faces at 64-unit
  boundaries; the size is `gl_subdivide_size`, 128 by default (`gl_model.c`). Replaced by
  REND-2.5.
- **REND-2.5** GLQuake computes REND-2.2 and REND-2.3 at the vertices of faces it cuts at
  multiples of `gl_subdivide_size` (`REND_SUBDIVIDE_SIZE`, 128 by default) on each axis
  (`GL_SubdivideSurface`, `SubdividePolygon`), with `turbsin` indexed in 256 steps. This
  renderer computes the same formulas for each pixel, with `8 sin` of the angle itself: a
  deliberate divergence, which draws the warp and the sky without the subdivision and gives
  each pixel the value GLQuake gives the vertices near it.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `REND_SUBDIVIDE_SIZE` | 128 | `gl_model.c`, `gl_subdivide_size` default |
| `REND_TURBSCALE` | 256 / (2 pi) | `gl_warp.c`, `TURBSCALE` |
| `REND_TURB_AMPLITUDE` | 8 | `gl_warp_sin.h` |
| `REND_SKY_SOLID_SPEED` | 8 | `gl_warp.c`, `EmitBothSkyLayers` |
| `REND_SKY_CLOUD_SPEED` | 16 | `gl_warp.c`, `EmitBothSkyLayers` |
| `REND_SKY_SCALE` | 378 | `gl_warp.c`, `EmitSkyPolys` (`6 * 63`) |

## REND-3 Colour, filtering and the view

**Source:** `WinQuake/gl_vidnt.c`, `Check_Gamma`; `WinQuake/gl_draw.c`, `GL_Upload32`,
`GL_ResampleTexture`, `GL_MipMap`, `gl_filter_min` and `gl_filter_max`; `WinQuake/gl_model.c`,
`Mod_LoadTextures`; `WinQuake/gl_screen.c`, `CalcFov` and `scr_fov`; `WinQuake/gl_rmain.c`,
`R_SetupGL` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

How GLQuake turns the palette into colours, filters its textures and sets up its view, which
decide how bright and how smooth a frame looks.

### Behaviour

- **REND-3.1** Each byte of the palette is corrected before any texture is made from it:
  `f = pow ((pal[i]+1)/256.0, vid_gamma)` and the byte becomes `f*255 + 0.5`, kept within 0 to
  255 (`Check_Gamma`). `vid_gamma` is `-gamma`'s value when it is given, else 1 on a 3Dfx
  card and 0.7 on any other ("default to 0.7 on non-3dfx hardware"). This renderer runs on no
  3Dfx card, so its gamma is 0.7 unless the caller gives another. The lightmaps are not
  corrected.
- **REND-3.2** A world texture is loaded with mipmaps (`GL_LoadTexture (..., true, false)` in
  `Mod_LoadTextures`): its width and height are each raised to the next power of two and the
  image point-sampled to that size (`GL_ResampleTexture`), then each smaller level, down to 1
  by 1, is the average of each 2 by 2 block of the one above with `(a+b+c+d)>>2` per channel
  (`GL_MipMap`). A level whose height is 1 averages no rows and keeps the bytes it starts with,
  as `GL_MipMap`'s `height >>= 1` loop does.
- **REND-3.3** World textures are drawn with `gl_filter_min = GL_LINEAR_MIPMAP_NEAREST` and
  `gl_filter_max = GL_LINEAR`: linear within the nearest mip level. The sky's layers and the
  lightmaps are drawn `GL_LINEAR` with no mipmaps (`R_InitSky`, `GL_BuildLightmaps`).
- **REND-3.4** The view's field of view is `fov` 90 degrees across, and its height
  `CalcFov (fov_x, width, height)`: `x = width/tan(fov_x/360*M_PI)`, `a = atan (height/x)`, in
  degrees `a*360/M_PI`. It draws from 4 to 4096 units (`MYgluPerspective (r_refdef.fov_y,
  screenaspect, 4, 4096)` in `R_SetupGL`).

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `REND_LIGHT_SHIFT` | 7 | `WinQuake/gl_rsurf.c`, `t >>= 7` in `R_BuildLightMap` |
| `REND_UNSET_STYLE` | 256 | `WinQuake/gl_rlight.c`, `d_lightstylevalue[j] = 256` in `R_AnimateLight` for a style with no string |
| `REND_ANIM_RATE` | 10 frames a second | `WinQuake/gl_rsurf.c`, `(int)(cl.time*10)` in `R_TextureAnimation` |
| `REND_TURB_AMPLITUDE` | 8 | `WinQuake/gl_warp_sin.h`, the peak of `turbsin` |
| `REND_GAMMA` | 0.7 | `WinQuake/gl_vidnt.c`, `vid_gamma = 0.7` in `Check_Gamma` |
| `REND_FOV_X` | 90 | `WinQuake/gl_screen.c`, `scr_fov = {"fov","90"}` |
| `REND_NEAR`, `REND_FAR` | 4, 4096 | `WinQuake/gl_rmain.c`, `MYgluPerspective (..., 4, 4096)` in `R_SetupGL` |
| `REND_LIGHTMAP_TEXEL` | 16 | `WinQuake/gl_rsurf.c`, `light_s*16` and `(extents[0]>>4)+1` |

## REND-4 Alias models

**Source:** `WinQuake/gl_rmain.c`, `R_DrawAliasModel`, `R_SetupAliasFrame`,
`GL_DrawAliasFrame`, `R_DrawViewModel` and `R_RotateForEntity`; `WinQuake/gl_model.c`,
`Mod_FloodFillSkin`, `Mod_LoadAllSkins` and `Mod_LoadAliasGroup`; `WinQuake/gl_mesh.c`,
`BuildTris`; `WinQuake/gl_rlight.c`, `R_LightPoint` and `RecursiveLightPoint`;
`WinQuake/anorm_dots.h`; `WinQuake/cl_main.c`, `CL_RelinkEntities`; `WinQuake/mathlib.c`,
`anglemod`; `WinQuake/gl_vidnt.c`, `VID_SetPalette`; `WinQuake/gl_rmisc.c`,
`R_TranslatePlayerSkin`; `WinQuake/cl_parse.c`, `CL_ParseUpdate` and `CL_NewTranslation`;
`WinQuake/render.h`, `TOP_RANGE` and `BOTTOM_RANGE`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`). The colour clamp is OpenGL 1.1,
section 2.13.6. The pose blend is `Quake/r_alias.c`, `GL_DrawAliasFrame`
(sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`).

GLQuake draws an alias model (`.mdl`: monsters, weapons, items, torches) as one pose of
triangles, turned and moved to its entity, its skin modulated by a shade per vertex from the
world's light below the entity.

### Behaviour

- **REND-4.1** Before each skin, and each frame of a skin group, is uploaded,
  `Mod_FloodFillSkin` runs on `skin`, the pixels after the *first* skin's type, never on the
  skin being uploaded: `skin = (byte *)(pskintype + 1)` is set once and every call passes it.
  It fills the region of `fillcolor = skin[0]` joined to pixel (0, 0), breadth first through a
  4096-entry ring (`FLOODFILL_FIFO_SIZE`) that ends when its read and write positions meet, so an
  overflow ends the fill early. Each queued pixel is marked 255; each pixel taken from the ring
  becomes the last neighbour colour that is neither `fillcolor` nor 255 seen in the order
  left, right, up, down, else `filledcolor`. `filledcolor` is the first index whose
  `d_8to24table` entry equals `255 << 0`; `VID_SetPalette` makes every entry
  `(255<<24) + r + (g<<8) + (b<<16)` and clears entry 255's alpha, so only entry 255, when its
  colour is (255, 0, 0), can match, and otherwise `filledcolor` is 0. Nothing is filled when
  `fillcolor` is `filledcolor` or 255.
- **REND-4.2** A skin has four slots, `gl_texturenum[i][4]`. A single skin fills all four. A
  group's frame `j` goes in slot `j&3`, and after the loop
  `for (; j < 4; j++) gl_texturenum[i][j&3] = gl_texturenum[i][j - k]` with `k` the group's
  count. The slot drawn is `anim = (int)(cl.time*10) & 3`. GLQuake does not check the entity's
  `skinnum`; a skin the model does not have is drawn as skin 0 here, our choice.
- **REND-4.3** A corner's skin coordinates are `s = stverts[k].s`, plus `skinwidth / 2` when the
  vertex is on the seam and the triangle does not face front, then `(s + 0.5) / skinwidth` and
  `(t + 0.5) / skinheight` (`BuildTris`). Strips join only triangles with the same
  `facesfront`, so this is the triangle's own. A skin is uploaded as a world texture is,
  `GL_LoadTexture (..., true, false)`: palette, resampling, mipmaps and filtering as REND-3.2
  and REND-3.3, every pixel opaque.
- **REND-4.4** A frame entry is one pose, or a group of poses whose `interval` is the group's
  *first* interval (`Mod_LoadAliasGroup`). The pose drawn is the frame's first, plus
  `(int)(cl.time / interval) % numposes` when it has more than one. A frame the model does not
  have draws frame 0 ("no such frame"). An interval of 0 or less is refused when the model
  loads (`Mod_LoadAliasGroup: interval<=0`); a NaN one, which loads, holds the first pose here,
  our choice: id's int conversion of a NaN quotient is undefined.
- **REND-4.5** A vertex is at `v * scale + scale_origin` in the model (`glTranslatef
  (scale_origin)`, `glScalef (scale)`), turned and moved by `R_RotateForEntity`: roll about x,
  then `-angles[0]` about y, then yaw about z, then the origin. Unlike a brush model
  (REND-1.9) the pitch is not negated first.
- **REND-4.6** **Withdrawn** 2026-10-30: it held every style at `REND_UNSET_STYLE`, which
  lightstyles now replace with their values; replaced by REND-4.10 and REND-5.6.
  `R_LightPoint (p)` is 255 in a map with no lighting lump. Otherwise it follows
  the line from `p` to 2048 units below it through the world's nodes from node 0: on a node it
  goes down the side `front < 0` of `start`, and when `end` is on the same side, only there;
  else to the crossing `mid = start + (end - start) * front/(front-back)`, then the node's faces,
  then the far side from `mid`. A face drawn tiled (sky, water) is skipped. For the others,
  `s` and `t` are the texinfo's dot products at `mid` with their offsets, truncated to ints; a
  face is hit when `s - texturemins[0]` and `t - texturemins[1]` are each from 0 to its
  extents. A face hit with no samples gives 0; one with samples gives
  `(sum over its styles of sample * style value) >> 8` at sample `(dt>>4) *
  ((extents[0]>>4)+1) + (ds>>4)`, every style at `REND_UNSET_STYLE`. A leaf hit with nothing
  gives 0.
- **REND-4.7** `ambientlight` and `shadelight` start as `R_LightPoint (origin)`. `ambientlight`
  is capped at 128, and when `ambientlight + shadelight > 192`, `shadelight` becomes
  `192 - ambientlight`. Then `progs/flame2.mdl` and `progs/flame.mdl` are set to 256 ("make
  torches full light"), and `shadelight` is divided by 200. The view model's floor of 24, the
  players' floor of 8 and the dynamic lights come before the caps: REND-4.11.
- **REND-4.8** The shade of a vertex is `shadedots[lightnormalindex] * shadelight`, with
  `shadedots` the row `((int)(angles[1] * (SHADEDOT_QUANT / 360.0))) & (SHADEDOT_QUANT - 1)` of
  `r_avertexnormal_dots`, 16 rows of 256 entries. It is given to `glColor3f`, so it is clamped
  to 0 to 1 before it is interpolated: OpenGL 1.1, 2.13.6, "After lighting (whether enabled or
  not), RGBA colors are clamped to the range [0, 1]." It is interpolated across the triangle
  (`gl_smoothmodels`, `GL_SMOOTH`), and the skin's colour is multiplied by it (`GL_MODULATE`).
- **REND-4.9** A model whose header has `EF_ROTATE` (items and weapons) is drawn with yaw
  `bobjrotate = anglemod(100*cl.time)` (`CL_RelinkEntities`), with `anglemod (a)` of the float
  `a` being `(360.0/65536) * ((int)(a*(65536/360.0)) & 65535)`.
- **REND-4.11** Before REND-4.7's caps, `R_DrawAliasModel` raises the view model's
  (`cl.viewent`) `ambientlight` and `shadelight` to 24 when below it; then for every live
  dynamic light (`die >= cl.time`) adds `add = radius - |origin - light origin|` to both when
  `add > 0`; `ambientlight` and `shadelight` are `gl_rmain.c`'s file-scope floats (the `int`
  pair in `R_DrawViewModel` is unused), so nothing is truncated; then raises a player's
  (entity 1 to `cl.maxclients`) to 8 when below it. `R_DrawViewModel` draws nothing
  while `cl.items & IT_INVISIBILITY` or `cl.stats[STAT_HEALTH] <= 0`, and draws the view model
  with `glDepthRange (gldepthmin, gldepthmin + 0.3*(gldepthmax-gldepthmin))`, the nearest 0.3
  of the depth range, after the scene and before the water ("hack the depth range to prevent
  view model from poking into walls").
- **REND-4.12** QuakeSpasm's `GL_DrawAliasFrame` blends two poses when they differ: a vertex
  is `verts1->v * iblend + verts2->v * blend` with `blend` the entity's lerp fraction
  (CLIENT-7.3) and `iblend = 1 - blend`, and its shade dot `shadedots[n1] * iblend +
  shadedots[n2] * blend` from the two poses' normals; equal poses draw the first with no
  blend. The preview host draws the blend; GLQuake (REND-4.4) is the `blend` 0 case.

- **REND-4.10** `R_LightPoint (p)` is 255 in a map with no lighting lump. Otherwise it follows
  the line from `p` to 2048 units below it through the world's nodes from node 0: on a node it
  goes down the side `front < 0` of `start`, and when `end` is on the same side, only there;
  else to the crossing `mid = start + (end - start) * front/(front-back)`, then the node's faces,
  then the far side from `mid`. A face drawn tiled (sky, water) is skipped. For the others,
  `s` and `t` are the texinfo's dot products at `mid` with their offsets, truncated to ints; a
  face is hit when `s - texturemins[0]` and `t - texturemins[1]` are each from 0 to its
  extents. A face hit with no samples gives 0; one with samples gives
  `(sum over its styles of sample * style value) >> 8` at sample `(dt>>4) *
  ((extents[0]>>4)+1) + (ds>>4)`, the style values as REND-5.6 says. A leaf hit with nothing
  gives 0.
- **REND-4.13** `R_TranslatePlayerSkin (playernum)` (`gl_rmisc.c`) builds an identity table,
  then for `i` in 0 to 15 sets `translate[TOP_RANGE+i]` to `top+i` and
  `translate[BOTTOM_RANGE+i]` to `bottom+i`, or `top+15-i` and `bottom+15-i` for a row of 128 or
  more ("the artists made some backwards ranges"), with `top = colors & 0xf0`, `bottom = (colors
  &15)<<4`, `colors` the slot's `cl.scores[playernum].colors`, and `TOP_RANGE` 16 and
  `BOTTOM_RANGE` 96 (`render.h`). It translates `texels[skinnum]` of
  `cl_entities[1+playernum]`'s alias model, the 8-bit pixels after REND-4.1's flood fill and a
  group's first image, or `texels[0]` for a skin number out of range, into `playertextures +
  playernum`. `CL_ParseUpdate` calls it when a player's model or skin number changes and
  `CL_NewTranslation` when its colours do (`cl_parse.c`). `R_DrawAliasModel` binds
  `playertextures - 1 + i` instead of the skin for entity `i` from 1 to `cl.maxclients` when its
  colormap is not `vid.colormap`, that is, a nonzero colormap ("Heads are just uncolored"). The
  renderer keeps one translated skin per slot, up to 16, remade when the entity's model, skin or
  colours change.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `REND_SHADEDOT_QUANT` | 16 | `WinQuake/gl_rmain.c`, `#define SHADEDOT_QUANT 16` |
| `REND_SHADE_SCALE` | 200 | `WinQuake/gl_rmain.c`, `shadelight = shadelight / 200.0` in `R_DrawAliasModel` |
| `REND_LIGHT_DEPTH` | 2048 | `WinQuake/gl_rlight.c`, `end[2] = p[2] - 2048` in `R_LightPoint` |
| `REND_ROTATE_RATE` | 100 degrees a second | `WinQuake/cl_main.c`, `anglemod(100*cl.time)` in `CL_RelinkEntities` |

## REND-5 Light styles

**Source:** `WinQuake/gl_rlight.c`, `R_AnimateLight` and `R_LightPoint`; `WinQuake/gl_rsurf.c`,
`R_BuildLightMap` and `R_RenderBrushPoly`; `WinQuake/r_main.c`, `d_lightstylevalue`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

A face's lightmaps are one per light style it lists. The progs sets each style's string with
`lightstyle`, a letter a tenth of a second; GLQuake turns the letters into a scale for each
style every frame and rebuilds the lightmaps of the faces whose styles changed.

### Behaviour

- **REND-5.1** There are 64 styles (`MAX_LIGHTSTYLES`). A face lists up to four, ended by 255
  (`MAXLIGHTMAPS`).
- **REND-5.2** A style with no string, or an empty one (`!cl_lightstyle[j].length`), is 256.
- **REND-5.3** Otherwise its value at time `cl.time` is `(map[k] - 'a') * 22` with
  `k = (int)(cl.time*10) % length`, so `a` is 0, `m` ("normal light") 264 and `z` ("double
  bright") 550. A letter before `a` gives a negative value, as id's code does. A negative time
  makes id's `k` negative and reads before the string; this renderer reads the first letter
  then, our choice, since the result in id's code is undefined.
- **REND-5.4** `d_lightstylevalue` is an `int` and `R_BuildLightMap`'s `scale` an `unsigned`, so
  the products and their sum are unsigned and wrap. The sum is then read into the signed `int t`,
  shifted right by 7 and capped at 255; GLQuake's luminance texture keeps `255 - t` as a byte,
  so the light drawn is the low byte of `t` when `t` is below 256, and 255 above it.
- **REND-5.5** GLQuake rebuilds a face's lightmap when one of its styles' values differs from
  the value it was built with (`R_RenderBrushPoly`: `d_lightstylevalue[fa->styles[maps]] !=
  fa->cached_light[maps]`). This renderer builds the faces whose styles changed again in the
  atlas; the texels are the same.
- **REND-5.6** `R_LightPoint` sums `sample * scale` with the same style values into the `int r`,
  so its products wrap the same way, and shifts it right by 8 (`RecursiveLightPoint`).
- **REND-5.7** Faces made again reach the GPU as QuakeSpasm uploads its lightmaps
  (`R_UploadLightmap`, Quake/r_brush.c at
  sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`): each block grows a rectangle,
  `lm->rectchange`, to every face built in it, and only that is written,
  "`glTexSubImage2D(GL_TEXTURE_2D, 0, 0, lm->rectchange.t, LMBLOCK_WIDTH, lm->rectchange.h,
  ...)`", with `LMBLOCK_WIDTH` and `LMBLOCK_HEIGHT` 256 (Quake/glquake.h). This renderer keeps
  one atlas cut into 256 by 256 blocks; a face counts in the block of its top left texel, and
  each block's rectangle, clipped to the atlas, is written once a frame, its columns as well as
  its rows, where QuakeSpasm writes the block's full width. The texels drawn are the same.

## REND-6 Coloured light

**Source:** `Quake/gl_model.c`, `Mod_LoadLighting`, and `Quake/r_brush.c`, `R_BuildLightMap`
(sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`). A `.lit` file is read as BSP-18
says.

QuakeSpasm, through LordHavoc's `.lit` support, lights a map in colour when a `.lit` file sits
beside it: a red, green and blue sample for each byte of the lighting lump.

### Behaviour

- **REND-6.1** With a `.lit` file the samples of a face are its triples, from `lightdata +
  (lofs * 3)` ("lit support via lordhavoc"); without one, each byte of the lighting lump is
  its own red, green and blue ("expand the white lighting data to color").
- **REND-6.2** Each channel is summed as REND-1.13 sums a texel, into an `unsigned` that wraps,
  shifted right by 7 (`r = *bl++ >> 7`) and capped at 255. The renderer draws a map without a
  `.lit` file as GLQuake does (REND-5.4), and one with it as QuakeSpasm does.

## REND-7 Dynamic lights

**Source:** `WinQuake/gl_rlight.c`, `R_PushDlights`, `R_MarkLights`, `R_RenderDlights`,
`R_RenderDlight`, `AddLightBlend`; `WinQuake/gl_rsurf.c`, `R_AddDynamicLights`,
`R_BuildLightMap`, `R_DrawBrushModel` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

The client's dynamic lights (CLIENT-5.5, CLIENT-5.7) light the faces near them for a frame.

### Behaviour

- **REND-7.1** Each frame, before the view is drawn, every live light (`die` not before
  `cl.time`, radius not 0) is pushed through the world's nodes from the root: at a node, with
  `dist` the light's origin against the node's plane, only the front child is visited when
  `dist > radius` and only the back child when `dist < -radius`; otherwise every face of the
  node gets the light's bit (`1 << i`) for this frame, and both children are visited. A leaf
  ends the walk.
- **REND-7.2** A face with light bits this frame has its lightmap rebuilt with the lights
  added (`R_AddDynamicLights`): for each marked light, `rad = radius - |dist|` with `dist`
  the light's origin against the face's plane, and a light with `rad < minlight` (the light's
  `minlight`) adds nothing; else `minlight = rad - minlight`, the light's impact on the plane
  is `origin - normal * dist`, and `local` is its texture coordinates less the face's
  `texturemins`. For each lightmap texel `(s, t)`, with `sd = |local.s - 16 s|` and `td =
  |local.t - 16 t|` as ints, the distance is `sd + (td >> 1)` when `sd > td` and `td + (sd >>
  1)` otherwise; a texel with `dist < minlight` gets `(rad - dist) * 256` added to its
  `blocklights` sum, which REND-5.4 then shifts and caps.
- **REND-7.3** `R_LightPoint` (REND-5.6) is not changed by dynamic lights.
- **REND-7.4** With `gl_flashblend` (1 by default in GLQuake, off in the FitzQuake lineage
  and off here by ADR-0012's rule, since the lineage lights the lightmaps instead) a light is
  drawn as a fan of 16 triangles of radius `radius * 0.35` facing the view, additive, with
  the centre `(0.2, 0.1, 0)` and the rim black, and no face is marked; a light the eye is
  inside adds `(1, 0.5, 0)` at `radius * 0.0003` to the view blend (VIEW-4.6).
- **REND-7.5** `R_DrawBrushModel` marks a brush entity's own faces the same way (REND-7.1)
  from its model's `hulls[0].firstclipnode`, the model's first node, when the model's
  `firstmodelsurface` is not 0, with the light's origin as it is in the world: the planes are
  the model's own, so a moved entity is lit as if it stood where its model was built.
- **REND-7.6** As QuakeSpasm's `R_MarkLights` (Quake/gl_rlight.c at
  sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`, from FitzQuake) does, a face of
  a node the sphere straddles (REND-7.1) is marked only when the light reaches it: with `impact
  = origin - normal * dist`, "`l = DotProduct (impact, surf->texinfo->vecs[0]) +
  surf->texinfo->vecs[0][3] - surf->texturemins[0]; s = l+0.5;if (s < 0) s = 0;else if (s >
  surf->extents[0]) s = surf->extents[0]; s = l - s;`", `t` the same on the second axis, each
  assigned to an int toward zero, and the face marked when "`(s*s+t*t+dist*dist) < maxdist`",
  `maxdist` the radius squared. The squares are summed in a double here, where C's ints would
  overflow for a light some 46,000 texture units off, our choice. id's code marks every face of
  the node. A face this skips is one REND-7.2 adds no light to, since REND-7.2's texel distance
  is never less than the straight one, except within a texture unit of the radius, where the two
  truncations to ints can differ; fewer faces are made again.
- **REND-7.7** The renderer takes the first 64 of the client's lights, QuakeSpasm's
  `MAX_DLIGHTS` (Quake/client.h at sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`:
  "`#define MAX_DLIGHTS 64 //johnfitz -- was 32`"), each face keeping a bit for each
  (Quake/gl_model.h: "`unsigned int dlightbits[(MAX_DLIGHTS + 31) >> 5];`"), where id's keeps 32
  in one `int`. A client with id's limits (CLIENT-8.2) gives it at most 32.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `REND_MAX_DLIGHTS` | 64 | QuakeSpasm's `Quake/client.h`, `MAX_DLIGHTS` ("64 //johnfitz -- was 32"); id's `client.h` has 32 (REND-7.7) |
| `REND_FLASHBLEND_SCALE` | 0.35 | `gl_rlight.c`, `R_RenderDlight` |

## REND-8 Skyboxes

**Source:** `Quake/gl_sky.c`, `Sky_NewMap`, `Sky_LoadSkyBox`, `Sky_ProjectPoly`,
`Sky_EmitSkyBoxVertex`, `Sky_DrawSkyBox` and `Sky_DrawSky`; `Quake/image.c`, `Image_LoadImage`,
`Image_LoadTGA` and `Image_LoadPCX`
(sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`). The two scrolling layers of id's
sky are REND-2.3.

### Behaviour

- **REND-8.1** QuakeSpasm reads the worldspawn's `sky` key (also `skyname` and `qlsky`) as a
  skybox name and loads the six faces `gfx/env/<name><side>` for the sides `rt`, `bk`, `lf`,
  `ft`, `up`, `dn`; a map with one draws the box where its sky faces are instead of the
  layers, and its `skyfog` key (the `r_skyfog` setting, 0.5, when absent) is the fraction of
  the fog colour (REND-10) blended over the sky, only while the fog density is above 0 and
  `skyfog` is above 0 (`Fog_GetDensity() > 0 && skyfog > 0`). The preview host draws a skybox
  when the map names one, under ADR-0012.
- **REND-8.2** QuakeSpasm's `Sky_NewMap` reads the worldspawn's keys, a leading `_` and trailing
  spaces dropped, and calls `Sky_LoadSkyBox (value)` for each `sky`, `skyname` and `qlsky` in
  turn, so the last one read decides, an empty value turning the box off; `skyfog` is `atof
  (value)`, 0 for text that is not a number. `Sky_LoadSkyBox` reads `gfx/env/<name><suf[i]>` for
  `suf` `rt`, `bk`, `lf`, `ft`, `up`, `dn` through `Image_LoadImage`, which tries `<file>.tga`,
  then `<file>.pcx`. A face it cannot read is `notexture`; when none is read it "go[es] back to
  scrolling sky". `Image_LoadPCX` takes only signature 0x0A, version 5, encoding 1, 8 bits a
  pixel and one colour plane (`Sys_Error` otherwise); the picture is `xmax - xmin + 1` by `ymax
  - ymin + 1`, the palette the file's last 768 bytes, and each row decodes until
  `bytes_per_line` pixels, a byte from 0xC0 a run of `byte & 0x3F` of the next byte, writing
  from the row's start, so the next row overwrites the padding and anything past the width.
  `Image_LoadTGA` stores rows from the top, flipping a file without attribute bit 0x20. The
  first file found decides: a `.tga` that does not read is a face not read, not the `.pcx`
  beside it. The preview host reads the TGA with the `image` crate, and diverges where
  QuakeSpasm stops or reads outside the file: a file it cannot read, a PCX that ends before its
  pixels or its palette, or one whose `bytes_per_line` is less than its width (whose rows
  QuakeSpasm leaves zero, so a small file could ask for gigabytes), is a face not read. A face
  not read is black instead of `notexture`'s debug checkerboard, and the faces share one texture
  array at the first read face's size, a face of another size taken at its nearest texels; a box
  larger than the device's largest texture keeps the scrolling layers.
- **REND-8.3** `Sky_DrawSky` draws the sky faces into the depth buffer before the world, then
  draws the box's six faces at `gl_farclip / sqrt(3)` around `r_origin` with
  `glDepthFunc(GL_GEQUAL)` and no depth write, so the box shows only through the sky faces; `#if
  1 /* FIXME */` always draws whole faces, `skymins` -1 and `skymaxs` 1. A direction `v` falls
  on `Sky_ProjectPoly`'s axis: 0 or 1 (+x, -x) when `|v[0]|` is larger than both others, else 2
  or 3 (+y, -y) when `|v[1]|` is, else 4 or 5 (+z, -z). Its `vec_to_st[axis]` gives `s` and `t`
  from -1 to 1 as signed components over the axis's: {-2,3,1}, {2,3,-1}, {1,3,2}, {-1,3,-2},
  {-2,-1,3}, {-2,1,-3}. `Sky_EmitSkyBoxVertex` makes them `(s+1)*0.5` and `(t+1)*0.5`, then `s *
  (w-1)/w + 0.5/w` to "avoid bilerp seam", and `1 - t`, on the texture
  `skybox_textures[skytexorder[axis]]`, `skytexorder` {0,2,1,3,4,5}. With fog, `skyfog` of the
  fog colour is blended over the box (REND-8.1), and the box is not drawn when `skyfog >= 1`.
  The preview host samples the box for each sky fragment along the direction from the eye, which
  is the same picture.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `REND_SKYFOG` | 0.5 | `gl_sky.c`, `r_skyfog` default |

## REND-9 Liquid alpha

**Source:** `WinQuake/gl_rsurf.c`, `R_DrawWaterSurfaces`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`). The warp is REND-2.2.

### Behaviour

- **REND-9.1** Water is drawn after the world's other faces with `r_wateralpha` (1 by
  default, so opaque): below 1 the faces are blended over the scene with that alpha. The
  preview host does the same, the alpha a setting.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `REND_WATERALPHA` | 1 | `gl_rmain.c`, `r_wateralpha` default |

## REND-10 Fog

**Source:** `Quake/gl_fog.c`, `Fog_ParseWorldspawn`, `Fog_Update`, `Fog_ParseServerMessage`,
`Fog_GetColor`, `Fog_GetDensity`, `Fog_SetupFrame`, `Fog_FogCommand_f`
(sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`).

id's engine has no fog; the FitzQuake lineage's global fog is what the rerelease's and the
corpus mods' maps set, through the worldspawn `fog` key and the `svc_fog` message, so the
preview host draws it (ADR-0012).

### Behaviour

- **REND-10.1** A new map starts with no fog (density 0, colour 0.3 grey) and then reads
  the worldspawn's `fog` key as `density red green blue` with `sscanf`, so a shorter value
  sets only the leading fields.
- **REND-10.2** `Fog_Update (density, r, g, b, time)` with a `time` above 0 fades from the
  current values (or from the point a fade in progress has reached) to the new ones over
  `time` seconds, ending at `cl.time + time`; with `time` 0 it sets them at once.
  `svc_fog` carries density, red, green and blue as bytes over 255 and the time as a short
  over 100, at least 0.
- **REND-10.3** While a fade is in progress (`fade_done > cl.time`) the colour and density
  are `f * old + (1 - f) * new` with `f = (fade_done - cl.time) / fade_time`. The colour's
  channels are clamped to 0 to 1 and rounded to the nearest 1/255 ("so solid-colored sky
  can match the fog perfectly").
- **REND-10.4** The fog is `GL_EXP2` with `GL_FOG_DENSITY = density / 64`: a fragment's
  colour is mixed toward the fog colour by `1 - exp (-((density / 64) * z)^2)`, `z` its eye
  distance. The preview host computes the same in its shader.
- **REND-10.5** The `fog` command with one argument, `fog <density>`, sets the density at once
  and keeps the colour last set (`Fog_FogCommand_f`, `case 2:`): "`d = Q_atof(Cmd_Argv(1));`",
  "`t = 0.0f;`", "`r = fog_red;`" and the same for green and blue. Then, on every path, the
  density is raised to 0 and each channel kept within 0 to 1: "`if      (d < 0.0f) d = 0.0f;`",
  "`if      (r < 0.0f) r = 0.0f;`", "`else if (r > 1.0f) r = 1.0f;`" and the same for green
  and blue, before "`Fog_Update(d, r, g, b, t);`". `fog_red` and the others are the values a
  fade ends at, so a fade in progress ends at once. The preview host's view script sends it
  (VIEWDIFF-1.4); its other forms are not read.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `REND_FOG_DEFAULT_DENSITY` | 0.0 | `gl_fog.c`, `DEFAULT_DENSITY` |
| `REND_FOG_DEFAULT_GRAY` | 0.3 | `gl_fog.c`, `DEFAULT_GRAY` |
| `REND_FOG_DENSITY_SCALE` | 64 | `gl_fog.c`, `Fog_SetupFrame` |

## REND-11 Sprites

**Source:** `Quake/r_sprite.c`, `R_GetSpriteFrame`, `R_DrawSpriteModel`
(sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`); the frames are SPR-2 and
SPR-3.

GLQuake draws every sprite facing the view plane; QuakeSpasm honours the sprite's orientation
type, which the preview host does (ADR-0012).

### Behaviour

- **REND-11.1** A frame out of the sprite's range is drawn as frame 0. A single frame is
  itself; a frame group picks by time: `time = cl.time + syncbase` (CLIENT-2.4), `target =
  time - (int) (time / fullinterval) * fullinterval` with `fullinterval` the group's last
  interval, and the first subframe whose interval exceeds `target`, the last if none does. An
  angled group (`SPR_ANGLED`) picks subframe `dir & 7` with `dir = (int) ((atan2 (r, f) +
  1.125 pi) * 4 / pi)`, `f` and `r` the dot products of the sprite's forward with the view's
  forward and right.
- **REND-11.2** The quad's axes by the sprite's type: `SPR_VP_PARALLEL_UPRIGHT` (0) up is
  world up and right is `forward x up` normalised; `SPR_FACING_UPRIGHT` (1) forward is the
  sprite's origin less the eye, flattened and normalised, right `(forward.y, -forward.x, 0)`,
  up world up; `SPR_VP_PARALLEL` (2) the view's up and right; `SPR_ORIENTED` (3) the sprite's
  own angles' vectors, drawn with a polygon offset so it sits on walls; `SPR_VP_PARALLEL_ORIENTED`
  (4) the view's right and up rotated by the sprite's roll. The corners are `origin + down *
  up + left * right`, `origin + up * up + left * right`, `origin + up * up + right * right`
  and `origin + down * up + right * right` with the frame's `up`, `down`, `left` and `right`
  (SPR-2) and the texture's `(0, tmax)`, `(0, 0)`, `(smax, 0)`, `(smax, tmax)`; transparent
  texels are cut by alpha test.

## REND-12 MD5 models

**Source:** `Quake/gl_model.c`, `Mod_LoadModel`; `Quake/gl_mesh.c`, `Mod_LoadMD5MeshModel`,
`MD5_BakeInfluences`, `MD5_ComputeNormals` and `MD5Anim_Load`; `Quake/r_alias.c`,
`R_SetupAliasFrame`; `Quake/image.c`, `Image_LoadImage`; `Quake/gl_model.h`, `MAX_SKINS`
(Shpoike/Quakespasm@`4c0c55f7830715cf8670f586c7be01ebce262d23`, cited for description only, as
[`md5.md`](md5.md) says); `Quake/r_alias.c`, `GLAlias_CreateShaders` and `R_DrawAliasModel`
(sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`). The files are MD5-1 to MD5-3.

The rerelease replaces id1's `.mdl` models with skeletal MD5 models. id's rerelease engine is
not published; QuakeSpasm-Spiked reads the files, and the preview host draws them through the
alias pipeline, with QuakeSpasm's shader formula for a vertex normal.

### Behaviour

- **REND-12.1** QuakeSpasm-Spiked's `Mod_LoadModel` tries each extension of `r_replacemodels`
  (default empty) in place of a model's, and passes over a file found when the original is on a
  later search-path element ("Ignoring %s from lower priority path"); it then picks the loader
  by the file's first four bytes, `MD5V` for MD5 (MD5-2.1). The preview host replaces
  `progs/x.mdl` with `progs/x.md5mesh` by default, so a rerelease install shows its MD5 models,
  with `md5_models` off as QuakeSpasm-Spiked's default: the MD5 model is used when it is found
  and the `.mdl` is not found on an earlier search-path element. A mesh or animation that does
  not read (MD5-2, MD5-3), which QuakeSpasm-Spiked stops on, leaves the `.mdl` drawn; with no
  `.mdl` it is an error naming the file. A torch `.mdl` (REND-4.7) gives its MD5 model full
  light.
- **REND-12.2** Every frame is made when the model is read, so an animation whose `numFrames` is
  larger than its file in bytes is refused (`<file> has too many frames`), as MD5-3.11 bounds
  the components. Without an `.md5anim` (MD5-3.1) the model has one frame, the mesh's joints,
  which are absolute: `Mod_LoadMD5MeshModel` inverts them directly ("absolute, so we can just
  invert now"). An animation frame's poses (MD5-3.7) are relative: QuakeSpasm-Spiked
  concatenates each onto its parent's, in joint order ("concat it onto the parent
  (relative->abs)"), the position turned by the parent's orientation and moved to its position,
  the orientations multiplied. A joint whose parent is not before it reads a pose not yet made
  in QuakeSpasm-Spiked; the preview host places it as if it had none. A vertex in a frame is the
  sum over its weights (MD5-2.12) of `bias * (joint orientation * weight position + joint
  position)`, as the mesh's own pose bakes it (`weight[idx].pos[...] =
  MD5FLOAT()*weight[idx].pos[3]`, then `Matrix3x4_RM_Transform4`); QuakeSpasm-Spiked then keeps
  four influences a vertex and skins the baked pose by the frame's joints, which agrees for a
  vertex on one joint, and the preview host computes every frame by the sum, every weight kept.
  A vertex's normal is `MD5_ComputeNormals`': the sum of `cross(v1 - v0, v2 - v0)` normalised
  over its triangles, normalised, a zero vector left zero. The triangles are wound as an
  `.mdl`'s are, which the alias pipeline's culling draws (REND-1.10), and for that winding the
  cross product points into the model, so every vertex took the facing-away shade of REND-12.5
  and a rerelease view model drew nearly black; the preview host takes `cross(v2 - v0, v1 -
  v0)`, the outward normal, so an MD5 model lights as its `.mdl` does, and makes it for every
  frame.
- **REND-12.3** Each animation frame is one frame of one pose shown for 0.1 seconds (MD5-3.10);
  a frame out of range draws frame 0. The triangles are the mesh's, in its order, each corner
  with its vertex's `s` and `t`, and are turned and moved as an alias model is (REND-4.5). Two
  frames blend by the lerp fraction (CLIENT-7.3) as REND-4.12 blends poses, positions and
  shades; QuakeSpasm-Spiked blends the joints' matrices instead. Each mesh of the file is drawn
  with its own skin.
- **REND-12.4** A mesh's skins are found from its shader name (MD5-2.13): for `n` from 0 up to
  `MAX_SKINS` (32), frames `f` from 0 to 3 of `progs/<shader>_<nn>_<ff>` until one is missing; a
  skin with no frame 0 ends the skins. Each is found by QuakeSpasm-Spiked's `Image_LoadImage`:
  the name with `.dds`, `.tga`, `.png`, `.jpeg`, `.jpg` and `.pcx` in turn, as it is, then under
  `textures/`, then its last path part under `textures/`, and at last `<name>.lmp`. The four
  slots (REND-4.2) are filled as "this stuff is hideous" fills them: one frame in all four, two
  or three as frames 0, 1, 0, 1, four in order. The first file found decides, as
  `Image_LoadImage` returns its loader's result: one that does not read gives no image. The
  preview host reads no `.dds`, and draws a mesh with no skin white; the `_luma`, `_glow`,
  `_pants` and `_shirt` images are not read, so an MD5 player wears no colours (REND-4.13).
- **REND-12.5** QuakeSpasm's GLSL alias shader shades a vertex by its normal:
  `r_avertexnormal_dot` is `1.0 + dot * (13.0 / 44.0)` when `dot(vertexnormal, ShadeVector)` is
  below 0 ("this reproduces anorm_dots within as reasonable a degree of tolerance") and `1.0 +
  dot` otherwise, the two poses' mixed by the blend and times the light. `ShadeVector` is
  `(cos(-a), sin(-a), 1)` normalised, `a = (quantizedangle / 16.0) * 2.0 * 3.14159` and
  `quantizedangle = ((int)(e->angles[1] * (SHADEDOT_QUANT / 360.0))) & (SHADEDOT_QUANT - 1)`
  (`R_DrawAliasModel`). The preview host shades an MD5 vertex so, times REND-4.7's `shadelight`,
  clamped to 0 to 1.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `MAX_SKINS` | 32 | QuakeSpasm-Spiked `gl_model.h` |

## REND-13 The QuakeSpasm look

**Source:** `Quake/gl_vidsdl.c`, `vid_gamma`; `Quake/gl_rmain.c`, `gl_fullbrights`,
`gl_overbright` and `GLSLGamma_GammaCorrect`; `Quake/gl_texmgr.c`, `glmodes`, `glmode_idx`
and `TexMgr_LoadPalette`; `Quake/gl_model.c`, `Mod_LoadTextures` and `Mod_CheckFullbrights`;
`Quake/r_brush.c`, `R_BuildLightMap`; `Quake/r_world.c` and `Quake/r_alias.c`, their GLSL;
`Quake/gl_screen.c`, `scr_fov_adapt` and `AdaptFovx`
(sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`).

The engines players run exported mods on draw a frame brighter, sharper and wider than GLQuake:
QuakeSpasm keeps the palette as it is, filters between mip levels, lets fullbright texels glow,
lets light go past a texture's colour, and widens the view to the window. This renderer draws
QuakeSpasm's look by default and GLQuake's as a mode.

### Behaviour

- **REND-13.1** A frame takes one of two looks. QuakeSpasm's is the default: its `gamma`,
  texture mode, `gl_fullbrights`, `gl_overbright` and `fov_adapt` as they ship (REND-13.2 to
  REND-13.6). GLQuake's, as REND-1 to REND-7 describe it, is a mode a caller chooses
  (`Look::GlQuake`), as ADR-0012 keeps `BspLoader::WinQuake`; the preview host's `--look
  glquake` chooses it.
- **REND-13.2** QuakeSpasm's gamma defaults to 1 (`Quake/gl_vidsdl.c`: "`cvar_t vid_gamma =
  {"gamma", "1", CVAR_ARCHIVE};`") and corrects the finished frame, not the palette; at 1
  nothing is corrected (`Quake/gl_rmain.c`: "`if (vid_gamma.value == 1 && vid_contrast.value ==
  1)`"). In QuakeSpasm's look the palette is used as it is. A gamma the caller gives still
  corrects the palette as REND-3.1 says, our choice, so `-gamma` keeps GLQuake's meaning.
- **REND-13.3** QuakeSpasm's texture mode defaults to the last of its modes
  (`Quake/gl_texmgr.c`: "`static int glmode_idx = NUM_GLMODES - 1; /* trilinear */`"),
  "`{GL_LINEAR, GL_LINEAR_MIPMAP_LINEAR, "GL_LINEAR_MIPMAP_LINEAR"}`": a world texture, and a
  skin filtered as one (REND-4.3), is linear within each of the two nearest mip levels and
  blends between them, where GLQuake takes the nearest level (REND-3.3).
- **REND-13.4** With `gl_fullbrights` "1" (`Quake/gl_rmain.c`), a texture with a texel above 223
  (`Mod_CheckFullbrights`: "`if (*pixels++ > 223)`") is loaded twice (`Mod_LoadTextures`):
  through the palette with "224-255 ... black" (`d_8to24table_nobright`) and through the one
  with "0-223 ... black" (`d_8to24table_fbright`, `TexMgr_LoadPalette` in `Quake/gl_texmgr.c`),
  and the second is added over the lit first: "`result += texture2D(FullbrightTex,
  gl_TexCoord[0].xy);`" then "`result = clamp(result, 0.0, 1.0);`" (`Quake/r_world.c`). Only a
  regular texture is split, not the sky or a liquid. An alias model's skin is split and added
  the same way (`Quake/r_alias.c`, `FullbrightTex`). Here a texture with a palette of its own
  (MAP-24) and a player's translated skin (REND-4.13) have no glow, our choice.
- **REND-13.5** With `gl_overbright` "1" (`Quake/gl_rmain.c`), `R_BuildLightMap` shifts each
  texel's unsigned sum by 8, not 7, and caps it at 255 (`Quake/r_brush.c`: "`if (overbright) { r
  = *bl++ >> 8; ...`", "`*dest++ = (r > 255)? 255 : r;`"), grey light and coloured alike, and
  the world's shader doubles it: "`if (UseOverbright) result.rgb *= 2.0;`" (`Quake/r_world.c`),
  so light can reach twice a texture's colour, before the fullbrights and the clamp (REND-13.4).
  A map with no light data is 255 either way. Alias models are lit as REND-4.7 says in both
  looks, our choice: QuakeSpasm's `R_SetupAliasLighting` is not ported.
- **REND-13.6** With `fov_adapt` "1" (`Quake/gl_screen.c`), the field of view across is
  `AdaptFovx (scr_fov.value, vid.width, vid.height)`: "`if ((x = height / width) == 0.75) return
  fov_x; a = atan(0.75 / x * tan(fov_x / 360 * M_PI)); a = a * 360 / M_PI; return a;`", the
  width that keeps a 4:3 image's height, so 90 is about 100.39 at 16:10. The view's height is
  then REND-3.4's `CalcFov` of it. The eye a click picks from uses the same width.

- **REND-13.7** **Withdrawn** 2026-11-06: it made the console's background opaque when it
  fills the screen, where QuakeSpasm's code it quotes makes it opaque when forced up;
  replaced by REND-13.8.
- **REND-13.8** QuakeSpasm draws its 2D layer on canvases (`GL_SetCanvas` in
  `Quake/gl_draw.c`) at the window's own size, not GLQuake's 640 wide layer: the console, the
  notify lines and the centre print at scale 1 (`scr_conscale` "1"); the menus' 320 by 200
  canvas centred both ways ("`glViewport (glx + (glwidth - 320*s) / 2, gly + (glheight -
  200*s) / 2, ...)`", `s` 1 with `scr_menuscale` "1"); and the bar's 320 wide canvas at the
  bottom centre at scale 1 (`scr_sbarscale` "1"). The fade behind a menu is "`glColor4f (0, 0,
  0, 0.5)`" (`Draw_FadeScreen`), and the console's background is `scr_conalpha` "0.5", opaque
  only when the console is forced up (`Draw_ConsoleBackground`: "`alpha = (con_forcedup) ? 1.0f :
  scr_conalpha.value;`"). In GLQuake's look the fade is 0.8 and the background as CON-3.2
  says.
- **REND-13.9** In QuakeSpasm's look the `gamma` setting is applied to the whole frame, the
  world, the models, the particles and the 2D layer, as `GLSLGamma_GammaCorrect`
  (`Quake/gl_rmain.c`) applies it once the frame is drawn: "`gl_FragColor =
  vec4(pow(frag.rgb, vec3(GammaValue)), 1.0);`", skipped when it is 1. The host draws inside
  egui's render pass and cannot copy the frame back, so it raises each fragment as it is
  written, which is the same for an opaque pixel; a blended one is raised before the blend
  rather than after, our choice. QuakeSpasm's `contrast` is not read here and stays at 1. The
  client's blend table is built at gamma 1 in this look, since QuakeSpasm's `R_PolyBlend`
  draws the blend as it is. In GLQuake's look `gamma` only builds the blend table (VIEW-4.7),
  as GLQuake's `V_UpdatePalette` ramps reach no screen.
