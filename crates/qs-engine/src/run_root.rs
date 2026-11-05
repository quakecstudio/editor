// SPDX-License-Identifier: GPL-3.0-or-later

//! ADR-0008's run roots: `<project>/build/run/<flavour>`, holding `id1`, a directory link to
//! the install's, and `<mod>`, a directory link to the build output `<project>/build/<mod>`;
//! the rerelease's also links `QuakeEX.kpf` on macOS and Linux. Each is made, or repaired,
//! before every launch, and nothing is written inside the install.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use qs_install::{GameBase, LinkError, link_dir};

/// Which game a run root runs (ADR-0008).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Flavour {
    /// The original game, `<install>/id1`.
    Original,
    /// The 2021 rerelease, `<install>/rerelease/id1`.
    Rerelease,
}

impl Flavour {
    /// The run root's folder name under `build/run`.
    #[must_use]
    pub const fn dir_name(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Rerelease => "rerelease",
        }
    }
}

/// A run root, ready to launch in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunRoot {
    /// The run root: what the host is given with `-basedir`.
    pub path: PathBuf,
    /// The mod's folder name in it: what the host is given with `-game`. `None` plays
    /// `id1` alone, as a game root whose `id1` is not the registered game must be played: id's
    /// engine refuses a mod there (`COM_CheckRegistered`, FS-4.5).
    pub game: Option<String>,
    /// The rerelease's `QuakeEX.kpf` to give the host with `-kpf`, on Windows, where a
    /// junction cannot link a file (ADR-0008).
    pub kpf: Option<PathBuf>,
}

/// Why a run root could not be made.
#[derive(Debug)]
pub enum RunRootError {
    /// The mod's name is not a single folder name.
    BadModName(String),
    /// A link could not be made or repaired.
    Link(LinkError),
    /// A folder could not be made.
    Io {
        /// The folder.
        path: PathBuf,
        /// The host's error.
        error: std::io::Error,
    },
}

impl fmt::Display for RunRootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadModName(name) => write!(f, "{name:?} is not a single folder name"),
            Self::Link(error) => write!(f, "{error}"),
            Self::Io { path, error } => write!(f, "{}: {error}", path.display()),
        }
    }
}

impl std::error::Error for RunRootError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Link(error) => Some(error),
            Self::Io { error, .. } => Some(error),
            Self::BadModName(_) => None,
        }
    }
}

impl From<LinkError> for RunRootError {
    fn from(error: LinkError) -> Self {
        Self::Link(error)
    }
}

/// Makes or repairs the `flavour` run root of `project` for the mod `mod_name` over the
/// install's base directory `base` (ADR-0008): its `id1` link, its `<mod>` link to
/// `<project>/build/<mod>` (made if it is not there yet) and, for the rerelease on macOS and
/// Linux, its `QuakeEX.kpf` link.
///
/// # Errors
///
/// [`RunRootError`] for a mod name that is not one folder name, a link that cannot be made
/// (a real folder in its place, an install that is gone) or a folder that cannot be made.
pub fn prepare_run_root(
    project: &Path,
    flavour: Flavour,
    base: &GameBase,
    mod_name: &str,
) -> Result<RunRoot, RunRootError> {
    let plain = !mod_name.is_empty()
        && mod_name != "."
        && mod_name != ".."
        && !mod_name.contains(['/', '\\', ':']);
    if !plain {
        return Err(RunRootError::BadModName(mod_name.to_owned()));
    }
    let build = project.join("build");
    let output = build.join(mod_name);
    fs::create_dir_all(&output).map_err(|error| RunRootError::Io {
        path: output.clone(),
        error,
    })?;
    let root = build.join("run").join(flavour.dir_name());
    link_dir(&base.id1, &root.join("id1"))?;
    link_dir(&output, &root.join(mod_name))?;
    let kpf = match (flavour, &base.kpf) {
        (Flavour::Rerelease, Some(kpf)) => link_kpf(kpf, &root)?,
        (Flavour::Rerelease | Flavour::Original, _) => None,
    };
    Ok(RunRoot {
        path: root,
        game: Some(mod_name.to_owned()),
        kpf,
    })
}

/// Links `kpf` into the run root `root` on macOS and Linux; on Windows it is passed with
/// `-kpf` instead, so it is returned.
#[cfg(not(windows))]
fn link_kpf(kpf: &Path, root: &Path) -> Result<Option<PathBuf>, RunRootError> {
    qs_install::link_file(kpf, &root.join("QuakeEX.kpf"))?;
    Ok(None)
}

#[cfg(windows)]
fn link_kpf(kpf: &Path, _root: &Path) -> Result<Option<PathBuf>, RunRootError> {
    Ok(Some(kpf.to_path_buf()))
}
