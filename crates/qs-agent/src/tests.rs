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
    AgentError, AgentProfile, Decision, Message, Model, Outcome, Permissions, Permit, READ_TOOLS,
    Reply, RunConfig, ToolCall, ToolRule, ToolSpec, run, system_prompt, tools,
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

// No spec statement: P4-08 and P5-15, the acceptance criteria. A run's context holds the
// project's AGENTS.md, each module's dialect and target engine with that engine's builtins, the
// open map with its WADs and entity classes, and the agent and map references in the system
// prompt; the brief is held to its snapshot,
// `src/snapshots/brief.txt` (a changed brief is written beside it as `brief.new.txt`)
#[test]
fn the_context_holds_the_engine_the_dialect_and_agents_md() {
    let (root, mut session) = project("context", FIXED);
    std::fs::write(
        root.join("AGENTS.md"),
        "Weapons live in weapons.qc. Keep damage numbers whole.\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("maps")).unwrap();
    std::fs::write(
        root.join("maps/start.map"),
        "{\n\"classname\" \"worldspawn\"\n\"wad\" \"gfx/base.wad;gfx/extra.wad\"\n}\n\
         {\n\"classname\" \"info_player_start\"\n\"origin\" \"0 0 24\"\n}\n\
         {\n\"classname\" \"light\"\n\"origin\" \"0 0 96\"\n}\n\
         {\n\"classname\" \"light\"\n\"origin\" \"64 0 96\"\n}\n",
    )
    .unwrap();
    let mut model = Recording::default();
    let config = RunConfig {
        map: Some(root.join("maps/start.map")),
        ..RunConfig::new(&root)
    };
    run(&mut session, &mut model, "Make the axe stronger.", &config).unwrap();
    assert!(model.system.contains("# QuakeC for the agent"));
    assert!(model.system.contains("# Working in QuakeC Studio"));
    assert!(model.system.contains("# Maps for the agent"));
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
    assert!(brief.contains("The open map is maps/start.map."), "{brief}");
    assert!(
        brief.contains("Its WADs: gfx/base.wad, gfx/extra.wad."),
        "{brief}"
    );
    assert!(brief.contains("- light: 2\n"), "{brief}");
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

/// The content of the result of call `id` in `transcript`.
fn result_of(transcript: &[Message], id: &str) -> Option<(Value, bool)> {
    transcript.iter().find_map(|message| match message {
        Message::Tool {
            call_id,
            content,
            is_error,
        } if call_id == id => Some((content.clone(), *is_error)),
        _ => None,
    })
}

// No spec statement: P4-30, the acceptance criterion: a call a rule denies never runs and the
// model reads why; a call that asks waits, the run asking the model nothing, until the modder
// answers, and the reply's later calls wait behind it
#[test]
fn denied_calls_never_run_and_asking_calls_wait() {
    let (root, mut session) = project("permissions", FIXED);
    let mut config = RunConfig::new(&root);
    config.permissions.rules.tools.insert(
        "file.read".to_owned(),
        ToolRule {
            default: Some(Decision::Ask),
            deny: vec!["progs.src".to_owned()],
            ..ToolRule::default()
        },
    );
    let mut run = super::Run::start(&mut session, "Read.", config).unwrap();
    run.answer(
        &mut session,
        Ok(calls(vec![
            ("file.read", json!({ "path": "progs.src" })),
            ("file.read", json!({ "path": "weapons.qc" })),
            ("project.files", json!({})),
        ])),
    )
    .unwrap();
    let (denied, is_error) = result_of(run.transcript(), "c0").unwrap();
    assert!(is_error);
    assert_eq!(
        denied,
        json!(
            "file.read was not run: it is not allowed: the rule deny `progs.src` of file.read \
             matches progs.src"
        )
    );
    let asking = run.asking().unwrap();
    assert_eq!(asking.call.id, "c1");
    assert_eq!(asking.subjects, ["weapons.qc"]);
    assert!(
        run.request().is_none(),
        "the model is asked nothing meanwhile"
    );
    assert!(
        result_of(run.transcript(), "c2").is_none(),
        "the next call waits"
    );
    run.permit(&mut session, Permit::Once);
    let (read, is_error) = result_of(run.transcript(), "c1").unwrap();
    assert!(!is_error && read.to_string().contains("damage"), "{read}");
    assert!(
        result_of(run.transcript(), "c2").is_some(),
        "then the rest run"
    );
    assert!(run.request().is_some());

    run.answer(
        &mut session,
        Ok(calls(vec![("file.read", json!({ "path": "weapons.qc" }))])),
    )
    .unwrap();
    assert!(run.asking().is_some(), "Allow once allows only that call");
    run.permit(&mut session, Permit::Deny);
    let (refused, _) = result_of(&run.transcript()[run.transcript().len() - 1..], "c0").unwrap();
    assert_eq!(
        refused,
        json!("file.read was not run: the modder did not allow it")
    );

    run.answer(
        &mut session,
        Ok(calls(vec![("file.read", json!({ "path": "weapons.qc" }))])),
    )
    .unwrap();
    run.permit(&mut session, Permit::Always);
    assert_eq!(
        run.permissions().rules.tools["file.read"].allow,
        ["weapons.qc"]
    );
    let ask_again = calls(vec![("file.read", json!({ "path": "weapons.qc" }))]);
    run.answer(&mut session, Ok(ask_again)).unwrap();
    assert!(
        run.asking().is_none(),
        "Always allow allows it from then on"
    );
    assert_eq!(
        run.permissions()
            .decide(
                &ToolCall {
                    id: String::new(),
                    name: "file.read".to_owned(),
                    arguments: json!({ "path": "progs.src" }),
                },
                &root
            )
            .decision,
        Decision::Deny,
        "and denies what was denied"
    );
}

// No spec statement: P4-30, the acceptance criterion: profiles switch the tools offered to the
// model, in tools/list order; a run with no one to ask refuses the calls that ask
#[test]
fn profiles_switch_the_tools_offered() {
    let (root, mut session) = project("profiles", FIXED);
    let every: Vec<String> = tools(&session).into_iter().map(|t| t.name).collect();
    let offered = |session: &mut Session<Workbench<FsLoader>>, profile| {
        let mut config = RunConfig::new(&root);
        config.permissions = Permissions::of(profile);
        let mut run = super::Run::start(session, "x", config).unwrap();
        run.request()
            .unwrap()
            .tools
            .into_iter()
            .map(|t| t.name)
            .collect::<Vec<_>>()
    };
    let ask = offered(&mut session, AgentProfile::Ask);
    let reading: Vec<String> = every
        .iter()
        .filter(|name| READ_TOOLS.contains(&name.as_str()))
        .cloned()
        .collect();
    assert_eq!(ask, reading, "reading only, in tools/list order");
    assert!(!ask.contains(&"edit.apply".to_owned()));
    assert_eq!(offered(&mut session, AgentProfile::Write), every);
    assert_eq!(offered(&mut session, AgentProfile::Full), every);

    let mut model = Scripted::new(vec![
        calls(vec![("vm.run", json!({ "function": "test_damage" }))]),
        finish(),
    ]);
    let mut asks = RunConfig::new(&root);
    asks.permissions.rules.tools.insert(
        "vm.run".to_owned(),
        ToolRule {
            default: Some(Decision::Ask),
            ..ToolRule::default()
        },
    );
    let report = run(&mut session, &mut model, "x", &asks).unwrap();
    let (refused, _) = result_of(&report.transcript, "c0").unwrap();
    assert_eq!(
        refused,
        json!("vm.run was not run: the modder did not allow it")
    );
}

// No spec statement: P4-12, the acceptance criterion: a thread started from an entity carries
// its classname, its current think function and its fields
#[test]
fn asking_about_an_entity_carries_its_state() {
    let ogre = super::EntityState {
        edict: 12,
        classname: "monster_ogre".to_owned(),
        fields: vec![
            ("health".to_owned(), "200".to_owned()),
            ("think".to_owned(), "ogre_stand1".to_owned()),
            ("nextthink".to_owned(), "3.1".to_owned()),
        ],
    };
    let task = super::ask_about(&ogre);
    assert!(task.contains("classname monster_ogre"), "{task}");
    assert!(task.contains("current think function is ogre_stand1, due at time 3.1"));
    assert!(task.contains("- health = 200"));
    let still = super::EntityState {
        edict: 1,
        classname: "worldspawn".to_owned(),
        fields: Vec::new(),
    };
    assert!(still.brief().contains("no think function"));
}

// No spec statement: P4-12, @ mentions attach a file's text, a definition's place, a builtin's
// declaration, the selected entity and the console's last error; what a mention names that is
// not there is said, and a task with none is unchanged
#[test]
fn mentions_attach_what_they_name() {
    let (root, mut session) = project("mentions", FIXED);
    let live = super::Live {
        entity: Some(super::EntityState {
            edict: 3,
            classname: "player".to_owned(),
            fields: vec![("health".to_owned(), "100".to_owned())],
        }),
        error: Some("NULL function in W_FireAxe".to_owned()),
    };
    let task =
        "Fix @error in @file:weapons.qc using @symbol:damage and @builtin:error, see @entity.";
    let expanded = super::expand_mentions(&mut session, &root, task, &live);
    assert!(expanded.starts_with(task));
    assert!(
        expanded.contains("float() damage = { return 5; };"),
        "{expanded}"
    );
    assert!(expanded.contains("damage is defined at"), "{expanded}");
    assert!(
        expanded.contains("void(string e) error = #10;"),
        "{expanded}"
    );
    assert!(expanded.contains("classname player"));
    assert!(expanded.contains("NULL function in W_FireAxe"));
    let missing = super::expand_mentions(
        &mut session,
        &root,
        "@file:nowhere.qc @entity",
        &super::Live::default(),
    );
    assert!(missing.contains("No file nowhere.qc") && missing.contains("No entity is selected"));
    assert_eq!(
        super::expand_mentions(&mut session, &root, "plain", &live),
        "plain"
    );
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

// No spec statement: P4-31, the acceptance criterion: a thread near the model's context limit
// is compacted, its summary kept, and it continues, its next turn fitting the context and
// keeping the last call with its result
#[test]
fn a_thread_compacted_near_the_limit_continues_within_the_context() {
    let (root, mut session) = project("compact", FIXED);
    let mut config = RunConfig::new(&root);
    config.context_limit = 6000;
    config.output_limit = 8000;
    let limit = config.context_limit;
    let mut run = super::Run::start(&mut session, "Read the defs", config).unwrap();
    let read = || calls(vec![("file.read", json!({ "path": root.join("defs.qc") }))]);
    let mut compacted = None;
    for _ in 0..20 {
        let request = run.request().unwrap();
        if request.tools.is_empty() {
            let last = request.transcript.last().unwrap();
            assert_eq!(
                last,
                &Message::User {
                    text: super::COMPACT_PROMPT.to_owned()
                }
            );
            assert!(super::estimated_tokens(&request.transcript) * 10 > limit * 8);
            run.answer(
                &mut session,
                Ok(Reply::Finish {
                    summary: "Read defs.qc twice; nothing changed.".to_owned(),
                }),
            )
            .unwrap();
            compacted = Some(run.transcript().to_vec());
            break;
        }
        run.answer(&mut session, Ok(read())).unwrap();
    }
    let compacted = compacted.unwrap();
    assert_eq!(run.summaries(), ["Read defs.qc twice; nothing changed."]);
    let Message::User { text } = &compacted[1] else {
        panic!("{:?}", compacted[1])
    };
    assert!(text.starts_with(super::SUMMARY_HEADING), "{text}");
    assert!(
        matches!(compacted[2], Message::Assistant { .. }),
        "the last call stays"
    );
    assert!(
        matches!(compacted[3], Message::Tool { .. }),
        "with its result"
    );
    let next = run.request().unwrap();
    assert!(!next.tools.is_empty(), "a turn of the work again");
    assert!(
        super::estimated_tokens(&next.transcript) < limit,
        "the next turn fits"
    );
    run.answer(&mut session, Ok(finish())).unwrap();
    let report = run.finish(&mut session);
    assert!(
        matches!(report.outcome, Outcome::Done { .. }),
        "{:?}",
        report.outcome
    );
}

// No spec statement: P4-31: messages queued while a turn ran and images given as context join
// the transcript the model reads next; a thread run again from an edited message goes on from
// that message
#[test]
fn queued_messages_images_and_a_resumed_thread() {
    let (root, mut session) = project("queue", FIXED);
    let mut run = super::Run::start(&mut session, "Look", RunConfig::new(&root)).unwrap();
    run.push_user("Also check the axe.");
    run.push_image("image/png", super::base64(b"png"));
    let request = run.request().unwrap();
    assert_eq!(
        &request.transcript[1..],
        [
            Message::User {
                text: "Also check the axe.".to_owned()
            },
            Message::Image {
                media_type: "image/png".to_owned(),
                data: "cG5n".to_owned()
            }
        ]
    );
    let edited = vec![Message::User {
        text: "Look at the shotgun instead.".to_owned(),
    }];
    let resumed = super::Run::resume(&mut session, edited.clone(), RunConfig::new(&root)).unwrap();
    assert_eq!(resumed.transcript(), edited.as_slice());
    assert!(resumed.checkpoint().is_some(), "a new checkpoint");
}

// No spec statement: P4-31: base64 as RFC 4648 section 10's test vectors give it
#[test]
fn base64_is_rfc_4648s() {
    for (bytes, encoded) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(super::base64(bytes.as_bytes()), encoded);
    }
    assert_eq!(
        Message::Image {
            media_type: "image/png".to_owned(),
            data: String::new()
        }
        .estimated_tokens(),
        super::IMAGE_TOKENS
    );
}

// No spec statement: P4-31: kept threads are listed the newest first, numbered in the order
// kept, searched by title and messages, and read back as they were
#[test]
fn threads_are_kept_listed_and_searched() {
    use super::history::{ThreadRecord, keep, load, threads_dir, title_of};
    let root = std::env::temp_dir().join(format!("qs-agent-history-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&root);
    let dir = threads_dir(&root);
    assert!(load(&dir).unwrap().is_empty());
    let record = |task: &str, said: &str| ThreadRecord {
        number: 0,
        title: title_of(task),
        transcript: vec![
            Message::User {
                text: task.to_owned(),
            },
            Message::Assistant {
                text: said.to_owned(),
                calls: Vec::new(),
            },
        ],
        checkpoint: Some(3),
        staged: vec![7],
        summaries: Vec::new(),
        tokens: 42,
        outcome: "done".to_owned(),
    };
    assert_eq!(
        keep(&dir, record("Fix the axe\nIt is weak", "Raised it")).unwrap(),
        1
    );
    assert_eq!(keep(&dir, record("Add a rocket", "Added")).unwrap(), 2);
    let threads = load(&dir).unwrap();
    let titles: Vec<&str> = threads.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, ["Add a rocket", "Fix the axe"]);
    assert_eq!(threads[1].staged, [7]);
    assert_eq!(threads[1].checkpoint, Some(3));
    assert!(threads[1].matches("AXE raised"));
    assert!(!threads[1].matches("rocket"));
    assert!(threads[0].matches(""));
}
