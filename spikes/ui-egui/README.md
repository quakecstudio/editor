# ui-egui

The [egui](https://github.com/emilk/egui) prototype of the UI framework spike
([#10](https://github.com/quakecstudio/editor/issues/10), P0-10): the editor of
[`ui-common`](../ui-common/README.md) drawn with egui, to be measured against the same editor
on gpui ([`ui-gpui`](../ui-gpui/README.md)) with the same scripted input.

It is a binary with a custom editor widget (egui's `TextEdit` lays out the whole buffer and
has one cursor), a `show_rows` project panel and a wgpu 3D viewport drawn through an
`egui_wgpu` paint callback. `--bench OUT.csv` replays the shared script and records each
frame's CPU time as eframe reports it.

- **Layer:** none. The spike is a workspace of its own ([`spikes/README.md`](../README.md)),
  outside `layers.toml`; nothing in the product depends on it.
- **Spec sections:** none.
- **Shipped:** no. The results are in
  [`docs/spikes/ui-framework.md`](../../docs/spikes/ui-framework.md) and the decision is
  [ADR-0003](../../docs/adr/0003-ui-framework.md).

How to run and measure it is in [`spikes/README.md`](../README.md#running-it).
