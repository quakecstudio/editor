// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::Region;
use crate::{Entity, MapFile, Surface, TexDef};
use std::fmt::Write as _;

/// A brush filling the box `min` to `max`, its faces as qbsp reads them.
fn cube(min: [i32; 3], max: [i32; 3]) -> String {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    format!(
        "{{\n( {x0} 0 0 ) ( {x0} 1 0 ) ( {x0} 0 1 ) t 0 0 0 1 1\n\
         ( {x1} 0 0 ) ( {x1} 0 1 ) ( {x1} 1 0 ) t 0 0 0 1 1\n\
         ( 0 {y0} 0 ) ( 0 {y0} 1 ) ( 1 {y0} 0 ) t 0 0 0 1 1\n\
         ( 0 {y1} 0 ) ( 1 {y1} 0 ) ( 0 {y1} 1 ) t 0 0 0 1 1\n\
         ( 0 0 {z0} ) ( 1 0 {z0} ) ( 0 1 {z0} ) t 0 0 0 1 1\n\
         ( 0 0 {z1} ) ( 0 1 {z1} ) ( 1 0 {z1} ) t 0 0 0 1 1\n}}\n"
    )
}

/// An entity of `classname` with `keys` and `body` inside it.
fn entity(classname: &str, keys: &[(&str, &str)], body: &str) -> String {
    let mut out = format!("{{\n\"classname\" \"{classname}\"\n");
    for (key, value) in keys {
        _ = writeln!(out, "\"{key}\" \"{value}\"");
    }
    out.push_str(body);
    out.push_str("}\n");
    out
}

fn region() -> Region {
    Region::new([0.0; 3], [128.0, 128.0, 64.0]).unwrap()
}

fn wall() -> Surface {
    Surface {
        texture: b"wall".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        },
    }
}

/// A world with a brush inside the region, one touching its +x face and one far outside;
/// a door with a brush in and one out, one far away, and lights in, on the edge and out.
fn map() -> MapFile {
    let text = [
        entity(
            "worldspawn",
            &[("wad", "q.wad")],
            &[
                cube([0, 0, 0], [128, 128, 16]),
                cube([128, 0, 0], [144, 128, 64]),
                cube([1024, 1024, 0], [1088, 1088, 64]),
            ]
            .concat(),
        ),
        entity(
            "func_door",
            &[("targetname", "in")],
            &[
                cube([16, 16, 16], [32, 32, 48]),
                cube([512, 0, 0], [528, 16, 16]),
            ]
            .concat(),
        ),
        entity(
            "func_door",
            &[("targetname", "out")],
            &cube([700, 700, 0], [716, 716, 16]),
        ),
        entity("light", &[("origin", "64 64 32")], ""),
        entity("light", &[("origin", "128 128 64")], ""),
        entity("light", &[("origin", "129 64 32")], ""),
    ]
    .concat();
    MapFile::parse(text.as_bytes()).unwrap()
}

fn names(file: &MapFile) -> Vec<String> {
    file.entities
        .iter()
        .map(|e| {
            let name = e
                .value(b"targetname")
                .or_else(|| e.value(b"origin"))
                .unwrap_or(b"");
            format!(
                "{} {}",
                String::from_utf8_lossy(e.classname().unwrap_or(b"")),
                String::from_utf8_lossy(name)
            )
        })
        .collect()
}

// spec MAP-16.1: a region has volume on every axis; the region of rooms is the box around their
// bounds, and rooms with none have none
#[test]
fn a_region_is_a_box_with_volume() {
    assert!(Region::new([0.0; 3], [1.0, 1.0, 0.0]).is_none());
    assert!(Region::new([2.0; 3], [1.0; 3]).is_none());
    let text = [
        entity("worldspawn", &[], ""),
        entity(
            "func_group",
            &[
                ("_tb_type", "_tb_group"),
                ("_tb_name", "Armory"),
                ("_tb_id", "1"),
            ],
            &cube([0, 0, 0], [128, 128, 64]),
        ),
        entity(
            "func_group",
            &[
                ("_tb_type", "_tb_group"),
                ("_tb_name", "Hall"),
                ("_tb_id", "2"),
            ],
            &cube([512, 512, 512], [640, 640, 640]),
        ),
        entity(
            "func_group",
            &[
                ("_tb_type", "_tb_group"),
                ("_tb_name", "Empty"),
                ("_tb_id", "3"),
            ],
            "",
        ),
    ]
    .concat();
    let rooms = MapFile::parse(text.as_bytes()).unwrap().rooms();
    let both = Region::of_rooms(rooms.list()).unwrap();
    assert_eq!((both.min(), both.max()), ([0.0; 3], [640.0; 3]));
    let empty = rooms.list().iter().filter(|r| r.name == "Empty");
    assert_eq!(Region::of_rooms(empty), None);
    assert_eq!(Region::of_rooms([]), None);
}

// spec MAP-16.2: the cut keeps the world's key-values and its brushes in or touching the
// region, each entity with a brush in it whole, and each point entity whose origin lies in it,
// edges included, in their order
#[test]
fn a_cut_keeps_what_is_in_the_region() {
    let cut = region().cut(&map(), &wall()).unwrap();
    assert_eq!(
        names(&cut),
        [
            "worldspawn ",
            "func_door in",
            "light 64 64 32",
            "light 128 128 64"
        ]
    );
    let world = &cut.entities[0];
    assert_eq!(world.value(b"wad"), Some(&b"q.wad"[..]));
    // Two of the world's three brushes, then the six seals.
    assert_eq!(world.brushes().count(), 2 + 6);
    assert_eq!(
        cut.entities[1].brushes().count(),
        2,
        "the door is kept whole"
    );
}

// spec MAP-16.3: six world brushes 16 thick seal the region from outside, reaching 16 past it
// on the other axes, painted with the surface given
#[test]
fn a_cut_is_sealed_on_six_sides() {
    let cut = region()
        .cut(
            &MapFile::parse(entity("worldspawn", &[], "").as_bytes()).unwrap(),
            &wall(),
        )
        .unwrap();
    let world: &Entity = &cut.entities[0];
    let boxes: Vec<[[f64; 3]; 2]> = world
        .brushes()
        .map(|brush| {
            let corners = brush.vertices();
            let lo = [0, 1, 2].map(|a| corners.iter().map(|c| c[a]).fold(f64::INFINITY, f64::min));
            let hi = [0, 1, 2].map(|a| {
                corners
                    .iter()
                    .map(|c| c[a])
                    .fold(f64::NEG_INFINITY, f64::max)
            });
            [lo, hi]
        })
        .collect();
    assert_eq!(
        boxes,
        [
            [[-16.0, -16.0, -16.0], [0.0, 144.0, 80.0]],
            [[128.0, -16.0, -16.0], [144.0, 144.0, 80.0]],
            [[-16.0, -16.0, -16.0], [144.0, 0.0, 80.0]],
            [[-16.0, 128.0, -16.0], [144.0, 144.0, 80.0]],
            [[-16.0, -16.0, -16.0], [144.0, 144.0, 0.0]],
            [[-16.0, -16.0, 64.0], [144.0, 144.0, 80.0]],
        ]
    );
    assert!(
        world
            .brushes()
            .flat_map(|b| &b.faces)
            .all(|f| f.texture() == b"wall")
    );
}

// spec MAP-16.2: the cut keeps each player start, one outside the region moved to its middle, and
// leaves out what lies outside, a target included
#[test]
fn a_cut_keeps_the_player_start_inside() {
    let text = [
        entity("worldspawn", &[], &cube([0, 0, 0], [128, 128, 16])),
        entity("info_player_start", &[("origin", "900 900 24")], ""),
        entity("info_player_deathmatch", &[("origin", "32 32 24")], ""),
        entity(
            "func_train",
            &[("target", "p1")],
            &cube([16, 16, 16], [32, 32, 48]),
        ),
        entity(
            "path_corner",
            &[("targetname", "p1"), ("origin", "800 0 0")],
            "",
        ),
    ]
    .concat();
    let cut = region()
        .cut(&MapFile::parse(text.as_bytes()).unwrap(), &wall())
        .unwrap();
    assert_eq!(
        names(&cut),
        [
            "worldspawn ",
            "info_player_start 64 64 32",
            "info_player_deathmatch 32 32 24",
            "func_train ",
        ]
    );
}
