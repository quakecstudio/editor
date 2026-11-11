# Input

Prefix: `INPUT` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes how a local player's keys and mouse become the command sent to the server, as
id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d` does it in `WinQuake/keys.c`,
`WinQuake/cl_input.c` and `WinQuake/in_win.c`. Implemented by `qs-sim` (`keys.rs`,
`input.rs`) and the preview host (`keys.rs`, `window.rs`).

## INPUT-1 Bindings

**Source:** `WinQuake/keys.c`, `keynames`, `Key_StringToKeynum`, `Key_KeynumToString`,
`Key_SetBinding`, `Key_Unbind_f`, `Key_Unbindall_f`, `Key_Bind_f`, `Key_Event`;
`WinQuake/keys.h`; `WinQuake/menu.c`, `M_FindKeysForCommand`.

### Behaviour

- **INPUT-1.1** A key is a number from 0 to 255: a printable character is its own (a letter
  in lower case), and `keys.h` numbers the rest (`K_TAB` 9, `K_ENTER` 13, `K_ESCAPE` 27,
  `K_SPACE` 32, `K_BACKSPACE` 127, the arrows 128 to 131, `K_ALT` 132, `K_CTRL` 133, `K_SHIFT`
  134, `K_F1` to `K_F12` 135 to 146, `K_INS` 147, `K_DEL` 148, `K_PGDN` 149, `K_PGUP` 150,
  `K_HOME` 151, `K_END` 152, `K_MOUSE1` to `K_MOUSE3` 200 to 202, `K_JOY1` to `K_JOY4` 203 to
  206, `K_AUX1` to `K_AUX32` 207 to 238, `K_MWHEELUP` 239, `K_MWHEELDOWN` 240, `K_PAUSE`
  255). `Key_StringToKeynum` reads a single character as its own number and a longer name from
  `keynames`, any case, `SEMICOLON` for `;` "because a raw semicolon seperates commands";
  `Key_KeynumToString` gives a printable character (33 to 126) as itself, else its name, else
  `<UNKNOWN KEYNUM>`.
- **INPUT-1.2** `bind <key>` prints "`"<key>" = "<binding>"`" or "`"<key>" is not bound`";
  `bind <key> <command>` binds it, a third word and more joined by spaces; any other count
  prints "`bind <key> [command] : attach a command to a key`"; an unknown key prints
  "`"<key>" isn't a valid key`". `unbind <key>` sets the binding to the empty string, else
  prints "`unbind <key> : remove commands from a key`" or the unknown key line. `unbindall`
  sets every bound key's binding to the empty string. An empty binding stays a binding.
- **INPUT-1.3** A key going down runs its binding: a `+` binding as "`<binding> <key>\n`", any
  other as "`<binding>\n`", added to the console's buffer. A key going up adds "`-<rest>
  <key>\n`" for a `+` binding, whatever has the keys, "to keep the character from continuing
  an action started before a console switch", and nothing for any other. A key numbered 200
  or more with no binding at all prints "`<name> is unbound, hit F4 to set.`" going down.
- **INPUT-1.4** `M_FindKeysForCommand` gives the first two keys, in number order, whose
  binding starts with the command (`strncmp (b, command, l)` for the command's length).
- **INPUT-1.5** `M_UnbindCommand` sets every key whose binding starts with the command, as
  INPUT-1.4 compares them, to the empty binding.

## INPUT-2 Buttons and the move

**Source:** `WinQuake/cl_input.c`, `KeyDown`, `KeyUp`, `IN_MLookUp`, `IN_Impulse`,
`CL_KeyState`, `CL_AdjustAngles`, `CL_BaseMove`, `CL_SendMove`, `CL_InitInput`;
`WinQuake/in_win.c`, `IN_MouseMove`.

### Behaviour

- **INPUT-2.1** `+<button>` and `-<button>` hold and release the buttons `moveup`, `movedown`,
  `left`, `right`, `forward`, `back`, `lookup`, `lookdown`, `strafe`, `moveleft`, `moveright`,
  `speed`, `attack`, `use`, `jump`, `klook` and `mlook`, with the key's number as their word,
  or none typed at the console (`-1` for a press). A press by a key already holding it does
  nothing; a third key prints "`Three keys down for a button!`"; the first press sets bits 0
  (down) and 1 (pressed). A release by a key not holding it does nothing; when no key holds it
  any more it clears bit 0 and sets bit 2 (released); a release with no word lets every key go
  and leaves only bit 2. `-mlook` that leaves it up with `lookspring` set starts the pitch
  drift. `impulse <n>` keeps `Q_atoi` of the word, as the byte the move sends.
- **INPUT-2.2** `CL_KeyState` is 0.5 for a button pressed this frame and held, 1 for held
  throughout, 0.75 for released and pressed again, 0.25 for pressed and released, else 0, and
  clears bits 1 and 2.
- **INPUT-2.3** `CL_AdjustAngles` turns by `frametime`, times `cl_anglespeedkey` with `speed`
  held: unless `strafe` is held, `right` and `left` turn the yaw by `cl_yawspeed`, the yaw
  then `anglemod`; with `klook` held the pitch drift stops and `forward` and `back` pitch by
  `cl_pitchspeed`; `lookup` and `lookdown` pitch by `cl_pitchspeed`, stopping the drift when
  either is not 0. The pitch is kept within -70 and 80 and the roll within -50 and 50.
- **INPUT-2.4** `CL_BaseMove` makes the side move from `moveright` and `moveleft`, and from
  `right` and `left` with `strafe` held, by `cl_sidespeed`; the up move from `moveup` and
  `movedown` by `cl_upspeed`; unless `klook` is held, the forward move from `forward` by
  `cl_forwardspeed` and `back` by `cl_backspeed`; all three times `cl_movespeedkey` with
  `speed` held. `IN_MouseMove` then multiplies the motion by `sensitivity`: with `strafe`
  held, or `lookstrafe` set and `mlook` held, `m_side` times x is added to the side move,
  else the yaw turns by `-m_yaw` times x; with `mlook` held the pitch drift stops; with `mlook`
  held and `strafe` not, the pitch turns by `m_pitch` times y within -70 and 80, else
  `m_forward` times y is taken from the forward move. Mouse look is `+mlook`, as id1's
  `default.cfg` binds `\` to it and a config can run it.
- **INPUT-2.5** `CL_SendMove` sends attack as bit 0 and jump as bit 1 when the button's
  state has bit 0 or 1 ("`if ( in_attack.state & 3 )`"), then clears bit 1, so a press too
  short for a frame still fires once; it sends the impulse and clears it.

## INPUT-3 The preview host's keys

**Source:** `WinQuake/keys.c`, `Key_Init`, `Key_Event`; `WinQuake/host.c`, `Host_Init`;
`WinQuake/in_win.c`, `IN_MouseEvent`.

### Behaviour

- **INPUT-3.1** The window's keys become key numbers: letters in lower case, digits and the
  unshifted punctuation keys as the characters they type, the named keys by `keys.h`; Shift,
  Ctrl (and Command on macOS) and Alt from the modifier state as it changes; the mouse's three
  buttons and the wheel only while the window holds the mouse, a wheel step a key going down
  and up at once as QuakeSpasm sends it. A key's repeats are dropped ("ignore most
  autorepeats"); Backspace's reach the console's line as text keys do.
- **INPUT-3.2** A key going down runs its binding in the game; in the console only when it is
  not one of `consolekeys` (every printable character but `` ` `` and `~`, and Enter, Tab, the
  arrows, Backspace, the page keys, Shift and the wheel); in a menu only when it is
  `menubound` (the F keys). Escape never runs a binding (`Key_Event` handles it first), and the
  backquote is left to the window, which toggles the console with it (CON-4.8, CON-4.9).
- **INPUT-3.3** **Withdrawn** 2026-11-06: the game's `config.cfg` is no longer read; replaced
  by INPUT-3.6.
- **INPUT-3.6** The host runs `exec quake.rc` once, before its first frame, as `Host_Init`
  does ("`Cbuf_InsertText ("exec quake.rc\n");`"). id1's `quake.rc` runs `default.cfg`,
  `config.cfg` and `autoexec.cfg`; `config.cfg` is the host's own (INPUT-4.3), not the game
  directory's, which another engine wrote for itself.
- **INPUT-3.4** QuakeSpasm's `IN_MouseMove` drops the motion while the keys are not the
  game's (`key_dest != key_game`); so does the host, which also frees the mouse then.
- **INPUT-3.5** In QuakeSpasm's look `exec default.cfg` reads the host's own default instead of
  the game's, as QuakeSpasm's `quakespasm.pak`, ahead of id1 on the search path, covers id1's
  with its own (`Misc/qs_pak/default.cfg`, the source of `Quake/default_cfg.h`): its bindings
  put the movement on W, A, S and D, as QuakeSpasm's do, and it holds `+mlook`. The text is
  the host's, written for it (`DEFAULT_CFG` in `apps/quakec-engine/src/keys.rs`). In GLQuake's look the
  game's `default.cfg` is read.

## INPUT-4 The host's configuration

**Source:** `WinQuake/host.c`, `Host_WriteConfiguration`; `WinQuake/keys.c`,
`Key_WriteBindings`; `WinQuake/cvar.c`, `Cvar_WriteVariables`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`); `Quake/host.c`,
`Host_WriteConfiguration` (sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`).

### Behaviour

- **INPUT-4.1** On exit the host writes its configuration as `Host_WriteConfiguration` writes
  `config.cfg`: "`unbindall`", then `Key_WriteBindings`' "`bind "<key>" "<binding>"`" for every
  key whose binding is not empty, in number order, then `Cvar_WriteVariables`' "`<name>
  "<value>"`" for every archived setting (`_cl_name`, `_cl_color`, `lookspring`, `lookstrafe`,
  `sensitivity`, `m_pitch`, `m_yaw`, `m_forward`, `m_side`, `cl_forwardspeed`,
  `cl_backspeed`, `gamma`, `viewsize`, `bgmvolume`, `volume`), then "`+mlook`" when mouse look
  is held, as QuakeSpasm adds it ("`if (in_mlook.state & 1) fprintf (f, "+mlook\n");`").
- **INPUT-4.2** **Withdrawn** 2026-11-06: the host's file is now `config.cfg` itself; replaced
  by INPUT-4.3.
- **INPUT-4.3** The host keeps its configuration in `preview-host.cfg` in its config
  directory (`qs-paths`), written there on exit and never to the game's directory. To the
  host's console that file is `config.cfg`: `exec config.cfg`, as `quake.rc` runs it, reads
  it, or nothing before the host has written one. The game directory's `config.cfg` is the
  saved state of whichever engine last played there, with that engine's bindings, and is not
  read.
