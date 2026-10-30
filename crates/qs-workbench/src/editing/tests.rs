// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::{FsLoader, MemoryLoader, normalize};
use qs_commands::{CommandError, Registry, Session};
use serde_json::{Value, json};

use super::apply;
use crate::{
    COMPLETE, Config, DEFINITION, HOVER, HighlightKind, REFERENCES, RENAME, SAVE_FILE, Span,
    SyntaxClass, UPDATE_FILE, Workbench, WorkbenchError, register,
};

const ROOT: &str = "/p";

fn path(name: &str) -> PathBuf {
    Path::new(ROOT).join(name)
}

/// `path(name)` as JSON, as a command writes it: normalized, with this OS's separators.
fn json_path(name: &str) -> Value {
    json!(normalize(&path(name)))
}

const DEFS: &str = ".float health;\nvoid(float d) hurt;\n";
const CODE: &str = "void(float d) hurt = { self.health = self.health - d; };\n";

/// A project of `defs.qc`, which declares `health` and `hurt`, and `code.qc`, which uses
/// `health` and defines `hurt`, with `defs.qc` open.
fn workbench() -> Workbench<MemoryLoader> {
    let mut loader = MemoryLoader::new();
    loader.insert(path("progs.src"), b"progs.dat\ndefs.qc\ncode.qc\n".to_vec());
    loader.insert(
        path("defs.qc"),
        format!("entity self;\n{DEFS}").into_bytes(),
    );
    loader.insert(path("code.qc"), CODE.as_bytes().to_vec());
    let mut workbench = Workbench::new(loader, Config::default());
    workbench.open_project(Path::new(ROOT)).unwrap();
    workbench.open_file(&path("defs.qc"), 0).unwrap();
    workbench
}

fn session() -> Session<Workbench<MemoryLoader>> {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    Session::new(workbench(), registry)
}

/// The byte offset of the `n`th `needle` in `text`.
fn nth(text: &str, needle: &str, n: usize) -> usize {
    text.match_indices(needle).nth(n).unwrap().0
}

fn at(file: &str, offset: usize) -> Value {
    json!({ "path": path(file), "offset": offset })
}

#[test]
fn an_update_checks_the_project_again_and_keeps_the_editors_version() {
    let mut session = session();
    let open = session.context().open().unwrap().clone();
    assert!(session.context().problems().is_empty());
    assert!(
        open.highlights()
            .iter()
            .any(|h| h.kind == HighlightKind::Field),
        "{:?}",
        open.highlights()
    );
    let text = "entity self;\n.float health;\nvoid(float d) hurt;\nfloat = 1;\n";
    session
        .execute(
            UPDATE_FILE,
            json!({ "path": path("defs.qc"), "text": text }),
        )
        .unwrap();
    let workbench = session.context();
    assert_eq!(workbench.problems().len(), 1);
    assert_eq!(workbench.problems()[0].location.line(), 4);
    let now = workbench.open().unwrap();
    assert_eq!(now.text(), text.as_bytes());
    assert_eq!(
        now.version(),
        open.version(),
        "the editor has this text already"
    );
    assert_ne!(
        now.revision(),
        open.revision(),
        "what was worked out from it changed"
    );
    let float = nth(text, "float", 1);
    assert!(
        now.classes()
            .iter()
            .any(|(span, class)| *class == SyntaxClass::Type && span.start == float),
        "{:?}",
        now.classes()
    );
    assert_eq!(session.undo_depth(), 0);
}

#[test]
fn a_files_text_is_the_editors_else_the_disks() {
    let mut workbench = workbench();
    workbench
        .update(&path("code.qc"), b"float c;\n".to_vec())
        .unwrap();
    assert_eq!(workbench.text(&path("code.qc")).unwrap(), b"float c;\n");
    assert_eq!(
        workbench.text(&path("defs.qc")).unwrap(),
        format!("entity self;\n{DEFS}").as_bytes()
    );
    assert_eq!(workbench.text(&path("nowhere.qc")), None);
}

#[test]
fn an_update_of_another_file_leaves_the_open_files_text() {
    let mut session = session();
    let before = session.context().open().unwrap().clone();
    session
        .execute(
            UPDATE_FILE,
            json!({ "path": path("code.qc"), "text": "float = 2;\n" }),
        )
        .unwrap();
    let workbench = session.context();
    assert_eq!(workbench.open().unwrap(), &before);
    assert_eq!(workbench.problems()[0].path, normalize(&path("code.qc")));
}

#[test]
fn an_update_that_breaks_the_project_file_is_refused() {
    let mut session = session();
    let error = session
        .execute(
            UPDATE_FILE,
            json!({ "path": path("quakec.toml"), "text": "[project\n" }),
        )
        .unwrap_err();
    assert!(matches!(error, CommandError::Failed { .. }), "{error:?}");
}

#[test]
fn definition_references_hover_and_completion_answer_with_places() {
    let mut session = session();
    let use_at = nth(CODE, "health", 0);
    let definition = session.execute(DEFINITION, at("code.qc", use_at)).unwrap();
    let defs = format!("entity self;\n{DEFS}");
    let declared = nth(&defs, "health", 0);
    assert_eq!(
        definition,
        json!({
            "path": json_path("defs.qc"),
            "start": declared,
            "end": declared + 6,
            "location": { "line": 2, "column": 8 }
        })
    );
    assert_eq!(
        session.execute(DEFINITION, at("code.qc", 0)).unwrap(),
        Value::Null,
        "no name at the start of `void`"
    );
    let references = session.execute(REFERENCES, at("code.qc", use_at)).unwrap();
    let references = references.as_array().unwrap();
    assert_eq!(references.len(), 3, "{references:?}");
    assert_eq!(references[0]["path"], json_path("code.qc"));
    let hover = session.execute(HOVER, at("code.qc", use_at)).unwrap();
    assert_eq!(hover["declaration"], json!(".float health"));
    assert_eq!(
        hover["place"]["start"],
        json!(use_at),
        "the hovered name's span"
    );
    let inside = nth(CODE, "self.health - d", 0);
    let candidates = session.execute(COMPLETE, at("code.qc", inside)).unwrap();
    let candidates = candidates.as_array().unwrap();
    let d = candidates.iter().find(|c| c["label"] == "d").unwrap();
    assert_eq!(d["kind"], json!("parameter"));
    let hurt = candidates.iter().find(|c| c["label"] == "hurt").unwrap();
    assert_eq!(hurt["kind"], json!("function"));
    assert_eq!(session.undo_depth(), 0, "questions change nothing");
}

#[test]
fn a_rename_changes_every_file_and_undoes() {
    let mut session = session();
    let open = session.context().open().unwrap().clone();
    let done = session
        .execute(
            RENAME,
            json!({ "path": path("code.qc"), "offset": nth(CODE, "health", 0), "name": "hp" }),
        )
        .unwrap();
    assert_eq!(
        done["files"],
        json!([json_path("code.qc"), json_path("defs.qc")])
    );
    assert_eq!(done["caution"], json!("health"), "a map may name a field");
    let renamed = session.context().open().unwrap().clone();
    assert_eq!(
        renamed.text(),
        b"entity self;\n.float hp;\nvoid(float d) hurt;\n"
    );
    assert_ne!(
        renamed.version(),
        open.version(),
        "the editor takes the new text"
    );
    let code = session.context().problems();
    assert!(code.is_empty(), "{code:?}");
    session.undo().unwrap();
    let back = session.context().open().unwrap().clone();
    assert_eq!(back.text(), open.text());
    assert_ne!(back.version(), renamed.version());
    session.redo().unwrap();
    assert_eq!(session.context().open().unwrap().text(), renamed.text());
}

#[test]
fn a_refused_rename_says_why_and_changes_nothing() {
    let mut session = session();
    let before = session.context().open().unwrap().clone();
    let error = session
        .execute(
            RENAME,
            json!({ "path": path("code.qc"), "offset": nth(CODE, "health", 0), "name": "9x" }),
        )
        .unwrap_err();
    assert!(error.to_string().contains("9x"), "{error}");
    assert!(matches!(&error, CommandError::Failed { .. }), "{error:?}");
    assert_eq!(session.context().open().unwrap(), &before);
    assert_eq!(session.undo_depth(), 0);
    let no_name = session
        .execute(
            RENAME,
            json!({ "path": path("code.qc"), "offset": 0, "name": "x" }),
        )
        .unwrap_err();
    let CommandError::Failed { error, .. } = &no_name else {
        panic!("{no_name:?}");
    };
    assert!(
        matches!(
            error.downcast_ref::<WorkbenchError>(),
            Some(WorkbenchError::Rename(_))
        ),
        "{no_name:?}"
    );
}

#[test]
fn saving_writes_the_editors_text() {
    let dir = std::env::temp_dir().join(format!("qs-workbench-save-{}", std::process::id()));
    let _ignored = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\na.qc\n").unwrap();
    std::fs::write(dir.join("a.qc"), "float a;\n").unwrap();
    let mut workbench = Workbench::new(FsLoader, Config::default());
    workbench.open_project(&dir).unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(workbench, registry);
    let file = dir.join("a.qc");
    session
        .execute(UPDATE_FILE, json!({ "path": file, "text": "float b;\n" }))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "float a;\n",
        "not before a save"
    );
    session.execute(SAVE_FILE, json!({ "path": file })).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "float b;\n");
    let missing = session
        .execute(SAVE_FILE, json!({ "path": dir.join("nowhere.qc") }))
        .unwrap_err();
    assert!(missing.to_string().contains("nowhere.qc"), "{missing}");
    // A directory cannot be written as a file.
    std::fs::create_dir_all(dir.join("sub.qc")).unwrap();
    session
        .execute(
            UPDATE_FILE,
            json!({ "path": dir.join("sub.qc"), "text": "x" }),
        )
        .unwrap();
    let error = session
        .execute(SAVE_FILE, json!({ "path": dir.join("sub.qc") }))
        .unwrap_err();
    assert!(error.to_string().contains("cannot be written"), "{error}");
    let _ignored = std::fs::remove_dir_all(&dir);
}

#[test]
fn edits_apply_in_place_order_whatever_their_order() {
    let edits = [
        (Span::new(6, 7), "Z".to_owned()),
        (Span::new(0, 1), "AA".to_owned()),
        (Span::new(3, 3), "-".to_owned()),
    ];
    assert_eq!(apply(b"abcdefgh", &edits), b"AAbc-defZh");
    assert_eq!(apply(b"abc", &[]), b"abc");
    assert_eq!(apply(b"abc", &[(Span::new(2, 9), "!".to_owned())]), b"ab!");
}
