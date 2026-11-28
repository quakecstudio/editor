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
use qs_wad::texture::Palette;
use serde_json::json;

use super::{MAP_COMPILE, MAP_LEAKS, MAP_PORTALS, MapPortals};
use crate::preview::{HasPreview, Preview, PreviewConfig};
use crate::{Config, HasWorkbench, Workbench, register, register_preview};
use qs_map_edit::testing::{Opening, level_bsp, room};
use qs_map_edit::texture_import::{DitherChoice, TEXTURE_IMPORT, quantized};
use qs_map_edit::{MAP_OPEN, MapReport, Seal};

/// A workbench and a preview with no game.
pub(crate) struct Context {
    pub(crate) workbench: Workbench<FsLoader>,
    preview: Preview,
}

impl Context {
    /// A context of `workbench` and `preview`.
    pub(crate) const fn new(workbench: Workbench<FsLoader>, preview: Preview) -> Self {
        Self { workbench, preview }
    }
}

impl crate::HasMap for Context {
    type Project = Workbench<FsLoader>;
    type Live = Preview;

    fn map_store(&mut self) -> &mut crate::MapStore {
        self.workbench.maps_mut()
    }

    fn map_project(&mut self) -> &mut Workbench<FsLoader> {
        &mut self.workbench
    }

    fn live_map(&mut self) -> Option<&mut Preview> {
        Some(&mut self.preview)
    }
}

impl HasWorkbench for Context {
    type Loader = FsLoader;

    fn workbench(&mut self) -> &mut Workbench<FsLoader> {
        &mut self.workbench
    }
}

impl HasPreview for Context {
    fn workbench_and_preview(&mut self) -> (&mut Workbench<FsLoader>, &mut Preview) {
        (&mut self.workbench, &mut self.preview)
    }
}

/// A session with the map at `path` open.
pub(crate) fn open(path: &Path) -> Commands<Context> {
    open_with_game(path, None)
}

/// A session with the map at `path` open and the game's files in `game`, when there is one.
fn open_with_game(path: &Path, game: Option<PathBuf>) -> Commands<Context> {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    register_preview(&mut registry).unwrap();
    let context = Context {
        workbench: Workbench::new(FsLoader, Config::default()),
        preview: Preview::new(PreviewConfig {
            host: None,
            app: Path::new("/nowhere/app").to_path_buf(),
            game,
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

// spec QBSP-15.36: map.find_leaks reports a leak's trail from the player start out through the
// hole, and none for the sealed room; neither writes a file
#[test]
fn map_leaks_reports_the_trail_out() {
    let open_room = room("leaky", "wall", Opening::Open);
    let mut session = open(&open_room);
    let leaky = report(&mut session, MAP_LEAKS);
    assert_eq!(leaky.seal, Seal::Leaks);
    assert_eq!(leaky.leak.first(), Some(&[0.0, 0.0, 24.0]));
    assert!(leaky.leak.iter().any(|p| p[1] > 128.0), "{:?}", leaky.leak);
    // The trail ends where the fill began, in the void beyond the room's walls.
    let end = leaky.leak.last().unwrap();
    assert!(end.iter().any(|c| c.abs() > 144.0), "{:?}", leaky.leak);
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

// spec QBSP-15.50: a texture the map's WAD lacks is listed once, by name
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

// No spec statement: P6-43, the acceptance criterion: a map with no `wad` key whose faces name
// textures of a level on the game's search path compiles with no missing texture, the BSP
// written holds them whole, and nothing is written into the game's folder; with no game they
// are missing
#[test]
fn a_map_with_no_wad_is_textured_from_the_games_levels() {
    let path = room("levels", "rock", Opening::Sealed);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"wad\" \"test.wad\"\n"));
    std::fs::write(&path, text.replace("\"wad\" \"test.wad\"\n", "")).unwrap();
    std::fs::remove_file(path.with_file_name("test.wad")).unwrap();
    // The game keeps its levels in a PAK, as Quake does.
    let game = path.with_file_name("game");
    let id1 = game.join("id1");
    std::fs::create_dir_all(&id1).unwrap();
    let level = level_bsp(&["wall", "rock", "unused"]);
    // The full game, whose search path is read (FS-4.2): id's pop table in gfx/pop.lmp.
    let pop: Vec<u8> = qs_vfs::FS_POP_TABLE
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect();
    let pak = qs_pak::write_pak(&[
        (qs_vfs::FS_POP_PATH, pop.as_slice()),
        ("maps/e1.bsp", level.as_slice()),
    ])
    .unwrap();
    std::fs::write(id1.join("pak0.pak"), &pak).unwrap();
    let folder = |dir: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect()
    };
    let before = (folder(&game), folder(&id1));

    let mut without = open(&path);
    let lacking = report(&mut without, MAP_LEAKS).missing_textures;
    assert_eq!(lacking, ["rock", "wall"], "no game, no texture");

    let mut session = open_with_game(&path, Some(game.clone()));
    let made = report(&mut session, MAP_COMPILE);
    assert!(made.missing_textures.is_empty(), "{made:?}");
    assert!(made.missing_wads.is_empty(), "{made:?}");
    let bytes = std::fs::read(path.with_extension("bsp")).unwrap();
    let bsp = qs_bsp::Bsp::read(&bytes, qs_bsp::BspLoader::GlQuake).unwrap();
    let mut held: Vec<(String, usize)> = bsp
        .textures
        .iter()
        .flatten()
        .map(|t| {
            (
                String::from_utf8_lossy(&t.name).into_owned(),
                t.pixels.len(),
            )
        })
        .collect();
    held.sort();
    let whole = 256 + 64 + 16 + 4;
    assert_eq!(
        held,
        [("rock".to_owned(), whole), ("wall".to_owned(), whole)]
    );
    assert_eq!((folder(&game), folder(&id1)), before, "nothing is added");
    assert_eq!(std::fs::read(id1.join("pak0.pak")).unwrap(), pak);
}

// spec QBSP-15.49: map.compile writes the BSP and portal file beside a sealed map, and no leak
// file
#[test]
fn map_compile_writes_the_bsp_beside_the_map() {
    let path = room("written", "wall", Opening::Sealed);
    let mut session = open(&path);
    let made = report(&mut session, MAP_COMPILE);
    let names: Vec<PathBuf> = ["bsp", "prt"]
        .map(|extension| path.with_extension(extension))
        .to_vec();
    assert_eq!(made.written, names);
    let bsp = std::fs::read(&names[0]).unwrap();
    assert_eq!(bsp.get(..4), Some(&29_u32.to_le_bytes()[..]));
    assert!(!path.with_extension("pts").exists());
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
    std::fs::write(&read.path, "PRT3\n1\n0\n").unwrap();
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

// No spec statement: P6-16: an imported texture put on a brush compiles into the BSP with the
// picture the preview showed, from the WAD the import named in the map's world
#[test]
fn an_imported_texture_compiles_into_the_bsp() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/fixtures/keep");
    let dir = std::env::temp_dir().join(format!("qs-wb-compile-import-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    for file in [
        "progs.src",
        "defs.qc",
        "world.qc",
        "maps/keep.map",
        "gfx/keep.wad",
        "gfx/palette.lmp",
    ] {
        let to = dir.join(file);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(fixture.join(file), to).unwrap();
    }
    let image = dir.join("brick16.png");
    let picture = image::RgbaImage::from_pixel(16, 16, image::Rgba([200, 40, 30, 255]));
    let mut png = std::io::Cursor::new(Vec::new());
    picture.write_to(&mut png, image::ImageFormat::Png).unwrap();
    std::fs::write(&image, png.into_inner()).unwrap();
    let mut session = open(&dir.join("maps/keep.map"));
    session
        .execute("project.open", json!({ "path": dir }))
        .unwrap();
    session
        .execute(TEXTURE_IMPORT, json!({ "path": image }))
        .unwrap();
    session
        .execute(
            "map.brush_texture",
            json!({ "entity": 4, "brushes": [0], "texture": "brick16" }),
        )
        .unwrap();
    let report: MapReport =
        serde_json::from_value(session.execute("map.compile", json!({})).unwrap()).unwrap();
    assert!(report.missing_textures.is_empty(), "{report:?}");
    let bsp_bytes = std::fs::read(dir.join("maps/keep.bsp")).unwrap();
    let bsp = qs_bsp::Bsp::read(&bsp_bytes, qs_bsp::BspLoader::default()).unwrap();
    let texture = bsp
        .textures
        .iter()
        .flatten()
        .find(|texture| texture.name == b"brick16")
        .unwrap();
    let palette = Palette::from_lmp(&std::fs::read(dir.join("gfx/palette.lmp")).unwrap()).unwrap();
    let compiled = palette
        .decode_pixels(b"brick16", texture.width, texture.height, texture.pixels)
        .unwrap();
    let shown = quantized(
        &std::fs::read(&image).unwrap(),
        "brick16",
        &palette,
        DitherChoice::None,
    )
    .unwrap();
    assert_eq!(compiled, shown);
}

// spec MAP-16.3: a region compile is sealed, so a room open to the void compiles in a region
// around it with no leak
// spec MAP-16.4: the seal is skip, solid and never drawn: no face of the region's BSP is skip,
// and a WAD with no skip is not reported for it
// spec MAP-16.5: a region map.compile writes its files under the map's name and -region, so the
// whole map's are kept
#[test]
fn a_region_around_an_open_room_does_not_leak() {
    let path = room("region-open", "wall", Opening::Open);
    let mut session = open(&path);
    let whole = report(&mut session, MAP_COMPILE);
    assert!(!whole.leak.is_empty(), "the whole map leaks");
    let whole_bsp = std::fs::read(path.with_extension("bsp")).unwrap();
    let region =
        json!({ "region": { "box": { "min": [-200, -200, -200], "max": [200, 200, 200] } } });
    let made: MapReport =
        serde_json::from_value(session.execute(MAP_COMPILE, region).unwrap()).unwrap();
    assert!(made.leak.is_empty(), "the region is sealed");
    assert!(
        made.missing_textures.is_empty(),
        "{:?}",
        made.missing_textures
    );
    let region_path = path.with_file_name("region-open-region.bsp");
    assert!(made.written.contains(&region_path), "{:?}", made.written);
    assert_eq!(
        std::fs::read(path.with_extension("bsp")).unwrap(),
        whole_bsp,
        "the whole map's BSP is kept"
    );
    let bytes = std::fs::read(&region_path).unwrap();
    let bsp = qs_bsp::Bsp::read(&bytes, qs_bsp::BspLoader::default()).unwrap();
    assert!(!bsp.faces.is_empty());
    for face in &bsp.faces {
        let slot = bsp.texinfo[face.texinfo].texture;
        let name = slot
            .and_then(|slot| bsp.textures.get(slot))
            .and_then(Option::as_ref)
            .map(|texture| texture.name.clone());
        assert_ne!(name.as_deref(), Some(&b"skip"[..]), "a seal face is drawn");
    }
}

// spec MAP-16.1: a box with no volume is refused
// spec MAP-16.5: map.compile takes a region of rooms or a box; an entity in no room, or past the
// map's, is refused and named
#[test]
fn a_region_with_no_volume_is_refused() {
    let path = room("region-none", "wall", Opening::Sealed);
    let mut session = open(&path);
    let flat = json!({ "region": { "box": { "min": [0, 0, 0], "max": [64, 64, 0] } } });
    let error = session.execute(MAP_COMPILE, flat).unwrap_err();
    assert!(error.to_string().contains("no volume"), "{error}");
    let roomless = json!({ "region": { "rooms": [1] } });
    let error = session.execute(MAP_COMPILE, roomless).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("entity 1 is in no layer or group"),
        "{error}"
    );
    let past = json!({ "region": { "rooms": [9] } });
    let error = session.execute(MAP_COMPILE, past).unwrap_err();
    assert!(error.to_string().contains('9'), "{error}");
}

// spec MAP-16.5: play from here with a region the camera is outside is refused before anything
// compiles
#[test]
fn play_from_outside_the_region_is_refused() {
    let path = room("region-eye", "wall", Opening::Sealed);
    let mut session = open(&path);
    let params = json!({
        "eye": [500, 500, 500],
        "region": { "box": { "min": [-200, -200, -200], "max": [200, 200, 200] } }
    });
    let error = session
        .execute(super::MAP_PLAY_FROM_HERE, params)
        .unwrap_err();
    assert!(error.to_string().contains("outside the region"), "{error}");
    assert!(!path.with_extension("bsp").exists(), "nothing was compiled");
}

// No spec statement: L-52: with no eye the map is played from its first player start, the eye
// view_ofs above its origin and the facing the start's own unless one is given; an eye given
// is the eye; a map with no start is refused before anything compiles, and plays from an eye
#[test]
fn with_no_eye_a_map_plays_from_its_own_start() {
    use super::PlayFromHereParams;
    use crate::preview::{GameStart, PreviewError};
    let params = |eye, angles| PlayFromHereParams {
        eye,
        angles,
        region: None,
        start: GameStart::default(),
        game: crate::GameOptions::default(),
    };
    let world = "{\n\"classname\" \"worldspawn\"\n}\n";
    let starts = format!(
        "{world}{{\n\"classname\" \"light\"\n\"origin\" \"1 1 1\"\n}}\n\
         {{\n\"classname\" \"info_player_start\"\n\"origin\" \"8 -16 24\"\n\"angle\" \"90\"\n}}\n\
         {{\n\"classname\" \"info_player_start\"\n\"origin\" \"99 99 99\"\n}}\n"
    );
    let file = MapFile::parse(starts.as_bytes()).unwrap();
    assert_eq!(
        params(None, None).view_in(&file).unwrap(),
        ([8.0, -16.0, 46.0], Some([0.0, 90.0, 0.0]))
    );
    assert_eq!(
        params(None, Some([10.0, 20.0, 0.0]))
            .view_in(&file)
            .unwrap(),
        ([8.0, -16.0, 46.0], Some([10.0, 20.0, 0.0]))
    );
    assert_eq!(
        params(Some([1.0, 2.0, 3.0]), None).view_in(&file).unwrap(),
        ([1.0, 2.0, 3.0], None)
    );
    let none = MapFile::parse(world.as_bytes()).unwrap();
    assert!(params(Some([1.0, 2.0, 3.0]), None).view_in(&none).is_ok());
    let error = params(None, None).view_in(&none).unwrap_err();
    assert!(error.to_string().contains("no player start"), "{error}");

    let dir = std::env::temp_dir().join(format!("qs-wb-no-start-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("empty.map");
    std::fs::write(&path, world).unwrap();
    let mut session = open(&path);
    let error = session
        .execute(super::MAP_PLAY_FROM_HERE, json!({}))
        .unwrap_err();
    assert!(error.to_string().contains("no player start"), "{error}");
    assert!(!path.with_extension("bsp").exists(), "nothing was compiled");

    // What is said of a start with no room, or outside a region, names the start.
    let path = room("region-start", "wall", Opening::Sealed);
    let mut session = open(&path);
    let far = json!({ "region": { "box": { "min": [300, 300, 300], "max": [400, 400, 400] } } });
    let error = session.execute(super::MAP_PLAY_FROM_HERE, far).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("player start is outside the region"),
        "{error}"
    );
    assert!(!path.with_extension("bsp").exists(), "nothing was compiled");
    let said = PreviewError::NoRoomAtStart([8.0, -16.0, 24.0]).to_string();
    assert!(
        said.contains("player start, (8 -16 24)") && said.contains("info_player_start"),
        "{said}"
    );
}

// spec MAP-16.1: the region of a layer is the box around its brushes: compiling it leaves out
// the brushes of the map outside it
#[test]
fn a_layer_compiles_as_its_region() {
    let path = room("region-layer", "wall", Opening::Sealed);
    // A layer of two pillars in opposite corners of the room, and a block far outside it in the
    // world.
    let mut text = std::fs::read(&path).unwrap();
    text.extend_from_slice(
        b"\n{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_layer\"\n\
          \"_tb_name\" \"Pillar\"\n\"_tb_id\" \"1\"\n}\n",
    );
    let mut file = MapFile::parse(&text).unwrap();
    let surface = Surface {
        texture: b"wall".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        },
    };
    for (min, max) in [
        ([-112.0, -112.0, -128.0], [-96.0, -96.0, 100.0]),
        ([96.0, 96.0, -128.0], [112.0, 112.0, 100.0]),
    ] {
        let pillar = Brush::cuboid(min, max, &surface).unwrap();
        file.entities[2].items.push(Item::Brush(pillar));
    }
    let far = Brush::cuboid([4000.0; 3], [4064.0; 3], &surface).unwrap();
    file.entities[0].items.push(Item::Brush(far));
    std::fs::write(&path, file.to_bytes()).unwrap();
    let mut session = open(&path);
    let rooms = json!({ "region": { "rooms": [2] } });
    let made: MapReport =
        serde_json::from_value(session.execute(MAP_COMPILE, rooms).unwrap()).unwrap();
    assert!(made.leak.is_empty());
    let bsp = std::fs::read(path.with_file_name("region-layer-region.bsp")).unwrap();
    let read = qs_bsp::Bsp::read(&bsp, qs_bsp::BspLoader::default()).unwrap();
    let world = read.models[0];
    assert!(
        world.maxs.iter().all(|&c| c < 1000.0),
        "the far block is left out: {:?}",
        world.maxs
    );
}

// No spec statement: L-40, the acceptance criterion: two rooms joined by a carved doorway are
// built using only the brush places map.brushes answered, and the map is sealed: a brush a
// tool adds takes the place `total`, the places are read again after a tool that cuts, and
// the doorway's own space holds no brush once it is carved
#[test]
fn two_rooms_are_joined_using_the_places_map_brushes_answers() {
    use qs_map_edit::{
        BrushesListed, MAP_BRUSH_CREATE, MAP_BRUSH_HOLLOW, MAP_BRUSH_SUBTRACT, MAP_BRUSHES,
        MAP_PLACE,
    };

    let dir = std::env::temp_dir().join(format!("qs-wb-joined-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("joined.map");
    std::fs::write(&path, "{\n\"classname\" \"worldspawn\"\n}\n").unwrap();
    let mut session = open(&path);
    let listed = |session: &mut Commands<Context>, params: serde_json::Value| -> BrushesListed {
        serde_json::from_value(session.execute(MAP_BRUSHES, params).unwrap()).unwrap()
    };
    // A box, added at the place `total`, then hollowed into a room.
    let room = |session: &mut Commands<Context>, min: [i32; 3], max: [i32; 3]| {
        let place = listed(session, json!({})).total;
        session
            .execute(
                MAP_BRUSH_CREATE,
                json!({ "min": min, "max": max, "texture": "wall" }),
            )
            .unwrap();
        session
            .execute(
                MAP_BRUSH_HOLLOW,
                json!({ "entity": 0, "brush": place, "wall": 16 }),
            )
            .unwrap();
    };
    room(&mut session, [-256, -256, 0], [256, 256, 224]);
    room(&mut session, [240, -256, 0], [752, 256, 224]);
    assert_eq!(
        listed(&mut session, json!({})).total,
        12,
        "six walls a room"
    );

    // The two rooms' walls between them, from x = 240 to 256, are what the doorway goes
    // through; before it is carved, brushes fill its space.
    let doorway = json!([[241, -31, 17], [255, 31, 127]]);
    assert!(
        !listed(&mut session, json!({ "within": doorway }))
            .brushes
            .is_empty()
    );
    let cutter = listed(&mut session, json!({})).total;
    session
        .execute(
            MAP_BRUSH_CREATE,
            json!({ "min": [224, -32, 16], "max": [272, 32, 128], "texture": "wall" }),
        )
        .unwrap();
    session
        .execute(MAP_BRUSH_SUBTRACT, json!({ "entity": 0, "brush": cutter }))
        .unwrap();
    assert!(
        listed(&mut session, json!({ "within": doorway }))
            .brushes
            .is_empty(),
        "the doorway's space is clear"
    );

    session
        .execute(
            MAP_PLACE,
            json!({ "classname": "info_player_start", "origin": [0, 0, 40], "keys": [] }),
        )
        .unwrap();
    let leaks = report(&mut session, MAP_LEAKS);
    assert_eq!(leaks.seal, Seal::Sealed, "{leaks:?}");
}

// No spec statement: L-41, the acceptance criterion: a doorway is carved between two rooms
// using only the places the brush tools answered, with no map.brushes call, and the map is
// sealed: a tool that adds a brush answers its place, one that hollows answers the walls it
// made, one that cuts answers the pieces, each with the entity's count, and one that only
// removes a brush answers none
#[test]
fn two_rooms_are_joined_using_the_places_the_brush_tools_answer() {
    use qs_map_edit::{
        BrushIndex, BrushesEdited, MAP_BRUSH_CREATE, MAP_BRUSH_DELETE, MAP_BRUSH_HOLLOW,
        MAP_BRUSH_MOVE, MAP_BRUSH_SUBTRACT, MAP_PLACE,
    };

    let dir = std::env::temp_dir().join(format!("qs-wb-answered-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("answered.map");
    std::fs::write(&path, "{\n\"classname\" \"worldspawn\"\n}\n").unwrap();
    let mut session = open(&path);
    let tool = |session: &mut Commands<Context>, id: &str, params: serde_json::Value| {
        let edited: BrushesEdited =
            serde_json::from_value(session.execute(id, params).unwrap()).unwrap();
        edited
    };
    let places =
        |range: std::ops::Range<usize>| -> Vec<BrushIndex> { range.map(BrushIndex::new).collect() };
    // A box, hollowed into a room at the place its tool answered.
    let room = |session: &mut Commands<Context>, min: [i32; 3], max: [i32; 3]| {
        let made = tool(
            session,
            MAP_BRUSH_CREATE,
            json!({ "min": min, "max": max, "texture": "wall" }),
        );
        assert_eq!(made.brushes.len(), 1, "{made:?}");
        tool(
            session,
            MAP_BRUSH_HOLLOW,
            json!({ "entity": 0, "brush": made.brushes[0], "wall": 16 }),
        )
    };
    let west = room(&mut session, [-256, -256, 0], [256, 256, 224]);
    assert_eq!((west.total, &west.brushes), (6, &places(0..6)));
    let east = room(&mut session, [240, -256, 0], [752, 256, 224]);
    assert_eq!((east.total, &east.brushes), (12, &places(6..12)));

    let cutter = tool(
        &mut session,
        MAP_BRUSH_CREATE,
        json!({ "min": [224, -32, 16], "max": [272, 32, 128], "texture": "wall" }),
    );
    assert_eq!((cutter.total, &cutter.brushes), (13, &places(12..13)));
    let cut = tool(
        &mut session,
        MAP_BRUSH_SUBTRACT,
        json!({ "entity": 0, "brush": cutter.brushes[0] }),
    );
    // The two walls the doorway goes through are in pieces, the cutter is gone, and the
    // ten walls it did not meet are not answered.
    assert!(cut.brushes.len() > 2, "{cut:?}");
    assert_eq!(cut.total, 10 + cut.brushes.len());

    // A brush moved is answered at its place; one removed answers none.
    let block = tool(
        &mut session,
        MAP_BRUSH_CREATE,
        json!({ "min": [-64, -64, 16], "max": [-32, -32, 48], "texture": "wall" }),
    );
    let moved = tool(
        &mut session,
        MAP_BRUSH_MOVE,
        json!({ "entity": 0, "brushes": block.brushes, "by": [16, 0, 0] }),
    );
    assert_eq!((moved.total, &moved.brushes), (block.total, &block.brushes));
    let gone = tool(
        &mut session,
        MAP_BRUSH_DELETE,
        json!({ "entity": 0, "brush": block.brushes[0] }),
    );
    assert_eq!((gone.total, gone.brushes.len()), (cut.total, 0));

    session
        .execute(
            MAP_PLACE,
            json!({ "classname": "info_player_start", "origin": [0, 0, 40], "keys": [] }),
        )
        .unwrap();
    let leaks = report(&mut session, MAP_LEAKS);
    assert_eq!(leaks.seal, Seal::Sealed, "{leaks:?}");
}

// No spec statement: L-41: the status of a map played names its light warnings and what
// stands in the dark together, counts one as one, says a paused game is paused, and is
// none when there is nothing to say
#[test]
fn the_played_status_says_all_it_found() {
    use super::{PlaceLight, played_status};
    use crate::preview::GameStart;

    let dark = |classname: &str| PlaceLight {
        classname: classname.to_owned(),
        origin: [0.0; 3],
        light: 3,
    };
    assert_eq!(played_status("room", GameStart::Running, &[], &[]), None);
    assert_eq!(
        played_status(
            "room",
            GameStart::Paused,
            &["a light in a wall".to_owned(), "another".to_owned()],
            &[dark("monster_army")],
        )
        .as_deref(),
        Some(
            "Paused in room: 2 light warnings, the first a light in a wall; 1 entity stands \
             in the dark, the first a monster_army"
        )
    );
    assert_eq!(
        played_status(
            "room",
            GameStart::Running,
            &[],
            &[dark("item_health"), dark("monster_dog")],
        )
        .as_deref(),
        Some("Playing room: 2 entities stand in the dark, the first a item_health")
    );
}

// spec ENGT-5.4: the compile report counts each lump of the BSP it wrote, and names BSP29
#[test]
fn the_report_counts_each_lump() {
    let path = room("counted", "wall", Opening::Sealed);
    let mut session = open(&path);
    let found = report(&mut session, MAP_COMPILE);
    assert_eq!(found.format, qs_map_edit::Written::Bsp29);
    let bytes = std::fs::read(path.with_extension("bsp")).unwrap();
    let bsp = qs_bsp::Bsp::read(&bytes, qs_bsp::BspLoader::FitzQuake).unwrap();
    let lumps = found.lumps;
    assert_eq!(lumps.models, bsp.models.len());
    assert_eq!(lumps.planes, bsp.planes.len());
    assert_eq!(lumps.faces, bsp.faces.len());
    assert_eq!(lumps.clipnodes, bsp.clipnodes.len());
    assert_eq!(lumps.leafs, bsp.leafs.len());
    assert_eq!(lumps.vertexes, bsp.vertexes.len());
    assert_eq!(lumps.marksurfaces, bsp.marksurfaces.len());
    assert_eq!(lumps.edges, bsp.edges.len());
    assert_eq!(lumps.entity_text, bsp.entity_lump.len());
    assert_eq!(lumps.entities, 2);
    let largest = bsp
        .faces
        .iter()
        .filter(|f| bsp.texinfo[f.texinfo].flags & qs_bsp::BSP_TEX_SPECIAL == 0)
        .flat_map(|f| f.extents)
        .max()
        .unwrap();
    assert_eq!(lumps.fitz_face_extent, usize::try_from(largest).unwrap());
    assert!(lumps.face_extent > 0);
    // A compile does not light or vis the map.
    assert_eq!((lumps.lighting, lumps.visibility), (0, 0));
}
