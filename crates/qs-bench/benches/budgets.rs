// SPDX-License-Identifier: GPL-3.0-or-later

//! One benchmark per budget in `docs/perf-budgets.md` (PRIN-5, ENG-13).
//!
//! `cargo xtask bench` runs this file and reads criterion's results from the
//! `budgets/<name>/new/` directories it writes. The benchmark names here must
//! match the budget table; an xtask test checks that they do.
//!
//! A budget marked `pending` in the table has a placeholder body here until
//! the issue named in its row measures the real operation and marks the row
//! `active`. Placeholders use a short measurement so CI stays quick.

use std::hint::black_box;
use std::time::Duration;

use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion, criterion_group, criterion_main};

/// The criterion group every budget benchmark belongs to.
const GROUP: &str = "budgets";

/// Runs a placeholder for a budget whose measurement a later issue fills in.
fn placeholder(group: &mut BenchmarkGroup<'_, WallTime>, name: &str) {
    group.bench_function(name, |bencher| bencher.iter(|| black_box(name.len())));
}

/// Open a 50,000-line project and type the first character (P2-18).
fn open_project_and_type(group: &mut BenchmarkGroup<'_, WallTime>) {
    placeholder(group, "open_project_and_type");
}

/// One keystroke to the next presented frame in the code editor (P2-18).
fn keystroke(group: &mut BenchmarkGroup<'_, WallTime>) {
    placeholder(group, "keystroke");
}

/// Edit one function in id1-sized code and rebuild progs.dat (P2-11).
fn id1_recompile(group: &mut BenchmarkGroup<'_, WallTime>) {
    placeholder(group, "id1_recompile");
}

/// Save a QuakeC file to the change visible in a windowed engine (P3-12).
fn save_to_game(group: &mut BenchmarkGroup<'_, WallTime>) {
    placeholder(group, "save_to_game");
}

fn budgets(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group(GROUP);
    group
        .sample_size(10)
        .warm_up_time(Duration::from_millis(100))
        .measurement_time(Duration::from_millis(500));
    open_project_and_type(&mut group);
    keystroke(&mut group);
    id1_recompile(&mut group);
    save_to_game(&mut group);
    group.finish();
}

criterion_group!(benches, budgets);
criterion_main!(benches);
