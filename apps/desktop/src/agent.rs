// SPDX-License-Identifier: GPL-3.0-or-later

//! The agent panel (P4-09, UI-5, PRIN-4), modelled on Zed's: threads that run at once, each a
//! [`Run`] the window drives a reply at a time while its model waits on a thread of its own,
//! so the frame never waits on a model; the model's text as it is written, each tool call and
//! its result, the run's outcome, and the checkpoint it recorded first, which restores the
//! project as it was. The edits every thread proposes are staged hunks, accepted or rejected
//! one by one below the threads (AGT-3).

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui::{self, RichText, Ui};
use qs_agent::{Message, Model, Outcome, Reply, Request, Run, RunConfig, RunReport};
use qs_commands::Session;
use qs_models::{
    Exchange, Federated, IdentityToken, KeyStore, Keychain, ProviderConfig, ProviderKind,
    ProviderModel, Secret,
};
use qs_workbench::{HunkId, StagedHunk};

use crate::shell::name_field;
use crate::strings::Text;

#[cfg(test)]
mod tests;

/// Before a thread still running.
const RUNNING: char = '\u{2026}';
/// Before a tool call.
const CALL: char = '\u{2192}';
/// A call that failed.
const FAILED: char = '\u{2717}';
/// A call that ran.
const RAN: char = '\u{2713}';
use crate::theme::{Theme, Token};

/// Makes the model a thread runs on, from the configuration; the window's makes a provider's
/// with its key from the keychain, a test's a scripted one.
pub type MakeModel = Box<dyn Fn(&ProviderConfig) -> Result<Box<dyn Model + Send>, String>>;

/// The file the agent's provider configuration is kept in, under the app's data directory.
#[must_use]
pub fn config_path(data: &Path) -> PathBuf {
    data.join("agent.json")
}

/// The model the window runs on: the provider `config` names, its key read from the keychain.
pub fn provider_model(config: &ProviderConfig) -> Result<Box<dyn Model + Send>, String> {
    let model = ProviderModel::connect(config.clone(), &Keychain).map_err(|e| e.to_string())?;
    Ok(Box::new(model))
}

/// What a thread's model thread sends back.
enum Event {
    /// A piece of the reply's text, as it is written.
    Text(String),
    /// The whole reply.
    Reply(Result<Reply, String>),
}

/// One task the agent works on.
struct Thread {
    task: String,
    run: Option<Run>,
    /// Where requests go to the model's thread; dropped when the run ends, which ends it.
    to_model: Option<mpsc::Sender<Request>>,
    from_model: mpsc::Receiver<Event>,
    /// The reply being written.
    streaming: String,
    report: Option<RunReport>,
    error: Option<String>,
}

impl Thread {
    /// Whether it still waits on its model.
    const fn running(&self) -> bool {
        self.run.is_some()
    }

    /// Takes `event`: a piece of text is shown; a reply is answered in `session` and the next
    /// request sent, or the run ended.
    fn take<C: 'static>(&mut self, session: &mut Session<C>, event: Event) {
        match event {
            Event::Text(piece) => self.streaming.push_str(&piece),
            Event::Reply(reply) => {
                self.streaming.clear();
                let Some(mut run) = self.run.take() else {
                    return;
                };
                if let Err(error) = run.answer(session, reply) {
                    self.error = Some(error.to_string());
                    self.report = Some(run.finish(session));
                    self.to_model = None;
                    return;
                }
                match run.request() {
                    Some(request) => {
                        let sent = self.to_model.as_ref().map(|to| to.send(request));
                        if matches!(sent, Some(Ok(()))) {
                            self.run = Some(run);
                        } else {
                            self.report = Some(run.finish(session));
                            self.to_model = None;
                        }
                    }
                    None => {
                        self.report = Some(run.finish(session));
                        self.to_model = None;
                    }
                }
            }
        }
    }
}

/// The provider form's fields.
struct Setup {
    kind: ProviderKind,
    model: String,
    base_url: String,
    /// A key typed and not yet saved; never shown again once saved.
    key: String,
    /// Workload identity in place of the key (P4-38), with its fields.
    federated: bool,
    identity_file: String,
    identity_command: String,
    federation_rule: String,
    organization: String,
    service_account: String,
    workspace: String,
    identity_provider: String,
    /// Whether the form is open over a saved configuration.
    open: bool,
    error: Option<String>,
}

impl Setup {
    /// The form for `config`, or for Anthropic's default with none.
    fn of(config: Option<&ProviderConfig>) -> Self {
        let config = config.cloned().unwrap_or_else(ProviderConfig::anthropic);
        let mut setup = Self {
            kind: config.kind,
            model: config.model,
            base_url: config.base_url.unwrap_or_default(),
            key: String::new(),
            federated: config.federated.is_some(),
            identity_file: String::new(),
            identity_command: String::new(),
            federation_rule: String::new(),
            organization: String::new(),
            service_account: String::new(),
            workspace: String::new(),
            identity_provider: String::new(),
            open: false,
            error: None,
        };
        if let Some(federated) = config.federated {
            match federated.identity {
                IdentityToken::File { path } => setup.identity_file = path.display().to_string(),
                IdentityToken::Command { program, args } => {
                    setup.identity_command = std::iter::once(program)
                        .chain(args)
                        .collect::<Vec<_>>()
                        .join(" ");
                }
            }
            match federated.exchange {
                Exchange::Anthropic {
                    federation_rule_id,
                    organization_id,
                    service_account_id,
                    workspace_id,
                } => {
                    setup.federation_rule = federation_rule_id;
                    setup.organization = organization_id;
                    setup.service_account = service_account_id;
                    setup.workspace = workspace_id.unwrap_or_default();
                }
                Exchange::OpenAi {
                    identity_provider_id,
                    service_account_id,
                } => {
                    setup.identity_provider = identity_provider_id;
                    setup.service_account = service_account_id;
                }
            }
        }
        setup
    }

    /// The workload identity the form says, when it is on for a provider that takes it.
    fn federation(&self) -> Option<Federated> {
        let text = |field: &String| field.trim().to_owned();
        if !self.federated || !self.kind.needs_key() {
            return None;
        }
        let mut words = self.identity_command.split_whitespace().map(str::to_owned);
        let identity = match words.next() {
            Some(program) => IdentityToken::Command {
                program,
                args: words.collect(),
            },
            None => IdentityToken::File {
                path: PathBuf::from(text(&self.identity_file)),
            },
        };
        let exchange = match self.kind {
            ProviderKind::Anthropic => Exchange::Anthropic {
                federation_rule_id: text(&self.federation_rule),
                organization_id: text(&self.organization),
                service_account_id: text(&self.service_account),
                workspace_id: Some(text(&self.workspace)).filter(|w| !w.is_empty()),
            },
            ProviderKind::OpenAiCompatible | ProviderKind::Ollama => Exchange::OpenAi {
                identity_provider_id: text(&self.identity_provider),
                service_account_id: text(&self.service_account),
            },
        };
        Some(Federated { identity, exchange })
    }

    /// The configuration the form says.
    fn config(&self) -> ProviderConfig {
        let mut config = ProviderConfig::new(self.kind, self.model.trim());
        let url = self.base_url.trim();
        if !url.is_empty() {
            config.base_url = Some(url.to_owned());
        }
        config.federated = self.federation();
        config
    }
}

/// The agent panel's state.
pub struct AgentPanel {
    threads: Vec<Thread>,
    selected: Option<usize>,
    task: String,
    config: Option<ProviderConfig>,
    setup: Setup,
    make: MakeModel,
    /// Where the provider configuration is kept between runs, without its key.
    config_file: Option<PathBuf>,
    /// Why the last thread asked for did not start.
    error: Option<String>,
}

impl AgentPanel {
    /// A panel with no threads, making its models with `make`.
    pub fn new(make: MakeModel) -> Self {
        Self {
            threads: Vec::new(),
            selected: None,
            task: String::new(),
            config: None,
            setup: Setup::of(None),
            make,
            config_file: None,
            error: None,
        }
    }

    /// Keeps the provider configuration in `file`, reading the one there.
    pub fn set_config_file(&mut self, file: PathBuf) {
        self.config = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
        self.setup = Setup::of(self.config.as_ref());
        self.config_file = Some(file);
    }

    /// Uses `config`, keeping it in the configuration file when there is one.
    pub fn set_config(&mut self, config: ProviderConfig) {
        if let Some(file) = &self.config_file {
            let written = serde_json::to_string_pretty(&config)
                .map_err(std::io::Error::other)
                .and_then(|text| {
                    if let Some(folder) = file.parent() {
                        std::fs::create_dir_all(folder)?;
                    }
                    std::fs::write(file, text)
                });
            if let Err(error) = written {
                tracing::warn!(%error, "the agent's provider was not kept");
            }
        }
        self.setup = Setup::of(Some(&config));
        self.config = Some(config);
    }

    /// How many threads there are.
    #[must_use]
    pub fn threads(&self) -> usize {
        self.threads.len()
    }

    /// How many threads still wait on their model.
    #[must_use]
    pub fn running(&self) -> usize {
        self.threads
            .iter()
            .filter(|thread| thread.running())
            .count()
    }

    /// The report of thread `index`, once it has ended.
    #[must_use]
    pub fn report(&self, index: usize) -> Option<&RunReport> {
        self.threads.get(index)?.report.as_ref()
    }

    /// Starts a thread on `task` over `session`, whose project is at `project`, its model on a
    /// thread of its own; `wake` asks for a frame when the model sends something.
    ///
    /// # Errors
    ///
    /// Why it could not start: no provider set up, no model, or no checkpoint.
    pub fn start<C: 'static>(
        &mut self,
        session: &mut Session<C>,
        project: &Path,
        task: &str,
        wake: impl Fn() + Send + 'static,
    ) -> Result<(), String> {
        let config = self
            .config
            .clone()
            .ok_or_else(|| Text::NoAgent.get().to_owned())?;
        let mut model = (self.make)(&config)?;
        let run = Run::start(session, task, RunConfig::new(project)).map_err(|e| e.to_string())?;
        let (to_model, requests) = mpsc::channel::<Request>();
        let (events, from_model) = mpsc::channel::<Event>();
        std::thread::spawn(move || {
            while let Ok(request) = requests.recv() {
                let reply = model.reply_streaming(
                    &request.system,
                    &request.tools,
                    &request.transcript,
                    &mut |piece| {
                        // A panel that has gone no longer reads; the run ends with it.
                        let _sent = events.send(Event::Text(piece.to_owned()));
                        wake();
                    },
                );
                if events.send(Event::Reply(reply)).is_err() {
                    return;
                }
                wake();
            }
        });
        let mut thread = Thread {
            task: task.to_owned(),
            run: None,
            to_model: Some(to_model),
            from_model,
            streaming: String::new(),
            report: None,
            error: None,
        };
        let sent = run.request().is_some_and(|request| {
            thread
                .to_model
                .as_ref()
                .is_some_and(|to| to.send(request).is_ok())
        });
        if sent {
            thread.run = Some(run);
        } else {
            thread.report = Some(run.finish(session));
        }
        self.threads.push(thread);
        self.selected = Some(self.threads.len().saturating_sub(1));
        Ok(())
    }

    /// Asks the model once, off the frame, for an inline edit (P4-10): `prompt` under
    /// [`qs_agent::INLINE_SYSTEM`], no tools; the receiver gets the new text or why not, and
    /// `wake` asks for a frame when it does.
    ///
    /// # Errors
    ///
    /// No provider set up, or no model.
    pub fn ask_once(
        &self,
        prompt: String,
        wake: impl Fn() + Send + 'static,
    ) -> Result<mpsc::Receiver<Result<String, String>>, String> {
        let config = self
            .config
            .clone()
            .ok_or_else(|| Text::NoAgent.get().to_owned())?;
        let mut model = (self.make)(&config)?;
        let (answer, answered) = mpsc::channel();
        std::thread::spawn(move || {
            let reply = model
                .reply(
                    qs_agent::INLINE_SYSTEM,
                    &[],
                    &[Message::User { text: prompt }],
                )
                .map(|reply| qs_agent::inline_answer(&reply));
            // The editor that asked may have gone; then nothing waits for the answer.
            let _sent = answer.send(reply);
            wake();
        });
        Ok(answered)
    }

    /// Says why a thread did not start, under the task box.
    pub fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }

    /// Takes what every thread's model has sent since the last frame, running their calls in
    /// `session`; whether anything arrived. It never waits.
    pub fn pump<C: 'static>(&mut self, session: &mut Session<C>) -> bool {
        let mut any = false;
        for thread in &mut self.threads {
            while let Ok(event) = thread.from_model.try_recv() {
                thread.take(session, event);
                any = true;
            }
        }
        any
    }

    /// Waits for the next thing thread `index`'s model sends and takes it; `false` when its
    /// model has ended. For tests, which drive the model by hand.
    #[cfg(test)]
    pub(crate) fn wait<C: 'static>(&mut self, session: &mut Session<C>, index: usize) -> bool {
        let Some(thread) = self.threads.get_mut(index) else {
            return false;
        };
        match thread.from_model.recv() {
            Ok(event) => {
                thread.take(session, event);
                true
            }
            Err(_) => false,
        }
    }

    /// Draws the panel: the provider form until one is set up, then the threads, the task box
    /// and the proposed changes; what was asked for.
    pub fn ui(&mut self, ui: &mut Ui, theme: &Theme, staged: &[StagedHunk]) -> Option<AgentAction> {
        let mut action = None;
        if self.config.is_none() || self.setup.open {
            if self.config.is_none() {
                muted(ui, theme, Text::NoAgent.get());
            }
            if self.setup_form(ui, theme) {
                action = Some(AgentAction::SaveSetup);
            }
            return action;
        }
        ui.horizontal(|ui| {
            if let Some(config) = &self.config {
                muted(ui, theme, &config.model);
            }
            if ui.small_button(Text::AgentSettings.get()).clicked() {
                self.setup.open = true;
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (index, thread) in self.threads.iter().enumerate() {
                let title: String = thread.task.chars().take(24).collect();
                let label = if thread.running() {
                    format!("{RUNNING} {title}")
                } else {
                    title
                };
                if ui
                    .selectable_label(self.selected == Some(index), label)
                    .clicked()
                {
                    self.selected = Some(index);
                }
            }
        });
        if let Some(thread) = self.selected.and_then(|index| self.threads.get(index)) {
            action = thread_view(ui, theme, thread).or(action);
        }
        let task = ui.add(
            egui::TextEdit::multiline(&mut self.task)
                .hint_text(Text::AgentTaskHint.get())
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        name_field(ui, &task, Text::AgentTaskHint);
        if ui.button(Text::AgentRun.get()).clicked() && !self.task.trim().is_empty() {
            action = Some(AgentAction::Start(std::mem::take(&mut self.task)));
        }
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).color(theme.color(Token::StatusError)));
        }
        if !staged.is_empty() {
            action = changes(ui, theme, staged).or(action);
        }
        action
    }

    /// The provider form; whether Save was pressed.
    fn setup_form(&mut self, ui: &mut Ui, theme: &Theme) -> bool {
        let setup = &mut self.setup;
        ui.horizontal_wrapped(|ui| {
            for (kind, text) in [
                (ProviderKind::Anthropic, Text::ProviderAnthropic),
                (ProviderKind::OpenAiCompatible, Text::ProviderOpenAi),
                (ProviderKind::Ollama, Text::ProviderOllama),
            ] {
                if ui.radio_value(&mut setup.kind, kind, text.get()).changed() {
                    let fresh = match kind {
                        ProviderKind::Anthropic => ProviderConfig::anthropic(),
                        other => ProviderConfig::new(other, ""),
                    };
                    setup.model = fresh.model;
                }
            }
        });
        field(ui, Text::AgentModel, &mut setup.model, false);
        field(ui, Text::AgentBaseUrl, &mut setup.base_url, false);
        if setup.kind.needs_key() {
            ui.horizontal(|ui| {
                ui.radio_value(&mut setup.federated, false, Text::AgentKey.get());
                ui.radio_value(&mut setup.federated, true, Text::AgentFederated.get());
            });
        }
        if setup.kind.needs_key() && setup.federated {
            // P4-38: an identity token exchanged for a short-lived API token; no key is kept.
            field(ui, Text::AgentIdentityFile, &mut setup.identity_file, false);
            field(
                ui,
                Text::AgentIdentityCommand,
                &mut setup.identity_command,
                false,
            );
            if setup.kind == ProviderKind::Anthropic {
                field(
                    ui,
                    Text::AgentFederationRule,
                    &mut setup.federation_rule,
                    false,
                );
                field(ui, Text::AgentOrganization, &mut setup.organization, false);
                field(
                    ui,
                    Text::AgentServiceAccount,
                    &mut setup.service_account,
                    false,
                );
                field(ui, Text::AgentWorkspace, &mut setup.workspace, false);
            } else {
                field(
                    ui,
                    Text::AgentIdentityProvider,
                    &mut setup.identity_provider,
                    false,
                );
                field(
                    ui,
                    Text::AgentServiceAccount,
                    &mut setup.service_account,
                    false,
                );
            }
        } else if setup.kind.needs_key() {
            field(ui, Text::AgentKey, &mut setup.key, true);
            muted(ui, theme, Text::AgentKeyKept.get());
        }
        if let Some(error) = &setup.error {
            ui.label(RichText::new(error).color(theme.color(Token::StatusError)));
        }
        ui.button(Text::AgentSave.get()).clicked()
    }

    /// Saves the provider form: the key to `keys`, the rest to the configuration file.
    pub fn save_setup(&mut self, keys: &dyn KeyStore) {
        let config = self.setup.config();
        if config.model.is_empty() {
            return;
        }
        let key = std::mem::take(&mut self.setup.key);
        if config.needs_key()
            && !key.trim().is_empty()
            && let Err(error) = keys.set(&config.key_name(), &Secret::new(key.trim().to_owned()))
        {
            self.setup.error = Some(error.to_string());
            return;
        }
        self.set_config(config);
    }

    /// Saves the provider form to the OS keychain and the configuration file.
    pub fn save_setup_to_keychain(&mut self) {
        self.save_setup(&Keychain);
    }
}

/// A labelled text field, named by its label for a screen reader; `secret` hides what is
/// typed.
fn field(ui: &mut Ui, label: Text, value: &mut String, secret: bool) {
    ui.label(label.get());
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .password(secret)
            .desired_width(f32::INFINITY),
    );
    name_field(ui, &response, label);
}

/// A line of muted, wrapped text.
fn muted(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).color(theme.color(Token::TextMuted))).wrap());
}

/// What the panel asks the window to do.
#[derive(Debug, PartialEq, Eq)]
pub enum AgentAction {
    /// Start a thread on this task.
    Start(String),
    /// Save the provider form.
    SaveSetup,
    /// Accept these hunks.
    Accept(Vec<HunkId>),
    /// Reject these hunks.
    Reject(Vec<HunkId>),
    /// Restore this checkpoint.
    Restore(u32),
}

/// One thread: what the model said and called, the reply being written, how it ended.
fn thread_view(ui: &mut Ui, theme: &Theme, thread: &Thread) -> Option<AgentAction> {
    let mut action = None;
    let transcript = thread
        .run
        .as_ref()
        .map(Run::transcript)
        .or_else(|| {
            thread
                .report
                .as_ref()
                .map(|report| report.transcript.as_slice())
        })
        .unwrap_or_default();
    let mono = theme.color(Token::TextMuted);
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for message in transcript.iter().skip(1) {
                match message {
                    Message::User { text } => muted(ui, theme, text),
                    Message::Assistant { text, calls } => {
                        if !text.is_empty() {
                            ui.add(egui::Label::new(text.as_str()).wrap());
                        }
                        for call in calls {
                            ui.label(
                                RichText::new(format!("{CALL} {}", call.name))
                                    .monospace()
                                    .color(mono),
                            );
                        }
                    }
                    Message::Tool { is_error, .. } => {
                        let (mark, token) = if *is_error {
                            (FAILED, Token::StatusError)
                        } else {
                            (RAN, Token::TextMuted)
                        };
                        ui.label(RichText::new(mark.to_string()).color(theme.color(token)));
                    }
                }
            }
            if !thread.streaming.is_empty() {
                ui.add(egui::Label::new(thread.streaming.as_str()).wrap());
            } else if thread.running() {
                muted(ui, theme, Text::AgentWorking.get());
            }
        });
    if let Some(error) = &thread.error {
        ui.label(RichText::new(error).color(theme.color(Token::StatusError)));
    }
    if let Some(report) = &thread.report {
        match &report.outcome {
            Outcome::Done { summary } => {
                ui.label(RichText::new(Text::AgentDone.get()).strong());
                ui.add(egui::Label::new(summary.as_str()).wrap());
            }
            Outcome::NotDone { reasons } => {
                ui.label(
                    RichText::new(Text::AgentNotDone.get()).color(theme.color(Token::StatusWarn)),
                );
                for reason in reasons {
                    muted(ui, theme, reason);
                }
            }
        }
        if let Some(number) = report.checkpoint
            && ui.button(Text::AgentRestore.get()).clicked()
        {
            action = Some(AgentAction::Restore(number));
        }
    }
    action
}

/// The staged hunks, each with Accept and Reject, and both for all of them.
fn changes(ui: &mut Ui, theme: &Theme, staged: &[StagedHunk]) -> Option<AgentAction> {
    let mut action = None;
    ui.separator();
    ui.label(RichText::new(Text::AgentChanges.get()).strong());
    ui.horizontal(|ui| {
        let all: Vec<HunkId> = staged.iter().map(|hunk| hunk.id).collect();
        if ui.small_button(Text::AgentAcceptAll.get()).clicked() {
            action = Some(AgentAction::Accept(all.clone()));
        }
        if ui.small_button(Text::AgentRejectAll.get()).clicked() {
            action = Some(AgentAction::Reject(all));
        }
    });
    for hunk in staged {
        let name = hunk.path.file_name().map_or_else(
            || hunk.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        ui.horizontal(|ui| {
            muted(ui, theme, &format!("{name}:{}", hunk.line));
            if ui.small_button(Text::AgentAccept.get()).clicked() {
                action = Some(AgentAction::Accept(vec![hunk.id]));
            }
            if ui.small_button(Text::AgentReject.get()).clicked() {
                action = Some(AgentAction::Reject(vec![hunk.id]));
            }
        });
        for (lines, token) in [
            (&hunk.removed, Token::StatusError),
            (&hunk.added, Token::StatusOk),
        ] {
            for line in lines.lines().take(8) {
                ui.label(
                    RichText::new(line)
                        .monospace()
                        .small()
                        .color(theme.color(token)),
                );
            }
        }
    }
    action
}
