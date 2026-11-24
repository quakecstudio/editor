# qs-map-gen

Makes a random playable level from a seed (L-48, L-74): `Request::generate` gives a `.map`
on several floors with a start, an exit, loops, locks and what opens them, a secret, lights,
monsters and weapons, and no trap a player cannot leave. One seed gives one level.

- **The plan** (`layout`): rooms on a grid of columns, each on a storey 128 apart, joined into
  a path from the start to the exit. The path hooks back onto an atrium's balcony, a place
  seen before it is reached and dropped from one way, since OBLIGE's author found a generator
  for Quake needs its floors in the plan from the start. A passage that climbs a storey may
  have a lift (`func_plat`).
- **The mission** (`mission`), as Dormans' grammars and Unexplored's cyclic generator have it:
  locks spread along the path (the silver and the gold key, a button opening a door
  elsewhere, an arena whose monsters' deaths fire a `trigger_counter`), what opens each off a
  room before it, a secret room behind a `func_door_secret`, a teleporter back to the start,
  monsters in ambush past a lock, and loops between rooms the same locks have opened.
- **The score** (`score`): the best of eight plans drawn from the seed is kept, by its loops,
  floors, teasers, secret, kinds of lock and shapes of room.
- **The rock** (`mass`): each room's cell starts as a solid block, and every room and passage
  is subtracted from the blocks it reaches (`qs_map::Brush::subtract`), so a level is sealed by
  what was never removed. Spaces have their corners on whole units and faces that are level,
  upright or at half a right angle, so every corner the subtraction makes is on the grid
  (MAP-3.3).
- **The rooms** (`rooms`) are the stock shapes of id's levels as presets: a pillared hall, an
  eight-sided chamber under a stepped dome, a hall around a pit of liquid crossed by bridges,
  a room on two levels joined by stairs, a courtyard under the sky, two aisles crossing under
  a lantern, a long room under a pitched vault, and an atrium with a balcony. **Their parts**
  (`rooms/parts`) build on a preset by a grammar: bays grow its footprint, braziers sprout in
  them, cornices, skirtings, columns and beams decorate it, chosen by the theme within a brush
  budget, and every room has lamps of the theme's light texture where its lights are and
  frames around its doorways, as id's rooms do. **Passages** (`passage`) are square or arched, with stairs or a lift where the
  floors differ and a door at the higher end.
- **No traps.** A pit's liquid is within `CLIMB_OUT` (28) of its rim, the most a swimmer
  climbs out over (`client.qc`, `CheckWaterJump`), and a pit of lava, which kills before a
  player climbs out, has rails along its edge and its bridges. `crates/qs-tests` checks each
  level compiled: from every place and set of keys the start reaches, the exit can still be
  reached (`no_trap.rs`), and measures the climb out of water in `qs-sim`
  (`water_escape.rs`).
- **The textures** (`theme`) are a theme's, chosen from the names the game has: themes for
  id's levels and for LibreQuake's, else any plain textures. Each theme paints a floor or a
  wall only with a texture the game's own levels mostly put on such faces, and a ceiling with
  one they put on ceilings a third of the time or more, by area measured from their BSPs
  (`scripts/measure-textures.py`, `theme/tests.rs`). **The entities** (`populate`) are
  id's classnames (`qcc/v101qc`), and only those the game defines are placed.

- **The kit** (`kit`, L-49) builds one room or one passage on its own, for a map built by
  hand or by an agent: `RoomKit::build` gives a room in a block of rock of its own, sealed but
  for the doorways asked for, and `PassageKit::build` joins two doorways, with stairs where
  their floors differ. Rooms and passages that meet at a doorway seal each other. The commands
  are `map.build_room` and `map.build_passage`, in `qs-map-edit`; the shapes are compiled and
  walked in `crates/qs-tests/tests/it/room_kit.rs`.

Layer: L4. Nothing is ported; the entity rules followed are quoted in
`docs/agent-reference/triggers.md` ("Puzzles from id's entities"). The command is `map.generate`, in `qs-map-edit`; the levels are
compiled and walked in `crates/qs-tests/tests/it/random_levels.rs`.
