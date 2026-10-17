// SPDX-License-Identifier: GPL-3.0-or-later

//! QuakeC Studio's side of the differential VM runner (`docs/spec/differential.md`, P1-13).
//!
//! A differential run executes one scenario in two VMs, the id VM oracle
//! (`oracles/vm/scenario.c`, built by `cargo xtask oracles`) and QuakeC Studio's `qc-vm`, and
//! compares the two traces statement by statement (DIFF-1). This crate is the QuakeC Studio
//! runner: it reads a scenario ([`scenario`]), drives `qc-vm` with `qc-edict` and the
//! `qc-builtins` mock engine through the host loop of DIFF-3 to DIFF-5, and compares two traces
//! by DIFF-9. Neither runner links the other; this page and the specs it cites are the whole
//! contract between them (DIFF-1.1).
//!
//! This is a test-layer crate: it is never part of the shipped app. It is shipped-quality all
//! the same (no panics, every value checked) because the differential suite is a merge gate
//! (QA-1) and P1-13's verification is a mutation test.
//!
//! Implements `docs/spec/differential.md`. The trace format is `qc-trace` (`trace.md`); the
//! VM is `qc-progs` and `qc-vm`; the scenario format is DIFF-2. Two traces are compared and
//! statement coverage is measured by [`compare`] (DIFF-9 to DIFF-11).
//!
//! # Layer
//!
//! Tests. It builds on the L1 VM crates and the L0 formats below them, and nothing depends on
//! it (`layers.toml`).

pub mod compare;
pub mod edict8;
pub mod host;
pub mod scenario;

pub use compare::{Comparison, Divergence, Slot, compare, coverage};
pub use host::{Ended, RunOutput, RunRefused, run_scenario};
pub use scenario::{Cvar, Input, ModelBounds, Refused, Scenario};
