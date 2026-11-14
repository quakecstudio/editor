// SPDX-License-Identifier: GPL-3.0-or-later

//! A context for the map editor's own tests: a map store, with a project and a game that
//! answer what a test gives them, in place of the IDE's workbench and preview.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::{Path, PathBuf};

use qs_commands::{Registry, Session};
use qs_map::{Entity, MapFile};

use super::{
    ClassInfo, EntityCheck, EntityProblem, GameFolders, HasMap, LiveChange, LiveMap, MapDocument,
    MapProject, MapStore, register_map_edit, register_map_game,
};

/// A map store with a project and, when a test gives it one, a game.
#[derive(Default)]
pub(crate) struct MapContext {
    pub(crate) store: MapStore,
    pub(crate) project: TestProject,
    pub(crate) game: Option<TestGame>,
}

impl MapContext {
    /// A session over `self` with every map command.
    pub(crate) fn into_session(self) -> Session<Self> {
        let mut registry = Registry::new();
        register_map_edit(&mut registry).unwrap();
        register_map_game(&mut registry).unwrap();
        Session::new(self, registry)
    }

    /// A session over a context with no project and no game.
    pub(crate) fn session() -> Session<Self> {
        Self::default().into_session()
    }

    /// The open map.
    pub(crate) fn map(&self) -> Option<&MapDocument> {
        self.store.map()
    }
}

impl HasMap for MapContext {
    type Project = TestProject;
    type Live = TestGame;

    fn map_store(&mut self) -> &mut MapStore {
        &mut self.store
    }

    fn map_project(&mut self) -> &mut TestProject {
        &mut self.project
    }

    fn live_map(&mut self) -> Option<&mut TestGame> {
        self.game.as_mut()
    }
}

/// A project that is what a test says: its folder, its classes, the keys its code fires
/// entities by and the problems its check finds with any map.
#[derive(Default)]
pub(crate) struct TestProject {
    pub(crate) root: Option<PathBuf>,
    pub(crate) classes: Vec<ClassInfo>,
    pub(crate) targeting: Vec<String>,
    pub(crate) problems: Option<Vec<EntityProblem>>,
}

impl MapProject for TestProject {
    fn project_root(&self) -> Option<PathBuf> {
        self.root.clone()
    }

    fn entity_class(&mut self, classname: &str) -> Option<ClassInfo> {
        self.classes
            .iter()
            .find(|class| class.name == classname)
            .cloned()
    }

    fn targeting_keys(&mut self) -> Vec<String> {
        self.targeting.clone()
    }

    fn entity_check(&mut self) -> Option<EntityCheck> {
        let problems = self.problems.clone()?;
        Some(Box::new(move |_: &MapFile| problems.clone()))
    }
}

/// A game that records what the map editor tells it.
#[derive(Default)]
pub(crate) struct TestGame {
    /// The folders its files are read from.
    pub(crate) folders: GameFolders,
    /// The map it plays, in step with the editor.
    pub(crate) playing: Option<PathBuf>,
    /// The changes sent to it, in order.
    pub(crate) sent: Vec<LiveChange>,
    /// The maps it was told were read again.
    pub(crate) reread: Vec<PathBuf>,
}

impl LiveMap for TestGame {
    fn plays_in_step(&self, map: &Path) -> bool {
        self.playing.as_deref() == Some(map)
    }

    fn send_changes(&mut self, _: &Path, _: &[Entity], changes: &[LiveChange]) {
        self.sent.extend_from_slice(changes);
    }

    fn read_again(&mut self, map: &Path) {
        self.reread.push(map.to_path_buf());
    }

    fn game_folders(&self) -> GameFolders {
        self.folders.clone()
    }
}
