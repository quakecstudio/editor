# The menus

Prefix: `MENU` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the menus a local player uses, as id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`
implements them in `WinQuake/menu.c`, drawn over the view as GLQuake's `WinQuake/gl_draw.c`
fades it, with `WinQuake/gl_screen.c`'s `SCR_ModalMessage`. Implemented by `qs-client`
(`menu.rs`) and driven by the preview host. The multiplayer and network menus and the video
mode menu are not a local preview host's and are not described (P3-34).

## MENU-1 Opening, keys and drawing

**Source:** `WinQuake/menu.c`, `M_ToggleMenu_f`, `M_Menu_Main_f`, `M_Keydown`, `M_Draw`,
`M_DrawCharacter`, `M_DrawPic`, `M_DrawTransPic`, `M_Print`, `M_PrintWhite`;
`WinQuake/keys.c`, `Key_Event`; `WinQuake/gl_draw.c`, `Draw_FadeScreen`.

### Behaviour

- **MENU-1.1** Escape in the game opens the main menu (`M_ToggleMenu_f`); in the console it
  toggles the console (CON-4.8); in a menu it is that menu's key. `togglemenu` in a menu other
  than the main one opens the main one, and in the main one closes it.
- **MENU-1.2** In a menu every key is the menu's (`M_Keydown`), sent to the menu shown.
- **MENU-1.3** A menu is drawn only while it is shown and keys go to it: first the view faded,
  black at alpha 0.8 over the screen (`Draw_FadeScreen`: "`glColor4f (0, 0, 0, 0.8)`"), or with
  the console down the console's background over the whole screen; then the menu, its pictures
  and characters placed in a 320 wide area centred on the screen (`x + ((vid.width -
  320)>>1)`). Opening or entering a menu plays `misc/menu2.wav` after the next draw; moving its
  cursor plays `misc/menu1.wav`.
- **MENU-1.4** `M_Print` draws each character `+128`, the red half of the font; `M_PrintWhite`
  as it is.

## MENU-2 The main menu

**Source:** `WinQuake/menu.c`, `M_Main_Draw`, `M_Main_Key`.

### Behaviour

- **MENU-2.1** It draws `gfx/qplaque.lmp` at (16, 4), `gfx/ttl_main.lmp` centred at y 4,
  `gfx/mainmenu.lmp` at (72, 32) and the cursor `gfx/menudot<f>.lmp` at (54, 32 + cursor *
  20), `f` `(int)(host_time * 10) % 6 + 1`. There are 5 items (`MAIN_ITEMS`).
- **MENU-2.2** Escape closes the menus for the game. The arrows move the cursor down and up,
  wrapping round. Enter opens the menu of the item: single player, multiplayer, options, help
  or quit.
- **MENU-2.3** **Withdrawn** 2026-11-06: the options menu is built (MENU-7); replaced by
  MENU-2.4.
- **MENU-2.4** This host has no multiplayer menu: Enter on that item stays in the main menu,
  our choice. Enter on the options item opens the options (MENU-7.1).

## MENU-3 The single player menu

**Source:** `WinQuake/menu.c`, `M_Menu_SinglePlayer_f`, `M_SinglePlayer_Draw`,
`M_SinglePlayer_Key`.

### Behaviour

- **MENU-3.1** It draws as the main menu does with `gfx/ttl_sgl.lmp` and `gfx/sp_menu.lmp`, 3
  items (`SINGLEPLAYER_ITEMS`).
- **MENU-3.2** **Withdrawn** 2026-11-06: New Game now asks first; replaced by MENU-3.4.
- **MENU-3.3** **Withdrawn** 2026-11-06: the load and save menus are built (MENU-6);
  replaced by MENU-3.5.
- **MENU-3.4** **Withdrawn** 2026-11-06: it left out id's `disconnect`; replaced by MENU-3.6.
- **MENU-3.6** Escape returns to the main menu; the arrows move and wrap. Enter on New Game
  with a server running asks "Are you sure you want to\nstart a new game?\n"
  (`SCR_ModalMessage`): drawn over the faded view, each line of up to 40 characters centred,
  from y `200 * 0.35` 8 apart in the 320 wide menu, as QuakeSpasm's `SCR_DrawNotifyString`
  places it (id's from `vid.height*0.35` of the screen), in both looks, our choice. `y` closes
  the menus and runs `map start`; `n` or Escape go back to this menu; any other key waits.
  With no server running it does not ask. id first adds "`disconnect`" when a server is
  running and "`maxplayers 1`"; this host has neither command, being one single player game
  whose `map` replaces the level (PHYS-13.6), and runs `map start` alone, our choice.
- **MENU-3.5** Enter on Load opens the load menu (MENU-6.1); on Save, the save menu when the
  game may be saved (MENU-6.1), else nothing.

## MENU-4 Help

**Source:** `WinQuake/menu.c`, `M_Menu_Help_f`, `M_Help_Draw`, `M_Help_Key`.

### Behaviour

- **MENU-4.1** Help shows page 0 of `NUM_HELP_PAGES` (6) on opening: `gfx/help<n>.lmp` at
  (0, 0).
- **MENU-4.2** Escape returns to the main menu. The up and right arrows show the next page,
  the down and left arrows the previous one, wrapping round, each with `misc/menu2.wav`.

## MENU-5 Quit

**Source:** `WinQuake/menu.c`, `M_Menu_Quit_f`, `M_Quit_Draw`, `M_Quit_Key`, `quitMessage`.

### Behaviour

- **MENU-5.1** The prompt keeps the menu it was opened from, if any (`wasInMenus`,
  `m_quit_prevstate`), and draws over it; its message is `rand () & 7` of eight, each four lines
  drawn with `M_Print` at (64, 84) to (64, 108) in a text box of `gfx/box_*.lmp` at (56, 76),
  24 wide and 4 lines. Asked again while it shows, nothing changes. This is the prompt id's
  builds other than Windows draw; Windows' shows the credits instead.
- **MENU-5.2** Escape, `n` and `N` return to the menu it came from, else to the game; `y` and
  `Y` quit (`Host_Quit_f`); any other key does nothing.

## MENU-6 Load and save

**Source:** `WinQuake/menu.c`, `M_ScanSaves`, `M_Menu_Load_f`, `M_Menu_Save_f`,
`M_Load_Draw`, `M_Save_Draw`, `M_Load_Key`, `M_Save_Key`.

### Behaviour

- **MENU-6.1** Opening either menu scans the 12 slots (`MAX_SAVEGAMES`): `s<n>.sav` in the
  game directory, whose name is "`fscanf (f, "%i\n", &version); fscanf (f, "%79s\n",
  name);`", its first 39 characters (`SAVEGAME_COMMENT_LENGTH`) kept and each `_` made a
  space; a slot whose file does not open is "`--- UNUSED SLOT ---`" and cannot be loaded. The
  save menu opens only when a game runs, it is not at an intermission and it has one player.
  Each draws `gfx/p_load.lmp` or `gfx/p_save.lmp` centred at y 4, the slots with `M_Print` at
  (16, 32 + 8 * n), and the cursor, character `12 + ((int)(realtime*4)&1)`, at (8, 32 +
  cursor * 8). The two share one cursor.
- **MENU-6.2** Escape returns to the single player menu; up and left move to the slot before,
  down and right to the one after, wrapping, each with `misc/menu1.wav`.
- **MENU-6.3** Enter in the load menu plays `misc/menu2.wav`; on a slot that can be loaded it
  closes the menus and runs `load s<n>`, else nothing more. Enter in the save menu closes the
  menus and runs `save s<n>`.

## MENU-7 Options

**Source:** `WinQuake/menu.c`, `M_Menu_Options_f`, `M_AdjustSliders`, `M_DrawSlider`,
`M_DrawCheckbox`, `M_Options_Draw`, `M_Options_Key`.

### Behaviour

- **MENU-7.1** The options have `OPTIONS_ITEMS` 13 items, as id's builds other than Windows'
  have (Windows' adds Use Mouse): Customize controls, Go to console, Reset to defaults, the
  five sliders, the four checkboxes, and Video Options, which a build with no video menu
  (`vid_menudrawfn == NULL`) neither draws nor stops on. This host has no video menu.
- **MENU-7.2** **Withdrawn** 2026-11-06: Customize controls opens the keys menu now;
  replaced by MENU-7.5.
- **MENU-7.3** It draws the plaque at (16, 4), `gfx/p_option.lmp` centred at y 4, the twelve
  labels with `M_Print` at (16, 32 + 8 * n), the sliders and checkboxes at x 220, and the
  cursor, character `12 + ((int)(realtime*4)&1)`, at (200, 32 + cursor * 8). A slider is its
  range kept within 0 and 1, character 128 at x - 8, ten 129s from x, 130 after them, and 131
  at `x + (SLIDER_RANGE-1)*8 * range` (`SLIDER_RANGE` 10). The ranges: Screen size
  `(viewsize - 30) / (120 - 30)`, Brightness `(1.0 - gamma) / 0.5`, Mouse Speed
  `(sensitivity - 1)/10`, CD Music Volume `bgmvolume`, Sound Volume `volume`. A checkbox is
  "on" or "off" with `M_Print`: Always Run when `cl_forwardspeed` passes 200, Invert Mouse
  when `m_pitch` is below 0, Lookspring and Lookstrafe when set.
- **MENU-7.4** `M_AdjustSliders (dir)` plays `misc/menu3.wav`, then on Screen size adds `dir *
  10` to `viewsize` within 30 and 120; on Brightness takes `dir * 0.05` from `gamma` within
  0.5 and 1; on Mouse Speed adds `dir * 0.5` to `sensitivity` within 1 and 11; on CD Music
  Volume adds `dir * 0.1` to `bgmvolume` (a build other than Windows'), and on Sound Volume
  `dir * 0.1` to `volume`, each within 0 and 1; on Always Run sets `cl_forwardspeed` and
  `cl_backspeed` to 200 when `cl_forwardspeed` passes 200, else to 400; on Invert Mouse
  negates `m_pitch`; on Lookspring and Lookstrafe sets `!value`. The sum is C's: a `float`
  setting and a `double` step, stored back as a `float`. Each is set as `Cvar_SetValue` sets
  it, written with `%f`. On any other item only the sound plays.

- **MENU-7.5** Escape returns to the main menu. The up and down arrows move, wrapping, with
  `misc/menu1.wav`; landing on the video item goes to item 11 going up, else to item 0. Enter
  on Go to console closes the menus and opens the console; on Reset to defaults runs `exec
  default.cfg`; on Customize controls opens the keys menu (MENU-8); on any other item it is the right arrow. The
  left and right arrows change the item's setting (MENU-7.4).

## MENU-8 The keys menu

**Source:** `WinQuake/menu.c`, `bindnames`, `M_Menu_Keys_f`, `M_FindKeysForCommand`,
`M_UnbindCommand`, `M_Keys_Draw`, `M_Keys_Key`.

### Behaviour

- **MENU-8.1** It lists `bindnames`' eighteen commands in order: `+attack` "attack", `impulse
  10` "change weapon", `+jump` "jump / swim up", `+forward` "walk forward", `+back`
  "backpedal", `+left` "turn left", `+right` "turn right", `+speed` "run", `+moveleft` "step
  left", `+moveright` "step right", `+strafe` "sidestep", `+lookup` "look up", `+lookdown`
  "look down", `centerview` "center view", `+mlook` "mouse look", `+klook` "keyboard look",
  `+moveup` "swim up", `+movedown` "swim down". It draws `gfx/ttl_cstm.lmp` centred at y 4;
  "`Enter to change, backspace to clear`" at (18, 32), or while it waits for a key "`Press a
  key or button for this action`" at (12, 32); each label at (16, 48 + 8 * n), and at x 140
  the first key `M_FindKeysForCommand` finds by `Key_KeynumToString`, then "`or`" 8 past it
  and the second 32 past it, or "`???`" for none; and at (130, 48 + 8 * cursor) `=` while it
  waits, else character `12 + ((int)(realtime*4)&1)`.
- **MENU-8.2** Escape returns to the options (`M_Menu_Options_f`); left and up move to the
  command before, right and down to the one after, wrapping, with `misc/menu1.wav`; Enter
  plays `misc/menu2.wav`, unbinds the command (INPUT-1.5) when two keys hold it, and waits for
  a key; Backspace and Delete play `misc/menu2.wav` and unbind it.
- **MENU-8.3** While it waits, the next key going down is its, whatever it is, mouse buttons
  included: it plays `misc/menu1.wav` and stops waiting; Escape and the backquote bind
  nothing; any other key runs "`bind "<key name>" "<command>"`" (`Cbuf_InsertText`).

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MENU_HELP_PAGES` | 6 | `WinQuake/menu.c`, `NUM_HELP_PAGES` |
| `MENU_SAVEGAMES` | 12 | `WinQuake/menu.c`, `MAX_SAVEGAMES` |
