// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap, index or sum is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::MemoryLoader;
use qs_commands::{Registry, Session};
use serde_json::json;

use super::{ACCEPT_NEXT_EDIT, DISMISS_NEXT_EDIT, NEXT_EDIT, NextEdit, changed, name_around};
use crate::{Config, OPEN_PROJECT, READ_FILE, RENAME, UPDATE_FILE, Workbench, register};

const ROOT: &str = "/p";

fn path(name: &str) -> PathBuf {
    Path::new(ROOT).join(name)
}

const DEFS: &str =
    "entity self;\n.float health;\nvoid end_sys_fields;\n.float charge;\nfloat speed;\n";
const WORLD: &str = "void() worldspawn = { self.charge = speed; };\n";
const MORE: &str = "void() more = { if (self.charge) self.charge = self.charge + 2; };\n";

/// A project whose field `charge` is declared in `defs.qc` and used once in `world.qc` and
/// three times in `more.qc`, open in a session.
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
        ("charge", "power", 4)
    );
    assert_eq!(first.typed_path, path("defs.qc"));
    assert_eq!(first.typed_end, defs.find("power").unwrap() + 5);
    // The other files by path: more.qc, top to bottom, then world.qc.
    assert_eq!(first.path, path("more.qc"));
    assert_eq!(&MORE[first.start..first.end], "charge");
    let mut left = Vec::new();
    while let Some(done) = session
        .execute(ACCEPT_NEXT_EDIT, json!({}))
        .unwrap()
        .as_object()
    {
        left.push((done["path"].clone(), done["left"].as_u64().unwrap()));
    }
    let files: Vec<(serde_json::Value, u64)> = [("more.qc", 3), ("more.qc", 2), ("more.qc", 1)]
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
    assert_eq!(next(&mut session).map(|next| next.left), Some(4));
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
// a field the engine finds by its place, which symbol.rename refuses, suggest nothing; a
// place that no longer says the old name is passed over
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

    // A rename moves the places of more.qc before they are taken: they no longer say the
    // old name where they were, and are passed over for the next that does.
    let mut session = self::session();
    type_text(&mut session, "defs.qc", &DEFS.replace("charge", "power"));
    let renamed =
        json!({ "path": path("more.qc"), "offset": MORE.find("more").unwrap(), "name": "extras" });
    session.execute(RENAME, renamed).unwrap();
    let done = session.execute(ACCEPT_NEXT_EDIT, json!({})).unwrap();
    assert_eq!(
        (&done["path"], &done["left"]),
        (&json!(path("world.qc")), &json!(0))
    );
    assert_eq!(
        text_of(&mut session, "more.qc"),
        MORE.replace("more", "extras")
    );
}
