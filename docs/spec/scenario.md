# Scenarios

Prefix: `SCN` (registered in [`README.md`](README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the deterministic scenario: a file naming a map, a seed, a player start, timed
inputs, console commands, assertions on edicts and globals, and screenshots, which
`scenario.run` plays in `qs-sim` and answers with a structured result (P3-19, AGT-10, AGT-2).
It is this project's design, not engine behaviour: what each frame does is `qs-sim`'s, as
`docs/spec/engine/` says. Implemented by `qs-sim` (`scenario`, the file and the run) and
`qs-workbench` (`scenario.run`, the command, and its screenshots).

## SCN-1 The file

**Source:** this design; the engine oracle's route script (`oracles/README.md`, DIFF-16.1),
which a scenario extends, so that every route is a scenario.

### Behaviour

- **SCN-1.1** A scenario is UTF-8 text read line by line, a `\r` before the line's end left out.
  An empty line or one starting with `#` is skipped. Every other line is a word, then a space
  and the rest of the line when it takes one; a word not listed here is refused with its line
  number, and so is a line whose rest does not read as its word says.
- **SCN-1.2** The lines a route has mean what DIFF-16.1 gives them: `map <name>` the map, once
  at least, the last one counting; `frametime <seconds>` the server frame's length, above 0
  and at most 1, 1/72 when absent; `engine <word>` one word of the engine's command line;
  `cmd <text>` a client string command and `console <text>` a line typed at the server's
  console, each read in the next frame; `move <pitch> <yaw> <roll> <forward> <side> <up>
  <buttons> <impulse>` the move sent each frame from then on, its angles numbers, its speeds
  whole numbers that fit a signed 16-bit value and its buttons and impulse whole numbers that
  fit a byte, the impulse sent once; and `frames <count>` that many server frames, from 1.
- **SCN-1.3** `seed <n>`, `n` a whole number from 0 to 4294967295, is the state the server's
  random source starts at in place of MOVE-6.1's 1; at most one `seed` line is read, and a
  scenario without one starts at 1, as a route does.
- **SCN-1.4** `start <x> <y> <z> <yaw>` moves the player, once it has entered the game, to that
  origin, facing that yaw with pitch and roll 0, with no velocity, as a teleporter does; at
  most one `start` line is read.
- **SCN-1.5** `expect <target> <op> <value>` is an assertion, checked when the steps before it
  have run. `target` is `global <name>`, a named global, or `field <edict> <name>`, a named
  field of an edict, the edict a number, `player` (edict 1) or `classname=<name>`, the
  lowest-numbered edict in use with that classname. `op` is `==` or `!=` for any value and `<`,
  `<=`, `>` or `>=` for floats. `value` is a number, a vector `'x y z'`, a string in double
  quotes, or an edict `#n`. Floats and the components of vectors are compared as `f32`
  values, exactly.
- **SCN-1.6** `screenshot <name>`, `name` letters, digits, `-` and `_`, asks for the frame the
  player sees when the steps before it have run: the eye, the player's origin plus its
  `view_ofs`, and its `v_angle`, are kept with the name.

## SCN-2 The run

**Source:** this design; DIFF-16.1's play of a route, which a scenario's run follows step for
step.

### Behaviour

- **SCN-2.1** A scenario runs in `qs-sim` in the process, with no window and no sound: the map
  is loaded from the game's search path with id's builtin table, the world settles, and each
  step runs in order, the client connecting in frame 1 and answering each signon stage in the
  next frame as DIFF-16.1 says, so the player enters the game in frame 4. `start` takes effect
  before the first frame after that.
- **SCN-2.2** An assertion passes when its target has a value and the comparison holds. It
  fails, with the value read, when the comparison does not hold; and when the target has no
  value (no such global or field, an edict not in use, no edict with the classname) or the value
  is of another type than `value`, saying so. A failed assertion does not stop the run.
- **SCN-2.3** A run error in a frame stops the run there: the result says which frame and why,
  and the assertions after it are not checked.
- **SCN-2.4** The result says whether the scenario passed, every assertion having passed and no
  frame having failed; how many frames ran; each assertion with its line, its text, whether it
  passed and the value read or why there is none; and each screenshot with its name, the frame
  it was asked for and the eye.
- **SCN-2.5** The same scenario on the same game gives the same result on every run: the
  server's frames have a fixed length, the random source starts at the scenario's seed, the
  client's inputs are the scenario's, and nothing reads the clock.
- **SCN-2.6** `scenario.run` builds the open project and plays the scenario on its program,
  over the game the preview plays. When a screenshot was asked for and the preview host is
  there, it starts the host headless (P3-05) on the same map, pauses it, and for each
  screenshot moves the player to the kept eye (`player/spawn_at`), steps one frame and takes a
  `screenshot`, writing the PNG beside the scenario as `<scenario>-<name>.png`. The image is
  the map seen from where the player was, in the host's own world; with no host, the result
  keeps each screenshot's eye and writes no image.
