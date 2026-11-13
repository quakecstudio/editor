// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code: a failed unwrap or a missing issue is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::json;

use super::{
    MAP_FIX, MAP_ISSUES, MAP_SEARCH, MapFound, MapIssue, MapIssues, NO_START, NO_VOLUME, QuickFix,
};
use crate::map_edit::{BrushIndex, EntityIndex, MAP_OPEN};
use crate::preview::{HasPreview, Preview, PreviewConfig};
use crate::{Config, HasWorkbench, Workbench, register, register_preview};

struct Context {
    workbench: Workbench<FsLoader>,
    preview: Preview,
}

impl HasWorkbench for Context {
    type Loader = FsLoader;

    fn workbench(&mut self) -> &mut Workbench<FsLoader> {
        &mut self.workbench
    }

    fn preview(&mut self) -> Option<&mut Preview> {
        Some(&mut self.preview)
    }
}

impl HasPreview for Context {
    fn workbench_and_preview(&mut self) -> (&mut Workbench<FsLoader>, &mut Preview) {
        (&mut self.workbench, &mut self.preview)
    }
}

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
    cuboid([-128, -128, -16], [128, 128, 0]) + &cuboid([-128, -128, 128], [128, 128, 144])
}

/// A map of `world_brushes` in the world, which names the keep's WAD, then `rest`.
fn map(world_brushes: &str, rest: &str) -> String {
    format!("{{\n\"classname\" \"worldspawn\"\n\"wad\" \"gfx/keep.wad\"\n{world_brushes}}}\n{rest}")
}

/// A player start, an entity every fixture but the start's has.
const START: &str = "{\n\"classname\" \"info_player_start\"\n\"origin\" \"0 0 24\"\n}\n";

/// A session with the keep project and the map `text` open, written to its own folder `name`.
fn session(name: &str, text: &str) -> Session<Context> {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    register_preview(&mut registry).unwrap();
    let context = Context {
        workbench: Workbench::new(FsLoader, Config::default()),
        preview: Preview::new(PreviewConfig {
            host: None,
            app: Path::new("/nowhere/app").to_path_buf(),
            game: None,
            extra: Vec::new(),
        }),
    };
    let mut session = Session::new(context, registry);
    session
        .execute("project.open", json!({ "path": keep() }))
        .unwrap();
    let dir = std::env::temp_dir()
        .join(format!("qs-wb-issues-{}", std::process::id()))
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.map");
    std::fs::write(&path, text).unwrap();
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    session
}

fn issues(session: &mut Session<Context>) -> Vec<MapIssue> {
    let found: MapIssues =
        serde_json::from_value(session.execute(MAP_ISSUES, json!({})).unwrap()).unwrap();
    found.issues
}

/// Applies the fix of the one issue whose message holds `message`, checks it is gone, undoes
/// it in one step and checks it is back; the fix.
fn fixes_and_undoes(session: &mut Session<Context>, message: &str) -> QuickFix {
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
    let mut session = session("empty", &map(&(room() + sliver), START));
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

// spec MAP-8.2: a misspelled key's fix renames it to the field it is closest to, in one step
// that undoes
#[test]
fn a_misspelled_key_is_renamed() {
    let ogre = "{\n\"classname\" \"monster_ogre\"\n\"origin\" \"0 0 24\"\n\"sped\" \"200\"\n}\n";
    let mut session = session("key", &map(&room(), &(START.to_owned() + ogre)));
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

// spec MAP-8.1: a misspelled classname's fix sets the class it is closest to, in one step that
// undoes
#[test]
fn a_misspelled_class_is_set() {
    let ogre = "{\n\"classname\" \"monster_orge\"\n\"origin\" \"0 0 24\"\n}\n";
    let mut session = session("class", &map(&room(), &(START.to_owned() + ogre)));
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

fn search(session: &mut Session<Context>, params: serde_json::Value) -> MapFound {
    serde_json::from_value(session.execute(MAP_SEARCH, params).unwrap()).unwrap()
}

// No spec statement: P6-19: the search finds entities by class, key and value and brushes by
// texture and box, every part given, in any case
#[test]
fn search_by_class_key_texture_and_box() {
    let ogre =
        "{\n\"classname\" \"monster_ogre\"\n\"origin\" \"64 0 24\"\n\"target\" \"gate\"\n}\n";
    let door = format!(
        "{{\n\"classname\" \"func_door\"\n\"targetname\" \"gate\"\n{}}}\n",
        cuboid([96, -16, 0], [112, 16, 96]).replace("wall", "door")
    );
    let rest = START.to_owned() + ogre + &door;
    let mut session = session("search", &map(&room(), &rest));
    let e = EntityIndex::new;
    let b = BrushIndex::new;
    assert_eq!(
        search(&mut session, json!({ "classname": "OGRE" })).entities,
        vec![e(2)]
    );
    assert_eq!(
        search(&mut session, json!({ "value": "gate" })).entities,
        vec![e(2), e(3)]
    );
    assert_eq!(
        search(
            &mut session,
            json!({ "key": "targetname", "value": "gate" })
        )
        .entities,
        vec![e(3)]
    );
    let doors = search(&mut session, json!({ "texture": "DOOR" }));
    assert_eq!(
        (doors.entities, doors.brushes),
        (vec![e(3)], vec![(e(3), b(0))])
    );
    let near = search(
        &mut session,
        json!({ "within": [[32.0, -32.0, 0.0], [100.0, 32.0, 64.0]] }),
    );
    assert_eq!(near.entities, vec![e(0), e(2), e(3)]);
    assert_eq!(
        near.brushes,
        vec![(e(0), b(0)), (e(3), b(0))],
        "the floor's top meets the box; the ceiling does not"
    );
    let everything = search(&mut session, json!({}));
    assert_eq!(everything.entities.len(), 4);
    assert!(everything.brushes.is_empty());
}

/// Runs `fix` and gives its error's text, checking the map is unchanged.
fn refused(session: &mut Session<Context>, fix: &QuickFix) -> String {
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
    let mut doors = session("pairs", &map(&room(), &(START.to_owned() + &door)));
    let found = issues(&mut doors);
    let sped = found
        .iter()
        .find(|issue| issue.message.contains("sped"))
        .unwrap_or_else(|| panic!("{found:?}"));
    assert_eq!((sped.entity, sped.brush), (Some(EntityIndex::new(2)), None));
    let sliver = "{\n( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) wall 0 0 0 1 1\n}\n";
    let mut open = session("sliver", &map(&(room() + sliver), ""));
    let start = issues(&mut open)
        .into_iter()
        .find(|issue| issue.message == NO_START)
        .unwrap();
    assert!(start.fix.is_some(), "the empty brush holds no point");
}
