// SPDX-License-Identifier: GPL-3.0-or-later

//! `scenario.run` (SCN-2.6, P3-19): a scenario file played in `qs-sim` in the process, over the
//! open project's program and the game the preview plays, and its screenshots taken by the
//! preview host started headless.

use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use qc_progs::Progs;
use qs_bsp::{Bsp, BspLoader};
use qs_engine::{
    Client, ClientError, Flavour, LaunchError, LaunchSpec, RunRootError, StepKind, launch,
    locate_host, prepare_run_root,
};
use qs_sim::scenario::{self as scenarios, Report, Scenario, ScenarioError, Shot};
use qs_sim::{LoadOptions, World};
use qs_vfs::{LoadedFile, StdFs, Vfs};
use serde::Serialize;

use super::{Preview, PreviewError, built_program, socket_path};
use crate::Workbench;
use qc_project::Loader;

/// What `scenario.run` answers: the run's report, and the screenshots written.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScenarioRun {
    /// The run's result (SCN-2.4).
    pub report: Report,
    /// Each screenshot's PNG, beside the scenario, in the order asked for; none when no host
    /// was there to take them (SCN-2.6).
    pub images: Vec<PathBuf>,
}

/// Why a scenario did not run.
#[derive(Debug)]
pub enum ScenarioRunError {
    /// The scenario file could not be read.
    Read(PathBuf, std::io::Error),
    /// Its text does not read as SCN-1 says.
    Parse(PathBuf, ScenarioError),
    /// The project did not build, or there is no game, as Play needs.
    Preview(PreviewError),
    /// The run root could not be made.
    RunRoot(RunRootError),
    /// The game's files or its map do not load.
    Game(String),
    /// The headless host for the screenshots did not start.
    Launch(LaunchError),
    /// The headless host failed a screenshot's command.
    Bridge(ClientError),
    /// A screenshot could not be written.
    Write(PathBuf, std::io::Error),
}

impl fmt::Display for ScenarioRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(path, error) => write!(f, "{}: {error}", path.display()),
            Self::Parse(path, error) => write!(f, "{}: {error}", path.display()),
            Self::Preview(error) => error.fmt(f),
            Self::RunRoot(error) => error.fmt(f),
            Self::Game(why) => f.write_str(why),
            Self::Launch(error) => error.fmt(f),
            Self::Bridge(error) => write!(f, "the headless host: {error}"),
            Self::Write(path, error) => write!(f, "{}: {error}", path.display()),
        }
    }
}

impl std::error::Error for ScenarioRunError {}

impl From<PreviewError> for ScenarioRunError {
    fn from(error: PreviewError) -> Self {
        Self::Preview(error)
    }
}

impl From<ClientError> for ScenarioRunError {
    fn from(error: ClientError) -> Self {
        Self::Bridge(error)
    }
}

/// Where screenshot `name` of the scenario at `scenario` is written: beside it, as
/// `<scenario>-<name>.png` (SCN-2.6).
#[must_use]
pub fn image_path(scenario: &Path, name: &str) -> PathBuf {
    let stem = scenario
        .file_stem()
        .map_or_else(|| "scenario".into(), |stem| stem.to_string_lossy());
    scenario.with_file_name(format!("{stem}-{name}.png"))
}

impl Preview {
    /// Builds the open project and plays the scenario at `path` on its program, over the game
    /// the preview plays, in `qs-sim` (SCN-2.1); its screenshots are taken by the preview host
    /// started headless when there is one (SCN-2.6).
    ///
    /// # Errors
    ///
    /// [`ScenarioRunError`]: the file does not read, the project does not build, there is no
    /// game, the map does not load, or a screenshot fails.
    pub fn run_scenario<L: Loader>(
        &self,
        workbench: &mut Workbench<L>,
        path: &Path,
    ) -> Result<ScenarioRun, ScenarioRunError> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| ScenarioRunError::Read(path.to_path_buf(), error))?;
        let scenario = Scenario::parse(&text)
            .map_err(|error| ScenarioRunError::Parse(path.to_path_buf(), error))?;
        let (project, _, mod_name) = built_program(workbench)?;
        let base = self.game_base()?;
        let run_root = prepare_run_root(&project, Flavour::Original, &base, &mod_name)
            .map_err(ScenarioRunError::RunRoot)?;
        let mut words = vec![
            "-basedir".to_owned(),
            run_root.path.to_string_lossy().into_owned(),
        ];
        if let Some(game) = &run_root.game {
            words.extend(["-game".to_owned(), game.clone()]);
        }
        words.extend(scenario.engine.iter().cloned());
        let report = play_on_game(&words, &scenario)?;
        let mut images = Vec::new();
        if report.screenshots.is_empty() {
            return Ok(ScenarioRun { report, images });
        }
        let Ok(host) = locate_host(self.config.host.as_deref(), &self.config.app) else {
            return Ok(ScenarioRun { report, images });
        };
        let spec = LaunchSpec {
            host,
            run_root,
            map: Some(scenario.map.clone()),
            socket: socket_path(),
            extra: vec!["--headless".to_owned()],
        };
        let mut running = launch(&spec, "QuakeC Studio").map_err(ScenarioRunError::Launch)?;
        let taken = shoot(&mut running.client, &report.screenshots, path, &mut images);
        if let Err(error) = running.client.shutdown() {
            tracing::debug!(%error, "the headless host did not answer shutdown");
        }
        if let Err(error) = running.child.wait() {
            tracing::debug!(%error, "the headless host was not waited for");
        }
        taken?;
        Ok(ScenarioRun { report, images })
    }
}

/// The scenario played in `qs-sim` on the game the engine command line `words` names, its
/// progs and map read from that game's search path (SCN-2.1): what `scenario.run` plays once
/// the project is built, and what `cargo xtask scenario` plays on a test game.
///
/// # Errors
///
/// [`ScenarioRunError::Game`] when the search path, the progs or the map do not load.
pub fn play_on_game(words: &[String], scenario: &Scenario) -> Result<Report, ScenarioRunError> {
    let game = |why: String| ScenarioRunError::Game(why);
    let vfs = Vfs::new(StdFs, words, ".").map_err(|e| game(format!("{e:?}")))?;
    let read = |name: &str| -> Result<Vec<u8>, ScenarioRunError> {
        vfs.load(name.as_bytes())
            .map_err(|e| game(format!("{name}: {e:?}")))?
            .map(LoadedFile::into_bytes)
            .ok_or_else(|| game(format!("{name} is not on the game's search path")))
    };
    let progs_bytes = read("progs.dat")?;
    let map = format!("maps/{}.bsp", scenario.map);
    let bsp_bytes = read(&map)?;
    let progs = Progs::read(&progs_bytes).map_err(|e| game(format!("progs.dat: {e}")))?;
    let bsp =
        Bsp::read(&bsp_bytes, BspLoader::default()).map_err(|e| game(e.message(&scenario.map)))?;
    let files = Vfs::new(StdFs, words, ".").map_err(|e| game(format!("{e:?}")))?;
    let reader = Rc::new(move |name: &str| {
        files
            .load(name.as_bytes())
            .ok()
            .flatten()
            .map(LoadedFile::into_bytes)
    });
    // SCN-2.1: id's builtin table, as DIFF-16.1 plays a route; the scenario's seed (SCN-1.3).
    let options = LoadOptions {
        registration: vfs.registration(),
        seed: scenario.seed,
        ..LoadOptions::default()
    };
    let mut world = World::load(&progs, &scenario.map, &bsp, reader, options)
        .map_err(|e| game(e.to_string()))?;
    world.settle(&progs).map_err(|e| game(e.to_string()))?;
    Ok(scenarios::run(&mut world, &progs, scenario))
}

/// Each of `shots` taken by the headless host: the game paused, the player moved to the
/// shot's place, one frame stepped and the frame written beside the scenario (SCN-2.6).
fn shoot(
    client: &mut Client,
    shots: &[Shot],
    scenario: &Path,
    images: &mut Vec<PathBuf>,
) -> Result<(), ScenarioRunError> {
    client.pause()?;
    for shot in shots {
        client.spawn_player_at(shot.origin, shot.angles)?;
        client.step(StepKind::Frame)?;
        let png = client.screenshot_png(None)?;
        let file = image_path(scenario, &shot.name);
        std::fs::write(&file, png).map_err(|error| ScenarioRunError::Write(file.clone(), error))?;
        images.push(file);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
