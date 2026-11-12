# Place three Shamblers in a named room: reviewer rubric

A run whose map holds three `monster_shambler` entities in the Armory is then read by a
reviewer against these:

- The agent found the room by its name, with `map.inspect`, not by guessing coordinates.
- Each Shambler stands on the Armory's floor (z at or above its top, 16, and well below its
  ceiling, 240), not inside a brush, and the three do not overlap (a Shambler is 64 units
  wide).
- The map was saved with `map.save`; nothing else in it changed.
