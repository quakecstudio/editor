// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use proptest::prelude::*;

use super::{ExternalMapError, Placement, scan_floats};
use crate::{Entity, Face, MapFile};

/// A prefab: one box brush in the world, from (0 0 0) to (32 16 8), and one in a func_wall.
const PREFAB: &str = "{\n\"classname\" \"worldspawn\"\n{\n\
( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) brick 4 2 15 0.5 2\n\
( 32 0 0 ) ( 32 0 1 ) ( 32 1 0 ) brick 4 2 15 0.5 2\n\
( 0 0 0 ) ( 0 0 1 ) ( 1 0 0 ) brick 4 2 15 0.5 2\n\
( 0 16 0 ) ( 1 16 0 ) ( 0 16 1 ) brick 4 2 15 0.5 2\n\
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) brick 4 2 15 0.5 2\n\
( 0 0 8 ) ( 0 1 8 ) ( 1 0 8 ) brick 4 2 15 0.5 2\n}\n}\n\
{\n\"classname\" \"func_wall\"\n{\n\
( 0 0 8 ) ( 0 1 8 ) ( 0 0 9 ) trim 0 0 0 1 1\n\
( 4 0 8 ) ( 4 0 9 ) ( 4 1 8 ) trim 0 0 0 1 1\n\
( 0 0 8 ) ( 0 0 9 ) ( 1 0 8 ) trim 0 0 0 1 1\n\
( 0 4 8 ) ( 1 4 8 ) ( 0 4 9 ) trim 0 0 0 1 1\n\
( 0 0 8 ) ( 1 0 8 ) ( 0 1 8 ) trim 0 0 0 1 1\n\
( 0 0 12 ) ( 0 1 12 ) ( 1 0 12 ) trim 0 0 0 1 1\n}\n}\n";

const NEAR: f64 = 1e-6;

/// A map with one instance of the prefab, with `keys` besides its classname and file.
fn instance(keys: &str) -> MapFile {
    let text = format!(
        "{{\n\"classname\" \"worldspawn\"\n}}\n{{\n\"classname\" \"misc_external_map\"\n\
         \"_external_map\" \"prefabs/box.map\"\n\"_external_map_classname\" \"func_group\"\n\
         {keys}}}\n"
    );
    MapFile::parse(text.as_bytes()).unwrap()
}

/// The placement of an instance with `keys`.
fn placement(keys: &str) -> Placement {
    Placement::of(&instance(keys).entities[1], 1).unwrap()
}

/// The prefab, for the file the instance names.
fn load(name: &str) -> Option<Vec<u8>> {
    (name == "prefabs/box.map").then(|| PREFAB.as_bytes().to_vec())
}

fn expanded(keys: &str) -> MapFile {
    instance(keys).with_external_maps(&mut load).unwrap()
}

/// The texels of `point` on `face`.
fn texels(face: &Face, point: [f64; 3]) -> [f64; 2] {
    face.texvecs()
        .unwrap()
        .map(|a| point[0] * a[0] + point[1] * a[1] + point[2] * a[2] + a[3])
}

/// Every face of `entity`'s brushes.
fn faces(entity: &Entity) -> Vec<Face> {
    entity
        .brushes()
        .flat_map(|brush| brush.faces.iter().cloned())
        .collect()
}

/// The prefab's faces, in the order an instance takes them.
fn prefab_faces() -> Vec<Face> {
    let prefab = MapFile::parse(PREFAB.as_bytes()).unwrap();
    prefab.entities.iter().flat_map(faces).collect()
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() < NEAR)
}

/// Whether every point of every placed face keeps its texel, within `within`.
fn keeps_texels(keys: &str, within: f64) -> bool {
    let map = expanded(keys);
    let placement = placement(keys);
    prefab_faces()
        .iter()
        .zip(&faces(&map.entities[1]))
        .all(|(from, to)| {
            from.points().iter().all(|&p| {
                let (want, got) = (texels(from, p), texels(to, placement.point(p)));
                (want[0] - got[0]).abs() < within && (want[1] - got[1]).abs() < within
            })
        })
}

// spec MAP-14.1, MAP-14.2, MAP-14.7: an instance takes every brush of the map it names, in its
// entities' order, becomes its classname with origin 0 0 0 and keeps its other keys
#[test]
fn an_instance_takes_the_prefab_s_brushes() {
    let map = expanded("\"origin\" \"64 -32 16\"\n\"note\" \"kept\"\n");
    let entity = &map.entities[1];
    assert_eq!(entity.classname(), Some(&b"func_group"[..]));
    assert_eq!(entity.value(b"origin"), Some(&b"0 0 0"[..]));
    assert_eq!(entity.value(b"note"), Some(&b"kept"[..]));
    assert_eq!(entity.brushes().count(), 2);
    for (from, to) in prefab_faces().iter().zip(&faces(entity)) {
        assert_eq!(from.texture(), to.texture());
        for (a, b) in from.points().iter().zip(to.points()) {
            assert!(
                near([a[0] + 64.0, a[1] - 32.0, a[2] + 16.0], *b),
                "{a:?} {b:?}"
            );
        }
    }
    assert_eq!(
        map.entities[0].brushes().count(),
        0,
        "the world is left as it was"
    );
    let text = String::from_utf8(instance("").to_bytes())
        .unwrap()
        .replace("misc_external_map", "MISC_External_Map");
    let upper = MapFile::parse(text.as_bytes()).unwrap();
    assert_eq!(
        upper.with_external_maps(&mut load).unwrap().entities[1]
            .brushes()
            .count(),
        2,
        "the classname is compared in any case"
    );
}

// spec MAP-14.6: every point of a placed face keeps the texel it had, whatever the scale, turn
// and move
#[test]
fn placed_faces_keep_their_texels() {
    assert!(keeps_texels(
        "\"origin\" \"100 -20 5\"\n\"_external_map_angles\" \"30 45 10\"\n\
         \"_external_map_scale\" \"2 0.5 3\"\n",
        NEAR
    ));
}

// spec MAP-14.4, MAP-14.6: the angles are pitch, yaw and roll, turning by Rz(yaw) Ry(pitch)
// Rx(roll); with none, the yaw is _external_map_angle
#[test]
fn angles_turn_right_handed() {
    let x = [1.0, 0.0, 0.0];
    assert!(near(
        placement("\"_external_map_angle\" \"90\"\n").point(x),
        [0.0, 1.0, 0.0]
    ));
    assert!(near(
        placement("\"_external_map_angles\" \"0 90 0\"\n").point(x),
        [0.0, 1.0, 0.0]
    ));
    assert!(near(
        placement("\"_external_map_angles\" \"90 0 0\"\n").point(x),
        [0.0, 0.0, -1.0]
    ));
    assert!(near(
        placement("\"_external_map_angles\" \"0 0 90\"\n").point([0.0, 1.0, 0.0]),
        [0.0, 0.0, 1.0]
    ));
    assert!(
        near(
            placement("\"_external_map_angles\" \"0 180 0\"\n\"_external_map_angle\" \"90\"\n")
                .point(x),
            [-1.0, 0.0, 0.0]
        ),
        "angles that are not all 0 win"
    );
    assert!(
        near(
            placement("\"_external_map_angles\" \"0.00001 0 0\"\n\"_external_map_angle\" \"90\"\n")
                .point(x),
            [0.0, 1.0, 0.0]
        ),
        "angles within 0.0001 of 0 are none"
    );
    let both = placement("\"_external_map_angles\" \"90 90 0\"\n");
    assert!(near(both.point(x), [0.0, 0.0, -1.0]));
    assert!(near(both.point([0.0, 0.0, 1.0]), [0.0, 1.0, 0.0]));
    let rolled = placement("\"_external_map_angles\" \"0 90 90\"\n");
    assert!(
        near(rolled.point([0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]),
        "the roll comes first"
    );
}

// spec MAP-14.5: three numbers scale each axis; fewer give one scale, 1 for 0 or none; a 0 of
// three is an error
#[test]
fn scales_one_or_three() {
    let scale = |keys: &str| placement(keys).scale;
    assert_eq!(scale(""), [1.0; 3]);
    assert_eq!(scale("\"_external_map_scale\" \"0\"\n"), [1.0; 3]);
    assert_eq!(scale("\"_external_map_scale\" \"2\"\n"), [2.0; 3]);
    assert_eq!(scale("\"_external_map_scale\" \"2 3\"\n"), [2.0; 3]);
    assert_eq!(
        scale("\"_external_map_scale\" \"2 3 0.5\"\n"),
        [2.0, 3.0, 0.5]
    );
    assert!(matches!(
        instance("\"_external_map_scale\" \"1 0 1\"\n").with_external_maps(&mut load),
        Err(ExternalMapError::ZeroScale { entity: 1 })
    ));
}

// spec MAP-14.3: up to three floats are read, stopping at the first that is not one
#[test]
fn floats_are_scanned() {
    assert_eq!(scan_floats(b""), ([0.0; 3], 0));
    assert_eq!(scan_floats(b" 1.5  -2"), ([1.5, -2.0, 0.0], 2));
    assert_eq!(scan_floats(b"1 x 3"), ([1.0, 0.0, 0.0], 1));
    assert_eq!(scan_floats(b"1 2 3 4"), ([1.0, 2.0, 3.0], 3));
    assert_eq!(scan_floats(b"0.1"), ([0.1_f32, 0.0, 0.0], 1));
}

// spec MAP-14.2: an entity with _omitbrushes set gives no brushes, and a map with none is an
// error
#[test]
fn omitted_brushes_are_left_out() {
    let with = |omit: &str| {
        PREFAB.replace(
            "\"classname\" \"func_wall\"",
            &format!("\"classname\" \"func_wall\"\n\"_omitbrushes\" \"{omit}\""),
        )
    };
    let count = |text: String| {
        instance("")
            .with_external_maps(&mut |_| Some(text.as_bytes().to_vec()))
            .unwrap()
            .entities[1]
            .brushes()
            .count()
    };
    assert_eq!(count(with("1")), 1);
    assert_eq!(count(with("0")), 2);
    let empty = "{\n\"classname\" \"worldspawn\"\n}\n";
    assert!(matches!(
        instance("").with_external_maps(&mut |_| Some(empty.as_bytes().to_vec())),
        Err(ExternalMapError::NoBrushes { .. })
    ));
}

// spec MAP-14.1, MAP-14.2: an instance without its keys or with brushes, and a file that
// cannot be read, does not parse or does not start with its world, is an error
#[test]
fn bad_instances_are_errors() {
    let without = |key: &str| {
        let text = String::from_utf8(instance("").to_bytes())
            .unwrap()
            .replace(key, "\"_unused\"");
        MapFile::parse(text.as_bytes())
            .unwrap()
            .with_external_maps(&mut load)
    };
    assert!(matches!(
        without("\"_external_map\""),
        Err(ExternalMapError::MissingKey {
            entity: 1,
            key: "_external_map"
        })
    ));
    assert!(matches!(
        without("\"_external_map_classname\""),
        Err(ExternalMapError::MissingKey {
            entity: 1,
            key: "_external_map_classname"
        })
    ));
    let mut braced = instance("");
    let brush = MapFile::parse(PREFAB.as_bytes()).unwrap().entities[0]
        .items
        .last()
        .cloned()
        .unwrap();
    braced.entities[1].items.push(brush);
    assert!(matches!(
        braced.with_external_maps(&mut load),
        Err(ExternalMapError::HasBrushes { entity: 1 })
    ));
    assert!(matches!(
        instance("").with_external_maps(&mut |_| None),
        Err(ExternalMapError::Unreadable { .. })
    ));
    assert!(matches!(
        instance("").with_external_maps(&mut |_| Some(b"{".to_vec())),
        Err(ExternalMapError::Map { .. })
    ));
    let not_world = PREFAB.replacen("worldspawn", "func_wall", 1);
    assert!(matches!(
        instance("").with_external_maps(&mut |_| Some(not_world.as_bytes().to_vec())),
        Err(ExternalMapError::NotWorld { .. })
    ));
    let plain = MapFile::parse(b"{\n\"classname\" \"worldspawn\"\n}\n").unwrap();
    assert_eq!(
        plain.with_external_maps(&mut |_| None).unwrap(),
        plain,
        "a map with no instance is as it was"
    );
}

proptest! {
    // spec MAP-14.6: texels are kept for any placement
    #[test]
    fn any_placement_keeps_texels(
        origin in prop::array::uniform3(-512.0..512.0_f32),
        angles in prop::array::uniform3(-180.0..180.0_f32),
        scale in prop::array::uniform3(prop_oneof![0.25..4.0_f32, -4.0..-0.25_f32]),
    ) {
        let keys = format!(
            "\"origin\" \"{} {} {}\"\n\"_external_map_angles\" \"{} {} {}\"\n\
             \"_external_map_scale\" \"{} {} {}\"\n",
            origin[0], origin[1], origin[2], angles[0], angles[1], angles[2],
            scale[0], scale[1], scale[2]
        );
        prop_assert!(keeps_texels(&keys, 1e-4));
    }
}

// spec MAP-14.6: the turn is the roll about x, then the pitch about y, then the yaw about z,
// each right-handed, for any three angles
#[test]
fn the_turn_composes_roll_pitch_and_yaw() {
    let (pitch, yaw, roll) = (30.0_f64, 50.0_f64, 20.0_f64);
    let turn = |v: [f64; 3], axis: usize, degrees: f64| {
        let (s, c) = degrees.to_radians().sin_cos();
        let (i, j) = match axis {
            0 => (1, 2),
            1 => (2, 0),
            _ => (0, 1),
        };
        let mut out = v;
        out[i] = c * v[i] - s * v[j];
        out[j] = s * v[i] + c * v[j];
        out
    };
    let placed = placement(&format!(
        "\"_external_map_angles\" \"{pitch} {yaw} {roll}\"\n\"origin\" \"1 2 3\"\n"
    ));
    for v in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [3.0, -2.0, 5.0],
    ] {
        let want = turn(turn(turn(v, 0, roll), 1, pitch), 2, yaw);
        let want = [want[0] + 1.0, want[1] + 2.0, want[2] + 3.0];
        assert!(
            near(placed.point(v), want),
            "{v:?}: {:?} {want:?}",
            placed.point(v)
        );
    }
}

// spec MAP-14.1, MAP-14.2: each error says what is wrong and names what it is about; only a map
// error has a cause
#[test]
fn errors_say_what_and_where() {
    let file = || "prefabs/box.map".to_owned();
    let errors = [
        (
            ExternalMapError::MissingKey {
                entity: 3,
                key: "_external_map",
            },
            "3",
        ),
        (ExternalMapError::HasBrushes { entity: 4 }, "4"),
        (ExternalMapError::Unreadable { file: file() }, "box.map"),
        (ExternalMapError::NotWorld { file: file() }, "worldspawn"),
        (ExternalMapError::NoBrushes { file: file() }, "box.map"),
        (ExternalMapError::ZeroScale { entity: 5 }, "5"),
    ];
    for (error, names) in errors {
        assert!(error.to_string().contains(names), "{error}");
        assert!(std::error::Error::source(&error).is_none());
    }
    let parsed = instance("")
        .with_external_maps(&mut |_| Some(b"{".to_vec()))
        .unwrap_err();
    assert!(
        parsed.to_string().starts_with("prefabs/box.map: "),
        "{parsed}"
    );
    assert!(std::error::Error::source(&parsed).is_some());
}
