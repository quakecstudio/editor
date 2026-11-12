// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use qs_workbench::map_edit::EntityIndex;
use qs_workbench::{Config, FsLoader, Workbench};

use super::{ClassLook, LinkView, Scene, start_camera};

/// The keep fixture's map, open in a workbench.
fn keep() -> Workbench<FsLoader> {
    let mut workbench = Workbench::new(FsLoader, Config::default());
    let map = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/keep/maps/keep.map");
    workbench.open_map(&map).unwrap();
    workbench
}

// No spec statement: P5-06: the world's six brushes give 36 faces of four edges; a point
// entity's box is its class's around its origin, else 8 units each way; a brush entity's is
// its brushes'
#[test]
fn the_scene_holds_the_brushes_and_the_entities_boxes() {
    let workbench = keep();
    let document = workbench.map().unwrap();
    let classes = HashMap::from([(
        "monster_ogre".to_owned(),
        ClassLook {
            size: Some([[-32.0, -32.0, -24.0], [32.0, 32.0, 64.0]]),
            ..ClassLook::default()
        },
    )]);
    let scene = Scene::build(document, &classes, &[]);
    // The world's six boxes and the door's one, six faces of four edges each.
    assert_eq!(scene.lines.len(), 7 * 6 * 4);
    // The door's brush, six faces of four edges, is drawn for the door, so hiding it hides them.
    let door = scene
        .lines
        .iter()
        .filter(|(owner, _)| *owner == EntityIndex::new(4))
        .count();
    assert_eq!(door, 6 * 4);
    let shape = |index: usize| {
        scene
            .entities
            .iter()
            .find(|s| s.index == EntityIndex::new(index))
            .unwrap()
    };
    assert_eq!(shape(2).bounds, [[64.0, 32.0, 0.0], [128.0, 96.0, 88.0]]);
    assert!((shape(2).yaw - 180.0).abs() < f32::EPSILON);
    assert_eq!(shape(3).bounds, [[-8.0, -8.0, 184.0], [8.0, 8.0, 200.0]]);
    assert!(shape(4).brush);
    assert_eq!(shape(4).bounds, [[224.0, -64.0, 0.0], [240.0, 64.0, 128.0]]);
    assert_eq!(
        scene.entities.len(),
        4,
        "the world is not an entity to pick"
    );
}

// No spec statement: P5-06: a ray picks the nearest box it enters, and none when it misses
#[test]
fn a_ray_picks_the_nearest_box() {
    let workbench = keep();
    let scene = Scene::build(workbench.map().unwrap(), &HashMap::new(), &[]);
    // Along +x at the door's height: the ogre's 8-unit default box is off this line, the door
    // is on it.
    assert_eq!(
        scene.pick([0.0, 0.0, 64.0], [1.0, 0.0, 0.0], &BTreeSet::new()),
        Some(EntityIndex::new(4))
    );
    assert_eq!(
        scene.pick([0.0, 64.0, 24.0], [1.0, 0.0, 0.0], &BTreeSet::new()),
        Some(EntityIndex::new(2))
    );
    assert_eq!(
        scene.pick([0.0, 200.0, 24.0], [1.0, 0.0, 0.0], &BTreeSet::new()),
        None
    );
    assert_eq!(
        scene.pick([0.0, 0.0, 64.0], [-1.0, 0.0, 0.0], &BTreeSet::new()),
        None,
        "behind the ray"
    );
    // From inside the door's box, the ogre ahead is picked, and the door when nothing is.
    assert_eq!(
        scene.pick([232.0, 64.0, 24.0], [-1.0, 0.0, 0.0], &BTreeSet::new()),
        Some(EntityIndex::new(2))
    );
    assert_eq!(
        scene.pick([232.0, 0.0, 120.0], [0.0, 0.0, 1.0], &BTreeSet::new()),
        Some(EntityIndex::new(4))
    );
}

// No spec statement: P5-13, MAP-27, MAP-8.8: a key the code fires entities by links its entity
// to the entity whose targetname it holds; the view draws every link or those of the selection,
// and none to a hidden entity, which a ray does not pick either
#[test]
fn links_follow_the_keys_the_code_fires_by() {
    let workbench = keep();
    let targeting = ["target".to_owned()];
    let scene = Scene::build(workbench.map().unwrap(), &HashMap::new(), &targeting);
    let none = BTreeSet::new();
    let ogre_to_door = [[96.0, 64.0, 24.0], [232.0, 0.0, 64.0]];
    assert_eq!(scene.links(LinkView::All, &none, &none), [ogre_to_door]);
    assert!(scene.links(LinkView::Selection, &none, &none).is_empty());
    let door = BTreeSet::from([EntityIndex::new(4)]);
    assert_eq!(
        scene.links(LinkView::Selection, &door, &none),
        [ogre_to_door]
    );
    let light = BTreeSet::from([EntityIndex::new(3)]);
    assert!(scene.links(LinkView::Selection, &light, &none).is_empty());
    assert!(scene.links(LinkView::All, &none, &door).is_empty());
    assert_eq!(scene.pick([0.0, 0.0, 64.0], [1.0, 0.0, 0.0], &door), None);
    // A key the code does not fire by links nothing.
    let other = Scene::build(workbench.map().unwrap(), &HashMap::new(), &[]);
    assert!(other.links(LinkView::All, &none, &none).is_empty());
    assert_eq!(
        scene.values.get("angle"),
        Some(&BTreeSet::from(["0".to_owned(), "180".to_owned()]))
    );
}

/// The spec's `base` BSP vector, from its `xxd` dump.
fn base_bsp() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/vectors/bsp/base.txt");
    let mut out = Vec::new();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let Some((_, rest)) = line.split_once(": ") else {
            continue;
        };
        for group in rest.get(..39).unwrap_or(rest).split_whitespace() {
            for pair in group.as_bytes().chunks(2) {
                out.push(u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap());
            }
        }
    }
    out
}

// No spec statement: P5-06, BUILTIN-10.3, BSP-15.4: a brush entity read from a BSP, which names a
// submodel and has no brushes, is a brush entity boxed by its submodel
#[test]
fn a_bsp_brush_entity_is_boxed_by_its_submodel() {
    let dir = std::env::temp_dir().join(format!("qs-map-scene-bsp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bsp = dir.join("base.bsp");
    std::fs::write(&bsp, base_bsp()).unwrap();
    std::fs::write(
        dir.join("base.ent"),
        "{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"func_door\"\n\"model\" \"*1\"\n}\n",
    )
    .unwrap();
    let mut workbench = Workbench::new(FsLoader, Config::default());
    workbench.open_map(&bsp).unwrap();
    let scene = Scene::build(workbench.map().unwrap(), &HashMap::new(), &[]);
    let door = scene.entities.first().unwrap();
    assert!(door.brush);
    // Submodel 1's (0, 0, 0) to (64, 64, 8), widened by 1 as the engine loads it (BSP-15.4).
    assert_eq!(door.bounds, [[-1.0, -1.0, -1.0], [65.0, 65.0, 9.0]]);
}

// No spec statement: P5-06: the camera starts behind and above the player start, facing as it
// does
#[test]
fn the_camera_starts_behind_the_player_start() {
    let workbench = keep();
    let camera = start_camera(workbench.map().unwrap());
    assert_eq!(camera.origin, [-320.0, 0.0, 88.0]);
    assert!(camera.yaw.abs() < f32::EPSILON);
    assert!(camera.pitch > 0.0, "looking down at it");
}
