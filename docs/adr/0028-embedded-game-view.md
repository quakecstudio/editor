# ADR-0028: Show the game inside the IDE by running the preview host's game in the IDE's process

- **Status:** Accepted
- **Date:** 2026-11-16
- **Decided by:** Justin Plock, 2026-11-16
- **Issue:** [#190](https://github.com/quakecstudio/editor/issues/190)
- **Requirements:** ENG-10, ENG-9

## Question

How does the IDE show the running game inside its own window (ENG-10): by running the game in
its process, by streaming frames from the preview host's process, or by sharing a GPU texture
across processes?

## Context

ENG-10 is "Embedded game view inside the IDE (later)", and #190 asks for "Game frames shown
inside the IDE via shared texture or frame streaming", "within one frame of latency of the docked
window". v1 docks the host's own window beside the IDE (ENG-9, #99, closed), and the design
document says "Embedding the frame inside the IDE (shared texture or frame streaming) is a later
option, not a v1 requirement".

The preview host is our own Rust engine ([ADR-0004](0004-preview-host-base.md)) and runs "as a
separate program beside the IDE and talks to it over a local socket" (design document, Phase
3). ADR-0004 already names the road this ADR has to choose on: the Rust host "could share the
Phase 5 renderer (`qs-render`) and run in the IDE's process, which is the natural road to the
post-MVP embedded game view", and "For that host the bridge would be a Rust API rather than a
socket protocol". [ADR-0003](0003-ui-framework.md) chose egui partly because "P5-05's renderer
draws inside the UI through `egui_wgpu` callbacks with no copy, and so could an in-process
embedded game view after the MVP".

What exists on 2026-11-16, read from the code:

- The host is an `eframe` app (`apps/quakec-engine/src/window.rs`, `Viewer`) whose game loop is
  `Game` in `apps/quakec-engine/src/game.rs`, over `qs-sim`, `qs-client`, `qs-sound` and
  `qs-render`; the host's sources are about 5,400 lines.
- `qs_render::Play::frame` returns a `MapCallback`, an `egui_wgpu` paint callback, and the IDE's
  map editor already draws `qs-render`'s output inside its window the same way
  (`apps/desktop/src/map_view.rs`, `egui_wgpu::Callback::new_paint_callback`).
- The bridge's `screenshot` returns "the frame the host last drew ... as a PNG in base64"
  (`docs/spec/bridge-protocol.md`), the only way frames cross the process boundary today.
- Shipped code may not use `unsafe` except one sanctioned module (CLAUDE.md; Justin's decision of
  2026-11-14 for macOS file opening), and the design's "Never crash" principle holds every
  shipped crate to no panics.

## Options

| Option | Consequences |
| --- | --- |
| A. In-process: the IDE runs the game itself (recommended) | The host's `Game` loop moves from `apps/quakec-engine` into a library crate, in L7 since `game.rs` uses `qs-render` (L7) besides `qs-sim` and `qs-bridge`, which both the host app and the desktop app use; the embedded view draws `Play::frame`'s callback in an egui panel as the map view does, so a frame costs no copy and no latency beyond the IDE's own. The bridge's events become calls in the IDE for the embedded game; the socket stays for the docked window and the CLI. Costs: the game shares the IDE's process, so a defect in `qs-sim` or the VM that the no-panic rules miss, or a hang, reaches the IDE (QuakeC runaway is already bounded, VM-3.1); the IDE opens a sound output; and two ways to run the game are tested. Plan: a library-extraction issue, then the view issue, each with the latency benchmark #190 names. |
| B. Frame streaming from the host's process | The host stays as it is; each frame it draws is read back from the GPU and sent to the IDE (over the local socket, or shared memory), which uploads it to a texture. Crash isolation is kept. Costs: a GPU read-back and an upload per frame, at least one frame behind the docked window, and for 1920 by 1080 RGBA about 8.3 MB a frame, 500 MB a second at 60 Hz (arithmetic, not measured). Shared memory would need `unsafe` mapping in our code, which the rules forbid; a socket alone carries the copy twice. #190's "within one frame of latency" is at risk. |
| C. Shared GPU texture across processes | The host renders into a texture the IDE draws directly: IOSurface on macOS, a DXGI shared handle on Windows, dma-buf on Linux. No copy and crash isolation. Costs: the project uses wgpu through its safe API; a texture shared with another process would go through each platform's API and `wgpu-hal`'s interop, which is `unsafe` (not checked against wgpu's documentation for this ADR), so three new `unsafe` modules against the rules, and synchronisation between two GPU devices. The largest and riskiest option. |
| D. Keep only the docked window | No work; ENG-10 stays unmet, and the post-MVP backlog keeps #190 open or drops it. |

## Decision

A, as Justin chose on 2026-11-16. It is the road ADR-0003 and ADR-0004 chose the renderer and the host's language
for: the renderer already draws inside egui through paint callbacks, and the IDE already does so
for the map editor, so the embedded view is the same frame with no copy and no added latency,
which is the only option that meets #190's "within one frame of latency" with certainty. B pays a
read-back and a copy each frame and starts a frame behind; C buys the same result as A with three
platform `unsafe` modules the project's rules forbid. A gives up the process boundary for the
embedded game; the no-panic rules, the VM's runaway limit and the command guard (design document,
"Never crash") are what keep a game fault from becoming an IDE crash, and the docked window stays
available, out of process, for a mod a modder does not trust.

## Consequences


- `plan/`: two post-MVP issues in place of #190's single deliverable: extract the host's game loop
  (`Game` and what it needs) from `apps/quakec-engine` into a library crate used by both apps,
  with the host's tests passing unchanged; then the embedded view in `apps/desktop`, a panel that
  runs that game and draws `Play::frame`'s callback, with #190's latency benchmark against the
  docked window.
- `docs/design/design-doc.md`: the Docking paragraph names in-process embedding as the chosen
  road, and the architecture says the embedded game runs in the IDE's process while the docked
  window and the CLI keep the socket.
- `layers.toml` and `docs/layers.md`: the new library crate, in L7.

## Links

- [#190](https://github.com/quakecstudio/editor/issues/190), the issue.
- [ADR-0003](0003-ui-framework.md), [ADR-0004](0004-preview-host-base.md),
  [ADR-0005](0005-default-preview-target.md).
- `docs/design/design-doc.md`, Docking and the Phase 3 summary of the preview host.
- `docs/spec/bridge-protocol.md`, the `screenshot` method.
- `apps/quakec-engine/src/window.rs` and `game.rs`; `apps/desktop/src/map_view.rs`;
  `crates/qs-render`, `Play::frame`.
