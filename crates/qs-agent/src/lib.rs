// SPDX-License-Identifier: GPL-3.0-or-later

//! The agent loop (P4-06, AGT-1, AGT-5, PRIN-4): a model plans, edits, compiles, tests and
//! previews through the IDE's commands, and proposes its edits as staged hunks for a person
//! to accept. The model is any [`Model`]; this crate is the runtime around it, and owns what
//! makes a first pass productive and safe:
//!
//! - **Every tool.** The model may call every command of the registry but those in
//!   [`DENIED`]: what is the person's (accepting or rejecting hunks, saving, restoring a
//!   checkpoint), what would write around the staged edits (`file.update`, `symbol.rename`),
//!   and what only drives the window (`view.*`, tabs, the palette).
//! - **A brief before the first turn.** The runtime records a checkpoint, compiles the project
//!   and runs its tests, and gives the model the task with the project's files, its problems
//!   and its tests as they are, so it starts from the facts instead of exploring for them.
//! - **Rules it works under** ([`SYSTEM`]): propose edits with `edit.apply`, read before
//!   writing, compile and test before finishing.
//! - **Guardrails.** A relative path is taken from the project; an edit outside the project is
//!   refused; a tool's output longer than [`RunConfig::output_limit`] is cut, saying so; the
//!   run has at most [`RunConfig::turns`] turns.
//! - **Done means done** (AGT-5). When the model says it is finished, the runtime compiles and
//!   runs the tests itself; the run is done only when the compile is clean and every test,
//!   unit and scenario, passes. Otherwise the model is told why, one reason a line, and goes
//!   on.
//!
//! The run ends with its edits staged for review (`edit.staged`), and the checkpoint it
//! recorded first, so the person can accept the hunks or undo everything (AGT-4, PRIN-4).

use std::path::{Component, Path, PathBuf};

use qs_commands::Session;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The commands the agent may not call: the person's decisions, writes around the staged
/// edits, and the window's own controls. Any command whose id starts with `view.` is denied
/// too.
pub const DENIED: [&str; 17] = [
    "edit.accept",
    "edit.reject",
    "file.save",
    "checkpoint.restore",
    "checkpoint.create",
    "file.update",
    "symbol.rename",
    "project.open",
    "project.take_checked",
    "file.open",
    "file.close",
    "edit.undo",
    "edit.redo",
    "commands.list",
    "preview.dock",
    "preview.pick",
    "preview.choose_game",
];

/// The rules the model works under, before the task.
pub const SYSTEM: &str = "You are QuakeC Studio's agent. You change a QuakeC project to do \
    the task, using the tools, which are the IDE's own commands.\n\
    - Read before you write: use project.files, project.search, file.read and the symbol tools \
    to find the code the task touches.\n\
    - Propose every change with edit.apply, giving each file's whole new text. Your changes \
    are staged for the person to review; you cannot accept, save or undo them, and nothing \
    you do is written to disk.\n\
    - Check your work: compile must be clean and project.test must pass. Add a test_ function \
    (it fails by calling error) or a tests/*.scenario when the task can be tested.\n\
    - Paths are the project's, absolute or relative to its directory.\n\
    - When you are finished, say so. The runtime compiles and runs the tests itself, and the \
    run is done only when both pass; if not, you are told why and go on.";

/// A tool the model may call: a command, as the registry describes it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolSpec {
    /// The command's id.
    pub name: String,
    /// What it does.
    pub description: String,
    /// The JSON Schema of its arguments.
    pub input_schema: Value,
}

/// One call the model asks for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// The model's id for the call, which its result answers.
    pub id: String,
    /// The tool.
    pub name: String,
    /// Its arguments.
    pub arguments: Value,
}

/// One entry of a run's transcript.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Message {
    /// The task with its brief, or the runtime telling the model why it is not done.
    User {
        /// The text.
        text: String,
    },
    /// What the model said and the calls it made.
    Assistant {
        /// Its text.
        text: String,
        /// Its calls.
        calls: Vec<ToolCall>,
    },
    /// A call's result.
    Tool {
        /// The call it answers.
        call_id: String,
        /// The command's output, or its error.
        content: Value,
        /// Whether the call failed.
        is_error: bool,
    },
}

/// What the model answers on its turn.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    /// Calls to make, with what it says about them.
    Calls {
        /// Its text.
        text: String,
        /// The calls, made in order.
        calls: Vec<ToolCall>,
    },
    /// It says the task is done.
    Finish {
        /// Its summary of the work.
        summary: String,
    },
}

/// A model: anything that answers a transcript with its next step.
pub trait Model {
    /// The next step for `transcript`, under the rules of `system`, `tools` being the tools
    /// it may call.
    ///
    /// # Errors
    ///
    /// The model's own error, as text, which ends the run.
    fn reply(
        &mut self,
        system: &str,
        tools: &[ToolSpec],
        transcript: &[Message],
    ) -> Result<Reply, String>;
}

/// What a run works on and how far it may go.
#[derive(Clone, Debug, PartialEq)]
pub struct RunConfig {
    /// The project's directory: relative paths are taken from it, and edits must lie under it.
    pub project: PathBuf,
    /// The most turns of the model, its finishing turns included.
    pub turns: usize,
    /// The parameters `project.test` is run with, such as a game folder for the scenarios.
    pub test_params: Value,
    /// The most characters of a tool's output the model is given; the rest is cut.
    pub output_limit: usize,
}

impl RunConfig {
    /// A run over the project at `project`, with 40 turns, the tests run with no parameters
    /// and outputs cut at 20 000 characters.
    #[must_use]
    pub fn new(project: impl Into<PathBuf>) -> Self {
        Self {
            project: project.into(),
            turns: 40,
            test_params: json!({}),
            output_limit: 20_000,
        }
    }
}

/// What the runtime found when it checked the work (AGT-5).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Verification {
    /// `compile` answered clean.
    pub compile_clean: bool,
    /// `project.test` answered passed: no test, unit or scenario, failed.
    pub tests_passed: bool,
    /// Why the run is not done, one line each; empty when it is.
    pub reasons: Vec<String>,
}

impl Verification {
    /// Whether the work may be marked done.
    #[must_use]
    pub const fn done(&self) -> bool {
        self.compile_clean && self.tests_passed
    }
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// The compile is clean and the tests pass.
    Done {
        /// The model's summary.
        summary: String,
    },
    /// The run used its turns before its work passed.
    NotDone {
        /// Why: the last check's reasons, or that the model never finished.
        reasons: Vec<String>,
    },
}

/// What a run did.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RunReport {
    /// How it ended.
    pub outcome: Outcome,
    /// The checkpoint recorded before it changed anything.
    pub checkpoint: Option<u32>,
    /// The edits it proposes, staged for review: `edit.staged`'s answer.
    pub staged: Value,
    /// The last check of its work, if the model finished at least once.
    pub verification: Option<Verification>,
    /// Every turn, the brief first.
    pub transcript: Vec<Message>,
}

/// Why a run did not run.
#[derive(Debug)]
pub enum AgentError {
    /// The model failed.
    Model(String),
    /// The checkpoint before the run could not be recorded.
    Checkpoint(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Model(why) => write!(f, "the model failed: {why}"),
            Self::Checkpoint(why) => write!(f, "no checkpoint could be recorded: {why}"),
        }
    }
}

impl std::error::Error for AgentError {}

/// Whether the agent may call the command `id`.
#[must_use]
pub fn allowed(id: &str) -> bool {
    !DENIED.contains(&id) && !id.starts_with("view.")
}

/// Every command of `session`'s registry the agent may call.
#[must_use]
pub fn tools<C: 'static>(session: &Session<C>) -> Vec<ToolSpec> {
    session
        .registry()
        .commands()
        .filter(|info| allowed(info.id().as_str()))
        .map(|info| ToolSpec {
            name: info.id().as_str().to_owned(),
            description: info.summary().to_owned(),
            input_schema: info.params().clone(),
        })
        .collect()
}

/// Runs `task` with `model` over `session`, whose project is open, as the module says.
///
/// # Errors
///
/// [`AgentError::Checkpoint`] when the checkpoint cannot be recorded, and
/// [`AgentError::Model`] when the model fails. A run whose work does not pass is not an error:
/// it ends [`Outcome::NotDone`].
pub fn run<C: 'static>(
    session: &mut Session<C>,
    model: &mut dyn Model,
    task: &str,
    config: &RunConfig,
) -> Result<RunReport, AgentError> {
    let made = session
        .execute(
            "checkpoint.create",
            json!({ "label": format!("agent: {task}") }),
        )
        .map_err(|error| AgentError::Checkpoint(error.to_string()))?;
    let checkpoint = made
        .get("number")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok());
    let specs = tools(session);
    let mut transcript = vec![Message::User {
        text: brief(session, task, config),
    }];
    let mut verification = None;
    let mut outcome = None;
    for _ in 0..config.turns {
        match model
            .reply(SYSTEM, &specs, &transcript)
            .map_err(AgentError::Model)?
        {
            Reply::Calls { text, calls } => {
                transcript.push(Message::Assistant {
                    text,
                    calls: calls.clone(),
                });
                for call in calls {
                    transcript.push(call_tool(session, &specs, &call, config));
                }
            }
            Reply::Finish { summary } => {
                transcript.push(Message::Assistant {
                    text: summary.clone(),
                    calls: Vec::new(),
                });
                let checked = verify(session, &config.test_params);
                let done = checked.done();
                if !done {
                    transcript.push(Message::User {
                        text: format!(
                            "Not done. Fix these, then finish again:\n{}",
                            checked.reasons.join("\n")
                        ),
                    });
                }
                verification = Some(checked);
                if done {
                    outcome = Some(Outcome::Done { summary });
                    break;
                }
            }
        }
    }
    let outcome = outcome.unwrap_or_else(|| Outcome::NotDone {
        reasons: verification.as_ref().map_or_else(
            || {
                vec![format!(
                    "the model did not finish in {} turns",
                    config.turns
                )]
            },
            |checked| checked.reasons.clone(),
        ),
    });
    let staged = session
        .execute("edit.staged", json!({}))
        .unwrap_or(Value::Null);
    Ok(RunReport {
        outcome,
        checkpoint,
        staged,
        verification,
        transcript,
    })
}

/// The first message: the task, then the project as it is: its files, the compile's problems
/// and the tests' verdicts.
fn brief<C: 'static>(session: &mut Session<C>, task: &str, config: &RunConfig) -> String {
    let mut text = format!(
        "Task: {task}\n\nThe project is at {}.\n",
        config.project.display()
    );
    if let Ok(files) = session.execute("project.files", json!({}))
        && let Some(files) = files.as_array()
    {
        text.push_str(&format!("\nIts {} source files:\n", files.len()));
        for file in files.iter().filter_map(Value::as_str) {
            let shown = Path::new(file)
                .strip_prefix(&config.project)
                .map_or_else(|_| file.to_owned(), |p| p.display().to_string());
            text.push_str(&format!("- {shown}\n"));
        }
    }
    let checked = verify(session, &config.test_params);
    text.push_str(&format!(
        "\nAs it is now, the compile is {} and the tests {}.\n",
        if checked.compile_clean {
            "clean"
        } else {
            "not clean"
        },
        if checked.tests_passed {
            "pass"
        } else {
            "do not pass"
        }
    ));
    for reason in &checked.reasons {
        text.push_str(&format!("- {reason}\n"));
    }
    text
}

/// The result of `call`: the command's output, or why it did not run.
fn call_tool<C: 'static>(
    session: &mut Session<C>,
    specs: &[ToolSpec],
    call: &ToolCall,
    config: &RunConfig,
) -> Message {
    let refused = |why: String| Message::Tool {
        call_id: call.id.clone(),
        content: json!(why),
        is_error: true,
    };
    if !specs.iter().any(|spec| spec.name == call.name) {
        return refused(format!("{} is not one of your tools", call.name));
    }
    let mut arguments = call.arguments.clone();
    absolute_paths(&mut arguments, &config.project);
    if call.name == "edit.apply" {
        let mut paths = Vec::new();
        collect_paths(&arguments, &mut paths);
        let project = lexical(&config.project);
        if let Some(outside) = paths.iter().find(|p| !Path::new(p).starts_with(&project)) {
            return refused(format!(
                "{outside} is outside the project at {}; edits stay in the project",
                project.display()
            ));
        }
    }
    let (content, is_error) = match session.execute(&call.name, arguments) {
        Ok(output) => (cut(output, config.output_limit), false),
        Err(error) => (json!(error.to_string()), true),
    };
    Message::Tool {
        call_id: call.id.clone(),
        content,
        is_error,
    }
}

/// `output`, or, when its JSON is longer than `limit` characters, its first `limit`
/// characters as text, saying it was cut.
fn cut(output: Value, limit: usize) -> Value {
    let text = output.to_string();
    if text.chars().count() <= limit {
        return output;
    }
    let kept: String = text.chars().take(limit).collect();
    json!(format!(
        "{kept}\n[cut: the output was {} characters, the first {limit} shown; ask for less]",
        text.chars().count()
    ))
}

/// Checks the work as AGT-5 says: a clean compile and passing tests.
fn verify<C: 'static>(session: &mut Session<C>, test_params: &Value) -> Verification {
    let mut checked = Verification::default();
    match session.execute("compile", json!({})) {
        Ok(compiled) if compiled.get("clean") == Some(&json!(true)) => {
            checked.compile_clean = true;
        }
        Ok(compiled) => {
            checked.reasons.push(format!(
                "compile is not clean: {} errors",
                compiled.get("errors").and_then(Value::as_u64).unwrap_or(0)
            ));
            let problems = compiled
                .get("modules")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|module| module.get("problems").and_then(Value::as_array))
                .flatten()
                .filter(|problem| problem.get("gravity") == Some(&json!("error")));
            for problem in problems {
                let message = problem.get("message").and_then(Value::as_str).unwrap_or("");
                let place = problem.get("place");
                let path = place
                    .and_then(|p| p.get("path"))
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                let line = place
                    .and_then(|p| p.get("location"))
                    .and_then(|l| l.get("line"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                checked
                    .reasons
                    .push(format!("error at {path}:{line}: {message}"));
            }
        }
        Err(error) => checked
            .reasons
            .push(format!("compile did not run: {error}")),
    }
    match session.execute("project.test", test_params.clone()) {
        Ok(run) if run.get("passed") == Some(&json!(true)) => checked.tests_passed = true,
        Ok(run) => {
            let failed = run
                .get("tests")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|test| test.get("verdict") == Some(&json!("failed")));
            for test in failed {
                let name = test.get("name").and_then(Value::as_str).unwrap_or("?");
                let message = test.get("message").and_then(Value::as_str).unwrap_or("");
                checked
                    .reasons
                    .push(format!("test {name} failed: {message}"));
            }
        }
        Err(error) => checked
            .reasons
            .push(format!("the tests did not run: {error}")),
    }
    checked
}

/// Every string under a key named `path`, or ending in `_path`, anywhere in `value`.
fn collect_paths(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, inner) in object {
                if is_path_key(key)
                    && let Some(path) = inner.as_str()
                {
                    out.push(path.to_owned());
                }
                collect_paths(inner, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_paths(item, out)),
        _ => {}
    }
}

/// Whether a parameter named `key` holds a path.
fn is_path_key(key: &str) -> bool {
    key == "path" || key.ends_with("_path")
}

/// Makes every path `arguments` names absolute, a relative one taken from `project`, with `.`
/// and `..` worked out.
fn absolute_paths(arguments: &mut Value, project: &Path) {
    match arguments {
        Value::Object(object) => {
            for (key, inner) in object.iter_mut() {
                if is_path_key(key)
                    && let Some(path) = inner.as_str()
                {
                    let full = lexical(&project.join(path));
                    *inner = Value::String(full.to_string_lossy().into_owned());
                } else {
                    absolute_paths(inner, project);
                }
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| absolute_paths(item, project)),
        _ => {}
    }
}

/// `path` with `.` and `..` worked out by its text.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests;
