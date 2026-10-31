// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of the compiler differential's scripted calls (DIFF-12), on programs `qc-codegen`
//! compiles from a `defs.qc` written from the system globals and fields of EDICT-2 and
//! EDICT-3.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap, index or panic is a failed test"
)]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use qc_codegen::{Options, compile_module};
use qc_edict::system::{SYSTEM_FIELDS, SYSTEM_GLOBALS};
use qc_progs::Progs;
use qc_sema::{Db, FileId, ModuleInput};
use qc_types::Etype;
use qc_vm::ServerState;

use super::draw::{Draw, Drawer, Pools, Rng};
use super::{
    COVERAGE_BAR, CallsError, CodebaseReport, Difference, Oracle, Report, Value, World, coverage,
    run_calls, run_codebase,
};
use crate::runner::scenarios;
use crate::scenario::{self, Scenario};

/// The type `defs.qc` declares a system variable with.
fn type_name(etype: Etype) -> &'static str {
    match etype {
        Etype::String => "string",
        Etype::Float => "float",
        Etype::Vector => "vector",
        Etype::Entity => "entity",
        Etype::Function => "void()",
        other => panic!("no system variable has type {other:?}"),
    }
}

/// The system part of a `defs.qc`: the globals of EDICT-2.1 and the fields of EDICT-3.1, in
/// slot order, each section closed as id's `defs.qc` closes it.
fn system_defs() -> String {
    let mut text = String::new();
    for var in SYSTEM_GLOBALS {
        writeln!(text, "{} {};", type_name(var.etype), var.name).unwrap();
    }
    text.push_str("void end_sys_globals;\n");
    for var in SYSTEM_FIELDS {
        writeln!(text, ".{} {};", type_name(var.etype), var.name).unwrap();
    }
    text.push_str("void end_sys_fields;\n");
    text.push_str(
        "void(string e) error = #10;\nvoid(entity e) remove = #15;\n\
         void(string s) dprint = #25;\nvoid(float style, string value) lightstyle = #35;\n",
    );
    text
}

/// The bodies of the engine's entry points (EDICT-9.1), so the setup runs.
const ENTRY_POINTS: &str = "void() main = {};\nvoid() StartFrame = {};\n\
    void() info_notnull = {};\n\
    void() PlayerPreThink = {};\nvoid() PlayerPostThink = {};\nvoid() ClientKill = {};\n\
    void() ClientConnect = {};\nvoid() PutClientInServer = {};\n\
    void() ClientDisconnect = {};\nvoid() SetNewParms = {};\nvoid() SetChangeParms = {};\n";

/// Globals after the program's own. `qcc` passes every parameter with `STORE_V`
/// (QC-22), which reads three slots, so a string or float immediate passed last in a program
/// with nothing after it would read past the globals (VM-1.4).
const PADDING: &str = "vector padding;\n";

/// The entry points [`ENTRY_POINTS`] defines that `source` does not.
fn entry_points(source: &str) -> String {
    ENTRY_POINTS
        .lines()
        .filter(|line| {
            let head = line.split('=').next().unwrap_or_default();
            !source.contains(head.trim_end())
        })
        .map(|line| format!("{line}\n"))
        .collect()
}

/// The progs `qc-codegen` writes for `source` after the system defs and the entry points.
fn compile(source: &str, fold: bool) -> Vec<u8> {
    let files = [
        ("defs.qc", system_defs()),
        ("entry.qc", entry_points(source)),
        ("t.qc", format!("{source}{PADDING}")),
    ];
    let mut db = Db::new();
    let mut input = ModuleInput::default();
    for (i, (name, text)) in files.iter().enumerate() {
        db.set_file(
            FileId::new(i),
            PathBuf::from(name),
            text.as_bytes().to_vec(),
        );
        input.compiled.push(FileId::new(i));
    }
    let names: Vec<String> = files.iter().map(|(n, _)| (*n).to_owned()).collect();
    let compiled = compile_module(
        &mut db,
        &input,
        |id| names[id.index()].clone(),
        Options { fold },
    )
    .unwrap_or_else(|e| panic!("{source:?} did not compile: {e:?}"));
    qc_progs::write(&compiled.progs).unwrap()
}

/// One client in the world of [`WORLD`].
fn scenario() -> Scenario {
    scenario::read(b"progs progs.dat\nclients 1\n", Path::new(".")).unwrap()
}

/// The world and one `info_notnull`, so three edicts are in use with the client.
const WORLD: &[u8] = b"{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"info_notnull\"\n}\n";

fn run(theirs: &[u8], ours: &[u8], rounds: u32) -> Report {
    run_calls(Oracle::Qcc, theirs, ours, &scenario(), WORLD, rounds).unwrap()
}

/// The first divergence of a run of the two sources, with the call's function.
fn first(theirs: &str, ours: &str) -> (Option<String>, Difference) {
    let report = run(&compile(theirs, false), &compile(ours, false), 16);
    let divergence = report
        .divergences
        .first()
        .unwrap_or_else(|| panic!("no divergence: {report:?}"))
        .clone();
    (
        divergence.function.map(|f| String::from_utf8(f).unwrap()),
        divergence.what,
    )
}

const FOLDED: &str = "float x;\nvector v;\n\
    void() worldspawn = { x = 1 + 2; };\n\
    void() f = { x = 2 * 3 + time; v = '1 2 3' * 2; };\n\
    void(float a, entity e, string s) g = { if (a > 1 + 1) x = a; else x = 0 - a; \
        if (e) x = x + 1; if (s == \"player\") x = x * 2; };\n";

// spec DIFF-12.17, DIFF-12.3: a program compiled with `--opt fold` off and on has other
// statements and globals, yet every call agrees by name and type.
#[test]
fn a_program_folded_and_not_agrees() {
    let (plain, folded) = (compile(FOLDED, false), compile(FOLDED, true));
    let (a, b) = (Progs::read(&plain).unwrap(), Progs::read(&folded).unwrap());
    assert!(a.statements().len() > b.statements().len());
    assert_ne!(a.globals().len(), b.globals().len());
    let report = run(&plain, &folded, 8);
    assert_eq!(report.divergences, []);
    assert!(report.calls > 0);
}

// spec DIFF-12.3: a named global that ends with another value is a divergence.
#[test]
fn a_global_that_differs_is_a_divergence() {
    let (function, what) = first(
        "float x;\nvoid() f = { x = 1; };\n",
        "float x;\nvoid() f = { x = 2; };\n",
    );
    assert_eq!(function.as_deref(), Some("f"));
    assert_eq!(
        what,
        Difference::Global(
            b"x".to_vec(),
            Value::Bits(1.0_f32.to_bits()),
            Value::Bits(2.0_f32.to_bits())
        )
    );
}

// spec DIFF-12.3: a field of an edict that ends with another value is a divergence, named by
// the edict and the field.
#[test]
fn a_field_that_differs_is_a_divergence() {
    let (_, what) = first(
        "void() f = { self.health = 1; };\n",
        "void() f = { self.health = 3; };\n",
    );
    let (one, three) = (
        Value::Bits(1.0_f32.to_bits()),
        Value::Bits(3.0_f32.to_bits()),
    );
    assert!(
        matches!(&what, Difference::Field(n, name, a, b)
            if *n > 0 && name == b"health" && *a == one && *b == three),
        "{what:?}"
    );
}

// spec DIFF-12.3: a string compares by its text, so two programs whose string tables differ
// agree when the texts do.
#[test]
fn strings_compare_by_text() {
    let report = run(
        &compile("string s;\nvoid() f = { s = \"a\"; };\n", false),
        &compile(
            "string s;\nvoid() f = { s = \"zzz\"; s = \"a\"; };\n",
            false,
        ),
        4,
    );
    assert_eq!(report.divergences, []);
    let (_, what) = first(
        "string s;\nvoid() f = { s = \"a\"; };\n",
        "string s;\nvoid() f = { s = \"b\"; };\n",
    );
    assert_eq!(
        what,
        Difference::Global(
            b"s".to_vec(),
            Value::Text(Some(b"a".to_vec())),
            Value::Text(Some(b"b".to_vec()))
        )
    );
}

// spec DIFF-12.3: a function value compares by the function's name.
#[test]
fn functions_compare_by_name() {
    let (_, what) = first(
        "void() a = {};\nvoid() b = {};\n.void() th;\nvoid() f = { self.th = a; };\n",
        "void() a = {};\nvoid() b = {};\n.void() th;\nvoid() f = { self.th = b; };\n",
    );
    let (a, b) = (
        Value::Function(Some(b"a".to_vec())),
        Value::Function(Some(b"b".to_vec())),
    );
    assert!(
        matches!(&what, Difference::Field(_, name, x, y) if name == b"th" && *x == a && *y == b),
        "{what:?}"
    );
}

// spec DIFF-12.3: a vector compares by its three slots.
#[test]
fn vectors_compare_by_their_slots() {
    let (_, what) = first(
        "vector v;\nvoid() f = { v = '1 2 3'; };\n",
        "vector v;\nvoid() f = { v = '1 2 4'; };\n",
    );
    let bits = |z: f32| [1.0_f32.to_bits(), 2.0_f32.to_bits(), z.to_bits()];
    assert!(
        matches!(&what, Difference::Global(name, Value::Vector(a), Value::Vector(b))
            if name == b"v" && *a == bits(3.0) && *b == bits(4.0)),
        "{what:?}"
    );
}

// spec DIFF-12.3: a call that ends in an error where the other returns is a divergence.
#[test]
fn an_ending_that_differs_is_a_divergence() {
    let (_, what) = first(
        "void() f = { error(\"no\"); };\n",
        "void() f = { dprint(\"no\"); };\n",
    );
    assert!(
        matches!(&what, Difference::Ending(Some(_), None)),
        "{what:?}"
    );
}

// spec DIFF-12.3: the builtins called are compared in order.
#[test]
fn builtins_that_differ_are_a_divergence() {
    let (_, what) = first("void() f = { dprint(\"a\"); };\n", "void() f = {};\n");
    assert_eq!(what, Difference::Builtins(vec![25], vec![]));
}

// spec DIFF-12.3: the mock engine's state is compared.
#[test]
fn an_engine_state_that_differs_is_a_divergence() {
    let (_, what) = first(
        "void() f = { lightstyle(0, \"a\"); };\n",
        "void() f = { lightstyle(0, \"b\"); };\n",
    );
    assert_eq!(what, Difference::Engine);
}

// spec DIFF-12.3: an edict free after one call and in use after the other is a divergence.
#[test]
fn an_edict_freed_in_one_is_a_divergence() {
    let (_, what) = first(
        "void() f = { remove(other); };\n",
        "void() f = { remove(self); };\n",
    );
    assert!(matches!(what, Difference::Free(_)), "{what:?}");
}

// spec DIFF-12.3: two programs that name other globals cannot be compared by name.
#[test]
fn other_definitions_are_a_divergence() {
    let (_, what) = first(
        "float x;\nvoid() f = { x = 1; };\n",
        "float x;\nfloat y;\nvoid() f = { x = 1; y = 1; };\n",
    );
    assert_eq!(what, Difference::Definitions);
}

// spec DIFF-12.3: a function the second program lacks is a global it lacks, found when the
// worlds are compared after the setup.
#[test]
fn a_missing_function_is_found_at_the_setup() {
    let report = run(
        &compile("void() f = {};\nvoid() h = {};\n", false),
        &compile("void() f = {};\n", false),
        2,
    );
    let found: Vec<_> = report
        .divergences
        .iter()
        .map(|d| (d.function.clone(), d.what.clone()))
        .collect();
    assert_eq!(found, [(None, Difference::Definitions)]);
}

// spec DIFF-12.4: with the same statements, the calls must execute the same ones, even when
// they end in the same state.
#[test]
fn the_same_statements_must_run_the_same_way() {
    let source = |k: u8| format!("float x;\nvoid() f = {{ if ({k} > 5.5) x = 1; else x = 1; }};\n");
    let (theirs, ours) = (compile(&source(5), false), compile(&source(6), false));
    assert!(
        Progs::read(&theirs)
            .unwrap()
            .statements()
            .iter()
            .eq(Progs::read(&ours).unwrap().statements().iter())
    );
    let report = run(&theirs, &ours, 1);
    let functions: Vec<_> = report
        .divergences
        .iter()
        .map(|d| (d.function.clone(), d.what.clone()))
        .collect();
    // DIFF-12.16: a branch no round took gives `f` another batch, which diverges the same way.
    assert!(!functions.is_empty());
    assert!(
        functions
            .iter()
            .all(|f| *f == (Some(b"f".to_vec()), Difference::Statements))
    );
}

// spec DIFF-12.17: the two worlds are compared after the setup, before any call.
#[test]
fn a_setup_that_differs_is_a_divergence_and_stops_the_run() {
    let report = run(
        &compile("float x;\nvoid() worldspawn = { x = 1; };\n", false),
        &compile("float x;\nvoid() worldspawn = { x = 2; };\n", false),
        4,
    );
    assert_eq!(report.calls, 0);
    let [divergence] = report.divergences.as_slice() else {
        panic!("{report:?}");
    };
    assert_eq!(divergence.function, None);
    assert!(matches!(divergence.what, Difference::Global(..)));
}

// spec DIFF-12.17: the setup runs the scenario's frames, so a frame's work is in the world the
// two progs' setups are compared by, and a frame that fails is a failed setup.
#[test]
fn the_setup_runs_the_scenarios_frames() {
    let framed = scenario::read(b"progs progs.dat\nclients 1\nframes 3\n", Path::new(".")).unwrap();
    let frames = |source: &str| compile(source, false);
    let counting = frames("float n;\nvoid() PlayerPreThink = { n = n + 1; };\n");
    let report = run_calls(Oracle::Qcc, &counting, &counting, &framed, WORLD, 1).unwrap();
    assert!(report.divergences.is_empty(), "{report:?}");
    let other = frames("float n;\nvoid() PlayerPreThink = { n = n + 2; };\n");
    let report = run_calls(Oracle::Qcc, &counting, &other, &framed, WORLD, 1).unwrap();
    assert_eq!(report.calls, 0);
    let [divergence] = report.divergences.as_slice() else {
        panic!("{report:?}");
    };
    assert_eq!(divergence.function, None);
    assert!(
        matches!(divergence.what, Difference::Global(..)),
        "{divergence:?}"
    );
    // With no frames the two never differ: a client's `PlayerPreThink` runs only in
    // a scenario frame, after the clients enter; the settle frames come before.
    let none = scenario::read(b"progs progs.dat\nclients 1\nframes 0\n", Path::new(".")).unwrap();
    let report = run_calls(Oracle::Qcc, &counting, &other, &none, WORLD, 1).unwrap();
    assert!(
        report.divergences.iter().all(|d| d.function.is_some()),
        "{report:?}"
    );
    let failing = frames("void() PlayerPreThink = { error(\"x\"); };\n");
    let setup = run_calls(Oracle::Qcc, &counting, &failing, &framed, WORLD, 1);
    assert!(matches!(setup, Err(CallsError::Setup(_))), "{setup:?}");
}

// spec DIFF-12.17: a progs that is refused, or a setup that fails, leaves nothing to call.
#[test]
fn a_refused_progs_or_a_failed_setup_is_an_error() {
    let good = compile("void() f = {};\n", false);
    let refused = run_calls(Oracle::Qcc, b"not a progs", &good, &scenario(), WORLD, 1);
    assert!(
        matches!(refused, Err(CallsError::Refused(_))),
        "{refused:?}"
    );
    let refused = run_calls(Oracle::Qcc, &good, b"", &scenario(), WORLD, 1);
    assert!(
        matches!(refused, Err(CallsError::Refused(_))),
        "{refused:?}"
    );
    let failing = compile("void() worldspawn = { error(\"x\"); };\n", false);
    let setup = run_calls(Oracle::Qcc, &failing, &good, &scenario(), WORLD, 1);
    assert!(matches!(setup, Err(CallsError::Setup(_))), "{setup:?}");
    let entering = compile("void() PutClientInServer = { error(\"x\"); };\n", false);
    let setup = run_calls(Oracle::Qcc, &good, &entering, &scenario(), WORLD, 1);
    assert!(matches!(setup, Err(CallsError::Setup(_))), "{setup:?}");
    for error in [
        CallsError::Scenario("s".to_owned()),
        CallsError::Setup("s".to_owned()),
    ] {
        assert!(error.to_string().contains('s'));
    }
}

// spec DIFF-12.5: coverage counts the first progs' distinct statements, the setup's
// included, over `numstatements - 1`.
#[test]
fn coverage_counts_the_first_progs_statements() {
    let source = "float x;\nvoid() worldspawn = { x = 1; };\nvoid(float a) f = { if (a) x = 2; else x = 3; };\n";
    let progs = compile(source, false);
    let report = run(&progs, &progs, 8);
    let total = Progs::read(&progs).unwrap().statements().len() - 1;
    assert_eq!(usize::try_from(report.total_statements).unwrap(), total);
    assert!(
        report
            .executed
            .iter()
            .all(|&s| s >= 1 && usize::try_from(s).unwrap() <= total)
    );
    assert!(
        (report.coverage() - coverage(&report.executed, report.total_statements)).abs() < 1e-12
    );
    // Every statement but the entry points' and worldspawn's DONE runs in some round.
    assert!(report.coverage() > 0.9, "{report:?}");
    assert!(coverage(&BTreeSet::new(), 0).abs() < f64::EPSILON);
    assert!((coverage(&BTreeSet::from([1, 2]), 4) - 0.5).abs() < f64::EPSILON);
}

// spec DIFF-12.5: a codebase passes with no divergence and at least 90% of its statements
// executed over the scenarios together.
#[test]
fn a_codebase_passes_at_the_bar() {
    let report = |executed: BTreeSet<i32>, divergences| {
        let mut scenario = Report {
            calls: 1,
            divergences: Vec::new(),
            executed: executed.clone(),
            total_statements: 10,
        };
        if divergences {
            scenario.divergences.push(super::Divergence {
                function: None,
                round: 0,
                what: Difference::Engine,
            });
        }
        CodebaseReport {
            scenarios: vec![("s", scenario)],
            executed,
            total_statements: 10,
        }
    };
    let nine: BTreeSet<i32> = (1..=9).collect();
    let eight: BTreeSet<i32> = (1..=8).collect();
    assert!((COVERAGE_BAR - 0.9).abs() < f64::EPSILON);
    assert!(report(nine.clone(), false).passes());
    assert!(!report(eight, false).passes());
    assert!(!report(nine.clone(), true).passes());
    assert_eq!(report(nine, true).divergences(), 1);
}

// spec DIFF-12.14: each pair is a row of the results table: the codebase, the oracle's progs and
// ours, the divergences, the statements executed, and whether it passes.
#[test]
fn a_report_is_a_row_of_the_results_table() {
    let scenario = Report {
        calls: 3,
        divergences: Vec::new(),
        executed: (1..=9).collect(),
        total_statements: 10,
    };
    let mut report = CodebaseReport {
        scenarios: vec![("s", scenario)],
        executed: (1..=9).collect(),
        total_statements: 10,
    };
    assert_eq!(
        report.summary_row("rerelease", "oracle.dat", "ours.dat"),
        "| rerelease | oracle.dat | ours.dat | 0 | 9 of 10 (90.0%) | pass |\n"
    );
    report.executed = (1..=8).collect();
    assert_eq!(
        report.summary_row("id-1.01", "oracle.dat", "ours-fold.dat"),
        "| id-1.01 | oracle.dat | ours-fold.dat | 0 | 8 of 10 (80.0%) | fail |\n"
    );
}

// spec DIFF-12.17, DIFF-12.5: a codebase runs every world of the differential scenarios and
// unions their statements.
#[test]
fn a_codebase_runs_every_scenario() {
    let progs = compile(FOLDED, false);
    let folded = compile(FOLDED, true);
    let report = run_codebase(Oracle::Qcc, &progs, &folded, 2).unwrap();
    let names: Vec<_> = report.scenarios.iter().map(|(n, _)| *n).collect();
    let expected: Vec<_> = scenarios("").iter().map(|s| s.name).collect();
    assert_eq!(names, expected);
    assert_eq!(report.divergences(), 0);
    let union: BTreeSet<i32> = report
        .scenarios
        .iter()
        .flat_map(|(_, r)| r.executed.iter().copied())
        .collect();
    assert_eq!(report.executed, union);
    assert!(run_codebase(Oracle::Qcc, b"", &folded, 1).is_err());
}

// spec DIFF-12.2: the generator is seeded by the function's name and the round, so a context
// is the same on every run and differs between rounds.
#[test]
fn the_generator_is_deterministic() {
    let draws = |name: &[u8], round| {
        let mut rng = Rng::new(name, round);
        (0..16).map(|_| rng.next()).collect::<Vec<_>>()
    };
    assert_eq!(draws(b"f", 1), draws(b"f", 1));
    assert_ne!(draws(b"f", 1), draws(b"f", 2));
    assert_ne!(draws(b"f", 1), draws(b"g", 1));
    let mut rng = Rng::new(b"f", 0);
    assert_eq!(rng.below(0), 0);
    assert!((0..100).all(|_| rng.below(7) < 7));
    assert_eq!(rng.pick::<u8>(&[]), None);
    assert_eq!(rng.pick(&[5]), Some(&5));
}

// spec DIFF-12.2: the float pool holds whole numbers from -1000 to 2^24, flag bits and masks,
// and fractions, every one exact in an f32.
#[test]
fn the_float_pool_is_what_it_says() {
    let mut rng = Rng::new(b"pool", 0);
    let values: Vec<f32> = (0..4000).map(|_| rng.float()).collect();
    assert!(
        values
            .iter()
            .all(|v| v.is_finite() && *v > -1000.0 && *v < 16_777_216.0)
    );
    assert!(values.iter().any(|v| v.fract() != 0.0));
    assert!(values.iter().any(|v| *v < 0.0));
    assert!(values.iter().any(|v| *v >= 65536.0));
    assert!(
        values
            .iter()
            .any(|v| (-1.0..=10.0).contains(v) && v.fract() == 0.0)
    );
}

/// The drawer over the first world of [`FOLDED`], with a function's record by name.
fn with_drawer(test: impl FnOnce(&Drawer<'_, '_>, &Progs<'_>)) {
    let bytes = compile(FOLDED, false);
    let progs = Progs::read(&bytes).unwrap();
    let scenario = scenario();
    let world = World::new(&progs, &scenario, WORLD).unwrap();
    let drawer = Drawer::new(&world);
    test(&drawer, &progs);
}

fn function(progs: &Progs<'_>, name: &[u8]) -> qc_progs::Function {
    progs.ed_find_function(name).unwrap()
}

// spec DIFF-12.2: a function's own constants are the floats and strings its statements name.
#[test]
fn a_functions_constants_are_its_operands() {
    with_drawer(|drawer, progs| {
        let Pools { numbers, texts } = drawer.pools(function(progs, b"g"));
        let numbers: Vec<f32> = numbers.iter().map(|b| f32::from_bits(*b)).collect();
        assert!(numbers.contains(&1.0), "{numbers:?}");
        assert_eq!(texts, [b"player".to_vec()]);
        assert_eq!(drawer.pools(function(progs, b"main")), Pools::default());
    });
}

// spec DIFF-12.2: each parameter is drawn by its type, round 0 of four draws no field or
// variable, later rounds draw some, and every eighth round runs with the server loading.
#[test]
fn contexts_follow_the_rounds() {
    with_drawer(|drawer, progs| {
        let g = function(progs, b"g");
        let pools = drawer.pools(g);
        let draw = |round| drawer.draw(&mut Rng::new(b"g", round), (g, &pools), round);
        let first = draw(0);
        assert!(first.fields.is_empty() && first.globals.is_empty());
        assert!(matches!(
            first.parms.as_slice(),
            [Draw::Float(_), Draw::Entity(_), Draw::Text(_)]
        ));
        assert_eq!(first.server, ServerState::Active);
        assert_eq!(draw(0), first);
        assert!((1..4).any(|round| !draw(round).fields.is_empty()));
        assert_eq!(draw(7).server, ServerState::Loading);
        assert_eq!(draw(15).server, ServerState::Loading);
        assert_eq!(draw(6).server, ServerState::Active);
        let f = function(progs, b"f");
        let pools = drawer.pools(f);
        assert!(
            drawer
                .draw(&mut Rng::new(b"f", 3), (f, &pools), 3)
                .parms
                .is_empty()
        );
    });
}

// spec DIFF-12.7: when the tables differ, a call that returns another value is a divergence,
// though it leaves every named global and field the same.
#[test]
fn a_return_value_that_differs_is_a_divergence() {
    let (function, what) = first(
        "float() f = { return 3; };\n",
        "float() f = { return 4; };\n",
    );
    assert_eq!(function.as_deref(), Some("f"));
    assert_eq!(
        what,
        Difference::Return(
            Value::Bits(3.0_f32.to_bits()),
            Value::Bits(4.0_f32.to_bits())
        )
    );
}

// spec DIFF-12.7: a return value agrees when any reading of its first word does: a string by
// its text, though the string tables differ.
#[test]
fn a_return_value_agrees_by_any_reading() {
    let report = run(
        &compile("string() f = { return \"a\"; };\n", false),
        &compile(
            "string() f = { return \"a\"; };\nvoid() g = { dprint(\"b\"); };\n",
            false,
        ),
        2,
    );
    assert!(
        report
            .divergences
            .iter()
            .all(|d| d.function.as_deref() != Some(b"f".as_slice())),
        "{report:?}"
    );
}

// spec DIFF-12.7: a QuakeC function the second progs has only as a builtin is a divergence.
#[test]
fn a_function_that_is_a_builtin_in_the_second_is_missing() {
    let report = run(
        &compile("void() h = {};\n", false),
        &compile("void() h = #99;\n", false),
        1,
    );
    assert!(
        report.divergences.iter().any(
            |d| d.function.as_deref() == Some(b"h".as_slice()) && d.what == Difference::Missing
        ),
        "{report:?}"
    );
}

// spec DIFF-12.17: the setups are compared as two calls are, the builtins they call in order
// before the engine's state.
#[test]
fn a_setup_that_calls_other_builtins_is_a_divergence() {
    let report = run(
        &compile(
            "void() worldspawn = { dprint(\"a\"); dprint(\"b\"); };\n",
            false,
        ),
        &compile("void() worldspawn = { dprint(\"a\"); };\n", false),
        1,
    );
    let [divergence] = report.divergences.as_slice() else {
        panic!("{report:?}");
    };
    assert_eq!(divergence.function, None);
    assert!(
        matches!(divergence.what, Difference::Builtins(..)),
        "{divergence:?}"
    );
}

// spec DIFF-12.3: an entity reads as its edict's number and a field as its name; a value
// that names neither reads as its bits.
#[test]
fn entities_and_fields_read_by_edict_and_name() {
    let progs = compile("entity e;\n.float f;\n", false);
    let progs = Progs::read(&progs).unwrap();
    let scenario = scenario();
    let mut world = World::new(&progs, &scenario, WORLD).unwrap();
    let size = world.host.machine.edicts().layout().edict_size();
    let entity =
        |world: &World<'_, '_, '_>| world.global(qc_types::DefType::Known(Etype::Entity), 28);
    world
        .host
        .set_global_slot(28, qc_types::Slot::from_i32(2 * size));
    assert_eq!(entity(&world), Value::Entity(2));
    world
        .host
        .set_global_slot(28, qc_types::Slot::from_i32(2 * size + 1));
    assert_eq!(
        entity(&world),
        Value::Bits(u32::try_from(2 * size + 1).unwrap())
    );
    let (_, (_, slot)) = world
        .program
        .fields
        .iter()
        .find(|(n, _)| n.as_slice() == b"f")
        .unwrap();
    let slot = i32::try_from(*slot).unwrap();
    world
        .host
        .set_global_slot(28, qc_types::Slot::from_i32(slot));
    assert_eq!(
        world.global(qc_types::DefType::Known(Etype::Field), 28),
        Value::FieldName(b"f".to_vec())
    );
    world.host.set_global_slot(28, qc_types::Slot::from_i32(-7));
    assert!(matches!(
        world.global(qc_types::DefType::Known(Etype::Field), 28),
        Value::Bits(_)
    ));
}

/// The divergences of a run of the two sources against the oracle `oracle`.
fn against(oracle: Oracle, theirs: &str, ours: &str) -> Vec<Difference> {
    let (theirs, ours) = (compile(theirs, false), compile(ours, false));
    run_calls(oracle, &theirs, &ours, &scenario(), WORLD, 4)
        .unwrap()
        .divergences
        .into_iter()
        .map(|d| d.what)
        .collect()
}

// spec DIFF-12.11: against FTEQCC, which names fewer globals, ours may name more, but each of
// FTEQCC's must be among ours in the same order.
#[test]
fn against_fteqcc_ours_may_name_more_globals() {
    let fewer = "float x;\nvoid() f = { x = 1; };\n";
    let more = "float x;\nfloat y;\nvoid() f = { x = 1; };\n";
    assert_eq!(against(Oracle::Fteqcc, fewer, more), []);
    assert_eq!(against(Oracle::Qcc, fewer, more), [Difference::Definitions]);
    assert_eq!(
        against(Oracle::Fteqcc, more, fewer),
        [Difference::Definitions]
    );
    let swapped = "float y;\nfloat x;\nvoid() f = { x = 1; };\n";
    assert_eq!(
        against(Oracle::Fteqcc, more, swapped),
        [Difference::Definitions]
    );
}

// spec DIFF-12.11: against FTEQCC a function's locals are not compared, so a global is paired
// with ours of its name outside every function's locals.
#[test]
fn against_fteqcc_a_local_is_not_paired_with_a_global() {
    let theirs = "void() g = { local float y; y = 2; };\nfloat x;\nvoid() f = { x = 1; };\n";
    let ours = "void() g = { local float x; x = 2; };\nfloat x;\nvoid() f = { x = 1; };\n";
    assert_eq!(against(Oracle::Fteqcc, theirs, ours), []);
}

// spec DIFF-12.11: against FTEQCC two calls that both run away agree, whatever each had done when
// it was stopped; against qcc they must leave the same state.
#[test]
fn against_fteqcc_two_runaway_calls_agree() {
    let theirs = "float x;\nvoid() f = { while (1) x = x + 1; };\n";
    let ours = "float x;\nvoid() f = { while (1) { x = x + 1; x = x + 0; } };\n";
    assert_eq!(against(Oracle::Fteqcc, theirs, ours), []);
    assert!(!against(Oracle::Qcc, theirs, ours).is_empty());
    let returns = "float x;\nvoid() f = { x = 1; };\n";
    assert!(matches!(
        against(Oracle::Fteqcc, theirs, returns).first(),
        Some(Difference::Ending(..))
    ));
}

// spec DIFF-12.11: against FTEQCC a function definition inside a body's locals, as a state's
// `[frame, next]` declares `next` there, is a global and is paired with FTEQCC's.
#[test]
fn against_fteqcc_a_states_next_function_is_a_global() {
    let theirs =
        "void() a;\nvoid() b;\nvoid() a = [1, b] { local float q; q = 1; };\nvoid() b = {};\n";
    let ours = "void() a = [1, b] { local float q; q = 1; };\nvoid() b = {};\n";
    assert_eq!(against(Oracle::Fteqcc, theirs, ours), []);
}

/// `bytes`, a version 6 progs, with global definition `name` given the type number `ty`.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "test code: offsets inside a small progs the test compiled"
)]
fn retyped(mut bytes: Vec<u8>, name: &str, ty: u16) -> Vec<u8> {
    let word = |b: &[u8], at: usize| {
        usize::try_from(i32::from_le_bytes(b[at..at + 4].try_into().unwrap())).unwrap()
    };
    let (defs, count, strings) = (word(&bytes, 16), word(&bytes, 20), word(&bytes, 40));
    for i in 0..count {
        let at = defs + i * 8;
        let s_name = strings + word(&bytes, at + 4);
        let end = s_name + bytes[s_name..].iter().position(|&b| b == 0).unwrap();
        if &bytes[s_name..end] == name.as_bytes() {
            bytes[at..at + 2].copy_from_slice(&ty.to_le_bytes());
        }
    }
    bytes
}

// spec DIFF-12.10: a global of FTEQCC's struct type is not compared, so two programs whose
// struct-typed global holds the same string at another place in each string table agree
#[test]
fn a_struct_typed_global_is_not_compared() {
    let theirs = compile("string s;\nvoid() f = { s = \"a\"; };\n", false);
    let ours = compile(
        "string s;\nvoid() g = { s = \"zz\"; };\nvoid() f = { s = \"a\"; };\n",
        false,
    );
    let run = |theirs: &[u8], ours: &[u8]| {
        run_calls(Oracle::Fteqcc, theirs, ours, &scenario(), WORLD, 4)
            .unwrap()
            .divergences
            .into_iter()
            .map(|d| d.what)
            .filter(|d| matches!(d, Difference::Global(..)))
            .count()
    };
    let float_typed = (
        retyped(theirs.clone(), "s", 2),
        retyped(ours.clone(), "s", 2),
    );
    assert_ne!(run(&float_typed.0, &float_typed.1), 0);
    let struct_typed = (retyped(theirs, "s", 14), retyped(ours, "s", 14));
    assert_eq!(run(&struct_typed.0, &struct_typed.1), 0);
}

/// `bytes` with each one-letter global name `from` written as `to`.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "test code: offsets inside a small progs the test compiled"
)]
fn renamed(mut bytes: Vec<u8>, from: u8, to: u8) -> Vec<u8> {
    let word = |b: &[u8], at: usize| {
        usize::try_from(i32::from_le_bytes(b[at..at + 4].try_into().unwrap())).unwrap()
    };
    let (defs, count, strings) = (word(&bytes, 16), word(&bytes, 20), word(&bytes, 40));
    for i in 0..count {
        let s_name = strings + word(&bytes, defs + i * 8 + 4);
        if bytes[s_name] == from && bytes[s_name + 1] == 0 {
            bytes[s_name] = to;
        }
    }
    bytes
}

// spec DIFF-12.11: a context writes a global by name to a float or string definition of the
// name, so a drawn float is not written to a field global of its name
#[test]
fn a_global_written_by_name_goes_to_a_scalar_definition() {
    let bytes = compile(".float z;\nfloat y;\nvoid() f = { y = 1; };\n", false);
    let bytes = renamed(bytes, b'y', b'z');
    let progs = Progs::read(&bytes).unwrap();
    let program = super::Program::new(&progs);
    let slot = program.global_slots[b"z".as_slice()];
    let float = program
        .globals
        .iter()
        .find(|(name, ty, _)| name == b"z" && *ty == qc_types::DefType::Known(Etype::Float))
        .unwrap();
    assert_eq!(slot, float.2);
}

// spec DIFF-12.12: a context clears the parameter slots past the parameters it draws, so a call
// reads none of what earlier calls left there
#[test]
fn a_context_clears_the_slots_past_its_parameters() {
    let progs = compile("float x;\n", false);
    let progs = Progs::read(&progs).unwrap();
    let scenario = scenario();
    let mut world = World::new(&progs, &scenario, WORLD).unwrap();
    for slot in 4..28 {
        world
            .host
            .set_global_slot(slot, qc_types::Slot::from_f32(7.0));
    }
    let context = super::draw::Context {
        self_edict: 0,
        other_edict: 0,
        fields: Vec::new(),
        globals: Vec::new(),
        parms: vec![Draw::Float(1.0)],
        server: ServerState::Active,
    };
    context.apply(&mut world.host, &world.program);
    let slot = |n: i32| world.host.machine.global(n).unwrap();
    assert_eq!(slot(4), qc_types::Slot::from_f32(1.0));
    assert!((7..28).all(|n| slot(n) == qc_types::Slot::ZERO));
}

// spec DIFF-12.13: the report lists each function with unexecuted statements, the most first,
// and leaves out a function the calls ran through
#[test]
fn uncovered_lists_each_functions_unexecuted_statements() {
    let bytes = compile(
        "float x;\nvoid() a = { x = 1; x = 2; x = 3; };\nvoid() b = { x = 4; };\n",
        false,
    );
    let progs = Progs::read(&bytes).unwrap();
    let start = |name: &str| {
        progs
            .functions()
            .iter()
            .find(|f| progs.string(f.s_name) == Some(name.as_bytes()))
            .unwrap()
            .first_statement
    };
    let (a, b) = (start("a"), start("b"));
    let executed: BTreeSet<i32> = [b, b + 1].into_iter().collect();
    let text = super::uncovered(&bytes, &executed).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines.contains(&format!("{}\t{}\ta", b - a, b - a).as_str()),
        "{text}"
    );
    assert!(!lines.iter().any(|l| l.ends_with("\tb")), "{text}");
}

// spec DIFF-12.15: each function is listed with the edicts whose function fields hold it, the
// edicts `self` is drawn from one time in two
#[test]
fn a_function_is_owned_by_the_edicts_that_hold_it() {
    let bytes = compile(
        "void() f = {};\nvoid() g = {};\nvoid() worldspawn = { self.think = f; };\n",
        false,
    );
    let progs = Progs::read(&bytes).unwrap();
    let scenario = scenario();
    let world = World::new(&progs, &scenario, WORLD).unwrap();
    let first = |name: &str| {
        progs
            .functions()
            .iter()
            .find(|f| progs.string(f.s_name) == Some(name.as_bytes()))
            .unwrap()
            .first_statement
    };
    let owners = super::draw::owners(&world, &progs);
    assert_eq!(owners.get(&first("f")), Some(&vec![0]));
    assert_eq!(owners.get(&first("g")), None);
}

// spec DIFF-12.16: a function with statements no call reached is called in one more batch, a
// quarter of its rounds, and no more once a batch reaches nothing new; a function its rounds
// cover is called no more
#[test]
fn a_partly_covered_function_gets_one_more_batch() {
    let calls = |source: &str| {
        let bytes = compile(source, false);
        run_calls(Oracle::Qcc, &bytes, &bytes, &scenario(), WORLD, 8)
            .unwrap()
            .calls
    };
    let covered = calls("float x;\nvoid() f = { x = 1; };\n");
    let partly = calls("float x;\nvoid() f = { if (0) x = 2; x = 1; };\n");
    assert_eq!(partly, covered + 2);
}
