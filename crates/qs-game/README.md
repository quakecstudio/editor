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
  scripts. Its tests cite the statements they pin.
- **Entry points:** [`Args`], the host's command line, whose `run` does what it says (the
  window, `--headless`, `--timerefresh` or a view script); [`Error`], why a stage failed.

What the host does, its options and the bridge's commands are described in
[`apps/quakec-engine/README.md`](../../apps/quakec-engine/README.md).
