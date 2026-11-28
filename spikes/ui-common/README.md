# ui-common

The framework-free half of the UI framework spike
([#10](https://github.com/quakecstudio/editor/issues/10), P0-10). Both prototypes,
[`ui-gpui`](../ui-gpui/README.md) and [`ui-egui`](../ui-egui/README.md), build on this crate,
so the two differ only in the framework and their measurements can be compared.

- **Layer:** none. The spike is a workspace of its own ([`spikes/README.md`](../README.md)),
  outside `layers.toml`; nothing in the product depends on it.
- **Spec sections:** none; it implements no engine or format behaviour.
- **Shipped:** no. It is kept so the measurements behind
  [ADR-0003](../../docs/adr/0003-ui-framework.md) can be repeated.

## Modules

| Module | Holds |
| --- | --- |
| `document` | The editor model both prototypes render: lines, cursors, IME composition and an inline diff |
| `lexer` | A line-at-a-time QuakeC tokenizer for highlighting; not the compiler's lexer (`qc-syntax`) |
| `project` | Loads the benchmark project: every `.qc` file under a directory, in path order |
| `script` | The scripted input both prototypes replay, one action per frame |
| `harness` | The command-line arguments, the per-frame driver that replays the script, and the startup timings |
| `stats` | Frame samples, percentiles and the CSV both prototypes write |
| `theme` | The dark theme's colours from the approved mockups |
| `scene` | The 3D viewport's content: the mesh, shader and matrix of a spinning box standing in for a map preview |
| `a11y` | What the editor exposes to assistive technology: one AccessKit text run per visible row, and the caret |
