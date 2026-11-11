# Debugging

QuakeC Studio's debugger stops the running game in your QuakeC, at a line you chose, and shows
the call stack and any expression you watch. It works in the game the toolbar's Play started.

## Breakpoints

Click a line number in the editor to set a breakpoint on that line; click it again to take it
away. The **Breakpoints** list, in the **Debugger** tab at the bottom, shows each one; a click
opens its line. Breakpoints are set in the running game at once, and in each game you start
later.

When the game reaches a breakpoint it stops in that frame. The editor shows the line, and the
Debugger tab fills in:

- **Call stack**: the function that stopped, and each function that called it. A frame the
  engine called from is marked `engine`.
- **Variables and watches**: each watch's value. Add one in its box, for example
  `self.owner.health`, and remove one with its **×**. Watches are read each time the game stops.

## Stepping

While the game is stopped in a frame:

| Button | What it does |
| --- | --- |
| **Step over** | Runs to the next statement of this function, calls included |
| **Step in** | Runs to the next statement, going into a call |
| **Step out** | Runs until this function returns to its caller |
| **Resume** (the toolbar) | Lets the game run on |

The toolbar's **Step** is different: it runs one whole server frame of a paused game.

## Looking at the game's state

Pause the game and look at the [Entities and Properties](play-loop.md#entities-and-properties)
panels: every edict, and the selected one's fields, which you can change before you resume.
The console's box runs any console command in the game.
