// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::json;

use super::{
    CREATE_CHECKPOINT, CheckpointMade, CheckpointRestored, LIST_CHECKPOINTS, RESTORE_CHECKPOINT,
};
use crate::{Config, Workbench, WorkbenchError, register};

const WORLD: &str = "void() worldspawn = {};\n";

/// A fresh project on disk for test `name`, of `progs.src`, `world.qc` and `extra.qc`.
fn project(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "qs-workbench-checkpoints-{name}-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).unwrap();
    }
    std::fs::create_dir_all(&root).unwrap();
    for (file, text) in [
        ("progs.src", "progs.dat\nworld.qc\nextra.qc\n"),
        ("world.qc", WORLD),
        ("extra.qc", "float extra;\n"),
    ] {
        std::fs::write(root.join(file), text).unwrap();
    }
    root
}

fn opened(root: &Path) -> Workbench<FsLoader> {
    let mut workbench = Workbench::new(FsLoader, Config::default());
    workbench.open_project(root).unwrap();
    workbench
}

// No spec statement: P4-05, AGT-4, the acceptance criterion. After an agent run's changes,
// on disk and unsaved, restoring its checkpoint puts every file back byte for byte, brings
// back the file it deleted, removes the one it made, and drops the unsaved text and its
// problems
#[test]
fn restoring_undoes_an_agent_run() {
    let root = project("restore");
    let mut workbench = opened(&root);
    let made = workbench.create_checkpoint("before the run", 0).unwrap();
    assert_eq!((made.number.get(), made.files), (1, 3));
    std::fs::write(root.join("world.qc"), "void() worldspawn = { saved; };\n").unwrap();
    std::fs::remove_file(root.join("extra.qc")).unwrap();
    std::fs::write(root.join("added.qc"), "float added;\n").unwrap();
    workbench
        .update(
            &root.join("world.qc"),
            b"void() worldspawn = { nope = 1; };\n".to_vec(),
        )
        .unwrap();
    assert!(!workbench.problems().is_empty());
    let restored = workbench.restore_checkpoint(1).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("world.qc")).unwrap(),
        WORLD
    );
    assert_eq!(
        std::fs::read_to_string(root.join("extra.qc")).unwrap(),
        "float extra;\n"
    );
    assert!(!root.join("added.qc").exists());
    assert_eq!(restored.removed, [PathBuf::from("added.qc")]);
    assert_eq!(
        restored.written,
        [PathBuf::from("extra.qc"), PathBuf::from("world.qc")]
    );
    assert_eq!(restored.discarded, [PathBuf::from("world.qc")]);
    assert!(!workbench.is_unsaved(&root.join("world.qc")));
    assert_eq!(
        workbench.text(&root.join("world.qc")).unwrap(),
        WORLD.as_bytes()
    );
    assert!(
        workbench.problems().is_empty(),
        "{:?}",
        workbench.problems()
    );
}

// No spec statement: P4-05. The checkpoint commands run through the registry, outside the
// undo history; they need an open project, and a number that is no checkpoint is refused
#[test]
fn the_checkpoint_commands() {
    let root = project("commands");
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    assert!(
        session.execute(CREATE_CHECKPOINT, json!({})).is_err(),
        "no project"
    );
    session
        .execute("project.open", json!({ "path": root }))
        .unwrap();
    let made: CheckpointMade = serde_json::from_value(
        session
            .execute(CREATE_CHECKPOINT, json!({ "label": "agent run 1" }))
            .unwrap(),
    )
    .unwrap();
    assert_eq!((made.number.get(), made.label.as_str()), (1, "agent run 1"));
    let plain: CheckpointMade =
        serde_json::from_value(session.execute(CREATE_CHECKPOINT, json!({})).unwrap()).unwrap();
    assert_eq!(
        (plain.number.get(), plain.label.as_str()),
        (2, "checkpoint")
    );
    let listed: Vec<CheckpointMade> =
        serde_json::from_value(session.execute(LIST_CHECKPOINTS, json!({})).unwrap()).unwrap();
    assert_eq!(listed, [made, plain]);
    std::fs::remove_file(root.join("extra.qc")).unwrap();
    let restored: CheckpointRestored = serde_json::from_value(
        session
            .execute(RESTORE_CHECKPOINT, json!({ "number": 1 }))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(restored.written, [PathBuf::from("extra.qc")]);
    assert!(
        session
            .execute(RESTORE_CHECKPOINT, json!({ "number": 9 }))
            .is_err()
    );
    assert_eq!(session.undo_depth(), 0);
    let mut none = Workbench::new(FsLoader, Config::default());
    assert!(matches!(
        none.restore_checkpoint(1),
        Err(WorkbenchError::NoProject)
    ));
}
