// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qs_bsp::{Bsp, BspLoader};
use qs_map::{Brush, Item, MapFile, Surface, TexDef};
use qs_map_bsp::{Compiled, Compiler, Dialect};

use super::{FullBuild, brush_models};

/// A sealed room 256 across with a player start, a light and a door, compiled as Play from here
/// compiles it, before its light.
pub(crate) fn compiled_room() -> Compiled {
    let mut file = MapFile::parse(
        b"{\n\"classname\" \"worldspawn\"\n}\n\
          {\n\"classname\" \"info_player_start\"\n\"origin\" \"-64 0 24\"\n}\n\
          {\n\"classname\" \"light\"\n\"origin\" \"0 0 96\"\n}\n\
          {\n\"classname\" \"func_door\"\n}\n",
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
    let (a, b) = (128.0, 144.0);
    let walls = [
        ([-b, -b, -b], [b, b, -a]),
        ([-b, -b, a], [b, b, b]),
        ([-b, -b, -a], [-a, b, a]),
        ([a, -b, -a], [b, b, a]),
        ([-a, -b, -a], [a, -a, a]),
        ([-a, a, -a], [a, b, a]),
    ];
    for (min, max) in walls {
        let brush = Brush::cuboid(min, max, &surface).unwrap();
        file.entities[0].items.push(Item::Brush(brush));
    }
    let door = Brush::cuboid([32.0, -16.0, -128.0], [48.0, 16.0, 0.0], &surface).unwrap();
    file.entities[3].items.push(Item::Brush(door));
    Compiler::default()
        .with_dialect(Dialect::Extended)
        .compile(&file.to_bytes(), None)
        .unwrap()
}

// No spec statement: P7-02: the full compile gives the fast compile's BSP full vis and full
// light, written where the played BSP is
#[test]
fn a_full_build_sees_and_lights_the_map() {
    let compiled = compiled_room();
    let dir = std::env::temp_dir().join(format!("qs-wb-full-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("room.bsp");
    let portals = compiled.portal_file.clone().unwrap();
    let build = FullBuild::start("room".to_owned(), target.clone(), compiled.bsp, portals);
    let lit = build.wait().unwrap();
    let bsp = Bsp::read(&lit.bsp, BspLoader::default()).unwrap();
    assert!(!bsp.visibility.is_empty(), "full vis");
    assert!(bsp.lighting.iter().any(|&b| b > 0), "full light");
    build.write(&lit).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), lit.bsp);
    assert!(
        !target.with_extension("lit").exists(),
        "white light writes no .lit"
    );
}

// No spec statement: P7-02: a full compile of a map with no portals to see through stops with
// vis's reason, which the preview says
#[test]
fn a_full_build_of_broken_portals_says_why() {
    let compiled = compiled_room();
    let build = FullBuild::start(
        "room".to_owned(),
        std::env::temp_dir().join("qs-wb-full-none.bsp"),
        compiled.bsp,
        b"not a portal file".to_vec(),
    );
    assert!(build.wait().is_err());
}

// spec BRIDGE-4.18: each brush entity of the new build, by its place in the entity lump, and
// its brush model, for map/swap
#[test]
fn brush_models_are_read_from_the_entity_lump() {
    let compiled = compiled_room();
    assert_eq!(brush_models(&compiled.bsp).unwrap(), [(3, "*1".to_owned())]);
    assert!(brush_models(b"not a bsp").is_err());
}
