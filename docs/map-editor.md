# The map editor

The map editor (P5-06, MAP-4, MAP-20, MAP-22) opens a `.map`, or the entities of a shipped
`.bsp` that has no `.map` source, from the Project tree or with `map.open`. It shows the map in
the Map tab and the selected entity's keys in the Properties panel. Every edit is a `map.*`
command ([`commands.md`](commands.md)), so each one is undoable and the agent and MCP can make
it too.

- **What it draws:** a `.map`'s brushes, cut from their planes (MAP-3.4), or a BSP's faces, as
  lines. Each point entity is a box in its class's colour, at the size its `/*QUAKED*/` comment
  or `setsize` gives (`project.entities`, P5-03). When the project's folder or the game holds
  the class's model, the first frame of that model is drawn as a wireframe preview. The
  selection is drawn in the accent colour.
- **The key editor:**
  - It lists the entity's pairs with their values, and a Remove button for each.
  - It shows the keys the entity's class reads but does not set, each with its type and
    default. Clicking one starts a new pair with that key.
  - A target key's value can be picked from the map's targetnames.
  - `map.check` (P5-04) reports the keys and classnames the project does not define.
- **Placing and linking (P5-13, MAP-27):**
  - The Entities browser below the view lists the classes the project's QuakeC defines, each
    with its model's wireframe, or its box where it has no model, in its class's colour. A
    point class dragged into the view is placed (`map.place`) 256 units ahead of the camera
    along the pointer's ray, on the 16-unit grid. A brush class is listed, but is made from
    brushes.
  - Lines join each entity to the entities its keys fire, for the selection or for every
    entity (Links). The keys are those the QuakeC compares with `targetname` (MAP-8.8), so
    `killtarget` and a mod's own keys link as `target` does.
  - Pick, beside such a key, sets it to the next entity clicked (`map.link`). An Alt-drag from
    the selection onto another entity sets the key last picked, else the shortest such key
    (`target` in id's QuakeC). An entity with no `targetname` is given its classname and the
    first number no entity has as its `targetname`.
  - Duplicating an entity whose `targetname` ends in a number gives the copy the next free
    number, and moves the numbers of its target keys on by as many, so a copied link in a
    chain names the next one. Zero padding is kept: `t09` becomes `t10`, `t010` becomes `t011`.
- **The fields (P5-13):** spawnflags are checkboxes named as the QuakeC names them, `angle`
  has Up and Down, a key ending in `color` has a colour picker that keeps the key's scale
  (0 to 1 or 0 to 255), and a float the map gives several values offers those values. Shift+click
  adds an entity to the selection; a key edit then sets every selected entity in one undo step
  (`map.set_key` with `also`), and a spawnflag ticked or cleared changes that bit alone in each
  entity's own flags (`map.set_flag`).
- **Show (P5-13):** the view can show only the entities the game spawns on easy, normal, hard or
  nightmare, or in coop or deathmatch. `map.filter` compiles the project's server QuakeC in
  memory, writing nothing, and spawns the entity lump qbsp would write for the map in `qs-sim`
  with those settings, so what the spawnflags rule out and what the QuakeC removes itself are
  both hidden. It is found again after each edit.
- **Groups and layers (P5-14, MAP-26):** a map made in TrenchBroom keeps its groups, linked
  groups and layers as written ([MAP-9](spec/map.md)).
  - An edit inside a linked group is copied into every group linked to it, moved by each
    group's transformation, in the same undo step; a key a copy protects
    (`_tb_protected_properties`) keeps its own value. A mirrored copy is mirrored, its brushes
    kept outside in. A linked group holding another group is not copied, nor is an edit that
    changed two linked copies differently; either edit is refused.
  - Moving a group moves everything in it and its `_tb_transformation`, so the groups linked to
    it stay where they are.
  - The Properties panel lists the layers, each with Hidden, Locked and Not exported
    (`map.set_layer`, written as TrenchBroom writes them); `map.export` writes the map without
    the layers marked not to be exported.
  - Hide, Isolate and Show all (and TrenchBroom's keys for them) hide entities in the editor
    without changing the map. Lock and Unlock all lock any selection, which TrenchBroom does not
    (its manual: "You cannot lock objects individually"); an edit that would change a locked
    entity, or a copy of one in a linked group, is refused. A layer's settings hold for
    everything in it, groups and their members included. What is hidden and locked is not undone
    with the map's edits; an undo or redo that changes how many entities there are forgets it.
- **The outline (P5-16, MAP-31):** the Map section of the left panel lists the default layer
  (the world and what no layer names), then each layer; in each, its groups and linked groups
  (in italics), then its entities by classname and targetname, with brush counts
  (`map.outline`). The filter keeps what holds its text in a classname, key or value, with the
  layers and groups around it. Choosing a row selects the entity and frames it in the view,
  keeping where the camera looks; a selection made in the view opens and scrolls to its row.
  Dragging a row onto a layer or group moves it there (`map.move_to`), into a linked group
  copied into the groups linked to it; the world and a layer stay where they are. An entity
  naming both a layer and a group is shown in the layer, as hiding and locking treat it (MAP-9.2);
  groups that name each other are shown under the default layer.
- **Brushes (P6-08, MAP-2):** the toolbar holds Select, Brush, Clip and Vertex. With a brush
  tool, a click selects the first brush the pointer's ray enters, by its faces, and Shift+click
  adds or removes one. Each edit is a `map.brush_*` command on the 16-unit grid, undone in one
  step, and kept convex: one that would leave a brush with no volume is refused.
  - Brush: a drag on the ground (height 0) draws a box 64 units tall in the world
    (`map.brush_create`), with the texture of the map's last face.
  - Clip: a drag on the ground draws the cut line, and the selected brush is cut by the vertical
    plane through it (`map.brush_clip`), keeping the part right of the line, left of it, or both
    as two brushes.
  - Vertex: the selected brushes' corners are drawn as dots; dragging one moves it across the
    horizontal plane it lies in, and the brush becomes the hull of its corners
    (`map.brush_vertex`). A corner moved inside the brush is refused.
  - The Properties panel subtracts the selected brush from the other brushes of its entity
    (`map.brush_subtract`), merges the selected brushes of one entity into their hull
    (`map.brush_merge`), and deletes the selected brushes (`map.brush_delete`).
- **Reviewing the agent's map edits (P6-21, MAP-29):** the agent builds geometry with the same
  brush, shape and CSG commands as the editor (`map.brush_create`, `map.brush_shape`,
  `map.brush_subtract` and the rest). Its first map command begins a review (`map.stage`): the
  map is edited at once, so a compile and the game see the change, and each run of changed parts
  since, an entity's keys or one of its brushes, waits as a hunk (`map.staged`). The Properties
  panel lists them with Accept and Reject, and both for every hunk; a rejected hunk's parts are
  put back as an edit undo takes back (`map.accept`, `map.reject`). Review ends when no hunk is
  left.
- **Compile profiles (P6-20, MAP-28):** a profile is a list of tasks run in order on the open
  map (`map.run_profile`), as TrenchBroom's compile profiles are: `bsp` (qs-map-bsp, the map as it
  is in the editor, to a BSP beside it with its portal and leak files), `vis` and `light` (on that
  BSP, each `full` or `fast`), `tool` (a program and its arguments, run in the map's folder),
  `copy`, and `launch` (the BSP and its `.lit` copied into the project's build and the game
  started on it). Paths take `${MAP}`, `${MAP_DIR}`, `${MAP_NAME}`, `${BSP}`, `${PROJECT}` and
  `${GAME}`. Full and Fast are built in; a project adds its own in `compile-profiles.toml`:

  ```toml
  [[profile]]
  name = "Ship"
  tasks = [
    { task = "tool", program = "qbsp", args = ["${MAP}"] },
    { task = "light", speed = "full" },
    { task = "copy", from = "${BSP}", to = "${PROJECT}/release/maps/${MAP_NAME}.bsp" },
  ]
  ```

  The Properties panel chooses the profile (`map.choose_profile`) and runs it with Compile or
  Cmd+B, then says how long it took, or which task stopped it and why. A task that fails stops
  the profile.
- **Portals (P6-20):** Show portals draws the portal file of the last compile (`map.portals`)
  in every pane, where the map's leaves meet.
- **Backups (P6-20):** `map.backup` writes the map as it is in the editor to
  `autosave/<name>.<n>.map` beside it, keeping the last 20; the window takes one each minute
  the map has unsaved edits, and `map.save` one of what it saved. Restore the newest backup
  puts it in the editor as an edit undo takes back (`map.backups`, `map.restore_backup`).
- **Repeat (P6-20):** `edit.repeat` (Cmd+R, TrenchBroom's Repeat) makes the last map edit once
  more: the same `map.*` command with the same arguments. Other changes are not repeated: a text
  edit made again at the same place would land in text that has moved.
- **Saving:** `map.save` (Cmd+S in the view) writes a `.map` over itself, and changes only the
  text an edit touched. A BSP's entities go to a `.ent` file beside the BSP, which QuakeSpasm
  and its descendants read in place of the lump (MAP-22). `{"bsp": "lump"}` rewrites the BSP's
  own entity lump instead (BSP-2.2).

## TrenchBroom's bindings

The editor follows TrenchBroom's defaults (MAP-20), read from
[TrenchBroom/TrenchBroom](https://github.com/TrenchBroom/TrenchBroom) at
`8f329f11e65e3d21b355a42bcae199cb3e4e0812`:

- `lib/TbAppLib/src/CameraTool3D.cpp`: `shouldLook`, `shouldPan` and `shouldOrbit`;
- `lib/TbPreferencesLib/include/prefs/Preferences.h`: `CameraFly*`;
- `lib/TbUiLib/src/ActionManager.cpp`: the "Move objects", "Yaw objects", "Duplicate",
  "Delete" and "Save" actions;
- `lib/TbUiLib/src/ActionManager.cpp`: the "Isolate", "Hide" and "Show All" actions of the View
  menu;
- `lib/TbUiLib/src/MapViewBase.cpp`: `move` and `rotate`;
- `lib/TbMdlLib/src/Map.cpp` and `Grid.cpp`: the default grid, `Grid(4)`, which is
  `exp2(4)` = 16 units.

The view takes the keys while it has the keyboard; click it to give it the keyboard.

| TrenchBroom | Its default | Here |
| --- | --- | --- |
| Look (`shouldLook`) | right mouse button drag | the same |
| Pan (`shouldPan`) | middle mouse button drag | the same |
| Move forward and back | mouse wheel | the same |
| Fly (`CameraFlyForward`, `Backward`, `Left`, `Right`, `Up`, `Down`) | W, S, A, D, Q, X | the same, while held |
| Select | left click | the same; a click on nothing clears the selection |
| Move objects forward, backward, left, right | Up, Down, Left, Right, by the grid along the view | the same, 16 units along the map axis nearest the camera's |
| Move objects up, down | Page Up, Page Down | the same, 16 units |
| Yaw objects clockwise, counter-clockwise | Alt+Left, Alt+Right, a quarter turn (`half_pi`) | the same, on a point entity's `angle`, or the yaw of its `angles` |
| Duplicate | Ctrl+D (Cmd+D on macOS) | the same, in place, with the copy selected |
| Delete | the platform's Delete | Delete or Backspace |
| Save (`Menu/File/Save`) | the platform's Save, Ctrl+S (Cmd+S on macOS) | the same |
| Isolate Selection (`Menu/View/Isolate`) | Ctrl+I | the same, Cmd+I on macOS |
| Hide Selection (`Menu/View/Hide`) | Ctrl+Alt+I | the same, Cmd+Alt+I on macOS |
| Show All (`Menu/View/Show All`) | Ctrl+Shift+I | the same, Cmd+Shift+I on macOS |

While the game plays the open map (the map named by the file's stem), every edit, undo and
redo of an entity reaches it with no recompile (P5-07, MAP-12): a placed or duplicated entity
is spawned at its place as the map's load spawns one, a deleted one is removed, and a moved,
turned or keyed one has the keys that changed set; a key taken out is set empty. The host keeps
which edict each of the map's entities became (BRIDGE-4.13), so an entity the game has freed
since, a picked-up item or a killed monster, is left alone, with a warning in the console, as
is a spawn that needs a model the map has not precached (BRIDGE-4.12). A brush's faces reach
the game at the next compile. The editor sends its edits only while its entities are the ones
the game loaded, by classname in order: a map edited since its last compile, one whose
compiler merged `func_group` or `func_detail` into the world, a placed brush entity, or an edit
the game refuses puts the two out of step, said once in the console, and the edits reach the
game at the next compile and map load.

Another program's writes are noticed (P5-17, MAP-32): about once a second the open map's
file and the project's QuakeC are looked at. A map with no unsaved edits is read again, its
camera and selection kept; one with unsaved edits keeps them, and the Properties panel offers
Reload from disk, Keep mine and Compare, which lists how the file differs entity by entity.
Saving never overwrites a file another program wrote since it was read. Changed QuakeC is read
again, and the entity classes with it. The map's WADs are watched by the texture browser
(P6-10).

Not yet here:
- orbiting (`shouldOrbit`, Alt with a right drag);
- rolling and pitching objects;
- the 2D views;
- a grid size other than 16;
- turning brush entities;
- moving a brush's corner up or down, an edge or a face, and brush tools in another entity than the
  world for new brushes.

The model previews need the model in the project's folder or in the game the Play loop uses.
