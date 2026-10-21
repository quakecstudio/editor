// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap is a failed test, and the guard tests panic on purpose"
)]

use std::collections::BTreeMap;
use std::fmt;

use proptest::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    BUILTIN_IDS, BoxError, Command, CommandError, CommandId, IdError, MAX_ID_LEN, NoParams,
    Outcome, RegisterError, Registry, Session, UndoStep,
};

/// The context the test commands work on.
#[derive(Debug, Default, PartialEq, Eq)]
struct Doc {
    text: String,
}

/// What the test session starts with.
#[derive(Default)]
struct SessionSpec {
    text: &'static str,
}

/// A session over a `Doc` holding `spec.text`, with every test command registered.
fn session(spec: SessionSpec) -> Session<Doc> {
    let mut registry = Registry::new();
    registry.register(Append).unwrap();
    registry.register(Len).unwrap();
    registry.register(Fail).unwrap();
    registry.register(Boom).unwrap();
    registry.register(Flaky).unwrap();
    registry.register(BoomOnUndo).unwrap();
    registry.register(BadOutput).unwrap();
    Session::new(
        Doc {
            text: spec.text.to_owned(),
        },
        registry,
    )
}

#[derive(Debug)]
struct TestError;

impl fmt::Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the test command refused")
    }
}

impl std::error::Error for TestError {}

/// Undoes an append of `len` bytes by truncating, and redoes it by appending `text`.
struct Truncate {
    text: String,
    /// Undo or redo calls left that fail before one succeeds.
    fails_left: u32,
    /// Whether undo panics instead.
    panics: bool,
}

impl Truncate {
    fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            fails_left: 0,
            panics: false,
        }
    }

    fn refuse(&mut self) -> Result<(), BoxError> {
        if self.fails_left > 0 {
            self.fails_left -= 1;
            return Err(Box::new(TestError));
        }
        Ok(())
    }
}

impl UndoStep<Doc> for Truncate {
    fn undo(&mut self, context: &mut Doc) -> Result<(), BoxError> {
        if self.panics {
            context.text.push_str("half-undone");
            panic!("undo step exploded");
        }
        self.refuse()?;
        let keep = context.text.len() - self.text.len();
        context.text.truncate(keep);
        Ok(())
    }

    fn redo(&mut self, context: &mut Doc) -> Result<(), BoxError> {
        self.refuse()?;
        context.text.push_str(&self.text);
        Ok(())
    }
}

/// The parameters of `doc.append`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AppendParams {
    /// The text to add at the end.
    text: String,
}

struct Append;

impl Command<Doc> for Append {
    const ID: &'static str = "doc.append";
    const SUMMARY: &'static str = "Appends text.";
    type Params = AppendParams;
    type Output = usize;
    type Error = TestError;

    fn run(
        &self,
        context: &mut Doc,
        params: AppendParams,
    ) -> Result<Outcome<usize, Doc>, TestError> {
        context.text.push_str(&params.text);
        Ok(Outcome::undoable(
            context.text.len(),
            Truncate::new(&params.text),
        ))
    }
}

struct Len;

impl Command<Doc> for Len {
    const ID: &'static str = "doc.len";
    const SUMMARY: &'static str = "Returns the length.";
    type Params = NoParams;
    type Output = usize;
    type Error = TestError;

    fn run(&self, context: &mut Doc, _: NoParams) -> Result<Outcome<usize, Doc>, TestError> {
        Ok(Outcome::unchanged(context.text.len()))
    }
}

struct Fail;

impl Command<Doc> for Fail {
    const ID: &'static str = "doc.fail";
    const SUMMARY: &'static str = "Always fails.";
    type Params = NoParams;
    type Output = ();
    type Error = TestError;

    fn run(&self, _: &mut Doc, _: NoParams) -> Result<Outcome<(), Doc>, TestError> {
        Err(TestError)
    }
}

struct Boom;

impl Command<Doc> for Boom {
    const ID: &'static str = "doc.boom";
    const SUMMARY: &'static str = "Changes the text, then panics.";
    type Params = NoParams;
    type Output = ();
    type Error = TestError;

    fn run(&self, context: &mut Doc, _: NoParams) -> Result<Outcome<(), Doc>, TestError> {
        context.text.push_str("partial");
        let index: usize = context.text.len() + 10;
        // An out-of-bounds index, the kind of bug the guard is for.
        let _ = context.text.as_bytes()[index];
        Ok(Outcome::unchanged(()))
    }
}

/// The parameters of `doc.flaky`.
#[derive(Deserialize, JsonSchema)]
struct FlakyParams {
    /// How many undo or redo calls fail before one succeeds.
    fails: u32,
}

struct Flaky;

impl Command<Doc> for Flaky {
    const ID: &'static str = "doc.flaky";
    const SUMMARY: &'static str = "Appends x with a step that fails some times.";
    type Params = FlakyParams;
    type Output = ();
    type Error = TestError;

    fn run(&self, context: &mut Doc, params: FlakyParams) -> Result<Outcome<(), Doc>, TestError> {
        context.text.push('x');
        let mut step = Truncate::new("x");
        step.fails_left = params.fails;
        Ok(Outcome::undoable((), step))
    }
}

struct BoomOnUndo;

impl Command<Doc> for BoomOnUndo {
    const ID: &'static str = "doc.boom_on_undo";
    const SUMMARY: &'static str = "Appends y with a step that panics.";
    type Params = NoParams;
    type Output = ();
    type Error = TestError;

    fn run(&self, context: &mut Doc, _: NoParams) -> Result<Outcome<(), Doc>, TestError> {
        context.text.push('y');
        let mut step = Truncate::new("y");
        step.panics = true;
        Ok(Outcome::undoable((), step))
    }
}

struct BadOutput;

impl Command<Doc> for BadOutput {
    const ID: &'static str = "doc.bad_output";
    const SUMMARY: &'static str = "Appends z and returns a map JSON cannot hold.";
    type Params = NoParams;
    type Output = BTreeMap<(u8, u8), u8>;
    type Error = TestError;

    fn run(
        &self,
        context: &mut Doc,
        _: NoParams,
    ) -> Result<Outcome<BTreeMap<(u8, u8), u8>, Doc>, TestError> {
        context.text.push('z');
        Ok(Outcome::undoable(
            BTreeMap::from([((1, 2), 3)]),
            Truncate::new("z"),
        ))
    }
}

fn append(session: &mut Session<Doc>, text: &str) -> Value {
    session
        .execute("doc.append", json!({ "text": text }))
        .unwrap()
}

fn text(session: &Session<Doc>) -> &str {
    &session.context().text
}

// The guard (PRIN-7). CI runs this crate's tests under `--profile release` as well, so these
// prove the release build catches the panic too.

#[test]
fn a_panicking_command_is_an_error_and_the_session_continues() {
    let mut session = session(SessionSpec::default());
    append(&mut session, "ab");

    let error = session.execute("doc.boom", json!({})).unwrap_err();

    let CommandError::Panicked { command, message } = &error else {
        panic!("expected a caught panic, got {error:?}");
    };
    assert_eq!(command.as_str(), "doc.boom");
    assert!(message.contains("out of bounds"), "{message}");
    assert!(
        error
            .to_string()
            .starts_with("doc.boom hit an internal error")
    );
    // The partial change stays, and the history that no longer matches it is gone.
    assert_eq!(text(&session), "abpartial");
    assert_eq!(session.undo_depth(), 0);
    assert!(matches!(session.undo(), Err(CommandError::NothingToUndo)));
    // The session goes on running commands.
    assert_eq!(append(&mut session, "c"), json!(10));
    assert_eq!(session.execute("doc.len", json!({})).unwrap(), json!(10));
    session.undo().unwrap();
    assert_eq!(text(&session), "abpartial");
}

#[test]
fn a_panic_with_a_string_payload_keeps_its_message() {
    let mut session = session(SessionSpec::default());
    session.execute("doc.boom_on_undo", json!({})).unwrap();

    let error = session.undo().unwrap_err();

    let CommandError::Panicked { command, message } = error else {
        panic!("expected a caught panic");
    };
    assert_eq!(command.as_str(), "doc.boom_on_undo");
    assert_eq!(message, "undo step exploded");
}

#[test]
fn a_panic_with_another_payload_still_has_a_message() {
    struct AnyPanic;
    impl Command<Doc> for AnyPanic {
        const ID: &'static str = "doc.any_panic";
        const SUMMARY: &'static str = "Panics with a payload that is not text.";
        type Params = NoParams;
        type Output = ();
        type Error = TestError;
        fn run(&self, _: &mut Doc, _: NoParams) -> Result<Outcome<(), Doc>, TestError> {
            std::panic::panic_any(42_u8)
        }
    }
    let mut registry = Registry::new();
    registry.register(AnyPanic).unwrap();
    let mut session = Session::new(Doc::default(), registry);

    let error = session.execute("doc.any_panic", json!({})).unwrap_err();

    assert!(
        matches!(&error, CommandError::Panicked { message, .. } if message == "a panic with no message"),
        "{error:?}"
    );
}

#[test]
fn a_panicking_undo_step_clears_both_histories_and_the_session_continues() {
    let mut session = session(SessionSpec::default());
    append(&mut session, "a");
    append(&mut session, "b");
    session.undo().unwrap();
    session.execute("doc.boom_on_undo", json!({})).unwrap();
    assert_eq!((session.undo_depth(), session.redo_depth()), (2, 0));

    let error = session.execute("edit.undo", json!({})).unwrap_err();

    assert!(matches!(error, CommandError::Panicked { .. }), "{error:?}");
    assert_eq!(text(&session), "ayhalf-undone");
    assert_eq!((session.undo_depth(), session.redo_depth()), (0, 0));
    append(&mut session, "c");
    assert_eq!(text(&session), "ayhalf-undonec");
}

#[test]
fn a_panicking_redo_step_clears_the_history() {
    struct PanicOnRedo;
    impl UndoStep<Doc> for PanicOnRedo {
        fn undo(&mut self, _: &mut Doc) -> Result<(), BoxError> {
            Ok(())
        }
        fn redo(&mut self, _: &mut Doc) -> Result<(), BoxError> {
            panic!("redo step exploded")
        }
    }
    struct Once;
    impl Command<Doc> for Once {
        const ID: &'static str = "doc.once";
        const SUMMARY: &'static str = "Changes nothing, with a step whose redo panics.";
        type Params = NoParams;
        type Output = ();
        type Error = TestError;
        fn run(&self, _: &mut Doc, _: NoParams) -> Result<Outcome<(), Doc>, TestError> {
            Ok(Outcome::undoable((), PanicOnRedo))
        }
    }
    let mut registry = Registry::new();
    registry.register(Once).unwrap();
    let mut session = Session::new(Doc::default(), registry);
    session.execute("doc.once", json!({})).unwrap();
    session.execute("doc.once", json!({})).unwrap();
    session.undo().unwrap();

    let error = session.redo().unwrap_err();

    assert!(
        matches!(&error, CommandError::Panicked { message, .. } if message == "redo step exploded"),
        "{error:?}"
    );
    assert_eq!((session.undo_depth(), session.redo_depth()), (0, 0));
}

// Running commands.

#[test]
fn a_command_runs_with_typed_params_and_returns_json() {
    let mut session = session(SessionSpec { text: "ab" });
    assert_eq!(append(&mut session, "cd"), json!(4));
    assert_eq!(text(&session), "abcd");
    assert_eq!(session.execute("doc.len", json!({})).unwrap(), json!(4));
}

#[test]
fn an_unknown_id_is_an_error() {
    let mut session = session(SessionSpec::default());
    for id in ["doc.nope", "", "DOC.APPEND", "doc.append "] {
        let error = session.execute(id, json!({})).unwrap_err();
        assert!(
            matches!(&error, CommandError::Unknown { id: sent } if sent == id),
            "{error:?}"
        );
    }
    assert_eq!(
        session
            .execute("doc.nope", json!({}))
            .unwrap_err()
            .to_string(),
        "no command has the id \"doc.nope\""
    );
}

#[test]
fn arguments_that_do_not_match_the_schema_are_an_error_and_change_nothing() {
    let mut session = session(SessionSpec { text: "ab" });
    for params in [
        json!({}),
        json!({ "text": 3 }),
        json!({ "text": "x", "extra": 1 }),
        json!(null),
        json!("x"),
    ] {
        let error = session.execute("doc.append", params).unwrap_err();
        assert!(
            matches!(&error, CommandError::InvalidParams { command, .. } if command.as_str() == "doc.append"),
            "{error:?}"
        );
        assert!(std::error::Error::source(&error).is_some());
    }
    for builtin in BUILTIN_IDS {
        let error = session.execute(builtin, json!({ "x": 1 })).unwrap_err();
        assert!(
            matches!(error, CommandError::InvalidParams { .. }),
            "{error:?}"
        );
    }
    assert_eq!(text(&session), "ab");
    assert_eq!(session.undo_depth(), 0);
}

#[test]
fn a_failing_command_is_an_error_and_keeps_the_history() {
    let mut session = session(SessionSpec::default());
    append(&mut session, "a");
    session.undo().unwrap();

    let error = session.execute("doc.fail", json!({})).unwrap_err();

    assert!(
        matches!(&error, CommandError::Failed { command, .. } if command.as_str() == "doc.fail"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "doc.fail failed: the test command refused"
    );
    assert!(std::error::Error::source(&error).is_some());
    assert_eq!((session.undo_depth(), session.redo_depth()), (0, 1));
}

#[test]
fn output_that_is_not_json_is_an_error_but_the_change_can_be_undone() {
    let mut session = session(SessionSpec::default());

    let error = session.execute("doc.bad_output", json!({})).unwrap_err();

    assert!(
        matches!(&error, CommandError::Output { command, .. } if command.as_str() == "doc.bad_output"),
        "{error:?}"
    );
    assert!(std::error::Error::source(&error).is_some());
    assert_eq!(text(&session), "z");
    session.undo().unwrap();
    assert_eq!(text(&session), "");
}

// Undo and redo (PRIN-3).

#[test]
fn undo_and_redo_walk_the_history() {
    let mut session = session(SessionSpec { text: ">" });
    append(&mut session, "a");
    append(&mut session, "b");
    session.execute("doc.len", json!({})).unwrap();
    assert_eq!((session.undo_depth(), session.redo_depth()), (2, 0));

    assert_eq!(
        session.execute("edit.undo", json!({})).unwrap(),
        json!({ "command": "doc.append" })
    );
    assert_eq!(text(&session), ">a");
    assert_eq!(session.undo().unwrap().as_str(), "doc.append");
    assert_eq!(text(&session), ">");
    assert!(matches!(session.undo(), Err(CommandError::NothingToUndo)));
    assert_eq!((session.undo_depth(), session.redo_depth()), (0, 2));

    assert_eq!(
        session.execute("edit.redo", json!({})).unwrap(),
        json!({ "command": "doc.append" })
    );
    assert_eq!(text(&session), ">a");
    session.redo().unwrap();
    assert_eq!(text(&session), ">ab");
    assert!(matches!(session.redo(), Err(CommandError::NothingToRedo)));
    assert_eq!(
        session
            .execute("edit.redo", json!({}))
            .unwrap_err()
            .to_string(),
        "nothing to redo"
    );
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(
        session
            .execute("edit.undo", json!({}))
            .unwrap_err()
            .to_string(),
        "nothing to undo"
    );
}

#[test]
fn a_new_change_empties_the_redo_history_and_a_read_does_not() {
    let mut session = session(SessionSpec::default());
    append(&mut session, "a");
    append(&mut session, "b");
    session.undo().unwrap();
    session.execute("doc.len", json!({})).unwrap();
    session.execute("commands.list", json!({})).unwrap();
    assert_eq!(session.redo_depth(), 1);

    append(&mut session, "c");

    assert_eq!((session.undo_depth(), session.redo_depth()), (2, 0));
    assert!(matches!(session.redo(), Err(CommandError::NothingToRedo)));
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(text(&session), "");
}

#[test]
fn a_failing_step_stays_in_line_and_can_be_retried() {
    let mut session = session(SessionSpec::default());
    session.execute("doc.flaky", json!({ "fails": 1 })).unwrap();

    let error = session.undo().unwrap_err();
    assert!(
        matches!(&error, CommandError::UndoFailed { command, .. } if command.as_str() == "doc.flaky"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "cannot undo doc.flaky: the test command refused"
    );
    assert!(std::error::Error::source(&error).is_some());
    assert_eq!((text(&session), session.undo_depth()), ("x", 1));
    session.undo().unwrap();
    assert_eq!((text(&session), session.redo_depth()), ("", 1));

    session.execute("doc.flaky", json!({ "fails": 2 })).unwrap();
    session.undo().unwrap_err();
    session.undo().unwrap_err();
    session.undo().unwrap();
    // The step failed twice, so its first redo succeeds.
    session.redo().unwrap();
    assert_eq!(text(&session), "x");
}

#[test]
fn a_failing_redo_step_stays_in_line() {
    struct FailRedo;
    impl UndoStep<Doc> for FailRedo {
        fn undo(&mut self, _: &mut Doc) -> Result<(), BoxError> {
            Ok(())
        }
        fn redo(&mut self, _: &mut Doc) -> Result<(), BoxError> {
            Err(Box::new(TestError))
        }
    }
    struct Once;
    impl Command<Doc> for Once {
        const ID: &'static str = "doc.once";
        const SUMMARY: &'static str = "Changes nothing, with a step whose redo fails.";
        type Params = NoParams;
        type Output = ();
        type Error = TestError;
        fn run(&self, _: &mut Doc, _: NoParams) -> Result<Outcome<(), Doc>, TestError> {
            Ok(Outcome::undoable((), FailRedo))
        }
    }
    let mut registry = Registry::new();
    registry.register(Once).unwrap();
    let mut session = Session::new(Doc::default(), registry);
    session.execute("doc.once", json!({})).unwrap();
    session.undo().unwrap();

    let error = session.redo().unwrap_err();

    assert!(
        matches!(&error, CommandError::RedoFailed { command, .. } if command.as_str() == "doc.once"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "cannot redo doc.once: the test command refused"
    );
    assert!(std::error::Error::source(&error).is_some());
    assert_eq!((session.undo_depth(), session.redo_depth()), (0, 1));
}

// The registry: every command is listable with its schema.

#[test]
fn every_command_is_listed_with_its_schema() {
    let mut session = session(SessionSpec::default());
    let listed = session.execute("commands.list", json!({})).unwrap();
    let listed = listed.as_array().unwrap();

    let ids: Vec<&str> = listed.iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        [
            "commands.list",
            "doc.append",
            "doc.bad_output",
            "doc.boom",
            "doc.boom_on_undo",
            "doc.fail",
            "doc.flaky",
            "doc.len",
            "edit.redo",
            "edit.undo",
        ]
    );
    let registry_ids: Vec<&str> = session
        .registry()
        .commands()
        .map(|c| c.id().as_str())
        .collect();
    assert_eq!(ids, registry_ids);
    for command in listed {
        assert_eq!(command["params"]["type"], "object", "{command}");
        assert_eq!(
            command["params"]["$schema"], "https://json-schema.org/draft/2020-12/schema",
            "{command}"
        );
        assert!(!command["summary"].as_str().unwrap().is_empty());
    }
    let append = &listed[1];
    assert_eq!(append["summary"], "Appends text.");
    assert_eq!(append["params"]["required"], json!(["text"]));
    assert_eq!(append["params"]["additionalProperties"], json!(false));
    assert_eq!(
        append["params"]["properties"]["text"],
        json!({ "type": "string", "description": "The text to add at the end." })
    );
}

#[test]
fn no_params_is_the_schema_mcp_recommends() {
    let registry = Registry::<Doc>::new();
    for id in BUILTIN_IDS {
        let info = registry.get(id).unwrap();
        assert_eq!(info.id().as_str(), id);
        assert_eq!(info.params()["type"], "object");
        assert_eq!(info.params()["additionalProperties"], json!(false));
        assert!(!info.summary().is_empty());
    }
    assert!(registry.get("doc.append").is_none());
}

#[test]
fn a_new_registry_holds_the_builtins_in_id_order() {
    let registry = Registry::<Doc>::default();
    let ids: Vec<&str> = registry.commands().map(|c| c.id().as_str()).collect();
    assert_eq!(ids, BUILTIN_IDS);
    let summaries: Vec<&str> = registry.commands().map(|c| c.summary()).collect();
    assert_eq!(
        summaries,
        [
            "Lists every command with its id, summary and parameter schema.",
            "Makes the most recently undone change again.",
            "Takes back the most recent change that has not been undone.",
        ]
    );
    for id in BUILTIN_IDS {
        assert_eq!(CommandId::new(id).unwrap().as_str(), id);
    }
}

#[test]
fn registering_a_taken_id_fails_and_keeps_the_first() {
    let mut registry = Registry::<Doc>::new();
    registry.register(Append).unwrap();
    struct Impostor;
    impl Command<Doc> for Impostor {
        const ID: &'static str = "doc.append";
        const SUMMARY: &'static str = "Not the real one.";
        type Params = NoParams;
        type Output = ();
        type Error = TestError;
        fn run(&self, _: &mut Doc, _: NoParams) -> Result<Outcome<(), Doc>, TestError> {
            Ok(Outcome::unchanged(()))
        }
    }

    let error = registry.register(Impostor).unwrap_err();

    assert!(
        matches!(&error, RegisterError::Duplicate(id) if id.as_str() == "doc.append"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "command doc.append is already registered"
    );
    assert!(std::error::Error::source(&error).is_none());
    assert_eq!(
        registry.get("doc.append").unwrap().summary(),
        "Appends text."
    );
}

#[test]
fn a_builtin_id_cannot_be_registered_again() {
    struct Undo;
    impl Command<Doc> for Undo {
        const ID: &'static str = "edit.undo";
        const SUMMARY: &'static str = "Not the real one.";
        type Params = NoParams;
        type Output = ();
        type Error = TestError;
        fn run(&self, _: &mut Doc, _: NoParams) -> Result<Outcome<(), Doc>, TestError> {
            Ok(Outcome::unchanged(()))
        }
    }
    let mut registry = Registry::<Doc>::new();
    assert!(matches!(
        registry.register(Undo),
        Err(RegisterError::Duplicate(_))
    ));
}

#[test]
fn a_malformed_id_cannot_be_registered() {
    struct Shouty;
    impl Command<Doc> for Shouty {
        const ID: &'static str = "Doc.Append";
        const SUMMARY: &'static str = "Badly named.";
        type Params = NoParams;
        type Output = ();
        type Error = TestError;
        fn run(&self, _: &mut Doc, _: NoParams) -> Result<Outcome<(), Doc>, TestError> {
            Ok(Outcome::unchanged(()))
        }
    }
    let mut registry = Registry::<Doc>::new();

    let error = registry.register(Shouty).unwrap_err();

    assert!(
        matches!(
            error,
            RegisterError::InvalidId {
                id: "Doc.Append",
                error: IdError::BadSegmentStart { found: 'D' }
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "invalid command id \"Doc.Append\": a command id segment starts with a lowercase \
         letter, not 'D'"
    );
    assert!(std::error::Error::source(&error).is_some());
    assert_eq!(registry.commands().count(), BUILTIN_IDS.len());
}

#[test]
fn parameters_that_are_not_an_object_cannot_be_registered() {
    struct Scalar;
    impl Command<Doc> for Scalar {
        const ID: &'static str = "doc.scalar";
        const SUMMARY: &'static str = "Takes a bare number.";
        type Params = u32;
        type Output = ();
        type Error = TestError;
        fn run(&self, _: &mut Doc, _: u32) -> Result<Outcome<(), Doc>, TestError> {
            Ok(Outcome::unchanged(()))
        }
    }
    let mut registry = Registry::<Doc>::new();

    let error = registry.register(Scalar).unwrap_err();

    assert!(
        matches!(&error, RegisterError::ParamsNotObject(id) if id.as_str() == "doc.scalar"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "the parameters of command doc.scalar are not a JSON object"
    );
    assert!(registry.get("doc.scalar").is_none());
}

#[test]
fn the_reference_lists_every_command_with_its_schema() {
    let session = session(SessionSpec::default());
    let markdown = session.registry().markdown();

    assert!(markdown.starts_with("<!-- Generated by `cargo xtask commands`"));
    for info in session.registry().commands() {
        let id = info.id();
        let anchor = id.as_str().replace('.', "");
        assert!(markdown.contains(&format!("| [`{id}`](#{anchor}) | {} |\n", info.summary())));
        let schema = serde_json::to_string_pretty(info.params()).unwrap();
        assert!(markdown.contains(&format!(
            "\n## `{id}`\n\n{}\n\nParameters:\n\n```json\n{schema}\n```\n",
            info.summary()
        )));
    }
    let table = markdown.find("| [`commands.list`]").unwrap();
    let first = markdown.find("## `commands.list`").unwrap();
    let last = markdown.find("## `edit.undo`").unwrap();
    assert!(table < first && first < last);
}

// Command ids.

#[test]
fn malformed_ids_are_rejected_with_the_reason() {
    let too_long = "a".repeat(MAX_ID_LEN + 1);
    let cases: [(&str, IdError); 9] = [
        ("", IdError::Empty),
        (
            &too_long,
            IdError::TooLong {
                len: MAX_ID_LEN + 1,
            },
        ),
        (".a", IdError::EmptySegment),
        ("a.", IdError::EmptySegment),
        ("a..b", IdError::EmptySegment),
        ("a.1b", IdError::BadSegmentStart { found: '1' }),
        ("_a", IdError::BadSegmentStart { found: '_' }),
        ("a-b", IdError::BadCharacter { found: '-' }),
        ("aB", IdError::BadCharacter { found: 'B' }),
    ];
    for (text, expected) in cases {
        assert_eq!(CommandId::new(text), Err(expected), "{text:?}");
    }
    assert_eq!(IdError::Empty.to_string(), "a command id cannot be empty");
    assert_eq!(
        IdError::TooLong { len: 129 }.to_string(),
        "a command id is at most 128 bytes, not 129"
    );
    assert_eq!(
        IdError::EmptySegment.to_string(),
        "a command id has an empty segment"
    );
    assert_eq!(
        IdError::BadCharacter { found: '-' }.to_string(),
        "a command id holds only lowercase letters, digits, '_' and '.', not '-'"
    );
}

#[test]
fn ids_at_the_length_limit_are_accepted() {
    let longest = "a".repeat(MAX_ID_LEN);
    assert_eq!(CommandId::new(&longest).unwrap().as_str(), longest);
    for text in ["a", "compile", "edit.undo", "a1_b.c_2", "vm.run"] {
        let id = CommandId::new(text).unwrap();
        assert_eq!((id.as_str(), id.to_string()), (text, text.to_owned()));
    }
}

proptest! {
    #[test]
    fn every_id_in_the_grammar_is_accepted(
        text in "[a-z][a-z0-9_]{0,15}(\\.[a-z][a-z0-9_]{0,15}){0,5}"
    ) {
        let id = CommandId::new(&text).unwrap();
        prop_assert_eq!(id.as_str(), text.as_str());
        prop_assert_eq!(serde_json::to_value(&id).unwrap(), json!(text));
    }

    #[test]
    fn an_id_with_a_character_outside_the_grammar_is_rejected(
        prefix in "[a-z]{1,8}",
        bad in any::<char>().prop_filter("outside the grammar", |c| {
            !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_' || *c == '.')
        }),
        suffix in "[a-z0-9_]{0,8}",
    ) {
        let text = format!("{prefix}{bad}{suffix}");
        prop_assert!(CommandId::new(&text).is_err());
    }
}
