// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::PathBuf;

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::json;

use super::PROJECT_ENTITIES;
use crate::{Config, OPEN_PROJECT, Workbench, register};

/// A session with no project open.
fn session() -> Session<Workbench<FsLoader>> {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    Session::new(Workbench::new(FsLoader, Config::default()), registry)
}

/// A project in a fresh directory named `name`, with one file, `a.qc`, holding `text`.
fn project(name: &str, text: &str) -> (PathBuf, Session<Workbench<FsLoader>>) {
    let dir = std::env::temp_dir().join(format!("qs-wb-entities-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.qc"), text).unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\na.qc\n").unwrap();
    let mut session = session();
    session
        .execute(OPEN_PROJECT, json!({ "path": dir }))
        .unwrap();
    (dir, session)
}

// No spec statement: P5-03: project.entities answers each class of the project's server
// QuakeC with its keys, defaults and spawnflags, and with fgd writes the FGD file too
#[test]
fn project_entities_lists_the_classes_and_writes_an_fgd() {
    let (dir, mut session) = project(
        "fgd",
        "entity self; .float spawnflags, wait; .string target;\n\
         /*QUAKED trigger_thing (.5 .5 .5) ? ONCE\nFires its targets.\n*/\n\
         void() trigger_thing = { if (!self.wait) self.wait = 0.2; \
         if (self.spawnflags & 1) self.wait = -1; if (self.target) self.wait = 1; };\n",
    );
    let fgd = dir.join("thing.fgd");
    let found = session
        .execute(PROJECT_ENTITIES, json!({ "fgd": fgd }))
        .unwrap();
    let classes = found["classes"].as_array().unwrap();
    assert_eq!(classes.len(), 1, "{found}");
    let class = &classes[0];
    assert_eq!(class["name"], "trigger_thing");
    assert_eq!(class["found"], "documented");
    assert_eq!(class["shape"], "brush");
    assert_eq!(class["doc"], json!(["Fires its targets."]));
    assert_eq!(
        class["keys"],
        json!([
            { "name": "target", "ty": "string", "default": null },
            { "name": "wait", "ty": "float", "default": "0.2" },
        ])
    );
    assert_eq!(class["spawnflags"], json!([{ "value": 1, "name": "ONCE" }]));
    let written = std::fs::read_to_string(&fgd).unwrap();
    assert!(
        written.starts_with("@SolidClass color(128 128 128) = trigger_thing : \"Fires"),
        "{written}"
    );
    assert!(
        written.contains("\twait(float) : \"wait\" : \"0.2\"\n"),
        "{written}"
    );
}

// No spec statement: P5-03: without fgd nothing is written; with no project open the command
// is refused, and an FGD path that cannot be written is an error
#[test]
fn project_entities_errors() {
    assert!(session().execute(PROJECT_ENTITIES, json!({})).is_err());
    let (dir, mut session) = project("errors", "void() main = { };\n");
    let found = session.execute(PROJECT_ENTITIES, json!({})).unwrap();
    assert_eq!(found["classes"], json!([]));
    assert_eq!(found["fgd"], json!(null));
    let missing = dir.join("no-such-dir").join("x.fgd");
    assert!(
        session
            .execute(PROJECT_ENTITIES, json!({ "fgd": missing }))
            .is_err()
    );
}
