// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qs_commands::Session;
use serde_json::{Value, json};

use super::{HunkSide, MAP_ACCEPT, MAP_REJECT, MAP_STAGE, MAP_STAGED, MAP_UNSTAGE, MapHunk};
use crate::map_compile::tests::{Context, Opening, open, room};
use crate::map_edit::{
    EntityIndex, MAP_BRUSH_CREATE, MAP_DELETE, MAP_HIDE, MAP_PLACE, MAP_SAVE, MAP_SELECT,
};

/// The entities `hunk` is in, as `place classname`, `was ` before those it only removed from.
fn named(hunk: &MapHunk) -> Vec<String> {
    hunk.entities
        .iter()
        .map(|entity| {
            let was = if entity.side == HunkSide::Before {
                "was "
            } else {
                ""
            };
            format!("{was}{} {}", entity.entity.get(), entity.classname)
        })
        .collect()
}

fn staged(session: &mut Session<Context>) -> Vec<MapHunk> {
    serde_json::from_value(session.execute(MAP_STAGED, json!({})).unwrap()).unwrap()
}

/// The open map's text, saved.
fn saved(session: &mut Session<Context>, path: &std::path::Path) -> String {
    session.execute(MAP_SAVE, json!({})).unwrap();
    String::from_utf8(std::fs::read(path).unwrap()).unwrap()
}

/// Adds a pillar of two brushes to the world and a light, after review began.
fn agent_edits(session: &mut Session<Context>) {
    session.execute(MAP_STAGE, json!({})).unwrap();
    for (min, max) in [
        ([-32.0, -32.0, -128.0], [32.0, 32.0, 0.0]),
        ([-32.0, -32.0, 0.0], [32.0, 32.0, 128.0]),
    ] {
        session
            .execute(MAP_BRUSH_CREATE, json!({ "min": min, "max": max }))
            .unwrap();
    }
    session
        .execute(
            MAP_PLACE,
            json!({ "classname": "light", "origin": [64.0, 64.0, 64.0] }),
        )
        .unwrap();
}

// No spec statement: P6-21, MAP-29: after review begins, the map's edits wait as hunks, the
// world's new brushes apart from a new entity; with no review there are none
#[test]
fn edits_after_review_begins_wait_as_hunks() {
    let path = room("review-list", "wall", Opening::Sealed);
    let mut session = open(&path);
    session
        .execute(
            MAP_PLACE,
            json!({ "classname": "light", "origin": [0.0, 0.0, 96.0] }),
        )
        .unwrap();
    assert!(staged(&mut session).is_empty(), "no review, no hunks");
    agent_edits(&mut session);
    let hunks = staged(&mut session);
    assert_eq!(hunks.len(), 2, "{hunks:?}");
    assert_eq!(named(&hunks[0]), ["0 worldspawn"]);
    assert_eq!((hunks[0].brushes_added, hunks[0].keys), (2, 0));
    assert_eq!(named(&hunks[1]), ["3 light"]);
    assert_eq!((hunks[1].keys, hunks[1].brushes_added), (1, 0));
    // Beginning again while review is open keeps what it began from.
    session.execute(MAP_STAGE, json!({})).unwrap();
    assert_eq!(staged(&mut session).len(), 2);
}

// No spec statement: P6-21: a rejected hunk's parts are put back, as an edit undo takes back,
// and an accepted one is kept; once none is left review ends
#[test]
fn hunks_are_accepted_or_rejected() {
    let path = room("review-decide", "wall", Opening::Sealed);
    let mut session = open(&path);
    let before = saved(&mut session, &path);
    agent_edits(&mut session);
    let left: Vec<MapHunk> = serde_json::from_value(
        session
            .execute(MAP_REJECT, json!({ "hunks": [1] }))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(named(&left[0]), ["0 worldspawn"]);
    assert!(
        !saved(&mut session, &path).contains("\"light\""),
        "the light is gone"
    );
    session.execute("edit.undo", json!({})).unwrap();
    assert!(
        saved(&mut session, &path).contains("\"light\""),
        "undo brings it back"
    );
    session.execute("edit.redo", json!({})).unwrap();
    let left: Vec<MapHunk> = serde_json::from_value(
        session
            .execute(MAP_ACCEPT, json!({ "hunks": [0] }))
            .unwrap(),
    )
    .unwrap();
    assert!(left.is_empty(), "review ended: {left:?}");
    let text = saved(&mut session, &path);
    let brushes = |text: &str| text.matches("\n{\n(").count();
    assert_eq!(brushes(&text), brushes(&before) + 2, "the pillar is kept");
    // Later edits are not reviewed until review begins again.
    session
        .execute(
            MAP_PLACE,
            json!({ "classname": "light", "origin": [8.0, 8.0, 8.0] }),
        )
        .unwrap();
    assert!(staged(&mut session).is_empty());
}

// No spec statement: P6-21: rejecting every hunk puts the map back byte for byte as review
// began; a number no hunk has is refused
#[test]
fn rejecting_everything_restores_the_map() {
    let path = room("review-all", "wall", Opening::Sealed);
    let mut session = open(&path);
    let before = saved(&mut session, &path);
    agent_edits(&mut session);
    let error = session
        .execute(MAP_ACCEPT, json!({ "hunks": [2] }))
        .unwrap_err();
    assert!(error.to_string().contains("no hunk 2"), "{error}");
    let left = session.execute(MAP_REJECT, json!({})).unwrap();
    assert_eq!(left, Value::Array(Vec::new()));
    assert_eq!(saved(&mut session, &path), before);
}

// No spec statement: P6-21: accepting with no hunk named keeps every one, and review ends
#[test]
fn accepting_everything_ends_review() {
    let path = room("review-accept-all", "wall", Opening::Sealed);
    let mut session = open(&path);
    agent_edits(&mut session);
    assert_eq!(staged(&mut session).len(), 2);
    let left = session.execute(MAP_ACCEPT, json!({})).unwrap();
    assert_eq!(left, Value::Array(Vec::new()));
    assert!(saved(&mut session, &path).contains("\"light\""));
}

// No spec statement: P6-21: a rejection keeps the selection while its entity is still there and
// what is hidden while the entities' count is the same; otherwise both are forgotten
#[test]
fn a_rejection_keeps_the_view_while_it_can() {
    let path = room("review-view", "wall", Opening::Sealed);
    let mut session = open(&path);
    agent_edits(&mut session);
    let document = |session: &Session<Context>| session.context().workbench.map().unwrap().clone();
    let start = EntityIndex::new(1);
    session
        .execute(MAP_HIDE, json!({ "entities": [1] }))
        .unwrap();
    session.execute(MAP_SELECT, json!({ "entity": 1 })).unwrap();
    // The brushes' hunk: the entities' count is the same.
    session
        .execute(MAP_REJECT, json!({ "hunks": [0] }))
        .unwrap();
    assert!(document(&session).is_hidden(start));
    assert_eq!(document(&session).selected(), Some(start));
    // The light's hunk, the light selected: one entity fewer.
    session.execute(MAP_SELECT, json!({ "entity": 2 })).unwrap();
    session.execute(MAP_REJECT, json!({})).unwrap();
    assert!(!document(&session).is_hidden(start));
    assert_eq!(document(&session).selected(), None);
}

// No spec statement: P6-21: undoing a rejection opens its review again, the edits waiting as
// before; a hunk that only removed parts names the entity they were in
#[test]
fn undoing_a_rejection_reopens_review() {
    let path = room("review-undo", "wall", Opening::Sealed);
    let mut session = open(&path);
    agent_edits(&mut session);
    session.execute(MAP_REJECT, json!({})).unwrap();
    assert!(staged(&mut session).is_empty());
    session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(staged(&mut session).len(), 2, "the edits wait again");
    session.execute(MAP_ACCEPT, json!({})).unwrap();
    session.execute(MAP_STAGE, json!({})).unwrap();
    session.execute(MAP_DELETE, json!({ "entity": 1 })).unwrap();
    let hunks = staged(&mut session);
    assert_eq!(hunks.len(), 1);
    assert_eq!(named(&hunks[0]), ["was 1 info_player_start"]);
    assert_eq!((hunks[0].keys, hunks[0].brushes_removed), (1, 0));
}

// No spec statement: P6-21: an undo that takes the map back to where review began ends it, so
// the person's later edits are not reviewed; map.unstage ends a review with nothing in it and
// leaves one with hunks; a rejection with no review open changes nothing
#[test]
fn review_ends_when_nothing_differs() {
    let path = room("review-settle", "wall", Opening::Sealed);
    let mut session = open(&path);
    session.execute(MAP_STAGE, json!({})).unwrap();
    session
        .execute(
            MAP_PLACE,
            json!({ "classname": "light", "origin": [0.0, 0.0, 64.0] }),
        )
        .unwrap();
    session.execute(MAP_UNSTAGE, json!({})).unwrap();
    assert_eq!(
        staged(&mut session).len(),
        1,
        "a review with hunks stays open"
    );
    session.execute("edit.undo", json!({})).unwrap();
    assert!(staged(&mut session).is_empty());
    session
        .execute(
            MAP_PLACE,
            json!({ "classname": "light", "origin": [8.0, 8.0, 64.0] }),
        )
        .unwrap();
    assert!(
        staged(&mut session).is_empty(),
        "the review ended with the undo"
    );
    session.execute(MAP_STAGE, json!({})).unwrap();
    session.execute(MAP_UNSTAGE, json!({})).unwrap();
    session
        .execute(
            MAP_PLACE,
            json!({ "classname": "light", "origin": [16.0, 8.0, 64.0] }),
        )
        .unwrap();
    assert!(
        staged(&mut session).is_empty(),
        "map.unstage ended the empty review"
    );
    let depth = session.undo_depth();
    let left = session.execute(MAP_REJECT, json!({})).unwrap();
    assert_eq!(left, Value::Array(Vec::new()));
    assert_eq!(session.undo_depth(), depth, "nothing to reject, no edit");
}
