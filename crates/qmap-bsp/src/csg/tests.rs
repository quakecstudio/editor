// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qs_map::qbsp::{QbspMap, load_map_file};

use super::csg_faces;
use crate::brush::BrushSet;
use crate::fixtures::{BrushSpec, MapSpec, map_text};
use crate::planes::Planes;
use crate::{Contents, Dialect, Face, Hull, ModelKind, Surface};

fn compile_csg(brushes: Vec<BrushSpec>) -> (QbspMap, Planes, Vec<Surface>) {
    let spec = MapSpec {
        world: brushes,
        entities: Vec::new(),
        ..MapSpec::default()
    };
    let map = load_map_file(map_text(&spec).as_bytes()).unwrap();
    let limits = Dialect::Id.limits();
    let mut planes = Planes::new(limits.planes);
    let set = BrushSet::load(
        &map.entities[0],
        &map,
        Hull::Draw,
        ModelKind::World,
        &mut planes,
        &mut Vec::new(),
        limits,
    )
    .unwrap();
    let surfaces = csg_faces(&set, &planes).unwrap();
    (map, planes, surfaces)
}

fn texture<'a>(map: &'a QbspMap, face: &Face) -> &'a [u8] {
    &map.miptex[map.texinfo[face.texinfo].miptex]
}

/// The faces on the plane x = `x` facing +x.
fn facing_x<'a>(planes: &Planes, surfaces: &'a [Surface], x: f32) -> Vec<&'a Face> {
    surfaces
        .iter()
        .flat_map(|s| &s.faces)
        .filter(|f| {
            let p = planes.facing(f.plane, f.side);
            p.normal == [1.0, 0.0, 0.0] && p.dist == x
        })
        .collect()
}

/// A big cube `a` and a smaller cube `b` sharing its +x face's plane.
fn sharing(order: [&str; 2]) -> Vec<BrushSpec> {
    order
        .into_iter()
        .map(|name| match name {
            "a" => BrushSpec::cube([0, 0, 0], [64, 64, 64], "a"),
            _ => BrushSpec::cube([32, 16, 16], [64, 48, 48], "b"),
        })
        .collect()
}

// spec QBSP-6.7: of two coincident faces facing the same way, the later solid brush in the file
// keeps its face
#[test]
fn the_later_solid_keeps_a_shared_face() {
    let (map, planes, surfaces) = compile_csg(sharing(["a", "b"]));
    let faces = facing_x(&planes, &surfaces, 64.0);
    assert!(faces.iter().any(|f| texture(&map, f) == b"b"), "b is later");

    let (map, planes, surfaces) = compile_csg(sharing(["b", "a"]));
    let faces = facing_x(&planes, &surfaces, 64.0);
    assert!(!faces.is_empty());
    assert!(faces.iter().all(|f| texture(&map, f) == b"a"), "a is later");
}

// spec QBSP-6.1: a brush cuts only the brushes whose bounds overlap its own, touching included
// spec QBSP-6.6: one surface per plane with faces left
#[test]
fn touching_brushes_cut_each_other() {
    let (_, planes, surfaces) = compile_csg(vec![
        BrushSpec::cube([0, 0, 0], [32, 32, 32], "a"),
        BrushSpec::cube([32, 0, 0], [64, 32, 32], "a"),
    ]);
    assert!(
        facing_x(&planes, &surfaces, 32.0).is_empty(),
        "the shared wall is inside"
    );
    let mut seen = std::collections::BTreeSet::new();
    assert!(surfaces.iter().all(|s| seen.insert(s.plane)));
}

// spec QBSP-6.3: a solid brush's faces inside water are kept, water in front
// spec QBSP-6.4: a water brush's faces are also added facing the other way
#[test]
fn solid_faces_inside_water_are_kept() {
    let (map, planes, surfaces) = compile_csg(vec![
        BrushSpec::cube([0, 0, 0], [64, 64, 64], "*water"),
        BrushSpec::cube([16, 16, 0], [48, 48, 32], "rock"),
    ]);
    let rock_top: Vec<&Face> = surfaces
        .iter()
        .flat_map(|s| &s.faces)
        .filter(|f| texture(&map, f) == b"rock")
        .filter(|f| planes.facing(f.plane, f.side).normal == [0.0, 0.0, 1.0])
        .collect();
    assert!(!rock_top.is_empty());
    assert!(rock_top.iter().all(|f| f.contents[0] == Contents::Water));
    let water: Vec<&Face> = surfaces
        .iter()
        .flat_map(|s| &s.faces)
        .filter(|f| texture(&map, f) == b"*water")
        .collect();
    assert!(
        water
            .iter()
            .any(|f| f.contents == [Contents::Empty, Contents::Water])
    );
    assert!(
        water
            .iter()
            .any(|f| f.contents == [Contents::Water, Contents::Empty])
    );
}
