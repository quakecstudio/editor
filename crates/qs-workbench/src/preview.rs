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

use qc_engines::Engine;
use qc_ide::ModuleBuild;
use qc_project::Loader;
use qs_engine::{
    Client, ClientError, EmbeddedHost, EntityTarget, ErrorCode, Flavour, InspectWhat, LaunchError,
    LaunchSpec, RunRootError, StepKind, WindowPlacement, locate_host, prepare_run_root,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use qs_commands::{Command, NoParams, Outcome, RegisterError, Registry};
use schemars::JsonSchema;

use crate::full_build::{FullBuild, Start};
use crate::ids::{BreakpointId, Edict, WatchIndex};
use crate::map_compile::{MapCompile, MapLeaks, MapPlayFromHere, MapPortalsCommand};
use crate::map_profile::{MapChooseProfile, MapProfiles, MapRunProfile};
use crate::{GameFolders, HasWorkbench, LiveMap, ModuleBuilt, Workbench, WorkbenchError};
use qs_map::Entity;
use qs_map_edit::{LiveChange, register_map_game};
use qs_map_light::Sidecars;

pub mod debug;
pub mod dock;
pub mod engine;
pub mod entities;
pub mod game_files;
pub mod scenario;

use debug::{Debugger, Location, StopReason};
use dock::DockFrame;

use entities::Entities;

/// The most console lines the preview keeps; older ones are dropped.
const CONSOLE_LINES: usize = 1000;
/// How long the host has to stop after `shutdown` before it is killed.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(5);
/// How long `stop` waits between looks at whether the host has stopped.
const SHUTDOWN_POLL: Duration = Duration::from_millis(20);

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
    /// A file the command writes could not be written.
    Write(PathBuf, std::io::Error),
    /// The player's box fits nowhere near the eye given: in or beside solid, or off the map.
    NoRoom([f64; 3]),
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
            Self::Write(path, error) => write!(f, "{}: {error}", path.display()),
            Self::NoRoom([x, y, z]) => write!(
                f,
                "there is no room for the player at ({x} {y} {z}) or above or below it: move \
                 the camera out of the wall or floor"
            ),
        }
    }
}

impl std::error::Error for PreviewError {}

impl From<ClientError> for PreviewError {
    fn from(error: ClientError) -> Self {
        Self::Bridge(error)
    }
}

/// Where the game runs.
enum Host {
    /// The preview host's process, which the preview stops and waits for.
    Process(std::process::Child),
    /// The app's own process, in its window (ADR-0028); the embedded host stops it.
    Embedded,
}

/// A host that runs, and what it plays.
struct Live {
    /// The bridge to it.
    client: Client,
    host: Host,
    /// The program it loaded, which a reload reads again.
    progs: PathBuf,
    /// The map the game is on, from its last `map/loaded` (BRIDGE-3.1).
    map: Option<String>,
    /// The classnames of the map's entities the game loaded, in order (BRIDGE-4.13).
    classnames: Vec<String>,
    /// Whether the map editor's list of entities is the game's (P5-07).
    step: Step,
    /// How many of the map editor's changes have been sent, each of which may move the map's
    /// entities in the game (BRIDGE-4.13).
    edits: u64,
}

/// Whether the map editor's entities and the game's are in step (P5-07).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Not yet compared since the map loaded.
    Unchecked,
    /// The same entities, in the same order.
    Same,
    /// Not the same: the editor's changes wait for the next compile.
    Apart,
}

/// The play loop's state: the host, when one runs, its state and its console.
pub struct Preview {
    config: PreviewConfig,
    live: Option<Live>,
    /// The game the project last played is for (ADR-0008), whose files are read first.
    flavour: Flavour,
    state: PlayState,
    console: VecDeque<ConsoleLine>,
    /// The last thing the loop did, for the toolbar: `Reloaded 0.9 s after save`.
    status: Option<String>,
    /// The game's edicts and the selected one.
    entities: Entities,
    /// What the reload held back would reset, from the host's dry run (BRIDGE-4.11, P3-10):
    /// each value lost, as `kind name: reason`. Empty when no reload waits.
    would_reset: Vec<String>,
    /// The classname of the edict a click in the game's window picked, for the shell to open
    /// its spawn function (P3-15).
    picked: Option<String>,
    /// The edits made while paused, which the host applies on resume (BRIDGE-4.4): each
    /// edict, field and the value as typed.
    pending: Vec<(u32, String, String)>,
    /// The breakpoints, where the game stopped, its stack and the watches.
    debugger: Debugger,
    /// The IDE's window as last told, and the placement last sent to the host.
    dock: (Option<DockFrame>, Option<WindowPlacement>),
    /// The full compile of the map Play from here started, swapped in when it is done (P7-02).
    full_build: Option<FullBuild>,
    /// A full compile asked for while one runs, which starts when it is done: only one runs.
    queued: Option<Start>,
    /// The app's own window as the game's host (ADR-0028), when the app can show the game.
    embedded: Option<Box<dyn EmbeddedHost>>,
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
            flavour: Flavour::Original,
            state: PlayState::Stopped,
            console: VecDeque::new(),
            status: None,
            entities: Entities::new(),
            picked: None,
            would_reset: Vec::new(),
            pending: Vec::new(),
            debugger: Debugger::new(),
            dock: (None, None),
            full_build: None,
            queued: None,
            embedded: None,
        }
    }

    /// A preview that can also show the game in the app's own window through `host`
    /// (ADR-0028): each Play asks it whether it shows the game now, and starts the game
    /// through it when it does, else in the preview host's process.
    #[must_use]
    pub fn with_embedded(config: PreviewConfig, host: Box<dyn EmbeddedHost>) -> Self {
        let mut preview = Self::new(config);
        preview.embedded = Some(host);
        preview
    }

    /// Whether the game that runs is shown in the app's own window.
    #[must_use]
    pub fn is_embedded(&self) -> bool {
        self.live
            .as_ref()
            .is_some_and(|live| matches!(live.host, Host::Embedded))
    }

    /// Docks the game's window beside the IDE's window `frame` (P3-11): the host is told
    /// with `window/place` when the placement changes, and the next game opens there. A host
    /// that cannot place its window (Wayland) is left where it opened.
    ///
    /// # Errors
    ///
    /// The bridge's error.
    pub fn dock(&mut self, frame: DockFrame) -> Result<(), PreviewError> {
        self.dock.0 = Some(frame);
        let placement = frame.placement();
        if self.dock.1 == Some(placement) {
            return Ok(());
        }
        let Some(client) = self.client() else {
            return Ok(());
        };
        if !client.offers("window/place") {
            return Ok(());
        }
        client.place_window(placement)?;
        self.dock.1 = Some(placement);
        Ok(())
    }

    /// Whether the game's window follows the IDE: no game runs, or its host places its window
    /// (BRIDGE-4.10).
    #[must_use]
    pub fn docks(&self) -> bool {
        self.live
            .as_ref()
            .is_none_or(|live| live.client.offers("window/place"))
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
            breakpoint.id = answer
                .get("id")
                .and_then(Value::as_u64)
                .map(BreakpointId::new);
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
    pub fn unwatch(&mut self, index: WatchIndex) {
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
    pub fn pending(&self, edict: Edict, field: &str) -> Option<&str> {
        self.pending
            .iter()
            .rev()
            .find(|(e, f, _)| *e == edict.get() && f == field)
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
    pub fn select(&mut self, edict: Edict) -> Result<(), PreviewError> {
        let answer = self
            .live_client()?
            .inspect(InspectWhat::Edict(edict.into()))?;
        self.entities.select(&answer);
        Ok(())
    }

    /// The game folder Play reads from, when one was chosen (`preview.choose_game`) or
    /// configured; `None` for the first install found.
    #[must_use]
    pub fn game_folder(&self) -> Option<&Path> {
        self.config.game.as_deref()
    }

    /// The base directory the game's files are read from: the game's the project last played
    /// is for, else the other game's where the install has only that one; `None` when no
    /// install is found.
    #[must_use]
    pub fn game_base_path(&self) -> Option<std::path::PathBuf> {
        self.game_base().ok().map(|base| base.path)
    }

    /// What the reload held back would reset (P3-10), each as `kind name: reason`; empty when
    /// no reload waits.
    #[must_use]
    pub fn would_reset(&self) -> &[String] {
        &self.would_reset
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
        self.select(Edict::new(edict))?;
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
        let answer = self.live_client()?.set_field(edict.into(), field, value)?;
        // BRIDGE-4.4: while paused between frames the host holds the edit until it resumes.
        if answer.get("applied").and_then(Value::as_str) == Some("on_resume") {
            self.pending
                .push((edict.get(), field.to_owned(), text.trim().to_owned()));
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

    /// Replaces the line [`Preview::status`] shows.
    pub(crate) fn set_status(&mut self, status: String) {
        self.status = Some(status);
    }

    /// The console's lines, oldest first.
    pub fn console(&self) -> impl Iterator<Item = &ConsoleLine> {
        self.console.iter()
    }

    /// The bridge client of the running host.
    pub fn client(&mut self) -> Option<&mut Client> {
        self.live.as_mut().map(|live| &mut live.client)
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
        let built = workbench.built_program()?;
        self.play_built(built, map)
    }

    /// Starts the host on the program `built` gives, the project, its program and its mod
    /// folder, on `map` or the host's own first map; a host already running is stopped first.
    pub(crate) fn play_built(
        &mut self,
        built: BuiltProgram,
        map: Option<String>,
    ) -> Result<(), PreviewError> {
        let BuiltProgram {
            project,
            progs,
            mod_name,
            flavour,
        } = built;
        self.stop();
        // ADR-0008: the project's own game, the rerelease's files for a rerelease project.
        let (base, flavour) = game_base(self.config.game.as_deref(), flavour)?;
        self.flavour = flavour;
        let run_root =
            prepare_run_root(&project, flavour, &base, &mod_name).map_err(PreviewError::RunRoot)?;
        // A game embedded in the app's window needs no host program.
        let embeds = self.embedded.as_ref().is_some_and(|host| host.shows());
        let host = if embeds {
            PathBuf::new()
        } else {
            locate_host(self.config.host.as_deref(), &self.config.app)
                .map_err(PreviewError::Launch)?
        };
        let mut extra = self.config.extra.clone();
        // The window opens where it docks; `window/place` then sets its frame exactly.
        if let Some(frame) = self.dock.0.filter(|_| !embeds) {
            let at = frame.placement();
            extra.extend(["--position", &at.x.to_string(), &at.y.to_string()].map(str::to_owned));
            extra.extend(
                ["--size", &at.width.to_string(), &at.height.to_string()].map(str::to_owned),
            );
        }
        self.dock.1 = None;
        let spec = LaunchSpec {
            host,
            run_root,
            map,
            socket: socket_path(),
            extra,
        };
        let (client, host) = match self.embedded.as_mut().filter(|_| embeds) {
            Some(embedded) => {
                let client = embedded
                    .start(&spec, "QuakeC Studio")
                    .map_err(PreviewError::Launch)?;
                (client, Host::Embedded)
            }
            None => {
                let running = spec.launch("QuakeC Studio").map_err(PreviewError::Launch)?;
                (running.client, Host::Process(running.child))
            }
        };
        self.live = Some(Live {
            client,
            host,
            progs,
            map: None,
            classnames: Vec::new(),
            step: Step::Unchecked,
            edits: 0,
        });
        self.state = PlayState::Playing;
        self.status = Some(format!("Playing {mod_name}"));
        self.console.clear();
        self.set_breakpoints();
        self.refresh()
    }

    /// The base directory the game's files are read from: the configured folder's, else an
    /// install's; the game's the project last played is for, else the rerelease's where that
    /// is the only game there, as before the first Play of a project for the rerelease.
    pub(crate) fn game_base(&self) -> Result<qs_install::GameBase, PreviewError> {
        let folder = self.config.game.as_deref();
        game_base(folder, self.flavour)
            .or_else(|_| game_base(folder, Flavour::Rerelease))
            .map(|(base, _)| base)
    }

    /// The running host's client, or [`PreviewError::NotRunning`].
    fn live_client(&mut self) -> Result<&mut Client, PreviewError> {
        self.client().ok_or(PreviewError::NotRunning)
    }

    /// Pauses the game between frames (BRIDGE-4.9).
    ///
    /// # Errors
    ///
    /// [`PreviewError::NotRunning`] with no host, or the bridge's error.
    pub fn pause(&mut self) -> Result<(), PreviewError> {
        self.live_client()?.pause()?;
        self.state = PlayState::Paused;
        Ok(())
    }

    /// Resumes a paused game (BRIDGE-4.9).
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
        on_loss: OnLoss,
    ) -> Result<Option<Value>, PreviewError> {
        if self.live.is_none() {
            return Ok(None);
        }
        let started = Instant::now();
        let progs = workbench.built_program()?.progs;
        let path = progs.to_string_lossy().into_owned();
        if on_loss == OnLoss::Ask {
            // P3-10: the host's own migration, run over a copy of the world, says what the
            // reload would lose; a loss holds the reload for the modder to choose.
            let predicted = self.live_client()?.predict_reload(&path)?;
            let lost = would_reset(&predicted);
            if !lost.is_empty() {
                self.status = Some(format!("Reloading would reset {}", lost.len()));
                self.would_reset = lost;
                return Ok(None);
            }
        }
        self.would_reset.clear();
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
        self.full_build = None;
        self.queued = None;
        let Some(mut live) = self.live.take() else {
            return;
        };
        if let Err(error) = live.client.shutdown() {
            tracing::debug!(%error, "the preview host did not answer shutdown");
        }
        match &mut live.host {
            Host::Process(child) => {
                let started = Instant::now();
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => break,
                        Ok(None) if started.elapsed() < SHUTDOWN_WAIT => {
                            std::thread::sleep(SHUTDOWN_POLL);
                        }
                        Ok(None) | Err(_) => {
                            if let Err(error) = child.kill() {
                                tracing::warn!(%error, "the preview host could not be stopped");
                            }
                            if let Err(error) = child.wait() {
                                tracing::warn!(%error, "the preview host was not waited for");
                            }
                            break;
                        }
                    }
                }
            }
            Host::Embedded => {
                if let Some(embedded) = &mut self.embedded {
                    embedded.stop();
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
        // A pick is reported by the poll that read it, and by no later one.
        self.picked = None;
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
                // A game in the app's own window is the embedded host's to end.
                if self.is_embedded()
                    && let Some(embedded) = &mut self.embedded
                {
                    embedded.stop();
                }
                self.live = None;
                self.full_build = None;
                self.queued = None;
                self.state = PlayState::Stopped;
                self.status = Some("The game closed".to_owned());
                self.entities.clear();
            }
        }
        self.finish_full_build();
    }

    /// A warning for the console, from a command.
    pub(crate) fn note(&mut self, text: &str) {
        self.push_line(text, "warning");
    }

    /// Starts the full compile of the map `map` the game plays from `target`, `bsp` the fast
    /// compile's BSP before its light, `portals` its portal file, `sidecars` what the compile
    /// tells light and `palette` the game's, to be swapped in when it is
    /// done (P7-02); one asked for while another runs waits for it, the other's result dropped.
    pub(crate) fn start_full_build(
        &mut self,
        map: String,
        target: PathBuf,
        (bsp, portals, sidecars, palette): (Vec<u8>, Vec<u8>, Sidecars, Option<Vec<u8>>),
    ) {
        let edits = self.live.as_ref().map_or(0, |live| live.edits);
        let start = Start {
            map,
            target,
            bsp,
            portals,
            sidecars,
            palette,
            edits,
        };
        if self.full_build.is_some() {
            self.queued = Some(start);
        } else {
            self.full_build = Some(FullBuild::start(start));
        }
    }

    /// The full compile, once it is done: a queued one started in its place, or this one swapped
    /// in ([`Preview::swap_full_build`]).
    fn finish_full_build(&mut self) {
        let Some(build) = self.full_build.as_mut() else {
            return;
        };
        if build.ready.is_none() {
            let Some(built) = build.take() else {
                return;
            };
            if let Some(next) = self.queued.take() {
                if let Ok(ready) = &built {
                    drop(std::fs::remove_file(&ready.bsp));
                }
                self.full_build = Some(FullBuild::start(next));
                return;
            }
            match built {
                Ok(ready) => build.ready = Some(ready),
                Err(error) => {
                    self.full_build = None;
                    self.note(&format!("The full compile was not swapped in: {error}"));
                    return;
                }
            }
        }
        self.swap_full_build();
    }

    /// The full compile that is ready swapped into the game in place of the fast one
    /// (BRIDGE-4.14), the player where they stand, then put where the played BSP is; said in the
    /// console. It is dropped when the game has left its map, when the map's entities were edited
    /// in the game since it started, which moves their places, or when another compile was
    /// written over the played BSP; it is tried again while the game is stopped at a statement.
    fn swap_full_build(&mut self) {
        let Some(build) = self.full_build.as_ref() else {
            return;
        };
        let live = self.live.as_ref();
        let on_map = live
            .and_then(|live| live.map.as_deref())
            .is_some_and(|map| map.eq_ignore_ascii_case(&build.map));
        let edited = live.is_some_and(|live| live.edits != build.edits);
        let stale = !build.current();
        let Some(ready) = build.ready.as_ref() else {
            return;
        };
        if !on_map || edited || stale {
            drop(std::fs::remove_file(&ready.bsp));
            self.full_build = None;
            if edited {
                self.note("The full compile was not swapped in: the map's entities changed in the game while it ran; it comes with the next Play from here");
            }
            return;
        }
        // The `.lit` beside the played BSP is the one the host draws the swapped one with; the
        // fast one's is put back if the swap is refused.
        let colour = build.target.with_extension("lit");
        let before = std::fs::read(&colour).ok();
        let put = |bytes: Option<&Vec<u8>>| match bytes {
            Some(bytes) => std::fs::write(&colour, bytes),
            None if colour.exists() => std::fs::remove_file(&colour),
            None => Ok(()),
        };
        let (side, models, target) = (
            ready.bsp.clone(),
            ready.models.clone(),
            build.target.clone(),
        );
        let swapped = put(ready.lit.as_ref())
            .map_err(|error| format!("{}: {error}", colour.display()))
            .and_then(|()| {
                let client = self.client().ok_or("the game stopped")?;
                client
                    .swap_map(&side.to_string_lossy(), &models)
                    .map_err(|error| match error {
                        ClientError::Rpc {
                            code: ErrorCode::StoppedInFrame,
                            ..
                        } => RETRY.to_owned(),
                        other => other.to_string(),
                    })
            });
        match swapped {
            Ok(_) => {
                self.full_build = None;
                match std::fs::rename(&side, &target) {
                    Ok(()) => self.push_line("The full compile is in: full vis and light", "print"),
                    Err(error) => self.note(&format!(
                        "The full compile is in, but {} was not written: {error}",
                        target.display()
                    )),
                }
            }
            Err(why) => {
                if let Err(error) = put(before.as_ref()) {
                    tracing::warn!(%error, "the fast compile's .lit was not put back");
                }
                if why != RETRY {
                    drop(std::fs::remove_file(&side));
                    self.full_build = None;
                    self.note(&format!("The full compile was not swapped in: {why}"));
                }
            }
        }
    }

    /// Waits for the full compile and swaps it in. For tests.
    #[cfg(test)]
    pub(crate) fn wait_for_full_build(&mut self) {
        // As the poll does: one queued behind it starts in its place, its result dropped.
        while let Some(next) = self.queued.take() {
            if let Some(Ok(ready)) = self.full_build.as_ref().map(FullBuild::wait) {
                drop(std::fs::remove_file(&ready.bsp));
            }
            self.full_build = Some(FullBuild::start(next));
        }
        let built = self.full_build.as_ref().map(FullBuild::wait);
        match built {
            Some(Ok(ready)) => {
                if let Some(build) = self.full_build.as_mut() {
                    build.ready = Some(ready);
                }
                self.swap_full_build();
            }
            Some(Err(error)) => {
                self.full_build = None;
                self.note(&format!("The full compile was not swapped in: {error}"));
            }
            None => {}
        }
    }

    /// One event (BRIDGE-3): `state/changed` sets the play state and `console/line` adds a
    /// line.
    fn take_event(&mut self, method: &str, params: &Value) {
        match method {
            "state/changed" => self.state_changed(params),
            "map/loaded" => {
                if let Some(live) = self.live.as_mut() {
                    live.map = params.get("map").and_then(Value::as_str).map(str::to_owned);
                    live.classnames = params
                        .get("classnames")
                        .and_then(Value::as_array)
                        .map_or_default(|names| {
                            names
                                .iter()
                                .map(|name| name.as_str().unwrap_or_default().to_owned())
                                .collect()
                        });
                    live.step = Step::Unchecked;
                }
                if self.entities.take_event(method, params)
                    && let Err(error) = self.refresh()
                {
                    tracing::debug!(%error, "the entities were not read again");
                }
            }
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
            // BRIDGE-3.5: a click in the paused game's window picked an edict; it is selected,
            // and its spawn function is the shell's to open.
            "edict/picked" => {
                let edict = params
                    .get("edict")
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .map(Edict::new);
                if let Some(edict) = edict {
                    self.picked = self
                        .entities
                        .classname(edict)
                        .filter(|classname| !classname.is_empty())
                        .map(str::to_owned);
                    if let Err(error) = self.select(edict) {
                        tracing::debug!(%error, "the picked edict was not read");
                    }
                }
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
    /// Whether the map editor's changes to the map at `path` go to the game: it plays that
    /// map, the map named by the file's stem in any case, and has not been found out of step
    /// with the editor since the map loaded (P5-07).
    #[must_use]
    pub fn plays_in_step(&self, path: &Path) -> bool {
        let Some(live) = self.live.as_ref() else {
            return false;
        };
        let stem = path.file_stem().and_then(|stem| stem.to_str());
        let on_map = live
            .map
            .as_deref()
            .zip(stem)
            .is_some_and(|(map, stem)| map.eq_ignore_ascii_case(stem));
        on_map && live.step != Step::Apart
    }

    /// Puts the game playing the map at `path` out of step with the editor, which read the map
    /// again from disk: its edits reach the game at the next compile (P5-17), said once.
    pub fn map_read_again(&mut self, path: &Path) {
        if !self.plays_in_step(path) {
            return;
        }
        if let Some(live) = self.live.as_mut() {
            live.step = Step::Apart;
        }
        self.push_line(
            "The map was read again from disk: its edits reach the game at the next compile",
            "warning",
        );
    }

    /// Sends the map editor's `changes` to the game when [`Preview::plays_in_step`] the map at
    /// `path` (P5-07, MAP-12). The first time after the map loads, the editor's entities
    /// before the change, `before`, are checked against those the game loaded, by classname in
    /// order (BRIDGE-4.13): a map edited since it was compiled, or one whose compiler merged
    /// entities into the world, is out of step, and its changes reach the game at the next
    /// compile. A change the game refuses, or a brush entity placed, which needs a compile,
    /// puts the two out of step too; each case is said once in the console.
    pub fn send_map_changes(&mut self, path: &Path, before: &[Entity], changes: &[LiveChange]) {
        if !self.plays_in_step(path) {
            return;
        }
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let mut note = None;
        if live.step == Step::Unchecked {
            let editor: Vec<String> = before
                .iter()
                .map(|entity| {
                    String::from_utf8_lossy(entity.classname().unwrap_or_default()).into_owned()
                })
                .collect();
            if live.classnames == editor {
                live.step = Step::Same;
            } else {
                live.step = Step::Apart;
                note = Some(
                    "The map in the editor is not the one the game loaded: its edits reach the \
                     game at the next compile"
                        .to_owned(),
                );
            }
        }
        for change in changes {
            if live.step == Step::Apart {
                break;
            }
            live.edits = live.edits.saturating_add(1);
            let sent = match change {
                LiveChange::NeedsCompile { .. } => Err(
                    "a brush entity reaches the game at the next compile; later edits wait for it"
                        .to_owned(),
                ),
                _ => change
                    .send(&mut live.client)
                    .map(drop)
                    .map_err(|error| error.to_string()),
            };
            if let Err(why) = sent {
                live.step = Step::Apart;
                note = Some(format!("The map edit did not reach the game: {why}"));
            }
        }
        if let Some(line) = note {
            self.push_line(&line, "warning");
        }
    }

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

impl<L: Loader> Workbench<L> {
    /// Builds the project; its root, its first program and the mod folder it is in, or why there
    /// is none.
    pub(super) fn built_program(&mut self) -> Result<BuiltProgram, PreviewError> {
        let project = self
            .root()
            .map(Path::to_path_buf)
            .ok_or(PreviewError::Build(WorkbenchError::NoProject))?;
        let modules = self.build().map_err(PreviewError::Build)?;
        let Some(built) = modules.iter().find(|m| m.built) else {
            return Err(PreviewError::NothingBuilt(modules));
        };
        let progs = built.output.clone();
        let mod_name = mod_folder(&project, &progs)
            .ok_or_else(|| PreviewError::OutsideBuild(progs.clone()))?;
        // ADR-0008: the game of the module played, which another module may not be for.
        let played = built.name.clone();
        let flavour = self.flavour_where(|module| module.name == played);
        Ok(BuiltProgram {
            project,
            progs,
            mod_name,
            flavour,
        })
    }

    /// Which game the open project is for (ADR-0008): the rerelease when one of its modules
    /// is compiled for the rerelease's engine (ENGT-3.5), else the original.
    pub(crate) fn flavour(&mut self) -> Flavour {
        self.flavour_where(|_| true)
    }

    /// The rerelease when a module `wanted` is true of is compiled for the rerelease's
    /// engine, else the original.
    fn flavour_where(&mut self, wanted: impl Fn(&ModuleBuild<'_>) -> bool) -> Flavour {
        let rerelease = self
            .host
            .project_modules()
            .iter()
            .any(|module| module.engine == Engine::Kex && wanted(module));
        if rerelease {
            Flavour::Rerelease
        } else {
            Flavour::Original
        }
    }
}

/// A project built to play: its root, its first program, the mod folder the program is in and
/// the game it is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BuiltProgram {
    pub(crate) project: PathBuf,
    pub(crate) progs: PathBuf,
    pub(crate) mod_name: String,
    pub(crate) flavour: Flavour,
}

/// The base directory of the game of `flavour`, with the flavour it is: `folder`'s, else the
/// first install found with that game. Where no install has the rerelease a rerelease project
/// plays on the original game's files, as the original.
pub(crate) fn game_base(
    folder: Option<&Path>,
    flavour: Flavour,
) -> Result<(qs_install::GameBase, Flavour), PreviewError> {
    let installs = match folder {
        Some(folder) => qs_install::examine_folder(folder).into_iter().collect(),
        None => qs_install::find_installs(),
    };
    let rerelease = || {
        installs
            .iter()
            .find_map(|install| install.rerelease.clone())
            .map(|base| (base, Flavour::Rerelease))
    };
    let original = || {
        installs
            .iter()
            .find_map(|install| install.original.clone())
            .map(|base| (base, Flavour::Original))
    };
    let found = match flavour {
        Flavour::Rerelease => rerelease().or_else(original),
        Flavour::Original => original(),
    };
    found.ok_or_else(|| PreviewError::NoGame(folder.map(Path::to_path_buf)))
}

/// The folder name `<mod>` of a program at `<project>/build/<mod>/<file>`.
pub(crate) fn mod_folder(project: &Path, progs: &Path) -> Option<String> {
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

/// What a full compile's swap refused while the game is stopped at a statement says, so it is
/// tried again (BRIDGE-4.14).
const RETRY: &str = "stopped in a frame";

/// Sends a change of the map's entities to a game.
pub trait SendLive {
    /// Sends the change over the bridge (BRIDGE-4.13): `entity/spawn` at its place,
    /// `entity/remove` or `entity/edit` of the entity there; nothing for
    /// [`LiveChange::NeedsCompile`], answered with `null`.
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`]: the game refused it, as for a model the map has not
    /// precached (BRIDGE-4.12) or an entity in no edict.
    fn send(&self, client: &mut Client) -> Result<Value, ClientError>;
}

impl SendLive for LiveChange {
    fn send(&self, client: &mut Client) -> Result<Value, ClientError> {
        match self {
            LiveChange::Spawn {
                place,
                classname,
                pairs,
            } => client.spawn(classname, &as_strs(pairs), Some(place.get())),
            LiveChange::Remove { place } => client.remove(EntityTarget::MapEntity(place.get())),
            LiveChange::NeedsCompile { .. } => Ok(Value::Null),
            LiveChange::Edit { place, pairs } => {
                client.edit_entity(EntityTarget::MapEntity(place.get()), &as_strs(pairs))
            }
        }
    }
}

/// Key-values as the client takes them.
fn as_strs(pairs: &[(String, String)]) -> Vec<(&str, &str)> {
    pairs
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect()
}

/// What the map editor asks of the game (`HasMap`): the play loop's.
impl LiveMap for Preview {
    fn plays_in_step(&self, map: &Path) -> bool {
        Self::plays_in_step(self, map)
    }

    fn send_changes(&mut self, map: &Path, before: &[Entity], changes: &[LiveChange]) {
        self.send_map_changes(map, before, changes);
    }

    fn read_again(&mut self, map: &Path) {
        self.map_read_again(map);
    }

    fn game_folders(&self) -> GameFolders {
        GameFolders {
            base: self.game_base_path(),
            chosen: self.game_folder().map(Path::to_path_buf),
        }
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
/// The id of the command that docks the game's window beside the IDE's.
pub const DOCK: &str = "preview.dock";
/// The id of the command that plays a scenario file (SCN-2.6).
pub const SCENARIO_RUN: &str = "scenario.run";
/// The id of the command that steps the stopped game by a statement, into, over or out.
pub const STEP_BY: &str = "preview.step_by";

/// What `preview.reload` does when the reload would lose a value (P3-10, BRIDGE-4.11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnLoss {
    /// Reload all the same: what is lost resets.
    #[default]
    Reload,
    /// Ask first: the reload waits, and the status lists what it would reset.
    Ask,
}

/// The values a `reload` result says are lost, as `kind name: reason` (WORLD-10.5).
fn would_reset(result: &Value) -> Vec<String> {
    let text = |change: &Value, key: &str| {
        change
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    result
        .get("lost")
        .and_then(Value::as_array)
        .map_or_default(|lost| {
            lost.iter()
                .map(|c| {
                    format!(
                        "{} {}: {}",
                        text(c, "kind"),
                        text(c, "name"),
                        text(c, "reason")
                    )
                })
                .collect()
        })
}

/// What a `preview.*` command leaves: the play state and the last thing the loop did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PreviewStatus {
    /// `stopped`, `playing` or `paused`.
    pub state: String,
    /// The last thing the loop did, such as `Reloaded 0.4 s after save`.
    pub status: Option<String>,
    /// The classname of the edict a click in the paused game's window picked, read by this
    /// poll, whose spawn function the shell opens (BRIDGE-3.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picked: Option<String>,
    /// What the reload held back would reset, each as `kind name: reason` (P3-10).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub would_reset: Vec<String>,
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
            picked: preview.picked.clone(),
            would_reset: preview.would_reset.clone(),
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
    pub edict: Edict,
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
    pub edict: Option<Edict>,
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
    pub index: WatchIndex,
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

struct DockAct;
impl Act for DockAct {
    const ID: &'static str = DOCK;
    const SUMMARY: &'static str = "Docks the game's window beside the IDE's window, given \
        where the IDE's window is and whether it has the focus.";
    type Params = DockFrame;
    fn act(preview: &mut Preview, frame: DockFrame) -> Result<(), PreviewError> {
        preview.dock(frame)
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
/// The parameters of `preview.reload`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReloadParams {
    /// What to do when the reload would lose a value: `reload` (the default) or `ask`.
    #[serde(default)]
    pub on_loss: OnLoss,
}

struct ReloadCommand;

impl<C: HasPreview + 'static> Command<C> for ReloadCommand {
    const ID: &'static str = RELOAD;
    const SUMMARY: &'static str = "Builds the open project and reloads its program in the \
        running game, the map going on; nothing when no game runs.";
    type Params = ReloadParams;
    type Output = PreviewStatus;
    type Error = PreviewError;

    fn run(
        &self,
        context: &mut C,
        params: ReloadParams,
    ) -> Result<Outcome<PreviewStatus, C>, PreviewError> {
        let (workbench, preview) = context.workbench_and_preview();
        preview.reload(workbench, params.on_loss)?;
        Ok(Outcome::outside_history(PreviewStatus::of(preview)))
    }
}

/// The parameters of `scenario.run`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScenarioRunParams {
    /// The scenario file (`docs/spec/scenario.md`).
    pub path: PathBuf,
}

/// `scenario.run`: builds the open project and plays a scenario on its program (SCN-2.6).
struct ScenarioRunCommand;

impl<C: HasPreview + 'static> Command<C> for ScenarioRunCommand {
    const ID: &'static str = SCENARIO_RUN;
    const SUMMARY: &'static str = "Builds the open project and plays a scenario file on it in \
        the server simulation, answering whether each assertion held, with the values read, and \
        writing its screenshots beside it.";
    type Params = ScenarioRunParams;
    type Output = scenario::ScenarioRun;
    type Error = scenario::ScenarioRunError;

    fn run(
        &self,
        context: &mut C,
        ScenarioRunParams { path }: ScenarioRunParams,
    ) -> Result<Outcome<scenario::ScenarioRun, C>, scenario::ScenarioRunError> {
        let (workbench, preview) = context.workbench_and_preview();
        let ran = preview.run_scenario(workbench, &path)?;
        Ok(Outcome::outside_history(ran))
    }
}

/// The id of the command that says what game data the preview can play (P3-21).
pub const GAME: &str = "preview.game";
/// The id of the command that chooses the game folder the preview plays from (P3-21).
pub const CHOOSE_GAME: &str = "preview.choose_game";

/// `preview.game`: what the configured game folder, or the first install found, holds.
struct GameCommand;

impl<C: HasPreview + 'static> Command<C> for GameCommand {
    const ID: &'static str = GAME;
    const SUMMARY: &'static str = "Says what game data the preview plays from: the chosen \
        folder, or the first install found, as found, shareware only, rerelease only or missing for \
        the game the project is for, with the files it lacks.";
    type Params = NoParams;
    type Output = game_files::GameFiles;
    type Error = PreviewError;

    fn run(
        &self,
        context: &mut C,
        NoParams {}: NoParams,
    ) -> Result<Outcome<game_files::GameFiles, C>, PreviewError> {
        let (workbench, preview) = context.workbench_and_preview();
        let files =
            game_files::GameFiles::check(preview.config.game.as_deref(), workbench.flavour());
        Ok(Outcome::outside_history(files))
    }
}

/// The parameters of `preview.choose_game`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChooseGameParams {
    /// The folder: a store's install, a steamcmd download, an innoextract output, or one
    /// holding `id1`.
    pub path: PathBuf,
}

/// `preview.choose_game`: the folder Play reads the game from, read in place; kept only when
/// it can be played.
struct ChooseGameCommand;

impl<C: HasPreview + 'static> Command<C> for ChooseGameCommand {
    const ID: &'static str = CHOOSE_GAME;
    const SUMMARY: &'static str = "Chooses the folder the preview reads the game from, in \
        place, and says what it holds; a folder that cannot be played is not kept.";
    type Params = ChooseGameParams;
    type Output = game_files::GameFiles;
    type Error = PreviewError;

    fn run(
        &self,
        context: &mut C,
        params: ChooseGameParams,
    ) -> Result<Outcome<game_files::GameFiles, C>, PreviewError> {
        let (workbench, preview) = context.workbench_and_preview();
        let files = game_files::GameFiles::check(Some(&params.path), workbench.flavour());
        if files.state.plays() {
            preview.config.game = Some(params.path);
        }
        Ok(Outcome::outside_history(files))
    }
}

/// Registers the `preview.*` commands and `scenario.run`.
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
    registry.register(ScenarioRunCommand)?;
    registry.register(GameCommand)?;
    registry.register(ChooseGameCommand)?;
    registry.register(SelectCommand)?;
    registry.register(PickCommand)?;
    register_map_game(registry)?;
    registry.register(MapLeaks)?;
    registry.register(MapCompile)?;
    registry.register(MapPlayFromHere)?;
    registry.register(MapPortalsCommand)?;
    registry.register(MapChooseProfile)?;
    registry.register(MapProfiles)?;
    registry.register(MapRunProfile)?;
    registry.register(SetFieldCommand)?;
    registry.register(ActCommand::<BreakpointAct>(core::marker::PhantomData))?;
    registry.register(ActCommand::<ConsoleAct>(core::marker::PhantomData))?;
    registry.register(ActCommand::<WatchAct>(core::marker::PhantomData))?;
    registry.register(ActCommand::<DockAct>(core::marker::PhantomData))?;
    registry.register(ActCommand::<UnwatchAct>(core::marker::PhantomData))?;
    registry.register(ActCommand::<StepByAct>(core::marker::PhantomData))?;
    engine::register_engine(registry)
}

#[cfg(test)]
impl Preview {
    /// A preview playing `running` on the map `map`, for tests of what is sent to its game.
    pub(crate) fn playing(running: qs_engine::Running, map: &str, classnames: &[&str]) -> Self {
        let mut preview = Self::new(PreviewConfig::default());
        preview.live = Some(Live {
            client: running.client,
            host: Host::Process(running.child),
            progs: PathBuf::new(),
            map: Some(map.to_owned()),
            classnames: classnames.iter().map(|&name| name.to_owned()).collect(),
            step: Step::Unchecked,
            edits: 0,
        });
        preview.state = PlayState::Playing;
        preview
    }
}

#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod tests;
