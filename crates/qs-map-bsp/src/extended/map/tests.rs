// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qs_map::MapFile;

use super::{ChopOrder, LoadedMap, MapBrush, Rotation, SideKind, SurfFlags, load};
use crate::extended::game::{Contents, Game};
use crate::extended::vec::{BspPlane, Facing, Plane, normalize};
use crate::fixtures::{BrushSpec, EntitySpec, MapSpec, Shape, map_text};

fn loaded(spec: &MapSpec) -> LoadedMap {
    load(
        &MapFile::parse(map_text(spec).as_bytes()).unwrap(),
        Game::Quake,
    )
}

fn world(brushes: Vec<BrushSpec>) -> LoadedMap {
    loaded(&MapSpec {
        world: brushes,
        entities: Vec::new(),
        ..MapSpec::default()
    })
}

fn normals(map: &LoadedMap, brush: &MapBrush) -> Vec<[f64; 3]> {
    brush
        .faces
        .iter()
        .map(|f| map.planes.get(f.planenum).unwrap().plane().normal)
        .collect()
}

fn flags(map: &LoadedMap, brush: &MapBrush) -> Vec<SurfFlags> {
    brush
        .faces
        .iter()
        .map(|f| map.texinfos.get(f.texinfo).unwrap().flags)
        .collect()
}

/// A corner of a cube cut off by the plane x + y + z = 64: three axial sides and a slope.
const TETRAHEDRON: &str = "{\n\"classname\" \"worldspawn\"\n{\n\
    ( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) wall 0 0 0 1 1\n\
    ( 0 0 0 ) ( 0 0 1 ) ( 1 0 0 ) wall 0 0 0 1 1\n\
    ( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) wall 0 0 0 1 1\n\
    ( 0 64 0 ) ( 64 0 0 ) ( 0 0 64 ) wall 0 0 0 1 1\n}\n}\n";

const AXIAL_ORDER: [[f64; 3]; 6] = [
    [-1.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, -1.0],
    [0.0, 0.0, 1.0],
];

// spec QBSP-15.14: each face's plane is its points' in double precision; a face on the plane of
// one before it is dropped
// spec QBSP-15.18: each side's polygon is its plane's square clipped by the others, and the
// bounds are around them
// spec QBSP-15.20: a box's six sides are put in the order -x, +x, -y, +y, -z, +z and get no
// bevel
#[test]
fn a_box_is_six_ordered_sides() {
    let map = world(vec![BrushSpec::cube([-16, -8, 0], [16, 8, 32], "wall")]);
    let brush = &map.entities[0].brushes[0];
    assert_eq!(normals(&map, brush), AXIAL_ORDER);
    assert!(brush.faces.iter().all(|f| f.kind == SideKind::Given));
    assert_eq!(
        (brush.bounds.mins, brush.bounds.maxs),
        ([-16.0, -8.0, 0.0], [16.0, 8.0, 32.0])
    );
    assert!(brush.faces.iter().all(|f| f.winding.len() == 4));
    assert_eq!(brush.faces[5].winding.area(), 32.0 * 16.0);
    assert_eq!(brush.contents, Contents::new(Contents::SOLID));
    assert_eq!(brush.sort_key(), (ChopOrder::default(), brush.line));

    let text = TETRAHEDRON.replacen(
        "}\n}",
        "( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) dup 0 0 0 1 1\n\
         ( 0 0 0 ) ( 0 0 0 ) ( 0 0 1 ) nonormal 0 0 0 1 1\n}\n}",
        1,
    );
    let map = load(&MapFile::parse(text.as_bytes()).unwrap(), Game::Quake);
    let given = map.entities[0].brushes[0]
        .faces
        .iter()
        .filter(|f| f.kind == SideKind::Given)
        .count();
    assert_eq!(given, 4);
    assert_eq!(
        map.texinfos.miptex(),
        [
            b"skip".to_vec(),
            b"wall".to_vec(),
            b"dup".to_vec(),
            b"nonormal".to_vec()
        ]
    );
}

// spec QBSP-15.15: a brush's contents are its first face's that is not empty, the entity's keys
// mark them mirrored or not clipping their own kind
#[test]
fn contents_come_from_the_first_face_and_the_entity() {
    let mut entity = EntitySpec::brushes(
        "func_wall",
        vec![BrushSpec::cube([0, 0, 0], [8, 8, 8], "*water1")],
    );
    entity.pairs = vec![
        ("_mirrorinside".into(), "0".into()),
        ("_noclipfaces".into(), "1".into()),
    ];
    let map = loaded(&MapSpec {
        entities: vec![entity],
        ..MapSpec::default()
    });
    let water = map.entities[1].brushes[0].contents;
    assert!(water.is_liquid());
    assert_eq!(
        water.flags() & (Contents::MIRROR_INSIDE_SET | Contents::MIRROR_INSIDE),
        Contents::MIRROR_INSIDE_SET
    );
    assert_ne!(water.flags() & Contents::SUPPRESS_CLIPPING_SAME_TYPE, 0);

    let mut visblocker = EntitySpec::brushes(
        "func_illusionary_visblocker",
        vec![BrushSpec::cube([0, 0, 0], [8, 8, 8], "wall")],
    );
    visblocker.pairs = vec![("_mirrorinside".into(), "1".into())];
    let map = loaded(&MapSpec {
        entities: vec![visblocker],
        ..MapSpec::default()
    });
    let blocker = map.entities[1].brushes[0].contents;
    assert_eq!(
        blocker.flags(),
        Contents::ILLUSIONARY_VISBLOCKER | Contents::MIRROR_INSIDE_SET | Contents::MIRROR_INSIDE
    );

    let map = world(vec![BrushSpec::cube([0, 0, 0], [8, 8, 8], "trigger")]);
    assert_eq!(
        map.entities[0].brushes[0].contents.flags() & Contents::MIRROR_INSIDE_SET,
        0
    );
}

// spec QBSP-15.16: textures are found in any case after the last slash; skip and hint faces are
// not drawn and get the plane's standard axes; liquids, unless lit, and skies are special
#[test]
fn textures_and_surface_flags() {
    let map = world(vec![
        BrushSpec::cube([0, 0, 0], [8, 8, 8], "base/WALL"),
        BrushSpec::cube([16, 0, 0], [24, 8, 8], "wall"),
        BrushSpec::cube([32, 0, 0], [40, 8, 8], "SKIP"),
        BrushSpec::cube([48, 0, 0], [56, 8, 8], "*lava1"),
        BrushSpec::cube([64, 0, 0], [72, 8, 8], "sky4"),
    ]);
    assert_eq!(
        map.texinfos.miptex(),
        [
            b"skip".to_vec(),
            b"WALL".to_vec(),
            b"*lava1".to_vec(),
            b"sky4".to_vec()
        ]
    );
    let brushes = &map.entities[0].brushes;
    assert_eq!(brushes[0].faces[0].texinfo, brushes[1].faces[0].texinfo);
    assert_eq!(brushes[0].faces[0].texture, b"base/WALL");
    assert!(
        flags(&map, &brushes[2])
            .iter()
            .all(|f| f.has(SurfFlags::NODRAW))
    );
    assert!(
        flags(&map, &brushes[3])
            .iter()
            .all(|f| f.has(SurfFlags::SPECIAL))
    );
    assert!(
        flags(&map, &brushes[4])
            .iter()
            .all(|f| f.has(SurfFlags::SPECIAL))
    );
    assert!(
        flags(&map, &brushes[0])
            .iter()
            .all(|f| *f == SurfFlags::default())
    );

    let mut lit = EntitySpec::brushes(
        "func_wall",
        vec![BrushSpec::cube([0, 0, 0], [8, 8, 8], "*lava1")],
    );
    lit.pairs = vec![("_litwater".into(), "1".into())];
    let map = loaded(&MapSpec {
        entities: vec![lit],
        ..MapSpec::default()
    });
    assert!(
        flags(&map, &map.entities[1].brushes[0])
            .iter()
            .all(|f| !f.has(SurfFlags::SPECIAL))
    );
}

// spec QBSP-15.17: in a brush with a hint face, every other face is a hintskip
#[test]
fn hint_brushes_skip_their_other_faces() {
    let text = "{\n\"classname\" \"worldspawn\"\n{\n\
                ( 0 0 0 ) ( 0 0 1 ) ( 0 1 0 ) hint 0 0 0 1 1\n\
                ( 8 0 0 ) ( 8 1 0 ) ( 8 0 1 ) skip 0 0 0 1 1\n\
                ( 0 0 0 ) ( 1 0 0 ) ( 0 0 1 ) skip 0 0 0 1 1\n\
                ( 0 8 0 ) ( 0 8 1 ) ( 1 8 0 ) skip 0 0 0 1 1\n\
                ( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) skip 0 0 0 1 1\n\
                ( 0 0 8 ) ( 1 0 8 ) ( 0 1 8 ) skip 0 0 0 1 1\n}\n}\n";
    let map = load(&MapFile::parse(text.as_bytes()).unwrap(), Game::Quake);
    let brush = &map.entities[0].brushes[0];
    assert!(brush.is_hint(&map.texinfos));
    let flags = flags(&map, brush);
    assert_eq!(
        flags
            .iter()
            .filter(|f| f.has(SurfFlags::HINT) && !f.has(SurfFlags::HINTSKIP))
            .count(),
        1
    );
    assert_eq!(
        flags.iter().filter(|f| f.has(SurfFlags::HINTSKIP)).count(),
        5
    );
    let plain = world(vec![BrushSpec::cube([0, 0, 0], [8, 8, 8], "wall")]);
    assert!(!plain.entities[0].brushes[0].is_hint(&plain.texinfos));
}

// spec QBSP-15.19: the world's extent is twice the farthest corner and hull reach, rounded up,
// and the side space
#[test]
fn the_world_extent() {
    let map = world(vec![BrushSpec::cube([-16, -8, 0], [16, 8, 33], "wall")]);
    assert_eq!(map.world_extent, ((33.0 + 64.0) * 2.0) + 24.0);
    let parallel = Plane {
        normal: [1.0, 0.0, 0.0],
        dist: 0.0,
    };
    assert_eq!(parallel.intersection(parallel, parallel), None);
}

// spec QBSP-15.20: a slanted brush gets its missing axial planes at its bounds, then a plane
// through each slanted edge that every side is behind
#[test]
fn slanted_brushes_get_bevels() {
    let wedge = BrushSpec {
        shape: Shape::Prism {
            corners: vec![[0, 0], [64, 0], [0, 32]],
            bottom: 0,
            top: 16,
        },
        texture: "wall".into(),
    };
    let map = world(vec![wedge]);
    let brush = &map.entities[0].brushes[0];
    assert_eq!(normals(&map, brush)[..6], AXIAL_ORDER);
    let bevels: Vec<usize> = (0..brush.faces.len())
        .filter(|&i| brush.faces[i].kind == SideKind::Bevel)
        .collect();
    assert_eq!(bevels, [1, 3], "+x and +y are the bounds' bevels");
    assert_eq!(brush.faces.len(), 7, "the slope's edges are axial along z");
    let plane = map.planes.get(brush.faces[1].planenum).unwrap().plane();
    assert!(
        (plane.dist - 64.0).abs() < 1e-9,
        "the bound, clipped in doubles"
    );

    let map = load(
        &MapFile::parse(TETRAHEDRON.as_bytes()).unwrap(),
        Game::Quake,
    );
    let brush = &map.entities[0].brushes[0];
    let normals: Vec<[f64; 3]> = normals(&map, brush)
        .into_iter()
        .map(|n| n.map(|c| (c * 1e6).round() / 1e6))
        .collect();
    let diagonal = |n: [f64; 3]| normalize(n).map(|c| (c * 1e6).round() / 1e6);
    assert_eq!(normals[..6], AXIAL_ORDER);
    assert_eq!(normals[6], diagonal([1.0, 1.0, 1.0]));
    let mut edges = normals[7..].to_vec();
    edges.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut want = vec![
        diagonal([0.0, 1.0, 1.0]),
        diagonal([1.0, 0.0, 1.0]),
        diagonal([1.0, 1.0, 0.0]),
    ];
    want.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(edges, want);
    assert_eq!(
        brush
            .faces
            .iter()
            .filter(|f| f.kind == SideKind::Bevel)
            .count(),
        6
    );
    let (p, _) = BspPlane::new(
        Plane {
            normal: normalize([1.0, 2.0, 0.0]),
            dist: 4.0,
        },
        Facing::AsGiven,
    );
    assert!(p.epsilon_equal(&p));
    let (turned, _) = BspPlane::new(p.plane().flipped(), Facing::AsGiven);
    assert!(!p.epsilon_equal(&turned));
}

// spec QBSP-15.21: an origin brush goes, its centre becoming the entity's origin, and the
// entity's brushes move back by it; a Hipnotic rotating entity's origin is its target's
#[test]
fn origin_brushes_and_rotation() {
    let door = EntitySpec::brushes(
        "func_door",
        vec![
            BrushSpec::cube([0, 0, 0], [16, 16, 16], "wall"),
            BrushSpec::cube([8, 8, 8], [24, 24, 24], "origin"),
        ],
    );
    let mut ordered = EntitySpec::brushes(
        "func_wall",
        vec![BrushSpec::cube([0, 0, 0], [8, 8, 8], "wall")],
    );
    ordered.pairs = vec![("_chop_order".into(), "5".into())];
    let mut unchopped = EntitySpec::brushes(
        "func_wall",
        vec![BrushSpec::cube([0, 0, 0], [8, 8, 8], "wall")],
    );
    unchopped.pairs = vec![("_chop".into(), "0".into())];
    let mut rotating = EntitySpec::brushes(
        "rotate_object",
        vec![BrushSpec::cube([0, 0, 0], [8, 8, 8], "wall")],
    );
    rotating.pairs = vec![("target".into(), "PIVOT".into())];
    let mut pivot = EntitySpec::point("info_rotate", [4, 4, 4]);
    pivot.pairs = vec![("targetname".into(), "pivot".into())];
    let mut omitted = EntitySpec::brushes(
        "func_wall",
        vec![BrushSpec::cube([0, 0, 0], [8, 8, 8], "wall")],
    );
    omitted.pairs = vec![("_omitbrushes".into(), "1".into())];
    let map = loaded(&MapSpec {
        entities: vec![door, ordered, unchopped, rotating, pivot, omitted],
        ..MapSpec::default()
    });
    let door = &map.entities[1];
    assert_eq!(door.rotation, Rotation::OriginBrush);
    assert_eq!(door.origin, [16.0, 16.0, 16.0]);
    assert_eq!(door.origin_key, Some([16.0, 16.0, 16.0]));
    assert_eq!(door.brushes.len(), 1);
    let b = &door.brushes[0];
    assert_eq!(
        (b.bounds.mins, b.bounds.maxs),
        ([-16.0, -16.0, -16.0], [0.0, 0.0, 0.0])
    );
    assert_eq!(map.entities[2].brushes[0].chop_order, ChopOrder(5));
    assert_eq!(map.entities[3].brushes[0].chop_order, ChopOrder(1));
    let rotating = &map.entities[4];
    assert_eq!(rotating.rotation, Rotation::Hipnotic);
    assert_eq!(rotating.origin, [4.0, 4.0, 4.0]);
    assert_eq!(rotating.brushes[0].bounds.maxs, [4.0, 4.0, 4.0]);
    assert!(map.entities[6].brushes.is_empty());
    assert_eq!(map.entities[0].rotation, Rotation::None);
    assert_eq!(map.entities[0].lmshift, 4);
    assert_eq!(
        [0.0, 0.5, 0.25, 2.0, 0.01].map(super::lmshift),
        [4, 3, 2, 5, 4]
    );
}
