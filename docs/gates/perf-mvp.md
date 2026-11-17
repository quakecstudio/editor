# MVP performance budgets

P4-23 ([#134](https://github.com/quakecstudio/editor/issues/134)), PRIN-5: every budget of
[`perf-budgets.md`](../perf-budgets.md) measured on ref-mac and the CI runner, each met or with
a dated plan for its miss. The budgets and how `cargo xtask bench` judges them are in that
file. ref-windows and ref-linux moved to L-16 (Justin, 2026-11-11).

**Status:** every budget is met on ref-mac, the machine the budgets hold on. On the CI runner
two map budgets are over their numbers; the runner is not a reference machine, and the plan for
them is below.

## On ref-mac

`QS_BENCH_HOST=ref-mac QS_BENCH_GAME=<GOG install> QS_BENCH_RERELEASE=<GOG install> cargo xtask
bench`, commit `01af86bd`, 2026-11-16: exit 0, every active row inside its budget. ref-mac is an
Apple M5 (10 cores) with 24 GB running macOS 27.2.1, reading the GOG install's `id1` and `mg1`.

| Benchmark | Statistic | Budget | Measured | |
| --- | --- | --- | --- | --- |
| `open_project_and_type` | p95 | 1 s | 260.46 ms | met |
| `keystroke` | p99 | 8.33 ms | 0.128 ms | met |
| `keystroke_split_fold_wrap` | p99 | 8.33 ms | 0.192 ms | met |
| `check_after_typing` | p99 | 8.33 ms | 0.356 ms | met |
| `entities_outliner` | p99 | 8.33 ms | 0.138 ms | met |
| `id1_recompile` | median | 200 ms | 50.87 ms | met |
| `save_to_game` | p95 | 1 s | 80.59 ms | met |
| `frame_e1m1` | p99 | 383 us | 161.82 us | met |
| `server_frame_e1m1` | p99 | 1 ms | 0.459 ms | met |
| `quakec_e1m1` | p99 | 500 us | 83.83 us | met |
| `map_load_e1m1` | median | 1 s | 93.66 ms | met |
| `frame_mg1` | p99 | 3.62 ms | 2.33 ms | met |
| `server_frame_mg1` | p99 | 1 ms | 0.547 ms | met |
| `quakec_mg1` | p99 | 500 us | 35.50 us | met |
| `map_load_mg1` | median | 1 s | 715.74 ms | met |
| `play_from_here_e1m1` | median | 6 s | 3.181 s | met |
| `region_compile_e1m1` | median | 1.2 s | 168.32 ms | met |
| `full_compile_e1m1` | median | 8 s | 6.028 s | met |
| `textured_view_e1m1` | median | 50 ms | 10.79 ms | met |

## On the CI runner

`cargo xtask bench`, Bench workflow run
[43898479297](https://github.com/quakecstudio/editor/actions/runs/43898479297), commit
`4cf003aa`, 2026-11-16, host `ci-Linux-X64`. The engine rows are placeholders there, because the
runner has no game (`QS_BENCH_GAME` unset), and a CI runner is judged by the 10% rule against
its own earlier runs, not by the budgets ([`perf-budgets.md`](../perf-budgets.md)).

| Benchmark | Statistic | Budget | Measured | |
| --- | --- | --- | --- | --- |
| `open_project_and_type` | p95 | 1 s | 645.85 ms | met |
| `keystroke` | p99 | 8.33 ms | 0.251 ms | met |
| `keystroke_split_fold_wrap` | p99 | 8.33 ms | 0.394 ms | met |
| `check_after_typing` | p99 | 8.33 ms | 6.21 ms | met |
| `entities_outliner` | p99 | 8.33 ms | 0.288 ms | met |
| `id1_recompile` | median | 200 ms | 124.66 ms | met |
| `play_from_here_e1m1` | median | 6 s | 10.295 s | over |
| `region_compile_e1m1` | median | 1.2 s | 351.81 ms | met |
| `full_compile_e1m1` | median | 8 s | 20.195 s | over |
| `textured_view_e1m1` | median | 50 ms | 30.64 ms | met |

**The two over their numbers (plan, 2026-11-16).** `play_from_here_e1m1` and
`full_compile_e1m1` are the map compiles, which use every core, and both are inside their
budgets on ref-mac (3.18 s and 6.03 s). Nothing is planned for the runner: the budgets are the
modder's machine's, and the runner holds these two to the 10% rule, which they pass (10.400 s
and 20.128 s in the run before).

That run failed on the 10% rule for `entities_outliner`: 0.288 ms against 0.211 ms in the run
before, both about a thirtieth of the budget. It is 0.138 ms on ref-mac. Whether it is the
shared runner's noise at this scale or a real slowdown is for the next scheduled run to show.
