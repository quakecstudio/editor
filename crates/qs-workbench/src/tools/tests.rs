// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::Path;

use qc_engines::Engine;
use qc_project::{MemoryLoader, ModuleKind};
use qs_commands::{Registry, Session};
use serde_json::{Value, json};

use super::{ENGINE_BUILTINS, PROJECT_SYMBOLS, builtins_of};
use crate::{Config, OPEN_PROJECT, Workbench, register};

const ROOT: &str = "/p";

fn session() -> Session<Workbench<MemoryLoader>> {
    let mut loader = MemoryLoader::new();
    for (name, text) in [
        ("progs.src", "progs.dat\ndefs.qc\nworld.qc\n"),
        ("defs.qc", ".float health;\nfloat speed = 300;\n"),
        (
            "world.qc",
            "void() worldspawn = {};\nvoid() W_FireAxe = {};\n",
        ),
    ] {
        loader.insert(Path::new(ROOT).join(name), text.as_bytes().to_vec());
    }
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    Session::new(Workbench::new(loader, Config::default()), registry)
}

// No spec statement: P4-03, project.symbols lists each global definition with where its name
// is written, a query keeping those whose name contains it without case
#[test]
fn project_symbols_lists_the_definitions() {
    let mut session = session();
    assert!(
        session.execute(PROJECT_SYMBOLS, json!({})).is_err(),
        "no project"
    );
    session
        .execute(OPEN_PROJECT, json!({ "path": ROOT }))
        .unwrap();
    let all = session.execute(PROJECT_SYMBOLS, json!({})).unwrap();
    let names: Vec<&str> = all
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    for name in ["health", "speed", "worldspawn", "W_FireAxe"] {
        assert!(names.contains(&name), "{names:?}");
    }
    let axe = session
        .execute(PROJECT_SYMBOLS, json!({ "query": "fireaxe" }))
        .unwrap();
    assert_eq!(axe.as_array().unwrap().len(), 1);
    assert_eq!(axe[0]["line"], 2);
    assert_eq!(axe[0]["column"], 8);
    assert!(axe[0]["path"].as_str().unwrap().ends_with("world.qc"));
}

// No spec statement: P4-03, engine.builtins lists a named engine's table, or the open
// project's engine's; an unknown engine or kind is refused
#[test]
fn engine_builtins_lists_the_table() {
    let mut session = session();
    assert!(
        session.execute(ENGINE_BUILTINS, json!({})).is_err(),
        "no project, no engine"
    );
    let vanilla = session
        .execute(ENGINE_BUILTINS, json!({ "engine": "vanilla" }))
        .unwrap();
    assert_eq!(vanilla["engine"], "vanilla");
    let first: &Value = &vanilla["builtins"][0];
    assert_eq!(first["number"], 1);
    assert_eq!(first["name"], "makevectors");
    session
        .execute(OPEN_PROJECT, json!({ "path": ROOT }))
        .unwrap();
    let project = session.execute(ENGINE_BUILTINS, json!({})).unwrap();
    assert!(!project["builtins"].as_array().unwrap().is_empty());
    assert!(
        session
            .execute(ENGINE_BUILTINS, json!({ "engine": "nope" }))
            .is_err()
    );
    assert!(
        session
            .execute(
                ENGINE_BUILTINS,
                json!({ "engine": "fteqw", "kind": "nope" })
            )
            .is_err()
    );
    let menu = session
        .execute(
            ENGINE_BUILTINS,
            json!({ "engine": "fteqw", "kind": "menuqc" }),
        )
        .unwrap();
    assert!(!menu["builtins"].as_array().unwrap().is_empty());
}

// No spec statement: P4-03, every engine's server table answers with its builtins
#[test]
fn every_engine_has_a_server_table() {
    for engine in Engine::ALL {
        let table = builtins_of(engine, ModuleKind::Ssqc);
        assert_eq!(table.engine, engine.id());
        assert!(!table.builtins.is_empty(), "{}", engine.id());
    }
}
