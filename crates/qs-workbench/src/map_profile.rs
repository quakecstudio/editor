// SPDX-License-Identifier: GPL-3.0-or-later

//! Compile profiles (P6-20, MAP-28): named lists of tasks that compile the open map with our
//! tools or others and start the game on it, as TrenchBroom's compile profiles run a map's
//! tools and launch an engine. A task's paths are written with expressions, `${MAP_DIR}` and
//! the others of [`Variable`], expanded for the open map when the profile runs.
//!
//! Two profiles are built in, [`FULL_PROFILE`] and [`FAST_PROFILE`]; a project adds its own in
//! [`PROFILES_FILE`] at its root, a TOML file of `[[profile]]` tables, each a `name` and its
//! `tasks`. A project's profile of a built-in's name replaces it.

use std::path::{Path, PathBuf};
use std::time::Instant;

use qs_commands::{Command, NoParams, Outcome};
use qs_map_light::{Light, LightError, Sampling};
use qs_map_vis::{Mode, Vis, VisError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::map_compile::MapReport;
use crate::preview::{HasPreview, PreviewError};
use crate::{Workbench, WorkbenchError};

/// The id of `map.profiles`.
pub const MAP_PROFILES: &str = "map.profiles";
/// The id of `map.run_profile`.
pub const MAP_RUN_PROFILE: &str = "map.run_profile";
/// The id of `map.choose_profile`.
pub const MAP_CHOOSE_PROFILE: &str = "map.choose_profile";
/// The file at a project's root that holds its compile profiles.
pub const PROFILES_FILE: &str = "compile-profiles.toml";
/// The built-in profile that compiles with vis and full light, then plays.
pub const FULL_PROFILE: &str = "Full";
/// The built-in profile that compiles with fast vis and fast light, then plays.
pub const FAST_PROFILE: &str = "Fast";

/// How much work vis does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VisSpeed {
    /// The portal flow, vis's default.
    #[default]
    Full,
    /// `vis -fast`.
    Fast,
}

/// How many samples light takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LightSpeed {
    /// `light -extra`: four samples a luxel.
    #[default]
    Full,
    /// One sample a luxel, light's default.
    Fast,
}

/// One task of a compile profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "task", rename_all = "snake_case", deny_unknown_fields)]
pub enum Task {
    /// qs-map-bsp: the map as it is in the editor, saved or not, to `${BSP}`, with its portal
    /// and leak files beside it, as `map.compile` writes them.
    Bsp,
    /// qs-map-vis: `${BSP}` given its visibility from the portal file beside it.
    Vis {
        /// How much work vis does.
        #[serde(default)]
        speed: VisSpeed,
    },
    /// qs-map-light: `${BSP}` lit, with a `.lit` beside it when a light is coloured.
    Light {
        /// How many samples it takes.
        #[serde(default)]
        speed: LightSpeed,
    },
    /// A program run in `${MAP_DIR}` with these arguments, each expanded; a status other than
    /// success stops the profile. It reads the map's file as last saved.
    Tool {
        /// The program: a path, or a name found on the `PATH`.
        program: String,
        /// Its arguments.
        #[serde(default)]
        args: Vec<String>,
    },
    /// A file copied, each path expanded; the folders of `to` are made.
    Copy {
        /// The file copied.
        from: String,
        /// Where to.
        to: String,
    },
    /// `${BSP}`, and the `.lit` beside it when there is one, copied into the project's build,
    /// and the game started on the map.
    Launch,
}

impl Task {
    /// Its name, as a profiles file writes it.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Bsp => "bsp",
            Self::Vis { .. } => "vis",
            Self::Light { .. } => "light",
            Self::Tool { .. } => "tool",
            Self::Copy { .. } => "copy",
            Self::Launch => "launch",
        }
    }
}

/// A compile profile: its name and its tasks, run in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Its name.
    pub name: String,
    /// What it does.
    pub tasks: Vec<Task>,
}

impl Profile {
    /// The profiles every project has.
    #[must_use]
    pub fn built_in() -> Vec<Self> {
        vec![
            Self {
                name: FULL_PROFILE.to_owned(),
                tasks: vec![
                    Task::Bsp,
                    Task::Vis {
                        speed: VisSpeed::Full,
                    },
                    Task::Light {
                        speed: LightSpeed::Full,
                    },
                    Task::Launch,
                ],
            },
            Self {
                name: FAST_PROFILE.to_owned(),
                tasks: vec![
                    Task::Bsp,
                    Task::Vis {
                        speed: VisSpeed::Fast,
                    },
                    Task::Light {
                        speed: LightSpeed::Fast,
                    },
                    Task::Launch,
                ],
            },
        ]
    }
}

/// The profiles file as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfilesFile {
    #[serde(default)]
    profile: Vec<Profile>,
}

/// A name a task's path may use, written `${NAME}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variable {
    /// `${MAP}`: the map's file.
    Map,
    /// `${MAP_DIR}`: the folder it is in.
    MapDir,
    /// `${MAP_NAME}`: its name without the extension.
    MapName,
    /// `${BSP}`: `${MAP_DIR}/${MAP_NAME}.bsp`.
    Bsp,
    /// `${PROJECT}`: the project's root.
    Project,
    /// `${GAME}`: the game's base folder, as the preview host finds it.
    Game,
}

impl Variable {
    /// Every variable.
    pub const ALL: [Self; 6] = [
        Self::Map,
        Self::MapDir,
        Self::MapName,
        Self::Bsp,
        Self::Project,
        Self::Game,
    ];

    /// Its name, as written between `${` and `}`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Map => "MAP",
            Self::MapDir => "MAP_DIR",
            Self::MapName => "MAP_NAME",
            Self::Bsp => "BSP",
            Self::Project => "PROJECT",
            Self::Game => "GAME",
        }
    }
}

/// The values of the [`Variable`]s for one map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expansions {
    map: PathBuf,
    project: Option<PathBuf>,
    game: Option<PathBuf>,
}

impl Expansions {
    /// The values for the map at `map`, in the project at `project` and the game at `game`.
    #[must_use]
    pub const fn new(map: PathBuf, project: Option<PathBuf>, game: Option<PathBuf>) -> Self {
        Self { map, project, game }
    }

    /// The folder the map is in.
    fn map_dir(&self) -> PathBuf {
        self.map.parent().map(Path::to_path_buf).unwrap_or_default()
    }

    /// The BSP the map compiles to.
    #[must_use]
    pub fn bsp(&self) -> PathBuf {
        self.map.with_extension("bsp")
    }

    /// The value of `variable`; `None` for a project or a game there is none of.
    #[must_use]
    pub fn value(&self, variable: Variable) -> Option<String> {
        let text = |path: &Path| path.to_string_lossy().into_owned();
        match variable {
            Variable::Map => Some(text(&self.map)),
            Variable::MapDir => Some(text(&self.map_dir())),
            Variable::MapName => Some(
                self.map
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            Variable::Bsp => Some(text(&self.bsp())),
            Variable::Project => self.project.as_deref().map(text),
            Variable::Game => self.game.as_deref().map(text),
        }
    }

    /// `text` with each `${NAME}` replaced by its variable's value.
    ///
    /// # Errors
    ///
    /// [`ExpandError`] for a name no variable has, a variable with no value, or a `${` with no
    /// `}`.
    pub fn expand(&self, text: &str) -> Result<String, ExpandError> {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find("${") {
            out.push_str(rest.get(..start).unwrap_or_default());
            let after = rest.get(start.saturating_add(2)..).unwrap_or_default();
            let end = after.find('}').ok_or_else(|| ExpandError::Unclosed {
                text: text.to_owned(),
            })?;
            let name = after.get(..end).unwrap_or_default();
            let variable = Variable::ALL
                .into_iter()
                .find(|variable| variable.name() == name)
                .ok_or_else(|| ExpandError::Unknown {
                    name: name.to_owned(),
                })?;
            let value = self.value(variable).ok_or_else(|| ExpandError::NoValue {
                name: name.to_owned(),
            })?;
            out.push_str(&value);
            rest = after.get(end.saturating_add(1)..).unwrap_or_default();
        }
        out.push_str(rest);
        Ok(out)
    }
}

/// Why a task's path could not be expanded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpandError {
    /// `${NAME}` names no variable.
    Unknown {
        /// The name.
        name: String,
    },
    /// The variable has no value: no project is open, or no game was found.
    NoValue {
        /// The name.
        name: String,
    },
    /// A `${` with no `}` after it.
    Unclosed {
        /// The text.
        text: String,
    },
}

impl std::fmt::Display for ExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown { name } => write!(f, "no variable is called ${{{name}}}"),
            Self::NoValue { name } => write!(f, "${{{name}}} has no value here"),
            Self::Unclosed { text } => write!(f, "a ${{ has no }} in {text:?}"),
        }
    }
}

impl std::error::Error for ExpandError {}

/// Why a task stopped its profile.
#[derive(Debug)]
pub enum TaskError {
    /// Compiling, lighting or reading the map failed.
    Workbench(WorkbenchError),
    /// Building the project or starting the game failed.
    Preview(PreviewError),
    /// Vis stopped.
    Vis(VisError),
    /// Light stopped.
    Light(LightError),
    /// A path's expression could not be expanded.
    Expand(ExpandError),
    /// A file could not be read, written, copied or removed, or a program not started.
    Io {
        /// The file or program.
        path: PathBuf,
        /// Why.
        error: std::io::Error,
    },
    /// A program ended with a status other than success.
    Tool {
        /// The program.
        program: String,
        /// Its status.
        status: std::process::ExitStatus,
        /// What it wrote to its standard error.
        said: String,
    },
}

impl std::fmt::Display for TaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Workbench(error) => error.fmt(f),
            Self::Preview(error) => error.fmt(f),
            Self::Vis(error) => error.fmt(f),
            Self::Light(error) => error.fmt(f),
            Self::Expand(error) => error.fmt(f),
            Self::Io { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Tool {
                program,
                status,
                said,
            } => write!(f, "{program}: {status}: {said}"),
        }
    }
}

impl std::error::Error for TaskError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Workbench(error) => Some(error),
            Self::Preview(error) => Some(error),
            Self::Vis(error) => Some(error),
            Self::Light(error) => Some(error),
            Self::Expand(error) => Some(error),
            Self::Io { error, .. } => Some(error),
            Self::Tool { .. } => None,
        }
    }
}

/// The [`TaskError::Io`] of `error` on `path`.
fn io(path: &Path) -> impl FnOnce(std::io::Error) -> TaskError + '_ {
    move |error| TaskError::Io {
        path: path.to_path_buf(),
        error,
    }
}

/// How one task of a profile ended.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum TaskResult {
    /// It did what it says.
    Done,
    /// It stopped the profile.
    Failed {
        /// Why, as the person reads it.
        message: String,
    },
}

/// One task run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TaskRun {
    /// The task.
    pub task: Task,
    /// How long it took, in seconds.
    pub seconds: f64,
    /// How it ended.
    #[serde(flatten)]
    pub result: TaskResult,
}

/// What `map.run_profile` did: each task run, in order, up to and including one that failed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProfileRun {
    /// The profile's name.
    pub profile: String,
    /// The tasks run.
    pub tasks: Vec<TaskRun>,
    /// How long the whole profile took, in seconds.
    pub seconds: f64,
    /// What compiling the map found, when a `bsp` task ran.
    pub report: Option<MapReport>,
}

impl ProfileRun {
    /// Whether every task was done.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.tasks
            .iter()
            .all(|task| task.result == TaskResult::Done)
    }
}

impl<L: crate::Loader> Workbench<L> {
    /// The built-in profiles, then the project's, a project's of a built-in's name in its
    /// place.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::ProfilesRead`] when the project's profiles file cannot be read, and
    /// [`WorkbenchError::ProfilesParse`] when it is not a profiles file.
    pub fn profiles(&self) -> Result<Vec<Profile>, WorkbenchError> {
        let mut profiles = Profile::built_in();
        let Some(root) = self.root() else {
            return Ok(profiles);
        };
        let path = root.join(PROFILES_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(profiles),
            Err(error) => return Err(WorkbenchError::ProfilesRead { path, error }),
        };
        let file: ProfilesFile =
            toml::from_str(&text).map_err(|error| WorkbenchError::ProfilesParse {
                path: path.clone(),
                error,
            })?;
        for profile in file.profile {
            match profiles.iter_mut().find(|known| known.name == profile.name) {
                Some(known) => *known = profile,
                None => profiles.push(profile),
            }
        }
        Ok(profiles)
    }

    /// The profile `map.run_profile` runs when given none: the one chosen, else
    /// [`FULL_PROFILE`].
    #[must_use]
    pub fn chosen_profile(&self) -> &str {
        self.profile.as_deref().unwrap_or(FULL_PROFILE)
    }
}

/// Runs `task` for the map `expansions` describes; what compiling found, for a `bsp` task.
fn run_task<C: HasPreview>(
    context: &mut C,
    task: &Task,
    expansions: &Expansions,
) -> Result<Option<MapReport>, TaskError> {
    let bsp = expansions.bsp();
    let read = |path: &Path| std::fs::read(path).map_err(io(path));
    let write = |path: &Path, bytes: &[u8]| std::fs::write(path, bytes).map_err(io(path));
    let expand = |text: &str| expansions.expand(text).map_err(TaskError::Expand);
    match task {
        Task::Bsp => {
            let (workbench, preview) = context.workbench_and_preview();
            let game = preview.game_base_path();
            let (compiled, report, _) = workbench
                .compile_map(game.as_deref())
                .map_err(TaskError::Workbench)?;
            write(&bsp, &compiled.bsp)?;
            for (extension, bytes) in [
                ("prt", compiled.portal_file.as_ref()),
                ("pts", compiled.leak_file.as_ref()),
            ] {
                let file = bsp.with_extension(extension);
                match bytes {
                    Some(bytes) => write(&file, bytes)?,
                    // A file of an earlier compile would be read as this one's.
                    None => remove_stale(&file)?,
                }
            }
            Ok(Some(report))
        }
        Task::Vis { speed } => {
            let portals = bsp.with_extension("prt");
            let prt = std::fs::read_to_string(&portals).map_err(io(&portals))?;
            let mode = match speed {
                VisSpeed::Full => Mode::default(),
                VisSpeed::Fast => Mode::Fast,
            };
            let seen = Vis::new(mode)
                .run(&read(&bsp)?, &prt)
                .map_err(TaskError::Vis)?;
            write(&bsp, &seen)?;
            Ok(None)
        }
        Task::Light { speed } => {
            let sampling = match speed {
                LightSpeed::Full => Sampling::Extra,
                LightSpeed::Fast => Sampling::Fast,
            };
            let lit = Light::new(sampling)
                .run(&read(&bsp)?)
                .map_err(TaskError::Light)?;
            write(&bsp, &lit.bsp)?;
            let colour = bsp.with_extension("lit");
            match &lit.lit {
                Some(bytes) => write(&colour, bytes)?,
                // A `.lit` from an earlier compile would colour this one wrongly (BSP-18.2).
                None => remove_stale(&colour)?,
            }
            Ok(None)
        }
        Task::Tool { program, args } => {
            let program = expand(program)?;
            let args = args
                .iter()
                .map(|arg| expand(arg))
                .collect::<Result<Vec<_>, _>>()?;
            let output = std::process::Command::new(&program)
                .args(&args)
                .current_dir(expansions.map_dir())
                .output()
                .map_err(io(Path::new(&program)))?;
            if output.status.success() {
                Ok(None)
            } else {
                Err(TaskError::Tool {
                    status: output.status,
                    said: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                    program,
                })
            }
        }
        Task::Copy { from, to } => {
            let (from, to) = (PathBuf::from(expand(from)?), PathBuf::from(expand(to)?));
            if let Some(folder) = to.parent() {
                std::fs::create_dir_all(folder).map_err(io(folder))?;
            }
            std::fs::copy(&from, &to).map_err(io(&from))?;
            Ok(None)
        }
        Task::Launch => {
            let name = expansions.value(Variable::MapName).unwrap_or_default();
            let (workbench, preview) = context.workbench_and_preview();
            let built = workbench.built_program().map_err(TaskError::Preview)?;
            let (project, _, mod_name) = &built;
            let maps = project.join("build").join(mod_name).join("maps");
            std::fs::create_dir_all(&maps).map_err(io(&maps))?;
            write(&maps.join(&name).with_extension("bsp"), &read(&bsp)?)?;
            let colour = bsp.with_extension("lit");
            let target = maps.join(&name).with_extension("lit");
            if colour.exists() {
                write(&target, &read(&colour)?)?;
            } else {
                remove_stale(&target)?;
            }
            preview
                .play_built(built, Some(name))
                .map_err(TaskError::Preview)?;
            Ok(None)
        }
    }
}

/// Removes `file` when it is there.
fn remove_stale(file: &Path) -> Result<(), TaskError> {
    match std::fs::remove_file(file) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(io(file)(error)),
        Ok(()) | Err(_) => Ok(()),
    }
}

/// `map.profiles`.
pub(crate) struct MapProfiles;

impl<C: HasPreview + 'static> Command<C> for MapProfiles {
    const ID: &'static str = MAP_PROFILES;
    const SUMMARY: &'static str = "Lists the compile profiles: the built-in Full and Fast, then \
        the project's own from compile-profiles.toml, each its name and its tasks.";
    type Params = NoParams;
    type Output = Vec<Profile>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        _: NoParams,
    ) -> Result<Outcome<Vec<Profile>, C>, WorkbenchError> {
        Ok(Outcome::unchanged(context.workbench().profiles()?))
    }
}

/// The parameters of `map.run_profile` and `map.choose_profile`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunProfileParams {
    /// The profile's name, as `map.profiles` lists it; for `map.run_profile`, the one chosen
    /// when not given.
    #[serde(default)]
    pub profile: Option<String>,
}

/// `map.choose_profile`.
pub(crate) struct MapChooseProfile;

impl<C: HasPreview + 'static> Command<C> for MapChooseProfile {
    const ID: &'static str = MAP_CHOOSE_PROFILE;
    const SUMMARY: &'static str = "Chooses the compile profile map.run_profile runs when given         none; none given chooses Full again.";
    type Params = RunProfileParams;
    type Output = String;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: RunProfileParams,
    ) -> Result<Outcome<String, C>, WorkbenchError> {
        let workbench = context.workbench();
        if let Some(name) = &params.profile
            && !workbench
                .profiles()?
                .iter()
                .any(|profile| &profile.name == name)
        {
            return Err(WorkbenchError::UnknownProfile { name: name.clone() });
        }
        workbench.profile = params.profile;
        Ok(Outcome::outside_history(
            workbench.chosen_profile().to_owned(),
        ))
    }
}

/// `map.run_profile`.
pub(crate) struct MapRunProfile;

impl<C: HasPreview + 'static> Command<C> for MapRunProfile {
    const ID: &'static str = MAP_RUN_PROFILE;
    const SUMMARY: &'static str = "Runs a compile profile on the open map, the one chosen when \
        none is named: its tasks in order, each timed, stopping at the first that fails; \
        reports each task's time and result.";
    type Params = RunProfileParams;
    type Output = ProfileRun;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: RunProfileParams,
    ) -> Result<Outcome<ProfileRun, C>, WorkbenchError> {
        let (workbench, preview) = context.workbench_and_preview();
        let name = params
            .profile
            .unwrap_or_else(|| workbench.chosen_profile().to_owned());
        let profile = workbench
            .profiles()?
            .into_iter()
            .find(|profile| profile.name == name)
            .ok_or(WorkbenchError::UnknownProfile { name })?;
        let map = workbench
            .map()
            .ok_or(WorkbenchError::NoMap)?
            .path()
            .to_path_buf();
        let expansions = Expansions::new(
            map,
            workbench.root().map(Path::to_path_buf),
            preview.game_base_path(),
        );
        let started = Instant::now();
        let mut run = ProfileRun {
            profile: profile.name,
            tasks: Vec::new(),
            seconds: 0.0,
            report: None,
        };
        for task in profile.tasks {
            let began = Instant::now();
            let result = match run_task(context, &task, &expansions) {
                Ok(report) => {
                    if report.is_some() {
                        run.report = report;
                    }
                    TaskResult::Done
                }
                Err(error) => TaskResult::Failed {
                    message: error.to_string(),
                },
            };
            let failed = result != TaskResult::Done;
            run.tasks.push(TaskRun {
                task,
                seconds: began.elapsed().as_secs_f64(),
                result,
            });
            if failed {
                break;
            }
        }
        run.seconds = started.elapsed().as_secs_f64();
        Ok(Outcome::outside_history(run))
    }
}

#[cfg(test)]
mod tests;
