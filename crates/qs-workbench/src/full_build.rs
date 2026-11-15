// SPDX-License-Identifier: GPL-3.0-or-later

//! The background full compile (P7-02, MAP-13): after Play from here starts the game on its fast
//! compile, the same BSP is given full vis and full light on a thread of its own, and when that is
//! done it is swapped into the running game in place of the fast one (BRIDGE-4.14), the player
//! where they stand and every entity as it is.
//!
//! The thread does all but the swap: it writes the full BSP beside the played one, under a name of
//! its own, and reads the brush models the swap names, so the poll that swaps it does no more than
//! send the request. Only one thread runs: a full compile asked for while one runs waits for it.

use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use qs_bsp::{Bsp, BspError, BspLoader, Submodel};
use qs_map::{MapError, MapFile};
use qs_map_light::{Light, LightError, Sampling};
use qs_map_vis::{Mode, Vis, VisError};

/// Why a full compile did not reach the game.
#[derive(Debug)]
pub enum FullBuildError {
    /// Vis stopped.
    Vis(VisError),
    /// Light stopped.
    Light(LightError),
    /// The thread stopped without an answer.
    Stopped,
    /// A file could not be written.
    Write {
        /// The file.
        path: PathBuf,
        /// Why.
        error: std::io::Error,
    },
    /// The lit BSP does not read.
    Bsp(BspError),
    /// Its entity lump does not read.
    Entities(MapError),
}

impl core::fmt::Display for FullBuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Vis(error) => write!(f, "vis: {error}"),
            Self::Light(error) => write!(f, "light: {error}"),
            Self::Stopped => f.write_str("the full compile stopped"),
            Self::Write { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Bsp(error) => f.write_str(&error.message("the full compile's BSP")),
            Self::Entities(error) => write!(f, "the full compile's entities: {error}"),
        }
    }
}

impl core::error::Error for FullBuildError {}

/// A full compile written and ready to swap in.
#[derive(Debug)]
pub(crate) struct Ready {
    /// The full BSP, beside the played one under a name of its own.
    pub(crate) bsp: PathBuf,
    /// Its `.lit`, when a light is coloured.
    pub(crate) lit: Option<Vec<u8>>,
    /// Each brush entity's place and model, for `map/swap` (BRIDGE-4.18).
    pub(crate) models: Vec<(usize, String)>,
}

/// What a full compile gives.
pub(crate) type Built = Result<Ready, FullBuildError>;

/// What a full compile starts from: the map the game plays, where its BSP is, and the fast
/// compile's BSP before its light with its portal file.
pub(crate) struct Start {
    /// The map's name, as the game knows it.
    pub(crate) map: String,
    /// The played BSP, which the full one replaces.
    pub(crate) target: PathBuf,
    /// The fast compile's BSP before its light.
    pub(crate) bsp: Vec<u8>,
    /// Its portal file.
    pub(crate) portals: Vec<u8>,
    /// How many live edits the game had been sent when it started.
    pub(crate) edits: u64,
}

/// A full compile on its own thread for the map the game plays.
pub(crate) struct FullBuild {
    /// The map's name, as the game knows it.
    pub(crate) map: String,
    /// The played BSP, which the full one replaces.
    pub(crate) target: PathBuf,
    /// How many live edits the game had been sent when it started: a later edit moves the map's
    /// entities in the game, so the places the swap names may no longer be theirs.
    pub(crate) edits: u64,
    /// The played BSP's contents when it started: another compile written over it since makes
    /// this one stale.
    played: Option<u64>,
    /// The thread's answer, until it is taken.
    done: Receiver<Built>,
    /// The answer, kept for another try while the game is stopped at a statement.
    pub(crate) ready: Option<Ready>,
}

impl FullBuild {
    /// Starts full vis (VIS-1) and full light (LIGHT-1.3) of `start`'s BSP.
    pub(crate) fn start(start: Start) -> Self {
        let Start {
            map,
            target,
            bsp,
            portals,
            edits,
        } = start;
        let played = contents(&target);
        let (send, done) = mpsc::channel();
        let side = side_file(&target);
        std::thread::spawn(move || {
            let built = full(&bsp, &portals, &side);
            // The preview may have dropped the build: nothing waits for it then.
            drop(send.send(built));
        });
        Self {
            map,
            target,
            edits,
            played,
            done,
            ready: None,
        }
    }

    /// The thread's answer once it is done; `None` while it runs.
    pub(crate) fn take(&self) -> Option<Built> {
        match self.done.try_recv() {
            Ok(built) => Some(built),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(FullBuildError::Stopped)),
        }
    }

    /// Waits for the thread. For tests.
    #[cfg(test)]
    pub(crate) fn wait(&self) -> Built {
        self.done.recv().unwrap_or(Err(FullBuildError::Stopped))
    }

    /// Whether the played BSP is still the one this build started from.
    pub(crate) fn current(&self) -> bool {
        contents(&self.target) == self.played
    }
}

/// The file the full BSP is written to before it is swapped in: `<map>.full.bsp` beside the
/// played one.
fn side_file(target: &Path) -> PathBuf {
    let stem = target
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    target.with_file_name(format!("{stem}.full.bsp"))
}

/// A digest of the file at `path`, `None` when it does not read.
fn contents(path: &Path) -> Option<u64> {
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(hasher.finish())
}

/// `bsp` with full vis, then full light, written to `side`, with its brush models.
fn full(bsp: &[u8], portals: &[u8], side: &Path) -> Built {
    let portals = String::from_utf8_lossy(portals);
    let seen = Vis::new(Mode::default())
        .run(bsp, &portals)
        .map_err(FullBuildError::Vis)?;
    let lit = Light::new(Sampling::Extra)
        .run(&seen)
        .map_err(FullBuildError::Light)?;
    let models = brush_models(&lit.bsp)?;
    std::fs::write(side, &lit.bsp).map_err(|error| FullBuildError::Write {
        path: side.to_path_buf(),
        error,
    })?;
    Ok(Ready {
        bsp: side.to_path_buf(),
        lit: lit.lit,
        models,
    })
}

/// Each of `bsp`'s entities that shows a brush model, by its place in the entity lump, the
/// game's place for it (BRIDGE-4.13), and that model, for `map/swap` (BRIDGE-4.18).
///
/// # Errors
///
/// [`FullBuildError::Bsp`] or [`FullBuildError::Entities`] for a BSP or an entity lump that
/// does not read.
pub fn brush_models(bsp: &[u8]) -> Result<Vec<(usize, String)>, FullBuildError> {
    let read = Bsp::read(bsp, BspLoader::default()).map_err(FullBuildError::Bsp)?;
    let entities = MapFile::parse(read.entities()).map_err(FullBuildError::Entities)?;
    Ok(entities
        .entities
        .iter()
        .enumerate()
        .filter_map(|(place, entity)| {
            let model = String::from_utf8_lossy(entity.value(b"model")?).into_owned();
            Submodel::parse(&model).map(|_| (place, model))
        })
        .collect())
}

#[cfg(test)]
pub(crate) mod tests;
