// SPDX-License-Identifier: GPL-3.0-or-later

//! `project.export` (P4-28, QA-15): the open project as a zip players install, the mod folder
//! with its programs, its own assets and the notices, never id's game data (LIC-4, LIC-5).
//!
//! What goes in, under `<mod>/` in the zip:
//!
//! - each module's program, from `build/<mod>/` after a build: `progs.dat`, and `csprogs.dat`
//!   or `menu.dat` when the project has those modules. Line maps (`.lno`) stay out;
//! - the project's own assets: every file under its [`ASSET_DIRS`], map sources (`.map`) left
//!   out, either as they are or packed into `pak0.pak`;
//! - `README.txt`, written here: what the mod is, the game it needs and how to install it;
//! - the project's license and notice files, as they are ([`NOTICE_PREFIXES`]).
//!
//! It never reads `build/` other than the programs, so it never follows the run roots' links
//! under `build/run/` (ADR-0008), and it never follows a link anywhere else. It refuses a file
//! that is id's: one whose size and MD5 are a row of `FS_KNOWN_FILES` (FS-6.2), or whose bytes
//! are a member of a PAK in the game folder's `id1` or `rerelease/id1`. Exporting the same
//! sources twice gives the same zip.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use qs_commands::{Command, Outcome};
use qs_pak::{Pak, PakWriteError, write_pak};
use qs_vfs::StdFs;
use qs_vfs::zip::{NewMember, ZipWriteError, write_zip};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{HasWorkbench, Workbench, WorkbenchError};

/// The id of `project.export`.
pub const PROJECT_EXPORT: &str = "project.export";

/// The folders of a project holding its own assets, as a Quake game directory names them.
pub const ASSET_DIRS: [&str; 7] = ["maps", "progs", "sound", "gfx", "music", "textures", "env"];

/// The beginnings of the names of the project's license and notice files, matched without
/// regard to case: `COPYING`, `LICENSE.txt`, `NOTICE.md`, `ATTRIBUTION.md` and the like.
pub const NOTICE_PREFIXES: [&str; 5] = ["copying", "license", "licence", "notice", "attribution"];

/// The parameters of `project.export`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    /// Pack the assets into `pak0.pak` in the mod folder instead of leaving them as files.
    #[serde(default)]
    pub pak: bool,
    /// Where to write the zip; `build/<mod>.zip` in the project when absent.
    #[serde(default)]
    pub out: Option<PathBuf>,
    /// The Quake folder whose `id1` (and `rerelease/id1`) the export is checked against, so
    /// that nothing of id's is shipped; the IDE passes the one Play uses. When absent, the
    /// first install found (FS-6) is used, as Play uses it.
    #[serde(default)]
    pub game: Option<PathBuf>,
}

/// The game a mod needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GameNeeded {
    /// The original game (1996): `id1/pak0.pak` and `id1/pak1.pak`.
    Original,
    /// The 2021 rerelease.
    Rerelease,
}

impl GameNeeded {
    /// What the README calls the game.
    const fn described(self) -> &'static str {
        match self {
            Self::Original => {
                "the original Quake (1996), registered: id1/pak0.pak and id1/pak1.pak"
            }
            Self::Rerelease => "Quake's 2021 rerelease (Quake Enhanced)",
        }
    }
}

/// What `project.export` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Exported {
    /// The zip written.
    pub path: PathBuf,
    /// The mod folder's name, the zip's top folder.
    pub mod_name: String,
    /// The game the mod needs.
    pub game: GameNeeded,
    /// Every member of the zip, in its order.
    pub files: Vec<String>,
    /// The assets the project's attribution file does not name, so whose license is unknown;
    /// every asset when the project has none.
    pub unknown_license: Vec<String>,
}

/// Why a mod could not be exported.
#[derive(Debug)]
pub enum ExportError {
    /// The project could not be opened or built.
    Workbench(WorkbenchError),
    /// These modules have errors, so they have no program to export.
    NotBuilt(Vec<String>),
    /// A module's program is not in `build/<mod>/`.
    OutsideBuild(PathBuf),
    /// A file of the export is id's game data.
    IdData {
        /// The project's file.
        file: PathBuf,
        /// What of id's it is.
        is: String,
    },
    /// A file could not be read or the zip written.
    Io {
        /// The file.
        path: PathBuf,
        /// The system's message.
        message: String,
    },
    /// The assets cannot go in a PAK.
    Pak(PakWriteError),
    /// The zip cannot be written.
    Zip(ZipWriteError),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Workbench(error) => error.fmt(f),
            Self::NotBuilt(modules) => write!(
                f,
                "the project does not build, so there is nothing to export: {} has errors",
                modules.join(", ")
            ),
            Self::OutsideBuild(path) => write!(
                f,
                "{} is not in the project's build/<mod>/ folder, so it is not exported",
                path.display()
            ),
            Self::IdData { file, is } => write!(
                f,
                "{} is id Software's game data ({is}), which a mod must never include; remove it \
                 from the project",
                file.display()
            ),
            Self::Io { path, message } => write!(f, "{}: {message}", path.display()),
            Self::Pak(error) => write!(f, "the assets cannot be packed: {error}"),
            Self::Zip(error) => write!(f, "the zip cannot be written: {error}"),
        }
    }
}

impl std::error::Error for ExportError {}

/// A file of the export, before it is zipped: its path in the mod folder and its bytes, and
/// the file it came from.
struct Piece {
    name: String,
    data: Vec<u8>,
    from: PathBuf,
}

impl<L: qc_project::Loader> Workbench<L> {
    /// Builds the open project and exports it as [`ExportParams`] asks.
    ///
    /// # Errors
    ///
    /// [`ExportError`]: no project, a module that does not build, id's data among the files, or
    /// a file that cannot be read or written.
    pub fn export(&mut self, params: &ExportParams) -> Result<Exported, ExportError> {
        let built = self.build().map_err(ExportError::Workbench)?;
        let failed: Vec<String> = built
            .iter()
            .filter(|module| !module.built)
            .map(|module| module.name.clone())
            .collect();
        if !failed.is_empty() {
            return Err(ExportError::NotBuilt(failed));
        }
        let info = self
            .project_info()
            .ok_or(ExportError::Workbench(WorkbenchError::NoProject))?;
        let root = info.root;
        let build = root.join("build");
        let mut programs = Vec::new();
        let mut mod_name = None;
        for module in &built {
            let folder = module
                .output
                .parent()
                .filter(|folder| folder.parent() == Some(build.as_path()))
                .and_then(|folder| folder.file_name())
                .and_then(|name| name.to_str())
                .ok_or_else(|| ExportError::OutsideBuild(module.output.clone()))?;
            mod_name.get_or_insert_with(|| folder.to_owned());
            let file = module
                .output
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| ExportError::OutsideBuild(module.output.clone()))?;
            programs.push(Piece {
                name: file.to_owned(),
                data: read(&module.output)?,
                from: module.output.clone(),
            });
        }
        let mod_name = mod_name.ok_or(ExportError::NotBuilt(Vec::new()))?;
        let game = if info
            .modules
            .iter()
            .any(|module| module.engine.contains("rerelease"))
        {
            GameNeeded::Rerelease
        } else {
            GameNeeded::Original
        };
        let assets = collect_assets(&root)?;
        let notices = collect_notices(&root)?;
        let game_folder = params.game.clone().or_else(|| {
            qs_install::find_installs()
                .into_iter()
                .next()
                .map(|install| install.root)
        });
        let id_files = IdFiles::of(game_folder.as_deref());
        for piece in programs.iter().chain(&assets).chain(&notices) {
            id_files.refuse(piece)?;
        }
        let unknown_license = unknown_licenses(&root, &assets);
        let mut pieces: Vec<Piece> = programs;
        if params.pak && !assets.is_empty() {
            let files: Vec<(&str, &[u8])> = assets
                .iter()
                .map(|piece| (piece.name.as_str(), piece.data.as_slice()))
                .collect();
            pieces.push(Piece {
                name: "pak0.pak".to_owned(),
                data: write_pak(&files).map_err(ExportError::Pak)?,
                from: root.clone(),
            });
        } else {
            pieces.extend(assets);
        }
        pieces.extend(notices);
        pieces.sort_by(|a, b| a.name.cmp(&b.name));
        let readme = readme(&mod_name, game, &pieces);
        pieces.push(Piece {
            name: "README.txt".to_owned(),
            data: readme.into_bytes(),
            from: root.clone(),
        });
        pieces.sort_by(|a, b| a.name.cmp(&b.name));
        let members: Vec<NewMember> = pieces
            .into_iter()
            .map(|piece| NewMember {
                name: format!("{mod_name}/{}", piece.name),
                data: piece.data,
            })
            .collect();
        let zip = write_zip(&members).map_err(ExportError::Zip)?;
        let path = params
            .out
            .clone()
            .unwrap_or_else(|| build.join(format!("{mod_name}.zip")));
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder).map_err(|e| io(folder, &e))?;
        }
        fs::write(&path, zip).map_err(|e| io(&path, &e))?;
        Ok(Exported {
            path,
            mod_name,
            game,
            files: members.into_iter().map(|member| member.name).collect(),
            unknown_license,
        })
    }
}

/// The error of `path`.
fn io(path: &Path, error: &std::io::Error) -> ExportError {
    ExportError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

/// The bytes of the file at `path`.
fn read(path: &Path) -> Result<Vec<u8>, ExportError> {
    fs::read(path).map_err(|e| io(path, &e))
}

/// Every file under the project's [`ASSET_DIRS`], by its path in the mod folder, never through a
/// link, map sources left out.
fn collect_assets(root: &Path) -> Result<Vec<Piece>, ExportError> {
    let mut found = BTreeMap::new();
    for dir in ASSET_DIRS {
        walk(&root.join(dir), dir, &mut found)?;
    }
    found
        .into_iter()
        .map(|(name, from)| {
            Ok(Piece {
                name,
                data: read(&from)?,
                from,
            })
        })
        .collect()
}

/// The files under `dir`, by `prefix/<path under dir>`; links and `.map` files are not taken.
fn walk(
    dir: &Path,
    prefix: &str,
    found: &mut BTreeMap<String, PathBuf>,
) -> Result<(), ExportError> {
    let Ok(meta) = fs::symlink_metadata(dir) else {
        return Ok(());
    };
    if !meta.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|e| io(dir, &e))? {
        let entry = entry.map_err(|e| io(dir, &e))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| io(&path, &e))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let inside = format!("{prefix}/{name}");
        if kind.is_dir() {
            walk(&path, &inside, found)?;
        } else if kind.is_file()
            && !Path::new(&name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("map"))
        {
            found.insert(inside, path);
        }
    }
    Ok(())
}

/// The project's license and notice files at its top, by name.
fn collect_notices(root: &Path) -> Result<Vec<Piece>, ExportError> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| io(root, &e))? {
        let entry = entry.map_err(|e| io(root, &e))?;
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let lower = name.to_lowercase();
        let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
        if is_file && NOTICE_PREFIXES.iter().any(|p| lower.starts_with(p)) {
            found.push(Piece {
                name,
                data: read(&path)?,
                from: path,
            });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// The assets the project's `ATTRIBUTION.md` does not name, by path in the mod folder or by
/// file name; all of them when it has none.
fn unknown_licenses(root: &Path, assets: &[Piece]) -> Vec<String> {
    let attribution = fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .find(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.to_lowercase().starts_with("attribution"))
        })
        .and_then(|entry| fs::read_to_string(entry.path()).ok())
        .unwrap_or_default();
    assets
        .iter()
        .filter(|piece| {
            let file = piece.name.rsplit('/').next().unwrap_or(&piece.name);
            !attribution.contains(&piece.name) && !attribution.contains(file)
        })
        .map(|piece| piece.name.clone())
        .collect()
}

/// The README of the mod `mod_name`, which needs `game`, holding `pieces`.
fn readme(mod_name: &str, game: GameNeeded, pieces: &[Piece]) -> String {
    let install = match game {
        GameNeeded::Original => format!(
            "1. Unzip this archive into your Quake folder, the one that holds id1, so that\n   \
             {mod_name} sits beside id1.\n\
             2. Start a Quake engine with -game {mod_name}, for example:\n   \
             quakespasm -game {mod_name}\n   ironwail -game {mod_name}\n   \
             vkquake -game {mod_name}\n"
        ),
        GameNeeded::Rerelease => format!(
            "1. Unzip this archive into the rerelease folder of your Quake install, the one\n   \
             that holds the rerelease's id1, so that {mod_name} sits beside it.\n\
             2. Start an engine that plays the rerelease with -game {mod_name}, for example:\n   \
             ironwail -game {mod_name}\n   vkquake -game {mod_name}\n"
        ),
    };
    let mut text = format!(
        "{mod_name}\n{}\n\nA Quake mod, made with QuakeC Studio.\n\n\
         It needs {}. It does not include any of id Software's game data: you need your own\n\
         copy of the game.\n\nInstalling\n----------\n\n{install}\nContents\n--------\n\n",
        "=".repeat(mod_name.chars().count()),
        game.described()
    );
    for piece in pieces {
        text.push_str(&format!("{mod_name}/{}\n", piece.name));
    }
    let notices: Vec<&str> = pieces
        .iter()
        .map(|piece| piece.name.as_str())
        .filter(|name| {
            let lower = name.to_lowercase();
            !name.contains('/') && NOTICE_PREFIXES.iter().any(|p| lower.starts_with(p))
        })
        .collect();
    text.push_str("\nLicense\n-------\n\n");
    if notices.is_empty() {
        text.push_str("The project has no license file.\n");
    } else {
        text.push_str(&format!(
            "The mod's license and notices are in {}, in this folder.\n",
            notices.join(", ")
        ));
    }
    text
}

/// id's files an export is checked against: the members of the game folder's PAKs, by size.
struct IdFiles {
    by_size: BTreeMap<usize, Vec<(PathBuf, String, Vec<u8>)>>,
}

impl IdFiles {
    /// The PAK members of `game`'s `id1` and `rerelease/id1`, none without a game folder.
    fn of(game: Option<&Path>) -> Self {
        let mut by_size: BTreeMap<usize, Vec<(PathBuf, String, Vec<u8>)>> = BTreeMap::new();
        for dir in game
            .into_iter()
            .flat_map(|game| [game.join("id1"), game.join("rerelease").join("id1")])
        {
            for entry in fs::read_dir(&dir).ok().into_iter().flatten().flatten() {
                let path = entry.path();
                let is_pak = path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("pak"));
                let Some(bytes) = is_pak.then(|| fs::read(&path).ok()).flatten() else {
                    continue;
                };
                let Ok(pak) = Pak::read(&bytes) else {
                    continue;
                };
                for member in pak.entries() {
                    if let Ok(data) = pak.entry_data(&bytes, member) {
                        let name = String::from_utf8_lossy(member.name()).into_owned();
                        by_size.entry(data.len()).or_default().push((
                            path.clone(),
                            name,
                            data.to_vec(),
                        ));
                    }
                }
            }
        }
        Self { by_size }
    }

    /// Refuses `piece` when it is id's: a known file of FS-6.2, or a member of the game's PAKs.
    fn refuse(&self, piece: &Piece) -> Result<(), ExportError> {
        let refused = |is: String| ExportError::IdData {
            file: piece.from.clone(),
            is,
        };
        if let Ok((size, md5)) = qs_install::size_and_md5(&StdFs, &piece.from)
            && let Some(known) = qs_install::FS_KNOWN_FILES
                .iter()
                .find(|known| known.size == size && known.md5 == md5)
        {
            return Err(refused(format!(
                "{}/{}, {}",
                known.game_dir, known.name, known.description
            )));
        }
        for (pak, name, data) in self.by_size.get(&piece.data.len()).into_iter().flatten() {
            if *data == piece.data {
                return Err(refused(format!("{name} in {}", pak.display())));
            }
        }
        Ok(())
    }
}

/// `project.export`.
pub(crate) struct ExportProject;

impl<C: HasWorkbench + 'static> Command<C> for ExportProject {
    const ID: &'static str = PROJECT_EXPORT;
    const SUMMARY: &'static str = "Builds the open project and exports it as a zip players \
        install: its programs, its own assets (optionally in pak0.pak), a README and its \
        license notices, never id's game data.";
    type Params = ExportParams;
    type Output = Exported;
    type Error = ExportError;

    fn run(
        &self,
        context: &mut C,
        params: ExportParams,
    ) -> Result<Outcome<Exported, C>, ExportError> {
        context
            .workbench()
            .export(&params)
            .map(Outcome::outside_history)
    }
}

#[cfg(test)]
mod tests;
