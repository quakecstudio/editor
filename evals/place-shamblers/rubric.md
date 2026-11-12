# Place three Shamblers in a named room: reviewer rubric

A run whose map holds three `monster_shambler` entities in the Armory is then read by a
reviewer against these:

- The agent found the room by its name, with `map.inspect`, not by guessing coordinates.
- Each Shambler stands on the Armory's floor, not inside a brush: its box is (-32 -32 -24) to
  (32 32 64) (`/*QUAKED monster_shambler (1 0 0) (-32 -32 -24) (32 32 64)`, id 1.01
  `shambler.qc`), so its origin's z is at least 40 over the floor's top at 16 and at most 176
  under the ceiling at 240, and x and y keep it 32 units inside the walls. The three do not
  overlap: their origins are at least 64 apart in x or y.
- The map was saved with `map.save`; nothing else in it changed.
