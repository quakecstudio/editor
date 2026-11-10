// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::MemoryLoader;

use crate::{Config, Workbench};

const ROOT: &str = "/p";

fn path(name: &str) -> PathBuf {
    Path::new(ROOT).join(name)
}

/// A project of `defs.qc` and `world.qc`, opened in a workbench checked in the background,
/// with `world.qc` open in the editor.
fn background() -> Workbench<MemoryLoader> {
    let mut loader = MemoryLoader::new();
    for (name, text) in [
        ("progs.src", "progs.dat\ndefs.qc\nworld.qc\n"),
        ("defs.qc", ".float health;\n"),
        ("world.qc", "void() worldspawn = {};\n"),
    ] {
        loader.insert(path(name), text.as_bytes().to_vec());
    }
    let mut workbench = Workbench::checked_in_background(loader, Config::default());
    workbench.open_project(Path::new(ROOT)).unwrap();
    workbench.open_file(&path("world.qc"), 0).unwrap();
    workbench
}

// No spec statement: P3-35, the check after typing off the frame. An edit's text is the open
// file's at once and its problems are not yet listed; once the background check has run they
// are, and the outline follows the new text
#[test]
fn an_edit_is_checked_off_the_frame() {
    let mut workbench = background();
    assert!(workbench.problems().is_empty());
    let text = b"void() worldspawn = { undefined_name = 1; };\nvoid() later = {};\n";
    workbench.update(&path("world.qc"), text.to_vec()).unwrap();
    assert_eq!(workbench.open().unwrap().text(), text.as_slice());
    assert!(
        workbench.problems().is_empty(),
        "the frame does not check the edit itself"
    );
    assert!(workbench.wait_checked());
    assert!(
        workbench
            .problems()
            .iter()
            .any(|problem| problem.path == path("world.qc")),
        "{:?}",
        workbench.problems()
    );
    let outline: Vec<&str> = workbench
        .open()
        .unwrap()
        .outline()
        .iter()
        .map(|symbol| symbol.name.as_str())
        .collect();
    assert!(outline.contains(&"later"), "{outline:?}");
}

// No spec statement: P3-35, a burst of edits is checked for the last one; a result for an
// earlier edit is never taken over a later edit's text
#[test]
fn only_the_latest_edit_is_taken() {
    let mut workbench = background();
    for name in ["a", "b", "c"] {
        let text = format!("void() worldspawn = {{}};\nvoid() {name} = {{}};\n");
        workbench
            .update(&path("world.qc"), text.into_bytes())
            .unwrap();
    }
    assert!(workbench.wait_checked());
    let outline: Vec<String> = workbench
        .open()
        .unwrap()
        .outline()
        .iter()
        .map(|symbol| symbol.name.clone())
        .collect();
    assert!(outline.contains(&"c".to_owned()), "{outline:?}");
    assert!(!outline.contains(&"a".to_owned()), "{outline:?}");
    assert!(!workbench.take_checked(), "nothing newer is waiting");
}

// No spec statement: P3-35, a workbench made without the background check checks each edit at
// once, as before, and has nothing to take or wait for
#[test]
fn without_the_background_check_an_edit_is_checked_at_once() {
    let mut loader = MemoryLoader::new();
    loader.insert(path("progs.src"), b"progs.dat\nworld.qc\n".to_vec());
    loader.insert(path("world.qc"), b"void() worldspawn = {};\n".to_vec());
    let mut workbench = Workbench::new(loader, Config::default());
    workbench.open_project(Path::new(ROOT)).unwrap();
    workbench
        .update(
            &path("world.qc"),
            b"void() worldspawn = { nope = 1; };\n".to_vec(),
        )
        .unwrap();
    assert!(!workbench.problems().is_empty());
    assert!(!workbench.take_checked());
    assert!(!workbench.wait_checked());
}
