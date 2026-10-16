// SPDX-License-Identifier: GPL-3.0-or-later

//! Finding the user's Quake game data, checking it, and linking it into a run root.
//!
//! [`find_installs`] looks where Steam, GOG (the registry, GOG Galaxy's library and the
//! offline installers' folder) and the Heroic launcher put Quake on the running OS, and
//! [`examine_folder`] accepts any folder the user points at that holds `id1/pak0.pak`,
//! recognising steamcmd downloads and GOG installers extracted with innoextract. Each
//! [`Install`] reports its original and rerelease base directories with their `id1` and
//! other game directories (FS-6.1). [`Install::validate`] identifies the files by their
//! published sizes and checksums and says which editions are there and what is missing
//! (FS-6.2 to FS-6.4). [`link_dir`] makes the directory links of ADR-0008's run roots.
//!
//! The game data is only ever read where it is: nothing here copies or writes inside an
//! install (LIC-4, PLAT-8). Where each store puts the files is in `docs/game-data.md`.

use std::io::Read;
use std::path::{Path, PathBuf};

use qs_paths::{Platform, StoreDirs, current_store_dirs};
use qs_vfs::{EntryKind, HostFs, StdFs};

mod known;
mod link;
mod md5;
mod registry;
mod stores;
mod vdf;

pub use known::{FS_KNOWN_FILES, FileStatus, KnownFile, Release, identify, size_and_md5};
pub use link::{LinkError, LinkOutcome, link_dir};
pub use registry::{Registry, SystemRegistry};

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;

/// Quake's Steam app id, as Ironwail's `QUAKE_STEAM_APPID` and Debian's
/// game-data-packager give it.
pub const STEAM_APP_ID: u32 = 2310;

/// GOG's product id for Quake, "The Offering": the original game and mission packs.
pub const GOG_QUAKE_ID: &str = "1435828198";

/// GOG's product id for Quake Enhanced, the 2021 rerelease.
pub const GOG_QUAKE_ENHANCED_ID: &str = "1739637082";

/// The name of the rerelease's folder inside a Steam install (FS-5.1).
const RERELEASE_DIR: &str = "rerelease";

/// The rerelease's localization archive, found in its base directory (FS-5.3).
const KPF_NAME: &str = "QuakeEX.kpf";

/// The host filesystem as the finder reads it: [`HostFs`], plus a canonical form of a path
/// so the same install reached two ways is reported once.
pub trait InstallFs: HostFs {
    /// `path` with links resolved, or `path` itself when it cannot be resolved.
    fn canonical(&self, path: &Path) -> PathBuf;
}

impl InstallFs for StdFs {
    fn canonical(&self, path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }
}

/// How an install was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// A Steam library listed by the Steam client.
    Steam,
    /// GOG: the registry entry its installers write, or a folder in GOG Galaxy's or the
    /// offline installers' library holding a `goggame-<id>.info` file.
    Gog,
    /// The Heroic Games Launcher's list of installed GOG games.
    Heroic,
    /// A folder the user chose that steamcmd downloaded into: it holds
    /// `steamapps/appmanifest_2310.acf`.
    Steamcmd,
    /// A folder the user chose that innoextract wrote a GOG installer into: the game is in
    /// its `app` folder.
    Innoextract,
    /// Any other folder the user chose.
    Folder,
}

/// A game directory beside `id1` that holds a `pak0.pak`: a mission pack, an episode, or a
/// mod.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GameDir {
    /// Its name on disk, as `-game` takes it.
    pub name: String,
    /// The directory.
    pub path: PathBuf,
    /// Its `pak0.pak`.
    pub pak0: PathBuf,
}

/// A base directory, the folder an engine is given with `-basedir` (FS-1.2, FS-6.1).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GameBase {
    /// The base directory.
    pub path: PathBuf,
    /// Its `id1` directory, whatever its case on disk (FS-3.7).
    pub id1: PathBuf,
    /// `id1/pak0.pak`.
    pub pak0: PathBuf,
    /// `id1/pak1.pak`, if there is one.
    pub pak1: Option<PathBuf>,
    /// Its other game directories that hold a `pak0.pak`, by name.
    pub game_dirs: Vec<GameDir>,
    /// `QuakeEX.kpf`, if there is one (FS-5.3).
    pub kpf: Option<PathBuf>,
}

/// One install of the game found on this machine.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Install {
    /// How it was found.
    pub source: Source,
    /// The folder it was found at: the store's install folder or the folder the user chose.
    pub root: PathBuf,
    /// The original game's base directory, if the install has one.
    pub original: Option<GameBase>,
    /// The 2021 rerelease's base directory, if the install has one (FS-5.1).
    pub rerelease: Option<GameBase>,
}

/// An edition of the game an install holds (FS-6.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Edition {
    /// The original game's first episode only: `id1/pak0.pak` and no `pak1.pak`.
    Shareware,
    /// The original registered game: the published `id1/pak1.pak`.
    Registered,
    /// The 2021 rerelease.
    Rerelease,
}

/// Why a file is reported missing (FS-6.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MissingReason {
    /// The original game is installed without `id1/pak1.pak`: it is the shareware game.
    SharewareOnly,
    /// Only the rerelease is installed, as the Epic Games Store and Game Pass sell it; its
    /// `id1` has only its own `pak0.pak` (FS-5.1).
    RereleaseOnly,
}

/// A file of the original game that an install lacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Missing {
    /// The file, relative to the base directory, such as `id1/pak1.pak`.
    pub file: &'static str,
    /// Why it is missing.
    pub reason: MissingReason,
}

/// A game file that was checked, and what it turned out to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedFile {
    /// The file.
    pub path: PathBuf,
    /// What it is.
    pub status: FileStatus,
}

/// The result of checking an install's files (FS-6.2 to FS-6.4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Validation {
    /// Every `pak0.pak` and `pak1.pak` found, original base first.
    pub files: Vec<CheckedFile>,
    /// The editions present, in the order of [`Edition`].
    pub editions: Vec<Edition>,
    /// The original game's files that are missing.
    pub missing: Vec<Missing>,
}

impl Install {
    /// Identifies every `pak0.pak` and `pak1.pak` of the install by its published size and
    /// checksum, and reports the editions present and the files missing.
    ///
    /// Implements FS-6.2 to FS-6.4. Reads each file whose size is a known file's in full,
    /// and only the header and directory of the others.
    pub fn validate<F: HostFs + ?Sized>(&self, fs: &F) -> Validation {
        self.validate_with(fs, &FS_KNOWN_FILES)
    }

    pub(crate) fn validate_with<F: HostFs + ?Sized>(
        &self,
        fs: &F,
        table: &[KnownFile],
    ) -> Validation {
        let mut validation = Validation::default();
        let mut check = |path: &Path| -> FileStatus {
            let status = known::identify_with(fs, path, table);
            validation.files.push(CheckedFile {
                path: path.to_path_buf(),
                status,
            });
            status
        };
        let mut shareware = false;
        let mut registered = false;
        let mut rerelease = false;
        if let Some(base) = &self.original {
            let pak0 = check(&base.pak0);
            let pak1 = base.pak1.as_deref().map(&mut check);
            for dir in &base.game_dirs {
                check(&dir.pak0);
            }
            registered =
                matches!(pak1, Some(FileStatus::Known(known)) if known.is_registered_pak1());
            shareware = pak1.is_none()
                && matches!(pak0, FileStatus::Known(known) if known.is_original_pak0());
        }
        if let Some(base) = &self.rerelease {
            let pak0 = check(&base.pak0);
            if let Some(pak1) = &base.pak1 {
                check(pak1);
            }
            for dir in &base.game_dirs {
                check(&dir.pak0);
            }
            rerelease = !matches!(pak0, FileStatus::Unusable(_));
        }
        for (present, edition) in [
            (shareware, Edition::Shareware),
            (registered, Edition::Registered),
            (rerelease, Edition::Rerelease),
        ] {
            if present {
                validation.editions.push(edition);
            }
        }
        match (&self.original, &self.rerelease) {
            (Some(base), _) if base.pak1.is_none() => validation.missing.push(Missing {
                file: "id1/pak1.pak",
                reason: MissingReason::SharewareOnly,
            }),
            (None, Some(_)) => {
                for file in ["id1/pak0.pak", "id1/pak1.pak"] {
                    validation.missing.push(Missing {
                        file,
                        reason: MissingReason::RereleaseOnly,
                    });
                }
            }
            (Some(_) | None, _) => {}
        }
        validation
    }
}

/// Finds Quake on this machine, as [`Finder::find`] does for the running OS.
#[must_use]
pub fn find_installs() -> Vec<Install> {
    Finder::new(
        &StdFs,
        &SystemRegistry,
        Platform::current(),
        current_store_dirs(),
    )
    .find()
}

/// Looks at a folder the user chose, as [`Finder::examine`] does.
#[must_use]
pub fn examine_folder(folder: &Path) -> Option<Install> {
    Finder::new(
        &StdFs,
        &SystemRegistry,
        Platform::current(),
        StoreDirs::default(),
    )
    .examine(folder)
}

/// Looks for installs through a filesystem, a registry, a platform's rules and its store
/// folders, all injected so every OS's layout can be tested on any host.
#[derive(Debug)]
pub struct Finder<'a, F: ?Sized, R: ?Sized> {
    fs: &'a F,
    registry: &'a R,
    platform: Platform,
    stores: StoreDirs,
}

impl<'a, F: InstallFs + ?Sized, R: Registry + ?Sized> Finder<'a, F, R> {
    /// A finder for `platform`, whose store folders are `stores`
    /// ([`qs_paths::store_dirs`]).
    pub const fn new(fs: &'a F, registry: &'a R, platform: Platform, stores: StoreDirs) -> Self {
        Self {
            fs,
            registry,
            platform,
            stores,
        }
    }

    /// Every install found in the stores' usual places, Steam first, then GOG, then
    /// Heroic. An install reached two ways (a folder and a link to it) is reported once.
    #[must_use]
    pub fn find(&self) -> Vec<Install> {
        let mut found = Vec::new();
        for (folder, source) in self.store_folders() {
            if let Some(install) = self.examine_as(&folder, source) {
                found.push(install);
            }
        }
        let mut seen = Vec::new();
        found.retain(|install| {
            let canonical = self.fs.canonical(&install.root);
            if seen.contains(&canonical) {
                return false;
            }
            seen.push(canonical);
            true
        });
        found
    }

    /// Looks at a folder the user chose: any folder whose `id1` holds `pak0.pak`, the
    /// folder above a Steam install's `rerelease`, a steamcmd download or the output of
    /// innoextract. `None` if the folder holds no game.
    #[must_use]
    pub fn examine(&self, folder: &Path) -> Option<Install> {
        let source = if self.is_steamcmd(folder) {
            Source::Steamcmd
        } else {
            Source::Folder
        };
        if let Some(install) = self.examine_as(folder, source) {
            return Some(install);
        }
        let app = self.child(folder, "app", EntryKind::Dir)?;
        let mut install = self.examine_as(&app, Source::Innoextract)?;
        install.root = folder.to_path_buf();
        Some(install)
    }

    fn is_steamcmd(&self, folder: &Path) -> bool {
        self.child(folder, "steamapps", EntryKind::Dir)
            .and_then(|steamapps| self.child(&steamapps, &manifest_name(), EntryKind::File))
            .is_some()
    }

    /// The install at `folder`, found by way of `source`.
    ///
    /// Implements FS-6.1.
    fn examine_as(&self, folder: &Path, source: Source) -> Option<Install> {
        let top = self.game_base(folder);
        let nested = self
            .child(folder, RERELEASE_DIR, EntryKind::Dir)
            .and_then(|dir| self.game_base(&dir));
        let (original, rerelease) = match top {
            Some(base) if base.kpf.is_some() => (None, Some(base)),
            top => (top, nested),
        };
        if original.is_none() && rerelease.is_none() {
            return None;
        }
        Some(Install {
            source,
            root: folder.to_path_buf(),
            original,
            rerelease,
        })
    }

    fn game_base(&self, base: &Path) -> Option<GameBase> {
        let id1 = self.child(base, qs_vfs::FS_GAMENAME, EntryKind::Dir)?;
        let pak0 = self.child(&id1, "pak0.pak", EntryKind::File)?;
        let pak1 = self.child(&id1, "pak1.pak", EntryKind::File);
        let mut game_dirs = Vec::new();
        for entry in self.fs.read_dir(base).unwrap_or_default() {
            let Some(name) = entry.name.to_str() else {
                continue;
            };
            if entry.kind != EntryKind::Dir
                || name.eq_ignore_ascii_case(qs_vfs::FS_GAMENAME)
                || name.eq_ignore_ascii_case(RERELEASE_DIR)
            {
                continue;
            }
            let path = base.join(name);
            if let Some(pak0) = self.child(&path, "pak0.pak", EntryKind::File) {
                game_dirs.push(GameDir {
                    name: name.to_owned(),
                    path,
                    pak0,
                });
            }
        }
        game_dirs.sort_by(|a, b| a.name.cmp(&b.name));
        Some(GameBase {
            path: base.to_path_buf(),
            id1,
            pak0,
            pak1,
            game_dirs,
            kpf: self.child(base, KPF_NAME, EntryKind::File),
        })
    }

    /// The entry `name` of the directory `dir` of kind `kind`, as FS-3.7 matches game
    /// directories and archives: an entry with exactly that name, else the first entry in
    /// name order that matches it ASCII case-insensitively, so the path keeps the name on
    /// disk. A link is not listed as a directory ([`qs_vfs::StdFs`]), so when no entry
    /// matches, the exact name is tried by opening it, which follows links.
    fn child(&self, dir: &Path, name: &str, kind: EntryKind) -> Option<PathBuf> {
        let mut entries = self.fs.read_dir(dir).ok()?;
        entries.retain(|entry| entry.kind == kind);
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let exact = entries.iter().find(|entry| entry.name == name);
        let found = exact.or_else(|| {
            entries.iter().find(|entry| {
                entry
                    .name
                    .to_str()
                    .is_some_and(|entry_name| entry_name.eq_ignore_ascii_case(name))
            })
        });
        if let Some(entry) = found {
            return Some(dir.join(&entry.name));
        }
        let path = dir.join(name);
        let exists = match kind {
            EntryKind::Dir => self.fs.read_dir(&path).is_ok(),
            EntryKind::File => self.fs.open(&path).is_ok() && self.fs.read_dir(&path).is_err(),
            EntryKind::Other => false,
        };
        exists.then_some(path)
    }

    /// A small text file's contents, or `None` if it cannot be read.
    fn read_text(&self, path: &Path) -> Option<String> {
        const LIMIT: u64 = 1 << 20;
        let file = self.fs.open(path).ok()?;
        let mut bytes = Vec::new();
        file.take(LIMIT).read_to_end(&mut bytes).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }
}

/// `appmanifest_2310.acf`, the file Steam and steamcmd write for an installed Quake.
fn manifest_name() -> String {
    format!("appmanifest_{STEAM_APP_ID}.acf")
}
