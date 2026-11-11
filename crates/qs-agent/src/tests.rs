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

use super::{
    AgentError, Message, Model, Outcome, Reply, RunConfig, ToolCall, ToolSpec, run, system_prompt,
    tools,
};

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
    fn reply(
        &mut self,
        system: &str,
        tools: &[ToolSpec],
        transcript: &[Message],
    ) -> Result<Reply, String> {
        assert_eq!(system, system_prompt());
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
    let limits = RunConfig {
        turns: 3,
        test_params: json!({ "game": game(&root) }),
        ..RunConfig::new(&root)
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
        &RunConfig::new(&root),
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
    let Message::User { text: brief } = &report.transcript[0] else {
        panic!("the brief comes first");
    };
    assert!(brief.starts_with("Task: make test_damage pass"), "{brief}");
    assert!(brief.contains("- weapons.qc"), "{brief}");
    assert!(
        brief.contains("the tests do not pass") && brief.contains("test test_damage failed"),
        "{brief}"
    );
}

// No spec statement: P4-06, PRIN-4. A call to a command that is not the agent's, such as
// accepting its own hunks, is refused as an error result; a command's own failure is an error
// result too; the model is still asked on
#[test]
fn calls_outside_the_tools_are_refused() {
    let (root, mut session) = project("refused", FIXED);
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
    let report = run(
        &mut session,
        &mut model,
        "look around",
        &RunConfig::new(&root),
    )
    .unwrap();
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
    let names: Vec<String> = tools(&session).into_iter().map(|t| t.name).collect();
    for denied in [
        "edit.accept",
        "edit.reject",
        "file.save",
        "checkpoint.restore",
        "file.update",
        "symbol.rename",
        "project.open",
    ] {
        assert!(!names.contains(&denied.to_owned()), "{denied} in {names:?}");
    }
    for tool in [
        "project.files",
        "project.search",
        "symbol.references",
        "vm.run",
    ] {
        assert!(names.contains(&tool.to_owned()), "{tool} not in {names:?}");
    }
}

// No spec statement: P4-06. A model that never finishes ends not done when its turns run out;
// a model that fails ends the run with its error
#[test]
fn turns_run_out_and_models_fail() {
    let (root, mut session) = project("limits", FIXED);
    let limits = RunConfig {
        turns: 2,
        ..RunConfig::new(&root)
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

// No spec statement: P4-06. A relative path is taken from the project; an edit outside the
// project is refused before it runs; a long output is cut, saying so; a compile error is a
// reason with its place
#[test]
fn the_guardrails_hold() {
    let (root, mut session) = project("guards", FIXED);
    let broken = "void(string e) error = #10;\nfloat() damage = { return nowhere; };\n";
    let mut model = Scripted::new(vec![
        calls(vec![
            (
                "edit.apply",
                json!({ "files": [{ "path": "../outside.qc", "text": "float x;\n" }] }),
            ),
            ("file.read", json!({ "path": "weapons.qc" })),
            ("project.search", json!({ "text": "damage" })),
            (
                "edit.apply",
                json!({ "files": [{ "path": "weapons.qc", "text": broken }] }),
            ),
        ]),
        finish(),
    ]);
    let config = RunConfig {
        turns: 2,
        output_limit: 60,
        ..RunConfig::new(&root)
    };
    let report = run(&mut session, &mut model, "break it", &config).unwrap();
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
    assert!(results[0].1, "an edit outside the project is refused");
    assert!(
        results[0]
            .0
            .as_str()
            .unwrap()
            .contains("outside the project")
    );
    assert!(!root.parent().unwrap().join("outside.qc").exists());
    assert!(!results[2].1, "a relative path is the project's");
    assert!(
        results[2]
            .0
            .as_str()
            .unwrap()
            .contains("[cut: the output was"),
        "{}",
        results[2].0
    );
    let Outcome::NotDone { reasons } = &report.outcome else {
        panic!("{:?}", report.outcome);
    };
    assert!(
        reasons
            .iter()
            .any(|r| r.starts_with("error at ") && r.contains("weapons.qc:2")),
        "{reasons:?}"
    );
}

/// A model that keeps what it was given and finishes at once.
#[derive(Default)]
struct Recording {
    system: String,
    brief: String,
}

impl Model for Recording {
    fn reply(
        &mut self,
        system: &str,
        _tools: &[ToolSpec],
        transcript: &[Message],
    ) -> Result<Reply, String> {
        self.system = system.to_owned();
        if let Some(Message::User { text }) = transcript.first() {
            self.brief.clone_from(text);
        }
        Ok(finish())
    }
}

// No spec statement: P4-08, the acceptance criterion. A run's context holds the project's
// AGENTS.md, each module's dialect and target engine with that engine's builtins, and the
// agent reference in the system prompt; the brief is held to its snapshot,
// `src/snapshots/brief.txt` (a changed brief is written beside it as `brief.new.txt`)
#[test]
fn the_context_holds_the_engine_the_dialect_and_agents_md() {
    let (root, mut session) = project("context", FIXED);
    std::fs::write(
        root.join("AGENTS.md"),
        "Weapons live in weapons.qc. Keep damage numbers whole.\n",
    )
    .unwrap();
    let mut model = Recording::default();
    run(
        &mut session,
        &mut model,
        "Make the axe stronger.",
        &RunConfig::new(&root),
    )
    .unwrap();
    assert!(model.system.contains("# QuakeC for the agent"));
    assert!(model.system.contains("# Working in QuakeC Studio"));
    let brief = model.brief.replace(&*root.to_string_lossy(), "<project>");
    assert!(brief.contains("Weapons live in weapons.qc."), "{brief}");
    assert!(
        brief.contains("Module progs: ssqc QuakeC in the vanilla dialect"),
        "{brief}"
    );
    assert!(
        brief.contains("- void(vector ang) makevectors = #1;"),
        "{brief}"
    );
    let snapshot = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/snapshots/brief.txt");
    let expected = std::fs::read_to_string(&snapshot).unwrap_or_default();
    if brief != expected {
        std::fs::write(snapshot.with_file_name("brief.new.txt"), &brief).unwrap();
        panic!("the brief changed; see src/snapshots/brief.new.txt:\n{brief}");
    }
}

// No spec statement: P4-09, a run steps one reply at a time as the window drives it: a request
// while turns remain, the calls run on each answer, none once it is done, and a model's
// failure is the answer's error; a model that cannot stream gives its text in one piece
#[test]
fn a_run_steps_one_reply_at_a_time() {
    let (root, mut session) = project("stepping", FIXED);
    let mut run = super::Run::start(&mut session, "Check it.", RunConfig::new(&root)).unwrap();
    assert!(run.checkpoint().is_some());
    let first = run.request().unwrap();
    assert_eq!(first.system, system_prompt());
    assert_eq!(first.transcript.len(), 1, "the brief");
    run.answer(&mut session, Ok(calls(vec![("compile", json!({}))])))
        .unwrap();
    assert_eq!(run.transcript().len(), 3, "the calls and their result");
    run.answer(&mut session, Ok(finish())).unwrap();
    assert!(run.request().is_none(), "done: nothing more to ask");
    let report = run.finish(&mut session);
    assert!(matches!(report.outcome, Outcome::Done { .. }));
    let mut failing = super::Run::start(&mut session, "x", RunConfig::new(&root)).unwrap();
    assert!(matches!(
        failing.answer(&mut session, Err("overloaded".to_owned())),
        Err(AgentError::Model(why)) if why == "overloaded"
    ));
    let mut pieces = Vec::new();
    let mut model = Scripted::new(vec![finish()]);
    let reply = model
        .reply_streaming(
            &system_prompt(),
            &tools(&session),
            &[Message::User {
                text: "x".to_owned(),
            }],
            &mut |piece| pieces.push(piece.to_owned()),
        )
        .unwrap();
    assert_eq!(reply, finish());
    assert_eq!(pieces, ["done"]);
}

// No spec statement: P4-10, an inline edit's prompt holds the file, the selection and the
// request, and its answer is the reply's text without code fences
#[test]
fn an_inline_edit_asks_and_answers_plainly() {
    let prompt = super::inline_prompt(Path::new("/p/a.qc"), "x = 1;\n", "x = 1;", "make it 2");
    assert!(
        prompt.contains("/p/a.qc") && prompt.contains("make it 2") && prompt.contains("x = 1;")
    );
    let plain = Reply::Finish {
        summary: "x = 2;".to_owned(),
    };
    assert_eq!(super::inline_answer(&plain), "x = 2;");
    let fenced = Reply::Finish {
        summary: "```quakec\nx = 2;\ny = 3;\n```".to_owned(),
    };
    assert_eq!(super::inline_answer(&fenced), "x = 2;\ny = 3;");
}
