// SPDX-License-Identifier: GPL-3.0-or-later

//! Incremental model tests: an edit re-runs only the dependent queries (GOAL-1).
//!
//! These assert the second acceptance criterion of the issue directly, through the query
//! counters [`crate::db::QueryCounts`]. No test here cites a spec statement: incremental
//! recomputation is a requirement on this compiler (GOAL-1 in `plan/requirements.yaml`), not a
//! behaviour of id's `qcc` or the engine that `docs/spec/` describes.

use crate::db::{Db, QueryCounts};
use crate::symbol::{DefId, FileId};

/// The parses, lowerings and module resolutions run between `before` and `after`. The
/// counters only grow, so a smaller `after` is a failed test.
fn reruns(before: QueryCounts, after: QueryCounts) -> (u64, u64, u64) {
    let ran = |b: u64, a: u64| a.checked_sub(b).expect("a query counter went down");
    (
        ran(before.parse, after.parse),
        ran(before.lower, after.lower),
        ran(before.module_index, after.module_index),
    )
}

/// A two-file module in a fresh database, resolved once so every query is cached.
fn seeded() -> (Db, Vec<FileId>) {
    let mut db = Db::new();
    let defs = FileId::new(0);
    let behaviour = FileId::new(1);
    db.set_file_text(defs, b"float shared;\nvoid() helper = {};".to_vec());
    db.set_file_text(
        behaviour,
        b"void() worker = { shared = 1; helper(); };".to_vec(),
    );
    let files = vec![defs, behaviour];
    db.module_index(&files);
    (db, files)
}

#[test]
fn first_resolve_runs_every_query() {
    let (db, _) = seeded();
    let counts = db.counts();
    // Two files parsed and lowered, one module resolved.
    assert_eq!(counts.parse, 2);
    assert_eq!(counts.lower, 2);
    assert_eq!(counts.module_index, 1);
}

#[test]
fn asking_again_without_edits_runs_nothing() {
    let (mut db, files) = seeded();
    let before = db.counts();
    db.module_index(&files);
    let after = db.counts();
    // Everything served from cache.
    assert_eq!(before, after);
}

#[test]
fn editing_one_file_relowers_only_that_file() {
    let (mut db, files) = seeded();
    let before = db.counts();
    // Edit the second file's function body only.
    db.set_file_text(
        FileId::new(1),
        b"void() worker = { shared = 2; helper(); };".to_vec(),
    );
    db.module_index(&files);
    let after = db.counts();
    // Exactly one file re-parsed and re-lowered, the edited one, and the module re-resolved
    // once, since one of its files changed.
    assert_eq!(reruns(before, after), (1, 1, 1));
}

#[test]
fn editing_the_other_file_relowers_only_it() {
    let (mut db, files) = seeded();
    let before = db.counts();
    db.set_file_text(
        FileId::new(0),
        b"float shared;\nvoid() helper = { };".to_vec(),
    );
    db.module_index(&files);
    let after = db.counts();
    assert_eq!(reruns(before, after), (1, 1, 1));
}

#[test]
fn parse_and_lower_of_unedited_file_are_cached() {
    let (mut db, _files) = seeded();
    // Edit file 1, then ask directly for file 0's parse and lower: both cached.
    db.set_file_text(
        FileId::new(1),
        b"void() worker = { shared = 3; helper(); };".to_vec(),
    );
    let before = db.counts();
    db.parse(FileId::new(0));
    db.lower(FileId::new(0));
    let after = db.counts();
    assert_eq!(before, after);
}

#[test]
fn an_edit_that_adds_a_reference_is_reflected() {
    let mut db = Db::new();
    let file = FileId::new(0);
    db.set_file_text(file, b"float a;\nvoid() f = { a = 1; };".to_vec());
    let before = db.module_index(&[file]);
    let a_before = before
        .defs
        .iter()
        .position(|d| d.name == "a")
        .map(DefId::new)
        .expect("a");
    assert_eq!(before.references(a_before, false).len(), 1);

    // Add a second use of `a`.
    db.set_file_text(
        file,
        b"float a;\nvoid() f = { a = 1; a = a + 1; };".to_vec(),
    );
    let after = db.module_index(&[file]);
    let a_after = after
        .defs
        .iter()
        .position(|d| d.name == "a")
        .map(DefId::new)
        .expect("a");
    // Now three uses of `a`.
    assert_eq!(after.references(a_after, false).len(), 3);
}

#[test]
fn setting_identical_text_still_advances_but_recomputes_once() {
    let (mut db, files) = seeded();
    let before = db.counts();
    // Re-set file 1 to the same bytes: an input change by contract, so it re-lowers once.
    db.set_file_text(
        FileId::new(1),
        b"void() worker = { shared = 1; helper(); };".to_vec(),
    );
    db.module_index(&files);
    let after = db.counts();
    assert_eq!(reruns(before, after), (1, 1, 1));
}
