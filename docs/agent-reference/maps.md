# Maps for the agent

## The .map format

Sources: [`docs/spec/map.md`](../spec/map.md) (MAP-1 to MAP-6).

- A `.map` is text: entities in `{ }`, each a list of `"key" "value"` pairs, one to a line,
  and for a brush entity its brushes, each in `{ }` of its own (MAP-2.1, MAP-2.2). A key
  holds at most 30 bytes and a value, in practice, at most 127 (MAP-2.4, MAP-1.6); a map
  holds at most 1024 entities (MAP-2.8).
- A brush is a convex solid: one face per line, three points `( x y z )` on its plane, then
  the texture name and its shifts, rotation and scales (MAP-3.2, MAP-4.1). The plane's
  normal is `(p0 - p1) x (p2 - p1)` and must point out of the brush (MAP-3.4). qbsp reads
  coordinates as integers (MAP-3.3), so keep them whole. Valve 220 faces (`[ ... ]` axes) need ericw-tools; id's qbsp does not
  read them (MAP-5.5).
- The first entity is the world, `worldspawn`, which holds the level's own brushes
  (MAP-6.1). Every later entity with brushes is a brush entity, given the model `*1`, `*2`,
  and so on in file order (MAP-6.2). The rest are point entities placed by `origin`, three
  numbers (MAP-2.7).
- Keys starting with `_` are for the compile tools, and the game ignores them (MAP-6.7).

## Units and the player

Sources: id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73 `qcc/v101qc`
(`defs.qc`, `client.qc`, `subs.qc`); [`docs/spec/engine/physics.md`](../spec/engine/physics.md);
[`docs/spec/bsp.md`](../spec/bsp.md) (BSP-11.3).

- Distances are in Quake units, and the z axis points up. The player's box runs from
  `'-16 -16 -24'` to `'16 16 32'` (`VEC_HULL_MIN`, `VEC_HULL_MAX`), 32 units wide and 56
  tall, with the eyes 22 units above the origin. A gap the player walks through must be at
  least 32 wide and 56 tall. Large monsters such as the shambler use hull 2, `'-32 -32 -24'`
  to `'32 32 64'`.
- Walking climbs a step of up to 18 units (`STEPSIZE`, PHYS-8.3). Use stairs of 16 or less.
- A jump adds 270 to the vertical speed (`PlayerJump`: `self.velocity_z = self.velocity_z +
  270`), and gravity is 800 (`sv_gravity`, PHYS-6.4). That makes the peak 270² / (2 · 800),
  about 45 units: a ledge of 40 is a safe jump.
- A swimmer climbs out of a liquid only over an edge at most 28 units above its surface.
  `CheckWaterJump` runs only while `self.waterlevel == 2` (waist deep, PHYS-8.4); it needs
  solid 24 ahead at `origin + 8` and open space at `origin + 8 + self.maxs_z - 8`, then sets
  `self.velocity_z = 225` (`client.qc`). Run in `qs-sim`, a swimmer climbs out over 28 and
  not over 30, in water and in lava alike (`crates/qs-tests/tests/it/water_escape.rs`;
  `qs_map_gen::CLIMB_OUT`). A pit of liquid whose rim is higher, or a dry pit deeper than a
  step with no stairs, is a trap: give it a way out.
- Lava does `10*self.waterlevel` every 0.2 s, up to 150 a second, and slime
  `4*self.waterlevel` every second (`WaterMove`), so a player who falls into lava dies
  before climbing out. Rail lava off from where the player walks: a rail higher than a step
  stops a walking player, and only a jump goes over it.
- A fall does at most 5 damage, when it lands faster than 650 (`PlayerPostThink`:
  `self.jump_flag < -650`), so a drop is a safe one-way way down.
- An `info_player_start` is required: `PutClientInServer` stops with "no
  info_player_start on level" without one (`client.qc`). Put its origin 24 units above the
  floor, where the box's bottom rests.

## The entities a map needs

Sources: the same `client.qc`; `qutils/LIGHT/ENTITIES.C` and `ENTITIES.H` at the same commit.

- `worldspawn` with `wad`, the texture WAD's path relative to the game directory; qbsp reads
  `_wad` first, and ericw-tools takes several paths separated by `;` (MAP-6.3). `message`
  is the level's title (MAP-6.5).
- `info_player_start`, as above, and `info_player_deathmatch` for deathmatch.
- `light` entities. The `light` tool lights only from them; `light` (or `_light`) is the
  brightness, 300 when not given (`DEFAULTLIGHTLEVEL`).
- The classes a mod spawns are its QuakeC functions of the same name. `map.summary` counts
  the open map's classes, and `project.entities` describes each class the project defines.

## Textures

Sources: [`docs/spec/map.md`](../spec/map.md) (MAP-4.5, MAP-4.6, MAP-6.3).

- A face's texture names a texture in the world's WAD. Use only names that WAD holds, and
  read the WAD rather than guessing. A name holds at most 15 bytes (MAP-4.6).
- A name starting with `*` is a liquid (water, slime or lava), and one starting with `sky`
  is the sky; neither gets a lightmap (MAP-4.5). Trigger brushes are hidden whatever their
  texture, because `InitTrigger` (`subs.qc`) clears their model; `trigger` is the usual name
  for them.

## Leaks

Source: `qutils/QBSP/OUTSIDE.C` (`FillOutside`) and `QBSP.C` at the same commit.

- The level must be sealed: no gap from any point entity to the void outside it. qbsp
  floods the outside from the map's edge. When the flood reaches an entity, it prints
  "reached occupant at" with the entity's origin, writes the path to `<map>.pts` and does
  no filling. The map still compiles, but every face outside is kept, so vis and light are
  slow and wrong.
- Seal the world with brushes on every side. Each entity's origin, other than the world's,
  must lie inside, and an entity at `'0 0 0'` is not counted (`FillOutside` skips it).

## What the rooms of id's and LibreQuake's levels are made of

Source: the levels' BSPs, measured by [`scripts/measure-levels.py`](../../scripts/measure-levels.py)
on 2026-11-21: id's from `id1/pak0.pak` of the original game, LibreQuake's from the test
game's `pak0.pak` and `pak1.pak`. A height is taken where a monster, an item or a player
start stands: from the floor under it to the ceiling over it, in the player's hull. A slanted
face is one whose plane is on none of the three axes. A figure "of the middle level" is the
median over the levels.

| | id's 32 levels with monsters | LibreQuake's 10 |
| --- | --- | --- |
| Height where things stand: a quarter, half and three quarters of them, of the middle level | 136, 200, 288 | 184, 280, 360 |
| Slanted faces, of all faces | 14% of the middle level; 4% (`e1m8`) to 33% (`end`) | 23%; 9% (`lq_e0m4`) to 51% (`lq_e0m3`) |
| Lights | 260 in the middle level, half of them of brightness 200 or less | 100, of brightness 300 |
| Monsters | 56 | 57 |
| Doors, lifts and trains, buttons | 23, 3, 6 | 12, 1, 2 |

- **No level is one height.** In `e1m1` half of what stands is under a ceiling of 184 or
  less, a quarter under 128 or less and a quarter under 216 or more, up to 392; in `e1m2`
  the same three are 160, 192 and 288, up to 576. A level of rooms all 128 high reads as a
  corridor: give the rooms that matter 256 and more, and keep passages at 112 to 128.
- **About one face in seven is slanted in id's levels**, one in four in LibreQuake's: cut
  corners, arches, sloped ceilings and ramps. A level of boxes has none.
- **Light is many small lights, not a few large ones**: `e1m1` has 252 lights for 6148
  faces. A sample at distance `d` from a light gets `(light - d) * ((1 - scalecos) +
  scalecos * cos)`, `cos` the cosine between the face's normal and the direction to the light
  (LIGHT-4.3, [`docs/spec/light.md`](../spec/light.md)), and `scalecos` is 0.5 (LIGHT-1.2). So
  a light adds nothing to what is `light` units or more from it, half as much at half that
  distance as beside it, and half as much to a wall it grazes as to one it faces: a light
  of 300 hung 300 above a floor leaves it dark. Put lights within 150 of what they light.
- **A level is busy with things that move**: `e1m1` has 21 doors, 2 lifts and 6 buttons.

## Rooms other than boxes

Source: [`crates/qs-map-gen`](../../crates/qs-map-gen/README.md) (L-49).

`map.build_room` builds one room of a shape into the open map, sealed but for the doorways
asked for, with its lights; `map.build_passage` joins two doorways. Each is one undo step.

| Shape | What it is | Smallest half-width | Smallest height |
| --- | --- | --- | --- |
| `hall` | Four pillars in from its corners, the ceiling raised in the middle, pilasters with torches | 192 | 192 |
| `chamber` | Eight walls under a dome of two steps, a dais of two steps in the middle; as wide as long | 224 | 192 |
| `pit` | A walk around a pit of liquid, two bridges crossing in its middle, beams under the ceiling | 224 | 192 |
| `split` | The part toward the side it is `turned` to raised by 64, stairs up to it in the middle | 224 | 256 |
| `court` | High walls open to the sky, an obelisk in the middle | 192 | 256 |
| `cross` | Two aisles 256 wide through the middle, the one toward `turned` the higher, a lantern over the crossing | 224 | 192 |
| `vault` | A long room under a pitched ceiling, its ridge toward `turned` | 192 | 224 |

- Give `map.build_room` the room's `middle`, `floor`, `half` widths and `height`, all
  multiples of 16, and the `doors`: the sides (`east`, `north`, `west`, `south`) to open. The
  room takes 64 of rock more than its half-widths on every side, so no other room or passage
  may come within that; put the middles of two rooms side by side 1024 apart and there is
  room between any two shapes for a passage with stairs.
- It answers each doorway's place (`at`): the middle of the opening on the room's outer
  face, at the floor there. A doorway of a `split` on the side it is turned to is 64 above
  its floor.
- `map.build_passage` takes two of those places that face each other along x or along y, and
  builds the passage between them with stairs when their floors differ, 16 a step. It needs
  a length of 64 and the climb: 128 for a climb of 64.
- It also answers where things may stand (`spots`) and where the player does (`stand`): put
  the player start, the monsters and the items there with `map.place_entity`.
- Keep the whole level within 4096 units of 0 on every axis: the game sends a position as
  the short `(int) (f * 8)` (NET-1.1, [`docs/spec/engine/net.md`](../spec/engine/net.md)), so
  a player or a monster past that is drawn somewhere else. Rooms 1024 apart fit seven in a
  row from -3072 to 3072; `map.issues` says when an entity is past it.
- Build a level from these, no two rooms of it the same shape and size, and vary the
  floors: a room 64 up or down from the last is four steps in the passage. `map.compile`
  says whether it is sealed, and `map.play_from_here` whether anything stands in the dark. With no
  `eye` it starts the player at the map's own `info_player_start`, as the game does.

## A whole level at once

Source: [`crates/qs-map-gen`](../../crates/qs-map-gen/README.md) (L-48, L-74).

- `map.generate` makes a random playable level in the project and opens it. One `seed` and
  `size` (`small`, `medium`, `large`) give one level, the best of eight plans drawn from the
  seed by a score of its loops, floors, teasers, secret, kinds of lock and shapes of room.
- Its rooms are on storeys 128 apart. The path hooks back on itself: it leaves an `atrium`
  on its floor, climbs a storey through two rooms and comes onto the atrium's balcony, which
  the player has seen from below and drops from, one way, back to its floor.
- Its mission is locks along the path, each opened from a room before it: the silver and
  the gold key, a button that opens a door elsewhere, and an arena whose door opens when its
  monsters are dead. A secret room behind a wall that opens when shot holds a megahealth; a
  teleporter in the room before the exit takes the player back to the start; monsters just
  past a lock lie in ambush. Rooms the same locks have opened are joined into loops, and a
  passage that climbs a storey may have a lift.
- Rooms differ in scale: great rooms as wide as their cell (up to 704 across), often
  courtyards, beside ordinary rooms and `junction`s where a corridor turns; two great rooms
  side by side open into each other through an arch. Each level takes a `form` besides its
  hook: a `hub`, a great room early on with its keys' and buttons' rooms off it; a `lake`, a
  courtyard with a lake of water and a walk around it; or a `canyon`, three courtyards open
  into each other under the sky.
- It answers each room's `shape` and a place to `stand` in it, and what the mission came to:
  `floors`, `loops`, `locks`, `teasers`, `secrets`, `form`. Look at a room with `map.play_from_here`,
  the eye 22 units above that place, and `engine.screenshot`.
- Rooms are dressed with parts on top of their shapes (`map.build_room` takes them as
  `parts`): `bays` out of walls with no doorway, needing a wall of 112, `braziers` in them,
  a `cornice`, a `skirting`, `columns`, `beams` across a ceiling or a court's sky, `fixtures`
  (a lamp panel of the theme's light texture over each main light and on each pilaster where
  its light is: id's `tlight` in a base, `light1` in a city, `light3` in a runic level) and
  `frames` of trim around the doorways. Every room has fixtures and frames; the rest are
  chosen by the theme. A frame goes around neither a secret door, which it would show, nor a
  lift passage's end, which is open to the passage's full height.
- No level holds a trap: every pit's liquid comes to within 28 of its rim, and lava is
  railed. The check of every generated level in `crates/qs-tests` (`no_trap.rs`) requires the
  exit reachable from every place and set of keys and switches the start reaches, by
  walking, steps of 18, falls, swimming, climbing out within 28, lifts and teleporters.
- The level is a map like any other: edit it with the map tools, and `map.compile` and
  `map.issues` check it.

## Checking what a map's triggers do

A key that names nothing, and a `targetname` nothing names, are in `map.issues`. What happens
when a trigger fires is in the QuakeC, so run it rather than read it:

- `map.fire` with `entity` (its place in the map) fires one entity in the simulation, its `use`
  or, when it has none, its `touch`, with the player as `other` and `activator`, and answers
  the trace: each `use`, `touch` and `think` the firing reached with the entity and the time,
  each lookup by `targetname` with what it found, each entity removed or spawned, each
  `centerprint` and sound, and the fields written. `skill` and `mode` choose what is spawned,
  `by` is `use`, `touch` or `either`, and `seconds` is the longest the trace runs (10 when not
  given). A door that should open shows its `use` and then its `think`s; a door that does not
  shows where the chain stopped.
- `map.logic_issues` fires every entity that has targets and lists the chains that cannot
  work: a target with no `use` function, a chain that removes what it then fires, a chain that
  loops or stops the game, and a counter fired by fewer entities than its count. Run it after
  wiring buttons, doors, counters and relays, before the map is played.

Both need the project's server QuakeC to compile, and neither changes the map or a running
game. The rules are [`docs/spec/fire.md`](../spec/fire.md).

## Checking that a map can be finished

Source: [`crates/qs-map-gen`](../../crates/qs-map-gen/README.md), `playable` (L-77, MAP-36).

- `map.playability` compiles the open map and searches the places a player reaches from the
  `info_player_start`. It answers `exit` (`reached`, `not_reached`, or `no_exit` for a map
  with no `trigger_changelevel`), `unreached` (each key, weapon, item and secret never reached,
  with its entity), `traps` (each place a player can come to and not leave, `at` a point inside
  it, `min` and `max` the box of the player's origins there), and `not_judged` (each entity whose
  effect on a player the check does not follow). Run it after a compile, and fix every finding
  before the map is played: a trap is a pit or a one-way drop with no way back to the exit.
- `moves` is `player` (the default: steps of 18, a jump onto a ledge of 43 and across a gap of
  240, falls, swimming and climbing out over a rim of 28, lifts, trains and teleporters) or
  `generator` (no jump, so a ledge a player must jump onto counts as a wall). `skill` (`easy`,
  `normal`, `hard`, `nightmare`) chooses the entities the game keeps (EDICT-8.4).
- Doors open as id's QuakeC opens them: with their key, when something fires their
  `targetname`, when shot (`health`), or when touched. An open door is solid where it opened
  to, so one that sinks into the floor leaves a step; one with `wait` other than -1 comes back.
  A `func_door_secret` is out of the way once opened. A wall a `killtarget` names is removed
  when it fires. A monster with a `target` is taken to die once the player sees it, or once its
  `targetname` fires when it waits in a closet. An item is where `droptofloor` puts it and
  rides the door under it.
- The findings join `map.issues` until the map changes. `map.generate` checks each level it
  makes the same way, with `generator` moves, and moves on to the next seed when it finds a
  trap.
