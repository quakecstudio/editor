# The menus

Prefix: `MENU` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the menus a local player uses, as id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`
implements them in `WinQuake/menu.c`, drawn over the view as GLQuake's `WinQuake/gl_draw.c`
fades it. Implemented by `qs-client` (`menu.rs`) and driven by the preview host. The
multiplayer and network menus and the video mode menu are not a local preview host's and are
not described (P3-34).

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
- **MENU-2.3** This menu has no multiplayer or options menu yet: Enter on those items stays in
  the main menu, our choice while P3-34 is open.

## MENU-3 The single player menu

**Source:** `WinQuake/menu.c`, `M_Menu_SinglePlayer_f`, `M_SinglePlayer_Draw`,
`M_SinglePlayer_Key`.

### Behaviour

- **MENU-3.1** It draws as the main menu does with `gfx/ttl_sgl.lmp` and `gfx/sp_menu.lmp`, 3
  items (`SINGLEPLAYER_ITEMS`).
- **MENU-3.2** Escape returns to the main menu; the arrows move and wrap. Enter on New Game
  closes the menus and starts a game: id's adds "`maxplayers 1`" and "`map start`" to the
  command buffer, first asking "Are you sure you want to\nstart a new game?" when a server is
  running (`SCR_ModalMessage`). This menu starts the start map at once without asking, our
  choice while P3-34 is open.
- **MENU-3.3** Load and save are not this menu's yet: Enter on them stays in the menu.

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

### Constants

| Name | Value | Source |
| --- | --- | --- |
| `MENU_HELP_PAGES` | 6 | `WinQuake/menu.c`, `NUM_HELP_PAGES` |
