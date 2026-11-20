// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code: a failed unwrap or a missing issue is a failed test"
)]

use std::path::{Path, PathBuf};

use qs_commands::Session;
use qs_map::MapFile;
use serde_json::json;

use super::{
    MAP_FIX, MAP_ISSUES, MAP_SEARCH, MapFound, MapIssue, MapIssues, NO_START, NO_VOLUME, QuickFix,
};
use crate::testing::{MapContext, TestProject};
use crate::{BrushIndex, ClassInfo, EntityIndex, EntityProblem, MAP_HIDE, MAP_LOCK, MAP_OPEN};

/// The desktop's keep project: its QuakeC defines the ogre, the light, the door and the
/// player start.
fn keep() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/fixtures/keep")
}

/// A box brush from `min` to `max`, faced with `wall`.
fn cuboid(min: [i32; 3], max: [i32; 3]) -> String {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    format!(
        "{{\n( {x0} 0 0 ) ( {x0} 1 0 ) ( {x0} 0 1 ) wall 0 0 0 1 1\n\
         ( {x1} 0 0 ) ( {x1} 0 1 ) ( {x1} 1 0 ) wall 0 0 0 1 1\n\
         ( 0 {y0} 0 ) ( 0 {y0} 1 ) ( 1 {y0} 0 ) wall 0 0 0 1 1\n\
         ( 0 {y1} 0 ) ( 1 {y1} 0 ) ( 0 {y1} 1 ) wall 0 0 0 1 1\n\
         ( 0 0 {z0} ) ( 1 0 {z0} ) ( 0 1 {z0} ) wall 0 0 0 1 1\n\
         ( 0 0 {z1} ) ( 0 1 {z1} ) ( 1 0 {z1} ) wall 0 0 0 1 1\n}}\n"
    )
}

/// A room: a floor and a ceiling, from -128 to 128 across.
fn room() -> String {
    format!(
        "{}{}",
        cuboid([-128, -128, -16], [128, 128, 0]),
        cuboid([-128, -128, 128], [128, 128, 144])
    )
}

/// A map of `world_brushes` in the world, which names the keep's WAD, then `rest`.
fn map(world_brushes: &str, rest: &str) -> String {
    format!("{{\n\"classname\" \"worldspawn\"\n\"wad\" \"gfx/keep.wad\"\n{world_brushes}}}\n{rest}")
}

/// A player start, an entity every fixture but the start's has.
const START: &str = "{\n\"classname\" \"info_player_start\"\n\"origin\" \"0 0 24\"\n}\n";

/// A session whose project's folder is the keep's, with no entity check, and the map `text`
/// open, written to its own folder `name`.
fn session(name: &str, text: &str) -> Session<MapContext> {
    session_with(name, text, None)
}

/// [`session`] with `check` for the project's check of a map's entities.
fn session_with(
    name: &str,
    text: &str,
    check: Option<fn(&MapFile) -> Vec<EntityProblem>>,
) -> Session<MapContext> {
    let context = MapContext {
        project: TestProject {
            root: Some(keep()),
            check,
            ..TestProject::default()
        },
        ..MapContext::default()
    };
    let mut session = context.into_session();
    let dir = std::env::temp_dir()
        .join(format!("qs-wb-issues-{}", std::process::id()))
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.map");
    std::fs::write(&path, text).unwrap();
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    session
}

/// A project's check that knows the field `speed` and the class `monster_ogre`: a key `sped`,
/// at its line, and a classname `monster_orge` are problems, each with the edit that puts it
/// right.
fn ogre_check(map: &MapFile) -> Vec<EntityProblem> {
    let mut problems = Vec::new();
    for (place, entity) in map.entities.iter().enumerate() {
        let entity_index = EntityIndex::new(place);
        if let Some(pair) = entity.pairs().find(|pair| pair.key() == b"sped") {
            problems.push(EntityProblem {
                entity: entity_index,
                line: pair.line(),
                message: "sped is not a field; did you mean speed?".to_owned(),
                fix: Some(QuickFix::RenameKey {
                    entity: entity_index,
                    from: "sped".to_owned(),
                    to: "speed".to_owned(),
                }),
            });
        }
        if entity.classname() == Some(b"monster_orge".as_slice()) {
            problems.push(EntityProblem {
                entity: entity_index,
                line: entity.line(),
                message: "no spawn function monster_orge; did you mean monster_ogre?".to_owned(),
                fix: Some(QuickFix::SetClassname {
                    entity: entity_index,
                    classname: "monster_ogre".to_owned(),
                }),
            });
        }
    }
    problems
}

fn issues(session: &mut Session<MapContext>) -> Vec<MapIssue> {
    let found: MapIssues =
        serde_json::from_value(session.execute(MAP_ISSUES, json!({})).unwrap()).unwrap();
    found.issues
}

/// Applies the fix of the one issue whose message holds `message`, checks it is gone, undoes
/// it in one step and checks it is back; the fix.
fn fixes_and_undoes(session: &mut Session<MapContext>, message: &str) -> QuickFix {
    let before = issues(session);
    let issue = before
        .iter()
        .find(|issue| issue.message.contains(message))
        .unwrap_or_else(|| panic!("no issue {message} in {before:?}"));
    let fix = issue.fix.clone().unwrap();
    session.execute(MAP_FIX, json!({ "fix": fix })).unwrap();
    let after = issues(session);
    assert!(
        !after.iter().any(|issue| issue.message.contains(message)),
        "{message} is still there: {after:?}"
    );
    session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(issues(session), before, "one undo puts it back");
    fix
}

// No spec statement: P6-19: a brush with no volume is listed with the brush, and removing it
// clears the issue in one step that undoes
#[test]
fn an_empty_brush_is_removed() {
    let sliver = "{\n( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) wall 0 0 0 1 1\n\
                  ( 0 0 0 ) ( 0 0 1 ) ( 0 1 0 ) wall 0 0 0 1 1\n\
                  ( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) wall 0 0 0 1 1\n}\n";
    let mut session = session("empty", &map(&format!("{}{}", room(), sliver), START));
    let fix = fixes_and_undoes(&mut session, NO_VOLUME);
    assert_eq!(
        fix,
        QuickFix::RemoveBrush {
            entity: EntityIndex::new(0),
            brush: BrushIndex::new(2),
        }
    );
    let issue = issues(&mut session)
        .into_iter()
        .find(|issue| issue.message == NO_VOLUME)
        .unwrap();
    assert_eq!(
        (issue.entity, issue.brush),
        (Some(EntityIndex::new(0)), Some(BrushIndex::new(2)))
    );
}

// No spec statement: P6-19, MAP-8.2: a problem the project's check finds with a key is listed
// with its fix, which renames the key in one step that undoes
#[test]
fn a_misspelled_key_is_renamed() {
    let ogre = "{\n\"classname\" \"monster_ogre\"\n\"origin\" \"0 0 24\"\n\"sped\" \"200\"\n}\n";
    let text = map(&room(), &format!("{START}{ogre}"));
    let mut session = session_with("key", &text, Some(ogre_check));
    let fix = fixes_and_undoes(&mut session, "sped");
    assert_eq!(
        fix,
        QuickFix::RenameKey {
            entity: EntityIndex::new(2),
            from: "sped".to_owned(),
            to: "speed".to_owned(),
        }
    );
}

// No spec statement: P6-19, MAP-8.1: a problem the project's check finds with a classname is
// listed with its fix, which sets the class in one step that undoes
#[test]
fn a_misspelled_class_is_set() {
    let ogre = "{\n\"classname\" \"monster_orge\"\n\"origin\" \"0 0 24\"\n}\n";
    let text = map(&room(), &format!("{START}{ogre}"));
    let mut session = session_with("class", &text, Some(ogre_check));
    let fix = fixes_and_undoes(&mut session, "monster_orge");
    assert_eq!(
        fix,
        QuickFix::SetClassname {
            entity: EntityIndex::new(2),
            classname: "monster_ogre".to_owned(),
        }
    );
}

// No spec statement: P6-19: a map with no player start gets one above the floor in the middle
// of the world, in one step that undoes; a world whose middle is solid gets no fix
#[test]
fn a_missing_start_is_placed() {
    let mut open = session("start", &map(&room(), ""));
    let fix = fixes_and_undoes(&mut open, NO_START);
    assert_eq!(
        fix,
        QuickFix::AddPlayerStart {
            origin: [0.0, 0.0, 32.0]
        }
    );
    let mut solid = session("solid", &map(&cuboid([-64, -64, -64], [64, 64, 64]), ""));
    let issue = issues(&mut solid)
        .into_iter()
        .find(|issue| issue.message == NO_START)
        .unwrap();
    assert_eq!(issue.fix, None);
    let mut bare = session("bare", &map("", ""));
    assert!(
        !issues(&mut bare)
            .iter()
            .any(|issue| issue.message == NO_START),
        "a world with no brushes is no map to play"
    );
}

// No spec statement: P6-19: a format diagnostic is listed with the entity and brush its line
// is in, and a texture no WAD holds is listed for the map
#[test]
fn format_and_texture_issues_are_placed() {
    let odd = room().replacen("wall 0 0 0 1 1", "nowhere 0.5 0 0 1 1", 1);
    let mut session = session("format", &map(&odd, START));
    let found = issues(&mut session);
    let fraction = found
        .iter()
        .find(|issue| issue.line == 5)
        .unwrap_or_else(|| panic!("{found:?}"));
    assert_eq!(
        (fraction.entity, fraction.brush),
        (Some(EntityIndex::new(0)), Some(BrushIndex::new(0)))
    );
    assert!(
        found
            .iter()
            .any(|issue| issue.message.contains("nowhere") && issue.entity.is_none()),
        "{found:?}"
    );
}

// No spec statement: L-40: that the world names no WAD is an issue only when a texture is
// missing: a map whose textures are all found, as one textured from the game's levels is, has
// nothing to fix, and one with a texture found nowhere is told of both
#[test]
fn no_wad_is_an_issue_only_with_a_texture_missing() {
    const NO_WAD: &str = "the world names no WAD";
    let named = |found: &[MapIssue]| found.iter().any(|issue| issue.message == NO_WAD);
    // No WAD and no face: nothing is missing.
    let bare = format!("{{\n\"classname\" \"worldspawn\"\n}}\n{START}");
    let mut bare_session = session("no-wad-bare", &bare);
    assert!(!named(&issues(&mut bare_session)));
    // No WAD and faces whose texture is nowhere: both are said.
    let unwadded = map(&room(), START).replace("\"wad\" \"gfx/keep.wad\"\n", "");
    let mut session = session("no-wad-missing", &unwadded);
    let found = issues(&mut session);
    assert!(named(&found), "{found:?}");
    assert!(
        found.iter().any(|issue| issue.message.contains("wall")),
        "{found:?}"
    );
}

/// A session of the map `text`, whose project gives the ogre a box 64 across.
fn search_session(text: &str) -> Session<MapContext> {
    let context = MapContext {
        project: TestProject {
            root: Some(keep()),
            classes: vec![ClassInfo {
                name: "monster_ogre".to_owned(),
                found: "documented".to_owned(),
                shape: "point".to_owned(),
                color: None,
                size: Some([[-32.0, -32.0, -24.0], [32.0, 32.0, 64.0]]),
                model: None,
                doc: Vec::new(),
                keys: Vec::new(),
                spawnflags: Vec::new(),
            }],
            ..TestProject::default()
        },
        ..MapContext::default()
    };
    let mut session = context.into_session();
    let dir = std::env::temp_dir()
        .join(format!("qs-wb-issues-{}", std::process::id()))
        .join("search");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.map");
    std::fs::write(&path, text).unwrap();
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    session
}

fn search(session: &mut Session<MapContext>, query: &str) -> MapFound {
    serde_json::from_value(
        session
            .execute(MAP_SEARCH, json!({ "query": query }))
            .unwrap(),
    )
    .unwrap()
}

// No spec statement: P7-08: map.search finds what TrenchBroom's search finds: text in
// classnames, keys and values, textures and layer and group names; a query by its fields;
// brushes by their entity and textures; faces alone by texture and normal; a layer by what it
// holds; a box from the class's size; nothing hidden
#[test]
fn search_by_text_and_query() {
    let ogre =
        "{\n\"classname\" \"monster_ogre\"\n\"origin\" \"64 0 24\"\n\"target\" \"gate\"\n}\n";
    let door = format!(
        "{{\n\"classname\" \"func_door\"\n\"targetname\" \"gate\"\n{}}}\n",
        cuboid([96, -16, 0], [112, 16, 96]).replace("wall", "door")
    );
    let lights = "{\n\"classname\" \"light\"\n\"origin\" \"0 0 64\"\n\"light\" \"400\"\n}\n\
        {\n\"classname\" \"light\"\n\"origin\" \"0 0 96\"\n\"light\" \"bright\"\n}\n";
    let layer = "{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_layer\"\n\"_tb_name\" \"Details\"\n\
        \"_tb_id\" \"1\"\n}\n\
        {\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\"_tb_name\" \"Hallway\"\n\
        \"_tb_id\" \"2\"\n\"_tb_layer\" \"1\"\n}\n\
        {\n\"classname\" \"info_null\"\n\"origin\" \"500 0 0\"\n\"_tb_group\" \"2\"\n}\n";
    let rest = format!("{}{}{}{}{}", START, ogre, door, lights, layer);
    let mut session = search_session(&map(&room(), &rest));
    let e = EntityIndex::new;
    let b = BrushIndex::new;
    let entities = |session: &mut Session<MapContext>, query: &str| search(session, query).entities;
    assert_eq!(entities(&mut session, "OGRE"), [e(2)]);
    assert_eq!(entities(&mut session, "gate"), [e(2), e(3)]);
    assert_eq!(entities(&mut session, "monster_*"), [e(2)]);
    assert_eq!(
        entities(&mut session, "hallway"),
        [e(7), e(8)],
        "by its name, by its group's"
    );
    assert_eq!(
        entities(&mut session, "properties.targetname is \"gate\""),
        [e(3)]
    );
    assert_eq!(
        entities(
            &mut session,
            "classname is \"light\" && properties.light > 300"
        ),
        [e(4)],
        "a light whose light is no number does not match"
    );
    assert_eq!(
        entities(&mut session, "properties contains \"target\""),
        [e(2)]
    );
    let doors = search(&mut session, "materials like \"DOOR\"");
    assert_eq!(
        (doors.entities, doors.brushes),
        (vec![], vec![(e(3), b(0))])
    );
    assert_eq!(
        search(&mut session, "entity.classname is \"func_door\"").brushes,
        [(e(3), b(0))]
    );
    assert_eq!(
        search(&mut session, "tags contains \"Func\"").brushes,
        [(e(3), b(0))]
    );
    let floors = search(&mut session, "material like \"wall\" && normal.z > 0.7");
    assert!(floors.entities.is_empty() && floors.brushes.is_empty());
    assert_eq!(floors.faces.len(), 2, "the floor's top and the ceiling's");
    assert!(floors.faces.iter().all(|face| face.entity == e(0)));
    assert_eq!(
        search(&mut session, "material like \"door\"").faces.len(),
        6
    );
    let near = search(
        &mut session,
        "bounds intersects bbox(vec(32, -32, 0), vec(100, 32, 64))",
    );
    assert_eq!(near.entities, [e(2), e(3)]);
    assert_eq!(near.brushes, [(e(0), b(0)), (e(3), b(0))]);
    assert_eq!(
        entities(&mut session, "bounds.max.z is 88"),
        [e(2)],
        "the ogre's class box: 24 up from 64"
    );
    assert_eq!(
        entities(&mut session, "center distanceTo vec(0, 0, 64) < 1"),
        [e(4)],
        "a light of no known class is 16 across"
    );
    let details = search(&mut session, "type is layer && name is \"Details\"");
    assert_eq!((details.entities, details.brushes), (vec![e(7)], vec![]));
    let default = search(&mut session, "type is layer && name is \"Default Layer\"");
    assert_eq!(default.entities, [e(1), e(2), e(3), e(4), e(5)]);
    assert_eq!(default.brushes, [(e(0), b(0)), (e(0), b(1))]);
    assert_eq!(entities(&mut session, "groupName is \"Hallway\""), [e(8)]);
    assert_eq!(
        entities(&mut session, "layerName is \"Details\""),
        [e(7), e(8)]
    );
    assert_eq!(
        entities(&mut session, "type is group && name like \"Hall*\""),
        [e(7)]
    );
    assert_eq!(
        entities(&mut session, "!(classname like \"info_*\")"),
        [e(2), e(3), e(4), e(5)]
    );
    for nothing in ["", "light is red", "\"\"", "true", "properties.message"] {
        assert_eq!(
            search(&mut session, nothing),
            MapFound::default(),
            "{nothing}"
        );
    }
    session
        .execute(MAP_HIDE, json!({ "entities": [2] }))
        .unwrap();
    assert!(
        entities(&mut session, "ogre").is_empty(),
        "nothing hidden is found"
    );
    session
        .execute(MAP_LOCK, json!({ "entities": [3] }))
        .unwrap();
    assert!(
        entities(&mut session, "gate").is_empty(),
        "nor anything locked"
    );
    assert!(
        search(&mut session, "materials like \"door\"")
            .brushes
            .is_empty()
    );
}

/// Runs `fix` and gives its error's text, checking the map is unchanged.
fn refused(session: &mut Session<MapContext>, fix: &QuickFix) -> String {
    let before = issues(session);
    let error = session
        .execute(MAP_FIX, json!({ "fix": fix }))
        .unwrap_err()
        .to_string();
    assert_eq!(issues(session), before, "a refused fix changes nothing");
    error
}

// No spec statement: P6-19: a fix that no longer fits the map is refused and changes nothing:
// a brush with volume is not removed, a key that is not there is not renamed, and a brush or
// entity that is not there is named
#[test]
fn stale_fixes_are_refused() {
    let mut session = session("stale", &map(&room(), START));
    let (world, start) = (EntityIndex::new(0), EntityIndex::new(1));
    let solid = QuickFix::RemoveBrush {
        entity: world,
        brush: BrushIndex::new(0),
    };
    assert!(refused(&mut session, &solid).contains("has volume"));
    let gone = QuickFix::RemoveBrush {
        entity: world,
        brush: BrushIndex::new(9),
    };
    assert!(refused(&mut session, &gone).contains("no brush 9"));
    let rename = QuickFix::RenameKey {
        entity: start,
        from: "sped".to_owned(),
        to: "speed".to_owned(),
    };
    assert!(refused(&mut session, &rename).contains("no key sped"));
    let nowhere = QuickFix::SetClassname {
        entity: EntityIndex::new(9),
        classname: "light".to_owned(),
    };
    assert!(refused(&mut session, &nowhere).contains('9'));
}

// No spec statement: P6-19: a key written after a brush is placed with its entity, not the
// brush; and a brush with no volume does not keep the player start's fix from being offered
#[test]
fn pairs_after_brushes_and_empty_brushes() {
    let door = format!(
        "{{\n\"classname\" \"func_door\"\n{}\"sped\" \"100\"\n}}\n",
        cuboid([96, -16, 0], [112, 16, 96])
    );
    let text = map(&room(), &format!("{START}{door}"));
    let mut doors = session_with("pairs", &text, Some(ogre_check));
    let found = issues(&mut doors);
    let sped = found
        .iter()
        .find(|issue| issue.message.contains("sped"))
        .unwrap_or_else(|| panic!("{found:?}"));
    assert_eq!((sped.entity, sped.brush), (Some(EntityIndex::new(2)), None));
    let sliver = "{\n( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) wall 0 0 0 1 1\n}\n";
    let mut open = session("sliver", &map(&format!("{}{}", room(), sliver), ""));
    let start = issues(&mut open)
        .into_iter()
        .find(|issue| issue.message == NO_START)
        .unwrap();
    assert!(start.fix.is_some(), "the empty brush holds no point");
}
