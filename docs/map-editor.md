# The map editor

The map editor (P5-06, MAP-4, MAP-20, MAP-22) opens a `.map`, or the entities of a shipped
`.bsp` that has no `.map` source, from the Project tree or with `map.open`. It shows the map in
the Map tab and the selected entity's keys in the Properties panel. Every edit is a `map.*`
command ([`commands.md`](commands.md)), so each one is undoable and the agent and MCP can make
it too.

- **What it draws:** a `.map`'s brushes, cut from their planes (MAP-3.4), or a BSP's faces, as
  lines. Each point entity is a box in its class's colour, at the size its `/*QUAKED*/` comment
  or `setsize` gives (`project.entities`, P5-03). When the project's folder or the game holds
  the class's model, the first frame of that model is drawn at the entity, turned to its
  angle: as a wireframe in Lines, and with its first skin in Textured and Lit, as TrenchBroom
  draws models (P7-08). The selection is drawn in the accent colour.
- **The key editor:**
  - It lists the entity's pairs with their values, and a Remove button for each.
  - It shows the keys the entity's class reads but does not set, each with its type and
    default. Clicking one starts a new pair with that key.
  - A target key's value can be picked from the map's targetnames.
  - `map.check` (P5-04) reports the keys and classnames the project does not define.
- **Placing and linking (P5-13, MAP-27):**
  - The Entities browser below the view lists the classes the project's QuakeC defines, each
    with its model's wireframe, or its box where it has no model, in its class's colour. A
    point class dragged into the view is placed (`map.place_entity`) 256 units ahead of the camera
    along the pointer's ray, on the 16-unit grid. A brush class is listed, but is made from
    brushes.
  - Lines join each entity to the entities its keys fire, for the selection or for every
    entity (Links). The keys are those the QuakeC compares with `targetname` (MAP-8.8), so
    `killtarget` and a mod's own keys link as `target` does.
  - Pick, beside such a key, sets it to the next entity clicked (`map.link_target`). An Alt-drag from
    the selection onto another entity sets the key last picked, else the shortest such key
    (`target` in id's QuakeC). An entity with no `targetname` is given its classname and the
    first number no entity has as its `targetname`.
  - Duplicating an entity whose `targetname` ends in a number gives the copy the next free
    number, and moves the numbers of its target keys on by as many, so a copied link in a
    chain names the next one. Zero padding is kept: `t09` becomes `t10`, `t010` becomes `t011`.
- **The fields (P5-13, UI-24):** spawnflags are checkboxes named as the QuakeC names them, with
  the engine's own after them on every class but `worldspawn` (Not in easy, Not in normal, Not
  in hard or nightmare, Not in deathmatch, EDICT-8.4), `angle` has Up and Down, a key ending in
  `color` has a colour picker that keeps the key's scale (0 to 1 or 0 to 255), and a float the
  map gives several values offers those values. A targeting key offers the map's targetnames as
  it is typed, each with the classnames of the entities that have it, and marks a name no entity
  has; `classname` offers the project's classes, each a point or a brush entity. A new key
  offers, as it is typed, the keys the class reads and the entity does not set, each with its
  type, default and what the class's `/*QUAKED*/` comment says of it, and taking one fills in
  its default; any other key, such as a compiler's `_minlight`, is typed and added as before.
  The arrow keys and Enter, or a click, take an offer. Shift+click adds an entity to the
  selection; a key edit then sets every selected entity in one undo step (`map.set_key` with
  `also`), and a spawnflag ticked or cleared changes that bit alone in each entity's own flags
  (`map.set_flag`).
- **Show (P5-13):** the view can show only the entities the game spawns on easy, normal, hard or
  nightmare, or in coop or deathmatch. `map.entities_removed_by_skill` compiles the project's server QuakeC in
  memory, writing nothing, and spawns the entity lump qbsp would write for the map in `qs-sim`
  with those settings, in the map compiled in memory so that doors and other brush entities
  have their models, so what the spawnflags rule out and what the QuakeC removes itself are
  both hidden. It is found again after each edit.
- **Fire (L-76, MAP-35):** `map.fire` fires an entity as the game would, its `use` or, when it
  has none, its `touch`, in `qs-sim` with the project's server QuakeC, and answers what the code
  did about it in order ([FIRE](spec/fire.md)): each `use`, `touch` and `think` the firing
  reached, each entity looked up by its `targetname`, removed or spawned, each `centerprint` and
  sound, and the fields written, each entity with its place in the map. Time is simulated: a
  delay of two seconds is two seconds of the trace, which ends when nothing the firing reached
  has a think pending, or after the length given (10 seconds when none is). Nothing is read from
  a classname, so a mod's own triggers are traced as id's are. It changes neither the map nor a
  game that is playing it. `map.logic_issues` fires every entity that has targets, each in a
  copy of the spawned map, and lists what only running shows:
  - a target with no `use` function, so that firing it does nothing;
  - a chain that removes an entity and then fires it;
  - a chain that comes back to the same entity again and again, or stops the game with an
    error;
  - an entity that fires its targets only at a firing the map cannot reach, such as a
    `trigger_counter` whose `count` is above the entities that target it.

  The issues join the issue browser's until the map or the code changes.

  In the window, Fire is on the selected entity's context menu, beside its classname in
  Properties, and in the Logic tab below the view, which lists the trace step by step: a click
  on a step selects its entity, and the scrubber steps through the trace while the view draws
  the links fired so far in the warning colour. Check logic, in the same tab, lists the
  issues. The entity is fired at the skill and in the kind of game the view shows, normal
  single player when it shows every entity. A `.map` is compiled in memory first, as id's qbsp
  compiles it, so doors and other brush entities have their models; nothing is written.
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
  Dragging a row onto a layer or group moves it there (`map.move_to_layer_or_group`), into a linked group
  copied into the groups linked to it; the world and a layer stay where they are. An entity
  naming both a layer and a group is shown in the layer, as hiding and locking treat it (MAP-9.2);
  groups that name each other are shown under the default layer.
- **Brushes (P6-08, MAP-2):** the toolbar holds Select, Brush, Clip and Vertex. With a brush
  tool, a click selects the first brush the pointer's ray enters, by its faces, and Shift+click
  adds or removes one. Each edit is a `map.brush_*` command on the grid, undone in one step, and
  kept convex: one that would leave a brush with no volume is refused.
- **Compass (P7-08):** each pane's lower left corner shows the map's x, y and z axes as the pane
  sees them, in red, green and blue with their letters, as TrenchBroom's compass.
- **Map bounds (P7-08):** the 2D views draw the soft map bounds, -4096 to 4096 units on each
  axis, in the warning colour, the box TrenchBroom's Quake configuration draws: where players,
  items and other entities can be.
- **The camera (P7-08):** a right-drag looks and Alt+right-drag orbits about the point of the
  brush under the pointer where the drag began, or 256 units ahead when there is none, as
  TrenchBroom's camera tool (`shouldOrbit`, `Camera::DefaultPointDistance`). Shift and the wheel
  zoom the 3D view in for now; Reset zoom beside the camera's place puts it back (TrenchBroom's
  Ctrl+Alt+Z is undo here). Camera settings, under the camera's place, set the field of view (50
  to 150 degrees, 90 at first), the look, pan and fly speeds, and inverted look and wheel
  (`view.map_camera`, kept in the layout). Cmd+U frames the selection, every selected entity and brush, keeping
  where the camera looks (TrenchBroom's Focus on Selection, Ctrl+U). The Properties panel shows
  the camera's x, y and z, and a value typed or dragged there moves it.
- **The view's menu (P7-08):** right-click in the view for Group and Ungroup, Create brush
  entity (the selected world brushes as an entity of a brush class, `map.make_brush_entity`),
  Make structural (a brush entity's brushes back to the world, `map.make_structural`), Create
  point entity (one of a point class where the menu opened), Select all of the selection's
  class, Hide, Isolate and Move to layer, as TrenchBroom's map view menu.
- **Making groups and layers (P7-08):** Cmd+G (or Group in Properties) puts the selected
  entities and world brushes in a new group, named "Group 1" and on (`map.group`). With a
  group's own entity selected, Cmd+Shift+G (Ungroup) takes its contents out (`map.ungroup`),
  Cmd+Shift+D (Linked duplicate) copies it as a group linked to it (`map.link_duplicate`) and
  Separate unlinks it (`map.unlink_group`). With an entity of a linked group selected, the view's
  menu's "Extract into linked groups" moves it, and the same entity of every linked copy, into
  new groups linked to each other (`map.extract_linked`). Add layer, beside Layers, adds a layer
  (`map.add_layer`), and "New objects in" picks the current layer: a brush drawn goes into the
  layer's own entity and a point entity placed names it, as TrenchBroom's current layer. Each is
  written as TrenchBroom writes it.
- **Search (P7-08):** the Issues tab's search box takes text or a query in TrenchBroom's query
  language (`map.search`), and Find selects every match, as TrenchBroom's search replaces the
  selection. Text finds names, classnames, keys, values, textures, smart tags and layer and group
  names, ignoring case, `*`, `?` and `%` matching the whole value. A query such as
  `classname is "light" && properties.light > 300` finds entities, groups and brushes,
  `material like "sky*"` finds faces, and `type is layer && name is "Details"` selects what a
  layer holds. Nothing hidden or locked is found; hovering over the box shows the syntax.
- **Clip points (P7-08):** with a brush selected and the Clip tool, a click on one of its faces
  in the 3D view places a clip point there, on the face and its grid, and a click in a 2D view
  places one anywhere on the grid; a double-click on a face makes the clip plane that face's.
  Return clips the brush by the plane through the points (two guess a third straight up), and
  Escape takes the last point back, as TrenchBroom's clip tool. A drag still cuts along a
  line on the ground.
- **Extruding by drag (P7-08):** with a brush selected, Shift and a drag on one of its faces move
  the face along its normal, on the grid, as TrenchBroom's extrude tool; with Cmd as well the
  drag splits off a new brush (or, pushed in, cuts the brush in two), with Cmd+Alt it stamps a
  new brush of the face and its copy, and with Alt it moves the face's corners
  (`map.brush_extrude`'s modes).
- **Hull (P7-08):** the Hull tool places a point where each click meets a brush, or the ground,
  on the grid, and Return makes the convex brush around them with the current texture
  (`map.brush_hull`), as TrenchBroom's complex shape tool; Escape clears the points.
- **Panes (P7-08):** the view is the 3D view alone, with a top view inset, beside a top view
  ("2 views"), beside top and front views ("3 views"), or with top, front and side views ("4
  views"). The pane with the keyboard is outlined, and the 2D views are linked as TrenchBroom's
  are by default: panning or zooming one moves the others along the axes they share.
- **Picking behind (P7-08):** in the 3D view, with a brush selected under the pointer, Cmd and the
  wheel step the selection to the brush behind it (up) or in front (down), TrenchBroom's
  selection drilling; a click in a 2D view takes the smallest brush across it under the pointer,
  so one behind another can be picked.
- **Selecting (P7-08):** Cmd+A selects every entity and world brush not hidden, Cmd+Alt+A
  inverts the selection and Cmd+Shift+A clears it. With brushes selected, Cmd+T selects what
  they touch, Cmd+E what lies inside them and Cmd+Shift+E what lies inside them stretched along
  the view's axis, then deletes them, as TrenchBroom's Select Touching, Inside and Tall; Select
  siblings in Properties takes every brush of their entities (TrenchBroom's Ctrl+B is Compile
  here).
- **Flip and turn (P7-08):** Cmd+F flips the selection across the view, along the map axis
  nearest the camera's right, and Cmd+Alt+F up and down (TrenchBroom's Ctrl+F and Ctrl+Alt+F);
  Alt+Left and Right turn it a right angle about the vertical. With brushes selected, the
  Properties panel turns the selection by any angle, 15 degrees at first, and flips it along x,
  y or z. Both go about the middle of the selection's box, point entities' facing with it
  (`map.flip`, `map.rotate_objects`).
- **Moving on an axis (P7-08):** the Move tool moves the selection across the ground in the 3D
  view and across its own plane in a 2D view; Alt moves it up and down, and Shift keeps it on the
  axis it has moved farthest, as TrenchBroom's axis restriction.
- **Textures (P7-08):** the texture chosen in the browser is the current one, which a new brush
  or shape takes (else the map's last face's). With more than one WAD the browser shows one at a
  time or every one, and a WAD file dropped on the window joins the map's `wad` key. With faces
  picked (Alt-click), the arrows shift their texture by the grid (twice with Shift, 1 with Cmd)
  and Page Up and Down turn it 15 degrees (90 with Shift, 1 with Cmd), TrenchBroom's alignment
  keys. A texture's menu (right-click) replaces the
  current texture with it on the selected brushes, or on every brush when none is selected
  (`map.replace_texture`), as TrenchBroom's Replace Material.
- **Copy and paste (P7-08):** Cmd+C puts the selected entities and brushes on the clipboard as
  a `.map`'s text, where they are (`map.copy`); Cmd+V pastes map text with the middle of its
  floor at the pointer, on the grid, and Cmd+Alt+V where the text has it (`map.paste`), as
  TrenchBroom's Paste and Paste at Original Position. Text copied from a `.map` file pastes too.
- **Escape (P7-08):** in the view, Escape drops a drag in progress, else the Vertex tool's picked
  corners, else goes back to Select, else clears the selection, in TrenchBroom's order.
- **The grid (P7-08):** drawing, dragging and the selection keys snap to the grid, a power of two
  from 0.125 to 256 units, 16 at first, as TrenchBroom's. The toolbar's Grid picker sets it
  (`view.map_grid`), and while the view has the keyboard - halves it, + doubles it
  (`view.map_grid_step`) and 1 to 9 set 1 to 256 units, TrenchBroom's keys. The layout keeps it.
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
- **Reviewing the agent's map edits (P6-21, MAP-29):** the built-in agent builds geometry with
  the same brush, shape and CSG commands as the editor (`map.brush_create`, `map.brush_shape`,
  `map.brush_subtract` and the rest). An outside agent's map edits, through the MCP server,
  are made at once and begin no review (L-43); undo, or the agent panel's Restore checkpoint,
  takes them back. The built-in agent's first map command begins a review (`map.stage`): the
  map is edited at once, so a compile and the game see the change, and each run of changed parts
  since, an entity's keys or one of its brushes, waits as a hunk (`map.staged_hunks`). The Properties
  panel lists them with Accept and Reject, and both for every hunk; a rejected hunk's parts are
  put back as an edit undo takes back (`map.accept_hunks`, `map.reject_hunks`). Review ends when no hunk is
  left.
- **Compile profiles (P6-20, MAP-28):** a profile is a list of tasks run in order on the open
  map (`map.run_profile`), as TrenchBroom's compile profiles are: `bsp` (qs-map-bsp, the map as it
  is in the editor, to a BSP beside it with its portal and leak files), `vis` and `light` (on that
  BSP, each `full` or `fast`), `tool` (a program and its arguments, run in the map's folder),
  `copy`, `rename` (a file moved, its folder made), `delete` (files by a name with `*` and `?`,
  TrenchBroom's Delete Files), and `launch` (the BSP and its `.lit` copied into the project's
  build and the game started on it). A relative path in `rename` and `delete` is taken from the
  map's folder. Paths take `${MAP}`, `${MAP_DIR}`, `${MAP_NAME}`, `${BSP}`, `${PROJECT}` and
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
