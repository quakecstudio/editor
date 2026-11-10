// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "test code: a failed unwrap, index or match is a failed test"
)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use qc_edict::system::{SYSTEM_GLOBALS, defs_source};
use qc_project::FsLoader;
use qc_types::Etype;
use qs_commands::{Registry, Session};
use qs_workbench::{Config, Workbench, register};
use serde_json::{Value, json};

use super::{AgentError, Limits, Message, Model, Outcome, Reply, ToolCall, ToolSpec, run, tools};

/// A model that answers with its script, in order, and then fails.
struct Scripted {
    replies: VecDeque<Reply>,
    seen: Vec<usize>,
}

impl Scripted {
    fn new(replies: Vec<Reply>) -> Self {
        Self {
            replies: replies.into(),
            seen: Vec::new(),
        }
    }
}

impl Model for Scripted {
    fn reply(&mut self, tools: &[ToolSpec], transcript: &[Message]) -> Result<Reply, String> {
        assert!(tools.iter().any(|t| t.name == "compile"));
        self.seen.push(transcript.len());
        self.replies
            .pop_front()
            .ok_or_else(|| "out of script".to_owned())
    }
}

fn finish() -> Reply {
    Reply::Finish {
        summary: "done".to_owned(),
    }
}

fn calls(calls: Vec<(&str, Value)>) -> Reply {
    Reply::Calls {
        text: String::new(),
        calls: calls
            .into_iter()
            .enumerate()
            .map(|(n, (name, arguments))| ToolCall {
                id: format!("c{n}"),
                name: name.to_owned(),
                arguments,
            })
            .collect(),
    }
}

/// The defs a program the VM takes needs, and a body for each function the engine calls.
fn defs() -> String {
    let mut text = defs_source();
    for var in SYSTEM_GLOBALS
        .iter()
        .filter(|var| var.etype == Etype::Function)
    {
        text.push_str(&format!("void() {} = {{}};\n", var.name));
    }
    text
}

const FAILING: &str = "void(string e) error = #10;\n\
    float() damage = { return 4; };\n\
    void() test_damage = { if (damage() != 5) error(\"damage is not 5\"); };\n";

const FIXED: &str = "void(string e) error = #10;\n\
    float() damage = { return 5; };\n\
    void() test_damage = { if (damage() != 5) error(\"damage is not 5\"); };\n";

/// A project on disk for test `name` whose `weapons.qc` is `weapons`, and its open session.
fn project(name: &str, weapons: &str) -> (PathBuf, Session<Workbench<FsLoader>>) {
    let root = std::env::temp_dir().join(format!("qs-agent-{name}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).unwrap();
    }
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("progs.src"), "progs.dat\ndefs.qc\nweapons.qc\n").unwrap();
    std::fs::write(root.join("defs.qc"), defs()).unwrap();
    std::fs::write(root.join("weapons.qc"), weapons).unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute("project.open", json!({ "path": root }))
        .unwrap();
    (root, session)
}

/// A game folder the scenarios can play over: an id1/pak0.pak of the full game.
fn game(root: &Path) -> PathBuf {
    let game = root.join("game");
    std::fs::create_dir_all(game.join("id1")).unwrap();
    let pop: Vec<u8> = qs_vfs::FS_POP_TABLE
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect();
    let pak0 = qs_pak::write_pak(&[(qs_vfs::FS_POP_PATH, pop)]).unwrap();
    std::fs::write(game.join("id1/pak0.pak"), pak0).unwrap();
    game
}

// No spec statement: P4-06, AGT-5, the acceptance criterion. A run whose scenario fails
// cannot be marked done, however often the model says it is finished: each time it is told
// the scenario failed, and the run ends not done with that reason
#[test]
fn a_failing_scenario_cannot_be_marked_done() {
    let (root, mut session) = project("scenario", FIXED);
    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(root.join("tests/start.scenario"), "map nomap\nframes 1\n").unwrap();
    let limits = Limits {
        turns: 3,
        test_params: json!({ "game": game(&root) }),
    };
    let mut model = Scripted::new(vec![finish(), finish(), finish()]);
    let report = run(
        &mut session,
        &mut model,
        "make the start scenario pass",
        &limits,
    )
    .unwrap();
    let Outcome::NotDone { reasons } = &report.outcome else {
        panic!("{:?}", report.outcome);
    };
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("tests/start.scenario") && r.contains("failed")),
        "{reasons:?}"
    );
    let verification = report.verification.unwrap();
    assert!(verification.compile_clean && !verification.tests_passed);
    let told = report
        .transcript
        .iter()
        .filter(|m| matches!(m, Message::User { text } if text.starts_with("Not done")))
        .count();
    assert_eq!(told, 3, "told after each finish");
    assert_eq!(report.checkpoint, Some(1));
}

// No spec statement: P4-06, AGT-1, AGT-5, PRIN-4. Told its first finish is not done because
// a test fails, the model stages a fix; the compile and tests run against the staged text and
// pass, so the run is done, proposing the fix as a hunk and leaving the file on disk alone
#[test]
fn a_run_fixes_its_work_until_it_passes() {
    let (root, mut session) = project("fix", FAILING);
    let weapons = root.join("weapons.qc");
    let mut model = Scripted::new(vec![
        finish(),
        calls(vec![(
            "edit.apply",
            json!({ "files": [{ "path": weapons, "text": FIXED }] }),
        )]),
        finish(),
    ]);
    let report = run(
        &mut session,
        &mut model,
        "make test_damage pass",
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(
        report.outcome,
        Outcome::Done {
            summary: "done".to_owned()
        }
    );
    let staged = report.staged.as_array().unwrap();
    assert_eq!(staged.len(), 1, "{staged:?}");
    assert!(staged[0]["added"].as_str().unwrap().contains("return 5"));
    assert_eq!(std::fs::read_to_string(&weapons).unwrap(), FAILING);
    assert!(report.transcript.iter().any(|m| matches!(
        m,
        Message::User { text } if text.contains("test test_damage failed: damage is not 5")
    )));
    assert_eq!(model.seen, [1, 3, 5], "the model sees each answer");
}

// No spec statement: P4-06, PRIN-4. A call to a command that is not the agent's, such as
// accepting its own hunks, is refused as an error result; a command's own failure is an error
// result too; the model is still asked on
#[test]
fn calls_outside_the_tools_are_refused() {
    let (_, mut session) = project("refused", FIXED);
    let mut model = Scripted::new(vec![
        calls(vec![
            ("edit.accept", json!({ "hunks": [0] })),
            (
                "symbol.hover",
                json!({ "path": "/nowhere.qc", "offset": "x" }),
            ),
            ("project.symbols", json!({ "query": "damage" })),
        ]),
        finish(),
    ]);
    let report = run(&mut session, &mut model, "look around", &Limits::default()).unwrap();
    let results: Vec<(&Value, bool)> = report
        .transcript
        .iter()
        .filter_map(|m| match m {
            Message::Tool {
                content, is_error, ..
            } => Some((content, *is_error)),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 3);
    assert!(results[0].1);
    assert!(
        results[0]
            .0
            .as_str()
            .unwrap()
            .contains("not one of your tools")
    );
    assert!(results[1].1);
    assert!(!results[2].1);
    assert!(matches!(report.outcome, Outcome::Done { .. }));
    assert!(tools(&session).iter().all(|t| t.name != "edit.accept"
        && t.name != "file.save"
        && t.name != "checkpoint.restore"));
}

// No spec statement: P4-06. A model that never finishes ends not done when its turns run out;
// a model that fails ends the run with its error
#[test]
fn turns_run_out_and_models_fail() {
    let (_, mut session) = project("limits", FIXED);
    let limits = Limits {
        turns: 2,
        ..Limits::default()
    };
    let looking = || calls(vec![("edit.staged", json!({}))]);
    let mut model = Scripted::new(vec![looking(), looking()]);
    let report = run(&mut session, &mut model, "wander", &limits).unwrap();
    assert_eq!(
        report.outcome,
        Outcome::NotDone {
            reasons: vec!["the model did not finish in 2 turns".to_owned()]
        }
    );
    assert_eq!(report.verification, None);
    let mut silent = Scripted::new(Vec::new());
    assert!(matches!(
        run(&mut session, &mut silent, "anything", &limits),
        Err(AgentError::Model(why)) if why == "out of script"
    ));
}
