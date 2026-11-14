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
