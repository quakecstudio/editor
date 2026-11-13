# Find and fix a leak: reviewer rubric

A run whose saved map compiles with no leak is then read by a reviewer against these:

- The agent found the leak with `map.leaks`, from its trail, not by reading every brush.
- The hole was filled with one brush in the north wall's line, 224 to 288 in x and 512 to 528 in
  y, from the floor's top at 16 to the ceiling at 240; the room inside is unchanged.
- The map was saved with `map.save`; nothing else in it changed.
