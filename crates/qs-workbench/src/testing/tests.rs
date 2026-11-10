// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::PathBuf;

use qc_project::MemoryLoader;
use qs_commands::{Registry, Session};
use qs_sim::scenario::{Check, Report};
use serde_json::json;

use super::{
    RUN_TESTS, RunTestsParams, TestKind, TestResult, TestRun, TestRunError, Verdict,
    scenario_verdict,
};
use crate::vm_run::tests::system_defs;
use crate::{Config, Workbench, register};

/// What a test project holds; the default has passing, failing and erroring tests.
struct ProjectSpec {
    code: String,
    scenarios: Vec<(&'static str, &'static str)>,
}

impl Default for ProjectSpec {
    fn default() -> Self {
        Self {
            code: "\
float counter;\n\
void(string e) error = #10;\n\
void() helper = { counter = counter + 1; };\n\
void() test_adds = { helper(); if (counter != 1) error(\"counter is not 1\"); };\n\
void() test_expects_two = { if (counter != 2) error(\"expected 2\"); };\n\
void(float x) test_takes_a_parameter = {};\n\
void() test_null_entity = { local entity e; e.health = 1; };\n\
void() test_sees_a_fresh_vm = { if (counter != 0) error(\"counter leaked\"); };\n"
                .to_owned(),
            scenarios: Vec::new(),
        }
    }
}

/// A fresh directory for test `name` holding `spec`'s scenarios, and a workbench over its
/// project, open; its program is written there.
fn project(name: &str, spec: &ProjectSpec) -> (PathBuf, Workbench<MemoryLoader>) {
    let root = std::env::temp_dir().join(format!(
        "qs-workbench-testing-{name}-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).unwrap();
    }
    std::fs::create_dir_all(&root).unwrap();
    for (file, text) in &spec.scenarios {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let mut loader = MemoryLoader::new();
    loader.insert(root.join("progs.src"), b"progs.dat\ncode.qc\n".to_vec());
    loader.insert(
        root.join("code.qc"),
        (system_defs() + &spec.code).into_bytes(),
    );
    let mut workbench = Workbench::new(loader, Config::default());
    workbench.open_project(&root).unwrap();
    (root, workbench)
}

fn verdicts(run: &TestRun) -> Vec<(&str, Verdict)> {
    run.tests
        .iter()
        .map(|test| (test.name.as_str(), test.verdict))
        .collect()
}

// spec QCT-1.1, QCT-2.1, QCT-2.2, QCT-4.1: the test_ functions with no parameters run in the
// order they are defined, each in a fresh VM; one that returns passes, one that calls error
// or stops on a run error fails with the error's message
#[test]
fn unit_tests_are_found_and_judged() {
    let (_, mut workbench) = project("units", &ProjectSpec::default());
    let run = workbench.run_tests(&RunTestsParams::default()).unwrap();
    assert_eq!(
        verdicts(&run),
        [
            ("test_adds", Verdict::Passed),
            ("test_expects_two", Verdict::Failed),
            ("test_null_entity", Verdict::Failed),
            ("test_sees_a_fresh_vm", Verdict::Passed),
        ],
        "helper and test_takes_a_parameter are not tests"
    );
    assert!(run.tests.iter().all(|test| test.kind == TestKind::Unit));
    assert!(
        run.tests[1]
            .message
            .as_deref()
            .unwrap()
            .contains("expected 2"),
        "{:?}",
        run.tests[1].message
    );
    assert!(run.tests[2].message.is_some());
    assert_eq!(run.tests[0].message, None);
    assert!(!run.passed);
    assert_eq!(
        (run.passed_count, run.failed_count, run.skipped_count),
        (2, 2, 0)
    );
}

// spec QCT-1.3: a filter keeps the tests whose name contains it
#[test]
fn a_filter_keeps_some_tests() {
    let (_, mut workbench) = project("filter", &ProjectSpec::default());
    let params = RunTestsParams {
        filter: Some("fresh".to_owned()),
        ..RunTestsParams::default()
    };
    let run = workbench.run_tests(&params).unwrap();
    assert_eq!(verdicts(&run), [("test_sees_a_fresh_vm", Verdict::Passed)]);
    assert!(run.passed);
}

// spec QCT-4.1: a project with no tests passes; one where no module builds answers no tests
// but the build's problems
#[test]
fn no_tests_and_no_build() {
    let empty = ProjectSpec {
        code: "void() helper = {};\n".to_owned(),
        ..ProjectSpec::default()
    };
    let (_, mut workbench) = project("empty", &empty);
    let run = workbench.run_tests(&RunTestsParams::default()).unwrap();
    assert!(run.passed && run.tests.is_empty());
    let broken = ProjectSpec {
        code: "void() test_x = { nowhere = 1; };\n".to_owned(),
        ..ProjectSpec::default()
    };
    let (_, mut workbench) = project("broken", &broken);
    let error = workbench.run_tests(&RunTestsParams::default()).unwrap_err();
    assert!(matches!(error, TestRunError::NothingBuilt(_)));
    assert!(error.to_string().contains("nowhere"), "{error}");
    let mut nothing_open = Workbench::new(MemoryLoader::new(), Config::default());
    assert!(matches!(
        nothing_open.run_tests(&RunTestsParams::default()),
        Err(TestRunError::Build(_))
    ));
}

// spec QCT-1.2, QCT-3.3: the .scenario files under tests/, at any depth, follow the unit tests
// in name order; with no game, each is skipped, saying why, and the run still passes
#[test]
fn scenario_tests_are_found_and_skipped_without_a_game() {
    let spec = ProjectSpec {
        code: "void() test_one = {};\n".to_owned(),
        scenarios: vec![
            ("tests/start.scenario", "map start\nframes 1\n"),
            ("tests/e1/e1m1.scenario", "map e1m1\nframes 1\n"),
            ("tests/notes.txt", "not a scenario"),
            ("other.scenario", "map start\n"),
        ],
    };
    let (root, mut workbench) = project("scenarios", &spec);
    let no_game = root.join("no-game-here");
    std::fs::create_dir_all(&no_game).unwrap();
    let params = RunTestsParams {
        game: Some(no_game),
        ..RunTestsParams::default()
    };
    let run = workbench.run_tests(&params).unwrap();
    assert_eq!(
        verdicts(&run),
        [
            ("test_one", Verdict::Passed),
            ("tests/e1/e1m1.scenario", Verdict::Skipped),
            ("tests/start.scenario", Verdict::Skipped),
        ]
    );
    assert!(run.tests[1..].iter().all(|test| {
        test.kind == TestKind::Scenario && test.message.as_deref().is_some_and(|m| !m.is_empty())
    }));
    assert!(run.passed);
    assert_eq!(
        (run.passed_count, run.failed_count, run.skipped_count),
        (1, 0, 2)
    );
}

// spec QCT-3.1, QCT-3.2: with a game folder given, each scenario is played over its files with
// the project's built program; one whose map the game lacks, or whose text does not read,
// fails saying why
#[test]
fn scenario_tests_play_over_the_game_given() {
    let spec = ProjectSpec {
        code: String::new(),
        scenarios: vec![
            ("tests/missing_map.scenario", "map nomap\nframes 1\n"),
            ("tests/unreadable.scenario", "frobnicate\n"),
        ],
    };
    let (root, mut workbench) = project("scenarios-game", &spec);
    let game = root.join("game");
    std::fs::create_dir_all(game.join("id1")).unwrap();
    // The full game, so a mod may run over it (FS-4.2): id's pop table in gfx/pop.lmp.
    let pop: Vec<u8> = qs_vfs::FS_POP_TABLE
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect();
    let pak0 = qs_pak::write_pak(&[(qs_vfs::FS_POP_PATH, pop)]).unwrap();
    std::fs::write(game.join("id1/pak0.pak"), pak0).unwrap();
    let params = RunTestsParams {
        game: Some(game),
        ..RunTestsParams::default()
    };
    let run = workbench.run_tests(&params).unwrap();
    assert_eq!(
        verdicts(&run),
        [
            ("tests/missing_map.scenario", Verdict::Failed),
            ("tests/unreadable.scenario", Verdict::Failed),
        ]
    );
    let message = |n: usize| run.tests[n].message.clone().unwrap_or_default();
    assert!(
        message(0).contains("maps/nomap.bsp"),
        "the program was found and the map was looked for: {}",
        message(0)
    );
    assert!(message(1).starts_with("line 1"), "{}", message(1));
    assert!(!run.passed);
}

// spec QCT-3.2: a scenario passes when its report passed, and fails naming each failed
// assertion's line, text and value, and the frame's error
#[test]
fn a_scenario_report_is_a_verdict() {
    let check = |line, passed| Check {
        line,
        text: format!("global g{line} == 1"),
        passed,
        found: "0".to_owned(),
    };
    let passed = Report {
        passed: true,
        frames: 3,
        checks: vec![check(2, true)],
        screenshots: Vec::new(),
        error: None,
    };
    assert_eq!(scenario_verdict(&passed), (Verdict::Passed, None));
    let failed = Report {
        passed: false,
        checks: vec![check(2, true), check(4, false)],
        error: Some("runaway loop".to_owned()),
        ..passed
    };
    assert_eq!(
        scenario_verdict(&failed),
        (
            Verdict::Failed,
            Some("line 4: expect global g4 == 1: found 0\nframe 3: runaway loop".to_owned())
        )
    );
}

// spec QCT-4.2: project.test through the registry, as the palette and the agent run it,
// answers what the workbench answers, outside the undo history
#[test]
fn the_command_answers_the_same() {
    let (_, mut direct) = project("command-direct", &ProjectSpec::default());
    let expected = direct.run_tests(&RunTestsParams::default()).unwrap();
    let (_, workbench) = project("command", &ProjectSpec::default());
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(workbench, registry);
    let answered: TestRun =
        serde_json::from_value(session.execute(RUN_TESTS, json!({})).unwrap()).unwrap();
    assert_eq!(answered.tests.len(), expected.tests.len());
    for (got, want) in answered.tests.iter().zip(&expected.tests) {
        assert_eq!(
            (&got.name, got.verdict, got.kind),
            (&want.name, want.verdict, want.kind)
        );
    }
    assert_eq!(session.undo_depth(), 0);
    let filtered: TestRun = serde_json::from_value(
        session
            .execute(RUN_TESTS, json!({ "filter": "adds" }))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        filtered.tests,
        [TestResult {
            kind: TestKind::Unit,
            name: "test_adds".to_owned(),
            verdict: Verdict::Passed,
            message: None,
        }]
    );
    assert!(session.execute(RUN_TESTS, json!({ "x": 1 })).is_err());
}
