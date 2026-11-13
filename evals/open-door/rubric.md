# Open a door that will not open: reviewer rubric

A run whose gate opens when the player walks into it is then read by a reviewer against these:

- The agent read the gate's keys in the map, with `map.inspect` or the map's text, and found its
  `targetname` with nothing firing it.
- The agent read `func_door` and `LinkDoors` in `doors.qc` and said why: a door with a
  `targetname` spawns no trigger field and opens only when a button or trigger fires it.
- The fix is in the map, not the QuakeC: the `targetname` removed, or a trigger in front of the
  gate that targets it; nothing else changed. The map was saved with `map.save`.
