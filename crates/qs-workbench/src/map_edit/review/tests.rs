// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qs_commands::Session;
use serde_json::{Value, json};

use super::{MAP_ACCEPT, MAP_REJECT, MAP_STAGE, MAP_STAGED, MapHunk};
use crate::map_compile::tests::{Context, Opening, open, room};
use crate::map_edit::{MAP_BRUSH_CREATE, MAP_PLACE, MAP_SAVE};

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
    assert_eq!(hunks[0].entities, ["0 worldspawn"]);
    assert_eq!((hunks[0].brushes_added, hunks[0].keys), (2, 0));
    assert_eq!(hunks[1].entities, ["3 light"]);
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
    assert_eq!(left[0].entities, ["0 worldspawn"]);
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
