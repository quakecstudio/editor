// SPDX-License-Identifier: GPL-3.0-or-later

//! Maps and QuakeC changed by another program (P5-17, MAP-32). A file watcher, or a test in its
//! place, tells `files.changed` which files changed; the open map with no unsaved edits is read
//! again with its selection kept, one with unsaved edits keeps them and waits for
//! `map.resolve`, and changed QuakeC is read again, so the entity definitions derived from it
//! follow. `map.compare` lists how the map on disk differs from the one being edited.

use std::path::{Path, PathBuf};

use qs_commands::{Command, Outcome};
use qs_map::MapFile;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{DiskStamp, EntityIndex, LiveChange, MapDocument};
use crate::{HasWorkbench, Workbench, WorkbenchError};

/// The id of `files.changed`.
pub const FILES_CHANGED: &str = "files.changed";
/// The id of `map.resolve`.
pub const MAP_RESOLVE: &str = "map.resolve";
/// The id of `map.compare`.
pub const MAP_COMPARE: &str = "map.compare";

/// The extensions of the files that make up a project's QuakeC.
const QUAKEC_EXTENSIONS: [&str; 4] = ["qc", "src", "qh", "toml"];

/// What happened to the open map when its file changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MapOnDisk {
    /// Not changed, or no map is open.
    #[default]
    Same,
    /// It had no unsaved edits and was read again, its selection kept.
    Reloaded,
    /// It has unsaved edits, kept until `map.resolve` says which to take.
    Conflict,
}

/// The parameters of `files.changed`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesChangedParams {
    /// The files another program wrote.
    pub paths: Vec<PathBuf>,
}

/// What `files.changed` did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FilesChanged {
    /// What happened to the open map.
    pub map: MapOnDisk,
    /// Whether the project's QuakeC was read again, and with it the entity definitions.
    pub quakec: bool,
}

/// Which map `map.resolve` keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    /// The map on disk, the unsaved edits dropped.
    Disk,
    /// The map being edited, to be saved over the one on disk.
    Mine,
}

/// The parameters of `map.resolve`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveParams {
    /// Which map to keep.
    pub keep: Keep,
}

/// One way the map on disk differs from the one being edited, by entity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapDifference {
    /// The entity, by its place in the map on disk.
    pub entity: EntityIndex,
    /// How it differs, as the map editor says it.
    pub text: String,
}

impl MapDocument {
    /// The map on disk, when another program changed it while this one had unsaved edits.
    #[must_use]
    pub const fn conflict(&self) -> Option<&MapFile> {
        self.conflict.as_ref()
    }
}

impl<L: crate::Loader> Workbench<L> {
    /// Reads the open map's file again after another program wrote it: with no unsaved edits it
    /// replaces the map, keeping the selection where the map still has it; with unsaved edits
    /// it is kept aside as the conflict.
    fn map_on_disk(&mut self) -> Result<MapOnDisk, WorkbenchError> {
        let Some(document) = self.map.as_ref() else {
            return Ok(MapOnDisk::Same);
        };
        let path = document.path.clone();
        let bytes =
            std::fs::read(&path).map_err(|_| WorkbenchError::NoFile { path: path.clone() })?;
        if DiskStamp::of(&bytes) == document.disk {
            return Ok(MapOnDisk::Same);
        }
        if document.unsaved {
            let file = disk_file(&path, &bytes)?;
            if let Some(document) = self.map.as_mut() {
                document.conflict = Some(file);
                document.touch();
            }
            return Ok(MapOnDisk::Conflict);
        }
        let selected = document.selected;
        let document = self.open_map(&path)?;
        let count = document.file.entities.len();
        if let Some(document) = self.map.as_mut() {
            document.selected = selected.filter(|entity| entity.get() < count);
        }
        Ok(MapOnDisk::Reloaded)
    }

    /// Takes the map on disk or keeps the one being edited, after another program changed it.
    fn resolve_map(&mut self, keep: Keep) -> Result<(), WorkbenchError> {
        let document = self.map_mut()?;
        let path = document.path.clone();
        match keep {
            Keep::Disk => {
                let selected = document.selected;
                let count = self.open_map(&path)?.file.entities.len();
                if let Some(document) = self.map.as_mut() {
                    document.selected = selected.filter(|entity| entity.get() < count);
                }
            }
            Keep::Mine => {
                let bytes = std::fs::read(&path)
                    .map_err(|_| WorkbenchError::NoFile { path: path.clone() })?;
                document.disk = DiskStamp::of(&bytes);
                document.conflict = None;
                document.touch();
            }
        }
        Ok(())
    }
}

/// The entities of the map `bytes` read from `path`, as `map.open` reads them.
fn disk_file(path: &Path, bytes: &[u8]) -> Result<MapFile, WorkbenchError> {
    MapFile::parse(bytes).map_err(|error| WorkbenchError::Map {
        path: path.to_path_buf(),
        error,
    })
}

/// Whether `path` is part of a project's QuakeC.
fn is_quakec(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            QUAKEC_EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// `files.changed`.
pub(super) struct FilesChangedCommand;

impl<C: HasWorkbench + 'static> Command<C> for FilesChangedCommand {
    const ID: &'static str = FILES_CHANGED;
    const SUMMARY: &'static str = "Takes files another program changed: the open map is read \
        again when it has no unsaved edits, else kept for map.resolve; changed QuakeC is read \
        again with the entity definitions derived from it.";
    type Params = FilesChangedParams;
    type Output = FilesChanged;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: FilesChangedParams,
    ) -> Result<Outcome<FilesChanged, C>, WorkbenchError> {
        let workbench = context.workbench();
        let map_path = workbench
            .map()
            .map(|document| document.path().to_path_buf());
        let same = |a: &Path, b: &Path| {
            a == b
                || std::fs::canonicalize(a)
                    .ok()
                    .zip(std::fs::canonicalize(b).ok())
                    .is_some_and(|(a, b)| a == b)
        };
        let map = if params
            .paths
            .iter()
            .any(|path| map_path.as_deref().is_some_and(|open| same(path, open)))
        {
            workbench.map_on_disk()?
        } else {
            MapOnDisk::Same
        };
        let quakec = workbench.root().is_some() && params.paths.iter().any(|path| is_quakec(path));
        if quakec {
            workbench.disk_changed()?;
        }
        Ok(Outcome::outside_history(FilesChanged { map, quakec }))
    }
}

/// `map.resolve`.
pub(super) struct MapResolve;

impl<C: HasWorkbench + 'static> Command<C> for MapResolve {
    const ID: &'static str = MAP_RESOLVE;
    const SUMMARY: &'static str = "After another program changed the open map's file while \
        it had unsaved edits: keep disk to read the file again, or mine to keep the edits and \
        save them over it.";
    type Params = ResolveParams;
    type Output = ();
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: ResolveParams,
    ) -> Result<Outcome<(), C>, WorkbenchError> {
        context.workbench().resolve_map(params.keep)?;
        Ok(Outcome::outside_history(()))
    }
}

/// `map.compare`.
pub(super) struct MapCompare;

impl<C: HasWorkbench + 'static> Command<C> for MapCompare {
    const ID: &'static str = MAP_COMPARE;
    const SUMMARY: &'static str = "How the open map's file, changed by another program, differs \
        from the map being edited, entity by entity.";
    type Params = qs_commands::NoParams;
    type Output = Vec<MapDifference>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        _params: qs_commands::NoParams,
    ) -> Result<Outcome<Vec<MapDifference>, C>, WorkbenchError> {
        let document = context.workbench().map().ok_or(WorkbenchError::NoMap)?;
        let Some(disk) = document.conflict() else {
            return Ok(Outcome::unchanged(Vec::new()));
        };
        let differences = LiveChange::between(&document.file().entities, &disk.entities)
            .into_iter()
            .map(|change| change.describe(&disk.entities))
            .collect();
        Ok(Outcome::unchanged(differences))
    }
}

#[cfg(test)]
mod tests;
