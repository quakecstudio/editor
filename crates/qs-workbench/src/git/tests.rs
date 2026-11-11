// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::{Value, json};

use super::{
    GIT_BLAME, GIT_BRANCHES, GIT_COMMIT, GIT_CONFLICTS, GIT_CREATE_BRANCH, GIT_DIFF, GIT_HEAD_TEXT,
    GIT_LOG, GIT_RESOLVE, GIT_STAGE, GIT_STATUS, GIT_SWITCH, GIT_UNSTAGE,
};
use crate::{Config, Workbench, register};

/// Runs git in `dir` as a person would; its stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git {args:?}: {:?}", output);
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// What `git status --porcelain` prints in `dir`.
fn porcelain(dir: &Path) -> Vec<String> {
    git(dir, &["status", "--porcelain", "--untracked-files=all"])
        .lines()
        .map(str::to_owned)
        .collect()
}

const WORLD: &str = "void() worldspawn =\n{\n\tlocal float a;\n\ta = 1;\n\ta = 2;\n\ta = 3;\n\
                     \ta = 4;\n\ta = 5;\n\ta = 6;\n\ta = 7;\n\ta = 8;\n\ta = 9;\n\ta = 11;\n};\n";

/// A project in a fresh repository with one commit, opened in a session.
fn project(name: &str) -> (PathBuf, Session<Workbench<FsLoader>>) {
    let dir = std::env::temp_dir().join(format!("qs-wb-git-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    std::fs::write(dir.join("world.qc"), WORLD).unwrap();
    std::fs::write(dir.join(".gitignore"), "build/\n").unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    for (key, value) in [
        ("user.name", "Tester"),
        ("user.email", "tester@example.invalid"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
    ] {
        git(&dir, &["config", key, value]);
    }
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute("project.open", json!({ "path": dir }))
        .unwrap();
    (dir, session)
}

/// `git.status`'s files as porcelain lines.
fn status_lines(session: &mut Session<Workbench<FsLoader>>) -> Vec<String> {
    let status = session.execute(GIT_STATUS, json!({})).unwrap();
    status["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["porcelain"].as_str().unwrap().to_owned())
        .collect()
}

// No spec statement: P4-35, the acceptance criterion: staging a hunk, committing and switching
// branches through the commands leave git status as the git command line reports it
#[test]
fn the_git_commands_agree_with_the_command_line() {
    let (dir, mut s) = project("accept");
    let world = dir.join("world.qc");
    let changed = WORLD
        .replace("a = 1;", "a = 10;")
        .replace("a = 11;", "a = 110;");
    std::fs::write(&world, &changed).unwrap();
    assert_eq!(porcelain(&dir), [" M world.qc"]);
    assert_eq!(status_lines(&mut s), porcelain(&dir));
    let hunks = s.execute(GIT_DIFF, json!({ "path": world })).unwrap();
    assert_eq!(hunks.as_array().unwrap().len(), 2);
    s.execute(GIT_STAGE, json!({ "path": world, "hunk": 0 }))
        .unwrap();
    assert_eq!(porcelain(&dir), ["MM world.qc"]);
    assert_eq!(status_lines(&mut s), porcelain(&dir));
    let staged = s
        .execute(GIT_DIFF, json!({ "path": world, "side": "staged" }))
        .unwrap();
    assert_eq!(staged.as_array().unwrap().len(), 1);
    s.execute(GIT_UNSTAGE, json!({ "path": world, "hunk": 0 }))
        .unwrap();
    assert_eq!(porcelain(&dir), [" M world.qc"]);
    s.execute(GIT_STAGE, json!({ "path": world })).unwrap();
    let id = s
        .execute(GIT_COMMIT, json!({ "message": "Raise a" }))
        .unwrap();
    assert_eq!(Value::from(git(&dir, &["rev-parse", "HEAD"]).trim()), id);
    assert!(porcelain(&dir).is_empty());
    assert!(status_lines(&mut s).is_empty());

    s.execute(GIT_CREATE_BRANCH, json!({ "branch": "feature" }))
        .unwrap();
    std::fs::write(&world, WORLD.replace("a = 2;", "a = 20;")).unwrap();
    s.execute(GIT_STAGE, json!({ "path": world })).unwrap();
    s.execute(GIT_COMMIT, json!({ "message": "On the feature" }))
        .unwrap();
    // The file is open in the editor with the feature's text; switching back reads main's.
    s.execute("file.open", json!({ "path": world })).unwrap();
    s.execute(GIT_SWITCH, json!({ "branch": "main" })).unwrap();
    assert_eq!(git(&dir, &["branch", "--show-current"]).trim(), "main");
    assert!(porcelain(&dir).is_empty());
    let open = s.context().open().unwrap();
    assert_eq!(String::from_utf8_lossy(open.text()), changed);
    let branches = s.execute(GIT_BRANCHES, json!({})).unwrap();
    assert_eq!(
        branches,
        json!([{ "name": "feature", "current": false }, { "name": "main", "current": true }])
    );
    let status = s.execute(GIT_STATUS, json!({})).unwrap();
    assert_eq!(status["branch"], "main");
}

// No spec statement: P4-35: a switch is refused while the editor has unsaved edits git cannot
// see, and with no repository the commands say so
#[test]
fn a_switch_waits_for_unsaved_edits_and_no_repository_is_said() {
    let (dir, mut s) = project("unsaved");
    let world = dir.join("world.qc");
    s.execute(GIT_CREATE_BRANCH, json!({ "branch": "other" }))
        .unwrap();
    s.execute("file.open", json!({ "path": world })).unwrap();
    s.execute(
        "file.update",
        json!({ "path": world, "text": "void() worldspawn = {};\n" }),
    )
    .unwrap();
    let refused = s.execute(GIT_SWITCH, json!({ "branch": "main" }));
    assert!(
        refused
            .as_ref()
            .is_err_and(|e| e.to_string().contains("save these files")),
        "{refused:?}"
    );
    let bare = std::env::temp_dir().join(format!("qs-wb-git-bare-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&bare);
    std::fs::create_dir_all(&bare).unwrap();
    std::fs::write(bare.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    std::fs::write(bare.join("world.qc"), WORLD).unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut none = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    assert!(none.execute(GIT_STATUS, json!({})).is_err(), "no project");
    none.execute("project.open", json!({ "path": bare }))
        .unwrap();
    // The temporary folder may itself be inside a repository on a developer's machine; on a
    // runner it is not.
    if git_toplevel(&bare).is_none() {
        assert_eq!(none.execute(GIT_STATUS, json!({})).unwrap(), Value::Null);
        let error = none
            .execute(GIT_BRANCHES, json!({}))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not in a git repository"), "{error}");
    }
}

/// The repository holding `dir`, if any.
fn git_toplevel(dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

// No spec statement: P4-35: a file's history, blame and text at HEAD; a conflict listed and
// resolved to a side, the file saved
#[test]
fn history_blame_head_text_and_a_resolved_conflict() {
    let (dir, mut s) = project("history");
    let world = dir.join("world.qc");
    let log = s.execute(GIT_LOG, json!({ "path": world })).unwrap();
    assert_eq!(log[0]["summary"], "first");
    let blame = s.execute(GIT_BLAME, json!({ "path": world })).unwrap();
    assert_eq!(blame.as_array().unwrap().len(), WORLD.lines().count());
    assert_eq!(blame[0]["author"], "Tester");
    let head = s.execute(GIT_HEAD_TEXT, json!({ "path": world })).unwrap();
    assert_eq!(head, json!(WORLD));
    let conflicted = "void() worldspawn =\n{\n<<<<<<< HEAD\n\tlocal float a;\n=======\n\
                      \tlocal float b;\n>>>>>>> other\n};\n";
    std::fs::write(&world, conflicted).unwrap();
    s.execute("file.open", json!({ "path": world })).unwrap();
    let found = s.execute(GIT_CONFLICTS, json!({ "path": world })).unwrap();
    assert_eq!(found, json!([{ "start": 3, "middle": 5, "end": 7 }]));
    s.execute(
        GIT_RESOLVE,
        json!({ "path": world, "conflict": 0, "side": "theirs" }),
    )
    .unwrap();
    let text = std::fs::read_to_string(&world).unwrap();
    assert_eq!(text, "void() worldspawn =\n{\n\tlocal float b;\n};\n");
    let open = s.context().open().unwrap();
    assert_eq!(String::from_utf8_lossy(open.text()), text);
    assert!(
        s.execute(
            GIT_RESOLVE,
            json!({ "path": world, "conflict": 0, "side": "ours" })
        )
        .is_err(),
        "no conflict is left"
    );
}
