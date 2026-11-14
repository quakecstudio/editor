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
use qs_agent::{
    AgentProfile, Asking, Message, Model, ModelError, Outcome, Permissions, Permit, Reply, Request,
    Run, RunConfig, RunReport, history,
};
use qs_commands::Session;
use qs_models::{KeyStore, Keychain, ProviderConfig, ProviderKind, ProviderModel, Secret};
use qs_workbench::{HunkId, StagedHunk};

use crate::shell::name_field;
use crate::strings::{Text, queued_count, tokens_used};

#[cfg(test)]
mod tests;

/// The media type of the screenshots given as context.
const PNG: &str = "image/png";
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
    Reply(Result<Reply, ModelError>),
}

/// One task the agent works on.
struct Thread {
    task: String,
    /// The project it works in, where it is kept when it ends (P4-31).
    project: PathBuf,
    /// Messages the modder queued while a turn ran, which the model reads on its next turn.
    queued: Vec<String>,
    /// Whether it has been kept in the thread history.
    kept: bool,
    /// The summaries its compactions left, kept past the run's end.
    summaries: Vec<String>,
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
    fn take<C: 'static>(
        &mut self,
        session: &mut Session<C>,
        event: Event,
        followed: &mut Vec<PathBuf>,
    ) {
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
                followed.extend(last_paths(run.transcript()));
                self.summaries = run.summaries().to_vec();
                self.go_on(session, run);
            }
        }
    }

    /// After `run` has taken a reply or an answer: it waits while a call asks the modder,
    /// sends its next request, or ends.
    fn go_on<C: 'static>(&mut self, session: &mut Session<C>, mut run: Run) {
        if run.asking().is_some() {
            self.run = Some(run);
            return;
        }
        for text in self.queued.drain(..) {
            run.push_user(&text);
        }
        let sent = run
            .request()
            .and_then(|request| self.to_model.as_ref().map(|to| to.send(request).is_ok()));
        if sent == Some(true) {
            self.run = Some(run);
        } else {
            self.report = Some(run.finish(session));
            self.to_model = None;
        }
    }

    /// Takes the modder's answer to the call that asks, then goes on; the run's permissions,
    /// when the answer was Always allow.
    fn permit<C: 'static>(
        &mut self,
        session: &mut Session<C>,
        permit: Permit,
    ) -> Option<Permissions> {
        let mut run = self.run.take()?;
        run.permit(session, permit);
        let kept = (permit == Permit::Always).then(|| run.permissions().clone());
        self.go_on(session, run);
        kept
    }
}

/// Whether the editor follows the agent: opens each file its calls name (P4-31).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Follow {
    /// It does not.
    #[default]
    Off,
    /// It does.
    On,
}

/// Starts `model` on a thread of its own: where requests go to it, and where its events come
/// back; `wake` asks for a frame when it sends one.
fn spawn_model(
    mut model: Box<dyn Model + Send>,
    wake: impl Fn() + Send + 'static,
) -> (mpsc::Sender<Request>, mpsc::Receiver<Event>) {
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
    (to_model, from_model)
}

/// The files the last reply's calls name in a `path` argument, which the editor follows.
fn last_paths(transcript: &[Message]) -> Vec<PathBuf> {
    transcript
        .iter()
        .rev()
        .find_map(|m| match m {
            Message::Assistant { calls, .. } => Some(calls),
            _ => None,
        })
        .into_iter()
        .flatten()
        .filter_map(|call| {
            call.arguments
                .get(PATH_ARGUMENT)?
                .as_str()
                .map(PathBuf::from)
        })
        .collect()
}

/// The argument a call names a file in.
const PATH_ARGUMENT: &str = "path";

/// Keeps `thread`, which has ended, in its project's thread history (P4-31): its messages, its
/// checkpoint and the ids of the hunks staged when it ended.
fn keep_thread<C: 'static>(_session: &mut Session<C>, thread: &Thread) {
    let Some(report) = &thread.report else {
        return;
    };
    let staged = report
        .staged
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|hunk| hunk.get(HUNK_ID)?.as_u64())
        .collect();
    let outcome = match &report.outcome {
        Outcome::Done { summary } => summary.clone(),
        Outcome::NotDone { reasons } => reasons.join("; "),
    };
    let record = history::ThreadRecord {
        number: history::ThreadNumber::UNKEPT,
        title: history::title_of(&thread.task),
        tokens: qs_agent::estimated_tokens(&report.transcript),
        transcript: report.transcript.clone(),
        checkpoint: report.checkpoint,
        staged,
        summaries: thread.summaries.clone(),
        outcome,
    };
    if let Err(error) = history::keep(&history::threads_dir(&thread.project), record) {
        tracing::warn!(%error, "the thread was not kept in the history");
    }
}

/// The field of a staged hunk's JSON that holds its id.
const HUNK_ID: &str = "id";

/// The thread history as the panel shows it: the search and the threads kept (P4-31).
#[derive(Debug, Default)]
struct HistoryView {
    query: String,
    threads: Vec<history::ThreadRecord>,
    /// The thread reopened, by its index in `threads`.
    open: Option<usize>,
}

/// The provider form's fields.
struct Setup {
    kind: ProviderKind,
    model: String,
    base_url: String,
    /// A key typed and not yet saved; never shown again once saved.
    key: String,
    /// Whether the form is open over a saved configuration.
    open: bool,
    error: Option<String>,
}

impl Setup {
    /// The form for `config`, or for Anthropic's default with none.
    fn of(config: Option<&ProviderConfig>) -> Self {
        let config = config.cloned().unwrap_or_else(ProviderConfig::anthropic);
        Self {
            kind: config.kind,
            model: config.model,
            base_url: config.base_url.unwrap_or_default(),
            key: String::new(),
            open: false,
            error: None,
        }
    }

    /// The configuration the form says.
    fn config(&self) -> ProviderConfig {
        let mut config = ProviderConfig::new(self.kind, self.model.trim());
        let url = self.base_url.trim();
        if !url.is_empty() {
            config.base_url = Some(url.to_owned());
        }
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
    /// The profile and rules a new thread's calls are decided by (P4-30).
    permissions: Permissions,
    /// Whether the editor follows the agent (P4-31).
    follow: Follow,
    /// The files the threads' calls named since the window last took them, to follow.
    followed: Vec<PathBuf>,
    /// How many threads ended since the window last took the count, for the notification.
    finished: usize,
    /// Images given as context to the next thread started: media type and base64 data.
    images: Vec<(String, String)>,
    /// The thread history, while it is shown.
    history: Option<HistoryView>,
    /// A message being edited to run again from: the thread, the message's index and its text.
    editing: Option<(usize, usize, String)>,
    /// The message being typed to queue in the selected thread while it runs.
    queue_text: String,
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
            permissions: Permissions::default(),
            follow: Follow::Off,
            followed: Vec::new(),
            finished: 0,
            images: Vec::new(),
            history: None,
            editing: None,
            queue_text: String::new(),
        }
    }

    /// Follows the agent, as the panel's toggle does. For tests.
    #[cfg(test)]
    pub(crate) const fn set_follow_for_test(&mut self) {
        self.follow = Follow::On;
    }

    /// Queues `text` for thread `index`'s next turn, as its box does. For tests.
    #[cfg(test)]
    pub(crate) fn queue_for_test(&mut self, index: usize, text: &str) {
        if let Some(thread) = self.threads.get_mut(index) {
            thread.queued.push(text.to_owned());
        }
    }

    /// The files the threads' calls named since this was last asked, while the editor follows
    /// the agent (P4-31).
    pub fn take_followed(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.followed)
    }

    /// How many threads ended since this was last asked, for the notification (P4-31).
    pub const fn take_finished(&mut self) -> usize {
        std::mem::replace(&mut self.finished, 0)
    }

    /// Gives a PNG as context: to the selected thread while it runs, else to the next thread
    /// started (P4-31).
    pub fn attach_png(&mut self, bytes: &[u8]) {
        let data = qs_agent::base64(bytes);
        let running = self
            .selected
            .and_then(|index| self.threads.get_mut(index))
            .and_then(|thread| thread.run.as_mut());
        match running {
            Some(run) => run.push_image(PNG, data),
            None => self.images.push((PNG.to_owned(), data)),
        }
    }

    /// Shows the thread history, `threads` the threads kept, the newest first (P4-31).
    pub fn show_history(&mut self, threads: Vec<history::ThreadRecord>) {
        self.history = Some(HistoryView {
            query: String::new(),
            threads,
            open: None,
        });
    }

    /// Runs thread `index` again from its message `at`, edited to `text` (P4-31): the messages
    /// before it and the edited one, a new checkpoint and the model asked from there.
    ///
    /// # Errors
    ///
    /// No provider, no model, no such thread, or no checkpoint.
    pub fn run_again<C: 'static>(
        &mut self,
        session: &mut Session<C>,
        (index, at, text): (usize, usize, String),
        wake: impl Fn() + Send + 'static,
    ) -> Result<(), String> {
        let config = self
            .config
            .clone()
            .ok_or_else(|| Text::NoAgent.get().to_owned())?;
        let thread = self
            .threads
            .get_mut(index)
            .ok_or_else(|| Text::AgentNoThread.get().to_owned())?;
        let before = thread
            .report
            .as_ref()
            .map(|report| report.transcript.as_slice())
            .or_else(|| thread.run.as_ref().map(Run::transcript))
            .unwrap_or_default();
        let mut transcript: Vec<Message> = before.iter().take(at).cloned().collect();
        transcript.push(Message::User { text });
        let model = (self.make)(&config)?;
        let mut run_config = RunConfig::new(&thread.project);
        run_config.permissions = self.permissions.clone();
        let run = Run::resume(session, transcript, run_config).map_err(|e| e.to_string())?;
        let (to_model, from_model) = spawn_model(model, wake);
        thread.to_model = Some(to_model);
        thread.from_model = from_model;
        thread.report = None;
        thread.error = None;
        thread.kept = false;
        thread.go_on(session, run);
        self.editing = None;
        Ok(())
    }

    /// Decides the calls of threads started from now on by `permissions`.
    pub fn set_permissions(&mut self, permissions: Permissions) {
        self.permissions = permissions;
    }

    /// The profile and rules new threads are decided by.
    #[must_use]
    pub const fn permissions(&self) -> &Permissions {
        &self.permissions
    }

    /// Takes the modder's answer to the call thread `index` asks about; the permissions to
    /// keep in the project's settings, when the answer was Always allow, which new threads use
    /// too.
    pub fn permit<C: 'static>(
        &mut self,
        session: &mut Session<C>,
        index: usize,
        permit: Permit,
    ) -> Option<Permissions> {
        let kept = self.threads.get_mut(index)?.permit(session, permit)?;
        self.permissions.rules = kept.rules.clone();
        Some(kept)
    }

    /// The call thread `index` waits for the modder's answer on, if one is.
    #[must_use]
    pub fn asking(&self, index: usize) -> Option<&Asking> {
        self.threads.get(index)?.run.as_ref()?.asking()
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
        let model = (self.make)(&config)?;
        let mut run_config = RunConfig::new(project);
        run_config.permissions = self.permissions.clone();
        let mut run = Run::start(session, task, run_config).map_err(|e| e.to_string())?;
        for (media_type, data) in self.images.drain(..) {
            run.push_image(&media_type, data);
        }
        let (to_model, from_model) = spawn_model(model, wake);
        let mut thread = Thread {
            task: task.to_owned(),
            project: project.to_path_buf(),
            queued: Vec::new(),
            kept: false,
            summaries: Vec::new(),
            run: None,
            to_model: Some(to_model),
            from_model,
            streaming: String::new(),
            report: None,
            error: None,
        };
        thread.go_on(session, run);
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
                .map(|reply| reply.inline_answer())
                .map_err(|error| error.to_string());
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
        let mut followed = Vec::new();
        for thread in &mut self.threads {
            while let Ok(event) = thread.from_model.try_recv() {
                thread.take(session, event, &mut followed);
                any = true;
            }
        }
        self.after_events(session, followed);
        any
    }

    /// After threads took their models' events: the files their calls named, to follow, and
    /// each thread that ended kept in the history and counted for the notification.
    fn after_events<C: 'static>(&mut self, session: &mut Session<C>, followed: Vec<PathBuf>) {
        if self.follow == Follow::On {
            self.followed.extend(followed);
        }
        for thread in &mut self.threads {
            if thread.report.is_some() && !thread.kept {
                thread.kept = true;
                self.finished = self.finished.saturating_add(1);
                keep_thread(session, thread);
            }
        }
    }

    /// Waits for thread `index`'s model's next reply and takes it, with the text sent before
    /// it; `false` when its model has ended. For tests, which drive the model by hand: the text
    /// and the reply come one after the other from the model's thread, so waiting for the text
    /// alone would leave the reply to a race with the next frame.
    #[cfg(test)]
    pub(crate) fn wait<C: 'static>(&mut self, session: &mut Session<C>, index: usize) -> bool {
        let Some(thread) = self.threads.get_mut(index) else {
            return false;
        };
        let mut followed = Vec::new();
        loop {
            let Ok(event) = thread.from_model.recv() else {
                return false;
            };
            let reply = matches!(event, Event::Reply(_));
            thread.take(session, event, &mut followed);
            if reply {
                break;
            }
        }
        self.after_events(session, followed);
        true
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
        ui.horizontal_wrapped(|ui| {
            if let Some(config) = &self.config {
                muted(ui, theme, &config.model);
            }
            if ui.small_button(Text::AgentSettings.get()).clicked() {
                self.setup.open = true;
            }
            let mut follow = self.follow == Follow::On;
            if ui.checkbox(&mut follow, Text::AgentFollow.get()).changed() {
                self.follow = if follow { Follow::On } else { Follow::Off };
            }
            if ui.small_button(Text::AgentHistory.get()).clicked() {
                action = Some(AgentAction::OpenHistory);
            }
            if ui.small_button(Text::AgentScreenshot.get()).clicked() {
                action = Some(AgentAction::AttachScreenshot);
            }
        });
        if self.history.is_some() {
            return self.history_ui(ui, theme, staged).or(action);
        }
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
        if let Some(index) = self.selected
            && let Some(thread) = self.threads.get_mut(index)
        {
            let mut asked = thread_view(ui, theme, index, thread, &mut self.editing);
            if thread.running() {
                queue_box(ui, theme, thread, &mut self.queue_text);
            }
            if let Some(AgentAction::RunAgain(..)) = &asked {
                self.history = None;
            }
            action = asked.take().or(action);
        }
        if !self.images.is_empty() {
            muted(ui, theme, Text::AgentImage.get());
        }
        let task = ui.add(
            egui::TextEdit::multiline(&mut self.task)
                .hint_text(Text::AgentTaskHint.get())
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        name_field(ui, &task, Text::AgentTaskHint);
        ui.horizontal(|ui| {
            if ui.button(Text::AgentRun.get()).clicked() && !self.task.trim().is_empty() {
                action = Some(AgentAction::Start(std::mem::take(&mut self.task)));
            }
            let mut profile = self.permissions.profile;
            egui::ComboBox::from_id_salt(Text::AgentProfile.get())
                .selected_text(profile_text(profile).get())
                .show_ui(ui, |ui| {
                    for choice in [AgentProfile::Ask, AgentProfile::Write, AgentProfile::Full] {
                        ui.selectable_value(&mut profile, choice, profile_text(choice).get());
                    }
                });
            if profile != self.permissions.profile {
                action = Some(AgentAction::Profile(profile));
            }
        });
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).color(theme.color(Token::StatusError)));
        }
        if !staged.is_empty() {
            action = changes(ui, theme, staged).or(action);
        }
        action
    }

    /// The thread history: a search over the threads kept, and one reopened with its messages,
    /// its checkpoint and the hunks it staged that still wait for a decision (P4-31).
    fn history_ui(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        staged: &[StagedHunk],
    ) -> Option<AgentAction> {
        let mut action = None;
        let view = self.history.as_mut()?;
        if ui.small_button(Text::AgentHistoryBack.get()).clicked() {
            if view.open.is_some() {
                view.open = None;
            } else {
                self.history = None;
            }
            return None;
        }
        if let Some(record) = view.open.and_then(|index| view.threads.get(index)) {
            ui.label(RichText::new(&record.title).strong());
            muted(ui, theme, &tokens_used(record.tokens));
            transcript_view(ui, theme, &record.transcript);
            summaries_view(ui, theme, &record.summaries);
            muted(ui, theme, &record.outcome);
            if let Some(number) = record.checkpoint {
                ui.horizontal(|ui| {
                    muted(
                        ui,
                        theme,
                        &format!("{} {number}", Text::AgentCheckpoint.get()),
                    );
                    if ui.button(Text::AgentRestore.get()).clicked() {
                        action = Some(AgentAction::Restore(number));
                    }
                });
            }
            let waiting: Vec<StagedHunk> = staged
                .iter()
                .filter(|hunk| record.staged.contains(&u64::from(hunk.id.get())))
                .cloned()
                .collect();
            if !waiting.is_empty() {
                action = changes(ui, theme, &waiting).or(action);
            }
            return action;
        }
        let search = ui.add(
            egui::TextEdit::singleline(&mut view.query)
                .hint_text(Text::AgentHistorySearch.get())
                .desired_width(f32::INFINITY),
        );
        name_field(ui, &search, Text::AgentHistorySearch);
        if view.threads.is_empty() {
            muted(ui, theme, Text::AgentNoHistory.get());
        }
        let mut chosen = None;
        for (index, record) in view.threads.iter().enumerate() {
            if !record.matches(&view.query) {
                continue;
            }
            if ui.selectable_label(false, &record.title).clicked() {
                chosen = Some(index);
            }
        }
        if chosen.is_some() {
            view.open = chosen;
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
        if !key.trim().is_empty()
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
    Restore(qs_agent::CheckpointNumber),
    /// Answer the call this thread asks about.
    Permit(usize, Permit),
    /// Use this profile from now on, kept in the project's settings.
    Profile(AgentProfile),
    /// List the threads kept in the project (P4-31).
    OpenHistory,
    /// Give the running game's frame as context.
    AttachScreenshot,
    /// Run a thread again from a message, edited: the thread, the message's index, its text.
    RunAgain(usize, usize, String),
}

/// What the panel calls `profile`.
const fn profile_text(profile: AgentProfile) -> Text {
    match profile {
        AgentProfile::Ask => Text::AgentProfileAsk,
        AgentProfile::Write => Text::AgentProfileWrite,
        AgentProfile::Full => Text::AgentProfileFull,
    }
}

/// The call `index` waits on: the tool, what it names and why it asks, with Allow once,
/// Always allow and Deny.
fn permission_request(
    ui: &mut Ui,
    theme: &Theme,
    index: usize,
    asking: &Asking,
) -> Option<AgentAction> {
    let mut action = None;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(
            RichText::new(format!("{} {}", Text::AgentAsks.get(), asking.call.name))
                .strong()
                .color(theme.color(Token::StatusWarn)),
        );
        for subject in asking.subjects.iter().filter(|s| !s.is_empty()) {
            ui.label(RichText::new(subject).monospace());
        }
        muted(ui, theme, &asking.why);
        ui.horizontal(|ui| {
            for (text, permit) in [
                (Text::AgentAllowOnce, Permit::Once),
                (Text::AgentAlwaysAllow, Permit::Always),
                (Text::AgentDeny, Permit::Deny),
            ] {
                if ui.button(text.get()).clicked() {
                    action = Some(AgentAction::Permit(index, permit));
                }
            }
        });
    });
    action
}

/// Each message after the brief: what the model said and called, each call's mark, the
/// modder's messages, and each image given.
fn transcript_view(ui: &mut Ui, theme: &Theme, transcript: &[Message]) {
    let mono = theme.color(Token::TextMuted);
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
            Message::Image { .. } => muted(ui, theme, Text::AgentImage.get()),
        }
    }
}

/// A thread's compaction summaries, under their heading, when it has any.
fn summaries_view(ui: &mut Ui, theme: &Theme, summaries: &[String]) {
    if summaries.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(Text::AgentSummaries.get())
        .id_salt(Text::AgentSummaries.get())
        .show(ui, |ui| {
            for summary in summaries {
                muted(ui, theme, summary);
            }
        });
}

/// The box a message is queued from for a running thread's next turn (P4-31).
fn queue_box(ui: &mut Ui, theme: &Theme, thread: &mut Thread, text: &mut String) {
    ui.horizontal(|ui| {
        let typed = ui.add(
            egui::TextEdit::singleline(text)
                .hint_text(Text::AgentQueueHint.get())
                .desired_width(200.0),
        );
        name_field(ui, &typed, Text::AgentQueueHint);
        if ui.button(Text::AgentQueue.get()).clicked() && !text.trim().is_empty() {
            thread.queued.push(std::mem::take(text));
        }
    });
    if !thread.queued.is_empty() {
        muted(ui, theme, &queued_count(thread.queued.len()));
    }
}

/// One thread: its messages, the reply being written, its token use and compactions, how it
/// ended, and, once it has, each of the modder's messages to edit and run again from.
fn thread_view(
    ui: &mut Ui,
    theme: &Theme,
    index: usize,
    thread: &Thread,
    editing: &mut Option<(usize, usize, String)>,
) -> Option<AgentAction> {
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
    muted(
        ui,
        theme,
        &tokens_used(qs_agent::estimated_tokens(transcript)),
    );
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            transcript_view(ui, theme, transcript);
            if !thread.streaming.is_empty() {
                ui.add(egui::Label::new(thread.streaming.as_str()).wrap());
            } else if thread.running() && thread.run.as_ref().and_then(Run::asking).is_none() {
                muted(ui, theme, Text::AgentWorking.get());
            }
        });
    summaries_view(ui, theme, &thread.summaries);
    if let Some(asking) = thread.run.as_ref().and_then(Run::asking) {
        action = permission_request(ui, theme, index, asking);
    }
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
        action = edit_messages(ui, index, &report.transcript, editing).or(action);
    }
    action
}

/// Each of the modder's messages of an ended thread with Edit, and the one being edited with
/// its text and Run again from here (P4-31).
fn edit_messages(
    ui: &mut Ui,
    index: usize,
    transcript: &[Message],
    editing: &mut Option<(usize, usize, String)>,
) -> Option<AgentAction> {
    let mut action = None;
    for (at, message) in transcript.iter().enumerate() {
        let Message::User { text } = message else {
            continue;
        };
        match editing {
            Some((thread, message_at, edited)) if *thread == index && *message_at == at => {
                ui.add(egui::TextEdit::multiline(edited).desired_width(f32::INFINITY));
                if ui.button(Text::AgentRunAgain.get()).clicked() {
                    action = Some(AgentAction::RunAgain(index, at, edited.clone()));
                }
            }
            _ => {
                let first: String = text
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .chars()
                    .take(48)
                    .collect();
                ui.horizontal(|ui| {
                    ui.label(first);
                    if ui.small_button(Text::AgentEdit.get()).clicked() {
                        *editing = Some((index, at, text.clone()));
                    }
                });
            }
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
