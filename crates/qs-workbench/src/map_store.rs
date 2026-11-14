// SPDX-License-Identifier: GPL-3.0-or-later

//! The open map's place in a command context, and what map editing is given by the rest of
//! the IDE: the project the map belongs to and the game that may be playing it.
//!
//! Map editing knows nothing of QuakeC or of the engine. A context that has a map
//! ([`HasMap`]) hands it the three things it asks of them: the project's folder and what its
//! code says of entities ([`MapProject`]), the running game its edits are sent to
//! ([`LiveMap`]), and the folders the game's files are read from ([`GameFiles`]).

use std::path::{Path, PathBuf};

use qs_map::{Entity, MapFile};

use crate::entity_classes::ClassInfo;
use crate::map_edit::{EntityIndex, LiveChange, MapDocument, QuickFix};

/// The map open in the map view, if one is.
#[derive(Debug, Default)]
pub struct MapStore {
    pub(crate) document: Option<MapDocument>,
}

/// A context the map commands work on: one with a [`MapStore`].
pub trait HasMap {
    /// The project its maps belong to.
    type Project: MapProject;
    /// The game that may be playing the open map.
    type Live: LiveMap;

    /// The open map.
    fn map_store(&mut self) -> &mut MapStore;

    /// The project.
    fn map_project(&mut self) -> &mut Self::Project;

    /// The game, for a context that can run one: the map editor's edits reach it (P5-07).
    /// `None` for one that cannot, such as the command line's.
    fn live_map(&mut self) -> Option<&mut Self::Live>;

    /// Where the game's files are read from; nowhere for a context with no game.
    fn game_files(&mut self) -> GameFiles {
        self.live_map()
            .map(|live| live.game_files())
            .unwrap_or_default()
    }
}

/// What map editing asks of the project a map belongs to. Everything is answered as owned
/// data, so a command reads it and then takes the map.
pub trait MapProject {
    /// The project's folder; `None` with no project open.
    fn project_root(&self) -> Option<PathBuf>;

    /// The entity classes the project's code defines; `None` with no project open.
    fn entity_classes(&mut self) -> Option<Vec<ClassInfo>>;

    /// The keys by which the project's code fires other entities, beside `target` and
    /// `killtarget`.
    fn targeting_keys(&mut self) -> Vec<String>;

    /// A check of a map's entities against the project's code (MAP-8); `None` with no project
    /// open.
    fn entity_check(&mut self) -> Option<EntityCheck>;
}

/// A check of a map's entities against a project's code: the problems of a map.
pub type EntityCheck = Box<dyn Fn(&MapFile) -> Vec<EntityProblem>>;

/// What is wrong with an entity of a map, by the project's code.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityProblem {
    /// The entity.
    pub entity: EntityIndex,
    /// The 1-based line it is on.
    pub line: u32,
    /// What is wrong.
    pub message: String,
    /// The edit that puts it right, when there is one.
    pub fix: Option<QuickFix>,
}

/// What map editing asks of a game that may be playing the open map.
pub trait LiveMap {
    /// Whether the game runs on the map at `map` and holds its entities as the editor does.
    fn plays_in_step(&self, map: &Path) -> bool;

    /// Sends `changes`, made to the entities `before`, to the game playing `map` (MAP-12).
    fn send_changes(&mut self, map: &Path, before: &[Entity], changes: &[LiveChange]);

    /// Tells the game playing `map` that the editor read the map again from disk, so its
    /// edits wait for the next compile (P5-17).
    fn read_again(&mut self, map: &Path);

    /// Where the game's files are read from.
    fn game_files(&self) -> GameFiles;
}

/// The folders a game's files are read from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameFiles {
    /// The original game's base directory, when an install is found.
    pub base: Option<PathBuf>,
    /// The game folder the user chose, when one is.
    pub chosen: Option<PathBuf>,
}
