# The view differential

Prefix: `VIEWDIFF` (registered in [`../README.md`](../README.md)). IDs follow the rules there:
never renumbered, never reused, withdrawn rather than deleted.

Describes how the preview host's frames and sound are held to id's engine (ENG-15, P3-28):
the view oracle, `oracles/engine/view_host.c`, runs id's whole engine headless, the software
renderer, the client and the mixer of `WinQuake/` at
id-Software/Quake@`bf4ac424ce754894ac8f1dae6a3981954bc9852d`, and `preview-host
--view-script` runs ours on the same script; `cargo xtask view-diff` compares what they write.
Implemented by `oracles/engine/view_host.c`, `apps/preview-host` and `xtask/src/view_diff.rs`.

The two renderers are not the same program: id's draws 8-bit colour through its colour map,
ours draws QuakeSpasm's look on the GPU (REND-13). The comparison asks whether a frame shows
the same place, lit the same way, and whether the sound is the same sound at the same level,
within tolerances recorded per map; it is not an exact match.

## VIEWDIFF-1 The script

**Source:** ours: `oracles/engine/view_host.c`, `main`; `apps/preview-host/src/view_script.rs`.

### Behaviour

- **VIEWDIFF-1.1** A script is lines of text; blank lines and lines starting with `#` are
  skipped, and the first other line is `map <name>`, a name with no space, which both engines
  load: the oracle through id's `map` command, the host as its map.
- **VIEWDIFF-1.2** Each line after it is one of `frames <n>`, n frames of 1/72 of a second, n a
  number from 0; `place <x> <y> <z> <pitch> <yaw>`, five finite numbers, the player moved there
  looking that way with no velocity and `fixangle` set, as the host's `player/spawn_at` does;
  and `shot <file>`, the view written to the file as a binary PPM (`P6`, 8 bits a channel). Any
  other line refuses the script.
- **VIEWDIFF-1.3** The sound mixed while the script runs is written to the second file as
  16-bit little-endian samples, left and right interleaved, at 11025 Hz
  (`SND_ORACLE_SPEED`): the oracle's what a sound card would have played by each frame's
  clock, ours what our mixer painted.

## VIEWDIFF-2 Drawing the view

**Source:** ours: `oracles/engine/view_host.c`, `VID_Init`; `apps/preview-host/src/headless.rs`,
`view_script`.

### Behaviour

- **VIEWDIFF-2.1** Both draw the view at 320 by 240, square pixels at 4:3: the oracle's video
  driver sets `vid.aspect` to 1, and the host draws offscreen at that size, so the field of
  view is 90 across in both (REND-13.6 widens nothing at 4:3).
- **VIEWDIFF-2.2** Only the 3D view is compared: the oracle sets `viewsize 120`, `crosshair
  0`, `r_drawviewmodel 0` and `con_notifytime 0` before the map loads, and the host draws no
  status bar, centre print or weapon. The view's colour shift is drawn in both.
- **VIEWDIFF-2.3** id's code calls the C library's `rand` through the shared generator of
  DIFF-6.2 (`oracles/engine/rand.c`), so the oracle's frames are the same on every OS.

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
  in windows of a quarter of a second (2756 frames), leading silent windows dropped: the
  oracle's sound starts when the engine does and ours when the map loads.
- **VIEWDIFF-4.2** When both are silent the sound passes; when one is, it fails.
- **VIEWDIFF-4.3** Otherwise the windows both have are compared: the sound's difference is
  the mean of the windows' absolute differences over the mean of the oracle's, and it passes
  when that is at most the map's `sound` tolerance (VIEWDIFF-5).

## VIEWDIFF-5 Tolerances

**Source:** ours: measured with `cargo xtask view-diff` on ref-mac; `xtask/src/view_diff.rs`
reads the table between the `view-diff` markers.

### Behaviour

- **VIEWDIFF-5.1** Each route is a map's start: the player placed at its first
  `info_player_start`, at its `angle`, then looking the other way, a shot after 72 frames
  and after each turn, then three seconds of frames for the sound.
- **VIEWDIFF-5.2** A map's tolerances are its row of the table, else the `*` row's.

<!-- view-diff -->
| Map | Frame | Brightness | Sound |
| --- | --- | --- | --- |
| `*` | 6 | 0.15 | 0.35 |
<!-- /view-diff -->
