// SPDX-License-Identifier: GPL-3.0-or-later

//! Play from here (P6-12, MAP-13): a new map built from brushes, compiled fast (the BSP with no
//! vis, lit in light's fast mode), played in `qs-sim` with the player put at the editor's
//! camera, who walks from one room into the other.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::rc::Rc;

use qc_progs::Progs;
use qs_bsp::{Bsp, BspLoader};
use qs_map::{Brush, Item, MapFile, Surface, TexDef};
use qs_map_light::{Light, Sampling};
use qs_sim::{LoadOptions, UserCmd, World};
use qs_wad::write::WadBuilder;
use qs_workbench::GameStart;
use qs_workbench::map_compile::PlayFromHereParams;

use crate::shape_stairs::{EYE, FRAME, progs};

/// Two rooms of 256 units, west and east of x = 0, joined by a doorway in the wall between
/// them; the player start and a light in the west one.
pub(crate) fn map() -> Vec<u8> {
    let mut file = MapFile::parse(
        b"{\n\"classname\" \"worldspawn\"\n}\n\
          {\n\"classname\" \"info_player_start\"\n\"origin\" \"-200 0 24\"\n}\n\
          {\n\"classname\" \"light\"\n\"origin\" \"-128 0 96\"\n}\n",
    )
    .unwrap();
    let surface = Surface {
        texture: b"wall".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        },
    };
    let (w, d, h) = (272.0, 128.0, 128.0);
    let boxes = [
        // Floor, ceiling and the four outer walls.
        ([-w, -d - 16.0, -16.0], [w, d + 16.0, 0.0]),
        ([-w, -d - 16.0, h], [w, d + 16.0, h + 16.0]),
        ([-w, -d - 16.0, 0.0], [-w + 16.0, d + 16.0, h]),
        ([w - 16.0, -d - 16.0, 0.0], [w, d + 16.0, h]),
        ([-w, -d - 16.0, 0.0], [w, -d, h]),
        ([-w, d, 0.0], [w, d + 16.0, h]),
        // The wall between the rooms, either side of a doorway 64 wide and 96 high.
        ([-8.0, -d, 0.0], [8.0, -32.0, h]),
        ([-8.0, 32.0, 0.0], [8.0, d, h]),
        ([-8.0, -32.0, 96.0], [8.0, 32.0, h]),
    ];
    file.entities[0].items.extend(
        boxes
            .iter()
            .map(|&(min, max)| Item::Brush(Brush::cuboid(min, max, &surface).unwrap())),
    );
    file.to_bytes()
}

// No spec statement: P6-12: a map built from brushes compiles fast, with no vis and fast light;
// the player put at the camera in one room stands there and walks through the doorway into the
// other
#[test]
fn a_new_map_plays_from_the_camera() {
    let compiled = qs_map_bsp::Compiler::default()
        .compile(&map(), None)
        .unwrap();
    let lit = Light::new(Sampling::Fast).run(&compiled.bsp).unwrap();
    let bytes: &'static [u8] = Box::leak(lit.bsp.into_boxed_slice());
    let bsp = Bsp::read(bytes, BspLoader::default()).unwrap();
    assert!(bsp.visibility.is_empty(), "no vis");
    assert!(!bsp.lighting.is_empty(), "lit");
    let progs_bytes = progs();
    let progs = Progs::read(&progs_bytes).unwrap();
    let mut world = World::load(
        &progs,
        "here",
        &bsp,
        Rc::new(|_: &str| None),
        LoadOptions::default(),
    )
    .unwrap();
    world.settle(&progs).unwrap();
    world.connect(&progs).unwrap();
    world.frame(&progs, FRAME).unwrap();
    // The camera: in the west room, away from the player start, facing east.
    let camera = [-96.0, 40.0, 24.0];
    world.move_player(&progs, camera, [0.0; 3]).unwrap();
    world.frame(&progs, FRAME).unwrap();
    let (eye, _) = world.player_view().unwrap();
    assert!(
        (eye[0] - camera[0]).abs() < 1.0 && (eye[1] - camera[1]).abs() < 1.0,
        "the player stands at the camera: {eye:?}"
    );
    assert!(eye[2] > EYE, "on the floor or above: {eye:?}");
    // East and a little south, through the doorway at y from -32 to 32.
    world.set_command(UserCmd {
        angles: [0.0, -15.0, 0.0],
        forward: 320.0,
        ..UserCmd::default()
    });
    let mut through = None;
    for _ in 0..216 {
        world.frame(&progs, FRAME).unwrap();
        let (eye, _) = world.player_view().unwrap();
        if eye[0] > 64.0 {
            through = Some(eye);
            break;
        }
    }
    assert!(through.is_some(), "walked into the east room");
}

// No spec statement: L-41, the acceptance criterion: a map with a lit room and an unlit room
// is reported with the unlit room dark: what stands in the west room, by its light, is not
// listed; what stands in the east room out of the light's sight through the doorway is, with
// its light; the light entity itself is passed over
#[test]
fn what_stands_in_an_unlit_room_is_reported_dark() {
    use qs_workbench::map_compile::PlaceLight;

    let mut text = map();
    text.extend_from_slice(
        b"{\n\"classname\" \"item_health\"\n\"origin\" \"-96 40 24\"\n}\n\
          {\n\"classname\" \"monster_army\"\n\"origin\" \"200 100 24\"\n}\n",
    );
    let compiled = qs_map_bsp::Compiler::default()
        .compile(&text, None)
        .unwrap();
    let lit = Light::new(Sampling::Fast).run(&compiled.bsp).unwrap();
    let bsp = Bsp::read(&lit.bsp, BspLoader::default()).unwrap();
    let dark = PlaceLight::dark_in(&bsp);
    assert_eq!(dark.len(), 1, "{dark:?}");
    assert_eq!(dark[0].classname, "monster_army");
    assert_eq!(dark[0].origin, [200.0, 100.0, 24.0]);
    assert!(dark[0].light < 24, "{dark:?}");
    // The west room is lit where the player start and the item stand.
    assert!(PlaceLight::at(&bsp, [-200.0, 0.0, 24.0]) >= 24);
    assert!(PlaceLight::at(&bsp, [-96.0, 40.0, 24.0]) >= 24);
    assert_eq!(PlaceLight::at(&bsp, [200.0, 100.0, 24.0]), dark[0].light);
}

// spec REND-1.14: a new map whose textures no WAD holds is drawn: each face of the map
// qs-map-bsp compiles from brushes takes the stand-in for a missing texture, so the world has
// vertices to draw (an empty world was the preview host's panic, #926)
#[test]
fn a_map_with_no_wad_draws_with_the_stand_in() {
    let compiled = qs_map_bsp::Compiler::default()
        .compile(&map(), None)
        .unwrap();
    let lit = Light::new(Sampling::Fast).run(&compiled.bsp).unwrap();
    let bsp = Bsp::read(&lit.bsp, BspLoader::default()).unwrap();
    assert!(
        bsp.textures.iter().all(Option::is_none),
        "no texture has data"
    );
    let colours: Vec<u8> = (0..=255_u8).flat_map(|i| [i, i, i]).collect();
    let palette = qs_render::Palette::read(&colours).unwrap();
    let mut builder = qs_render::MeshBuilder::new(&palette, qs_render::Look::QuakeSpasm);
    builder.add(&bsp, &[qs_render::Placement::WORLD]).unwrap();
    let mesh = builder.finish();
    let stand_in = mesh.textures.len() - 1;
    assert!(!mesh.vertices.is_empty() && !mesh.indices.is_empty());
    assert!(!mesh.batches.is_empty());
    assert!(mesh.batches.iter().all(|batch| batch.texture == stand_in));
    let image = &mesh.textures[stand_in];
    assert_eq!((image.width, image.height), (32, 32));
    assert_eq!(
        &image.rgba[..4],
        &[159, 91, 83, 255],
        "pink at the top left"
    );
}

// No spec statement: MAP-13: the player's origin goes view_ofs below the camera, moved up to the
// nearest height its box fits when the camera is near the floor; a camera inside a wall, or not a
// point, has no room
#[test]
fn the_player_is_put_where_its_box_fits() {
    let compiled = qs_map_bsp::Compiler::default()
        .compile(&map(), None)
        .unwrap();
    let bsp = Bsp::read(&compiled.bsp, BspLoader::default()).unwrap();
    let at = |eye: [f64; 3]| {
        PlayFromHereParams {
            eye,
            angles: None,
            region: None,
            start: GameStart::default(),
        }
        .origin_in(&bsp)
    };
    assert_eq!(at([-96.0, 40.0, 80.0]).unwrap(), [-96.0, 40.0, 58.0]);
    let low = at([-96.0, 40.0, 30.0]).unwrap();
    assert_eq!([low[0], low[1]], [-96.0, 40.0]);
    assert!(
        (24.0..=25.0).contains(&low[2]),
        "lifted onto the floor: {low:?}"
    );
    assert!(
        at([0.0, 80.0, 80.0]).is_err(),
        "inside the wall between the rooms"
    );
    assert!(at([f64::NAN, 0.0, 80.0]).is_err());
}

// spec REND-6.1, LIGHT-6.5: a map lit in full mode with a coloured light writes a .lit the
// preview host's loader reads for the lit BSP, and the map's lightmaps are drawn in its colour
#[test]
fn a_coloured_light_lights_the_preview_in_colour() {
    let mut file = MapFile::parse(&map()).unwrap();
    file.entities[2].set(b"_color", b"1 0 0").unwrap();
    // REND-1.14: a face with no texture has no lightmap, so the walls take one from a WAD.
    let mut texture = b"wall".to_vec();
    texture.resize(16, 0);
    for value in [16_u32, 16, 40, 40 + 256, 40 + 256 + 64, 40 + 256 + 64 + 16] {
        texture.extend_from_slice(&value.to_le_bytes());
    }
    texture.resize(40 + 256 + 64 + 16 + 4, 1);
    let mut wad = WadBuilder::new();
    wad.add(b"wall", qs_wad::WAD_TYP_MIPTEX, texture).unwrap();
    let compiled = qs_map_bsp::Compiler::default()
        .compile(&file.to_bytes(), Some(&wad.to_bytes().unwrap()))
        .unwrap();
    let lit = Light::new(Sampling::Extra).run(&compiled.bsp).unwrap();
    let bsp = Bsp::read(&lit.bsp, BspLoader::default()).unwrap();
    let lit_file = lit.lit.unwrap();
    let colour = qs_bsp::Lit::read(&lit_file, bsp.lighting.len()).unwrap();
    let colours: Vec<u8> = (0..=255_u8).flat_map(|i| [i, i, i]).collect();
    let palette = qs_render::Palette::read(&colours).unwrap();
    let mut builder = qs_render::MeshBuilder::new(&palette, qs_render::Look::GlQuake);
    let extras = qs_render::Extras {
        lit: Some(&colour),
        ..qs_render::Extras::default()
    };
    builder
        .add_with(&bsp, &extras, &[qs_render::Placement::WORLD])
        .unwrap();
    let lightmap = builder.finish().lightmap.rgba;
    // The atlas's first texel is its white one; the faces' lightmaps start after it.
    let texels: Vec<&[u8]> = lightmap.chunks(4).skip(1).collect();
    assert!(texels.iter().any(|t| t[0] > 0), "the room is lit");
    assert!(
        texels.iter().all(|t| t[1] == 0 && t[2] == 0),
        "and only in red"
    );
}
