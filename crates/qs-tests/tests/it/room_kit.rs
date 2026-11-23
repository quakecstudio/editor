// SPDX-License-Identifier: GPL-3.0-or-later

//! The rooms and passages `qs-map-gen`'s kit builds one at a time (L-49), compiled by
//! `qs-map-bsp` and walked in the player's hull: a room alone is sealed, and a room with a
//! doorway on each side, joined by passages to four rooms of other shapes on other floors, is
//! sealed with them and walked from its middle to each.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    reason = "test code: a failed unwrap, panic or index is a failed test, and map units fit"
)]

use qs_bsp::{Bsp, BspLoader};
use qs_map::{Brush, Entity, Item, MapFile};
use qs_map_gen::{
    Arch, Door, KIT_MIN_WALL, KIT_WALL, KitError, PassageKit, RoomKit, RoomPart, RoomParts,
    RoomShape, Side, Textures, WallLights,
};
use qs_sim::collision::HullSet;

use crate::random_levels::{NEAR, blocked, walked};

/// How far apart the middles of two joined rooms are.
const APART: f64 = 1024.0;

fn textures() -> Textures {
    Textures {
        wall: "wall".to_owned(),
        floor: "floor".to_owned(),
        ceiling: "ceiling".to_owned(),
        trim: "trim".to_owned(),
        door: "door".to_owned(),
        liquid: Some("*lava1".to_owned()),
        sky: Some("sky1".to_owned()),
        wall_lights: WallLights::Torches,
    }
}

/// A room of `shape` at `middle` on `floor`, as small as the shape may be but 32 wider one
/// way, turned to `turned`, with doorways on `doors`.
fn kit(shape: RoomShape, middle: [f64; 2], floor: f64, turned: Side, doors: &[Side]) -> RoomKit {
    let least = shape.least_half();
    let half = match shape {
        RoomShape::Chamber => [least, least],
        RoomShape::Hall
        | RoomShape::Pit
        | RoomShape::Split
        | RoomShape::Court
        | RoomShape::Cross
        | RoomShape::Vault
        | RoomShape::Atrium => [least + 32.0, least],
    };
    RoomKit {
        shape,
        middle,
        floor,
        half,
        height: shape.least_height(),
        turned,
        doors: doors.to_vec(),
        arch: Arch::Cut,
        wall: KIT_WALL,
        parts: Vec::new(),
    }
}

/// The map of `brushes` with a player start at `start`.
fn map(brushes: Vec<Brush>, start: [f64; 3]) -> Vec<u8> {
    let mut world = Entity::new();
    world.set(b"classname", b"worldspawn").unwrap();
    world.items.extend(brushes.into_iter().map(Item::Brush));
    let mut player = Entity::new();
    player.set(b"classname", b"info_player_start").unwrap();
    let [x, y, z] = start;
    player
        .set(b"origin", format!("{x} {y} {z}").as_bytes())
        .unwrap();
    let mut file = MapFile::new();
    file.push_entity(world);
    file.push_entity(player);
    file.to_bytes()
}

/// Compiles `map`, which must be sealed, and gives every place a player walks to from
/// `start`, with whether the player's box is in rock at each of `stands`.
fn walk(map: &[u8], start: [f64; 3], stands: &[[f64; 3]], what: &str) -> Vec<[f32; 3]> {
    let compiled = qs_map_bsp::Compiler::default()
        .compile(map, None)
        .unwrap_or_else(|error| panic!("{what}: {error}"));
    assert!(compiled.leak_trail.is_empty(), "{what}: leaks");
    assert!(
        compiled.portal_file.is_some(),
        "{what}: the outside was not filled"
    );
    let bsp = Bsp::read(&compiled.bsp, BspLoader::default()).unwrap();
    let hulls = HullSet::new(&bsp);
    let [h0, h1, h2, _] = bsp.models[0].headnode;
    let hull = hulls.hull(1, [h0, h1, h2]);
    for stand in stands {
        assert!(
            !blocked(&hull, h1, stand.map(|at| at as f32)),
            "{what}: {stand:?} is in rock"
        );
    }
    walked(&hull, h1, start.map(|at| at as f32))
}

// No spec statement: L-49, each shape built alone with no doorway is sealed, and its standing
// place and every spot it answers are open to the player's box
#[test]
fn a_room_alone_is_sealed() {
    for shape in RoomShape::ALL {
        let room = kit(shape, [0.0, 0.0], 0.0, Side::North, &[])
            .build(&textures())
            .unwrap();
        assert!(room.openings.is_empty());
        let mut stands = room.spots.clone();
        stands.push(room.stand);
        walk(
            &map(room.brushes, room.stand),
            room.stand,
            &stands,
            shape.name(),
        );
    }
}

// No spec statement: L-74, each shape dressed with every part, with no doorway and its wall
// thick enough for bays, is sealed, its standing place and spots are open, and each is
// walked to; bays asked of a thin wall are refused
#[test]
fn a_room_dressed_with_every_part_is_sealed_and_walked() {
    for shape in RoomShape::ALL {
        let mut asked = kit(shape, [0.0, 0.0], 0.0, Side::North, &[]);
        asked.parts = RoomPart::ALL.to_vec();
        assert!(matches!(
            asked.build(&textures()),
            Err(qs_map_gen::GenError::Kit(KitError::BayWall { .. }))
        ));
        asked.wall = RoomPart::BAY_DEPTH + KIT_MIN_WALL;
        let room = asked.build(&textures()).unwrap();
        let plain = kit(shape, [0.0, 0.0], 0.0, Side::North, &[])
            .build(&textures())
            .unwrap();
        let fits = RoomPart::ALL.iter().any(|part| part.fits(shape));
        assert_eq!(
            room.brushes.len() > plain.brushes.len(),
            fits,
            "{}",
            shape.name()
        );
        let mut stands = room.spots.clone();
        stands.push(room.stand);
        let reached = walk(
            &map(room.brushes, room.stand),
            room.stand,
            &stands,
            shape.name(),
        );
        for stand in &stands {
            let [x, y, z] = stand.map(|at| at as f32);
            let near = reached.iter().any(|at| {
                (at[0] - x).abs() <= NEAR && (at[1] - y).abs() <= NEAR && (at[2] - z).abs() <= NEAR
            });
            // An atrium's balcony is not climbed to from its floor.
            let balcony = shape == RoomShape::Atrium && z > room.stand[2] as f32 + 64.0;
            assert!(
                near || balcony,
                "{}: {stand:?} is not reached",
                shape.name()
            );
        }
    }
}

// No spec statement: L-49, each shape with a doorway on each side, joined by passages to four
// rooms of the other shapes on floors up to 64 above and below, is sealed with them, and a
// player walks from its middle to each of them, but for an atrium's balcony, which is one way
#[test]
fn a_room_joined_on_each_side_is_sealed_and_walked() {
    let sides = [Side::East, Side::North, Side::West, Side::South];
    let offsets = [[APART, 0.0], [0.0, APART], [-APART, 0.0], [0.0, -APART]];
    let opposite = [Side::West, Side::South, Side::East, Side::North];
    let floors = [64.0, -64.0, 32.0, 0.0];
    for (place, shape) in RoomShape::ALL.into_iter().enumerate() {
        let what = shape.name();
        let paints = textures();
        let centre: RoomParts = kit(shape, [0.0, 0.0], 0.0, Side::East, &sides)
            .build(&paints)
            .unwrap();
        assert_eq!(centre.openings.len(), 4);
        let mut brushes = centre.brushes.clone();
        let mut stands = vec![centre.stand];
        for (n, side) in sides.into_iter().enumerate() {
            let other = RoomShape::ALL[(place + n + 1) % RoomShape::ALL.len()];
            // Turned toward the centre, so a room on two levels is entered on its raised part.
            let room = kit(other, offsets[n], floors[n], opposite[n], &[opposite[n]])
                .build(&paints)
                .unwrap();
            let from = centre.openings.iter().find(|o| o.side == side).unwrap().at;
            let to = room.openings[0].at;
            let passage = PassageKit {
                from,
                to,
                arch: if n % 2 == 0 { Arch::Cut } else { Arch::Square },
                door: Door::None,
                wall: KIT_WALL,
            }
            .build(&paints)
            .unwrap();
            let climb = (to[2] - from[2]).abs();
            // 16 a step.
            assert_eq!(
                f64::from(u32::try_from(passage.steps).unwrap()) * 16.0,
                climb,
                "{what} to {}",
                other.name()
            );
            brushes.extend(room.brushes);
            brushes.extend(passage.brushes);
            stands.push(room.stand);
        }
        let map = map(brushes, centre.stand);
        let reached = walk(&map, centre.stand, &stands, what);
        let near = |reached: &[[f32; 3]], stand: [f64; 3]| {
            let [x, y, z] = stand.map(|at| at as f32);
            reached.iter().any(|at| {
                (at[0] - x).abs() <= NEAR && (at[1] - y).abs() <= NEAR && (at[2] - z).abs() <= NEAR
            })
        };
        for (n, &stand) in stands.iter().enumerate() {
            // An atrium's balcony, on the side it is turned to (east, the first room after the
            // centre), is dropped from and not climbed to: that room is not walked to from the
            // atrium's floor, and the atrium is walked to from it.
            if shape == RoomShape::Atrium && n == 1 {
                assert!(!near(&reached, stand), "{what}: the balcony is climbed to");
                let back = walk(&map, stand, &stands, what);
                assert!(
                    near(&back, centre.stand),
                    "{what}: not reached from the balcony"
                );
                continue;
            }
            assert!(near(&reached, stand), "{what}: {stand:?} is not reached");
        }
    }
}

// No spec statement: L-49, a passage is a doorway's height at each end however far it climbs,
// so it is sealed by the rooms it joins whatever their heights: the lowest room of a shape,
// its shell as thin as a shell may be, joined to a room 192 above it
#[test]
fn a_passage_that_climbs_far_is_sealed_by_low_rooms() {
    let paints = textures();
    for (lower, upper) in [(Side::East, Side::West), (Side::West, Side::East)] {
        let [dx, _] = match lower {
            Side::East => [APART, 0.0],
            Side::North | Side::West | Side::South => [-APART, 0.0],
        };
        let mut low = kit(RoomShape::Pit, [0.0, 0.0], 0.0, Side::North, &[lower]);
        low.wall = 32.0;
        let mut high = kit(RoomShape::Hall, [dx, 0.0], 192.0, Side::North, &[upper]);
        high.wall = 32.0;
        let (low, high) = (low.build(&paints).unwrap(), high.build(&paints).unwrap());
        let passage = PassageKit {
            from: low.openings[0].at,
            to: high.openings[0].at,
            arch: Arch::Cut,
            door: Door::None,
            wall: 32.0,
        }
        .build(&paints)
        .unwrap();
        assert_eq!(passage.steps, 12);
        let mut brushes = low.brushes;
        brushes.extend(high.brushes);
        brushes.extend(passage.brushes);
        let reached = walk(
            &map(brushes, low.stand),
            low.stand,
            &[low.stand, high.stand],
            "a climb of 192",
        );
        let [x, y, z] = high.stand.map(|at| at as f32);
        assert!(reached.iter().any(|at| {
            (at[0] - x).abs() <= NEAR && (at[1] - y).abs() <= NEAR && (at[2] - z).abs() <= NEAR
        }));
    }
}

// No spec statement: L-49, a passage longer than 2048, or that climbs more than 512, is
// refused, as one too short for its stairs is; so is one whose ends are not in line
#[test]
fn a_passage_out_of_bounds_is_refused() {
    let paints = textures();
    let refused = |from: [f64; 3], to: [f64; 3]| {
        PassageKit {
            from,
            to,
            arch: Arch::Square,
            door: Door::None,
            wall: KIT_WALL,
        }
        .build(&paints)
        .unwrap_err()
        .to_string()
    };
    let far = "a passage is at most 2048 long and climbs at most 512";
    assert_eq!(refused([0.0; 3], [2064.0, 0.0, 0.0]), far);
    assert_eq!(refused([0.0; 3], [1024.0, 0.0, 528.0]), far);
    assert_eq!(refused([0.0; 3], [32_000_064.0, 0.0, 32_000_000.0]), far);
    assert!(refused([0.0; 3], [96.0, 0.0, 64.0]).contains("it needs a length of 128"));
    assert!(refused([0.0; 3], [64.0, 64.0, 0.0]).contains("in line along x or along y"));
    // The longest and the highest that are taken.
    let most = PassageKit {
        from: [0.0; 3],
        to: [2048.0, 0.0, 512.0],
        arch: Arch::Square,
        door: Door::None,
        wall: KIT_WALL,
    }
    .build(&paints)
    .unwrap();
    assert_eq!(most.steps, 32);
}
