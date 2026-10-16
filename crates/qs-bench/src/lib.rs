// SPDX-License-Identifier: GPL-3.0-or-later

//! The QuakeC Studio performance-budget benchmarks.
//!
//! The benchmarks live in `benches/budgets.rs`, one per budget in
//! `docs/perf-budgets.md`. `cargo xtask bench` runs them, checks each active
//! one against its budget and the last recorded run, and appends the results to
//! the JSON history. This library is empty: a package needs a target besides
//! its benchmarks.
