// SPDX-License-Identifier: GPL-3.0-or-later

//! A thread run by an external agent over the Agent Client Protocol (P4-14, UI-11): the agent
//! is a subprocess (`qs_acp::Agent`) that brings its own model and account, and the thread
//! shows what it says and does. The IDE's own tools reach it over MCP, and its file reads and
//! writes come through the IDE: a read gets the editor's text, unsaved changes included, and a
//! write is staged as hunks to accept or reject, as the built-in agent's are (AGT-3).

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText, Ui};
use qs_acp::{
    AcpError, Agent, AgentCommand, ClientInfo, Event, FileRequestId, Files, McpServer, OptionKind,
    Outcome, PermissionId, PermissionOption, SessionId, StopReason, ToolCall, ToolStatus,
};
use qs_agent::CheckpointNumber;
use qs_commands::Session;
use qs_settings::ExternalAgent;
use qs_workbench::{APPLY_EDIT, READ_FILE};
use serde_json::json;

use super::{CALL, FAILED, RAN, THREAD_MAX_HEIGHT, muted};
use crate::strings::Text;
use crate::theme::{Theme, Token};

/// The version of Claude Code's ACP adapter the panel's Use Claude Code runs: the one the
/// client was checked against (2026-11-20).
const CLAUDE_ADAPTER: &str = "@agentclientprotocol/claude-agent-acp@0.109.0";

/// The command line program beside the app, which serves the IDE's tools over MCP.
const TOOLS_PROGRAM: &str = "quakec-ide";

/// The entry Use Claude Code adds to `agent.external`: Claude Code through its ACP adapter,
/// run by `npx`, which needs Node and the `claude` command signed in.
pub(crate) fn claude_code() -> ExternalAgent {
    ExternalAgent {
        name: Text::AgentClaudeCode.get().to_owned(),
        command: "npx".to_owned(),
        args: vec!["-y".to_owned(), CLAUDE_ADAPTER.to_owned()],
        env: std::collections::BTreeMap::new(),
    }
}

/// The IDE's tools as an MCP server for the agent of a thread in `project`: the command line
/// program beside the app, connected to this window (AGT-7); none when the program is not
/// there, as in a build that made only the app.
pub(crate) fn ide_tools(project: &Path) -> Vec<McpServer> {
    let program = std::env::current_exe()
        .ok()
        .and_then(|app| Some(app.parent()?.join(TOOLS_PROGRAM)))
        .filter(|program| program.is_file());
    program
        .into_iter()
        .map(|command| McpServer {
            name: Text::AppName.get().to_owned(),
            command,
            args: vec![
                "mcp".to_owned(),
                project.to_string_lossy().into_owned(),
                "--connect".to_owned(),
            ],
            env: Vec::new(),
        })
        .collect()
}

/// One thing a thread shows, in the order it came.
#[derive(Debug, PartialEq, Eq)]
enum Item {
    /// What the agent said; pieces that follow each other are one.
    Said(String),
    /// What the agent reasoned; pieces that follow each other are one.
    Thought(String),
    /// A tool call, with the changes reported since folded in.
    Tool(ToolCall),
}

/// Where a thread is.
#[derive(Debug, PartialEq, Eq)]
enum State {
    /// The agent is starting: no session yet.
    Starting,
    /// A turn is running.
    Working,
    /// The turn ended.
    Ended(StopReason),
    /// The thread cannot go on, and why.
    Failed(String),
}

/// A permission request that waits for the modder.
#[derive(Debug)]
struct Asked {
    request: PermissionId,
    tool: ToolCall,
    options: Vec<PermissionOption>,
}

/// A thread an external agent runs.
#[derive(Debug)]
pub(crate) struct ExternalThread {
    /// What the modder asked.
    task: String,
    /// The project it works in.
    project: PathBuf,
    /// The agent's name, as the settings give it.
    name: String,
    /// The agent; `None` once the thread is over and the process is ended.
    agent: Option<Agent>,
    /// The MCP servers the session is made with.
    servers: Vec<McpServer>,
    session: Option<SessionId>,
    items: Vec<Item>,
    asked: Option<Asked>,
    state: State,
    /// The checkpoint recorded before the agent started, which puts the project back: an
    /// agent that writes the disk itself, as Claude Code does, leaves no hunks to reject.
    checkpoint: Option<CheckpointNumber>,
}

impl ExternalThread {
    /// Starts `agent` in `project` on `task`; `wake` is called whenever the agent has said
    /// something.
    ///
    /// # Errors
    ///
    /// Why the agent could not be started.
    pub(crate) fn start(
        agent: &ExternalAgent,
        project: &Path,
        task: &str,
        (servers, checkpoint): (Vec<McpServer>, Option<CheckpointNumber>),
        wake: impl Fn() + Send + 'static,
    ) -> Result<Self, String> {
        let command = AgentCommand {
            program: PathBuf::from(&agent.command),
            args: agent.args.clone(),
            env: agent.env.clone().into_iter().collect(),
        };
        let mut running = Agent::spawn(&command, project, wake).map_err(|e| e.to_string())?;
        let info = ClientInfo {
            name: env!("CARGO_PKG_NAME").to_owned(),
            title: Text::AppName.get().to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        running
            .client()
            .initialize(&info, Files::ThroughClient)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            task: task.to_owned(),
            project: project.to_path_buf(),
            name: agent.name.clone(),
            agent: Some(running),
            servers,
            session: None,
            items: Vec::new(),
            asked: None,
            state: State::Starting,
            checkpoint,
        })
    }

    /// What the modder asked.
    pub(crate) fn task(&self) -> &str {
        &self.task
    }

    /// Whether the agent is still starting or working.
    pub(crate) const fn running(&self) -> bool {
        matches!(self.state, State::Starting | State::Working)
    }

    /// Takes what the agent said since the last call: the handshake goes on, what it says is
    /// kept to show, and its file requests are carried out in `session`. Whether anything
    /// came.
    pub(crate) fn pump<C: 'static>(&mut self, session: &mut Session<C>) -> bool {
        let events = self.agent.as_mut().map_or_default(Agent::poll);
        self.take_all(session, events)
    }

    /// As [`Self::pump`], after waiting for the agent to say something. For tests.
    #[cfg(test)]
    pub(crate) fn wait<C: 'static>(&mut self, session: &mut Session<C>) -> bool {
        let events = self.agent.as_mut().map_or_default(Agent::wait);
        self.take_all(session, events)
    }

    /// The option names of the permission request that waits, if one does. For tests.
    #[cfg(test)]
    pub(crate) fn asking(&self) -> Option<Vec<String>> {
        let asked = self.asked.as_ref()?;
        Some(asked.options.iter().map(|o| o.name.clone()).collect())
    }

    /// Answers the permission request that waits with its option `chosen`. For tests.
    #[cfg(test)]
    pub(crate) fn permit_for_test(&mut self, chosen: usize) {
        self.permit(chosen);
    }

    /// The checkpoint recorded before the agent started. For tests.
    #[cfg(test)]
    pub(crate) const fn checkpoint(&self) -> Option<CheckpointNumber> {
        self.checkpoint
    }

    /// Stops the turn, as the Stop button does. For tests.
    #[cfg(test)]
    pub(crate) fn stop_for_test(&mut self) {
        self.stop();
    }

    /// What the thread shows, a line an item, and how it stands. For tests.
    #[cfg(test)]
    pub(crate) fn shown(&self) -> (Vec<String>, String) {
        let items = self
            .items
            .iter()
            .map(|item| match item {
                Item::Said(text) => format!("said: {text}"),
                Item::Thought(text) => format!("thought: {text}"),
                Item::Tool(call) => format!(
                    "tool: {} {:?}",
                    call.title.as_deref().unwrap_or_default(),
                    call.status
                ),
            })
            .collect();
        (items, format!("{:?}", self.state))
    }

    /// Takes each of `events`; whether there were any.
    fn take_all<C: 'static>(&mut self, session: &mut Session<C>, events: Vec<Event>) -> bool {
        let any = !events.is_empty();
        for event in events {
            if let Err(error) = self.take(session, event) {
                self.fail(error.to_string());
            }
        }
        any
    }

    /// Ends the thread with `why`, and the agent's process with it.
    fn fail(&mut self, why: String) {
        self.state = State::Failed(why);
        self.asked = None;
        self.agent = None;
    }

    /// Takes one event.
    fn take<C: 'static>(&mut self, session: &mut Session<C>, event: Event) -> Result<(), AcpError> {
        let Some(agent) = self.agent.as_mut() else {
            return Ok(());
        };
        match event {
            Event::Initialized { .. } => {
                agent.client().new_session(&self.project, &self.servers)?
            }
            Event::SessionReady(id) => {
                agent.client().prompt(&id, &self.task)?;
                self.session = Some(id);
                self.state = State::Working;
            }
            Event::Message(piece) => match self.items.last_mut() {
                Some(Item::Said(text)) => text.push_str(&piece),
                _ => self.items.push(Item::Said(piece)),
            },
            Event::Thought(piece) => match self.items.last_mut() {
                Some(Item::Thought(text)) => text.push_str(&piece),
                _ => self.items.push(Item::Thought(piece)),
            },
            Event::ToolCall(call) | Event::ToolCallUpdate(call) => self.tool(call),
            // The plan is the agent's own note of its steps; its tool calls show the work.
            Event::Plan(_) => {}
            Event::PermissionAsked {
                request,
                tool,
                options,
            } => {
                self.asked = Some(Asked {
                    request,
                    tool,
                    options,
                });
            }
            Event::ReadFile { request, path } => self.read(session, &request, &path)?,
            Event::WriteFile {
                request,
                path,
                content,
            } => self.write(session, &request, &path, &content)?,
            Event::TurnEnded(reason) => {
                self.state = State::Ended(reason);
                self.asked = None;
                self.agent = None;
            }
            Event::Refused { method, message } => self.fail(format!("{method}: {message}")),
            Event::WrongVersion(version) => {
                self.fail(format!("{} {version}", Text::AgentVersion.get()));
            }
            Event::Exited => self.fail(Text::AgentExited.get().to_owned()),
        }
        Ok(())
    }

    /// Folds `call` into the call of its id, or adds it: a later report gives only what
    /// changed (`tool-calls.mdx`: "Only the fields being updated need to be included").
    fn tool(&mut self, call: ToolCall) {
        let known = self.items.iter_mut().find_map(|item| match item {
            Item::Tool(known) if known.id == call.id => Some(known),
            Item::Tool(_) | Item::Said(_) | Item::Thought(_) => None,
        });
        let Some(known) = known else {
            self.items.push(Item::Tool(call));
            return;
        };
        if let Some(title) = call.title.filter(|title| !title.is_empty()) {
            known.title = Some(title);
        }
        if call.status.is_some() {
            known.status = call.status;
        }
        if !call.text.is_empty() {
            known.text = call.text;
        }
        for path in call.paths {
            if !known.paths.contains(&path) {
                known.paths.push(path);
            }
        }
    }

    /// Answers a read: a file of the project with the editor's text (`file.read`), any other
    /// from the disk, which the agent's own tools could read anyway.
    fn read<C: 'static>(
        &mut self,
        session: &mut Session<C>,
        request: &FileRequestId,
        path: &Path,
    ) -> Result<(), AcpError> {
        let Some(agent) = self.agent.as_mut() else {
            return Ok(());
        };
        let text = if path.starts_with(&self.project) {
            session
                .execute(READ_FILE, json!({ "path": path }))
                .map_err(|error| error.to_string())
                .and_then(|answer| {
                    answer
                        .get("text")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .ok_or_else(|| Text::AgentUnread.get().to_owned())
                })
        } else {
            std::fs::read_to_string(path).map_err(|error| error.to_string())
        };
        match text {
            Ok(text) => agent.client().answer_read(request, &text),
            Err(why) => agent.client().refuse_file(request, &why),
        }
    }

    /// Answers a write: a file of the project is staged as hunks for the modder to accept or
    /// reject (`edit.apply`), and nothing reaches the disk until then; a file outside the
    /// project is refused.
    fn write<C: 'static>(
        &mut self,
        session: &mut Session<C>,
        request: &FileRequestId,
        path: &Path,
        content: &str,
    ) -> Result<(), AcpError> {
        let Some(agent) = self.agent.as_mut() else {
            return Ok(());
        };
        if !path.starts_with(&self.project) {
            return agent
                .client()
                .refuse_file(request, Text::AgentOutside.get());
        }
        let staged = session.execute(
            APPLY_EDIT,
            json!({ "files": [{ "path": path, "text": content }] }),
        );
        match staged {
            Ok(_) => agent.client().answer_write(request),
            Err(error) => agent.client().refuse_file(request, &error.to_string()),
        }
    }

    /// Answers the permission request that waits with the option `chosen`.
    fn permit(&mut self, chosen: usize) {
        let (Some(asked), Some(agent)) = (self.asked.take(), self.agent.as_mut()) else {
            return;
        };
        let outcome = asked
            .options
            .get(chosen)
            .map_or(Outcome::Cancelled, |option| {
                Outcome::Selected(option.id.clone())
            });
        if let Err(error) = agent.client().answer(&asked.request, &outcome) {
            self.fail(error.to_string());
        }
    }

    /// Stops the turn: the agent is asked to cancel, and ends the turn itself
    /// (`prompt-turn.mdx`, Cancellation); before there is a session the agent is ended.
    fn stop(&mut self) {
        self.asked = None;
        let (Some(session), Some(agent)) = (&self.session, self.agent.as_mut()) else {
            self.state = State::Ended(StopReason::Cancelled);
            self.agent = None;
            return;
        };
        if let Err(error) = agent.client().cancel(session) {
            self.fail(error.to_string());
        }
    }

    /// Draws the thread: what the agent said and did, the permission it asks for, and how the
    /// turn ended; Stop while it runs, and once it is over Restore checkpoint, whose
    /// checkpoint is returned when it is clicked.
    pub(crate) fn ui(&mut self, ui: &mut Ui, theme: &Theme) -> Option<CheckpointNumber> {
        muted(ui, theme, &self.name);
        egui::ScrollArea::vertical()
            .max_height(THREAD_MAX_HEIGHT)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                muted(ui, theme, &self.task);
                for item in &self.items {
                    item_view(ui, theme, item);
                }
            });
        let mut chosen = None;
        if let Some(asked) = &self.asked {
            chosen = permission_view(ui, theme, asked);
        }
        if let Some(chosen) = chosen {
            self.permit(chosen);
        }
        let ended = match &self.state {
            State::Starting => Some((Text::AgentStarting, Token::TextMuted)),
            State::Working if self.asked.is_none() => Some((Text::AgentWorking, Token::TextMuted)),
            State::Working | State::Failed(_) => None,
            State::Ended(reason) => Some(match reason {
                StopReason::EndTurn => (Text::AgentDone, Token::Text),
                StopReason::Cancelled => (Text::AgentStopped, Token::TextMuted),
                StopReason::MaxTokens | StopReason::MaxTurnRequests => {
                    (Text::AgentHitLimit, Token::StatusWarn)
                }
                StopReason::Refusal => (Text::AgentRefused, Token::StatusWarn),
            }),
        };
        if let Some((text, token)) = ended {
            ui.label(RichText::new(text.get()).color(theme.color(token)));
        }
        if let State::Failed(why) = &self.state {
            ui.add(
                egui::Label::new(RichText::new(why).color(theme.color(Token::StatusError))).wrap(),
            );
        }
        if self.running() && ui.button(Text::AgentStop.get()).clicked() {
            self.stop();
        }
        self.checkpoint
            .filter(|_| !self.running() && ui.button(Text::AgentRestore.get()).clicked())
    }
}

/// One item of a thread.
fn item_view(ui: &mut Ui, theme: &Theme, item: &Item) {
    match item {
        Item::Said(text) => {
            ui.add(egui::Label::new(text.as_str()).wrap());
        }
        Item::Thought(text) => {
            ui.add(
                egui::Label::new(
                    RichText::new(text)
                        .italics()
                        .color(theme.color(Token::TextMuted)),
                )
                .wrap(),
            );
        }
        Item::Tool(call) => {
            let (mark, token) = match call.status {
                Some(ToolStatus::Completed) => (RAN, Token::TextMuted),
                Some(ToolStatus::Failed) => (FAILED, Token::StatusError),
                Some(ToolStatus::Pending | ToolStatus::InProgress) | None => {
                    (CALL, Token::TextMuted)
                }
            };
            let title = call.title.as_deref().unwrap_or_default();
            ui.add(
                egui::Label::new(
                    RichText::new(format!("{mark} {title}"))
                        .monospace()
                        .color(theme.color(token)),
                )
                .wrap(),
            );
        }
    }
}

/// The permission the agent asks for, with its own options as buttons; the index of the one
/// clicked.
fn permission_view(ui: &mut Ui, theme: &Theme, asked: &Asked) -> Option<usize> {
    let mut chosen = None;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let title = asked.tool.title.as_deref().unwrap_or_default();
        ui.add(
            egui::Label::new(
                RichText::new(format!("{} {title}", Text::AgentAsks.get()))
                    .strong()
                    .color(theme.color(Token::StatusWarn)),
            )
            .wrap(),
        );
        for path in &asked.tool.paths {
            ui.label(RichText::new(path.to_string_lossy()).monospace());
        }
        ui.horizontal_wrapped(|ui| {
            for (index, option) in asked.options.iter().enumerate() {
                let button = match option.kind {
                    OptionKind::AllowOnce | OptionKind::AllowAlways => {
                        egui::Button::new(option.name.as_str())
                    }
                    OptionKind::RejectOnce | OptionKind::RejectAlways => egui::Button::new(
                        RichText::new(option.name.as_str()).color(theme.color(Token::TextMuted)),
                    ),
                };
                if ui.add(button).clicked() {
                    chosen = Some(index);
                }
            }
        });
    });
    chosen
}

#[cfg(test)]
mod tests;
