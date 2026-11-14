// SPDX-License-Identifier: GPL-3.0-or-later

//! A context for the map editor's own tests: a map store, with a project and a game that
//! answer what a test gives them, in place of the IDE's workbench and preview.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};

use qs_commands::{Registry, Session};
use qs_map::{Brush, Entity, Item, MapFile, Surface, TexDef};
use qs_wad::write::WadBuilder;
use serde_json::json;

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

    /// A session over a context whose project's folder is `root`, with no game.
    pub(crate) fn in_project(root: &Path) -> Session<Self> {
        Self {
            project: TestProject {
                root: Some(root.to_path_buf()),
                ..TestProject::default()
            },
            ..Self::default()
        }
        .into_session()
    }

    /// A session over a context with no project and no game, the map at `path` open.
    pub(crate) fn opened(path: &Path) -> Session<Self> {
        let mut session = Self::session();
        session
            .execute(super::MAP_OPEN, json!({ "path": path }))
            .unwrap();
        session
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
/// entities by and its check of a map's entities.
#[derive(Default)]
pub(crate) struct TestProject {
    pub(crate) root: Option<PathBuf>,
    pub(crate) classes: Vec<ClassInfo>,
    pub(crate) targeting: Vec<String>,
    pub(crate) check: Option<fn(&MapFile) -> Vec<EntityProblem>>,
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
        let check = self.check?;
        Some(Box::new(check))
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

/// A texture lump `name`, 16 by 16, every pixel 1.
fn miptex(name: &str) -> Vec<u8> {
    let mut lump = name.as_bytes().to_vec();
    lump.resize(16, 0);
    for value in [16_u32, 16, 40, 40 + 256, 40 + 256 + 64, 40 + 256 + 64 + 16] {
        lump.extend_from_slice(&value.to_le_bytes());
    }
    lump.resize(40 + 256 + 64 + 16 + 4, 1);
    lump
}

/// A room of six walls of `wall` around -128 to 128, its +y wall left out when `open`, the
/// first wall's texture `first`, with a player start inside, naming `test.wad`; written with
/// that WAD (holding only `wall`) into a folder of its own; the map's path.
pub(crate) fn room(name: &str, first: &str, open: Opening) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("qs-wb-compile-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut wad = WadBuilder::new();
    wad.add(b"wall", qs_wad::WAD_TYP_MIPTEX, miptex("wall"))
        .unwrap();
    std::fs::write(dir.join("test.wad"), wad.to_bytes().unwrap()).unwrap();
    let mut file = MapFile::parse(
        b"{\n\"classname\" \"worldspawn\"\n\"wad\" \"test.wad\"\n}\n\
          {\n\"classname\" \"info_player_start\"\n\"origin\" \"0 0 24\"\n}\n",
    )
    .unwrap();
    let surface = |texture: &str| Surface {
        texture: texture.as_bytes().to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        },
    };
    let (a, b) = (128.0, 144.0);
    let walls = [
        ([-b, -b, -b], [b, b, -a]),
        ([-b, -b, a], [b, b, b]),
        ([-b, -b, -a], [-a, b, a]),
        ([a, -b, -a], [b, b, a]),
        ([-a, -b, -a], [a, -a, a]),
        ([-a, a, -a], [a, b, a]),
    ];
    let kept = match open {
        Opening::Sealed => walls.len(),
        Opening::Open => walls.len().saturating_sub(1),
    };
    for (i, (min, max)) in walls.into_iter().take(kept).enumerate() {
        let texture = if i == 0 { first } else { "wall" };
        let brush = Brush::cuboid(min, max, &surface(texture)).unwrap();
        file.entities[0].items.push(Item::Brush(brush));
    }
    let path = dir.join(format!("{name}.map"));
    std::fs::write(&path, file.to_bytes()).unwrap();
    path
}

/// Whether the room has its +y wall.
#[derive(Clone, Copy)]
pub(crate) enum Opening {
    Sealed,
    Open,
}
