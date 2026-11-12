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
- brush editing itself.

The model previews need the model in the project's folder or in the game the Play loop uses.
