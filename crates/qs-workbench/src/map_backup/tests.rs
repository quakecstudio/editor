// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::Path;

use qs_commands::Session;
use serde_json::{Value, json};

use super::{BACKUP_FOLDER, BACKUPS_KEPT, Backup, MAP_BACKUP, MAP_BACKUPS, MAP_RESTORE_BACKUP};
use crate::map_compile::tests::{Context, Opening, open, room};
use crate::map_edit::{MAP_SAVE, MAP_SET_KEY, MapSaved};

fn backups(session: &mut Session<Context>) -> Vec<Backup> {
    serde_json::from_value(session.execute(MAP_BACKUPS, json!({})).unwrap()).unwrap()
}

fn backup(session: &mut Session<Context>) -> Backup {
    serde_json::from_value(session.execute(MAP_BACKUP, json!({})).unwrap()).unwrap()
}

/// Sets the world's `message` to `text`.
fn set_message(session: &mut Session<Context>, text: &str) -> Value {
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 0, "key": "message", "value": text }),
        )
        .unwrap()
}

fn has_message(path: &Path, text: &str) -> bool {
    String::from_utf8_lossy(&std::fs::read(path).unwrap())
        .contains(&format!("\"message\" \"{text}\""))
}

// No spec statement: P6-20: a backup is the map as it is in the editor, unsaved edits included,
// written to autosave/<name>.<n>.map beside it, numbered from 1; the last 20 are kept
#[test]
fn backups_are_numbered_and_the_oldest_removed() {
    let map = room("backup-numbers", "wall", Opening::Sealed);
    let mut session = open(&map);
    assert!(backups(&mut session).is_empty());
    set_message(&mut session, "unsaved");
    let first = backup(&mut session);
    assert_eq!(first.number, 1);
    assert_eq!(
        first.path,
        map.parent()
            .unwrap()
            .join(BACKUP_FOLDER)
            .join("backup-numbers.1.map")
    );
    assert!(has_message(&first.path, "unsaved"));
    assert!(!has_message(&map, "unsaved"), "the map itself is not saved");
    let extra = 2;
    for _ in 1..BACKUPS_KEPT + extra {
        backup(&mut session);
    }
    let kept = backups(&mut session);
    assert_eq!(kept.len(), BACKUPS_KEPT);
    let numbers: Vec<u32> = kept.iter().map(|backup| backup.number).collect();
    let newest = u32::try_from(BACKUPS_KEPT + extra).unwrap();
    let expected: Vec<u32> = (u32::try_from(extra + 1).unwrap()..=newest).rev().collect();
    assert_eq!(numbers, expected);
    assert!(!first.path.exists());
    // A file of another map, or not numbered, in the folder is not one of this map's.
    let folder = map.parent().unwrap().join(BACKUP_FOLDER);
    for other in ["other.1.map", "backup-numbers.map", "backup-numbers.x.map"] {
        std::fs::write(folder.join(other), "").unwrap();
    }
    assert_eq!(backups(&mut session).len(), BACKUPS_KEPT);
}

// No spec statement: P6-20: saving the map takes a backup of what it saved
#[test]
fn saving_takes_a_backup() {
    let map = room("backup-save", "wall", Opening::Sealed);
    let mut session = open(&map);
    set_message(&mut session, "saved");
    let saved: MapSaved =
        serde_json::from_value(session.execute(MAP_SAVE, json!({})).unwrap()).unwrap();
    let taken = saved.backup.unwrap();
    assert_eq!(std::fs::read(&taken).unwrap(), std::fs::read(&map).unwrap());
    assert!(has_message(&taken, "saved"));
}

// No spec statement: P6-20: after a crash with unsaved edits, the map opened again is as last
// saved, and its newest backup restores the edits, as an edit undo takes back
#[test]
fn a_backup_restores_the_map_after_a_crash() {
    let map = room("backup-crash", "wall", Opening::Sealed);
    let mut session = open(&map);
    set_message(&mut session, "lost in the crash");
    backup(&mut session);
    // The crash: the session ends with nothing saved.
    drop(session);
    let mut session = open(&map);
    assert!(!has_message(&map, "lost in the crash"));
    let found = backups(&mut session);
    assert_eq!(found.len(), 1);
    session
        .execute(MAP_RESTORE_BACKUP, json!({ "path": found[0].path }))
        .unwrap();
    session.execute(MAP_SAVE, json!({})).unwrap();
    assert!(has_message(&map, "lost in the crash"));
    session.execute("edit.undo", json!({})).unwrap();
    session.execute(MAP_SAVE, json!({})).unwrap();
    assert!(!has_message(&map, "lost in the crash"));
    let error = session
        .execute(
            MAP_RESTORE_BACKUP,
            json!({ "path": map.with_extension("nothing") }),
        )
        .unwrap_err();
    assert!(error.to_string().contains("nothing"), "{error}");
}
