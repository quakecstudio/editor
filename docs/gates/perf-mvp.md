# MVP performance budgets

P4-23 ([#134](https://github.com/quakecstudio/editor/issues/134)), PRIN-5: every budget of
[`perf-budgets.md`](../perf-budgets.md) measured on the three reference machines, each met or
with a dated plan for its miss. The budgets and how `cargo xtask bench` judges them are in
that file.

**Status:** measured on the CI runner and, for the engine routes and `save_to_game`, on
ref-mac. The full run on each reference machine is still to do. It needs the machines and a
copy of the game, so a person has to run it (below).

## On the CI runner

`cargo xtask bench`, Bench workflow run
[42751429066](https://github.com/quakecstudio/editor/actions/runs/42751429066), commit
`2980d70`, 2026-11-10. The host was `ci-Linux-X64`: AMD EPYC 9V74, 4 cores. The engine rows are
placeholders there, because the runner has no game (`QS_BENCH_GAME` unset). The 10% rule
against earlier runs on the same host is what CI holds.

| Benchmark | Statistic | Budget | Measured | |
| --- | --- | --- | --- | --- |
| `open_project_and_type` | p95 | 1 s | 716.5 ms | met |
| `keystroke` | p99 | 8.33 ms | 0.382 ms | met |
| `check_after_typing` | p99 | 8.33 ms | 0.391 ms | met |
| `entities_outliner` | p99 | 8.33 ms | 0.220 ms | met |
| `id1_recompile` | median | 200 ms | 181.5 ms | met |

## On ref-mac

Recorded in [`perf-budgets.md`](../perf-budgets.md) on 2026-11-06. ref-mac is an Apple M5 with
24 GB running macOS 27.2, reading the GOG install.

| Benchmark | Statistic | Budget | Measured | |
| --- | --- | --- | --- | --- |
| `save_to_game` | p95 | 1 s | 77.04 ms | met |
| `frame_e1m1` | p99 | 383 us | 172 us | met |
| `server_frame_e1m1` | p99 | 1 ms | 0.54 ms | met |
| `quakec_e1m1` | p99 | 500 us | 0.10 ms | met |
| `map_load_e1m1` | median | 1 s | 0.02 s | met |
| `frame_mg1` | p99 | 3.62 ms | 2.43 ms | met |
| `server_frame_mg1` | p99 | 1 ms | 0.49 ms | met |
| `quakec_mg1` | p99 | 500 us | 0.04 ms | met |
| `map_load_mg1` | median | 1 s | 0.69 s | met |

## To measure

On each of ref-mac, ref-windows and ref-linux (the machines of
[`perf-budgets.md`](../perf-budgets.md#test-hardware)), with the original game and the
rerelease installed:

```bash
QS_BENCH_HOST=ref-mac \
QS_BENCH_GAME=/path/to/Quake \
QS_BENCH_RERELEASE=/path/to/Quake/rerelease \
  cargo xtask bench
cargo xtask bench history
```

On ref-windows and ref-linux, `QS_BENCH_HOST` is `ref-windows` and `ref-linux`. On a reference
machine the run fails on any budget it misses, and the engine benchmarks refuse to run without
the game. Each machine's `cargo xtask bench history` output goes into a table here, as above.
A budget it misses gets a dated plan here and a follow-up issue in `plan/`.
