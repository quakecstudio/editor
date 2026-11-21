# Models for the agent

Sources: [`docs/spec/mdl.md`](../spec/mdl.md) (MDL-1 to MDL-9); [`docs/spec/render.md`](../spec/render.md)
(REND-4.13); the model commands (`crates/qs-workbench/src/models.rs`, L-47).

## What a model is

What a monster, a weapon or an item looks like is its `.mdl` file, an alias model, which the
QuakeC names in `precache_model` and `setmodel`. It holds:

- **Skins**: pictures of `skinwidth` by `skinheight` pixels, each pixel a colour of the
  game's palette (MDL-2.2). An entity shows skin number `self.skin`.
- **Texture coordinates and triangles**: where each vertex lies on the skin, and which three
  vertices make each triangle (MDL-3.1, MDL-4.1).
- **Frames**: where every vertex is in each pose, packed in three bytes a vertex, with the
  header's `scale` and `scale_origin` saying where a byte is (MDL-5.3). An entity shows
  frame number `self.frame`. The game finds a frame by its number, not its name.

The QuakeC's `$frame` line names the frames in the file's order, from 0 (see the `monsters`
topic). The names in the file are for people: `model.inspect` lists them in order as a
`$frame` line takes them.

A model's size is not the entity's: QuakeC's `setsize` gives the entity the box it collides
with, and every model's own bounds are the same to the engine (MDL-1.7).

## Limits

- A skin is at most 480 high and its width a multiple of 4; a model has 1 to 2000 vertices
  and one skin, one triangle and one frame or more (MDL-1.4).
- Engines made from GLQuake take at most 1024 vertices, 32 skins, 2048 triangles and 256
  poses, counting each frame of a group (MDL-1.8). Stay within them.

## A mod's own models

- A model of the mod's own is a file under the project's `progs` folder, such as
  `progs/brute.mdl`, named in QuakeC as `"progs/brute.mdl"`. Play puts the project's `progs`,
  `sound` and `gfx` files, and the compiled maps of its `maps`, where the game reads them.
- `model.copy_from_game` copies a model of the game, by its name there, into the project to
  change. A file of the same name as the game's takes the game's place for every entity that
  uses it; a file of a new name is a new model, for the QuakeC that names it.

## A model made elsewhere

`model.import` makes a `.mdl` in the project of a mesh made in another program:

- **An `.obj` file** is one pose, named `base`. Most programs write it with y up, which is
  what is assumed; give `up` as `z` for one written with z up.
- **An `.iqe` file** (Inter-Quake Export, which Blender's IQM exporter writes) may have a
  skeleton and animations. A model has no skeleton, so every frame of every animation
  becomes a frame of the model, named by its animation and its number (`run1`, `run2`): the
  answer's `frame_line` is the QuakeC's `$frame` line.
- **Size**: Quake's units are small, a player being 56 high. Give `scale` to make the mesh
  that size, or `model.scale` it after; `model.inspect` answers the box it fills.
- **Its skin**: a mesh with texture coordinates keeps them. Give `png_path`, a picture whose
  width is a multiple of 4 and whose height is at most 480, or it is one colour to paint
  over. A mesh without them is laid out front and back, what faces forward (+x) on the
  skin's left half and what faces back on its right: `model.export_skin` it, paint it and
  `model.set_skin` it.
- **Limits**: 2000 vertices at most, and 1024 vertices and 2048 triangles for engines made
  from GLQuake. Make the mesh simpler in the program it came from when it has more.
- To add poses to a model already imported, import the other poses as a model beside it
  and `model.add_frames` from it.

## Changing one

Each of these is one step that `edit.undo` takes back.

| Command | What it does |
| --- | --- |
| `model.import` | Makes a model of a mesh in an `.obj` or an `.iqe` file |
| `model.inspect` | The skins, their size and the rows of the palette each is of, the vertices and triangles, the box the frames fill, and the frames by name |
| `model.recolor` | Recolours a skin by rows of the palette |
| `model.export_skin`, `model.set_skin` | A skin out as a PNG, and a PNG of the same size back as a skin, each changed pixel the nearest of the palette's colours 0 to 223 |
| `model.scale`, `model.move` | Makes the model larger or smaller, or moves it against the entity's origin; no vertex is rounded again |
| `model.rename_frames`, `model.remove_frames`, `model.add_frames` | Names frames anew, all at once by the names they have, takes frames out, or adds frames of another copy of the model with other poses |

- **The palette is 16 rows of 16 shades**, a colour's row its number divided by 16. Rows 0
  to 7 run from dark to bright and rows 8 to 15 from bright to dark (REND-4.13: "the artists
  made some backwards ranges"); `model.recolor` keeps a pixel's shade between the two kinds.
  The game recolours a player's shirt from row 1 and trousers from row 6 the same way.
  `model.inspect` answers the rows a skin is of, most pixels first: those are the rows to
  give `model.recolor`. Look
  at a skin (`model.export_skin`, then read the PNG) before and after.
- **Colours 224 to 255 glow**: they show at full brightness in the dark. A painted pixel
  takes one only when it is exactly that colour of the palette; any other colour takes the
  nearest of colours 0 to 223.
- **A skin needs a background**: engines made from GLQuake fill the part of a skin that
  is joined to its top left pixel and of that pixel's colour with black before drawing
  (REND-4.1). id's skins have a background there that no triangle uses. A skin painted one
  colour all over, top left pixel included, is drawn black: leave the top left pixel as it
  is, or keep it another colour than what it touches.
- **Scale about the origin**: a model stands with its origin where the entity's is, so a
  monster made 1.3 times larger also reaches 1.3 times as far below it. Its feet are at the
  low z of the box `model.inspect` answers; when QuakeC's `setsize` box starts at -24, move
  the model (`model.move`) so its low z is -24 again.
- **Taking a frame out or adding one renumbers** what follows: change the QuakeC's `$frame`
  line with it, or every later frame shows the wrong pose.
- Frames can be added only from a model of the same vertices, and only where they stay in
  the box the model's own frames fill.

## A monster with a look of its own

1. `model.copy_from_game` the model of the monster it is made from to `progs/<name>.mdl`.
2. `model.recolor` its skin, `model.scale` it, and `model.move` it back onto its feet.
3. Name the new file in the monster's `precache_model` and `setmodel`, and `setsize` it to
   fit. Keep the `$frame` line of the monster it came from: the frames are the same.
4. `project.compile`, then play a map with one in it and `engine.screenshot`.
