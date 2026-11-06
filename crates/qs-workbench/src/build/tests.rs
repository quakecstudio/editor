// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_progs::{Progs, ReadOptions};
use qc_project::MemoryLoader;
use qs_commands::{Registry, Session};
use serde_json::json;

use crate::{
    BUILD_PROJECT, CHECK_PROJECT, Config, Dialect, Gravity, ModuleBuilt, OPEN_PROJECT, Reported,
    Workbench, WorkbenchError, register,
};

/// What a test project holds; the default is one that builds.
struct ProjectSpec {
    world: String,
}

impl Default for ProjectSpec {
    fn default() -> Self {
        Self {
            world: "void() worldspawn = { local float x; x = speed * 2; };\n".to_owned(),
        }
    }
}

/// A fresh directory for the test `name`, where its program is written.
fn root(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("qs-workbench-build-{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A workbench over a bare project in `root`: a `progs.src` listing `defs.qc` and `world.qc`.
fn workbench(root: &Path, spec: &ProjectSpec) -> Workbench<MemoryLoader> {
    let mut loader = MemoryLoader::new();
    for (name, text) in [
        ("progs.src", "progs.dat\ndefs.qc\nworld.qc\n"),
        ("defs.qc", "float speed = 300;\n"),
        ("world.qc", spec.world.as_str()),
    ] {
        loader.insert(root.join(name), text.as_bytes().to_vec());
    }
    Workbench::new(loader, Config::default())
}

fn opened(root: &Path, spec: &ProjectSpec) -> Workbench<MemoryLoader> {
    let mut workbench = workbench(root, spec);
    workbench.open_project(root).unwrap();
    workbench
}

// No spec statement: building or checking needs an open project.
#[test]
fn building_or_checking_needs_a_project() {
    let dir = root("none");
    let mut workbench = workbench(&dir, &ProjectSpec::default());
    assert!(matches!(workbench.build(), Err(WorkbenchError::NoProject)));
    assert!(matches!(workbench.check(), Err(WorkbenchError::NoProject)));
    assert_eq!(WorkbenchError::NoProject.to_string(), "no project is open");
}

// spec PRJ-3.2, LNO-1.4: a bare project's module is written to `build/<mod>/progs.dat`, and its
// line map beside it with the extension `.lno`
#[test]
fn a_module_is_written_to_its_output_with_its_line_map() {
    let dir = root("ok");
    let mut workbench = opened(&dir, &ProjectSpec::default());
    let built = workbench.build().unwrap();
    // The mod is named after the project directory (PRJ-2.3).
    let output = dir
        .join("build")
        .join(dir.file_name().unwrap())
        .join("progs.dat");
    assert_eq!(built.len(), 1);
    assert_eq!(built[0].output, output);
    assert!(built[0].built);
    assert!(built[0].problems.is_empty(), "{:?}", built[0].problems);
    let progs = std::fs::read(&output).unwrap();
    // The test project declares no system globals, so its header's CRC is not id's (PROGS-15.8).
    let options = ReadOptions {
        check_header_crc: false,
    };
    let read = Progs::read_with(&progs, &options).unwrap();
    assert!(
        read.functions()
            .iter()
            .any(|f| read.string(f.s_name) == Some(&b"worldspawn"[..]))
    );
    assert!(
        std::fs::read(output.with_extension("lno"))
            .unwrap()
            .starts_with(b"LNOF")
    );
}

// spec PRJ-3.2: a module with an error writes nothing, and its error is placed in its file
#[test]
fn a_module_with_an_error_writes_nothing() {
    let dir = root("error");
    let spec = ProjectSpec {
        world: "void() worldspawn = {\n  missing = 1;\n};\n".to_owned(),
    };
    let mut workbench = opened(&dir, &spec);
    let built = workbench.build().unwrap();
    assert!(!built[0].built);
    assert!(!built[0].output.exists());
    let error = &built[0].problems[0];
    assert_eq!(error.gravity, Gravity::Error);
    let place = error.place.as_ref().unwrap();
    assert_eq!(place.path, dir.join("world.qc"));
    assert_eq!((place.location.line(), place.location.column()), (2, 3));
    let checked = workbench.check().unwrap();
    assert!(
        checked
            .iter()
            .any(|p| p.gravity == Gravity::Error && p.code == error.code)
    );
}

// No spec statement: the build compiles the editor's text of an open file, not the disk's.
#[test]
fn the_build_compiles_the_editors_text() {
    let dir = root("edited");
    let mut workbench = opened(&dir, &ProjectSpec::default());
    workbench
        .update(
            &dir.join("world.qc"),
            b"void() worldspawn = { nope(); };\n".to_vec(),
        )
        .unwrap();
    assert!(!workbench.build().unwrap()[0].built);
    workbench
        .update(&dir.join("world.qc"), b"void() worldspawn = {};\n".to_vec())
        .unwrap();
    assert!(workbench.build().unwrap()[0].built);
}

// No spec statement: `project.build` and `project.check` run through the registry, as the
// window, the command line and the agent run them.
#[test]
fn build_and_check_are_commands() {
    let dir = root("commands");
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(workbench(&dir, &ProjectSpec::default()), registry);
    session
        .execute(OPEN_PROJECT, json!({ "path": dir }))
        .unwrap();
    let built: Vec<ModuleBuilt> =
        serde_json::from_value(session.execute(BUILD_PROJECT, json!({})).unwrap()).unwrap();
    assert!(built[0].built);
    let checked: Vec<Reported> =
        serde_json::from_value(session.execute(CHECK_PROJECT, json!({})).unwrap()).unwrap();
    assert!(checked.is_empty(), "{checked:?}");
    assert_eq!(session.undo_depth(), 0, "neither is a change to undo");
    assert!(session.execute(BUILD_PROJECT, json!({ "x": 1 })).is_err());
}

// No spec statement: a dialect the editor gives is the build's too (PRJ-2.8).
#[test]
fn the_build_reads_the_project_in_the_editors_dialect() {
    let dir = root("dialect");
    let spec = ProjectSpec {
        world: "void() worldspawn = { float x = 1; x += 2; };\n".to_owned(),
    };
    let mut vanilla = opened(&dir, &spec);
    assert!(!vanilla.build().unwrap()[0].built);
    let mut fte = workbench(&dir, &spec);
    fte.set_config(Config {
        dialect: Some(Dialect::Fteqcc),
        ..Config::default()
    });
    fte.open_project(&dir).unwrap();
    assert!(fte.build().unwrap()[0].built);
}

/// A workbench over `spec`'s bare project, read in the FTEQCC dialect for the KEX rerelease.
fn for_kex(dir: &Path, spec: &ProjectSpec) -> Workbench<MemoryLoader> {
    let mut workbench = workbench(dir, spec);
    workbench.set_config(Config {
        dialect: Some(Dialect::Fteqcc),
        target: Some(qc_project::Target::Rerelease),
        ..Config::default()
    });
    workbench.open_project(dir).unwrap();
    workbench
}

// spec ENGT-3.12: a program for KEX is written as the rerelease's build makes it, a `$` key
// string as it is
#[test]
fn a_kex_program_keeps_its_localized_keys() {
    let dir = root("kex");
    let spec = ProjectSpec {
        world: "string msg;\nvoid() worldspawn = { msg = \"$qc_need_silver_key\"; };\n".to_owned(),
    };
    let mut workbench = for_kex(&dir, &spec);
    let built = workbench.build().unwrap();
    assert!(built[0].built, "{:?}", built[0].problems);
    let progs = std::fs::read(&built[0].output).unwrap();
    let read = Progs::read_with(
        &progs,
        &ReadOptions {
            check_header_crc: false,
        },
    )
    .unwrap();
    assert_eq!(read.header().version, 6);
    assert!(
        progs
            .windows(b"$qc_need_silver_key\0".len())
            .any(|w| w == b"$qc_need_silver_key\0")
    );
}

// spec ENGT-3.12: a program KEX cannot load, more globals than a version 6 statement reaches,
// is refused with the reason and not written
#[test]
fn a_program_kex_cannot_load_is_not_written() {
    use std::fmt::Write as _;
    let dir = root("kex-refused");
    let mut world = String::new();
    for n in 0..33_000 {
        writeln!(world, "float g{n};").unwrap();
    }
    world.push_str("void() worldspawn = { g0 = 1; };\n");
    let spec = ProjectSpec { world };
    let mut workbench = for_kex(&dir, &spec);
    let built = workbench.build().unwrap();
    assert!(!built[0].built);
    let refused = built[0]
        .problems
        .iter()
        .find(|p| p.code == "target")
        .unwrap();
    assert_eq!(refused.gravity, Gravity::Error);
    assert!(
        refused
            .message
            .contains("a version 6 statement reaches 32768"),
        "{}",
        refused.message
    );
    assert!(!built[0].output.exists(), "nothing written");
}
