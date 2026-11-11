# The play loop

The play loop is QuakeC Studio's centre: the game runs while you edit, and each save reaches it
in about a second, the map going on.

## The toolbar

| Button | What it does |
| --- | --- |
| **Play** | Builds the project and starts the game on its program, on `start`. While the game runs it reads **Playing**, and a click stops the game. |
| **Pause** | Pauses the game between frames. It reads **Paused** while the game is paused, and Play's place reads **Resume**. |
| **Step** | Runs one server frame of the game, pausing it first, and leaves it paused. |
| **Restart map** | Starts the running map again. |

The game runs in the preview host, QuakeC Studio's own engine, built on the same VM as the
IDE. It has a window of its own, docked beside the IDE's, which follows the IDE's window when
you move it.

## Save to reload

Each time you save a file while the game runs, the IDE builds the project and reloads the new
program in the game. The map, the monsters and the player stay where they were; the new code
runs from the next frame. The toolbar says how long the reload took.

Some changes cannot keep everything. If the new program drops or retypes a field or a global
that holds a value, the IDE does not reload at once. A bar under the toolbar says **Reloading
would reset**, with what would be lost, and offers **Reload anyway** or **Restart map**.

If the project does not build, nothing is reloaded: the game keeps running the code it had,
the toolbar says **The reload failed; see the Console**, and Problems lists the errors.

## Entities and Properties

The **Entities** panel, on the left, lists every edict in use in the running game, grouped by
classname, with a filter. Click one to select it.

**Properties**, on the right, shows the selected edict's fields. While the game runs they are
read only. Pause, and each field can be edited: type a new value and press Enter. The new
values are sent to the game when you resume. A field that holds a function, such as `think`,
is a link to that function's definition, and **Open its spawn function** opens the function
named after the classname.

While the game is paused, a click in its window selects the edict under the pointer and opens
its spawn function.

## The console

The **Console** tab, at the bottom, shows what the game prints, its errors marked, with a
filter. Its box sends a line to the game's console, such as `impulse 9` or `map e1m1`.

## Tests

The project's tests are its `test_` functions, each run alone in the VM, and its
`tests/*.scenario` files, each played in the server simulation on the game's own maps. A
scenario names a map, the inputs, and what must hold after so many frames:

```text
map start
frames 4
expect field player health == 100
```

Run them all with `project.test` from the palette, or `quakec-ide test <folder>`.
[`docs/spec/scenario.md`](../spec/scenario.md) describes every line a scenario can have.
