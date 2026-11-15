// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::collections::{BTreeSet, HashMap};

use qs_map::{Brush, Item, MapFile, Surface, TexDef};
use qs_map_edit::{BrushIndex, EntityIndex};
use qs_workbench::{Config, FsLoader, Workbench};

use super::{Picked, Region};
use crate::map_scene::Scene;
use crate::map_tools::BrushTools;

/// A scene of `file`, written to its own folder `name`.
fn scene_of(name: &str, file: &MapFile) -> Scene {
    let dir = std::env::temp_dir().join(format!("qs-select-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("select.map");
    std::fs::write(&path, file.to_bytes()).unwrap();
    let mut workbench = Workbench::new(FsLoader, Config::default());
    workbench.maps_mut().open_map(&path).unwrap();
    let document = workbench.map().unwrap();
    Scene::build(document, &HashMap::new(), &[], &mut |_| None)
}

fn rock() -> Surface {
    Surface {
        texture: b"rock".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0, 0.0],
            rotate: 0.0,
            scale: [1.0, 1.0],
        },
    }
}

/// A map whose world holds, in order: the selecting box 0 to 64; one overlapping it, 32 to 96;
/// one inside it, 16 to 48; one far off at 200; and one inside it across but high above,
/// z 500 to 564. Its entities: a light inside the box at 32 32 32 and one far off at 1000 0 0.
fn scene(name: &str) -> Scene {
    let mut file = MapFile::parse(
        b"{\n\"classname\" \"worldspawn\"\n}\n\
          {\n\"classname\" \"light\"\n\"origin\" \"32 32 32\"\n}\n\
          {\n\"classname\" \"light\"\n\"origin\" \"1000 0 0\"\n}\n",
    )
    .unwrap();
    let surface = Surface {
        texture: b"rock".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0, 0.0],
            rotate: 0.0,
            scale: [1.0, 1.0],
        },
    };
    for (min, max) in [
        ([0.0; 3], [64.0; 3]),
        ([32.0; 3], [96.0; 3]),
        ([16.0; 3], [48.0; 3]),
        ([200.0; 3], [264.0; 3]),
        ([16.0, 16.0, 500.0], [48.0, 48.0, 564.0]),
    ] {
        let brush = Brush::cuboid(min, max, &surface).unwrap();
        file.entities[0].items.push(Item::Brush(brush));
    }
    let dir = std::env::temp_dir().join(format!("qs-select-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("select.map");
    std::fs::write(&path, file.to_bytes()).unwrap();
    let mut workbench = Workbench::new(FsLoader, Config::default());
    workbench.maps_mut().open_map(&path).unwrap();
    let document = workbench.map().unwrap();
    Scene::build(document, &HashMap::new(), &[], &mut |_| None)
}

fn world(brush: usize) -> (EntityIndex, BrushIndex) {
    (EntityIndex::new(0), BrushIndex::new(brush))
}

// No spec statement: P7-08: TrenchBroom's Select Touching takes what the selecting brush touches,
// Select Inside what lies wholly in it, and Select Tall what lies in it stretched along the
// view's axis; the selecting brush itself is left out, and nothing hidden is taken
#[test]
fn a_brush_selects_what_it_touches_holds_or_stands_over() {
    let scene = scene("regions");
    let none = BTreeSet::new();
    let by = [world(0)];
    let touching = scene.picked_by(&by, Region::Touching, &none);
    assert_eq!(touching.brushes, [world(1), world(2)]);
    assert_eq!(touching.entities, [EntityIndex::new(1)]);
    let inside = scene.picked_by(&by, Region::Inside, &none);
    assert_eq!(inside.brushes, [world(2)]);
    assert_eq!(inside.entities, [EntityIndex::new(1)]);
    let tall = scene.picked_by(&by, Region::Tall(2), &none);
    assert_eq!(
        tall.brushes,
        [world(2), world(4)],
        "the high one, stretched along z"
    );
    let along_x = scene.picked_by(&by, Region::Tall(0), &none);
    assert_eq!(along_x.brushes, [world(2)], "not the high one along x");
    let hidden = BTreeSet::from([EntityIndex::new(1)]);
    assert!(
        scene
            .picked_by(&by, Region::Inside, &hidden)
            .entities
            .is_empty()
    );
    assert_eq!(
        scene.picked_by(&[], Region::Touching, &none),
        Picked::default()
    );
}

// No spec statement: P7-08: Select All takes every entity but the world and every world brush
// not hidden; Invert Selection takes what the selection leaves out; Select Siblings every
// brush of the entities holding the selected brushes
#[test]
fn all_inverted_and_siblings() {
    let scene = scene("all");
    let none = BTreeSet::new();
    let all = scene.all(&none);
    assert_eq!(all.entities, [EntityIndex::new(1), EntityIndex::new(2)]);
    assert_eq!(all.brushes.len(), 5);
    let some = Picked {
        entities: vec![EntityIndex::new(1)],
        brushes: vec![world(0), world(3)],
    };
    let inverted = scene.inverted(&some, &none);
    assert_eq!(inverted.entities, [EntityIndex::new(2)]);
    assert_eq!(inverted.brushes, [world(1), world(2), world(4)]);
    assert_eq!(scene.siblings(&[world(3)]).len(), 5);
    assert!(scene.siblings(&[]).is_empty());
    let hidden = BTreeSet::from([EntityIndex::new(0), EntityIndex::new(2)]);
    let shown = scene.all(&hidden);
    assert_eq!(shown.entities, [EntityIndex::new(1)]);
    assert!(shown.brushes.is_empty());
}

// No spec statement: P7-08: a ray down through the map meets the brushes under it nearest
// first; a 2D view's click picks the smallest of them across the view, as TrenchBroom picks
// the smallest visible area; Cmd and the wheel drill the selection along the ray, up away from
// the camera and down toward it, as its selection drilling
#[test]
fn a_ray_orders_drills_and_picks_the_smallest_across() {
    let scene = scene("drill");
    let none = BTreeSet::new();
    let down = ([40.0, 40.0, 1000.0], [0.0, 0.0, -1.0]);
    assert_eq!(
        scene.brushes_along(down.0, down.1, &none),
        [world(4), world(1), world(0), world(2)]
    );
    assert_eq!(
        scene.pick_across(down.0, down.1, 2, &none),
        Some(world(4)),
        "of the two 32 across, the nearer"
    );
    let hidden = BTreeSet::from([EntityIndex::new(0)]);
    assert!(scene.brushes_along(down.0, down.1, &hidden).is_empty());
    let mut tools = BrushTools::default();
    tools.choose(vec![world(1)], &scene);
    assert!(tools.drill(&scene, down, 1.0, &none));
    assert_eq!(tools.chosen(), [world(0)], "pushed away");
    assert!(tools.drill(&scene, down, -1.0, &none));
    assert_eq!(tools.chosen(), [world(1)], "pulled back");
    assert!(tools.drill(&scene, down, -1.0, &none));
    assert!(tools.drill(&scene, down, -1.0, &none));
    assert_eq!(tools.chosen(), [world(4)], "held at the nearest");
    tools.choose(vec![world(3)], &scene);
    assert!(
        !tools.drill(&scene, down, 1.0, &none),
        "not under the pointer"
    );
}

// No spec statement: P7-08: Select Touching leaves out brushes kept apart only along the cross
// of an edge of each, which no face plane parts: two tetrahedra whose edges cross 8 units
// apart do not touch, and touch once they meet
#[test]
fn edges_crossing_apart_do_not_touch() {
    for (gap, touching) in [(8.0, false), (0.0, true)] {
        let mut file = MapFile::parse(b"{\n\"classname\" \"worldspawn\"\n}\n").unwrap();
        let below = [
            [-32.0, 0.0, 32.0],
            [32.0, 0.0, 32.0],
            [0.0, -32.0, 0.0],
            [0.0, 32.0, 0.0],
        ];
        let above = [
            [0.0, -32.0, 32.0 + gap],
            [0.0, 32.0, 32.0 + gap],
            [-32.0, 0.0, 64.0 + gap],
            [32.0, 0.0, 64.0 + gap],
        ];
        for points in [below, above] {
            let brush = Brush::convex_hull(&points, &rock()).unwrap();
            file.entities[0].items.push(Item::Brush(brush));
        }
        let scene = scene_of(&format!("edges-{gap}"), &file);
        let picked = scene.picked_by(&[world(0)], Region::Touching, &BTreeSet::new());
        assert_eq!(picked.brushes == [world(1)], touching, "gap {gap}");
    }
}

// No spec statement: P7-08: a brush entity's own brush selects what it touches but not the
// entity that holds it
#[test]
fn a_brush_does_not_select_its_own_entity() {
    let mut file = MapFile::parse(
        b"{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"func_detail\"\n}\n\
          {\n\"classname\" \"light\"\n\"origin\" \"32 32 32\"\n}\n",
    )
    .unwrap();
    let brush = Brush::cuboid([0.0; 3], [64.0; 3], &rock()).unwrap();
    file.entities[1].items.push(Item::Brush(brush));
    let scene = scene_of("owner", &file);
    let by = [(EntityIndex::new(1), BrushIndex::new(0))];
    let picked = scene.picked_by(&by, Region::Touching, &BTreeSet::new());
    assert_eq!(
        picked.entities,
        [EntityIndex::new(2)],
        "the light, not the func_detail"
    );
}
