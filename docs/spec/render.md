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
- **REND-2.4** GLQuake computes REND-2.2 and REND-2.3 at the vertices of faces it cuts at
  64-unit boundaries (`GL_SubdivideSurface`), with `turbsin` indexed in 256 steps. This
  renderer computes the same formulas for each pixel, with `8 sin` of the angle itself: a
  deliberate divergence, which draws the warp and the sky without the subdivision and gives
  each pixel the value GLQuake gives the vertices near it.

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
`GL_DrawAliasFrame` and `R_RotateForEntity`; `WinQuake/gl_model.c`, `Mod_FloodFillSkin`,
`Mod_LoadAllSkins` and `Mod_LoadAliasGroup`; `WinQuake/gl_mesh.c`, `BuildTris`;
`WinQuake/gl_rlight.c`, `R_LightPoint` and `RecursiveLightPoint`; `WinQuake/anorm_dots.h`;
`WinQuake/cl_main.c`, `CL_RelinkEntities`; `WinQuake/mathlib.c`, `anglemod`; `WinQuake/gl_vidnt.c`,
`VID_SetPalette` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`). The colour clamp
is OpenGL 1.1, section 2.13.6.

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
  players' floor of 8 and dynamic lights are not drawn yet.
- **REND-4.8** The shade of a vertex is `shadedots[lightnormalindex] * shadelight`, with
  `shadedots` the row `((int)(angles[1] * (SHADEDOT_QUANT / 360.0))) & (SHADEDOT_QUANT - 1)` of
  `r_avertexnormal_dots`, 16 rows of 256 entries. It is given to `glColor3f`, so it is clamped
  to 0 to 1 before it is interpolated: OpenGL 1.1, 2.13.6, "After lighting (whether enabled or
  not), RGBA colors are clamped to the range [0, 1]." It is interpolated across the triangle
  (`gl_smoothmodels`, `GL_SMOOTH`), and the skin's colour is multiplied by it (`GL_MODULATE`).
- **REND-4.9** A model whose header has `EF_ROTATE` (items and weapons) is drawn with yaw
  `bobjrotate = anglemod(100*cl.time)` (`CL_RelinkEntities`), with `anglemod (a)` of the float
  `a` being `(360.0/65536) * ((int)(a*(65536/360.0)) & 65535)`.

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
  fa->cached_light[maps]`). This renderer builds the whole atlas again in that case; the
  texels are the same.
- **REND-5.6** `R_LightPoint` sums `sample * scale` with the same style values into the `int r`,
  so its products wrap the same way, and shifts it right by 8 (`RecursiveLightPoint`).

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
