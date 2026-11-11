# The view differential

Prefix: `VIEWDIFF` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes how the preview host's frames and sound are held to id's engine (ENG-15, P3-28):
the view oracle, `oracles/engine/view_host.c`, runs id's whole engine headless, the software
renderer, the client and the mixer of `WinQuake/` at
id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`, and `preview-host
--view-script` runs ours on the same script; `cargo xtask view-diff` compares what they write.
Implemented by `oracles/engine/view_host.c`, `apps/quakec-engine` and `xtask/src/view_diff.rs`.

The two renderers are not the same program: id's draws 8-bit colour through its colour map,
ours draws QuakeSpasm's look on the GPU (REND-13). The comparison asks whether a frame shows
the same place, lit the same way, and whether the sound is the same sound at the same level,
within tolerances recorded per map; it is not an exact match.

## VIEWDIFF-1 The script

**Source:** ours: `oracles/engine/view_host.c`, `main`; `apps/quakec-engine/src/view_script.rs`.

### Behaviour

- **VIEWDIFF-1.1** A script is lines of text; blank lines and lines starting with `#` are
  skipped, and the first other line is `map <name>`, a name with no space, which both engines
  load: the oracle through id's `map` command, on a line of its own (`Cmd_Exec_f` inserts a
  file as it is, and the rerelease's `quake.rc` ends without a newline, so a command after it
  would run on into its last `alias`), the host as its map.
- **VIEWDIFF-1.2** Each line after it is one of `frames <n>`, n frames of 1/72 of a second, n a
  number from 0; `place <x> <y> <z> <pitch> <yaw>`, five finite numbers, the player moved there
  looking that way with no velocity and `fixangle` set, as the host's `player/spawn_at` does;
  `shot <file>`, the view written to the file as a binary PPM (`P6`, 8 bits a channel); and
  `toggleconsole` and `togglemenu`, the console and the main menu opened or closed as those
  commands do (CON-4.8, MENU-1.1). Any other line refuses the script.
- **VIEWDIFF-1.3** The sound mixed while the script runs is written to the second file as
  16-bit little-endian samples, left and right interleaved, at 11025 Hz
  (`SND_ORACLE_SPEED`): the oracle's what a sound card would have played by each frame's
  clock, ours what our mixer painted.

## VIEWDIFF-2 Drawing the view

**Source:** ours: `oracles/engine/view_host.c`, `VID_Init`; `apps/quakec-engine/src/headless.rs`,
`view_script`.

### Behaviour

- **VIEWDIFF-2.1** Both draw the view at 320 by 240, square pixels at 4:3: the oracle's video
  driver sets `vid.aspect` to 1, and the host draws offscreen at that size, so the field of
  view is 90 across in both (REND-13.6 widens nothing at 4:3).
- **VIEWDIFF-2.2** Only the 3D view is compared: the oracle sets `viewsize 120`, `crosshair
  0`, `r_drawviewmodel 0` and `con_notifytime 0` at each `place`, after whatever the game's
  configuration set, and the host draws no status bar, centre print or weapon. The view's
  colour shift is drawn in both.
- **VIEWDIFF-2.3** id's code calls the C library's `rand` through the generator of DIFF-6.2
  (`oracles/engine/rand.c`), so the oracle's frames are the same on every OS. In id's engine
  the client and the server draw from the one `rand`, so every particle or sound the client
  starts moves the server's sequence; the oracle gives the client's files that call it
  (`cl_main.c`, `cl_parse.c`, `cl_tent.c`, `menu.c`, `r_part.c`, `snd_dma.c`) the same
  generator from its own state, so the server draws what the server oracle draws (DIFF-16), as
  qs-sim does.
- **VIEWDIFF-2.4** The oracle draws the view only from a `place` to the `shot` after it, keeping
  `block_drawing` set otherwise, as `vid_win.c` does while the game is switched away. id's C
  span drawer, the `nonintel` path of `d_scan.c` used without `id386`, reads outside a surface's
  texture on some of `end`'s frames before the player is placed (`D_DrawSpans8`, found with
  AddressSanitizer), undefined behaviour the shipped x86 builds, which drew spans in assembly,
  did not reach. `V_CalcRefdef` smooths the eye's height only in the frames it draws, at most 12
  units behind and catching up at 80 a second, so a route waits a quarter of a second after each
  `place` before its `shot` (VIEWDIFF-5.4).
- **VIEWDIFF-2.5** The oracle plays no demos at start: it sets `cls.demonum` to -1, so `quake.rc`'s
  `startdemos` plays none (`Host_Startdemos_f`: "`if (!sv.active && cls.demonum != -1 &&
  !cls.demoplayback)`"). A demo the
  engine cannot play, as the rerelease's are to id's, raises an error that drops the rest of
  that frame's commands.
- **VIEWDIFF-2.6** The view oracle is built on Linux and macOS only. On Windows id's files take
  their `_WIN32` paths, which include `winquake.h`: it includes `<ddraw.h>`, `<dsound.h>` and
  SciTech's `<mgraph.h>` and declares the DirectSound buffer `snd_dma.c` reads, drivers the
  Windows SDK and the pinned sources do not provide. `cargo xtask view-diff` refuses to run
  there.

## VIEWDIFF-3 Comparing frames

**Source:** ours: `xtask/src/view_diff.rs`, `compare_frames`.

### Behaviour

- **VIEWDIFF-3.1** Each frame's pixels become luma, `0.299 r + 0.587 g + 0.114 b`, averaged
  over 4 by 4 blocks: an 80 by 60 grid, so a texel's filtering or a polygon edge one pixel
  over does not count.
- **VIEWDIFF-3.2** The frame's difference is the mean of the blocks' absolute differences,
  from 0 to 255, and its brightness is the ratio of our mean luma to the oracle's, 1 when the
  oracle's is 0 and ours is too.
- **VIEWDIFF-3.3** A frame passes when its difference is at most the map's `frame` tolerance
  and its brightness is within the map's `brightness` tolerance of 1 (VIEWDIFF-5). Frames of
  different sizes, or a file that is not a binary PPM, fail.

## VIEWDIFF-4 Comparing sound

**Source:** ours: `xtask/src/view_diff.rs`, `compare_sound`.

### Behaviour

- **VIEWDIFF-4.1** Each recording becomes the root mean square of its samples, both channels,
  in windows of a quarter of a second (2756 frames), and the last 12 windows are compared: the
  route's last three seconds, the player standing still. What comes before, the map loading
  and the player being moved, is left out: the oracle's sound starts when the engine does,
  ours when the map loads, and id's engine plays a sound as it starts.
- **VIEWDIFF-4.2** When both are silent the sound passes; when one is, it fails. A recording
  shorter than the 12 windows fails.
- **VIEWDIFF-4.3** Otherwise the windows both have are compared, ours shifted against the
  oracle's by up to two windows either way: the sound's difference is the least, over the
  shifts, of the sum of the windows' absolute differences over the sum of the oracle's, and it
  passes when that is at most the map's `sound` tolerance (VIEWDIFF-5). The shift allows for
  the engines loading a map in different times and for our mixer painting a tenth of a second
  ahead (SND-5.1); a sound one starts at random and the other does not is not allowed for: id's
  client and server draw from one generator (VIEWDIFF-2.3), ours each from its own.

## VIEWDIFF-5 Tolerances

**Source:** ours: measured with `cargo xtask view-diff` on ref-mac; `xtask/src/view_diff.rs`
reads the table between the `view-diff` markers.

### Behaviour

- **VIEWDIFF-5.1** **Withdrawn** 2026-11-09: it gave each route two shots; P3-34 adds the
  main menu and the console. Replaced by VIEWDIFF-5.4.
- **VIEWDIFF-5.2** A map's tolerances are its row of the table, else the `*` row's. A row says
  why the map's differs from the `*` row.
- **VIEWDIFF-5.3** A map with a monster, an entity whose class starts `monster_`, within 1000
  units of the start has its sound not compared, its frames still are: 1000 is as far as an
  idle sound carries, `SND_NOMINAL_CLIP_DIST` over an attenuation of 1 (SND-3.2). The two
  servers start at different frames, id's running while its client signs on over the loopback
  and ours with the player in at once, so a monster's random choice to idle falls at another
  time in each (on e1m8, e2m4, e3m3, e3m4, e4m5, e4m7 and `end` the sounds that differed were
  logged from the host: monsters' `idle` sounds), and no shift lines them up.
- **VIEWDIFF-5.4** Each route is a map's start: three seconds of frames, so that the sounds
  each client hears before the player's view is its listener are over, then the player placed
  at the map's first `info_player_start` at its `angle`, a quarter of a second, a shot, the
  player turned the other way, a quarter of a second, a shot; placed at the start again, the
  main menu opened (`togglemenu`), a quarter of a second, a shot, the menu closed; placed again,
  the console opened (`toggleconsole`), half a second for it to come down (CON-3.1), a shot,
  the console closed; a second of frames so the menu's sounds are over; and three seconds of
  frames for the sound. Each shot follows a `place`, after which alone the oracle draws
  (VIEWDIFF-2.4), and the place leaves the console up in the oracle.
- **VIEWDIFF-5.5** The menu's and the console's shots are held to their own tolerances, not the
  map's: the host draws them in QuakeSpasm's look (REND-13.8), the view behind the menu faded by
  black at 0.5 and the console's background at `scr_conalpha` 0.5 with its text at the frame's
  size (CON-5), where id's software engine fades the view darker and draws the console opaque.
  The menu's shot fits within a difference of 32 and a brightness within 0.6 of 1, the console's
  within 22 and 0.3. On LibreQuake's maps, Linux and macOS alike, the menu measured differences
  of 20 to 28.5 at brightnesses of 1.18 to 1.56, and the console 7.8 to 19.3 at 0.75 to 1.05
  (Differential run 42690189343). They catch a menu or console not drawn, not a small fault in
  how either is drawn, which the menu's and the console's own tests pin (MENU, CON).

<!-- view-diff -->
| Map | Frame | Brightness | Sound | Why |
| --- | --- | --- | --- | --- |
| `*` | 6 | 0.15 | 0.35 | |
| `lq_e0m1` | 25 | 0.3 | 0.35 | LibreQuake: the sky fills both shots, id's warped and point-sampled, ours QuakeSpasm's filtered sky (18.5 and 1.20 measured) |
| `lq_e0m2` | 8 | 0.25 | 0.35 | LibreQuake: the sky fills the ahead shot (6.0 and 1.19 measured) |
| `lq_e0m3` | 13 | 0.3 | 0.35 | LibreQuake: the sky fills the ahead shot (9.9 and 1.22 measured) |
| `start` | 6 | 0.2 | 0.35 | the rerelease's start shows its skybox through the ahead shot's ceiling, which id's engine does not read (REND-8; 1.16 measured; id1's start 1.02 to 1.04) |
| `e3m1` | 6 | 0.85 | 0.35 | a flickering light fills the ahead shot, its style at another phase (1.79 measured) |
| `e4m4` | 6 | 0.15 | 0.5 | an ambient sound near the start, 0.37 to 0.41 measured |
| `e4m7` | 14 | 0.15 | 0.35 | the behind shot looks at a scrolling texture at another position (11.4 measured) |
| `end` | 8 | 0.15 | 0.35 | the water's surface moves (6.8 measured) |
<!-- /view-diff -->
