// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use serde_json::json;

use super::{BrushesListed, Counted, MapSummary};
use crate::testing::MapContext;
use crate::{BrushIndex, EntityIndex, MAP_BRUSHES, MAP_OPEN, MAP_SUMMARY};

/// A box brush of texture `texture` from `min` to `max`, as a `.map` writes it.
fn cuboid([x0, y0, z0]: [i32; 3], [x1, y1, z1]: [i32; 3], texture: &str) -> String {
    format!(
        "{{\n( {x0} 0 0 ) ( {x0} 1 0 ) ( {x0} 0 1 ) {texture} 0 0 0 1 1\n\
         ( {x1} 0 0 ) ( {x1} 0 1 ) ( {x1} 1 0 ) {texture} 0 0 0 1 1\n\
         ( 0 {y0} 0 ) ( 0 {y0} 1 ) ( 1 {y0} 0 ) {texture} 0 0 0 1 1\n\
         ( 0 {y1} 0 ) ( 1 {y1} 0 ) ( 0 {y1} 1 ) {texture} 0 0 0 1 1\n\
         ( 0 0 {z0} ) ( 1 0 {z0} ) ( 0 1 {z0} ) floor 0 0 0 1 1\n\
         ( 0 0 {z1} ) ( 0 1 {z1} ) ( 1 0 {z1} ) {texture} 0 0 0 1 1\n}}\n"
    )
}

/// A world of two brushes, a door of one, two lights, a monster and an entity with no class.
fn map() -> String {
    format!(
        "{{\n\"classname\" \"worldspawn\"\n\"message\" \"The Keep\"\n\"wad\" \"keep.wad\"\n{}{}}}\n\
         {{\n\"classname\" \"func_door\"\n{}}}\n\
         {{\n\"classname\" \"light\"\n\"origin\" \"0 0 64\"\n}}\n\
         {{\n\"classname\" \"light\"\n\"origin\" \"64 0 64\"\n}}\n\
         {{\n\"classname\" \"monster_dog\"\n\"origin\" \"500 0 24\"\n}}\n\
         {{\n\"origin\" \"1 2 3\"\n}}\n",
        cuboid([0, 0, 0], [128, 128, 64], "brick"),
        cuboid([256, -32, 0], [512, 96, 192], "metal"),
        cuboid([128, 32, 0], [144, 96, 64], "door"),
    )
}

/// A session with [`map`] open.
fn opened(name: &str) -> qs_commands::Session<MapContext> {
    let dir = std::env::temp_dir().join(format!("qs-map-survey-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("keep.map");
    std::fs::write(&path, map()).unwrap();
    let mut session = MapContext::session();
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    session
}

fn listed(
    session: &mut qs_commands::Session<MapContext>,
    params: serde_json::Value,
) -> BrushesListed {
    serde_json::from_value(session.execute(MAP_BRUSHES, params).unwrap()).unwrap()
}

// No spec statement: L-40, MAP-16: map.brushes names each brush of an entity by its place, with
// the box around it and its textures each once; the world's when no entity is named; an entity
// the map has not is refused, as is a map that is not open
#[test]
fn brushes_are_listed_with_their_places_and_boxes() {
    let mut none = MapContext::session();
    assert!(none.execute(MAP_BRUSHES, json!({})).is_err(), "no map open");

    let mut session = opened("list");
    let world = listed(&mut session, json!({}));
    assert_eq!(world.entity, EntityIndex::WORLD);
    assert_eq!(world.total, 2);
    let places: Vec<BrushIndex> = world.brushes.iter().map(|brush| brush.brush).collect();
    assert_eq!(places, [BrushIndex::new(0), BrushIndex::new(1)]);
    assert_eq!(
        world.brushes[0].bounds,
        Some([[0.0, 0.0, 0.0], [128.0, 128.0, 64.0]])
    );
    assert_eq!(world.brushes[0].textures, ["brick", "floor"]);
    assert_eq!(
        world.brushes[1].bounds,
        Some([[256.0, -32.0, 0.0], [512.0, 96.0, 192.0]])
    );

    let door = listed(&mut session, json!({ "entity": 1 }));
    assert_eq!((door.entity, door.total), (EntityIndex::new(1), 1));
    assert_eq!(door.brushes[0].textures, ["door", "floor"]);
    let light = listed(&mut session, json!({ "entity": 2 }));
    assert_eq!((light.total, light.brushes.len()), (0, 0));
    assert!(
        session
            .execute(MAP_BRUSHES, json!({ "entity": 99 }))
            .is_err()
    );
}

// No spec statement: L-40: `within` keeps the brushes whose box touches the one given, each
// with the place it has among all of its entity's brushes, and `total` still counts them all
#[test]
fn brushes_are_found_by_a_box() {
    let mut session = opened("within");
    for (within, places) in [
        // Inside the second brush only.
        (json!([[300, 0, 10], [310, 10, 20]]), vec![1]),
        // Across both.
        (json!([[100, 0, 0], [300, 10, 10]]), vec![0, 1]),
        // Touching the first brush's face at x = 128 counts.
        (json!([[128, 0, 0], [200, 10, 10]]), vec![0]),
        // Between the two, touching neither.
        (json!([[130, 0, 0], [250, 10, 10]]), vec![]),
        // Above everything.
        (json!([[0, 0, 500], [600, 600, 600]]), vec![]),
        // The corners the other way round on every axis, and on one.
        (json!([[310, 10, 20], [300, 0, 10]]), vec![1]),
        (json!([[300, 0, 0], [100, 10, 10]]), vec![0, 1]),
    ] {
        let found = listed(&mut session, json!({ "within": within }));
        let got: Vec<usize> = found
            .brushes
            .iter()
            .map(|brush| brush.brush.get())
            .collect();
        assert_eq!(got, places, "{within}");
        assert_eq!(found.total, 2);
    }
}

// No spec statement: L-40, the acceptance criterion: map.summary answers the map's counts,
// checked against the map's text: its entities, each classname's count the most first, its
// brushes by where they are, its textures by face, the box around its brushes and the world's
// keys
#[test]
fn the_summary_counts_what_the_map_holds() {
    let mut none = MapContext::session();
    assert!(none.execute(MAP_SUMMARY, json!({})).is_err(), "no map open");

    let mut session = opened("summary");
    let summary: MapSummary =
        serde_json::from_value(session.execute(MAP_SUMMARY, json!({})).unwrap()).unwrap();
    let text = map();
    // The file's own counts: an entity opens at the start of a line outside any other, and a
    // face is a line that begins with a point.
    let classnames = text.matches("\"classname\"").count();
    let faces = text
        .lines()
        .filter(|line| line.trim_start().starts_with('('))
        .count();
    assert_eq!(
        summary.entities,
        classnames + 1,
        "one entity has no classname"
    );
    assert_eq!(
        summary.textures.iter().map(|t| t.count).sum::<usize>(),
        faces
    );
    assert_eq!((summary.world_brushes, summary.entity_brushes), (2, 1));
    let counted = |name: &str, count: usize| Counted {
        name: name.to_owned(),
        count,
    };
    assert_eq!(
        summary.classes,
        [
            counted("light", 2),
            counted("", 1),
            counted("func_door", 1),
            counted("monster_dog", 1),
            counted("worldspawn", 1),
        ]
    );
    assert_eq!(
        summary.textures,
        [
            counted("brick", 5),
            counted("door", 5),
            counted("metal", 5),
            counted("floor", 3),
        ]
    );
    assert_eq!(
        summary.bounds,
        Some([[0.0, -32.0, 0.0], [512.0, 128.0, 192.0]])
    );
    assert_eq!(
        summary.world,
        [
            ("classname".to_owned(), "worldspawn".to_owned()),
            ("message".to_owned(), "The Keep".to_owned()),
            ("wad".to_owned(), "keep.wad".to_owned()),
        ]
    );
    assert!(summary.path.ends_with("keep.map"));

    // `most` keeps the most used of each list and nothing else is cut.
    let short: MapSummary =
        serde_json::from_value(session.execute(MAP_SUMMARY, json!({ "most": 2 })).unwrap())
            .unwrap();
    assert_eq!(short.classes, summary.classes[..2]);
    assert_eq!(short.textures, summary.textures[..2]);
    assert_eq!(
        (short.entities, short.bounds),
        (summary.entities, summary.bounds)
    );
    let none: MapSummary =
        serde_json::from_value(session.execute(MAP_SUMMARY, json!({ "most": 0 })).unwrap())
            .unwrap();
    assert!(none.classes.is_empty() && none.textures.is_empty());

    // A file is summarised from the disk without being opened: the open map stays the open
    // map, and a file that is not there is refused.
    let other = summary.path.with_file_name("other.map");
    std::fs::write(&other, "{\n\"classname\" \"worldspawn\"\n}\n").unwrap();
    let read: MapSummary = serde_json::from_value(
        session
            .execute(MAP_SUMMARY, json!({ "path": other }))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        (read.entities, read.world_brushes, read.bounds),
        (1, 0, None)
    );
    assert!(read.path.ends_with("other.map"));
    let still: MapSummary =
        serde_json::from_value(session.execute(MAP_SUMMARY, json!({})).unwrap()).unwrap();
    assert_eq!(still, summary);
    let missing = summary.path.with_file_name("missing.map");
    assert!(
        session
            .execute(MAP_SUMMARY, json!({ "path": missing }))
            .is_err()
    );
    let mut none = MapContext::session();
    assert!(
        none.execute(MAP_SUMMARY, json!({ "path": other })).is_ok(),
        "no map need be open"
    );
}

// No spec statement: L-40: a brush that is not closed has no box: its corners are where its
// open faces were cut off, far outside the map, so it is listed with none, no box finds it,
// and the map's own box does not count it
#[test]
fn a_brush_that_is_not_closed_has_no_box() {
    // A box with its top face left out.
    let open = cuboid([0, 0, 0], [64, 64, 64], "brick");
    let top = open.lines().nth(6).unwrap().to_owned();
    assert!(top.contains("( 0 0 64 )"), "{top}");
    let open = open.replace(&format!("{top}\n"), "");
    let text = format!(
        "{{\n\"classname\" \"worldspawn\"\n{}{open}}}\n",
        cuboid([128, 0, 0], [192, 64, 64], "brick")
    );
    let dir = std::env::temp_dir().join(format!("qs-map-survey-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("open.map");
    std::fs::write(&path, text).unwrap();
    let mut session = MapContext::session();
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();

    let all = listed(&mut session, json!({}));
    assert_eq!(all.total, 2);
    assert!(all.brushes[0].bounds.is_some());
    assert_eq!(all.brushes[1].bounds, None);
    let far = listed(
        &mut session,
        json!({ "within": [[0, 0, 1000], [64, 64, 2000]] }),
    );
    assert!(far.brushes.is_empty(), "{far:?}");
    let summary: MapSummary =
        serde_json::from_value(session.execute(MAP_SUMMARY, json!({})).unwrap()).unwrap();
    assert_eq!(
        summary.bounds,
        Some([[128.0, 0.0, 0.0], [192.0, 64.0, 64.0]])
    );
    assert_eq!(summary.world_brushes, 2, "it is still counted");
}
