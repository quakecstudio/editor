# The status bar and the screen's text

Prefix: `HUD` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes the status bar, the inventory bar above it, the frag list, the scoreboards and the
intermission and finale overlays, and the text printed in the centre of the screen, as
implemented by `WinQuake/sbar.c` and `screen.c` in
[id-Software/Quake](https://github.com/id-Software/Quake) at commit
`bf4ac424ce754894ac8f1dae6a3981954bc9852d`. To be implemented by the preview host's client
(P3-26, #240). The rerelease draws its text through the localization of RRE-1.

Positions are in the 320 by 200 coordinates of the pictures in `gfx.wad` (WAD-3): the bar
is drawn `HUD_SBAR_HEIGHT` pixels above the bottom of the screen, and on a screen wider than
320 pixels it is centred, `(width - 320) / 2` from the left, except in deathmatch, where
it stays at the left. Numbers are the `num_0` to `num_9` and `num_minus` pictures (24 pixels
wide), or the red `anum_` set when `color` is 1. `sb_lines` is how many rows of the bar the
view size leaves: 24 shows the bar alone and more shows the inventory too.

## HUD-1 The status bar

**Source:** `WinQuake/sbar.c`, `Sbar_Draw`, `Sbar_DrawInventory`, `Sbar_DrawFace`,
`Sbar_DrawNum`, `Sbar_itoa`, `Sbar_Init`.

### Behaviour

- **HUD-1.1** Nothing is drawn while the console fills the screen. With more than 24 lines
  the inventory (HUD-1.4) is drawn, and the frag list (HUD-1.5) when `maxclients` is not 1.
  With the scores shown (`+showscores`) or `STAT_HEALTH` at or below 0 the `scorebar` picture
  and the scoreboard (HUD-2) replace the bar; otherwise, with any lines, the `sbar` picture
  is drawn and over it: the armour, the face, the health, the ammo icon and the ammo count.
- **HUD-1.2** Armour: with `IT_INVULNERABILITY` the number 666 at (24, 0) in red and the
  `disc` picture at (0, 0); else `STAT_ARMOR` at (24, 0), red when 25 or less, and the
  `sb_armor3`, `sb_armor2` or `sb_armor1` picture at (0, 0) by the first of `IT_ARMOR3`,
  `IT_ARMOR2`, `IT_ARMOR1` set. Health: `STAT_HEALTH` at (136, 0), red when 25 or less. The
  ammo icon at (224, 0) is `sb_shells`, `sb_nails`, `sb_rocket` or `sb_cells` by the first of
  `IT_SHELLS`, `IT_NAILS`, `IT_ROCKETS`, `IT_CELLS` set; `STAT_AMMO` at (248, 0), red when 10
  or less. Rogue's items read its `RIT_` bits and its own pictures instead, and Hipnotic draws
  its two keys at (209, 3) and (209, 12).
- **HUD-1.3** The face at (112, 0): `face_inv2` with both `IT_INVISIBILITY` and
  `IT_INVULNERABILITY`, else `face_quad` with `IT_QUAD`, `face_invis` with
  `IT_INVISIBILITY`, `face_invul2` with `IT_INVULNERABILITY`; otherwise `face<n>` with
  `n = 5 - f` for `f = 4` at health 100 or more and `health / 20` below, and the pain face
  `face_p<n>` while `cl.time <= faceanimtime` (VIEW-4.1). Rogue's team modes (teamplay 4 to
  6) draw a team border and the frags there instead.
- **HUD-1.4** The inventory at 24 pixels above the bar: the `ibar` picture; for each weapon
  bit `IT_SHOTGUN << i` (`i` 0 to 6) that is set, the picture `inv_<weapon>`, `inv2_<weapon>`
  when it is the active weapon, or `inva<k>_<weapon>` with `k = flashon - 1` while
  `flashon = (int) ((cl.time - item_gettime[i]) * 10)` is below 10, where the drawn frame is
  `(flashon % 5) + 2`, at (`i * 24`, -16); the four ammo counts `STAT_SHELLS` to
  `STAT_CELLS` as three right-aligned digits of the small yellow font (character `18 +
  digit`) at `((6 * i + d + 1) * 8 - 2, -24)`, spaces not drawn; the items `sb_key1`,
  `sb_key2`, `sb_invis`, `sb_invuln`, `sb_suit`, `sb_quad` for the bits 17 to 22 at
  (`192 + i * 16`, -16); and the sigils for bits 28 to 31 at (`288 + i * 8`, -16).
- **HUD-1.5** The frag list draws up to four clients sorted by frags, descending (a stable
  bubble sort, ties in slot order), each a top and bottom colour block (CLIENT-1.6's rows,
  `Sbar_ColorForMap` adding 8) 28 by 4 and 28 by 3 pixels at `(x * 8 + 10, height -
  HUD_SBAR_HEIGHT - 23)` and the frags as three characters at `((x + 1) * 8, -24)`, the local
  client bracketed by characters 16 and 17; `x` starts at 23 and grows by 4 per client.
- **HUD-1.6** `Sbar_DrawNum (x, y, num, digits, color)` draws `num` as decimal digits, a
  `-` as the minus picture, keeping the last `digits` digits of a longer number and
  right-aligning a shorter one by 24 pixels per missing digit.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `HUD_SBAR_HEIGHT` | 24 | `sbar.h`, `SBAR_HEIGHT` |
| `HUD_NUM_WIDTH` | 24 | `sbar.c`, `Sbar_DrawNum` |
| `HUD_LOW_ARMOR` | 25 | `sbar.c`, `Sbar_Draw` |
| `HUD_LOW_HEALTH` | 25 | `sbar.c`, `Sbar_Draw` |
| `HUD_LOW_AMMO` | 10 | `sbar.c`, `Sbar_Draw` |
| `HUD_FLASH_FRAMES` | 10 | `sbar.c`, `Sbar_DrawInventory` |

## HUD-2 Scoreboards and overlays

**Source:** `WinQuake/sbar.c`, `Sbar_SoloScoreboard`, `Sbar_DrawScoreboard`,
`Sbar_DeathmatchOverlay`, `Sbar_IntermissionOverlay`, `Sbar_FinaleOverlay`,
`Sbar_SortFrags`.

### Behaviour

- **HUD-2.1** The solo scoreboard prints `Monsters:%3i /%3i` (`STAT_MONSTERS`,
  `STAT_TOTALMONSTERS`) at (8, 4), `Secrets :%3i /%3i` at (8, 12), `Time :%3i:%i%i` of
  `cl.time` as minutes, tens and units at (184, 4), and the level name centred on 232 at
  (232 - 4 * length, 12). In deathmatch the deathmatch overlay follows.
- **HUD-2.2** The deathmatch overlay draws `gfx/ranking.lmp` centred at y 8 and every client
  with a name, sorted by frags: colour blocks 40 by 4 at `(80, 40 + 10 * i)` (plus the
  centring offset), the frags as three characters from x 88, character 12 before the local
  client, and the name from x 144.
- **HUD-2.3** The intermission overlay (not deathmatch) draws `gfx/complete.lmp` at (64, 24),
  `gfx/inter.lmp` at (0, 56), `completed_time` as minutes at (160, 64) with the colon and
  two second digits at 234, 246 and 266, the secrets found and total at (160, 104) and
  (240, 104) with the slash at 232, and the monsters at y 144 the same way; in deathmatch
  HUD-2.2 instead. The finale draws `gfx/finale.lmp` centred at y 16.

## HUD-3 Centre print

**Source:** `WinQuake/screen.c`, `SCR_CenterPrint`, `SCR_DrawCenterString`,
`SCR_CheckDrawCenterString`.

### Behaviour

- **HUD-3.1** `SCR_CenterPrint (str)` keeps `str` (at most 1023 bytes), sets
  `centertime_off = scr_centertime` and `centertime_start = cl.time`, and counts its lines
  (one more per `\n`).
- **HUD-3.2** Each frame `centertime_off` loses `frametime`; the text is drawn while it is
  above 0 or an intermission is on, and only while the game has the keys. Each line is at
  most 40 characters, centred (`x = (width - 8 * length) / 2`), 8 pixels per row from
  `y = 0.35 * height` for up to four lines and `y = 48` for more.
- **HUD-3.3** During an intermission the characters appear one at a time: at most
  `scr_printspeed * (cl.time - centertime_start)` of them are drawn.

### Constants

| Constant | Value | Source |
| --- | --- | --- |
| `HUD_CENTERTIME` | 2 | `screen.c`, `scr_centertime` default (seconds) |
| `HUD_PRINTSPEED` | 8 | `screen.c`, `scr_printspeed` default (characters per second) |
| `HUD_CENTER_LINE_LENGTH` | 40 | `screen.c`, `SCR_DrawCenterString` |
