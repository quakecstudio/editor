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

use super::{MAP_COMPILE, MAP_LEAKS, MAP_PORTALS, MapPortals, MapReport, Seal};
use crate::map_edit::MAP_OPEN;
use crate::preview::{HasPreview, Preview, PreviewConfig};
use crate::{Config, HasWorkbench, Workbench, register, register_preview};

/// A workbench and a preview with no game.
pub(crate) struct Context {
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

/// A session with the map at `path` open.
pub(crate) fn open(path: &Path) -> Commands<Context> {
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
    assert!(closed.missing_wads.is_empty());
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
    assert_eq!(report(&mut session, MAP_LEAKS).missing_wads, ["test.wad"]);
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

// spec MAP-14.1, MAP-14.7: an external map instance is compiled as the brush entity it becomes,
// its brushes read from the file it names beside the map; a file that is not there stops the
// compile
#[test]
fn external_maps_are_compiled_in_place() {
    let path = room("external", "wall", Opening::Sealed);
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir.join("prefabs")).unwrap();
    let mut prefab = MapFile::parse(b"{\n\"classname\" \"worldspawn\"\n}\n").unwrap();
    let surface = Surface {
        texture: b"wall".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        },
    };
    let pillar = Brush::cuboid([-8.0, -8.0, 0.0], [8.0, 8.0, 64.0], &surface).unwrap();
    prefab.entities[0].items.push(Item::Brush(pillar));
    std::fs::write(dir.join("prefabs/pillar.map"), prefab.to_bytes()).unwrap();
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str(
        "{\n\"classname\" \"misc_external_map\"\n\"_external_map\" \"prefabs/pillar.map\"\n\
         \"_external_map_classname\" \"func_wall\"\n\"origin\" \"32 32 -128\"\n}\n",
    );
    std::fs::write(&path, &text).unwrap();
    let mut session = open(&path);
    let made = report(&mut session, MAP_COMPILE);
    let bsp = std::fs::read(&made.written[0]).unwrap();
    let lump = String::from_utf8_lossy(&bsp);
    assert!(
        lump.contains("\"classname\" \"func_wall\""),
        "the instance became a func_wall"
    );
    assert!(lump.contains("\"model\" \"*1\""), "with a model of its own");
    std::fs::remove_file(dir.join("prefabs/pillar.map")).unwrap();
    let mut session = open(&path);
    let error = session.execute(MAP_COMPILE, json!({})).unwrap_err();
    assert!(error.to_string().contains("prefabs/pillar.map"), "{error}");
}

// No spec statement: P6-20: map.portals reads the portal file the last compile wrote beside the
// map, each portal's points; before any compile there is none to read
#[test]
fn map_portals_reads_the_last_compiles_portals() {
    let path = room("portals", "wall", Opening::Sealed);
    let mut session = open(&path);
    let error = session.execute(MAP_PORTALS, json!({})).unwrap_err();
    assert!(error.to_string().contains("portals.prt"), "{error}");
    report(&mut session, MAP_COMPILE);
    let read: MapPortals =
        serde_json::from_value(session.execute(MAP_PORTALS, json!({})).unwrap()).unwrap();
    assert_eq!(read.path, path.with_extension("prt"));
    let text = std::fs::read_to_string(&read.path).unwrap();
    let count: usize = text.lines().nth(2).unwrap().trim().parse().unwrap();
    assert_eq!(read.portals.len(), count);
    assert!(read.portals.iter().all(|portal| portal.len() >= 3));
    std::fs::write(&read.path, "PRT2\n1\n0\n").unwrap();
    let error = session.execute(MAP_PORTALS, json!({})).unwrap_err();
    assert!(error.to_string().contains("not a portal file"), "{error}");
}

// No spec statement: P6-20: a compile that writes no portal file, as a leaking one does not,
// removes the one an earlier compile left, so map.portals does not read it as this one's
#[test]
fn a_leaking_compile_removes_the_last_portal_file() {
    let path = room("stale-portals", "wall", Opening::Sealed);
    let mut session = open(&path);
    report(&mut session, MAP_COMPILE);
    assert!(path.with_extension("prt").exists());
    let leaky = room("stale-portals-open", "wall", Opening::Open);
    std::fs::copy(&leaky, &path).unwrap();
    let mut session = open(&path);
    let found = report(&mut session, MAP_COMPILE);
    assert_eq!(found.seal, Seal::Leaks);
    assert!(!path.with_extension("prt").exists());
    assert!(session.execute(MAP_PORTALS, json!({})).is_err());
}
