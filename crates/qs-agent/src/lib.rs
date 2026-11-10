// SPDX-License-Identifier: GPL-3.0-or-later

//! The agent loop (P4-06, AGT-1, AGT-5, PRIN-4): a model plans, edits, compiles, tests and
//! previews through the IDE's commands, and proposes its edits as staged hunks for a person
//! to accept. The model is any [`Model`]; this crate is the runtime around it.
//!
//! A run:
//!
//! 1. records a checkpoint of the project (`checkpoint.create`), so the run can be undone
//!    (AGT-4);
//! 2. gives the model the task and the agent's tools ([`AGENT_TOOLS`]), and runs each tool call
//!    it makes through the command session, answering the result;
//! 3. when the model says it is finished, checks the work itself: `compile` must be clean and
//!    `project.test` must pass, unit tests and scenarios alike (AGT-5). If either fails, the
//!    model is told why and the run goes on; a run is never marked done otherwise;
//! 4. ends done, or not done with the reason, after at most [`Limits::turns`] turns, leaving
//!    its edits staged for review (`edit.staged`).
//!
//! The model never accepts, rejects or saves its own edits, and never restores a checkpoint:
//! those are the person's (PRIN-4), so they are not among its tools.

use qs_commands::Session;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The commands the built-in agent may call: reading the project, staging edits, compiling,
/// testing, running QuakeC and scenarios, and the running game.
pub const AGENT_TOOLS: [&str; 20] = [
    "project.symbols",
    "project.check",
    "file.read",
    "symbol.definition",
    "symbol.references",
    "symbol.hover",
    "engine.builtins",
    "edit.apply",
    "edit.staged",
    "compile",
    "project.test",
    "vm.run",
    "scenario.run",
    "checkpoint.list",
    "preview.play",
    "preview.reload",
    "preview.stop",
    "engine.inspect",
    "engine.console",
    "engine.screenshot",
];

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
    /// The task, or the runtime telling the model why it is not done.
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
    /// The next step for `transcript`, `tools` being the tools it may call.
    ///
    /// # Errors
    ///
    /// The model's own error, as text, which ends the run.
    fn reply(&mut self, tools: &[ToolSpec], transcript: &[Message]) -> Result<Reply, String>;
}

/// How far a run may go and how its work is checked.
#[derive(Clone, Debug, PartialEq)]
pub struct Limits {
    /// The most turns of the model, its finishing turns included.
    pub turns: usize,
    /// The parameters `project.test` is run with when the model finishes, such as a game
    /// folder for the scenarios.
    pub test_params: Value,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            turns: 40,
            test_params: json!({}),
        }
    }
}

/// What the runtime found when the model said it was finished (AGT-5).
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
    /// Every turn.
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

/// The tools of [`AGENT_TOOLS`] `session`'s registry has.
#[must_use]
pub fn tools<C: 'static>(session: &Session<C>) -> Vec<ToolSpec> {
    AGENT_TOOLS
        .iter()
        .filter_map(|id| session.registry().get(id))
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
    limits: &Limits,
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
        text: task.to_owned(),
    }];
    let mut verification = None;
    let mut outcome = None;
    for _ in 0..limits.turns {
        match model
            .reply(&specs, &transcript)
            .map_err(AgentError::Model)?
        {
            Reply::Calls { text, calls } => {
                transcript.push(Message::Assistant {
                    text,
                    calls: calls.clone(),
                });
                for call in calls {
                    transcript.push(call_tool(session, &specs, &call));
                }
            }
            Reply::Finish { summary } => {
                transcript.push(Message::Assistant {
                    text: summary.clone(),
                    calls: Vec::new(),
                });
                let checked = verify(session, &limits.test_params);
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
                    limits.turns
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

/// The result of `call`: the command's output, or why it did not run.
fn call_tool<C: 'static>(session: &mut Session<C>, specs: &[ToolSpec], call: &ToolCall) -> Message {
    let (content, is_error) = if specs.iter().any(|spec| spec.name == call.name) {
        match session.execute(&call.name, call.arguments.clone()) {
            Ok(output) => (output, false),
            Err(error) => (json!(error.to_string()), true),
        }
    } else {
        (
            json!(format!("{} is not one of your tools", call.name)),
            true,
        )
    };
    Message::Tool {
        call_id: call.id.clone(),
        content,
        is_error,
    }
}

/// Checks the run's work as AGT-5 says: a clean compile and passing tests.
fn verify<C: 'static>(session: &mut Session<C>, test_params: &Value) -> Verification {
    let mut checked = Verification::default();
    match session.execute("compile", json!({})) {
        Ok(compiled) if compiled.get("clean") == Some(&json!(true)) => {
            checked.compile_clean = true;
        }
        Ok(compiled) => checked.reasons.push(format!(
            "compile is not clean: {} errors",
            compiled.get("errors").and_then(Value::as_u64).unwrap_or(0)
        )),
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

#[cfg(test)]
mod tests;
