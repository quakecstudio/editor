// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap, index or sum is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::{FsLoader, MemoryLoader};
use qs_commands::{Registry, Session};
use serde_json::json;

use crate::ids::CheckpointNumber;

use super::{ACCEPT_NEXT_EDIT, DISMISS_NEXT_EDIT, NEXT_EDIT, NextEdit, changed, name_around};
use crate::{
    Config, FORMAT_FILE, OPEN_PROJECT, READ_FILE, RENAME, UPDATE_FILE, Workbench, register,
};

const ROOT: &str = "/p";

fn path(name: &str) -> PathBuf {
    Path::new(ROOT).join(name)
}

const DEFS: &str = "entity self;\n.float health;\nvoid end_sys_fields;\n.float charge;\n\
                    float speed;\nvoid() tick = { self.charge = 0; };\n";
const WORLD: &str = "void() worldspawn = { self.charge = speed; };\n";
const MORE: &str = "void() more = { if (self.charge) self.charge = self.charge + 2; };\n";

/// A project whose field `charge` is declared in `defs.qc` and used once there, once in
/// `world.qc` and three times in `more.qc`, open in a session.
fn session() -> Session<Workbench<MemoryLoader>> {
    let mut loader = MemoryLoader::new();
    for (name, text) in [
        ("progs.src", "progs.dat\ndefs.qc\nworld.qc\nmore.qc\n"),
        ("defs.qc", DEFS),
        ("world.qc", WORLD),
        ("more.qc", MORE),
    ] {
        loader.insert(path(name), text.as_bytes().to_vec());
    }
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(loader, Config::default()), registry);
    session
        .execute(OPEN_PROJECT, json!({ "path": ROOT }))
        .unwrap();
    session
}

/// Gives `file` the text `text`, as the editor does after each key.
fn type_text(session: &mut Session<Workbench<MemoryLoader>>, file: &str, text: &str) {
    session
        .execute(UPDATE_FILE, json!({ "path": path(file), "text": text }))
        .unwrap();
}

/// Types `name` over `old` in `text` of `file` a key at a time: `old` deleted from its end,
/// then `name` typed.
fn retype(
    session: &mut Session<Workbench<MemoryLoader>>,
    (file, text): (&str, &str),
    (old, name): (&str, &str),
) -> String {
    let at = text.find(old).unwrap();
    let (head, tail) = (&text[..at], &text[at + old.len()..]);
    let mut typed = text.to_owned();
    for kept in (0..old.len()).rev() {
        typed = format!("{head}{}{tail}", &old[..kept]);
        type_text(session, file, &typed);
    }
    for end in 1..=name.len() {
        typed = format!("{head}{}{tail}", &name[..end]);
        type_text(session, file, &typed);
    }
    typed
}

fn next(session: &mut Session<Workbench<MemoryLoader>>) -> Option<NextEdit> {
    serde_json::from_value(session.execute(NEXT_EDIT, json!({})).unwrap()).unwrap()
}

fn text_of(session: &mut Session<Workbench<MemoryLoader>>, file: &str) -> String {
    let read = session
        .execute(READ_FILE, json!({ "path": path(file) }))
        .unwrap();
    read["text"].as_str().unwrap().to_owned()
}

// No spec statement: P4-11, UI-7: the one run of bytes two texts differ by, and the name
// around a run that is all a name's
#[test]
fn the_change_between_two_texts_and_the_name_it_is_in() {
    assert_eq!(changed(b"a charge b", b"a charg b"), (7, 8, 7));
    assert_eq!(changed(b"a charg b", b"a chargx b"), (7, 7, 8));
    assert_eq!(changed(b"aaa", b"aa"), (2, 3, 2));
    assert_eq!(changed(b"same", b"same"), (4, 4, 4));
    assert_eq!(changed(b"", b"x"), (0, 0, 1));
    assert_eq!(name_around(b".float charge;", 12, 13), Some((7, 13)));
    assert_eq!(name_around(b".float charge;", 7, 7), Some((7, 13)));
    assert_eq!(name_around(b".float charge;", 13, 13), Some((7, 13)));
    // A run with a byte that is no name's, and a place with no name beside it.
    assert_eq!(name_around(b".float charge;", 6, 8), None);
    assert_eq!(name_around(b"a  b", 2, 2), None);
    assert_eq!(name_around(b"ab", 1, 9), None);
}

// No spec statement: P4-11, UI-7, the acceptance criterion: a field's name typed over where
// it is declared suggests every use of it, in its own file first and then the others, the
// name as it is typed so far; each acceptance changes the next and leaves the rest, and the
// last leaves none
#[test]
fn renaming_a_field_suggests_every_use_and_each_is_taken_in_turn() {
    let mut session = session();
    assert_eq!(next(&mut session), None);
    let defs = retype(&mut session, ("defs.qc", DEFS), ("charge", "power"));
    assert!(defs.contains(".float power;"));
    let first = next(&mut session).unwrap();
    assert_eq!(
        (first.old.as_str(), first.new.as_str(), first.left),
        ("charge", "power", 5)
    );
    assert_eq!(first.typed_path, path("defs.qc"));
    assert_eq!(first.typed_end, defs.find("power").unwrap() + 5);
    // The file typed in first, then the others by path: more.qc, top to bottom, then
    // world.qc.
    assert_eq!(first.path, path("defs.qc"));
    assert_eq!(&defs[first.start..first.end], "charge");
    let mut left = Vec::new();
    while let Some(done) = session
        .execute(ACCEPT_NEXT_EDIT, json!({}))
        .unwrap()
        .as_object()
    {
        left.push((done["path"].clone(), done["left"].as_u64().unwrap()));
    }
    let files: Vec<(serde_json::Value, u64)> = [
        ("defs.qc", 4),
        ("more.qc", 3),
        ("more.qc", 2),
        ("more.qc", 1),
    ]
    .into_iter()
    .chain([("world.qc", 0)])
    .map(|(file, left)| (json!(path(file)), left))
    .collect();
    assert_eq!(left, files);
    assert_eq!(
        text_of(&mut session, "more.qc"),
        MORE.replace("charge", "power")
    );
    assert_eq!(
        text_of(&mut session, "world.qc"),
        WORLD.replace("charge", "power")
    );
    assert_eq!(next(&mut session), None);
    // Each acceptance is its own undo step.
    session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(text_of(&mut session, "world.qc"), WORLD);
    assert_eq!(
        text_of(&mut session, "more.qc"),
        MORE.replace("charge", "power")
    );
}

// No spec statement: P4-11: while the name is no name, or the old one again, nothing is
// suggested, and the places wait; an edit anywhere else, a byte in the name that is no
// name's, and edit.dismiss_next drop them
#[test]
fn suggestions_wait_for_a_name_and_go_with_another_edit() {
    let mut session = session();
    let at = DEFS.find("charge").unwrap();
    let with = |name: &str| format!("{}{name}{}", &DEFS[..at], &DEFS[at + 6..]);
    type_text(&mut session, "defs.qc", &with("charg"));
    assert_eq!(
        next(&mut session).map(|next| next.new),
        Some("charg".to_owned())
    );
    for name in ["", "9", "charge"] {
        type_text(&mut session, "defs.qc", &with(name));
        assert_eq!(next(&mut session), None, "{name:?}");
    }
    type_text(&mut session, "defs.qc", &with("p"));
    assert_eq!(next(&mut session).map(|next| next.left), Some(5));
    // A space in the name ends it.
    type_text(&mut session, "defs.qc", &with("p q"));
    assert_eq!(next(&mut session), None);
    type_text(&mut session, "defs.qc", &with("pq"));
    assert_eq!(next(&mut session), None);

    let mut session = self::session();
    type_text(&mut session, "defs.qc", &with("power"));
    assert!(next(&mut session).is_some());
    type_text(&mut session, "world.qc", &format!("{WORLD}\n"));
    assert_eq!(next(&mut session), None);

    let mut session = self::session();
    type_text(&mut session, "defs.qc", &with("power"));
    session.execute(DISMISS_NEXT_EDIT, json!({})).unwrap();
    assert_eq!(next(&mut session), None);
    assert_eq!(
        session.execute(ACCEPT_NEXT_EDIT, json!({})).unwrap(),
        json!(null)
    );
}

// No spec statement: P4-11: a name typed over at a use, a name declared nowhere else, and
// a field the engine finds by its place, which symbol.rename refuses, suggest nothing
#[test]
fn only_a_declaration_that_may_be_renamed_suggests() {
    let mut session = session();
    type_text(&mut session, "world.qc", &WORLD.replace("charge", "charg"));
    assert_eq!(next(&mut session), None, "a use");
    let mut session = self::session();
    type_text(&mut session, "defs.qc", &DEFS.replace("health", "healt"));
    assert_eq!(next(&mut session), None, "a system field");
    let mut session = self::session();
    type_text(&mut session, "more.qc", &MORE.replace("more", "mor"));
    assert_eq!(next(&mut session), None, "nothing else says it");
}

/// A session over a project of one file, `defs.qc`, holding `text`.
fn session_of(text: &str) -> Session<Workbench<MemoryLoader>> {
    let mut loader = MemoryLoader::new();
    loader.insert(path("progs.src"), b"progs.dat\ndefs.qc\n".to_vec());
    loader.insert(path("defs.qc"), text.as_bytes().to_vec());
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(loader, Config::default()), registry);
    session
        .execute(OPEN_PROJECT, json!({ "path": ROOT }))
        .unwrap();
    session
}

/// Takes every suggestion left.
fn take_all(session: &mut Session<Workbench<MemoryLoader>>) -> usize {
    let mut taken = 0;
    while session
        .execute(ACCEPT_NEXT_EDIT, json!({}))
        .unwrap()
        .is_object()
    {
        taken += 1;
    }
    taken
}

// No spec statement: P4-11: the places are symbol.rename's: a vector's components are
// suggested with it, each keeping its suffix (QC-11.7), so taking every suggestion leaves
// the program the rename would
#[test]
fn a_vector_is_suggested_with_its_components() {
    const VECTOR: &str = "entity self;\n.vector dest1;\n\
        void() go = { self.dest1 = '0 0 0'; self.dest1_z = 4; self.dest1_x = self.dest1_z; };\n";
    let mut session = session_of(VECTOR);
    type_text(
        &mut session,
        "defs.qc",
        &VECTOR.replacen("dest1", "dest_a", 1),
    );
    let first = next(&mut session).unwrap();
    assert_eq!(
        (first.old.as_str(), first.new.as_str(), first.left),
        ("dest1", "dest_a", 4)
    );
    session.execute(ACCEPT_NEXT_EDIT, json!({})).unwrap();
    let second = next(&mut session).unwrap();
    assert_eq!(
        (second.old.as_str(), second.new.as_str(), second.left),
        ("dest1_z", "dest_a_z", 3)
    );
    assert_eq!(take_all(&mut session), 3);
    assert_eq!(
        text_of(&mut session, "defs.qc"),
        VECTOR.replace("dest1", "dest_a")
    );
}

// No spec statement: P4-11: a name symbol.rename refuses is suggested nowhere: one that is
// another definition's, so the uses would mean something else, and a keyword where a use
// would be read as one; typing on to a name it allows suggests again
#[test]
fn a_name_the_rename_refuses_is_not_suggested() {
    let mut session = session();
    let at = DEFS.find("charge").unwrap();
    let with = |name: &str| format!("{}{name}{}", &DEFS[..at], &DEFS[at + 6..]);
    for name in ["speed", "health"] {
        type_text(&mut session, "defs.qc", &with(name));
        assert_eq!(next(&mut session), None, "{name}");
        assert_eq!(
            session.execute(ACCEPT_NEXT_EDIT, json!({})).unwrap(),
            json!(null),
            "{name}"
        );
    }
    assert_eq!(text_of(&mut session, "world.qc"), WORLD);
    type_text(&mut session, "defs.qc", &with("speeds"));
    assert_eq!(next(&mut session).map(|next| next.left), Some(5));

    // A global a statement starts with, where `while` would start a loop.
    const GLOBAL: &str = "float a;\nvoid() f = { a = 1; };\n";
    let mut session = session_of(GLOBAL);
    type_text(&mut session, "defs.qc", &GLOBAL.replacen("a;", "while;", 1));
    assert_eq!(next(&mut session), None);
    type_text(
        &mut session,
        "defs.qc",
        &GLOBAL.replacen("a;", "whiles;", 1),
    );
    assert_eq!(next(&mut session).map(|next| next.left), Some(1));
}

// No spec statement: P4-11: a name a macro writes at one of its uses, which symbol.rename
// refuses, is suggested nowhere, so no count names a place Tab cannot change
#[test]
fn a_name_used_through_a_macro_is_not_suggested() {
    const MACRO: &str = "entity self;\n.float charge;\n#define CHARGE self.charge\n\
        void() tick = { CHARGE = 0; self.charge = 1; };\n";
    let mut session = session_of(MACRO);
    type_text(
        &mut session,
        "defs.qc",
        &MACRO.replacen("charge", "power", 1),
    );
    assert_eq!(next(&mut session), None);
}

// No spec statement: P4-11: suggestions last as long as the text they were made from: a
// rename, a formatting, an undo, and another project opened each drop them, so none is
// made in a text that has moved under it
#[test]
fn a_change_from_elsewhere_drops_the_suggestions() {
    let typed = DEFS.replacen("charge", "power", 1);
    let start = || {
        let mut session = session();
        type_text(&mut session, "defs.qc", &typed);
        assert_eq!(next(&mut session).map(|next| next.left), Some(5));
        session
    };

    let mut session = start();
    let renamed =
        json!({ "path": path("more.qc"), "offset": MORE.find("more").unwrap(), "name": "extras" });
    session.execute(RENAME, renamed).unwrap();
    assert_eq!(next(&mut session), None, "a rename");
    assert_eq!(take_all(&mut session), 0);

    // world.qc with white space the formatter removes, before the name is typed over.
    let mut session = self::session();
    type_text(
        &mut session,
        "world.qc",
        &format!("{}   \n", WORLD.trim_end()),
    );
    type_text(&mut session, "defs.qc", &typed);
    assert_eq!(next(&mut session).map(|next| next.left), Some(5));
    let formatted = session
        .execute(FORMAT_FILE, json!({ "path": path("world.qc") }))
        .unwrap();
    assert_eq!(formatted, json!({ "changed": true }));
    assert_eq!(next(&mut session), None, "a formatting");

    // The first use taken and taken back: the uses after it in more.qc are where they
    // were, and none is suggested where it no longer is.
    let mut session = start();
    session.execute(ACCEPT_NEXT_EDIT, json!({})).unwrap();
    session.execute(ACCEPT_NEXT_EDIT, json!({})).unwrap();
    assert_eq!(next(&mut session).map(|next| next.left), Some(3));
    session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(text_of(&mut session, "more.qc"), MORE);
    assert_eq!(next(&mut session), None, "an undo");
    assert_eq!(take_all(&mut session), 0);

    let mut session = start();
    session
        .execute(OPEN_PROJECT, json!({ "path": ROOT }))
        .unwrap();
    assert_eq!(next(&mut session), None, "a project opened");
}

// No spec statement: P4-11: once a suggestion is taken the name is the one it was given:
// typing in the declared name again drops the rest, so no two uses are given two names
#[test]
fn typing_in_the_name_after_a_suggestion_is_taken_drops_the_rest() {
    let mut session = session();
    let typed = DEFS.replacen("charge", "power", 1);
    type_text(&mut session, "defs.qc", &typed);
    session.execute(ACCEPT_NEXT_EDIT, json!({})).unwrap();
    assert_eq!(next(&mut session).map(|next| next.left), Some(4));
    let now = text_of(&mut session, "defs.qc");
    type_text(&mut session, "defs.qc", &now.replacen("power", "powers", 1));
    assert_eq!(next(&mut session), None);
    assert_eq!(text_of(&mut session, "more.qc"), MORE);
}

// No spec statement: P4-11, P3-35: with the check off the frame no key waits for the
// rename: the name's places are asked for when the check of the edit is back
#[test]
fn the_places_are_asked_for_when_the_edit_is_checked() {
    let mut loader = MemoryLoader::new();
    for (name, text) in [
        ("progs.src", "progs.dat\ndefs.qc\nworld.qc\n"),
        ("defs.qc", DEFS),
        ("world.qc", WORLD),
    ] {
        loader.insert(path(name), text.as_bytes().to_vec());
    }
    let mut workbench = Workbench::checked_in_background(loader, Config::default());
    workbench.open_project(Path::new(ROOT)).unwrap();
    let typed = DEFS.replacen("charge", "power", 1);
    workbench
        .update(&path("defs.qc"), typed.into_bytes())
        .unwrap();
    assert_eq!(workbench.next_edit(), None);
    assert!(workbench.wait_checked());
    assert_eq!(workbench.next_edit().map(|next| next.left), Some(2));
}

/// The project of `defs.qc` and `world.qc` on disk, open, with the field's name typed over
/// and its two uses suggested.
fn typed_on_disk(name: &str) -> (PathBuf, Workbench<FsLoader>) {
    let root =
        std::env::temp_dir().join(format!("qs-workbench-next-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for (file, text) in [
        ("progs.src", "progs.dat\ndefs.qc\nworld.qc\n"),
        ("defs.qc", DEFS),
        ("world.qc", WORLD),
    ] {
        std::fs::write(root.join(file), text).unwrap();
    }
    let mut workbench = Workbench::new(FsLoader, Config::default());
    workbench.open_project(&root).unwrap();
    (root, workbench)
}

// No spec statement: P4-11: files changed on disk, and a checkpoint restored, are not the
// text the suggestions were made from, and drop them
#[test]
fn files_read_again_drop_the_suggestions() {
    let typed = DEFS.replacen("charge", "power", 1).into_bytes();

    let (root, mut workbench) = typed_on_disk("disk");
    workbench
        .update(&root.join("defs.qc"), typed.clone())
        .unwrap();
    assert_eq!(workbench.next_edit().map(|next| next.left), Some(2));
    std::fs::write(root.join("world.qc"), format!("\n{WORLD}")).unwrap();
    workbench.disk_changed().unwrap();
    assert_eq!(workbench.next_edit(), None);
    std::fs::remove_dir_all(root).unwrap();

    let (root, mut workbench) = typed_on_disk("checkpoint");
    workbench.create_checkpoint("before", 0).unwrap();
    workbench.update(&root.join("defs.qc"), typed).unwrap();
    assert_eq!(workbench.next_edit().map(|next| next.left), Some(2));
    workbench
        .restore_checkpoint(CheckpointNumber::FIRST)
        .unwrap();
    assert_eq!(workbench.next_edit(), None);
    std::fs::remove_dir_all(root).unwrap();
}
