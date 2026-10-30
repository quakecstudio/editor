// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::MemoryLoader;
use qs_commands::{CommandError, Registry, Session};
use serde_json::json;

use crate::{
    Config, EntryKind, Location, OPEN_FILE, OPEN_PROJECT, SymbolKind, Workbench, WorkbenchError,
    register,
};

const ROOT: &str = "/p";

fn path(name: &str) -> PathBuf {
    Path::new(ROOT).join(name)
}

/// A workbench whose loader holds `files`, `(path under ROOT, text)`, with no project open.
fn workbench(files: &[(&str, &str)]) -> Workbench<MemoryLoader> {
    let mut loader = MemoryLoader::new();
    for (name, text) in files {
        loader.insert(path(name), text.as_bytes().to_vec());
    }
    Workbench::new(loader, Config::default())
}

/// The ordinary project: a `progs.src` listing `defs.qc` and `code/world.qc`.
const PROJECT: &[(&str, &str)] = &[
    ("progs.src", "progs.dat\ndefs.qc\ncode/world.qc\n"),
    ("defs.qc", ".float health;\nfloat speed = 300;\n"),
    ("code/world.qc", "void() worldspawn = {};\n"),
];

fn opened(files: &[(&str, &str)]) -> Workbench<MemoryLoader> {
    let mut workbench = workbench(files);
    workbench.open_project(Path::new(ROOT)).unwrap();
    workbench
}

fn at(line: u32, column: u32) -> (u32, u32) {
    (line, column)
}

fn pair(location: Location) -> (u32, u32) {
    (location.line(), location.column())
}

#[test]
fn a_location_counts_lines_from_lf_and_columns_in_characters() {
    assert_eq!(pair(Location::of(b"", 0)), at(1, 1));
    assert_eq!(pair(Location::of(b"ab\ncd", 0)), at(1, 1));
    assert_eq!(pair(Location::of(b"ab\ncd", 2)), at(1, 3));
    assert_eq!(pair(Location::of(b"ab\ncd", 3)), at(2, 1));
    assert_eq!(pair(Location::of(b"ab\ncd", 4)), at(2, 2));
    assert_eq!(pair(Location::of(b"ab\ncd", 5)), at(2, 3));
    // Past the end is the end.
    assert_eq!(pair(Location::of(b"ab\ncd", 99)), at(2, 3));
    // A CR is a character of its line; only LF starts one.
    assert_eq!(pair(Location::of(b"a\r\nb", 2)), at(1, 3));
    assert_eq!(pair(Location::of(b"a\rb", 2)), at(1, 3));
    assert_eq!(pair(Location::of(b"a\r\nb", 3)), at(2, 1));
    // A two-byte character is one column.
    assert_eq!(pair(Location::of("\u{e9}x".as_bytes(), 2)), at(1, 2));
    // A byte that is not UTF-8 is one column, as its replacement character.
    assert_eq!(pair(Location::of(b"\xffx", 1)), at(1, 2));
    assert_eq!(pair(Location::of(b"\n\n\n", 3)), at(4, 1));
}

#[test]
fn the_tree_lists_directories_before_files_each_by_name() {
    let mut files = PROJECT.to_vec();
    files[0] = (
        "progs.src",
        "progs.dat\nz.qc\ndefs.qc\ncode/world.qc\ncode/ai/fight.qc\n",
    );
    files.push(("z.qc", ""));
    files.push(("code/ai/fight.qc", ""));
    let workbench = opened(&files);
    let rows: Vec<(usize, String, bool)> = workbench
        .tree()
        .iter()
        .map(|e| (e.depth, e.name.clone(), e.kind == EntryKind::Directory))
        .collect();
    let expected = [
        (0, "code", true),
        (1, "ai", true),
        (2, "fight.qc", false),
        (1, "world.qc", false),
        (0, "defs.qc", false),
        (0, "z.qc", false),
    ]
    .map(|(d, n, dir)| (d, n.to_owned(), dir));
    assert_eq!(rows, expected);
    let tree = workbench.tree();
    assert_eq!(tree[2].kind, EntryKind::File(path("code/ai/fight.qc")));
}

#[test]
fn a_file_outside_the_project_directory_is_placed_by_its_whole_path() {
    let mut loader = MemoryLoader::new();
    loader.insert(
        path("progs.src"),
        b"progs.dat\n../shared/defs.qc\n".to_vec(),
    );
    loader.insert("/shared/defs.qc", b"float x;\n".to_vec());
    let mut workbench = Workbench::new(loader, Config::default());
    workbench.open_project(Path::new(ROOT)).unwrap();
    let names: Vec<(usize, String)> = workbench
        .tree()
        .iter()
        .map(|e| (e.depth, e.name.clone()))
        .collect();
    assert!(names.contains(&(1, "shared".to_owned())), "{names:?}");
    assert!(names.contains(&(2, "defs.qc".to_owned())), "{names:?}");
}

#[test]
fn with_no_project_the_tree_and_problems_are_empty() {
    let workbench = workbench(&[]);
    assert!(workbench.tree().is_empty());
    assert!(workbench.problems().is_empty());
    assert!(workbench.project_symbols().is_empty());
    assert_eq!(workbench.root(), None);
    assert_eq!(workbench.open(), None);
}

#[test]
fn every_problem_has_its_file_and_where_its_span_starts() {
    let mut files = PROJECT.to_vec();
    files[2] = ("code/world.qc", "void() worldspawn = {};\nfloat = 3;\n");
    let workbench = opened(&files);
    let problems = workbench.problems();
    assert!(!problems.is_empty());
    let first = &problems[0];
    assert_eq!(first.path, path("code/world.qc"));
    assert!(first.diagnostic.is_error());
    assert_eq!(first.location.line(), 2);
    assert_eq!(
        Location::of(files[2].1.as_bytes(), first.diagnostic.span.start),
        first.location
    );
    assert!(
        problems.iter().all(|p| p.path == path("code/world.qc")),
        "{problems:?}"
    );
}

#[test]
fn problems_are_by_path_then_source_order() {
    let mut files = PROJECT.to_vec();
    files[1] = ("defs.qc", "float = 1;\nfloat = 2;\n");
    files[2] = ("code/world.qc", "float = 3;\n");
    let workbench = opened(&files);
    let order: Vec<(PathBuf, u32)> = workbench
        .problems()
        .iter()
        .map(|p| (p.path.clone(), p.location.line()))
        .collect();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(order, sorted);
    assert!(order.iter().any(|(p, _)| *p == path("defs.qc")));
    assert!(order.iter().any(|(p, _)| *p == path("code/world.qc")));
}

#[test]
fn opening_a_file_moves_its_caret_and_counts_each_open() {
    let mut workbench = opened(PROJECT);
    let location = workbench.open_file(&path("defs.qc"), 15).unwrap();
    assert_eq!(pair(location), at(2, 1));
    let open = workbench.open().unwrap();
    assert_eq!(open.path(), path("defs.qc"));
    assert_eq!(open.offset(), 15);
    let first = open.generation();
    workbench.open_file(&path("defs.qc"), 15).unwrap();
    assert_ne!(workbench.open().unwrap().generation(), first);
    // The end of the file is a place for the caret.
    let len = PROJECT[1].1.len();
    let end = workbench.open_file(&path("defs.qc"), len).unwrap();
    assert_eq!(pair(end), at(3, 1));
    // The path is normalized as the project graph names it.
    workbench
        .open_file(Path::new("/p/code/../defs.qc"), 0)
        .unwrap();
    assert_eq!(workbench.open().unwrap().path(), path("defs.qc"));
}

#[test]
fn a_file_that_cannot_be_read_or_an_offset_past_its_end_is_refused() {
    let mut workbench = opened(PROJECT);
    workbench.open_file(&path("defs.qc"), 0).unwrap();
    let before = workbench.open().cloned();
    let missing = workbench.open_file(&path("nowhere.qc"), 0).unwrap_err();
    assert!(
        matches!(&missing, WorkbenchError::NoFile { path: p } if *p == path("nowhere.qc")),
        "{missing:?}"
    );
    assert!(missing.to_string().contains("nowhere.qc"));
    let len = PROJECT[1].1.len();
    let past = workbench
        .open_file(&path("defs.qc"), len.saturating_add(1))
        .unwrap_err();
    assert!(
        matches!(past, WorkbenchError::Offset { offset, len: l, .. } if offset == len + 1 && l == len),
        "{past:?}"
    );
    assert!(past.to_string().contains("past the end"));
    assert_eq!(
        workbench.open().cloned(),
        before,
        "a refused open changes nothing"
    );
}

#[test]
fn opening_a_project_closes_the_open_file() {
    let mut workbench = opened(PROJECT);
    workbench.open_file(&path("defs.qc"), 0).unwrap();
    workbench.open_project(Path::new(ROOT)).unwrap();
    assert_eq!(workbench.open(), None);
    assert_eq!(workbench.root(), Some(Path::new(ROOT)));
}

#[test]
fn a_project_that_fails_to_open_leaves_the_one_open_before() {
    let mut files = PROJECT.to_vec();
    files.push(("bad/quakec.toml", "[project\n"));
    let mut workbench = opened(&files);
    workbench.open_file(&path("defs.qc"), 0).unwrap();
    let error = workbench.open_project(&path("bad")).unwrap_err();
    assert!(matches!(error, WorkbenchError::Project(_)), "{error:?}");
    assert!(std::error::Error::source(&error).is_some());
    assert_eq!(workbench.root(), Some(Path::new(ROOT)));
    assert_eq!(workbench.open().unwrap().path(), path("defs.qc"));
    assert_eq!(workbench.tree().len(), 3, "the project is read again");
}

#[test]
fn the_outline_and_project_symbols_come_from_the_analysis() {
    let mut workbench = opened(PROJECT);
    workbench.open_file(&path("defs.qc"), 0).unwrap();
    let open = workbench.open().unwrap();
    assert_eq!(open.text(), PROJECT[1].1.as_bytes());
    let outline: Vec<(String, SymbolKind)> = open
        .outline()
        .iter()
        .map(|s| (s.name.clone(), s.kind))
        .collect();
    assert_eq!(
        outline,
        [
            ("health".to_owned(), SymbolKind::Field),
            ("speed".to_owned(), SymbolKind::Constant),
        ]
    );
    let names: Vec<String> = workbench
        .project_symbols()
        .iter()
        .map(|s| s.name.clone())
        .collect();
    assert_eq!(names, ["worldspawn", "health", "speed"]);
}

fn session(files: &[(&str, &str)]) -> Session<Workbench<MemoryLoader>> {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    Session::new(workbench(files), registry)
}

#[test]
fn the_commands_open_a_project_and_a_file_outside_the_history() {
    let mut session = session(PROJECT);
    let opened = session
        .execute(OPEN_PROJECT, json!({ "path": ROOT }))
        .unwrap();
    assert_eq!(opened, json!({ "files": 2 }));
    let file = session
        .execute(OPEN_FILE, json!({ "path": "/p/defs.qc", "offset": 15 }))
        .unwrap();
    assert_eq!(
        file,
        json!({ "path": "/p/defs.qc", "location": { "line": 2, "column": 1 } })
    );
    let start = session
        .execute(OPEN_FILE, json!({ "path": "/p/defs.qc" }))
        .unwrap();
    assert_eq!(start["location"], json!({ "line": 1, "column": 1 }));
    assert_eq!(session.context().open().unwrap().offset(), 0);
    assert_eq!(session.undo_depth(), 0, "opening is not in the history");
}

#[test]
fn a_command_that_cannot_open_its_target_fails() {
    let mut session = session(PROJECT);
    let error = session
        .execute(OPEN_FILE, json!({ "path": "/p/nowhere.qc" }))
        .unwrap_err();
    assert!(matches!(error, CommandError::Failed { .. }), "{error:?}");
    let mut files = PROJECT.to_vec();
    files.push(("quakec.toml", "[project\n"));
    let mut session = self::session(&files);
    let error = session
        .execute(OPEN_PROJECT, json!({ "path": ROOT }))
        .unwrap_err();
    assert!(matches!(error, CommandError::Failed { .. }), "{error:?}");
    let error = session
        .execute(OPEN_FILE, json!({ "path": "/p/defs.qc", "line": 3 }))
        .unwrap_err();
    assert!(
        matches!(error, CommandError::InvalidParams { .. }),
        "{error:?}"
    );
}

#[test]
fn the_config_applies_from_the_next_open() {
    let files = [
        ("progs.src", "progs.dat\na.qc\n"),
        (
            "a.qc",
            "void() f = { local float i; for (i = 0; i < 3; i++) {} };\n",
        ),
    ];
    let mut workbench = opened(&files);
    let vanilla = workbench.problems().len();
    workbench.set_config(Config {
        dialect: Some(qc_project::Dialect::Fteqcc),
        ..Config::default()
    });
    assert_eq!(
        workbench.problems().len(),
        vanilla,
        "not before the next open"
    );
    workbench.open_project(Path::new(ROOT)).unwrap();
    let fteqcc = workbench.problems().len();
    assert!(vanilla > fteqcc, "{vanilla} > {fteqcc}");
}
