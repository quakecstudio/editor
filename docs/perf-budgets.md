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
| `open_project_and_type` | PRIN-5 | 1 s | p95 | [#74](https://github.com/quakecstudio/editor/issues/74) P2-18 | pending |
| `keystroke` | PRIN-5 | 8.33 ms | p99 | [#74](https://github.com/quakecstudio/editor/issues/74) P2-18 | pending |
| `id1_recompile` | PRIN-5 | 200 ms | median | [#67](https://github.com/quakecstudio/editor/issues/67) P2-11 | pending |
| `save_to_game` | ENG-13 | 1 s | p95 | [#100](https://github.com/quakecstudio/editor/issues/100) P3-12 | pending |
<!-- /perf-budgets -->

- **`open_project_and_type`:** open a 50,000-line QuakeC project in the editor and type the first
  character, from launch to that character on screen. At most 1 s at the 95th percentile.
- **`keystroke`:** one keystroke in the code editor with semantic highlighting on, from the key
  event to the next presented frame. At most one frame at 120 Hz, 8.33 ms, at the 99th
  percentile, the same threshold as the UI framework spike ([#10](https://github.com/quakecstudio/editor/issues/10)).
- **`id1_recompile`:** edit one function in id1-sized QuakeC and rebuild `progs.dat`
  incrementally. Median under 200 ms.
- **`save_to_game`:** save a QuakeC file in the editor until the change is visible in a
  **windowed** engine: compile, reload and the next rendered frame. p95 at most 1.0 s **on each of
  macOS, Windows and Linux**, measured with real windowed engines, not only the headless preview
  host ([#93](https://github.com/quakecstudio/editor/issues/93)). The benchmark needs a display
  and an engine, so it is measured on the reference machines below; the CI runners have neither.

## How the harness works

The benchmarks are in `crates/qs-bench/benches/budgets.rs`, one criterion benchmark per row, all
in the criterion group `budgets`. A test in `xtask` fails when a row has no benchmark of the same
name.

`cargo xtask bench` (or `make bench`):

1. runs the benchmarks with `cargo bench --package qs-bench --bench budgets`, writing criterion's
   output to a fresh `target/xtask/bench/criterion/`;
2. reads each benchmark's samples and computes the median, p95 and p99 time per iteration
   (nearest rank over criterion's samples);
3. for each **active** row, fails if the row's statistic is over its budget, or more than 10%
   slower than the last passing run recorded on the same host;
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
`cargo xtask bench` on a GitHub-hosted Linux runner for every pull request and every push to
`main`, with `QS_BENCH_HOST` set to the runner class (`ci-Linux-X64`). It restores the history
saved by the last run on `main` from the Actions cache, so a pull request is compared with
`main`, prints the history, uploads it as the `bench-history` artifact, and saves it back to the
cache only from `main`. A regression or over-budget result fails the job with an error
annotation on the pull request. The job is not a required check: shared runners vary too much
for a 10% threshold to block merges, and their absolute numbers do not decide whether a budget
is met; the reference machines do.
