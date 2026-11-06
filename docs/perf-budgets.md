# Performance budgets

QuakeC Studio has to be fast like Zed (PRIN-5) and show a saved change in the running game in
about a second (ENG-13). This document turns those targets into budgets with numbers, names the
benchmark that measures each one, and lists the machines the numbers hold on. TEST-8 in
[`testing.md`](testing.md) makes them part of every layer's readiness bar.

## The budgets

`cargo xtask bench` reads the table between the `perf-budgets` markers, so keep its shape:
one row per budget, the benchmark name in backticks, the budget as a number and a unit (`ns`,
`us`, `ms` or `s`), the statistic as `median`, `p95` or `p99`, and the status as `pending` or
`active`.

<!-- perf-budgets -->
| Benchmark | Requirement | Budget | Statistic | Filled in by | Status |
| --- | --- | --- | --- | --- | --- |
| `open_project_and_type` | PRIN-5 | 1 s | p95 | [#714](https://github.com/quakecstudio/editor/issues/714) P2-166 | active |
| `keystroke` | PRIN-5 | 8.33 ms | p99 | [#714](https://github.com/quakecstudio/editor/issues/714) P2-166 | active |
| `check_after_typing` | PRIN-5 | 100 ms | p95 | [#714](https://github.com/quakecstudio/editor/issues/714) P2-166 | active |
| `id1_recompile` | PRIN-5 | 200 ms | median | [#67](https://github.com/quakecstudio/editor/issues/67) P2-11 | active |
| `save_to_game` | ENG-13 | 1 s | p95 | [#100](https://github.com/quakecstudio/editor/issues/100) P3-12 | active |
| `frame_e1m1` | ENG-17 | 383 us | p99 | [#444](https://github.com/quakecstudio/editor/issues/444) P3-29 | active |
| `server_frame_e1m1` | ENG-17 | 1 ms | p99 | [#444](https://github.com/quakecstudio/editor/issues/444) P3-29 | active |
| `quakec_e1m1` | ENG-17 | 500 us | p99 | [#444](https://github.com/quakecstudio/editor/issues/444) P3-29 | active |
| `map_load_e1m1` | ENG-17 | 1 s | median | [#444](https://github.com/quakecstudio/editor/issues/444) P3-29 | active |
| `frame_mg1` | ENG-17 | 3.62 ms | p99 | [#444](https://github.com/quakecstudio/editor/issues/444) P3-29 | active |
| `server_frame_mg1` | ENG-17 | 1 ms | p99 | [#444](https://github.com/quakecstudio/editor/issues/444) P3-29 | active |
| `quakec_mg1` | ENG-17 | 500 us | p99 | [#444](https://github.com/quakecstudio/editor/issues/444) P3-29 | active |
| `map_load_mg1` | ENG-17 | 1 s | median | [#444](https://github.com/quakecstudio/editor/issues/444) P3-29 | active |
<!-- /perf-budgets -->

- **`open_project_and_type`:** open a 50,000-line QuakeC project in the editor and type the first
  character, from launch to that character on screen. At most 1 s at the 95th percentile. The
  benchmark runs the app headless, as its tests do, so it times everything but the renderer;
  the `editor_bench` example measures it in a window on each reference machine.
- **`keystroke`:** one keystroke in the code editor with semantic highlighting on, from the key
  event to the next presented frame. At most one frame at 120 Hz, 8.33 ms, at the 99th
  percentile, the same threshold as the UI framework spike ([#10](https://github.com/quakecstudio/editor/issues/10)).
- **`check_after_typing`:** the one frame that sends the editor's text to the project and
  checks it again, once typing has paused for the editor's settle time (0.3 s), on the same
  50,000-line project. At most 100 ms at the 95th percentile, not one frame: it runs once per
  pause in typing, not per keystroke, and holds the whole project's check, 85.63 ms on ref-mac
  in a window (decided by Justin on 2026-11-04 for P2-166). Moving the check off the frame
  would make it one frame's budget.
- **`id1_recompile`:** edit one function in id1-sized QuakeC and rebuild `progs.dat`
  incrementally. Median under 200 ms.
- **`save_to_game`:** save a QuakeC file in the editor until the change is visible in a
  **windowed** engine: compile, reload and the next rendered frame. p95 at most 1.0 s **on each of
  macOS, Windows and Linux**, measured with real windowed engines, not only the headless preview
  host ([#93](https://github.com/quakecstudio/editor/issues/93)). The benchmark needs a display
  and an engine, so it is measured on the reference machines below; the CI runners have neither.
  With `QS_BENCH_GAME` naming a game root holding `id1`, `cargo xtask bench` builds the release
  preview host and the benchmark plays a copy of id 1.01's QuakeC on e1m1 in its window. Each
  iteration makes `id1_recompile`'s edit to `W_FireAxe`, or takes it back, saves, and times
  `preview.reload` (the build and the reload, BRIDGE-4.8) and a `screenshot`, which the host
  answers once it has drawn the next frame. Without `QS_BENCH_GAME` the benchmark is a
  placeholder and the row is judged as pending; on a reference machine it refuses to run
  without one. ref-mac, GOG install, 2026-11-06: p95 77.04 ms.

- **The routes (`frame_*`, `server_frame_*`, `quakec_*`, `map_load_*`):** the preview host
  runs as players' engines do (ENG-17), on two routes: id1's e1m1 from `QS_BENCH_GAME`, and the
  rerelease's mg1 hub from `QS_BENCH_RERELEASE`, a game root holding `id1` and `mg1`. Each route
  stands at its start, so the server frames are the map's own with the player idle. The
  release host runs once per route with `--timerefresh <eye>`: it loads the map (`map_load_*`),
  runs a second of server frames, each timed (`server_frame_*`) with the wall time inside QuakeC
  in it (`quakec_*`), then draws id's `timerefresh` (`R_TimeRefresh_f`: 128 frames turning
  through a circle, no server frame between them) 16 times offscreen at 1280 by 800, each
  pass finished on the GPU and timed whole, its mean frame one sample (`frame_*`), as
  QuakeSpasm's `timerefresh` reports it: offscreen nothing holds the CPU back to the GPU, so
  a frame's own submit time would leave the GPU's work out. Each measurement is one criterion sample, so the percentiles are the
  measurements' own, not averages. With a route's variable unset its benchmarks are
  placeholders and the rows are judged as pending; on a reference machine they refuse to run.
  The eyes: e1m1 `480 -352 110`, the hub `0 -1920 46`.

### QuakeSpasm's numbers

ENG-17 holds the frame time to QuakeSpasm's on the same route and machine. QuakeSpasm is built
by hand at `sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5` and run only on the
reference machines, never in CI (ADR-0005): windowed at 1280 by 800, `-nosound`, `vid_vsync
0`, the map loaded, `setpos` to the route's start, then `timerefresh` five times
(Quake/gl_rmisc.c: the 128 frames drawn, then `glFinish`, timed together). Three runs each, on
ref-mac (Apple M5, 24 GB, macOS 27.2), 2026-11-06, seconds per 128 frames:

| Route | Runs | Median | Slowest |
| --- | --- | --- | --- |
| e1m1 | 0.047 0.025 0.024 0.020 0.018, 0.049 0.020 0.025 0.020 0.019, 0.041 0.024 0.019 0.018 0.024 | 0.024 | 0.049 |
| mg1 hub (protocol 666) | 0.344 0.436 0.429 0.410 0.433, 0.300 0.383 0.372 0.375 0.419, 0.311 0.463 0.445 0.448 0.445 | 0.419 | 0.463 |

The first pass of each run is the slowest; QuakeSpasm reports no per-frame times. Each
`frame_*` budget is QuakeSpasm's slowest pass as a mean frame, 0.049 s / 128 = 383 us and
0.463 s / 128 = 3.62 ms, and the margin our engine is allowed is none: the 99th percentile of
our 16 passes' mean frames, which is the slowest pass and so the first, cold one, must be
within QuakeSpasm's slowest pass, which is its first too. Our run
on the same day: e1m1 p99 0.172 ms a frame, the hub p99 2.43 ms.

QuakeSpasm has no finer clock for a server frame, its QuakeC or a map load than `host_speeds`,
which prints whole milliseconds, so those budgets are ours: a server frame within 1 ms and its
QuakeC within 0.5 ms at the 99th percentile, a fourteenth of the 72 Hz tick, and a map loaded
within ENG-13's second. Measured the same day: server frames p99 0.54 ms (e1m1) and 0.49 ms
(hub), QuakeC p99 0.10 ms and 0.04 ms, loads 0.02 s and 0.69 s. The mg3 and Arcane Dimensions
routes, and the Windows and Linux reference machines, are measured in P4-24.

## How the harness works

The benchmarks are in `crates/qs-bench/benches/budgets.rs`, one criterion benchmark per row, all
in the criterion group `budgets`. A test in `xtask` fails when a row has no benchmark of the same
name.

`cargo xtask bench` (or `make bench`):

1. fetches id 1.01 into the asset cache for `id1_recompile` (`cargo xtask corpus fetch
   id-1.01`; offline once cached), then runs the benchmarks with `cargo bench --package qs-bench
   --bench budgets`, with `QS_BENCH_ID1` naming that directory, writing criterion's output to a
   fresh `target/xtask/bench/criterion/`;
2. reads each benchmark's samples and computes the median, p95 and p99 time per iteration
   (nearest rank over criterion's samples);
3. for each **active** row, fails if the row's statistic is over its budget on a reference
   machine (`QS_BENCH_HOST` set to `ref-mac`, `ref-windows` or `ref-linux`), or more than 10%
   slower than the last passing run recorded on the same host. Another host, a CI runner among
   them, is judged by the 10% rule alone: the budgets are the modder's, and hold on the
   reference machines (decided by Justin on 2026-11-04 for P2-166);
4. appends the run to the JSON history, passed or not, and prints the results.

`pending` rows still run, so every budget's benchmark exists and is exercised, but they are
neither checked nor compared: their body is a placeholder until the issue in "Filled in by"
measures the real operation and changes the row to `active` in the same PR.

Options:

- `--history <path>` uses another history file. The default is `target/bench/history.json`.
- `--accept` records an intended slowdown: regressions are reported but do not fail the run,
  and the run becomes the new baseline. Over-budget results still fail.
- `cargo xtask bench history` prints every recorded run.

In GitHub Actions each failure is also printed as an `::error` annotation, so a regression shows
on the pull request.

### The history

Each run records the time, the commit, the host, whether it passed, and per benchmark its
status, budget, statistic, sample count and median, p95 and p99 in nanoseconds. Runs compare
only with earlier runs from the same host, because numbers from different machines are not
comparable. The host key is `QS_BENCH_HOST` if it is set, otherwise the OS, architecture and CPU
model; set `QS_BENCH_HOST` to name a reference machine (for example `ref-mac`) or a CI runner
class whose CPU model varies from run to run.

### Percentiles and samples

Criterion times batches of iterations, so the percentiles are over its per-sample averages. For an
operation that takes milliseconds or more, a sample is one or a few iterations and the percentile
is close to the per-event one. For a fast operation whose tail matters (`keystroke`), averaging
hides the slow events, so its benchmark must make each sample reflect single events rather than
an average of many, and say how in its doc comment.

## Test hardware

The budgets must hold on the three reference machines from the design doc's UI acceptance test,
one per OS:

| Host key | Machine | OS |
| --- | --- | --- |
| `ref-mac` | Apple M-series Mac (M1 or later) | macOS, current release |
| `ref-windows` | mid-range Windows laptop | Windows 11 |
| `ref-linux` | Linux desktop or laptop | a current distribution under Wayland |

The history records each host's CPU model and core count, so the exact machine behind every
number is in the data. [#134](https://github.com/quakecstudio/editor/issues/134) (P4-23) measures
every budget on these machines for the MVP and writes `docs/gates/perf-mvp.md`.

### In CI

The `Benchmarks` workflow ([`.github/workflows/bench.yml`](../.github/workflows/bench.yml)) runs
`cargo xtask bench` on a GitHub-hosted Linux runner every Monday and on demand (Actions, Run
workflow), with `QS_BENCH_HOST` set to the runner class (`ci-Linux-X64`). It restores the history
saved by the last run on `main` from the Actions cache, prints the history, uploads it as the
`bench-history` artifact, and saves it back to the cache only from `main`. A regression or
over-budget result fails the run with an error annotation.

It does not run on pull requests or in the merge queue, and it is not a required check: CI stays
lean until there is product code to measure, and shared runners vary too much for a 10% threshold
to block merges. Pull requests still compile the benchmarks, in the `Clippy` job
(`--all-targets`). The runners' absolute numbers do not decide whether a budget is met; the
reference machines do.
