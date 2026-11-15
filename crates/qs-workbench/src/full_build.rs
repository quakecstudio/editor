// SPDX-License-Identifier: GPL-3.0-or-later

//! The background full compile (P7-02, MAP-13): after Play from here starts the game on its fast
//! compile, the same BSP is given full vis and full light on a thread of its own, and when that is
//! done it is swapped into the running game in place of the fast one (BRIDGE-4.14), the player
//! where they stand and every entity as it is.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use qs_bsp::{Bsp, BspError, BspLoader, Submodel};
use qs_map::{MapError, MapFile};
use qs_map_light::{Light, LightError, Lighted, Sampling};
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

/// What a full compile gives: the lit BSP, or why there is none.
type Built = Result<Lighted, FullBuildError>;

/// A full compile running on its own thread for the map the game plays.
pub(crate) struct FullBuild {
    /// The map's name, as the game knows it.
    pub(crate) map: String,
    /// Where the played BSP is, which the full one replaces.
    pub(crate) target: PathBuf,
    /// The lit BSP, or why there is none, once the thread is done.
    done: Receiver<Built>,
}

impl FullBuild {
    /// Starts full vis (VIS-1) and full light (LIGHT-1.3) of `bsp`, the fast compile's BSP before
    /// its light, with its portal file `portals`, for the map `map` played from `target`.
    pub(crate) fn start(map: String, target: PathBuf, bsp: Vec<u8>, portals: Vec<u8>) -> Self {
        let (send, done) = mpsc::channel();
        std::thread::spawn(move || {
            let built = full(&bsp, &portals);
            // The preview may have dropped the build, for a new play: nothing waits for it then.
            drop(send.send(built));
        });
        Self { map, target, done }
    }

    /// The lit BSP, or why there is none, once the thread is done; `None` while it runs.
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

    /// Writes `lit` where the played BSP is, with its `.lit` or none, as Play from here writes
    /// the fast one (BSP-18.2).
    ///
    /// # Errors
    ///
    /// [`FullBuildError::Write`] for the file that could not be written.
    pub(crate) fn write(&self, lit: &Lighted) -> Result<(), FullBuildError> {
        let failed = |path: &Path, error| FullBuildError::Write {
            path: path.to_path_buf(),
            error,
        };
        std::fs::write(&self.target, &lit.bsp).map_err(|e| failed(&self.target, e))?;
        let colour = self.target.with_extension("lit");
        match &lit.lit {
            Some(bytes) => std::fs::write(&colour, bytes).map_err(|e| failed(&colour, e))?,
            None if colour.exists() => {
                std::fs::remove_file(&colour).map_err(|e| failed(&colour, e))?;
            }
            None => {}
        }
        Ok(())
    }
}

/// `bsp` with full vis, then full light.
fn full(bsp: &[u8], portals: &[u8]) -> Built {
    let portals = String::from_utf8_lossy(portals);
    let seen = Vis::new(Mode::default())
        .run(bsp, &portals)
        .map_err(FullBuildError::Vis)?;
    Light::new(Sampling::Extra)
        .run(&seen)
        .map_err(FullBuildError::Light)
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
