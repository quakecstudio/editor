# UI framework spike

Prototypes for [#10](https://github.com/quakecstudio/editor/issues/10) (P0-10): the same editor
built on [gpui](https://gpui.rs) and on [egui](https://github.com/emilk/egui), measured with the
same scripted input. The results are in
[`docs/spikes/ui-framework.md`](../docs/spikes/ui-framework.md); the decision is ADR-0003,
proposed in [#250](https://github.com/quakecstudio/editor/pull/250).

**Not shipped.** Nothing here is a dependency of the product, and nothing in the main workspace
depends on it. It is kept so the measurements can be repeated on the other reference machines
and when the frameworks change.

| Crate | What it is |
| --- | --- |
| [`ui-common`](ui-common/src/lib.rs) | Framework-free parts both prototypes share: the document (lines, multi-cursor, IME composition, inline diff), a QuakeC highlighter with simulated semantic classes, the scripted input, the frame statistics, the dark theme from the mockups, the 3D scene and the AccessKit text runs |
| [`ui-gpui`](ui-gpui/src/main.rs) | gpui prototype: a custom editor element, `uniform_list` project panel, wgpu viewport read back into a gpui image |
| [`ui-egui`](ui-egui/src/main.rs) | egui prototype: a custom editor widget, `show_rows` project panel, wgpu viewport through an `egui_wgpu` paint callback |

## Why a separate workspace

`spikes/Cargo.toml` is a workspace of its own, so `cargo` at the repository root, `make ci` and
every CI job ignore it: gpui and egui (796 crates in `Cargo.lock` across all targets) are never
built on pull requests.
For the same reason its lint set is short (no `unsafe`, no `unwrap`, `expect` or `panic`) rather
than the main workspace's full no-panic set, and its dependencies are pinned here instead of in
the root `[workspace.dependencies]`. `cargo deny` was run against it by hand with the root
`deny.toml`; the results are in the spike report.

## Running it

Needs the toolchain from `rust-toolchain.toml`. On Linux, gpui needs the Wayland, X11,
xkbcommon and fontconfig development packages and a C compiler; on Windows, the Windows SDK
(gpui compiles its HLSL shaders at build time).

```bash
spikes/fetch-corpus.sh /tmp/qs-rerelease          # id's rerelease QuakeC, checked by SHA-256
cargo run --release --manifest-path spikes/Cargo.toml -p ui-gpui -- /tmp/qs-rerelease
cargo run --release --manifest-path spikes/Cargo.toml -p ui-egui -- /tmp/qs-rerelease
```

Each prototype opens the first 50,000 lines of the project's `.qc` files, in path order, as one
buffer at line 25,000. Keys: type, Enter, Backspace, Cmd+Alt+Down (Ctrl+Alt+Down on Windows
and Linux) or Alt+click to add a cursor, Escape for one cursor, Page Up and Page Down, the mouse
wheel. The OS input method works in both.

To measure, keep the window visible and the display awake, then:

```bash
QS_BENCH_HOST=ref-windows spikes/bench.sh /tmp/qs-rerelease docs/spikes/data/ref-windows 3
```

`bench.sh` runs each prototype with and without the 3D viewport three times, writes one CSV per
run and `summary.md`, and prints the summary. A single run is
`ui-gpui <project> --bench out.csv [--no-viewport]`. Use the host keys from
[`docs/perf-budgets.md`](../docs/perf-budgets.md#test-hardware): `ref-mac`, `ref-windows`,
`ref-linux`.

## What a frame's time is

Every frame applies one scripted action to the document (the same code in both), then the
framework lays out, paints and submits the frame. The recorded time is CPU time from the start
of that frame's work to the hand-off to the GPU, without any wait for vsync:

- **egui:** `eframe::Frame::info().cpu_usage`, which eframe measures around UI, tessellation and
  wgpu submission and from which it subtracts the vsync wait.
- **gpui:** the action's time plus gpui's own `Window::draw` and present durations from its
  `profiler` feature.

The CSV columns are documented in [`docs/spikes/data/README.md`](../docs/spikes/data/README.md).
