// SPDX-License-Identifier: GPL-3.0-or-later

//! The files a key or a console command names (UI-24): the levels `map` and `changelevel`
//! take, the models `precache_model` takes and the sounds `precache_sound` takes, from the
//! project's folder and the game's search path, each by the name the engine is given, so the
//! key editor and the console offer them and mark a name no file has.

use std::collections::BTreeMap;
use std::path::Path;

use qs_commands::{Command, Outcome};
use qs_vfs::{StdFs, Vfs};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{HasMap, MapEditError, MapProject};

/// The id of `map.game_files`.
pub const MAP_GAME_FILES: &str = "map.game_files";

/// The folder of the levels; the world model is `maps/<map>.bsp` (BUILTIN-10.3).
const MAPS_FOLDER: &str = "maps/";
/// The folder `S_LoadSound` reads `sound/<name>` from (SND-1.4).
const SOUND_FOLDER: &str = "sound/";
/// The ending of a level's file.
const BSP_ENDING: &str = ".bsp";
/// The ending of a sound's file.
const WAV_ENDING: &str = ".wav";
/// The endings of the models `Mod_ForName` reads: alias models, brush models and sprites.
const MODEL_ENDINGS: [&str; 3] = [".mdl", ".bsp", ".spr"];
/// The start of the name of a BSP that is an item's box and no level.
const BOX_MODEL: &str = "b_";

/// A kind of file a key or a command names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GameFileKind {
    /// A level, by the name `map` and `changelevel` take: `e1m1` for `maps/e1m1.bsp`.
    Level,
    /// A model, by its path: `progs/ogre.mdl`.
    Model,
    /// A sound, by its path under `sound/`: `doors/drclos4.wav`.
    Sound,
}

impl GameFileKind {
    /// The name the engine is given for the file at `path` on the search path, when it is a
    /// file of this kind.
    fn name_of(self, path: &str) -> Option<String> {
        match self {
            Self::Level => path
                .strip_prefix(MAPS_FOLDER)?
                .strip_suffix(BSP_ENDING)
                .filter(|name| !name.contains('/') && !name.starts_with(BOX_MODEL))
                .map(str::to_owned),
            Self::Model => MODEL_ENDINGS
                .iter()
                .any(|ending| path.ends_with(ending))
                .then(|| path.to_owned()),
            Self::Sound => path
                .strip_prefix(SOUND_FOLDER)
                .filter(|name| name.ends_with(WAV_ENDING))
                .map(str::to_owned),
        }
    }

    /// The files of this kind in the project's folder `root` and on the search path of the game at
    /// `base`, by name, each once: the project's where both have one.
    #[must_use]
    pub fn files(self, root: Option<&Path>, base: Option<&Path>) -> Vec<GameFile> {
        let mut found: BTreeMap<String, FileOrigin> = BTreeMap::new();
        if let Some(base) = base {
            let base = base.to_string_lossy().into_owned();
            if let Ok(vfs) = Vfs::new(StdFs, &["-basedir", base.as_str()], ".") {
                for file in vfs.files() {
                    let path = String::from_utf8_lossy(file.name()).to_lowercase();
                    if let Some(name) = self.name_of(&path) {
                        found.insert(name, FileOrigin::Game);
                    }
                }
            }
        }
        if let Some(root) = root {
            let mut paths = Vec::new();
            loose_files(root, "", &mut paths);
            for path in paths {
                if let Some(name) = self.name_of(&path.to_lowercase()) {
                    found.insert(name, FileOrigin::Project);
                }
            }
        }
        found
            .into_iter()
            .map(|(name, origin)| GameFile { name, origin })
            .collect()
    }
}

/// Where a file of the listing is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileOrigin {
    /// The project's folder, which takes the game's place for a file of the same name.
    Project,
    /// The game's search path.
    Game,
}

/// A file of the listing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GameFile {
    /// The name the engine is given for it.
    pub name: String,
    /// Where it is.
    pub origin: FileOrigin,
}

/// The parameters of `map.game_files`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GameFilesParams {
    /// The kind of file to list.
    pub kind: GameFileKind,
}

/// Every file under `folder`, by its path from the project's folder with `/` between its
/// parts, `prefix` the path to `folder`; a folder whose name starts with `.` is not read.
fn loose_files(folder: &Path, prefix: &str, paths: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{prefix}{name}");
        match entry.file_type() {
            Ok(kind) if kind.is_dir() && !name.starts_with('.') => {
                loose_files(&entry.path(), &format!("{path}/"), paths);
            }
            Ok(kind) if kind.is_file() => paths.push(path),
            Ok(_) | Err(_) => {}
        }
    }
}

/// `map.game_files`.
pub(super) struct MapGameFiles;

impl<C: HasMap + 'static> Command<C> for MapGameFiles {
    const ID: &'static str = MAP_GAME_FILES;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "Lists the levels, models or sounds of the project's folder \
        and the game's search path, each by the name the engine is given (a level as map takes \
        it, a model by its path, a sound by its path under sound/) and where it is; the \
        project's takes the game's place for a name both have.";
    type Params = GameFilesParams;
    type Output = Vec<GameFile>;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: GameFilesParams,
    ) -> Result<Outcome<Vec<GameFile>, C>, MapEditError> {
        let base = context.game_folders().base;
        let root = context.map_project().project_root();
        Ok(Outcome::unchanged(
            params.kind.files(root.as_deref(), base.as_deref()),
        ))
    }
}

#[cfg(test)]
mod tests;
