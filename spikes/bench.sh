#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Runs the spike's benchmark: both prototypes, with and without the 3D viewport, RUNS times
# each, in alternating order, and writes one CSV per run, the run's printed summary (with its
# startup times) as a .txt beside it, and summary.md.
#
# usage: QS_BENCH_HOST=ref-mac spikes/bench.sh <project-dir> <out-dir> [runs]
#
# Keep the window on screen and the display awake while it runs (it takes about a minute per
# run at 60 Hz): an occluded or sleeping window gets no frames on macOS, and egui then runs
# without presenting, which makes its numbers meaningless.
set -euo pipefail

project=${1:?usage: bench.sh <project-dir> <out-dir> [runs]}
out=${2:?usage: bench.sh <project-dir> <out-dir> [runs]}
runs=${3:-3}
host=${QS_BENCH_HOST:-$(uname -s)-$(uname -m)}
here=$(cd "$(dirname "$0")" && pwd)

cargo build --release --locked --manifest-path "$here/Cargo.toml" -p ui-egui -p ui-gpui
mkdir -p "$out"

run=1
while [[ "$run" -le "$runs" ]]; do
  for framework in egui gpui; do
    for mode in viewport noviewport; do
      flag=""
      if [[ "$mode" == noviewport ]]; then
        flag="--no-viewport"
      fi
      csv="$out/$host-$framework-$mode-$run.csv"
      echo "== $framework $mode run $run"
      # shellcheck disable=SC2086 # $flag is empty or one word
      QS_BENCH_HOST="$host" "$here/target/release/ui-$framework" "$project" --bench "$csv" $flag |
        tee "${csv%.csv}.txt"
    done
  done
  run=$((run + 1))
done

python3 "$here/summarize.py" "$out"/*.csv >"$out/summary.md"
cat "$out/summary.md"
