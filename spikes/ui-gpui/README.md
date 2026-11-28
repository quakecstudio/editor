# ui-gpui

The [gpui](https://gpui.rs) prototype of the UI framework spike
([#10](https://github.com/quakecstudio/editor/issues/10), P0-10): the editor of
[`ui-common`](../ui-common/README.md) drawn with gpui, to be measured against the same editor
on egui ([`ui-egui`](../ui-egui/README.md)) with the same scripted input.

It is a binary with a custom editor element (gpui ships no editor), a `uniform_list` project
panel and a wgpu 3D viewport, each its own view. The viewport renders offscreen and is read
back into a gpui image, because gpui exposes none of its GPU devices; that cost is part of
what the spike measures. `--bench OUT.csv` replays the shared script and records each frame's
CPU time.

- **Layer:** none. The spike is a workspace of its own ([`spikes/README.md`](../README.md)),
  outside `layers.toml`; nothing in the product depends on it.
- **Spec sections:** none.
- **Shipped:** no. The results are in
  [`docs/spikes/ui-framework.md`](../../docs/spikes/ui-framework.md) and the decision is
  [ADR-0003](../../docs/adr/0003-ui-framework.md).

How to run and measure it is in [`spikes/README.md`](../README.md#running-it).
