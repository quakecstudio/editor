# qs-game

The preview host's game as a library (ADR-0004, ADR-0028): the game loop over `qs-sim`,
`qs-client`, `qs-render` and `qs-sound`, the loading of a map from an install, the window, the
headless runs and the bridge server. The `quakec-engine` binary (`apps/quakec-engine`) runs it
in a process of its own; the IDE runs the same game in its own window for the embedded game
view.

- **Layer:** L7 UI. It draws with `qs-render` and `eframe` (L7) over `qs-sim`, `qs-client`,
  `qs-sound` and `qs-bridge` (L4), `qc-vm`, `qc-edict` and `qc-builtins` (L1) and the formats
  (L0).
- **Spec sections:** the host's side of BRIDGE-1 to BRIDGE-7 of
  [`bridge-protocol.md`](../../docs/spec/bridge-protocol.md); WORLD-9 and WORLD-10's debugger
  and reload as the host drives them; INPUT-4's host files; VIEWDIFF-1 and VIEWDIFF-2's view
  scripts; the console's demo commands of CLIENT-10 (`record`, `stop`, `playdemo`,
  `timedemo`, `startdemos`, `demos`, `stopdemo`), ported from QuakeSpasm's `cl_demo.c` and
  `host_cmd.c`. Its tests cite the statements they pin.
- **Entry points:** `Args`, the host's command line, whose `run` does what it says (the
  window, `--headless`, `--timerefresh` or a view script); `GameView`, the same game drawn in a
  panel of another app's window, which the app draws each frame and lets answer its bridge
  between frames; `Error`, why a stage failed.

**The embedded view against the docked window (ENG-10, #1001).** On an Apple M-series Mac,
2026-11-16, LibreQuake's `lq_e0m1`: the IDE's whole window with the game in its Game tab, each
frame drawn on the device and read back, took 3.68 ms a frame on average and 8.02 ms at the
longest over 300 frames (`cargo run --release -p quakec-studio --example embedded_game`); the
docked window's own work was 0.02 ms a frame on average and 1.27 ms at the longest
(`quakec-engine --benchmark 5`, which does not count the GPU). Both run the same frame code
(`Viewer::frame`), which draws the server frame it just ran, and both are well inside one
72 Hz frame of 13.9 ms, so the embedded view is never a frame behind the docked window.

What the host does, its options and the bridge's commands are described in
[`apps/quakec-engine/README.md`](../../apps/quakec-engine/README.md).
