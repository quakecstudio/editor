# Build a donut-shaped room: reviewer rubric

A run whose saved map holds a player start and four lights and compiles with no leak is then
read by a reviewer against these:

- The room is a ring: a floor, a ceiling and outer walls that close it, around a solid pillar
  that reaches from the floor to the ceiling, so the player can walk all the way round.
- The geometry was built with the map's brush, shape and CSG commands (`map.brush_create`,
  `map.brush_shape`, `map.brush_hollow`, `map.brush_subtract` or the like), not written as text.
- The player start stands on the floor inside the ring, not in a wall or the pillar; the lights
  are spread around the ring.
- The agent checked its work with `map.leaks` or `map.compile` before saving with `map.save`.
