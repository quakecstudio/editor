// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test, and the sizes are small"
)]

use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use qs_wad::WAD_TYP_MIPTEX;
use qs_wad::texture::Palette;
use qs_wad::write::WadBuilder;
use serde_json::json;

use super::{MAP_TEXTURES, MapTextures, TextureOrigin, TextureSources, WadState};
use crate::map_edit::{MAP_OPEN, MAP_SET_KEY};
use crate::{Config, OPEN_PROJECT, Workbench, WorkbenchError, register};

/// A texture lump `name`, `width` by `height`, every pixel `index`.
fn miptex(name: &str, width: u32, height: u32, index: u8) -> Vec<u8> {
    let mut lump = name.as_bytes().to_vec();
    lump.resize(16, 0);
    let area = width * height;
    let mut offset = 40_u32;
    lump.extend_from_slice(&width.to_le_bytes());
    lump.extend_from_slice(&height.to_le_bytes());
    for level in 0..4 {
        lump.extend_from_slice(&offset.to_le_bytes());
        offset += area >> (2 * level);
    }
    lump.resize(lump.len() + (offset - 40) as usize, index);
    lump
}

/// A WAD2 file of `textures`, each a name, size and the index all its pixels are.
fn wad(textures: &[(&str, u32, u32, u8)]) -> Vec<u8> {
    let mut builder = WadBuilder::new();
    for &(name, width, height, index) in textures {
        builder
            .add(
                name.as_bytes(),
                WAD_TYP_MIPTEX,
                miptex(name, width, height, index),
            )
            .unwrap();
    }
    builder.to_bytes().unwrap()
}

/// A palette whose index `i` is the colour `(i, 255 - i, 7)`.
fn palette_bytes() -> Vec<u8> {
    (0..=255_u8).flat_map(|i| [i, 255 - i, 7]).collect()
}

/// A brush of six faces, `textures` on them in order and the first on the rest.
fn brush(textures: &[&str]) -> String {
    let planes = [
        "( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 )",
        "( 64 0 0 ) ( 64 0 1 ) ( 64 1 0 )",
        "( 0 0 0 ) ( 0 0 1 ) ( 1 0 0 )",
        "( 0 64 0 ) ( 1 64 0 ) ( 0 64 1 )",
        "( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 )",
        "( 0 0 64 ) ( 0 1 64 ) ( 1 0 64 )",
    ];
    let mut out = String::from("{\n");
    for (i, plane) in planes.iter().enumerate() {
        let texture = textures.get(i).unwrap_or(&textures[0]);
        out.push_str(&format!("{plane} {texture} 0 0 0 1 1\n"));
    }
    out.push_str("}\n");
    out
}

/// Where the test puts the WAD the world names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WadPlace {
    Project,
    BesideMap,
    Game,
    Nowhere,
}

/// What a test's files hold; the default is a project with `gfx/a.wad` of `brick` and
/// `metal`, a map using `brick` on five faces and `nope` on one, and a game with a palette.
struct Scene {
    wad_key: &'static str,
    place: WadPlace,
    palette: bool,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            wad_key: "gfx/a.wad",
            place: WadPlace::Project,
            palette: true,
        }
    }
}

/// The project's root, the game's base directory and a session with the map open.
fn opened(test: &str, scene: &Scene) -> (PathBuf, PathBuf, Session<Workbench<FsLoader>>) {
    let dir = std::env::temp_dir().join(format!("qs-wb-tex-{test}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    let project = dir.join("project");
    let game = dir.join("game");
    std::fs::create_dir_all(project.join("maps")).unwrap();
    std::fs::create_dir_all(game.join("id1/gfx")).unwrap();
    std::fs::write(project.join("progs.src"), "progs.dat\n").unwrap();
    if scene.palette {
        std::fs::write(game.join("id1/gfx/palette.lmp"), palette_bytes()).unwrap();
    }
    let bytes = wad(&[("brick", 16, 16, 3), ("metal", 32, 16, 9)]);
    let wad_path = match scene.place {
        WadPlace::Project => Some(project.join(scene.wad_key)),
        WadPlace::BesideMap => Some(project.join("maps").join(scene.wad_key)),
        WadPlace::Game => Some(game.join("id1").join(scene.wad_key)),
        WadPlace::Nowhere => None,
    };
    if let Some(path) = wad_path {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let map = format!(
        "{{\n\"classname\" \"worldspawn\"\n\"wad\" \"{}\"\n{}}}\n",
        scene.wad_key,
        brush(&["brick", "brick", "nope"])
    );
    std::fs::write(project.join("maps/test.map"), map).unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute(OPEN_PROJECT, json!({ "path": project }))
        .unwrap();
    session
        .execute(MAP_OPEN, json!({ "path": project.join("maps/test.map") }))
        .unwrap();
    (project, game, session)
}

fn sources(project: &Path, game: &Path, session: &Session<Workbench<FsLoader>>) -> TextureSources {
    let workbench = session.context();
    TextureSources::load(workbench.map().unwrap(), Some(project), Some(game), None)
}

fn listing(scene: &Scene, test: &str) -> (TextureSources, MapTextures) {
    let (project, game, session) = opened(test, scene);
    let sources = sources(&project, &game, &session);
    let listing = sources.listing(session.context().map().unwrap());
    (sources, listing)
}

#[test]
fn every_texture_of_the_wad_and_the_map_is_listed_with_its_uses() {
    let (_, listing) = listing(&Scene::default(), "list");
    let rows: Vec<(&str, &TextureOrigin, u32, u32, usize)> = listing
        .textures
        .iter()
        .map(|t| (t.name.as_str(), &t.origin, t.width, t.height, t.uses))
        .collect();
    let wad = TextureOrigin::Wad {
        wad: "gfx/a.wad".to_owned(),
    };
    assert_eq!(
        rows,
        [
            ("brick", &wad, 16, 16, 5),
            ("metal", &wad, 32, 16, 0),
            ("nope", &TextureOrigin::Missing, 0, 0, 1),
        ]
    );
    assert_eq!(listing.wads.len(), 1);
    assert_eq!(listing.wads[0].state, WadState::Read { textures: 2 });
}

#[test]
fn a_wad_is_found_in_the_project_beside_the_map_or_in_the_game() {
    for place in [WadPlace::Project, WadPlace::BesideMap, WadPlace::Game] {
        let scene = Scene {
            place,
            ..Scene::default()
        };
        let (sources, listing) = listing(&scene, &format!("{place:?}"));
        assert_eq!(
            listing.wads[0].state,
            WadState::Read { textures: 2 },
            "{place:?}"
        );
        let watched = sources.watched();
        assert_eq!(watched.len(), 1, "{place:?}");
        assert!(watched[0].ends_with("gfx/a.wad"), "{place:?}: {watched:?}");
    }
}

#[test]
fn a_wad_that_is_nowhere_leaves_the_map_s_names_missing() {
    let scene = Scene {
        place: WadPlace::Nowhere,
        ..Scene::default()
    };
    let (sources, listing) = listing(&scene, "nowhere");
    assert_eq!(listing.wads[0].state, WadState::Missing);
    assert!(sources.watched().is_empty());
    assert!(
        listing
            .textures
            .iter()
            .all(|t| t.origin == TextureOrigin::Missing)
    );
    assert_eq!(listing.textures.len(), 2);
}

#[test]
fn the_first_wad_holding_a_name_wins() {
    let sources = TextureSources::from_files(
        vec![
            ("a.wad".to_owned(), wad(&[("brick", 16, 16, 1)])),
            (
                "b.wad".to_owned(),
                wad(&[("brick", 32, 32, 2), ("sky1", 16, 16, 3)]),
            ),
            ("bad.wad".to_owned(), b"not a wad".to_vec()),
        ],
        None,
        None,
    );
    let (_, _, session) = opened("first", &Scene::default());
    let listing = sources.listing(session.context().map().unwrap());
    let names: Vec<(&str, u32)> = listing
        .textures
        .iter()
        .map(|t| (t.name.as_str(), t.width))
        .collect();
    assert_eq!(names, [("brick", 16), ("sky1", 16), ("nope", 0)]);
    assert_eq!(listing.wads[2].state, WadState::Unreadable);
}

#[test]
fn a_texture_decodes_in_the_game_palette() {
    let (sources, _) = listing(&Scene::default(), "decode");
    assert!(sources.has_palette());
    let decoded = sources.decode("brick").unwrap();
    assert_eq!((decoded.width, decoded.height), (16, 16));
    assert_eq!(&decoded.rgba[..4], [3, 252, 7, 255]);
    assert!(sources.decode("nope").is_none());
    let decoded = sources.decode("METAL").unwrap();
    assert_eq!(&decoded.rgba[..4], [9, 246, 7, 255]);
}

#[test]
fn with_no_palette_nothing_is_decoded() {
    let scene = Scene {
        palette: false,
        ..Scene::default()
    };
    let (sources, listing) = listing(&scene, "nopalette");
    assert!(!sources.has_palette());
    assert!(sources.decode("brick").is_none());
    assert_eq!(listing.textures.len(), 3);
    let with = TextureSources::from_files(
        vec![("a.wad".to_owned(), wad(&[("brick", 16, 16, 1)]))],
        None,
        Some(Palette::from_lmp(&palette_bytes()).unwrap()),
    );
    assert!(with.decode("brick").is_some());
}

#[test]
fn map_textures_answers_the_listing() {
    let (_, _, mut session) = opened("command", &Scene::default());
    let answer: MapTextures =
        serde_json::from_value(session.execute(MAP_TEXTURES, json!({})).unwrap()).unwrap();
    // The command line's context has no game, so the WAD comes from the project.
    assert_eq!(answer.textures.len(), 3);
    assert_eq!(answer.textures[0].uses, 5);
}

#[test]
fn map_textures_needs_an_open_map() {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    let error = session.execute(MAP_TEXTURES, json!({})).unwrap_err();
    assert!(
        error
            .to_string()
            .contains(&WorkbenchError::NoMap.to_string()),
        "{error}"
    );
}

#[test]
fn the_project_s_palette_comes_before_the_game_s() {
    let scene = Scene {
        palette: false,
        ..Scene::default()
    };
    let (project, game, session) = opened("projectpalette", &scene);
    let own: Vec<u8> = (0..=255_u8).flat_map(|i| [255 - i, i, 1]).collect();
    std::fs::create_dir_all(project.join("gfx")).unwrap();
    std::fs::write(project.join("gfx/palette.lmp"), own).unwrap();
    std::fs::write(game.join("id1/gfx/palette.lmp"), palette_bytes()).unwrap();
    let sources = sources(&project, &game, &session);
    assert_eq!(
        &sources.decode("brick").unwrap().rgba[..4],
        [252, 3, 1, 255]
    );
}

/// A WAD3 file of one palette texture `name`, 16 by 16, every pixel index 1, whose own palette
/// makes index 1 `colour`.
fn wad3(name: &str, colour: [u8; 3]) -> Vec<u8> {
    let mut lump = miptex(name, 16, 16, 1);
    lump.extend_from_slice(&2_u16.to_le_bytes());
    lump.extend_from_slice(&[0, 0, 0]);
    lump.extend_from_slice(&colour);
    let mut out = b"WAD3".to_vec();
    out.extend_from_slice(&1_i32.to_le_bytes());
    out.extend_from_slice(&i32::try_from(12 + lump.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&lump);
    out.extend_from_slice(&12_i32.to_le_bytes());
    out.extend_from_slice(&i32::try_from(lump.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&i32::try_from(lump.len()).unwrap().to_le_bytes());
    out.push(qs_wad::WAD_TYP_MIPTEX_PALETTE);
    out.extend_from_slice(&[0, 0, 0]);
    let mut field = name.as_bytes().to_vec();
    field.resize(16, 0);
    out.extend_from_slice(&field);
    out
}

// spec WAD-8.4: a WAD3 texture is shown in its own palette
#[test]
fn a_wad3_texture_is_read_and_shown_in_its_own_palette() {
    let sources = TextureSources::from_files(
        vec![("hl.wad".to_owned(), wad3("crate", [10, 20, 30]))],
        None,
        Some(Palette::from_lmp(&palette_bytes()).unwrap()),
    );
    let (_, _, session) = opened("wad3", &Scene::default());
    let listing = sources.listing(session.context().map().unwrap());
    assert_eq!(listing.wads[0].state, WadState::Read { textures: 1 });
    assert_eq!(
        &sources.decode("crate").unwrap().rgba[..4],
        [10, 20, 30, 255]
    );
}

#[test]
fn an_absolute_wad_path_from_another_machine_is_found_by_its_name() {
    let scene = Scene {
        wad_key: "C:/quake/id1/gfx/a.wad",
        place: WadPlace::Nowhere,
        ..Scene::default()
    };
    let (project, game, session) = opened("absolute", &scene);
    std::fs::write(project.join("a.wad"), wad(&[("brick", 16, 16, 3)])).unwrap();
    let listing = sources(&project, &game, &session).listing(session.context().map().unwrap());
    assert_eq!(listing.wads[0].state, WadState::Read { textures: 1 });
}

#[test]
fn a_file_that_is_no_wad_is_unreadable_not_missing() {
    let scene = Scene {
        place: WadPlace::Nowhere,
        ..Scene::default()
    };
    let (project, game, session) = opened("unreadable", &scene);
    std::fs::create_dir_all(project.join("gfx")).unwrap();
    std::fs::write(project.join("gfx/a.wad"), b"not a wad").unwrap();
    let sources = sources(&project, &game, &session);
    let listing = sources.listing(session.context().map().unwrap());
    assert_eq!(listing.wads[0].state, WadState::Unreadable);
    assert_eq!(sources.watched().len(), 1);
}

#[test]
fn the_sources_are_stale_when_the_wad_key_or_the_game_changes() {
    let (project, game, mut session) = opened("stale", &Scene::default());
    let loaded = TextureSources::load(
        session.context().map().unwrap(),
        Some(&project),
        Some(&game),
        None,
    );
    let document = session.context().map().unwrap();
    assert!(!loaded.is_stale(document, None));
    assert!(loaded.is_stale(document, Some(&game)));
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 0, "key": "wad", "value": "other.wad" }),
        )
        .unwrap();
    assert!(loaded.is_stale(session.context().map().unwrap(), None));
}

// spec WAD-8.3: a texture past the size a GPU takes, or with fewer pixels than its size, is not
// decoded
#[test]
fn a_texture_that_cannot_be_shown_is_not_decoded() {
    let palette = Palette::from_lmp(&palette_bytes()).unwrap();
    assert!(palette.decode_pixels(b"a", 0, 16, &[]).is_err());
    assert!(
        palette
            .decode_pixels(b"a", 8192, 16, &vec![0; 8192 * 16])
            .is_err()
    );
    assert!(palette.decode_pixels(b"a", 16, 16, &[0; 255]).is_err());
    assert!(palette.decode_pixels(b"a", 16, 16, &[0; 256]).is_ok());
}
