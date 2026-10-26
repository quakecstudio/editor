// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the end-to-end runner (DIFF-1, DIFF-9, DIFF-11), with a fake oracle so the
//! per-codebase loop and the report are exercised without the C oracle.
//!
//! The fake oracle answers with QuakeC Studio's own trace of the scenario file it is given,
//! changed where a test wants a divergence. So these tests prove the loop: that each scenario
//! is written, handed to the oracle once, compared with the runner's trace, and counted in the
//! report. That the two VMs agree is proved against id's C by the oracle suite
//! (`cargo xtask oracles`), not here.

#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code: a failed expect or unwrap is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_trace::{decode, encode};

use super::{Mode, progs_path, report_line, run_codebase, scenarios};
use crate::host::fixture::{FuncSpec, ProgsBuilder, Stmt};
use crate::run_scenario;
use crate::scenario::read_file;

/// A runnable progs whose spawn function and engine entry points share one empty body, so any
/// scenario's spawns and frames run without a `NULL function` error (EDICT-9.4).
fn runnable_progs() -> Vec<u8> {
    let entry = |name: &'static str| FuncSpec {
        name,
        first_statement: 1,
        ..FuncSpec::default()
    };
    ProgsBuilder::default()
        .statement(Stmt::op("DONE", 0, 0, 0))
        .function(entry("worldspawn"))
        .function(entry("StartFrame"))
        .function(entry("PlayerPreThink"))
        .function(entry("PlayerPostThink"))
        .function(entry("ClientConnect"))
        .function(entry("PutClientInServer"))
        .function(entry("SetNewParms"))
        .function(entry("ClientDisconnect"))
        .build()
}

/// A scratch directory under the OS temp dir for a test's scenario files (never inside the
/// source tree, so it is not scanned by the license or attribution checks).
fn work(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("qs-differential-runner-tests")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_modes_select_their_codebases() {
    assert_eq!(Mode::Fast.codebases(), ["id-1.01"]);
    assert!(Mode::Full.codebases().contains(&"id-1.01"));
    assert!(Mode::Full.codebases().contains(&"rerelease"));
}

#[test]
fn progs_path_is_under_the_asset_cache() {
    let path = progs_path(Path::new("/cache"), "id-1.01");
    assert!(
        path.ends_with("progs/id-1.01/progs.dat"),
        "{}",
        path.display()
    );
}

#[test]
fn every_codebase_has_scenarios() {
    for id in Mode::Full.codebases() {
        let specs = scenarios(id);
        assert!(!specs.is_empty(), "{id}");
        // Each scenario names a map, so a non-default map name is exercised.
        assert!(specs.iter().all(|s| s.directives.contains("map ")), "{id}");
    }
}

/// QuakeC Studio's trace of the scenario file at `scenario`, read as the oracle reads it.
fn own_trace(scenario: &Path) -> qc_trace::Trace {
    let scn = read_file(scenario).expect("the scenario reads");
    let case = scenario.parent().expect("a case directory");
    let progs_bytes = std::fs::read(case.join("progs.dat")).expect("progs");
    let entities = std::fs::read(case.join("map.ent")).unwrap_or_default();
    run_scenario(&scn, &progs_bytes, &entities)
        .expect("runs")
        .trace
}

// spec DIFF-1.1: each scenario is written once and run in both runners, and the two traces are
// compared; an oracle trace equal to the runner's agrees (DIFF-9.1), so the codebase has zero
// divergences.
#[test]
fn a_codebase_with_a_matching_oracle_has_no_divergences() {
    let progs = runnable_progs();
    let dir = work("matching");
    let mut asked: Vec<PathBuf> = Vec::new();
    let mut oracle = |scenario: &Path, trace: &Path| {
        asked.push(scenario.to_path_buf());
        let bytes = encode(&own_trace(scenario));
        std::fs::write(trace, &bytes).expect("write trace");
        Ok(bytes)
    };
    let report = run_codebase("id-1.01", &progs, &dir, &mut oracle).expect("the codebase runs");
    assert_eq!(report.divergences, 0, "a matching oracle agrees");
    // The oracle was asked once for each scenario of the codebase, each its own case file.
    let names: Vec<&str> = scenarios("id-1.01").iter().map(|spec| spec.name).collect();
    assert_eq!(report.scenarios, names.len());
    let cases: Vec<PathBuf> = names
        .iter()
        .map(|name| dir.join("id-1.01").join(name).join("case.scn"))
        .collect();
    assert_eq!(asked, cases);
    // The report prints and passes the differential bar (zero divergences, ADR-0013).
    assert!(report_line(&report));
}

// spec DIFF-9.5, DIFF-9.6: an oracle trace one record shorter diverges, and a divergence no
// reading accepts is a failure, counted for its own scenario only.
#[test]
fn a_codebase_with_a_diverging_oracle_reports_divergences() {
    let progs = runnable_progs();
    let dir = work("diverging");
    let mut calls = 0_usize;
    let mut oracle = |scenario: &Path, trace: &Path| {
        let mut data = own_trace(scenario);
        // Only the first scenario's oracle trace is short (DIFF-9.5).
        if calls == 0 {
            data.records.pop();
        }
        calls += 1;
        let bytes = encode(&data);
        std::fs::write(trace, &bytes).expect("write trace");
        Ok(bytes)
    };
    let report = run_codebase("id-1.01", &progs, &dir, &mut oracle).expect("the codebase runs");
    assert!(report.scenarios > 1, "{}", report.scenarios);
    assert_eq!(report.divergences, 1, "only the short trace diverges");
    assert!(!report_line(&report), "divergences fail the report");
}

// A scenario whose run errors (here the oracle cannot produce a decodable trace) counts as a
// divergence (DIFF-9.6), not a silent skip.
#[test]
fn an_oracle_error_counts_as_a_divergence() {
    let progs = runnable_progs();
    let dir = work("oracle-error");
    let mut oracle = |_scenario: &Path, _trace: &Path| Ok(b"not a trace".to_vec());
    let report = run_codebase("id-1.01", &progs, &dir, &mut oracle).expect("the codebase runs");
    assert_eq!(report.divergences, report.scenarios);
    assert!(!report_line(&report));
}

#[test]
fn a_bad_progs_fails_the_codebase() {
    let dir = work("bad-progs");
    let mut oracle = |_scenario: &Path, _trace: &Path| Ok(Vec::new());
    assert!(run_codebase("id-1.01", b"not a progs", &dir, &mut oracle).is_err());
}

#[test]
fn an_oracle_trace_decodes_as_written() {
    // A sanity check that the fake oracle writes a decodable trace, so the agreeing test is
    // meaningful.
    let progs = runnable_progs();
    let dir = work("decodes");
    let mut captured = Vec::new();
    {
        let mut oracle = |scenario: &Path, trace: &Path| {
            let scn = read_file(scenario).expect("reads");
            let case = scenario.parent().expect("case");
            let progs_bytes = std::fs::read(case.join("progs.dat")).expect("progs");
            let entities = std::fs::read(case.join("map.ent")).unwrap_or_default();
            let output = run_scenario(&scn, &progs_bytes, &entities).expect("runs");
            let bytes = encode(&output.trace);
            std::fs::write(trace, &bytes).expect("write");
            captured = bytes.clone();
            Ok(bytes)
        };
        run_codebase("id-1.01", &progs, &dir, &mut oracle).expect("runs");
    }
    assert!(decode(&captured).is_ok(), "the oracle trace decodes");
}
