// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_engines::Engine;
use qc_project::{MemoryLoader, ModuleKind};
use qs_commands::{Registry, Session};
use serde_json::{Value, json};

use super::{
    COMPILE, Compiled, ENGINE_BUILTINS, PROJECT_FILES, PROJECT_INFO, PROJECT_SEARCH,
    PROJECT_SYMBOLS, SEARCH_LIMIT, builtins_of,
};
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

// No spec statement: P4-03, AGT-5, compile builds the project as project.build does and says
// whether it is clean, with its counts; an error makes it unclean and writes nothing
#[test]
fn compile_says_whether_the_build_is_clean() {
    let root = std::env::temp_dir().join(format!("qs-workbench-compile-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).unwrap();
    }
    std::fs::create_dir_all(&root).unwrap();
    let compiled = |world: &str| -> Compiled {
        let mut loader = MemoryLoader::new();
        loader.insert(root.join("progs.src"), b"progs.dat\nworld.qc\n".to_vec());
        loader.insert(root.join("world.qc"), world.as_bytes().to_vec());
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new(Workbench::new(loader, Config::default()), registry);
        session
            .execute(OPEN_PROJECT, json!({ "path": root }))
            .unwrap();
        serde_json::from_value(session.execute(COMPILE, json!({})).unwrap()).unwrap()
    };
    let clean = compiled("void() worldspawn = {};\n");
    assert!(clean.clean, "{clean:?}");
    assert_eq!((clean.errors, clean.warnings), (0, 0));
    assert!(clean.modules[0].built);
    let broken = compiled("void() worldspawn = { nowhere = 1; };\n");
    assert!(!broken.clean);
    assert!(broken.errors >= 1, "{broken:?}");
    assert!(!broken.modules[0].built);
    let mut none = session();
    assert!(none.execute(COMPILE, json!({})).is_err(), "no project");
}

// No spec statement: P4-06, project.files lists every file the project reads and
// project.search finds the lines holding a text, unsaved edits included, without case unless
// asked; both need an open project
#[test]
fn files_and_search_read_the_project() {
    let mut session = session();
    assert!(session.execute(PROJECT_FILES, json!({})).is_err());
    assert!(
        session
            .execute(PROJECT_SEARCH, json!({ "text": "x" }))
            .is_err()
    );
    session
        .execute(OPEN_PROJECT, json!({ "path": ROOT }))
        .unwrap();
    let files: Vec<PathBuf> =
        serde_json::from_value(session.execute(PROJECT_FILES, json!({})).unwrap()).unwrap();
    let root = Path::new(ROOT);
    assert_eq!(
        files,
        [root.join("defs.qc"), root.join("world.qc")],
        "the sources, not progs.src"
    );
    let found = session
        .execute(PROJECT_SEARCH, json!({ "text": "w_fireaxe" }))
        .unwrap();
    assert_eq!(found["matches"].as_array().unwrap().len(), 1);
    assert_eq!(
        Path::new(found["matches"][0]["path"].as_str().unwrap()),
        root.join("world.qc")
    );
    assert_eq!(found["matches"][0]["line"], 2);
    assert_eq!(found["truncated"], false);
    let exact = session
        .execute(
            PROJECT_SEARCH,
            json!({ "text": "w_fireaxe", "case_sensitive": true }),
        )
        .unwrap();
    assert!(exact["matches"].as_array().unwrap().is_empty());
    session
        .execute(
            "file.update",
            json!({ "path": "/p/defs.qc", "text": "float W_FireAxe_damage;\n" }),
        )
        .unwrap();
    let edited = session
        .execute(PROJECT_SEARCH, json!({ "text": "FireAxe" }))
        .unwrap();
    assert_eq!(edited["matches"].as_array().unwrap().len(), 2, "{edited}");
}

// No spec statement: P4-06, project.search answers at most SEARCH_LIMIT lines and says when
// there were more
#[test]
fn search_stops_at_its_limit() {
    let mut loader = MemoryLoader::new();
    let many: String = (0..=SEARCH_LIMIT)
        .map(|n| format!("float hit{n};\n"))
        .collect();
    loader.insert(
        Path::new(ROOT).join("progs.src"),
        b"progs.dat\nmany.qc\n".to_vec(),
    );
    loader.insert(Path::new(ROOT).join("many.qc"), many.into_bytes());
    let mut workbench = Workbench::new(loader, Config::default());
    workbench.open_project(Path::new(ROOT)).unwrap();
    let found = workbench.search("hit", false);
    assert_eq!(found.matches.len(), SEARCH_LIMIT);
    assert!(found.truncated);
    assert!(
        workbench.search("", false).matches.is_empty(),
        "nothing to find"
    );
}

// No spec statement: P4-08, project.info gives each module's kind, dialect and target engine,
// whose id engine.builtins takes; it needs an open project
#[test]
fn project_info_names_each_modules_dialect_and_engine() {
    let mut session = session();
    assert!(
        session.execute(PROJECT_INFO, json!({})).is_err(),
        "no project"
    );
    session
        .execute(OPEN_PROJECT, json!({ "path": ROOT }))
        .unwrap();
    let info = session.execute(PROJECT_INFO, json!({})).unwrap();
    assert_eq!(Path::new(info["root"].as_str().unwrap()), Path::new(ROOT));
    let module = &info["modules"][0];
    assert_eq!(module["kind"], "ssqc", "{info}");
    assert_eq!(module["dialect"], "vanilla");
    let engine = module["engine"].as_str().unwrap();
    assert!(!module["engine_name"].as_str().unwrap().is_empty());
    let builtins = session
        .execute(ENGINE_BUILTINS, json!({ "engine": engine }))
        .unwrap();
    assert_eq!(builtins["engine"], engine);
}
