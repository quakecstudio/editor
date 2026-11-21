# qs-map-gen

Makes a random playable level from a seed (L-48): `Request::generate` gives a `.map` with a
start, an exit, lights, monsters, weapons and a key behind a locked door. One seed gives one
level.

- **The plan** (`layout`) is OBLIGE's model: rooms on a grid joined into a path from the start
  to the exit, rooms off the path, the key in one of those and its door further on.
- **The rock** (`mass`): each room's cell starts as a solid block, and every room and passage
  is subtracted from the blocks it reaches (`qs_map::Brush::subtract`), so a level is sealed by
  what was never removed. Spaces have their corners on whole units and faces that are level,
  upright or at half a right angle, so every corner the subtraction makes is on the grid
  (MAP-3.3).
- **The rooms** (`rooms`) are the stock shapes of id's levels: a pillared hall under a raised
  ceiling, an eight-sided chamber under a stepped dome, a hall around a pit of liquid crossed
  by bridges, a room on two levels joined by stairs, a courtyard under the sky, two aisles
  crossing under a lantern, and a long room under a pitched vault. **Passages** (`passage`) are
  square or arched, with stairs where the floors differ and a door at the higher end.
- **The textures** (`theme`) are a theme's, chosen from the names the game has: themes for
  id's levels and for LibreQuake's, else any plain textures. **The entities** (`populate`) are
  id's classnames (`qcc/v101qc`), and only those the game defines are placed.

- **The kit** (`kit`, L-49) builds one room or one passage on its own, for a map built by
  hand or by an agent: `RoomKit::build` gives a room in a block of rock of its own, sealed but
  for the doorways asked for, and `PassageKit::build` joins two doorways, with stairs where
  their floors differ. Rooms and passages that meet at a doorway seal each other. The commands
  are `map.build_room` and `map.build_passage`, in `qs-map-edit`; the shapes are compiled and
  walked in `crates/qs-tests/tests/it/room_kit.rs`.

Layer: L4. Nothing is ported. The command is `map.generate`, in `qs-map-edit`; the levels are
compiled and walked in `crates/qs-tests/tests/it/random_levels.rs`.
