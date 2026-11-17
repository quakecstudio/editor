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
use crate::light_flags::SurfaceFlags;

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

// spec QBSP-15.53: a `+` texture adds each lower frame of its sequence the map lacks, after it
// and in order: digits from 0, letters from A in either case; a frame the map names itself, in
// any case, is not added twice
#[test]
fn animated_textures_add_their_lower_frames() {
    let map = world(vec![
        BrushSpec::cube([0, 0, 0], [8, 8, 8], "+3lava"),
        BrushSpec::cube([16, 0, 0], [24, 8, 8], "+0LAVA"),
        BrushSpec::cube([32, 0, 0], [40, 8, 8], "+catek"),
        BrushSpec::cube([48, 0, 0], [56, 8, 8], "+0slime"),
    ]);
    assert_eq!(
        map.texinfos.miptex(),
        [
            b"skip".to_vec(),
            b"+3lava".to_vec(),
            b"+0lava".to_vec(),
            b"+1lava".to_vec(),
            b"+2lava".to_vec(),
            b"+catek".to_vec(),
            b"+Aatek".to_vec(),
            b"+Batek".to_vec(),
            b"+0slime".to_vec(),
        ]
    );
    assert_eq!(map.texinfos.bad_animation(), None);
}

// spec QBSP-15.53: a `+` texture whose frame is not 0 to 9 or A to J stops the compile with
// `Bad animating texture`; a `+` alone is frame 0 and adds nothing
#[test]
fn a_bad_animating_texture_stops_the_compile() {
    let map = world(vec![
        BrushSpec::cube([0, 0, 0], [8, 8, 8], "+xlava"),
        BrushSpec::cube([16, 0, 0], [24, 8, 8], "+klava"),
        BrushSpec::cube([32, 0, 0], [40, 8, 8], "+"),
    ]);
    assert_eq!(map.texinfos.bad_animation(), Some(&b"+xlava"[..]));
    assert_eq!(
        map.texinfos.miptex(),
        [
            b"skip".to_vec(),
            b"+xlava".to_vec(),
            b"+klava".to_vec(),
            b"+".to_vec()
        ]
    );
    assert_eq!(
        crate::CompileError::BadAnimation(b"+xlava".to_vec()).to_string(),
        "Bad animating texture +xlava"
    );
    let alone = world(vec![BrushSpec::cube([0, 0, 0], [8, 8, 8], "+")]);
    assert_eq!(
        alone.texinfos.bad_animation(),
        None,
        "a `+` alone is frame 0"
    );
}

/// The lighting flags of the first face of the one brush of an entity of class `classname`
/// with `pairs`, textured `texture`, in a world with `world_pairs`.
fn light_of(
    classname: &str,
    texture: &str,
    pairs: &[(&str, &str)],
    world_pairs: &[(&str, &str)],
) -> SurfaceFlags {
    let owned = |pairs: &[(&str, &str)]| {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect::<Vec<_>>()
    };
    let mut entity = EntitySpec::brushes(
        classname,
        vec![BrushSpec::cube([0, 0, 0], [8, 8, 8], texture)],
    );
    entity.pairs = owned(pairs);
    let map = loaded(&MapSpec {
        entities: vec![entity],
        world_pairs: owned(world_pairs),
        ..MapSpec::default()
    });
    let face = &map.entities[1].brushes[0].faces[0];
    map.texinfos.get(face.texinfo).unwrap().light
}

// spec QBSP-15.54: a face with none of the keys has the default flags, and so has the world's
#[test]
fn a_face_without_light_keys_has_the_default_flags() {
    assert_eq!(
        light_of("func_group", "wall", &[], &[]),
        SurfaceFlags::default()
    );
    let map = world(vec![BrushSpec::cube([0, 0, 0], [8, 8, 8], "wall")]);
    let face = &map.entities[0].brushes[0].faces[0];
    assert_eq!(
        map.texinfos.get(face.texinfo).unwrap().light,
        SurfaceFlags::default()
    );
}

// spec QBSP-15.54: -1 for `_dirt`, `_bounce`, `_minlight` or `_shadow` and 1 for `_lightignore`
// set the flag; a texture named by a `_minlight_exclude` key takes no minimum light
#[test]
fn the_switches_of_an_entity_reach_its_faces() {
    let on = light_of(
        "func_group",
        "wall",
        &[
            ("_dirt", "-1"),
            ("_bounce", "-1"),
            ("_minlight", "-1"),
            ("_shadow", "-1"),
            ("_lightignore", "1"),
        ],
        &[],
    );
    assert!(on.no_dirt && on.no_bounce && on.no_minlight && on.no_shadow && on.light_ignore);
    // `_minlight` of -1 is also a minimum light of none.
    assert_eq!(on.minlight, Some(0.0));
    let off = light_of(
        "func_group",
        "wall",
        &[("_dirt", "1"), ("_shadow", "1"), ("_lightignore", "-1")],
        &[],
    );
    assert!(!off.no_dirt && !off.no_bounce && !off.no_minlight && !off.no_shadow);
    assert!(!off.light_ignore);
    for (key, texture, excluded) in [
        ("_minlight_exclude", "WALL", true),
        ("_minlight_exclude2", "wall", true),
        ("_minlight_exclude9", "wall", true),
        ("_minlight_exclude", "other", false),
        ("_minlight_exclude1", "wall", true),
        ("_minlight_exclude10", "wall", false),
    ] {
        let flags = light_of("func_group", "wall", &[(key, texture)], &[]);
        assert_eq!(flags.no_minlight, excluded, "{key} {texture}");
    }
}

// spec QBSP-15.54: a `func_detail_illusionary` and a liquid cast no shadow unless `_shadow` is 1
#[test]
fn illusionary_brushes_and_liquids_cast_no_shadow_unless_asked() {
    assert!(light_of("func_detail_illusionary", "wall", &[], &[]).no_shadow);
    assert!(light_of("FUNC_DETAIL_ILLUSIONARY", "wall", &[("_shadow", "0")], &[]).no_shadow);
    assert!(!light_of("func_detail_illusionary", "wall", &[("_shadow", "1")], &[]).no_shadow);
    assert!(light_of("func_wall", "*water1", &[], &[]).no_shadow);
    assert!(!light_of("func_wall", "*water1", &[("_shadow", "1")], &[]).no_shadow);
    assert!(!light_of("func_detail", "wall", &[], &[]).no_shadow);
}

// spec QBSP-15.54: `_phong` gives 89 degrees where `_phong_angle` gives none; either is the
// worldspawn's when the entity has no such key; the angles are held to 0 to 360
#[test]
fn phong_keys_and_the_worldspawns() {
    let phong = |pairs: &[(&str, &str)], world: &[(&str, &str)]| {
        light_of("func_group", "wall", pairs, world).phong_angle
    };
    assert_eq!(phong(&[("_phong", "1")], &[]), 89.0);
    assert_eq!(phong(&[("_phong", "1"), ("_phong_angle", "30")], &[]), 30.0);
    assert_eq!(phong(&[("_phong_angle", "45")], &[]), 45.0);
    assert_eq!(phong(&[("_phong_angle", "400")], &[]), 360.0);
    assert_eq!(phong(&[("_phong_angle", "-5")], &[]), 0.0);
    assert_eq!(phong(&[], &[("_phong", "1")]), 89.0);
    assert_eq!(phong(&[], &[("_phong_angle", "60")]), 60.0);
    // The entity's own key, even 0, is the one read.
    assert_eq!(phong(&[("_phong", "0")], &[("_phong", "1")]), 0.0);
    assert_eq!(
        phong(&[("_phong_angle", "20")], &[("_phong_angle", "60")]),
        20.0
    );
    assert_eq!(phong(&[("_phong", "1")], &[("_phong_angle", "60")]), 60.0);
    let flags = light_of(
        "func_group",
        "wall",
        &[("_phong_angle_concave", "500"), ("_phong_group", "3")],
        &[("_phong_angle_concave", "10"), ("_phong_group", "9")],
    );
    assert_eq!((flags.phong_angle_concave, flags.phong_group), (360.0, 3));
    // Only `_phong` and `_phong_angle` are the worldspawn's.
    let flags = light_of("func_group", "wall", &[], &[("_phong_group", "9")]);
    assert_eq!((flags.phong_angle_concave, flags.phong_group), (0.0, 0));
}

// spec QBSP-15.54: `_minlight` is at least 0, `_maxlight` above 0 is held to 510,
// `_lightcolorscale` and `_light_alpha` to 0 to 1; `_surflight_group`, `_object_channel_mask`
// and `_light_twosided` are whole numbers
#[test]
fn the_amounts_of_an_entity_reach_its_faces() {
    let flags = light_of(
        "func_group",
        "wall",
        &[
            ("_minlight", "37.5"),
            ("_maxlight", "600"),
            ("_lightcolorscale", "0.5"),
            ("_light_alpha", "2"),
            ("_light_twosided", "1"),
            ("_surflight_group", "4"),
            ("_object_channel_mask", "2"),
        ],
        &[],
    );
    assert_eq!((flags.minlight, flags.maxlight), (Some(37.5), 510.0));
    assert_eq!((flags.lightcolorscale, flags.light_alpha), (0.5, Some(1.0)));
    assert_eq!(flags.light_twosided, Some(true));
    assert_eq!(
        (flags.surflight_group, flags.object_channel_mask),
        (4, Some(2))
    );
    let flags = light_of(
        "func_group",
        "wall",
        &[
            ("_minlight", "-3"),
            ("_maxlight", "-1"),
            ("_lightcolorscale", "1"),
            ("_light_alpha", "-1"),
            ("_light_twosided", "0"),
        ],
        &[],
    );
    assert_eq!((flags.minlight, flags.maxlight), (Some(0.0), 0.0));
    assert_eq!((flags.lightcolorscale, flags.light_alpha), (1.0, Some(0.0)));
    assert_eq!(flags.light_twosided, Some(false));
    assert_eq!(
        light_of("func_group", "wall", &[("_lightcolorscale", "7")], &[]).lightcolorscale,
        1.0
    );
    assert_eq!(
        light_of("func_group", "wall", &[("_lightcolorscale", "-7")], &[]).lightcolorscale,
        0.0
    );
}

// spec QBSP-15.54: the minimum light's colour is `_mincolor`, or `_minlight_color` when that
// is nothing; fractions of 1 are scaled to 255 and each channel held to 0 to 255
#[test]
fn the_minimum_lights_colour() {
    let colour = |pairs: &[(&str, &str)]| light_of("func_group", "wall", pairs, &[]).minlight_color;
    assert_eq!(colour(&[("_mincolor", "255 128 0")]), [255, 128, 0]);
    assert_eq!(colour(&[("_mincolor", "1 0.5 0")]), [255, 127, 0]);
    assert_eq!(colour(&[("_minlight_color", "10 20 30")]), [10, 20, 30]);
    assert_eq!(
        colour(&[("_mincolor", "0 0 0"), ("_minlight_color", "10 20 30")]),
        [10, 20, 30]
    );
    assert_eq!(
        colour(&[("_mincolor", "4 5 6"), ("_minlight_color", "10 20 30")]),
        [4, 5, 6]
    );
    assert_eq!(colour(&[("_mincolor", "300 -20 2")]), [255, 0, 2]);
    assert_eq!(colour(&[]), [0, 0, 0]);
}

// spec QBSP-15.54: faces alike but for their lighting flags have texinfos of their own
#[test]
fn faces_with_different_light_flags_do_not_share_a_texinfo() {
    let group = |pairs: Vec<(String, String)>| EntitySpec {
        pairs,
        ..EntitySpec::brushes(
            "func_group",
            vec![BrushSpec::cube([0, 0, 0], [8, 8, 8], "wall")],
        )
    };
    let map = loaded(&MapSpec {
        entities: vec![
            group(Vec::new()),
            group(vec![("_dirt".into(), "-1".into())]),
            group(vec![("_dirt".into(), "-1".into())]),
        ],
        ..MapSpec::default()
    });
    let texinfo = |entity: usize| map.entities[entity].brushes[0].faces[0].texinfo;
    assert_ne!(texinfo(1), texinfo(2));
    assert_eq!(texinfo(2), texinfo(3));
}
