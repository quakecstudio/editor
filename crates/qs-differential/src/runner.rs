// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! The end-to-end differential runner over the corpus (`docs/spec/differential.md`, P1-13).
//!
//! For each corpus codebase an oracle compiler builds (id 1.01 with id's qcc, the rerelease
//! with the pinned FTEQCC), this writes scenarios (DIFF-2) with hand-authored entity lumps that
//! exercise as much of the codebase as the mock engine allows (DIFF-1.5: nothing moves and
//! nothing collides, DIFF-5.8), runs each in the id VM oracle and in QuakeC Studio's runner,
//! compares the two traces (DIFF-9) and measures the statements executed per codebase (DIFF-11).
//!
//! The oracle is injected as a closure that runs the id VM oracle on a scenario file and
//! returns its trace bytes, so the scenario authoring, the per-codebase loop and the
//! aggregation are unit-tested without the C oracle; the `differential` binary
//! (`src/bin/differential.rs`) supplies the real oracle subprocess, and `cargo xtask
//! differential` and CI drive it over the built oracles and corpus progs.
//!
//! Implements `docs/spec/differential.md` DIFF-1 (the two runners meeting over the scenario and
//! trace), DIFF-2 (the scenarios), DIFF-9 (the comparison) and DIFF-11 (coverage).

#![expect(
    clippy::print_stdout,
    reason = "the runner reports each codebase's divergences and coverage to standard output"
)]

use std::fs;
use std::path::{Path, PathBuf};

use crate::scenario::{self, Scenario};
use crate::suite::{self, CodebaseReport, ScenarioResult, SuiteError};

mod fixtures;

pub use fixtures::{ScenarioSpec, scenarios};

/// A closure that runs the id VM oracle on a written scenario file and returns its trace bytes
/// (`docs/spec/differential.md` DIFF-1): given the scenario path and the trace path to write,
/// it returns the trace or a [`SuiteError`]. The binary supplies the real oracle subprocess;
/// tests supply a fake.
pub type Oracle<'a> = dyn FnMut(&Path, &Path) -> Result<Vec<u8>, SuiteError> + 'a;

/// Which codebases to run (P1-13). The bar is zero divergences in both modes (ADR-0013);
/// coverage is reported, not gated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The fast CI subset: id 1.01 only; enforce zero divergences (QA-1, DIFF-9.6). The subset
    /// `cargo xtask ci` and the CI `Test Assets` job prove on every change.
    Fast,
    /// The full nightly corpus: every oracle-built codebase; enforce zero divergences
    /// (DIFF-9.6) and report statement coverage (DIFF-11, ADR-0013).
    Full,
}

impl Mode {
    /// The codebases this mode runs (their ids in `corpus/manifest.toml`).
    #[must_use]
    pub fn codebases(self) -> &'static [&'static str] {
        match self {
            Mode::Fast => &["id-1.01"],
            Mode::Full => &[
                "id-1.01",
                "rerelease",
                "rerelease-hipnotic",
                "rerelease-rogue",
                "rerelease-mg1",
                "rerelease-ctf",
                "rerelease-mg3",
            ],
        }
    }
}

/// Runs one codebase's scenarios and returns its report (DIFF-9, DIFF-11).
///
/// `progs` is the codebase's progs.dat (`cargo xtask corpus build` wrote it); `work` is a
/// scratch directory the scenario files are written under; `oracle` runs the id VM oracle on a
/// written scenario file and returns its trace bytes. The first divergence of each scenario is
/// printed to standard output (DIFF-9.5).
///
/// # Errors
///
/// A message when the progs cannot be read or a scenario's files cannot be written; a scenario
/// whose own run fails (a refused scenario, a bad oracle trace) is recorded as a divergence and
/// its message printed, so one bad scenario does not hide the rest.
pub fn run_codebase(
    id: &str,
    progs: &[u8],
    work: &Path,
    oracle: &mut Oracle<'_>,
) -> Result<CodebaseReport, String> {
    let total = suite::numstatements(progs).map_err(|e| e.to_string())?;
    let mut report = CodebaseReport::new(id, total);
    let dir = work.join(id);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;

    for spec in scenarios(id) {
        match run_one(id, progs, &dir, &spec, oracle) {
            Ok(result) => {
                if let Some(divergence) = result.divergence() {
                    println!(
                        "FAIL  {id}/{}: divergence at statement {} ({divergence:?})",
                        spec.name,
                        divergence.statement()
                    );
                }
                report.record(&result);
            }
            Err(e) => {
                println!("FAIL  {id}/{}: {e}", spec.name);
                report.count_error();
            }
        }
    }
    Ok(report)
}

/// Writes one scenario's files, runs it in the oracle and in QuakeC Studio's runner, and
/// compares the two traces (DIFF-1).
fn run_one(
    id: &str,
    progs: &[u8],
    dir: &Path,
    spec: &ScenarioSpec,
    oracle: &mut Oracle<'_>,
) -> Result<ScenarioResult, SuiteError> {
    let case = dir.join(spec.name);
    fs::create_dir_all(&case)
        .map_err(|e| SuiteError::Refused(format!("{}: {e}", case.display())))?;
    write(&case.join("progs.dat"), progs)?;
    let entities = spec.entities.clone().unwrap_or_default();
    let entities_line = if spec.entities.is_some() {
        write(&case.join("map.ent"), &entities)?;
        "entities map.ent\n"
    } else {
        ""
    };
    let text = format!("progs progs.dat\n{entities_line}{}", spec.directives);
    let scenario_path = case.join("case.scn");
    write(&scenario_path, text.as_bytes())?;

    // DIFF-2.5: the EDICT-8 parser reads the lump to the end of the slice or a NUL; the runner
    // reads the same bytes the oracle does. Refusing the scenario here would be a defect, since
    // the oracle accepts it (DIFF-2.9), so a refusal is reported as the error it is.
    let scn: Scenario = scenario::read(text.as_bytes(), &case)
        .map_err(|e| SuiteError::Refused(format!("{id}/{}: {e}", spec.name)))?;

    let oracle_trace = case.join("oracle.trace");
    suite::run_and_compare(&scn, progs, &entities, || {
        oracle(&scenario_path, &oracle_trace)
    })
}

/// Writes `bytes` to `path`, mapping an error into [`SuiteError::Refused`].
fn write(path: &Path, bytes: &[u8]) -> Result<(), SuiteError> {
    fs::write(path, bytes).map_err(|e| SuiteError::Refused(format!("{}: {e}", path.display())))
}

/// The progs.dat of codebase `id` under the asset cache (`cargo xtask corpus build` wrote it to
/// `<assets>/progs/<id>/progs.dat`).
#[must_use]
pub fn progs_path(assets: &Path, id: &str) -> PathBuf {
    assets.join("progs").join(id).join("progs.dat")
}

/// Prints one codebase's report and returns whether it met the differential bar (ADR-0013):
/// zero divergences over at least one scenario (DIFF-9.6). Statement coverage (DIFF-11) is
/// printed for information but not gated, because the mock does not drive the running world
/// (DIFF-5.8, DIFF-1.5).
#[must_use]
pub fn report_line(report: &CodebaseReport) -> bool {
    let coverage = report.coverage() * 100.0;
    let covered = report.executed.len();
    println!(
        "      {}: {} scenarios, {} divergences, {covered} of {} statements ({coverage:.1}%)",
        report.id, report.scenarios, report.divergences, report.total_statements
    );
    if !report.passes() {
        println!(
            "FAIL  {}: {} divergences over {} scenarios",
            report.id, report.divergences, report.scenarios
        );
        return false;
    }
    println!("ok    {}", report.id);
    true
}

#[cfg(test)]
#[path = "runner/tests.rs"]
mod tests;
