// SPDX-License-Identifier: GPL-3.0-or-later

//! The play loop (P3-12): the preview host started on the open project's build, paused,
//! resumed, stepped, the map restarted and the progs reloaded after a save, as the toolbar,
//! the palette, the CLI and the agent ask through the `preview.*` commands.
//!
//! The model is Roblox Studio's and Unity's Play button: one control starts the game on the
//! project as it is, and a change saved while it plays reaches it without a restart.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use qc_project::Loader;
use qs_engine::{
    Client, ClientError, Flavour, InspectWhat, LaunchError, LaunchSpec, RunRootError, Running,
    StepKind, launch, locate_host, prepare_run_root,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use qs_commands::{Command, NoParams, Outcome, RegisterError, Registry};
use schemars::JsonSchema;

use crate::{HasWorkbench, ModuleBuilt, Workbench, WorkbenchError};

pub mod debug;
pub mod entities;

use debug::{Debugger, Location, StopReason};

use entities::Entities;

/// The most console lines the preview keeps; older ones are dropped.
const CONSOLE_LINES: usize = 1000;
/// How long the host has to stop after `shutdown` before it is killed.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(5);

/// Whether the game is running.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayState {
    /// No host is running.
    #[default]
    Stopped,
    /// The host runs its frames.
    Playing,
    /// The host is paused between frames, or stopped in one at a breakpoint.
    Paused,
}

/// Where the preview finds the host and the game.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreviewConfig {
    /// The preview host's executable; beside the app's own when `None` (`locate_host`).
    pub host: Option<PathBuf>,
    /// The app's executable, beside which the host is looked for.
    pub app: PathBuf,
    /// The game folder; the first install `qs-install` finds when `None`.
    pub game: Option<PathBuf>,
    /// Further words of the host's command line, such as `--nosound`.
    pub extra: Vec<String>,
}

impl PreviewConfig {
    /// The configuration for the app at `app`: the host at `QS_PREVIEW_HOST` when it is set,
    /// else beside the app; the game at `QS_GAME_DIR` when it is set, else the first install
    /// found. The settings screen and the first-run guide (P3-21) set them later.
    #[must_use]
    pub fn from_env(app: PathBuf) -> Self {
        Self {
            host: std::env::var_os("QS_PREVIEW_HOST").map(PathBuf::from),
            app,
            game: std::env::var_os("QS_GAME_DIR").map(PathBuf::from),
            extra: Vec::new(),
        }
    }
}

/// One line the host's console printed (`console/line`, BRIDGE-3.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleLine {
    /// What it says.
    pub text: String,
    /// `print`, `dprint` or `error`, as the host sends it.
    pub level: String,
}

impl ConsoleLine {
    /// Whether the host printed it as an error.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.level == "error"
    }

    /// Whether the console's filter shows it: its text holds `filter`, without regard to case,
    /// or the filter is blank.
    #[must_use]
    pub fn matches(&self, filter: &str) -> bool {
        let filter = filter.trim();
        filter.is_empty() || self.text.to_lowercase().contains(&filter.to_lowercase())
    }
}

/// Why a preview command did not do what it was asked.
#[derive(Debug)]
pub enum PreviewError {
    /// No project is open, or it could not be built.
    Build(WorkbenchError),
    /// The project built no program, so there is nothing to play.
    NothingBuilt(Vec<ModuleBuilt>),
    /// The module's program is not under `build/<mod>/`, where the run root links it.
    OutsideBuild(PathBuf),
    /// No game was found, or the folder given holds none.
    NoGame(Option<PathBuf>),
    /// The run root could not be made.
    RunRoot(RunRootError),
    /// The host did not start or connect.
    Launch(LaunchError),
    /// The host answered a command with an error, or went away.
    Bridge(ClientError),
    /// The command needs a running host.
    NotRunning,
    /// Fields are edited only while the game is paused.
    NotPaused,
    /// The selected edict has no such field, or nothing is selected.
    NoField(String),
    /// The text is not a value of the field's type.
    BadValue {
        /// The field.
        field: String,
        /// Its type.
        kind: String,
    },
}

impl core::fmt::Display for PreviewError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Build(error) => write!(f, "{error}"),
            Self::NothingBuilt(modules) => {
                let failed = modules.iter().filter(|m| !m.built).count();
                write!(
                    f,
                    "the project built no program ({failed} modules with errors)"
                )
            }
            Self::OutsideBuild(path) => write!(
                f,
                "{} is not under the project's build/<mod>/ folder, which the game plays",
                path.display()
            ),
            Self::NoGame(Some(path)) => write!(f, "{}: no Quake game here", path.display()),
            Self::NoGame(None) => f.write_str("no Quake install was found"),
            Self::RunRoot(error) => write!(f, "{error}"),
            Self::Launch(error) => write!(f, "{error}"),
            Self::Bridge(error) => write!(f, "{error}"),
            Self::NotRunning => f.write_str("the game is not running"),
            Self::NotPaused => f.write_str("pause the game to edit its fields"),
            Self::NoField(field) => write!(f, "the selected entity has no field {field}"),
            Self::BadValue { field, kind } => write!(f, "{field} takes a {kind}"),
        }
    }
}

impl std::error::Error for PreviewError {}

impl From<ClientError> for PreviewError {
    fn from(error: ClientError) -> Self {
        Self::Bridge(error)
    }
}

/// A host that runs, and what it plays.
struct Live {
    running: Running,
    /// The program it loaded, which a reload reads again.
    progs: PathBuf,
}

/// The play loop's state: the host, when one runs, its state and its console.
pub struct Preview {
    config: PreviewConfig,
    live: Option<Live>,
    state: PlayState,
    console: VecDeque<ConsoleLine>,
    /// The last thing the loop did, for the toolbar: `Reloaded 0.9 s after save`.
    status: Option<String>,
    /// The game's edicts and the selected one.
    entities: Entities,
    /// The edits made while paused, which the host applies on resume (BRIDGE-4.4): each
    /// edict, field and the value as typed.
    pending: Vec<(u32, String, String)>,
    /// The breakpoints, where the game stopped, its stack and the watches.
    debugger: Debugger,
}

impl core::fmt::Debug for Preview {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Preview")
            .field("state", &self.state)
            .field("running", &self.live.is_some())
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl Preview {
    /// A preview with no host running.
    #[must_use]
    pub const fn new(config: PreviewConfig) -> Self {
        Self {
            config,
            live: None,
            state: PlayState::Stopped,
            console: VecDeque::new(),
            status: None,
            entities: Entities::new(),
            pending: Vec::new(),
            debugger: Debugger::new(),
        }
    }

    /// The breakpoints, where the game stopped, its stack and the watches.
    #[must_use]
    pub const fn debugger(&self) -> &Debugger {
        &self.debugger
    }

    /// Adds the breakpoint at `line` of `file`, relative to the project's root, or takes it
    /// away when there is one; set in or cleared from the running game too
    /// (`breakpoint/set`, `breakpoint/clear`).
    ///
    /// # Errors
    ///
    /// The bridge's error; the breakpoint is kept for the next game.
    pub fn toggle_breakpoint(&mut self, file: &Path, line: u32) -> Result<(), PreviewError> {
        match self.debugger.toggle(file, line) {
            Some(removed) => {
                if let (Some(id), Some(client)) = (removed.id, self.client()) {
                    client.clear_breakpoint(id)?;
                }
                Ok(())
            }
            None if self.live.is_some() => self.set_breakpoint(file, line),
            None => Ok(()),
        }
    }

    /// Sets the breakpoint at `line` of `file` in the running game, keeping its id.
    fn set_breakpoint(&mut self, file: &Path, line: u32) -> Result<(), PreviewError> {
        let name = file.to_string_lossy().replace('\\', "/");
        let answer = self.live_client()?.set_breakpoint(&name, line)?;
        if let Some(breakpoint) = self.debugger.breakpoint_mut(file, line) {
            breakpoint.id = answer.get("id").and_then(Value::as_u64);
            breakpoint.function = answer
                .get("location")
                .and_then(Location::from_value)
                .map(|l| l.function);
        }
        Ok(())
    }

    /// Sets every breakpoint in the game that started; one the host refuses is logged.
    fn set_breakpoints(&mut self) {
        let lines: Vec<_> = self
            .debugger
            .breakpoints()
            .iter()
            .map(|b| (b.file.clone(), b.line))
            .collect();
        for (file, line) in lines {
            if let Err(error) = self.set_breakpoint(&file, line) {
                tracing::debug!(%error, file = %file.display(), line, "breakpoint not set");
            }
        }
    }

    /// Adds the watch `expression`, read while the game is stopped in a frame.
    ///
    /// # Errors
    ///
    /// The bridge's error when it is read at once.
    pub fn watch(&mut self, expression: &str) -> Result<(), PreviewError> {
        self.debugger.watch(expression);
        self.read_watches()
    }

    /// Takes away watch `index`.
    pub fn unwatch(&mut self, index: usize) {
        self.debugger.unwatch(index);
    }

    /// Reads each watch's value while stopped in a frame (`evaluate`).
    fn read_watches(&mut self) -> Result<(), PreviewError> {
        if self.debugger.stopped().is_none() {
            return Ok(());
        }
        let expressions: Vec<String> = self
            .debugger
            .watches()
            .iter()
            .map(|w| w.expression.clone())
            .collect();
        let mut values = Vec::new();
        for expression in &expressions {
            let value = match self.live_client()?.evaluate(expression) {
                Ok(answer) => answer.get("value").map(|value| self.entities.show(value)),
                Err(error) => Some(error.to_string()),
            };
            values.push(value);
        }
        for (watch, value) in self.debugger.watches_mut().iter_mut().zip(values) {
            watch.value = value;
        }
        Ok(())
    }

    /// Runs `command` in the game's console (`console`); its output comes as console lines.
    ///
    /// # Errors
    ///
    /// [`PreviewError::NotRunning`] with no host, or the bridge's error.
    pub fn console_command(&mut self, command: &str) -> Result<(), PreviewError> {
        self.push_line(&format!("] {command}"), "print");
        self.live_client()?.console(command)?;
        Ok(())
    }

    /// Steps the game stopped in a frame by `kind`: a statement, into a call, over one or out
    /// to the caller (BRIDGE-4.3); a game paused between frames steps a frame.
    ///
    /// # Errors
    ///
    /// [`PreviewError::NotRunning`] with no host, or the bridge's error.
    pub fn step_by(&mut self, kind: StepKind) -> Result<(), PreviewError> {
        if self.debugger.stopped().is_none() {
            return self.step();
        }
        self.live_client()?.step(kind)?;
        Ok(())
    }

    /// The value typed for field `field` of edict `edict` while paused, which the game takes on
    /// resume.
    #[must_use]
    pub fn pending(&self, edict: u32, field: &str) -> Option<&str> {
        self.pending
            .iter()
            .rev()
            .find(|(e, f, _)| *e == edict && f == field)
            .map(|(_, _, value)| value.as_str())
    }

    /// The game's edicts and the selected one.
    #[must_use]
    pub const fn entities(&self) -> &Entities {
        &self.entities
    }

    /// Reads the game's edicts again (`inspect entities`).
    ///
    /// # Errors
    ///
    /// [`PreviewError::NotRunning`] with no host, or the bridge's error.
    pub fn refresh(&mut self) -> Result<(), PreviewError> {
        let answer = self.live_client()?.inspect(InspectWhat::Entities)?;
        self.entities.replace(&answer);
        Ok(())
    }

    /// Selects edict `edict`, reading its fields (`inspect edict`).
    ///
    /// # Errors
    ///
    /// [`PreviewError::NotRunning`] with no host, or the bridge's error.
    pub fn select(&mut self, edict: u32) -> Result<(), PreviewError> {
        let answer = self.live_client()?.inspect(InspectWhat::Edict(edict))?;
        self.entities.select(&answer);
        Ok(())
    }

    /// Selects the edict under the point `x`, `y` of the game's window, fractions from its top
    /// left (`pick`); the edict, or `None` when the pick hit none.
    ///
    /// # Errors
    ///
    /// [`PreviewError::NotRunning`] with no host, or the bridge's error.
    pub fn pick(&mut self, x: f64, y: f64) -> Result<Option<u32>, PreviewError> {
        let answer = self.live_client()?.pick(x, y)?;
        let Some(edict) = answer
            .get("edict")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n != 0)
        else {
            return Ok(None);
        };
        self.select(edict)?;
        Ok(Some(edict))
    }

    /// Sets field `field` of the selected edict to the value `text` names, read as the
    /// field's type, while the game is paused (`field/set`); the edict is then read again.
    ///
    /// # Errors
    ///
    /// [`PreviewError::NotPaused`] while the game runs, [`PreviewError::NoField`] for a field
    /// the selected edict lacks, [`PreviewError::BadValue`] for text that is not of its type,
    /// or the bridge's error.
    pub fn set_field(&mut self, field: &str, text: &str) -> Result<(), PreviewError> {
        if self.state != PlayState::Paused {
            return Err(PreviewError::NotPaused);
        }
        let selected = self
            .entities
            .selected()
            .ok_or_else(|| PreviewError::NoField(field.to_owned()))?;
        let edict = selected.edict;
        let kind = selected
            .fields
            .iter()
            .find(|f| f.name == field)
            .map(|f| f.kind().to_owned())
            .ok_or_else(|| PreviewError::NoField(field.to_owned()))?;
        let value = entities::parse_value(&kind, text).ok_or_else(|| PreviewError::BadValue {
            field: field.to_owned(),
            kind,
        })?;
        let answer = self.live_client()?.set_field(edict, field, value)?;
        // BRIDGE-4.4: while paused between frames the host holds the edit until it resumes.
        if answer.get("applied").and_then(Value::as_str) == Some("on_resume") {
            self.pending
                .push((edict, field.to_owned(), text.trim().to_owned()));
            return Ok(());
        }
        self.select(edict)
    }

    /// Whether the game runs, is paused or is stopped.
    #[must_use]
    pub const fn state(&self) -> PlayState {
        self.state
    }

    /// The last thing the loop did.
    #[must_use]
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// The console's lines, oldest first.
    pub fn console(&self) -> impl Iterator<Item = &ConsoleLine> {
        self.console.iter()
    }

    /// The bridge client of the running host.
    pub fn client(&mut self) -> Option<&mut Client> {
        self.live.as_mut().map(|live| &mut live.running.client)
    }

    /// Builds the open project and starts the host on its first program, on `map` or the
    /// host's own first map; a host already running is stopped first.
    ///
    /// # Errors
    ///
    /// [`PreviewError`] when the project does not build, no game is found, the run root
    /// cannot be made or the host does not start.
    pub fn play<L: Loader>(
        &mut self,
        workbench: &mut Workbench<L>,
        map: Option<String>,
    ) -> Result<(), PreviewError> {
        self.stop();
        let (project, progs, mod_name) = built_program(workbench)?;
        let base = self.game_base()?;
        let run_root = prepare_run_root(&project, Flavour::Original, &base, &mod_name)
            .map_err(PreviewError::RunRoot)?;
        let host = locate_host(self.config.host.as_deref(), &self.config.app)
            .map_err(PreviewError::Launch)?;
        let spec = LaunchSpec {
            host,
            run_root,
            map,
            socket: socket_path(),
            extra: self.config.extra.clone(),
        };
        let running = launch(&spec, "QuakeC Studio").map_err(PreviewError::Launch)?;
        self.live = Some(Live { running, progs });
        self.state = PlayState::Playing;
        self.status = Some(format!("Playing {mod_name}"));
        self.console.clear();
        self.set_breakpoints();
        self.refresh()
    }

    /// The original game's base directory: the configured folder's, else the first install
    /// found with one.
    fn game_base(&self) -> Result<qs_install::GameBase, PreviewError> {
        match &self.config.game {
            Some(folder) => qs_install::examine_folder(folder)
                .and_then(|install| install.original)
                .ok_or_else(|| PreviewError::NoGame(Some(folder.clone()))),
            None => qs_install::find_installs()
                .into_iter()
                .find_map(|install| install.original)
                .ok_or(PreviewError::NoGame(None)),
        }
    }

    /// The running host's client, or [`PreviewError::NotRunning`].
    fn live_client(&mut self) -> Result<&mut Client, PreviewError> {
        self.client().ok_or(PreviewError::NotRunning)
    }

    /// Pauses the game between frames (BRIDGE-4.1).
    ///
    /// # Errors
    ///
    /// [`PreviewError::NotRunning`] with no host, or the bridge's error.
    pub fn pause(&mut self) -> Result<(), PreviewError> {
        self.live_client()?.pause()?;
        self.state = PlayState::Paused;
        Ok(())
    }

    /// Resumes a paused game (BRIDGE-4.1).
    ///
    /// # Errors
    ///
    /// [`PreviewError::NotRunning`] with no host, or the bridge's error.
    pub fn resume(&mut self) -> Result<(), PreviewError> {
        self.live_client()?.resume()?;
        self.state = PlayState::Playing;
        self.pending.clear();
        self.debugger.resumed();
        match self.entities.selected().map(|s| s.edict) {
            Some(edict) => self.select(edict),
            None => Ok(()),
        }
    }

    /// Runs one server frame of a paused game, or one statement when stopped at a breakpoint
    /// (BRIDGE-4.2), and stays paused.
    ///
    /// # Errors
    ///
    /// [`PreviewError::NotRunning`] with no host, or the bridge's error.
    pub fn step(&mut self) -> Result<(), PreviewError> {
        if self.state == PlayState::Playing {
            self.pause()?;
        }
        self.live_client()?.step(StepKind::Frame)?;
        self.state = PlayState::Paused;
        Ok(())
    }

    /// Starts the map again, as id's console command `restart` does.
    ///
    /// # Errors
    ///
    /// [`PreviewError::NotRunning`] with no host, or the bridge's error.
    pub fn restart(&mut self) -> Result<(), PreviewError> {
        self.live_client()?.console("restart")?;
        Ok(())
    }

    /// Builds the open project and reloads its program in the running game (BRIDGE-4.7), its
    /// map going on; does nothing with no host. Returns what the host migrated.
    ///
    /// # Errors
    ///
    /// [`PreviewError`] when the project does not build or the host refuses the reload.
    pub fn reload<L: Loader>(
        &mut self,
        workbench: &mut Workbench<L>,
    ) -> Result<Option<Value>, PreviewError> {
        if self.live.is_none() {
            return Ok(None);
        }
        let started = Instant::now();
        let (_, progs, _) = built_program(workbench)?;
        let path = progs.to_string_lossy().into_owned();
        let migrated = self.live_client()?.reload(Some(&path))?;
        if let Some(live) = &mut self.live {
            live.progs = progs;
        }
        self.status = Some(format!(
            "Reloaded {:.1} s after save",
            started.elapsed().as_secs_f32()
        ));
        Ok(Some(migrated))
    }

    /// Stops the host: `shutdown`, then a kill when it has not stopped in time.
    pub fn stop(&mut self) {
        let Some(mut live) = self.live.take() else {
            return;
        };
        if let Err(error) = live.running.client.shutdown() {
            tracing::debug!(%error, "the preview host did not answer shutdown");
        }
        let started = Instant::now();
        loop {
            match live.running.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if started.elapsed() < SHUTDOWN_WAIT => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Ok(None) | Err(_) => {
                    if let Err(error) = live.running.child.kill() {
                        tracing::warn!(%error, "the preview host could not be stopped");
                    }
                    if let Err(error) = live.running.child.wait() {
                        tracing::warn!(%error, "the preview host was not waited for");
                    }
                    break;
                }
            }
        }
        self.state = PlayState::Stopped;
        self.status = None;
        self.entities.clear();
        self.pending.clear();
        self.debugger.unset_all();
    }

    /// Reads the host's events: its state changes and console lines; the host closing the
    /// bridge stops the preview.
    pub fn poll(&mut self) {
        let Some(client) = self.client() else {
            return;
        };
        match client.poll_events() {
            Ok(events) => {
                for event in events {
                    self.take_event(&event.method, &event.params);
                }
            }
            Err(_) => {
                self.live = None;
                self.state = PlayState::Stopped;
                self.status = Some("The game closed".to_owned());
                self.entities.clear();
            }
        }
    }

    /// One event (BRIDGE-3): `state/changed` sets the play state and `console/line` adds a
    /// line.
    fn take_event(&mut self, method: &str, params: &Value) {
        match method {
            "state/changed" => self.state_changed(params),
            "console/line" => {
                let text = params.get("text").and_then(Value::as_str).unwrap_or("");
                let level = params
                    .get("level")
                    .and_then(Value::as_str)
                    .unwrap_or("print");
                self.push_line(text, level);
            }
            "qc/error" => {
                let message = params.get("message").and_then(Value::as_str).unwrap_or("");
                self.debugger.failed(message);
                self.push_line(message, "error");
            }
            _ => {
                if self.entities.take_event(method, params)
                    && let Err(error) = self.refresh()
                {
                    tracing::debug!(%error, "the entities were not read again");
                }
            }
        }
    }
}

impl Preview {
    /// A line for the console, the oldest dropped past [`CONSOLE_LINES`].
    fn push_line(&mut self, text: &str, level: &str) {
        if self.console.len() == CONSOLE_LINES {
            self.console.pop_front();
        }
        self.console.push_back(ConsoleLine {
            text: text.to_owned(),
            level: level.to_owned(),
        });
    }

    /// `state/changed` (BRIDGE-3.3): the play state; stopped in a frame, where and why, and
    /// the stack and the watches read; running again, none of them.
    fn state_changed(&mut self, params: &Value) {
        let paused = params.get("state").and_then(Value::as_str) == Some("paused");
        self.state = if paused {
            PlayState::Paused
        } else {
            PlayState::Playing
        };
        if !paused {
            self.debugger.resumed();
            return;
        }
        let reason = params
            .get("reason")
            .and_then(Value::as_str)
            .and_then(StopReason::of);
        let location = params.get("location").and_then(Location::from_value);
        let (Some(reason), Some(location)) = (reason, location) else {
            return;
        };
        self.debugger.stop(reason, location);
        let read = self
            .live_client()
            .and_then(|client| Ok(client.inspect(InspectWhat::Stack)?));
        match read {
            Ok(answer) => self.debugger.set_stack(&answer),
            Err(error) => tracing::debug!(%error, "the stack was not read"),
        }
        if let Err(error) = self.read_watches() {
            tracing::debug!(%error, "the watches were not read");
        }
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Builds the project; its root, its first program and the mod folder it is in, or why there
/// is none.
fn built_program<L: Loader>(
    workbench: &mut Workbench<L>,
) -> Result<(PathBuf, PathBuf, String), PreviewError> {
    let project = workbench
        .root()
        .map(Path::to_path_buf)
        .ok_or(PreviewError::Build(WorkbenchError::NoProject))?;
    let modules = workbench.build().map_err(PreviewError::Build)?;
    let Some(built) = modules.iter().find(|m| m.built) else {
        return Err(PreviewError::NothingBuilt(modules));
    };
    let progs = built.output.clone();
    let mod_name =
        mod_folder(&project, &progs).ok_or_else(|| PreviewError::OutsideBuild(progs.clone()))?;
    Ok((project, progs, mod_name))
}

/// The folder name `<mod>` of a program at `<project>/build/<mod>/<file>`.
fn mod_folder(project: &Path, progs: &Path) -> Option<String> {
    let folder = progs.parent()?;
    (folder.parent()? == project.join("build"))
        .then(|| folder.file_name()?.to_str().map(str::to_owned))
        .flatten()
}

/// A socket or pipe name of this process's own.
fn socket_path() -> PathBuf {
    let name = format!("quakec-studio-{}", std::process::id());
    if cfg!(windows) {
        PathBuf::from(format!(r"\\.\pipe\{name}"))
    } else {
        std::env::temp_dir().join(format!("{name}.sock"))
    }
}

/// A context the `preview.*` commands work on: one with a [`Workbench`] and a [`Preview`].
pub trait HasPreview: HasWorkbench {
    /// The workbench and the preview, together, for the commands that build and play.
    fn workbench_and_preview(&mut self) -> (&mut Workbench<Self::Loader>, &mut Preview);
}

/// The id of the command that builds the project and starts the game.
pub const PLAY: &str = "preview.play";
/// The id of the command that pauses the game.
pub const PAUSE: &str = "preview.pause";
/// The id of the command that resumes the game.
pub const RESUME: &str = "preview.resume";
/// The id of the command that runs one frame of the paused game.
pub const STEP: &str = "preview.step";
/// The id of the command that starts the map again.
pub const RESTART: &str = "preview.restart";
/// The id of the command that stops the game.
pub const STOP: &str = "preview.stop";
/// The id of the command that builds the project and reloads it in the running game.
pub const RELOAD: &str = "preview.reload";
/// The id of the command that reads the running game's events.
pub const POLL: &str = "preview.poll";
/// The id of the command that selects an edict.
pub const SELECT: &str = "preview.select";
/// The id of the command that selects the edict under a point of the game's window.
pub const PICK: &str = "preview.pick";
/// The id of the command that sets a field of the selected edict.
pub const SET_FIELD: &str = "preview.set_field";
/// The id of the command that adds or takes away a breakpoint.
pub const BREAKPOINT: &str = "preview.breakpoint";
/// The id of the command that runs a console command in the game.
pub const CONSOLE: &str = "preview.console";
/// The id of the command that adds a watch.
pub const WATCH: &str = "preview.watch";
/// The id of the command that takes a watch away.
pub const UNWATCH: &str = "preview.unwatch";
/// The id of the command that steps the stopped game by a statement, into, over or out.
pub const STEP_BY: &str = "preview.step_by";

/// What a `preview.*` command leaves: the play state and the last thing the loop did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PreviewStatus {
    /// `stopped`, `playing` or `paused`.
    pub state: String,
    /// The last thing the loop did, such as `Reloaded 0.4 s after save`.
    pub status: Option<String>,
}

impl PreviewStatus {
    fn of(preview: &Preview) -> Self {
        let state = match preview.state() {
            PlayState::Stopped => "stopped",
            PlayState::Playing => "playing",
            PlayState::Paused => "paused",
        };
        Self {
            state: state.to_owned(),
            status: preview.status.clone(),
        }
    }
}

/// The parameters of `preview.play`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlayParams {
    /// The map to start on; the host's own first map when absent.
    #[serde(default)]
    pub map: Option<String>,
}

/// The parameters of `preview.select`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectParams {
    /// The edict's number.
    pub edict: u32,
}

/// The parameters of `preview.pick`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PickParams {
    /// The point across the game's window, from 0 at its left to 1 at its right.
    pub x: f64,
    /// The point down the game's window, from 0 at its top to 1 at its bottom.
    pub y: f64,
}

/// The parameters of `preview.set_field`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetFieldParams {
    /// The selected edict's field.
    pub field: String,
    /// The value as the Properties panel writes it: `3`, `'1 2 3'`, `"text"`, `player #1`.
    pub value: String,
}

/// The selected edict, as `preview.select`, `preview.pick` and `preview.set_field` leave it.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SelectedEntity {
    /// The edict's number, or none when nothing is selected.
    pub edict: Option<u32>,
    /// Its classname.
    pub classname: Option<String>,
}

impl SelectedEntity {
    fn of(preview: &Preview) -> Self {
        let selected = preview.entities().selected();
        Self {
            edict: selected.map(|s| s.edict),
            classname: selected.map(|s| s.classname.clone()),
        }
    }
}

/// `preview.select`: selects an edict and reads its fields.
struct SelectCommand;

impl<C: HasPreview + 'static> Command<C> for SelectCommand {
    const ID: &'static str = SELECT;
    const SUMMARY: &'static str = "Selects an edict of the running game and reads its fields \
        for the Properties panel.";
    type Params = SelectParams;
    type Output = SelectedEntity;
    type Error = PreviewError;

    fn run(
        &self,
        context: &mut C,
        SelectParams { edict }: SelectParams,
    ) -> Result<Outcome<SelectedEntity, C>, PreviewError> {
        let (_, preview) = context.workbench_and_preview();
        preview.select(edict)?;
        Ok(Outcome::outside_history(SelectedEntity::of(preview)))
    }
}

/// `preview.pick`: selects the edict under a point of the game's window.
struct PickCommand;

impl<C: HasPreview + 'static> Command<C> for PickCommand {
    const ID: &'static str = PICK;
    const SUMMARY: &'static str = "Selects the edict under a point of the game's window.";
    type Params = PickParams;
    type Output = SelectedEntity;
    type Error = PreviewError;

    fn run(
        &self,
        context: &mut C,
        PickParams { x, y }: PickParams,
    ) -> Result<Outcome<SelectedEntity, C>, PreviewError> {
        let (_, preview) = context.workbench_and_preview();
        preview.pick(x, y)?;
        Ok(Outcome::outside_history(SelectedEntity::of(preview)))
    }
}

/// `preview.set_field`: sets a field of the selected edict while the game is paused.
struct SetFieldCommand;

impl<C: HasPreview + 'static> Command<C> for SetFieldCommand {
    const ID: &'static str = SET_FIELD;
    const SUMMARY: &'static str = "Sets a field of the selected edict while the game is \
        paused, the value read as the field's type.";
    type Params = SetFieldParams;
    type Output = SelectedEntity;
    type Error = PreviewError;

    fn run(
        &self,
        context: &mut C,
        SetFieldParams { field, value }: SetFieldParams,
    ) -> Result<Outcome<SelectedEntity, C>, PreviewError> {
        let (_, preview) = context.workbench_and_preview();
        preview.set_field(&field, &value)?;
        Ok(Outcome::outside_history(SelectedEntity::of(preview)))
    }
}

/// The parameters of `preview.breakpoint`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BreakpointParams {
    /// The file, relative to the project's root.
    pub file: PathBuf,
    /// The line, from 1.
    pub line: u32,
}

/// The parameters of `preview.console`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConsoleParams {
    /// The console command, such as `impulse 9`.
    pub command: String,
}

/// The parameters of `preview.watch`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WatchParams {
    /// The expression, such as `self.owner.health`.
    pub expression: String,
}

/// The parameters of `preview.unwatch`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnwatchParams {
    /// The watch's place in the list, from 0.
    pub index: usize,
}

/// How `preview.step_by` steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StepBy {
    /// One statement.
    Statement,
    /// Into the next call.
    In,
    /// To the next statement of this function.
    Over,
    /// Out to the caller.
    Out,
}

impl StepBy {
    const fn kind(self) -> StepKind {
        match self {
            Self::Statement => StepKind::Statement,
            Self::In => StepKind::In,
            Self::Over => StepKind::Over,
            Self::Out => StepKind::Out,
        }
    }
}

/// The parameters of `preview.step_by`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepByParams {
    /// A statement, into, over or out.
    pub kind: StepBy,
}

/// A `preview.*` command whose parameters say what it does to the preview.
trait Act: Send + Sync {
    const ID: &'static str;
    const SUMMARY: &'static str;
    type Params: serde::de::DeserializeOwned + JsonSchema;
    fn act(preview: &mut Preview, params: Self::Params) -> Result<(), PreviewError>;
}

struct BreakpointAct;
impl Act for BreakpointAct {
    const ID: &'static str = BREAKPOINT;
    const SUMMARY: &'static str = "Adds a breakpoint at a line of a file, or takes it away \
        when there is one; set in the running game too and in each game that starts.";
    type Params = BreakpointParams;
    fn act(preview: &mut Preview, p: BreakpointParams) -> Result<(), PreviewError> {
        preview.toggle_breakpoint(&p.file, p.line)
    }
}

struct ConsoleAct;
impl Act for ConsoleAct {
    const ID: &'static str = CONSOLE;
    const SUMMARY: &'static str = "Runs a command in the running game's console.";
    type Params = ConsoleParams;
    fn act(preview: &mut Preview, p: ConsoleParams) -> Result<(), PreviewError> {
        preview.console_command(&p.command)
    }
}

struct WatchAct;
impl Act for WatchAct {
    const ID: &'static str = WATCH;
    const SUMMARY: &'static str = "Adds a watch, an expression read each time the game stops \
        in a frame.";
    type Params = WatchParams;
    fn act(preview: &mut Preview, p: WatchParams) -> Result<(), PreviewError> {
        preview.watch(&p.expression)
    }
}

struct UnwatchAct;
impl Act for UnwatchAct {
    const ID: &'static str = UNWATCH;
    const SUMMARY: &'static str = "Takes a watch away.";
    type Params = UnwatchParams;
    fn act(preview: &mut Preview, p: UnwatchParams) -> Result<(), PreviewError> {
        preview.unwatch(p.index);
        Ok(())
    }
}

struct StepByAct;
impl Act for StepByAct {
    const ID: &'static str = STEP_BY;
    const SUMMARY: &'static str = "Steps the game stopped in a frame by a statement, into a \
        call, over one or out to the caller.";
    type Params = StepByParams;
    fn act(preview: &mut Preview, p: StepByParams) -> Result<(), PreviewError> {
        preview.step_by(p.kind.kind())
    }
}

/// An [`Act`] as a command.
struct ActCommand<A>(core::marker::PhantomData<A>);

impl<A: Act + 'static, C: HasPreview + 'static> Command<C> for ActCommand<A> {
    const ID: &'static str = A::ID;
    const SUMMARY: &'static str = A::SUMMARY;
    type Params = A::Params;
    type Output = PreviewStatus;
    type Error = PreviewError;

    fn run(
        &self,
        context: &mut C,
        params: A::Params,
    ) -> Result<Outcome<PreviewStatus, C>, PreviewError> {
        let (_, preview) = context.workbench_and_preview();
        A::act(preview, params)?;
        Ok(Outcome::outside_history(PreviewStatus::of(preview)))
    }
}

/// One of the controls that act on the running game alone.
trait Control: Send + Sync {
    const ID: &'static str;
    const SUMMARY: &'static str;
    fn apply(preview: &mut Preview) -> Result<(), PreviewError>;
}

struct PauseControl;
impl Control for PauseControl {
    const ID: &'static str = PAUSE;
    const SUMMARY: &'static str = "Pauses the running game between frames.";
    fn apply(preview: &mut Preview) -> Result<(), PreviewError> {
        preview.pause()
    }
}

struct ResumeControl;
impl Control for ResumeControl {
    const ID: &'static str = RESUME;
    const SUMMARY: &'static str = "Resumes the paused game.";
    fn apply(preview: &mut Preview) -> Result<(), PreviewError> {
        preview.resume()
    }
}

struct StepControl;
impl Control for StepControl {
    const ID: &'static str = STEP;
    const SUMMARY: &'static str =
        "Runs one server frame of the game, pausing it first, and leaves it paused.";
    fn apply(preview: &mut Preview) -> Result<(), PreviewError> {
        preview.step()
    }
}

struct RestartControl;
impl Control for RestartControl {
    const ID: &'static str = RESTART;
    const SUMMARY: &'static str = "Starts the running game's map again.";
    fn apply(preview: &mut Preview) -> Result<(), PreviewError> {
        preview.restart()
    }
}

struct StopControl;
impl Control for StopControl {
    const ID: &'static str = STOP;
    const SUMMARY: &'static str = "Stops the game.";
    fn apply(preview: &mut Preview) -> Result<(), PreviewError> {
        preview.stop();
        Ok(())
    }
}

struct PollControl;
impl Control for PollControl {
    const ID: &'static str = POLL;
    const SUMMARY: &'static str =
        "Reads the running game's events: its state changes and console lines.";
    fn apply(preview: &mut Preview) -> Result<(), PreviewError> {
        preview.poll();
        Ok(())
    }
}

/// A [`Control`] as a command.
struct ControlCommand<A>(core::marker::PhantomData<A>);

impl<A: Control + 'static, C: HasPreview + 'static> Command<C> for ControlCommand<A> {
    const ID: &'static str = A::ID;
    const SUMMARY: &'static str = A::SUMMARY;
    type Params = NoParams;
    type Output = PreviewStatus;
    type Error = PreviewError;

    fn run(
        &self,
        context: &mut C,
        NoParams {}: NoParams,
    ) -> Result<Outcome<PreviewStatus, C>, PreviewError> {
        let (_, preview) = context.workbench_and_preview();
        A::apply(preview)?;
        Ok(Outcome::outside_history(PreviewStatus::of(preview)))
    }
}

/// `preview.play`: builds the project and starts the game on it.
struct PlayCommand;

impl<C: HasPreview + 'static> Command<C> for PlayCommand {
    const ID: &'static str = PLAY;
    const SUMMARY: &'static str = "Builds the open project and starts the preview host on its \
        program, stopping a game already running.";
    type Params = PlayParams;
    type Output = PreviewStatus;
    type Error = PreviewError;

    fn run(
        &self,
        context: &mut C,
        PlayParams { map }: PlayParams,
    ) -> Result<Outcome<PreviewStatus, C>, PreviewError> {
        let (workbench, preview) = context.workbench_and_preview();
        preview.play(workbench, map)?;
        Ok(Outcome::outside_history(PreviewStatus::of(preview)))
    }
}

/// `preview.reload`: builds the project and reloads it in the running game.
struct ReloadCommand;

impl<C: HasPreview + 'static> Command<C> for ReloadCommand {
    const ID: &'static str = RELOAD;
    const SUMMARY: &'static str = "Builds the open project and reloads its program in the \
        running game, the map going on; nothing when no game runs.";
    type Params = NoParams;
    type Output = PreviewStatus;
    type Error = PreviewError;

    fn run(
        &self,
        context: &mut C,
        NoParams {}: NoParams,
    ) -> Result<Outcome<PreviewStatus, C>, PreviewError> {
        let (workbench, preview) = context.workbench_and_preview();
        preview.reload(workbench)?;
        Ok(Outcome::outside_history(PreviewStatus::of(preview)))
    }
}

/// Registers the `preview.*` commands.
///
/// # Errors
///
/// [`RegisterError`] when one of their ids is taken.
pub fn register_preview<C: HasPreview + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(PlayCommand)?;
    registry.register(ControlCommand::<PauseControl>(core::marker::PhantomData))?;
    registry.register(ControlCommand::<ResumeControl>(core::marker::PhantomData))?;
    registry.register(ControlCommand::<StepControl>(core::marker::PhantomData))?;
    registry.register(ControlCommand::<RestartControl>(core::marker::PhantomData))?;
    registry.register(ControlCommand::<StopControl>(core::marker::PhantomData))?;
    registry.register(ControlCommand::<PollControl>(core::marker::PhantomData))?;
    registry.register(ReloadCommand)?;
    registry.register(SelectCommand)?;
    registry.register(PickCommand)?;
    registry.register(SetFieldCommand)?;
    registry.register(ActCommand::<BreakpointAct>(core::marker::PhantomData))?;
    registry.register(ActCommand::<ConsoleAct>(core::marker::PhantomData))?;
    registry.register(ActCommand::<WatchAct>(core::marker::PhantomData))?;
    registry.register(ActCommand::<UnwatchAct>(core::marker::PhantomData))?;
    registry.register(ActCommand::<StepByAct>(core::marker::PhantomData))?;
    Ok(())
}

#[cfg(test)]
mod tests;
