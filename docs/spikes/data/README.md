# UI framework spike: raw data

Raw frame timings from the UI framework spike ([#10](https://github.com/quakecstudio/editor/issues/10)),
written by [`spikes/bench.sh`](../../../spikes/bench.sh) and summarised in
[`ui-framework.md`](../ui-framework.md). One directory per reference machine
([`docs/perf-budgets.md`](../../perf-budgets.md#test-hardware)), one CSV per run, and the
`summary.md` that `spikes/summarize.py` made from them.

File names are `<host>-<framework>-<viewport|noviewport>-<run>.csv`. Columns:

| Column | Meaning |
| --- | --- |
| `framework` | `egui` or `gpui`, with `-noviewport` when the 3D viewport was off |
| `host` | The host key (`QS_BENCH_HOST`) |
| `phase` | The script phase: `warmup` (not measured), `type`, `scroll`, `multicursor`, `ime`, `diff` |
| `frame` | The step's index in the script |
| `frame_ns` | CPU time to produce the frame, without the vsync wait (see [`spikes/README.md`](../../../spikes/README.md#what-a-frames-time-is)) |
| `action_ns` | Time to apply the scripted action to the document; the same code in both prototypes |
| `interval_ns` | Wall time since the previous frame started; bounded below by the display's refresh period |
| `draw_ns`, `present_ns` | gpui only: its `Window::draw` and present times, which with `action_ns` make up `frame_ns`; 0 for egui |

| Directory | Machine | Display |
| --- | --- | --- |
| [`ref-mac/`](ref-mac/) | Mac17,2 (Apple M5, 10 CPU cores, 10-core GPU, 24 GB), macOS 27.0.1 | External LG UltraFine 5K at 60 Hz |
