// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the end-to-end differential runner (DIFF-1, DIFF-2.5, DIFF-9, DIFF-11).
//!
//! The entity-lump, coverage and report tests build their inputs directly; `run_and_compare`
//! is tested with a fake oracle, so the comparison and coverage glue is exercised without the
//! C oracle (the mutation test of P1-13 covers this module). The spec-citation ratchet
//! (`crates/qs-tests`) holds DIFF-2.5, DIFF-9 and DIFF-11 to these and the comparator's tests.

#![expect(
    clippy::expect_used,
    reason = "test code: a failed expect is a failed test"
)]

use std::path::PathBuf;

use qc_trace::{Changes, Header, Record, Step, Trace, encode};

use super::{
    CodebaseReport, ScenarioResult, SuiteError, entity_lump, initial_globals, numstatements,
    records_before, run_and_compare,
};
use crate::compare::{Comparison, Divergence};
use crate::host::fixture::{FuncSpec, ProgsBuilder, Stmt};
use crate::scenario::{DIFF_DEFAULT_FRAMETIME, DIFF_DEFAULT_SEED, Scenario};

/// A scenario with the given map and the rest at their defaults (DIFF-2.2).
fn scenario(map: &str) -> Scenario {
    Scenario {
        progs: PathBuf::from("progs.dat"),
        entities: None,
        map: map.to_owned(),
        maxclients: 1,
        clients: 0,
        submodels: 0,
        frames: 0,
        frametime: DIFF_DEFAULT_FRAMETIME,
        seed: DIFF_DEFAULT_SEED,
        models: Vec::new(),
        cvars: Vec::new(),
        inputs: Vec::new(),
    }
}

/// A runnable progs whose spawn function and engine entry points share one empty body, so a
/// scenario that spawns a worldspawn and runs frames does not fail with `NULL function`.
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

/// A `.map` with the world, a point entity and a brush entity (one brush), as `qs-map` reads
/// it. The brush is a minimal cube in standard format.
const MAP: &[u8] = br#"{
"classname" "worldspawn"
"message" "a map"
"wad" "quake.wad"
}
{
"classname" "info_player_start"
"origin" "0 0 24"
}
{
"classname" "func_door"
"angle" "90"
"model" "ignored.bsp"
{
( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) t1 0 0 0 1 1
( 0 0 0 ) ( 0 0 1 ) ( 1 0 0 ) t1 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) t1 0 0 0 1 1
( 64 64 64 ) ( 64 65 64 ) ( 64 64 65 ) t1 0 0 0 1 1
( 64 64 64 ) ( 65 64 64 ) ( 64 65 64 ) t1 0 0 0 1 1
( 64 64 64 ) ( 64 64 65 ) ( 65 64 64 ) t1 0 0 0 1 1
}
}
"#;

// spec DIFF-2.5: the entities file is read as ED_LoadFromFile reads a BSP entity lump; the
// runner makes it from a .map with the brushes removed, and both runners read the same file.
#[test]
fn an_entity_lump_drops_the_brushes_and_keeps_the_pairs() {
    let lump = entity_lump(MAP).expect("the map parses");
    let text = String::from_utf8(lump).expect("the lump is text");
    // Every entity's pairs survive, as quoted pairs; no brush face line does.
    assert!(text.contains("\"classname\" \"worldspawn\""), "{text}");
    assert!(text.contains("\"message\" \"a map\""), "{text}");
    assert!(
        text.contains("\"classname\" \"info_player_start\""),
        "{text}"
    );
    assert!(text.contains("\"classname\" \"func_door\""), "{text}");
    assert!(text.contains("\"angle\" \"90\""), "{text}");
    // No brush geometry leaks in.
    assert!(!text.contains('('), "a brush face line leaked: {text}");
    assert!(!text.contains("t1"), "a texture name leaked: {text}");
    // Three entities, three opening and closing braces.
    assert_eq!(text.matches('{').count(), 3, "{text}");
    assert_eq!(text.matches('}').count(), 3, "{text}");
}

// spec DIFF-2.5: a brush entity's model is the submodel qbsp assigns (*1, *2, ...), not the
// value the .map held; MAP-6.2 numbers brush entities after the world from 1 in file order.
#[test]
fn a_brush_entity_gets_its_submodel_number() {
    let lump = entity_lump(MAP).expect("the map parses");
    let text = String::from_utf8(lump).expect("the lump is text");
    // The only brush entity is func_door, the first brush entity after the world: model *1.
    assert!(text.contains("\"model\" \"*1\""), "{text}");
    // The .map's literal model value is overwritten, not kept.
    assert!(!text.contains("ignored.bsp"), "{text}");
}

// spec DIFF-2.5: a brush entity without a model pair still gains its submodel number, so
// setmodel finds the submodel.
#[test]
fn a_brush_entity_without_a_model_pair_gains_one() {
    let map = br#"{
"classname" "worldspawn"
}
{
"classname" "func_wall"
{
( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) t 0 0 0 1 1
( 0 0 0 ) ( 0 0 1 ) ( 1 0 0 ) t 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) t 0 0 0 1 1
( 1 1 1 ) ( 1 2 1 ) ( 1 1 2 ) t 0 0 0 1 1
( 1 1 1 ) ( 2 1 1 ) ( 1 2 1 ) t 0 0 0 1 1
( 1 1 1 ) ( 1 1 2 ) ( 2 1 1 ) t 0 0 0 1 1
}
}
"#;
    let text = String::from_utf8(entity_lump(map).expect("parses")).expect("text");
    assert!(text.contains("\"model\" \"*1\""), "{text}");
}

#[test]
fn a_map_qs_map_refuses_is_a_suite_error() {
    let err = entity_lump(b"not a map {{{").expect_err("the map is rejected");
    assert!(matches!(err, SuiteError::Map(_)), "{err:?}");
}

#[test]
fn initial_globals_and_numstatements_read_the_progs() {
    let progs = runnable_progs();
    let globals = initial_globals(&progs).expect("globals");
    assert_eq!(globals.len(), 128, "the builder lays out 128 globals");
    // One DONE statement plus the reserved statement 0.
    assert_eq!(numstatements(&progs).expect("count"), 2);
    assert!(matches!(
        initial_globals(b"not a progs"),
        Err(SuiteError::Progs(_))
    ));
    assert!(matches!(
        numstatements(b"not a progs"),
        Err(SuiteError::Progs(_))
    ));
}

/// A header for building traces directly (TRACE-2).
fn header() -> Header {
    Header::new(5927, 516, 96, 105, 1000).expect("a valid header")
}

/// A step record at `statement` with `opcode` and no change set (TRACE-4.2).
fn step(statement: i32, opcode: u16) -> Record {
    Record::Step(Step {
        function: 1,
        statement,
        opcode,
        builtin: None,
        changes: Changes::default(),
    })
}

// spec DIFF-11.2: coverage counts only the records before the first divergence; an equal
// comparison counts the whole trace.
#[test]
fn records_before_counts_the_whole_trace_when_equal() {
    let trace = Trace {
        header: header(),
        records: vec![step(1, 10), step(2, 11), step(3, 12)],
    };
    assert_eq!(records_before(&trace, &Comparison::Equal), 3);
}

// spec DIFF-11.2: only the records before the first divergence count: the diverging
// change-carrying record itself does not.
// spec DIFF-9.5: the divergence names the index of the first differing change-carrying record.
#[test]
fn records_before_stops_at_the_divergence() {
    let trace = Trace {
        header: header(),
        records: vec![step(1, 10), step(2, 11), step(3, 12)],
    };
    // A record divergence at change-carrying index 1 (the second step): count record 0 only.
    let divergence = Comparison::Failure(Divergence::Record {
        index: 1,
        statement_a: 2,
        statement_b: 2,
        what: "opcode",
    });
    assert_eq!(records_before(&trace, &divergence), 1);
    // A length divergence at index 2, this trace the longer: its third record has no
    // counterpart, so the two before it count.
    let length = Comparison::Failure(Divergence::Length {
        index: 2,
        shorter_is_a: false,
    });
    assert_eq!(records_before(&trace, &length), 2);
    // At index 3, this trace the shorter: every record it has agreed.
    let shorter = Comparison::Failure(Divergence::Length {
        index: 3,
        shorter_is_a: true,
    });
    assert_eq!(records_before(&trace, &shorter), 3);
    // A header divergence compared nothing: index 0, so no record counts.
    let header_div = Comparison::Failure(Divergence::Header {
        field: "numstrings",
    });
    assert_eq!(records_before(&trace, &header_div), 0);
}

// spec DIFF-1.1: a scenario runs in both runners and the two traces are compared; here the
// oracle returns the same trace QuakeC Studio wrote, so they agree (DIFF-9.1).
#[test]
fn run_and_compare_agrees_when_the_oracle_matches() {
    let progs = runnable_progs();
    let mut scn = scenario("start");
    scn.frames = 1;
    let entities = b"{\n\"classname\" \"worldspawn\"\n}\n";
    // Run ourselves once to get the trace, then feed its bytes back as the oracle's trace.
    let ours = crate::run_scenario(&scn, &progs, entities).expect("runs");
    let oracle_bytes = encode(&ours.trace);
    let result = run_and_compare(&scn, &progs, entities, || Ok(oracle_bytes.clone()))
        .expect("the comparison runs");
    assert!(result.agrees(), "{:?}", result.comparison);
    assert!(result.divergence().is_none());
    // The whole trace counts for coverage (DIFF-11.2).
    assert_eq!(result.records_before_divergence, result.trace.records.len());
}

// spec DIFF-9.6: a divergence the readings leave is a failure; here the oracle's trace has one
// fewer change-carrying record, so a length divergence is reported (DIFF-9.5).
#[test]
fn run_and_compare_reports_a_divergence() {
    let progs = runnable_progs();
    let mut scn = scenario("start");
    scn.frames = 1;
    let entities = b"{\n\"classname\" \"worldspawn\"\n}\n";
    let ours = crate::run_scenario(&scn, &progs, entities).expect("runs");
    // The oracle's trace drops the last record, so it is shorter (DIFF-9.5).
    let mut truncated = ours.trace.clone();
    truncated.records.pop();
    let oracle_bytes = encode(&truncated);
    let result =
        run_and_compare(&scn, &progs, entities, || Ok(oracle_bytes.clone())).expect("runs");
    assert!(!result.agrees());
    assert!(result.divergence().is_some(), "a divergence is reported");
}

#[test]
fn run_and_compare_surfaces_a_bad_oracle_trace_and_a_refusal() {
    let progs = runnable_progs();
    let scn = scenario("start");
    let entities = b"{\n\"classname\" \"worldspawn\"\n}\n";
    // A trace that does not decode.
    let err = run_and_compare(&scn, &progs, entities, || Ok(b"not a trace".to_vec()))
        .expect_err("a bad oracle trace is an error");
    assert!(matches!(err, SuiteError::OracleTrace(_)), "{err:?}");
    // A progs QuakeC Studio refuses (empty bytes) ends before any trace (DIFF-3.1).
    let refused = run_and_compare(&scn, b"", entities, || Ok(Vec::new()))
        .expect_err("a refused progs is an error");
    assert!(matches!(refused, SuiteError::Refused(_)), "{refused:?}");
}

// spec DIFF-11.2: a codebase's coverage is distinct executed statements over numstatements - 1.
// spec DIFF-9.6: zero divergences is the differential bar of P1-13 (ADR-0013); coverage is
// reported, not gated.
#[test]
fn a_codebase_report_aggregates_coverage_and_divergences() {
    // A progs with numstatements 11: statements 1 to 10 (DIFF-11.1).
    let mut report = CodebaseReport::new("demo", 11);
    assert_eq!(report.total_statements, 10);

    // Scenario one executes statements 1 to 5, agreeing.
    let trace_a = Trace {
        header: header(),
        records: (1..=5).map(|s| step(s, 10)).collect(),
    };
    report.record(&ScenarioResult {
        trace: trace_a.clone(),
        comparison: Comparison::Equal,
        records_before_divergence: trace_a.records.len(),
    });
    // Scenario two executes 4 to 9, also agreeing.
    let trace_b = Trace {
        header: header(),
        records: (4..=9).map(|s| step(s, 10)).collect(),
    };
    report.record(&ScenarioResult {
        trace: trace_b.clone(),
        comparison: Comparison::Equal,
        records_before_divergence: trace_b.records.len(),
    });

    // Distinct statements 1..=9 of 10: 0.9 coverage, zero divergences, two scenarios. Coverage
    // is reported; the bar is zero divergences (ADR-0013), so the codebase passes.
    assert_eq!(report.scenarios, 2);
    assert_eq!(report.divergences, 0);
    assert_eq!(report.executed.len(), 9);
    assert!((report.coverage() - 0.9).abs() < f64::EPSILON);
    assert!(
        report.passes(),
        "zero divergences over scenarios meets the bar"
    );

    // A diverging scenario raises the divergence count and fails the codebase.
    report.record(&ScenarioResult {
        trace: Trace {
            header: header(),
            records: vec![step(10, 10)],
        },
        comparison: Comparison::Failure(Divergence::Length {
            index: 0,
            shorter_is_a: false,
        }),
        records_before_divergence: 0,
    });
    assert_eq!(report.divergences, 1);
    assert!(!report.passes(), "a divergence fails the differential bar");
}

// spec DIFF-9.6: a report over no scenarios does not pass the differential bar (ADR-0013);
// spec DIFF-11.2: a progs with no statements has coverage 0, not a divide by zero.
#[test]
fn a_report_with_no_scenarios_does_not_pass_and_no_statements_is_zero_coverage() {
    let report = CodebaseReport::new("empty", 11);
    assert!(!report.passes(), "no scenarios ran");
    assert_eq!(report.coverage(), 0.0);

    // A progs with no statements: coverage is 0 (no divide by zero), and one agreeing scenario
    // meets the bar, because the bar is zero divergences, not a coverage figure (ADR-0013).
    let mut no_statements = CodebaseReport::new("tiny", 1);
    no_statements.record(&ScenarioResult {
        trace: Trace {
            header: header(),
            records: Vec::new(),
        },
        comparison: Comparison::Equal,
        records_before_divergence: 0,
    });
    assert_eq!(no_statements.coverage(), 0.0);
    assert!(no_statements.passes(), "zero divergences meets the bar");
}
