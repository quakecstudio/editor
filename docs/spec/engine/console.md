# The console

Prefix: `CON` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the client's console: its text, how printed text wraps into it, the notify lines,
the console drawn sliding down over the view, and the input line a player types commands
into, as id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d` implements them in
`WinQuake/console.c`, `WinQuake/keys.c` and, for the slide and the background, GLQuake's
`WinQuake/gl_screen.c` and `WinQuake/gl_draw.c`. Implemented by `qs-client` (`console.rs`)
and drawn by the preview host. The commands a line runs are qs-sim's console path (P3-08).

## CON-1 The text

**Source:** `WinQuake/console.c`, `Con_CheckResize`, `Con_Linefeed`, `Con_Print`, `Con_Clear_f`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

The console keeps the last text printed as fixed-width lines in one buffer of
`CON_TEXTSIZE` bytes, the newest line at `con_current`.

### Behaviour

- **CON-1.1** The line width is `(vid.width >> 3) - 2` characters, and the buffer holds
  `CON_TEXTSIZE / con_linewidth` lines (`con_totallines`), a ring indexed `line %
  con_totallines`. A width below 1 (no video yet) is 38. A new width keeps the last lines,
  each cut or padded with spaces to the new width, the newest at the bottom, clears the notify
  times (CON-2.1) and the scrollback; the buffer starts as spaces.
- **CON-1.2** `Con_Print` reads the text a byte at a time. A first byte of 1 or 2 is not
  printed and sets the high bit (128) of every character after it, which draws them in the
  other colour; 1 also plays `misc/talk.wav`.
- **CON-1.3** Before each character the word starting there is measured: up to `con_linewidth`
  characters before one at or below a space. A word that is not a full line long and does not
  fit in what is left of the line starts a new one. The measure is taken at every character, so
  a word longer than a line fills lines while a line or more of it is left, then wraps the rest
  when it no longer fits: 40 characters at 38 wide are 3 on one line and 37 on the next.
- **CON-1.4** At the start of a line (`con_x` 0) the next line is cleared to spaces and becomes
  `con_current`, and the time is recorded for it (CON-2.1). `\n` ends the line; `\r` ends it
  too and makes the next character's line the same one again (`con_current--`), so the next
  text overwrites it. Any other character is stored and the position advances, starting a new
  line at the width.
- **CON-1.5** `clear` fills the buffer with spaces. Printing resets the scrollback to the
  bottom.

## CON-2 The notify lines

**Source:** `WinQuake/console.c`, `Con_Print`, `Con_DrawNotify`, `Con_ClearNotify`;
`WinQuake/gl_screen.c`, `SCR_DrawConsole`.

### Behaviour

- **CON-2.1** `NUM_CON_TIMES` (4) times are kept, line `n`'s at `n % 4`, set to `realtime`
  when the line is started.
- **CON-2.2** With the console up and the key destination the game or a message, the notify
  lines are drawn: of the last four lines, from the oldest, each whose time is not 0 and not
  more than `con_notifytime` (3) seconds ago, its `con_linewidth` characters at x `(i+1)*8`,
  each line 8 below the last from the top.
- **CON-2.3** Toggling the console clears the times, so nothing printed before shows as a
  notify line after.

## CON-3 The console on the screen

**Source:** `WinQuake/gl_screen.c`, `SCR_SetUpToDrawConsole`, `SCR_DrawConsole`;
`WinQuake/console.c`, `Con_DrawConsole`, `Con_DrawInput`; `WinQuake/gl_draw.c`,
`Draw_ConsoleBackground`.

### Behaviour

- **CON-3.1** The console is forced up (`con_forcedup`) and fills the screen while there is no
  world model or the client has not finished signing on; else it is half the screen's height
  when the key destination is the console, and none otherwise. Its drawn height moves toward
  that by `scr_conspeed` (300) pixels a second of host frame time, stopping there; forced up,
  it is there at once.
- **CON-3.2** A console `lines` pixels high draws `gfx/conback.lmp` across the screen with its
  top at `lines - vid.height`; when `lines` is at most three quarters of the screen
  (`(vid.height * 3) >> 2`) it is drawn with alpha `1.2 * lines / that`.
- **CON-3.3** It shows `(lines - 16) >> 3` rows of text, the last ending 16 above the bottom,
  the first at `lines - 16 - rows * 8`, which may be above the screen: the rows up to
  `con_current`, moved up by the scrollback and never before line 0, each its characters at x
  `(i+1)*8`.
- **CON-3.4** The input line is drawn 16 above the console's bottom when the key destination is
  the console or the console is forced up: its text with the cursor at the edit position,
  character 10 or 11 as `(int)(realtime * con_cursorspeed) & 1` (`con_cursorspeed` 4), the rest
  of the line spaces; when the position is at or past the width, the line is drawn from `1 +
  key_linepos - con_linewidth`. Drawing it then ends the line at the position
  ("`key_lines[edit_line][key_linepos] = 0;`"), so what Backspace or the down arrow left after
  it is gone from the next frame on.

## CON-4 Typing

**Source:** `WinQuake/keys.c`, `Key_Console`, `Key_Init`; `WinQuake/console.c`,
`Con_ToggleConsole_f`.

### Behaviour

- **CON-4.1** There are 32 lines of history (`key_lines`), each `MAXCMDLINE` (256) bytes, each
  starting with the prompt `]`; the edit line is one of them and the position starts at 1.
- **CON-4.2** Enter runs the line without its prompt as a console command, prints it, prompt
  included, then moves the edit line to the next of the 32 (`& 31`), resets the history to it
  and starts it as `]` at position 1.
- **CON-4.3** Tab replaces the line with a command, else a variable, whose name starts with
  what is typed, then a space, the position after it; nothing typed completes nothing.
  `Cmd_CompleteCommand` and `Cvar_CompleteVariable` return the first match in their lists,
  which hold the most recently registered first; this console takes the first by name, our
  choice, since its commands are qs-sim's console path and not registered one by one.
- **CON-4.4** Backspace and the left arrow move the position back, not before 1; the text after
  it is not drawn, and is cut off when the line is next drawn (CON-3.4).
- **CON-4.5** The up arrow steps back through the history, skipping lines with nothing after
  the prompt, to the edit line at most, then the one after it; the down arrow steps forward
  the same way, and reaching the edit line sets its prompt and the position 1, its old text cut
  off when the line is next drawn (CON-3.4). The line found is copied into the edit line, the
  position at its end.
- **CON-4.6** Page up and the wheel up scroll back 2 lines, page down and the wheel down 2
  forward; the scrollback stays within 0 and `con_totallines - (vid.height >> 3) - 1`. Home
  goes to that limit, End to 0.
- **CON-4.7** Any other key from 32 to 127 is stored at the position, which advances, while
  the position is below `MAXCMDLINE - 1`; other keys do nothing.
- **CON-4.8** `toggleconsole` leaves the console for the game, clearing the line, when the
  client is connected; else it opens the main menu. From any other destination it opens the
  console. Either way the notify times are cleared. id1's `default.cfg` binds it to the
  backquote and `~` ("`bind \` "toggleconsole"`"), and the backquote is never typed into the
  line (`Key_Init`: "`consolekeys['\`'] = false;`"), so it reaches the binding.

## CON-5 QuakeSpasm's console

**Source:** `Quake/console.c`, `Con_DrawConsole` and `Con_DrawInput`; `Quake/gl_draw.c`,
`GL_SetCanvas` (`CANVAS_CONSOLE`) (sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`).

In QuakeSpasm's look (REND-13.7) the console is drawn as QuakeSpasm draws it.

### Behaviour

- **CON-5.1** The console is laid out at the screen's full height and moved up by what is not
  shown (`CANVAS_CONSOLE`: "`lines = vid.conheight - (scr_con_current * vid.conheight /
  glheight)`"): its background as CON-3.2 places it, `(lines + 7) / 8` rows counted up from the
  bottom, two of them left for the input line and the version.
- **CON-5.2** The text's rows end 16 above the bottom; while scrolled back the last two give
  way to a blank row and a row of `^` every fourth column.
- **CON-5.3** The input line is drawn 16 above the bottom from the prestep CON-3.4 gives, up to
  the end of its text, with a block cursor at the position when `(int)(realtime *
  con_cursorspeed) & 1` is 0; the line is not cut off by drawing it, unlike CON-3.4.
  QuakeSpasm resets the blink on each key and draws an insert or overwrite cursor picture; this
  console draws character 11, our choice.
- **CON-5.4** The version is drawn at the bottom right, its last character at column
  `con_linewidth + 1`: QuakeSpasm's "`QuakeSpasm 0.97.0`", this host's `QuakeC Studio` and its
  version.

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `CON_TEXTSIZE` | 16384 | `WinQuake/console.c`, `CON_TEXTSIZE` |
| `CON_NUM_TIMES` | 4 | `WinQuake/console.c`, `NUM_CON_TIMES` |
| `CON_NOTIFY_TIME` | 3 | `WinQuake/console.c`, `con_notifytime`'s default |
| `CON_CURSOR_SPEED` | 4 | `WinQuake/console.c`, `con_cursorspeed` |
| `CON_SPEED` | 300 | `WinQuake/gl_screen.c`, `scr_conspeed`'s default |
| `CON_MAX_LINE` | 256 | `WinQuake/keys.c`, `MAXCMDLINE` |
| `CON_HISTORY` | 32 | `WinQuake/keys.c`, `key_lines[32]` |

## CON-6 The client's commands

**Source:** `WinQuake/console.c`, `Con_Init`; `WinQuake/menu.c`, `M_Init`, `M_Menu_Main_f`,
`M_Menu_SinglePlayer_f`, `M_Menu_Help_f`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`); `Quake/cmd.c`, `Cmd_List_f`
and `Cmd_AddCommand` (sezero/quakespasm@`c9c8ee9895961669c4674e8f8ca8c64cc43e38b5`);
`QW/client/keys.c`, `Key_Console` (id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

### Behaviour

- **CON-6.1** **Withdrawn** 2026-11-06: the load, save and options menus' commands are
  known; replaced by CON-6.4.
- **CON-6.4** The client's console runs the client's commands as well as the server's:
  `toggleconsole` and `clear` (`Con_Init`), and `togglemenu`, `menu_main`, `menu_singleplayer`,
  `menu_load`, `menu_save`, `menu_options`, `help` and `menu_quit` (`M_Init`). id runs each where `Cmd_ExecuteString` meets it; here the
  server's console passes them on in order and the client runs them after the text it was
  given, our choice: none of them reads or changes what the server's commands do. `help` is "`Cmd_AddCommand ("help", M_Menu_Help_f);`": it opens the help
  menu on page 0, and prints nothing. id's `M_Init` also registers the multiplayer, setup,
  keys and video menus' commands; this host has none of those menus (MENU-2.4, MENU-7.2), so
  they are unknown commands.
- **CON-6.2** `cmdlist`, QuakeSpasm's `Cmd_List_f`, prints "`   <name>`" for each command whose
  name starts with its first argument (`Q_strncmp`, case-sensitive), every command with none,
  in `strcmp` order, as `Cmd_AddCommand` inserts "each entry in alphabetical order"; then
  "`<n> commands`", with "` beginning with "<partial>"`" after it when an argument was given.
  id's WinQuake has no command that lists the commands; QuakeSpasm's is taken, our choice.
- **CON-6.3** A line entered with `\` or `/` first runs without it. WinQuake's `Key_Console`
  runs the line as typed ("`Cbuf_AddText (key_lines[edit_line]+1);`"), and QuakeSpasm's too;
  QuakeWorld's runs it without: "`if (key_lines[edit_line][1] == '\\' ||
  key_lines[edit_line][1] == '/') Cbuf_AddText (key_lines[edit_line]+2);`". QuakeWorld sends
  any other line as chat; this console runs it as a command, as WinQuake's does. Our choice,
  asked for by Justin on 2026-11-06. The echoed line keeps what was typed.

## CON-7 The client's settings

**Source:** `WinQuake/cl_input.c`, `cl_main.c`, `gl_screen.c`, `view.c` and `snd_dma.c` (the
declarations); `WinQuake/cl_input.c`, `CL_BaseMove`; `WinQuake/in_win.c`, `IN_MouseMove`
(id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`).

### Behaviour

- **CON-7.1** The client's settings are in the one table with the server's (PHYS-12.1), so
  the console, the menus and the progs' `cvar` read and set them, with id's defaults:
  `cl_upspeed` "200", `cl_forwardspeed` "200", `cl_backspeed` "200", `cl_sidespeed` "350",
  `cl_movespeedkey` "2.0", `lookspring` "0", `lookstrafe` "0", `sensitivity` "3", `m_pitch`
  "0.022", `m_yaw` "0.022", `viewsize` "100", `gamma` "1", `bgmvolume` "1" and `volume` "0.7".
- **CON-7.2** The preview host reads them each frame: the move's speeds and the run key's
  factor as `CL_BaseMove` does, the mouse turned by `sensitivity` times `m_yaw` and `m_pitch`
  as `IN_MouseMove` does, `viewsize` and `gamma` for the view (VIEW-3.6, VIEW-4.7), and
  `volume` for the mixer (SND-5.4). `bgmvolume`, `lookspring` and `lookstrafe` are kept and
  shown but change nothing yet: this host plays no CD music and has no mouse-look key.
