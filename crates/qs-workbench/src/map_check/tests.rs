// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::PathBuf;

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::{Value, json};

use qs_map::MapFile;

use super::{MAP_CHECK, MAP_RENAME_KEY};
use crate::map_compile::tests::{Context, open};
use crate::{Config, MapProject, OPEN_FILE, OPEN_PROJECT, READ_FILE, RENAME, Workbench, register};
use qs_map_edit::{EntityIndex, QuickFix};

const CODE: &str = "entity self; .string classname, target, targetname; .float speed;\n\
    void() worldspawn = {};\n\
    /*QUAKED func_door (0 .5 .8) ?\n*/\n\
    void() func_door = { if (!self.speed) self.speed = 100; };\n";

const MAP: &str = "{\n\"classname\" \"worldspawn\"\n\"wad\" \"base.wad\"\n}\n\
    {\n\"classname\" \"func_door\"\n\"sped\" \"50\"\n}\n\
    {\n\"classname\" \"func_door\"\n\"speed\"   \"75\"\n}\n";

/// A project in a fresh directory named `name` with [`CODE`] and `maps/test.map` holding
/// [`MAP`], open.
fn project(name: &str) -> (PathBuf, Session<Workbench<FsLoader>>) {
    let dir = std::env::temp_dir().join(format!("qs-wb-map-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(dir.join("a.qc"), CODE).unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\na.qc\n").unwrap();
    std::fs::write(dir.join("maps/test.map"), MAP).unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute(OPEN_PROJECT, json!({ "path": dir }))
        .unwrap();
    (dir, session)
}

// spec MAP-8.2: map.check reports a misspelled key of a map with the field to write instead
#[test]
fn a_misspelled_key_in_a_map_is_reported_with_a_fix() {
    let (dir, mut session) = project("check");
    let checked = session
        .execute(MAP_CHECK, json!({ "path": dir.join("maps/test.map") }))
        .unwrap();
    assert_eq!(
        checked["diagnostics"],
        json!([{
            "line": 7,
            "message": "sped is not a field, so the game ignores it; did you mean speed?",
            "suggestion": "speed",
        }])
    );
    let missing = session.execute(MAP_CHECK, json!({ "path": dir.join("maps/none.map") }));
    assert!(missing.is_err());
}

// spec MAP-8.7: renaming a field warns that a map may name it; once confirmed, the key is
// renamed in the open maps with its value and formatting kept, unsaved, and undo puts it back
#[test]
fn renaming_a_field_rewrites_the_key_in_open_maps_after_confirmation() {
    let (dir, mut session) = project("rename");
    let map = dir.join("maps").join("test.map");
    session.execute(OPEN_FILE, json!({ "path": map })).unwrap();
    let offset = CODE.find("speed").unwrap();
    let renamed = session
        .execute(
            RENAME,
            json!({ "path": dir.join("a.qc"), "offset": offset, "name": "pace" }),
        )
        .unwrap();
    assert_eq!(renamed["caution"], json!("speed"), "a map may name it");
    let text = |session: &mut Session<Workbench<FsLoader>>| -> String {
        let read: Value = session.execute(READ_FILE, json!({ "path": map })).unwrap();
        read["text"].as_str().unwrap().to_owned()
    };
    assert_eq!(
        text(&mut session),
        MAP,
        "nothing changes before confirmation"
    );
    let done = session
        .execute(MAP_RENAME_KEY, json!({ "from": "speed", "to": "pace" }))
        .unwrap();
    assert_eq!(done["maps"], json!([{ "path": map, "pairs": 1 }]));
    assert_eq!(
        text(&mut session),
        MAP.replace("\"speed\"   ", "\"pace\"   ")
    );
    assert_eq!(std::fs::read_to_string(&map).unwrap(), MAP, "unsaved");
    session.undo().unwrap();
    assert_eq!(text(&mut session), MAP);
    let none = session
        .execute(MAP_RENAME_KEY, json!({ "from": "nonesuch", "to": "x" }))
        .unwrap();
    assert_eq!(none["maps"], json!([]));
    let bad = session.execute(MAP_RENAME_KEY, json!({ "from": "speed", "to": "a\"b" }));
    assert!(bad.is_err(), "a key cannot hold a quote");
}

/// A workbench with a project of `code` open, in a fresh directory named `name`.
fn workbench(name: &str, code: &str) -> Workbench<FsLoader> {
    let dir = std::env::temp_dir().join(format!("qs-wb-seam-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.qc"), code).unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\na.qc\n").unwrap();
    let mut workbench = Workbench::new(FsLoader, Config::default());
    workbench.open_project(&dir).unwrap();
    workbench
}

// No spec statement: P6-40, MAP-4: the workbench answers the map editor's questions of the
// project from its server QuakeC: a class with its keys typed and its spawnflags named, none
// for a classname the code does not define, and the keys the code fires entities by
#[test]
fn the_project_answers_the_map_editor_from_its_quakec() {
    let code = "entity self, world; .string classname, target, killtarget, targetname;\n\
        .float spawnflags, speed; float SILENT = 2;\n\
        entity(entity start, .string fld, string match) find = #18;\n\
        void() worldspawn = {};\n\
        void() fire = { local entity t; t = find(world, targetname, self.target);\n\
            t = find(world, targetname, self.killtarget); };\n\
        /*QUAKED path_corner (0.5 0.25 0) (-8 -8 -8) (8 8 8) START_ON\n*/\n\
        void() path_corner = { if (!self.speed) self.speed = 100;\n\
            if (self.spawnflags & SILENT) fire(); };\n";
    let mut workbench = workbench("classes", code);
    assert!(MapProject::project_root(&workbench).is_some());
    assert_eq!(
        MapProject::targeting_keys(&mut workbench),
        ["killtarget", "target"]
    );
    let class = workbench.entity_class("path_corner").unwrap();
    assert_eq!(
        serde_json::to_value(&class).unwrap(),
        json!({
            "name": "path_corner",
            "found": "documented",
            "shape": "point",
            "color": [0.5, 0.25, 0.0],
            "size": [[-8.0, -8.0, -8.0], [8.0, 8.0, 8.0]],
            "model": null,
            "doc": [],
            "keys": [
                { "name": "killtarget", "ty": "string", "default": null },
                { "name": "speed", "ty": "float", "default": "100" },
                { "name": "target", "ty": "string", "default": null },
            ],
            "spawnflags": [
                { "value": 1, "name": "START_ON" },
                { "value": 2, "name": "SILENT" },
            ],
        })
    );
    assert_eq!(workbench.entity_class("worldspawn"), None);
    assert_eq!(workbench.entity_class("func_nothing"), None);
}

// No spec statement: P6-40: with no project open the workbench has no folder, no class, no
// targeting key and no entity check to give the map editor
#[test]
fn no_project_answers_the_map_editor_nothing() {
    let mut workbench = Workbench::new(FsLoader, Config::default());
    assert_eq!(MapProject::project_root(&workbench), None);
    assert_eq!(workbench.entity_class("path_corner"), None);
    assert!(MapProject::targeting_keys(&mut workbench).is_empty());
    assert!(workbench.entity_check().is_none());
}

// spec MAP-8.2: the entity check the workbench gives the map editor finds a misspelled key and
// a misspelled class of a map, each with the edit that puts it right, and a problem with no
// suggestion with none
#[test]
fn the_entity_check_gives_problems_with_their_fixes() {
    let mut workbench = workbench("check", CODE);
    let check = workbench.entity_check().unwrap();
    let map = MapFile::parse(
        b"{\n\"classname\" \"worldspawn\"\n}\n\
          {\n\"classname\" \"func_door\"\n\"sped\" \"50\"\n}\n\
          {\n\"classname\" \"func_dor\"\n}\n\
          {\n\"origin\" \"0 0 0\"\n}\n",
    )
    .unwrap();
    let problems = check(&map);
    let fixes: Vec<_> = problems
        .iter()
        .map(|problem| (problem.entity.get(), problem.line, problem.fix.clone()))
        .collect();
    assert_eq!(
        fixes,
        [
            (
                1,
                6,
                Some(QuickFix::RenameKey {
                    entity: EntityIndex::new(1),
                    from: "sped".to_owned(),
                    to: "speed".to_owned(),
                })
            ),
            (
                2,
                9,
                Some(QuickFix::SetClassname {
                    entity: EntityIndex::new(2),
                    classname: "func_door".to_owned(),
                })
            ),
            (3, 11, None),
            (3, 12, None),
        ],
        "{problems:?}"
    );
    assert!(problems.iter().all(|problem| !problem.message.is_empty()));
}

/// The keep project open with a map of a floor, a player start and `entity`, through
/// `map.open`; the map's issues as `map.issues` lists them, from the keep's real QuakeC.
fn keep_issues(name: &str, entity: &str) -> (Session<Context>, Vec<qs_map_edit::MapIssue>) {
    let keep =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/fixtures/keep");
    let dir = std::env::temp_dir().join(format!("qs-wb-keep-issues-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.map");
    let floor = "{\n( -128 0 0 ) ( -128 1 0 ) ( -128 0 1 ) wall 0 0 0 1 1\n\
        ( 128 0 0 ) ( 128 0 1 ) ( 128 1 0 ) wall 0 0 0 1 1\n\
        ( 0 -128 0 ) ( 0 -128 1 ) ( 1 -128 0 ) wall 0 0 0 1 1\n\
        ( 0 128 0 ) ( 1 128 0 ) ( 0 128 1 ) wall 0 0 0 1 1\n\
        ( 0 0 -16 ) ( 1 0 -16 ) ( 0 1 -16 ) wall 0 0 0 1 1\n\
        ( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) wall 0 0 0 1 1\n}\n";
    let text = format!(
        "{{\n\"classname\" \"worldspawn\"\n\"wad\" \"gfx/keep.wad\"\n{floor}}}\n\
         {{\n\"classname\" \"info_player_start\"\n\"origin\" \"0 0 24\"\n}}\n{entity}"
    );
    std::fs::write(&path, text).unwrap();
    // With a game, as the window has, which map.issues needs; the project opens after the map,
    // which it leaves open.
    let mut session = open(&path);
    session
        .execute(OPEN_PROJECT, json!({ "path": keep }))
        .unwrap();
    let issues = map_issues(&mut session);
    (session, issues)
}

fn map_issues(session: &mut Session<Context>) -> Vec<qs_map_edit::MapIssue> {
    let found: qs_map_edit::MapIssues =
        serde_json::from_value(session.execute(qs_map_edit::MAP_ISSUES, json!({})).unwrap())
            .unwrap();
    found.issues
}

/// The fix `map.issues` gives for the issue naming `name`, applied through `map.fix`: the
/// issue is gone after it.
fn fixed(session: &mut Session<Context>, issues: &[qs_map_edit::MapIssue], name: &str) -> QuickFix {
    let fix = issues
        .iter()
        .find(|issue| issue.message.contains(name))
        .and_then(|issue| issue.fix.clone())
        .unwrap();
    session
        .execute(qs_map_edit::MAP_FIX, json!({ "fix": fix }))
        .unwrap();
    assert!(
        !map_issues(session)
            .iter()
            .any(|issue| issue.message.contains(name))
    );
    fix
}

// spec MAP-8.2: through the workbench, map.issues finds a misspelled key from the project's
// QuakeC, and map.fix renames it to the field it is closest to
#[test]
fn the_workbench_s_check_reaches_map_issues_for_a_key() {
    let ogre = "{\n\"classname\" \"monster_ogre\"\n\"origin\" \"0 0 24\"\n\"sped\" \"200\"\n}\n";
    let (mut session, issues) = keep_issues("key", ogre);
    assert_eq!(
        fixed(&mut session, &issues, "sped"),
        QuickFix::RenameKey {
            entity: EntityIndex::new(2),
            from: "sped".to_owned(),
            to: "speed".to_owned(),
        }
    );
}

// spec MAP-8.1: through the workbench, map.issues finds a misspelled classname from the
// project's QuakeC, and map.fix sets the class it is closest to
#[test]
fn the_workbench_s_check_reaches_map_issues_for_a_class() {
    let ogre = "{\n\"classname\" \"monster_orge\"\n\"origin\" \"0 0 24\"\n}\n";
    let (mut session, issues) = keep_issues("class", ogre);
    assert_eq!(
        fixed(&mut session, &issues, "monster_orge"),
        QuickFix::SetClassname {
            entity: EntityIndex::new(2),
            classname: "monster_ogre".to_owned(),
        }
    );
}
