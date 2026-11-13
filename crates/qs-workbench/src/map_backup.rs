// SPDX-License-Identifier: GPL-3.0-or-later

//! Automatic backups of the open map (P6-20, MAP-28): the map as it is in the editor, saved or
//! not, written to `autosave/<name>.<n>.map` beside it, as TrenchBroom keeps its autosaves in a
//! folder beside the map, numbered from 1. `map.save` takes one of what it saved, the window
//! takes one at an interval while the map has unsaved edits, and the last
//! [`BACKUPS_KEPT`] are kept. After a crash, `map.backups` lists them and
//! `map.restore_backup` puts one in the editor, as an edit that undo takes back.

use std::path::{Path, PathBuf};

use qs_commands::{Command, NoParams, Outcome};
use qs_map::MapFile;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::map_edit::{MapSource, edit_map};
use crate::{HasWorkbench, Workbench, WorkbenchError};

/// The id of `map.backup`.
pub const MAP_BACKUP: &str = "map.backup";
/// The id of `map.backups`.
pub const MAP_BACKUPS: &str = "map.backups";
/// The id of `map.restore_backup`.
pub const MAP_RESTORE_BACKUP: &str = "map.restore_backup";
/// The folder beside a map its backups are written to.
pub const BACKUP_FOLDER: &str = "autosave";
/// How many backups of a map are kept, the oldest removed past it.
pub const BACKUPS_KEPT: usize = 20;
/// The extension of a map and of its backups.
const MAP_EXTENSION: &str = "map";

/// One backup of the open map.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Backup {
    /// Its file.
    pub path: PathBuf,
    /// Its number, the newest the highest.
    pub number: u32,
}

/// The backups of the map at `map`, the newest first.
fn backups_of(map: &Path) -> Vec<Backup> {
    let Some(stem) = map
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
    else {
        return Vec::new();
    };
    let folder = map
        .parent()
        .map(|dir| dir.join(BACKUP_FOLDER))
        .unwrap_or_default();
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Vec::new();
    };
    let mut found: Vec<Backup> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let number = name
                .strip_prefix(&stem)?
                .strip_prefix('.')?
                .strip_suffix(MAP_EXTENSION)?
                .strip_suffix('.')?
                .parse()
                .ok()?;
            Some(Backup {
                path: entry.path(),
                number,
            })
        })
        .collect();
    found.sort_by_key(|backup| std::cmp::Reverse(backup.number));
    found
}

impl<L: crate::Loader> Workbench<L> {
    /// Writes the open map, as it is in the editor, as its next backup, and removes the oldest
    /// past [`BACKUPS_KEPT`]; the backup's file.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoMap`] with no map open, [`WorkbenchError::NoMapSource`] for a BSP,
    /// and [`WorkbenchError::Save`] when the backup cannot be written.
    pub fn backup_map(&mut self) -> Result<Backup, WorkbenchError> {
        let document = self.map().ok_or(WorkbenchError::NoMap)?;
        if let MapSource::Bsp(_) = document.source() {
            return Err(WorkbenchError::NoMapSource {
                path: document.path().to_path_buf(),
            });
        }
        let map = document.path().to_path_buf();
        let text = document.file().to_bytes();
        let existing = backups_of(&map);
        let number = existing
            .first()
            .map_or(1, |newest| newest.number.saturating_add(1));
        let stem = map
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let folder = map
            .parent()
            .map(|dir| dir.join(BACKUP_FOLDER))
            .unwrap_or_default();
        let failed = |path: &Path| {
            let path = path.to_path_buf();
            move |error| WorkbenchError::Save { path, error }
        };
        std::fs::create_dir_all(&folder).map_err(failed(&folder))?;
        let path = folder.join(format!("{stem}.{number}.{MAP_EXTENSION}"));
        std::fs::write(&path, text).map_err(failed(&path))?;
        // The new one is kept; of those before it, the newest `BACKUPS_KEPT - 1`.
        for old in existing.iter().skip(BACKUPS_KEPT.saturating_sub(1)) {
            std::fs::remove_file(&old.path).map_err(failed(&old.path))?;
        }
        Ok(Backup { path, number })
    }

    /// The open map's backups, the newest first.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoMap`] with no map open.
    pub fn map_backups(&self) -> Result<Vec<Backup>, WorkbenchError> {
        let document = self.map().ok_or(WorkbenchError::NoMap)?;
        Ok(backups_of(document.path()))
    }
}

/// `map.backup`.
pub(crate) struct MapBackup;

impl<C: HasWorkbench + 'static> Command<C> for MapBackup {
    const ID: &'static str = MAP_BACKUP;
    const SUMMARY: &'static str = "Writes the open map, as it is in the editor, saved or not, as \
        its next backup in the autosave folder beside it, keeping the last 20.";
    type Params = NoParams;
    type Output = Backup;
    type Error = WorkbenchError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<Backup, C>, WorkbenchError> {
        Ok(Outcome::outside_history(context.workbench().backup_map()?))
    }
}

/// `map.backups`.
pub(crate) struct MapBackups;

impl<C: HasWorkbench + 'static> Command<C> for MapBackups {
    const ID: &'static str = MAP_BACKUPS;
    const SUMMARY: &'static str = "Lists the open map's backups in the autosave folder beside \
        it, the newest first.";
    type Params = NoParams;
    type Output = Vec<Backup>;
    type Error = WorkbenchError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<Vec<Backup>, C>, WorkbenchError> {
        Ok(Outcome::unchanged(context.workbench().map_backups()?))
    }
}

/// The parameters of `map.restore_backup`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreParams {
    /// The backup's file, as `map.backups` lists it.
    pub path: PathBuf,
}

/// `map.restore_backup`.
pub(crate) struct MapRestoreBackup;

impl<C: HasWorkbench + 'static> Command<C> for MapRestoreBackup {
    const ID: &'static str = MAP_RESTORE_BACKUP;
    const SUMMARY: &'static str = "Puts a backup of the open map in the editor in place of the \
        map, unsaved, as an edit that undo takes back.";
    type Params = RestoreParams;
    type Output = ();
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: RestoreParams,
    ) -> Result<Outcome<(), C>, WorkbenchError> {
        let path = params.path;
        let bytes =
            std::fs::read(&path).map_err(|_| WorkbenchError::NoFile { path: path.clone() })?;
        let file = MapFile::parse(&bytes).map_err(|error| WorkbenchError::Map {
            path: path.clone(),
            error,
        })?;
        let ((), step) = edit_map(context, |document| {
            document.replace_file(file);
            Ok(())
        })?;
        Ok(Outcome::undoable((), step))
    }
}

#[cfg(test)]
mod tests;
