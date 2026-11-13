// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use qs_commands::{Registry, Session as Commands};
use qs_map::{Brush, Item, MapFile, Surface, TexDef};
use qs_wad::write::WadBuilder;
use serde_json::json;

use super::{MAP_COMPILE, MAP_LEAKS, MapReport, Seal};
use crate::map_edit::MAP_OPEN;
use crate::preview::{HasPreview, Preview, PreviewConfig};
use crate::{Config, HasWorkbench, Workbench, register, register_preview};

/// A workbench and a preview with no game.
struct Context {
    workbench: Workbench<FsLoader>,
    preview: Preview,
}

impl HasWorkbench for Context {
    type Loader = FsLoader;

    fn workbench(&mut self) -> &mut Workbench<FsLoader> {
        &mut self.workbench
    }

    fn preview(&mut self) -> Option<&mut Preview> {
        Some(&mut self.preview)
    }
}

impl HasPreview for Context {
    fn workbench_and_preview(&mut self) -> (&mut Workbench<FsLoader>, &mut Preview) {
        (&mut self.workbench, &mut self.preview)
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
fn room(name: &str, first: &str, open: Opening) -> PathBuf {
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
enum Opening {
    Sealed,
    Open,
}

/// A session with the map at `path` open.
fn open(path: &Path) -> Commands<Context> {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    register_preview(&mut registry).unwrap();
    let context = Context {
        workbench: Workbench::new(FsLoader, Config::default()),
        preview: Preview::new(PreviewConfig {
            host: None,
            app: Path::new("/nowhere/app").to_path_buf(),
            game: None,
            extra: Vec::new(),
        }),
    };
    let mut session = Commands::new(context, registry);
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    session
}

fn report(session: &mut Commands<Context>, id: &str) -> MapReport {
    serde_json::from_value(session.execute(id, json!({})).unwrap()).unwrap()
}

// spec QBSP-9.8: map.leaks reports a leak's trail from the player start out through the hole,
// and none for the sealed room; neither writes a file
#[test]
fn map_leaks_reports_the_trail_out() {
    let open_room = room("leaky", "wall", Opening::Open);
    let mut session = open(&open_room);
    let leaky = report(&mut session, MAP_LEAKS);
    assert_eq!(leaky.seal, Seal::Leaks);
    assert_eq!(leaky.leak.first(), Some(&[0.0, 0.0, 24.0]));
    assert!(leaky.leak.last().unwrap()[1] > 128.0, "{:?}", leaky.leak);
    assert!(
        leaky.missing_textures.is_empty(),
        "{:?}",
        leaky.missing_textures
    );
    assert!(leaky.written.is_empty());
    assert!(!open_room.with_extension("bsp").exists());
    let sealed = room("sealed", "wall", Opening::Sealed);
    let mut session = open(&sealed);
    let closed = report(&mut session, MAP_LEAKS);
    assert_eq!(closed.seal, Seal::Sealed);
    assert!(closed.leak.is_empty());
    assert_eq!(closed.missing_wad, None);
}

// spec QBSP-9.2: with no entity in empty space nothing is filled: neither sealed nor leaking
#[test]
fn a_map_with_no_occupant_is_not_called_sealed() {
    let path = room("empty", "wall", Opening::Open);
    let text = std::fs::read_to_string(&path).unwrap();
    let start = text.find("{\n\"classname\" \"info_player_start\"").unwrap();
    std::fs::write(&path, text.get(..start).unwrap()).unwrap();
    let mut session = open(&path);
    let found = report(&mut session, MAP_LEAKS);
    assert_eq!(found.seal, Seal::NoOccupant);
    assert!(found.leak.is_empty());
}

// No spec statement: P6-11: a WAD the world names that is nowhere to be found is reported
#[test]
fn a_missing_wad_is_reported() {
    let path = room("nowad", "wall", Opening::Sealed);
    std::fs::remove_file(path.with_file_name("test.wad")).unwrap();
    let mut session = open(&path);
    assert_eq!(
        report(&mut session, MAP_LEAKS).missing_wad.as_deref(),
        Some("test.wad")
    );
}

// No spec statement: P6-11: the last report is kept with the map until it is edited
#[test]
fn the_report_is_kept_until_an_edit() {
    let path = room("kept", "wall", Opening::Open);
    let mut session = open(&path);
    let made = report(&mut session, MAP_LEAKS);
    let kept = session.context().workbench.map().unwrap().report().cloned();
    assert_eq!(kept, Some(made));
    session
        .execute(
            "map.set_key",
            json!({ "entity": 1, "key": "angle", "value": "90" }),
        )
        .unwrap();
    assert_eq!(session.context().workbench.map().unwrap().report(), None);
}

// spec MAP-9.9: a layer left out of exports is left out of the compile: a player start in it
// is not there, so the open room has no occupant rather than a leak
#[test]
fn an_omitted_layer_is_not_compiled() {
    let path = room("omitted", "wall", Opening::Open);
    let text = std::fs::read_to_string(&path).unwrap();
    let start = text.find("{\n\"classname\" \"info_player_start\"").unwrap();
    let mut text = text.get(..start).unwrap().to_owned();
    text.push_str(
        "{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_layer\"\n\"_tb_name\" \"WIP\"\n\
         \"_tb_id\" \"1\"\n\"_tb_layer_omit_from_export\" \"1\"\n}\n\
         {\n\"classname\" \"info_player_start\"\n\"origin\" \"0 0 24\"\n\"_tb_layer\" \"1\"\n}\n",
    );
    std::fs::write(&path, text).unwrap();
    let mut session = open(&path);
    assert_eq!(report(&mut session, MAP_LEAKS).seal, Seal::NoOccupant);
}

// spec MAP-6.8: a texture the map's WAD lacks is listed once, by name
#[test]
fn map_leaks_lists_the_missing_textures() {
    let path = room("missing", "nowhere", Opening::Sealed);
    let mut session = open(&path);
    let found = report(&mut session, MAP_LEAKS);
    assert_eq!(found.missing_textures, ["nowhere"]);
    assert!(found.leak.is_empty());
    assert!(
        found.warnings.iter().any(|w| w.contains("nowhere")),
        "{:?}",
        found.warnings
    );
}

// spec QBSP-1.7: map.compile writes the BSP, portal and leak files beside the map
#[test]
fn map_compile_writes_the_bsp_beside_the_map() {
    let path = room("written", "wall", Opening::Sealed);
    let mut session = open(&path);
    let made = report(&mut session, MAP_COMPILE);
    let names: Vec<PathBuf> = ["bsp", "prt", "pts"]
        .map(|extension| path.with_extension(extension))
        .to_vec();
    assert_eq!(made.written, names);
    let bsp = std::fs::read(&names[0]).unwrap();
    assert_eq!(bsp.get(..4), Some(&29_u32.to_le_bytes()[..]));
    assert_eq!(std::fs::read(&names[2]).unwrap(), Vec::<u8>::new());
}

// No spec statement: P6-13: with no map open, or a BSP open with no source, nothing compiles
#[test]
fn only_an_open_map_source_compiles() {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    register_preview(&mut registry).unwrap();
    let context = Context {
        workbench: Workbench::new(FsLoader, Config::default()),
        preview: Preview::new(PreviewConfig {
            host: None,
            app: Path::new("/nowhere/app").to_path_buf(),
            game: None,
            extra: Vec::new(),
        }),
    };
    let mut session = Commands::new(context, registry);
    for id in [MAP_LEAKS, MAP_COMPILE] {
        let error = session.execute(id, json!({})).unwrap_err();
        assert!(error.to_string().contains("no map"), "{id}: {error}");
    }
}
