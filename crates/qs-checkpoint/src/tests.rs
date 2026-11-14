// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::string_slice,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use proptest::prelude::*;

use super::{CheckpointError, CheckpointNumber, Store, project_files};

static DIRS: AtomicUsize = AtomicUsize::new(0);

/// A fresh, empty folder.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "qs-checkpoint-{name}-{}-{}",
        std::process::id(),
        DIRS.fetch_add(1, Ordering::Relaxed)
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap();
    }
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes each `(path, bytes)` under `root`.
fn write(root: &Path, files: &[(&str, &[u8])]) {
    for (path, bytes) in files {
        let full = root.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, bytes).unwrap();
    }
}

/// Every file of the project under `root`, by path, with its bytes.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    project_files(root)
        .unwrap()
        .into_iter()
        .map(|path| {
            let bytes = fs::read(root.join(&path)).unwrap();
            (path, bytes)
        })
        .collect()
}

// No spec statement: P4-05, AGT-4. Restoring puts back every file's exact bytes, files
// deleted since come back, files made since go, and build/ is left alone
#[test]
fn restoring_returns_every_file_as_it_was() {
    let root = folder("restore");
    write(
        &root,
        &[
            ("progs.src", b"progs.dat\nworld.qc\n"),
            ("world.qc", b"void() worldspawn = {};\r\n"),
            ("src/weapons/axe.qc", b"// axe\n"),
            ("empty.qc", b""),
            ("gfx/palette.lmp", &[0, 255, 13, 10, 0]),
        ],
    );
    let store = Store::open(&root).unwrap();
    let before = snapshot(&root);
    let checkpoint = store.create("before the agent", 1_700_000_000).unwrap();
    assert_eq!((checkpoint.number.get(), checkpoint.files), (1, 5));
    fs::write(
        root.join("world.qc"),
        b"void() worldspawn = { changed; };\n",
    )
    .unwrap();
    fs::remove_file(root.join("src/weapons/axe.qc")).unwrap();
    write(&root, &[("added.qc", b"new"), ("new/deep/file.qc", b"x")]);
    write(&root, &[("build/progs.dat", b"built")]);
    let restored = store.restore(CheckpointNumber::FIRST).unwrap();
    assert_eq!(snapshot(&root), before);
    assert_eq!(
        restored.removed,
        [PathBuf::from("added.qc"), PathBuf::from("new/deep/file.qc")]
    );
    assert_eq!(
        restored.written,
        [
            PathBuf::from("src/weapons/axe.qc"),
            PathBuf::from("world.qc")
        ]
    );
    assert!(!root.join("new").exists(), "the folders it emptied go");
    assert_eq!(fs::read(root.join("build/progs.dat")).unwrap(), b"built");
    let again = store.restore(CheckpointNumber::FIRST).unwrap();
    assert!(again.written.is_empty() && again.removed.is_empty());
}

// No spec statement: P4-05. A folder that is not a git working tree gets the hidden store; a
// git working tree keeps it in .git, out of its own history; neither is in a checkpoint
#[test]
fn the_store_is_hidden_or_in_git() {
    let plain = folder("plain");
    let store = Store::open(&plain).unwrap();
    assert_eq!(store.git_dir(), plain.join(".quakec-studio/checkpoints"));
    let repo = folder("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    write(&repo, &[("a.qc", b"a")]);
    let store = Store::open(&repo).unwrap();
    assert_eq!(store.git_dir(), repo.join(".git/quakec-studio/checkpoints"));
    assert_eq!(store.create("one", 0).unwrap().files, 1);
}

// No spec statement: P4-05. The store is a bare git repository git reads: its objects pass
// fsck and the checkpoint's tree lists the files, folders after files as git orders them
#[test]
fn git_reads_the_store() {
    let root = folder("git");
    write(
        &root,
        &[
            ("b.qc", b"b"),
            ("a/x.qc", b"x"),
            ("a.qc", b"a"),
            ("a-b.qc", b"c"),
        ],
    );
    let store = Store::open(&root).unwrap();
    store.create("first", 1_700_000_000).unwrap();
    fs::write(root.join("b.qc"), b"bb").unwrap();
    store.create("second", 1_700_000_100).unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("--git-dir")
            .arg(store.git_dir())
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap()
    };
    git(&["fsck", "--strict", "--no-dangling"]);
    let listed = git(&["ls-tree", "-r", "--name-only", "refs/checkpoints/1"]);
    assert_eq!(listed, "a-b.qc\na.qc\na/x.qc\nb.qc\n");
    assert_eq!(git(&["show", "refs/checkpoints/2:b.qc"]), "bb");
    let log = git(&["log", "--format=%s", "refs/checkpoints/2"]);
    assert_eq!(
        log, "second\nfirst\n",
        "each checkpoint's parent is the one before"
    );
}

// No spec statement: P4-05. The checkpoints are listed in order with their labels and file
// counts; a number with no checkpoint is refused, and a damaged object is reported
#[test]
fn checkpoints_are_listed_and_errors_reported() {
    let root = folder("list");
    let store = Store::open(&root).unwrap();
    assert!(store.list().unwrap().is_empty());
    write(&root, &[("a.qc", b"a")]);
    store.create("first", 0).unwrap();
    write(&root, &[("b.qc", b"b")]);
    let second = store.create("agent run: add b", 0).unwrap();
    let listed = store.list().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[1], second);
    assert_eq!(
        (
            listed[0].number.get(),
            listed[0].label.as_str(),
            listed[0].files
        ),
        (1, "first", 1)
    );
    assert!(matches!(
        store.restore(CheckpointNumber::new(3)),
        Err(CheckpointError::NotFound(n)) if n == CheckpointNumber::new(3)
    ));
    let reference = store.git_dir().join("refs/checkpoints/2");
    let commit = fs::read_to_string(&reference).unwrap();
    let object = store
        .git_dir()
        .join("objects")
        .join(&commit[..2])
        .join(commit.trim().get(2..).unwrap());
    fs::write(object, b"not zlib").unwrap();
    assert!(matches!(
        store.restore(CheckpointNumber::new(2)),
        Err(CheckpointError::Corrupt(_))
    ));
    fs::write(reference, "nonsense\n").unwrap();
    assert!(matches!(store.list(), Err(CheckpointError::Corrupt(_))));
}

/// A file tree: up to eight paths of up to three short parts, each with up to 40 bytes.
fn tree() -> impl Strategy<Value = BTreeMap<String, Vec<u8>>> {
    proptest::collection::btree_map(
        proptest::collection::vec("[a-c]{1,2}", 1..4).prop_map(|parts| parts.join("/") + ".qc"),
        proptest::collection::vec(any::<u8>(), 0..40),
        0..8,
    )
}

/// Writes `files` under `root` after removing every file and folder there but the store.
fn replace(root: &Path, files: &BTreeMap<String, Vec<u8>>) -> bool {
    for entry in fs::read_dir(root).unwrap().filter_map(Result::ok) {
        if entry.file_name() == ".quakec-studio" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            fs::remove_dir_all(path).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
    }
    for (path, bytes) in files {
        let full = root.join(path);
        // A path that is a folder of another (a.qc and a.qc/b.qc) cannot be written; such a
        // tree is skipped.
        if fs::create_dir_all(full.parent().unwrap()).is_err() || fs::write(full, bytes).is_err() {
            return false;
        }
    }
    true
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    // No spec statement: P4-05, AGT-4. For any tree checkpointed and any tree after it, a
    // restore gives the first tree back exactly
    #[test]
    fn any_tree_comes_back(first in tree(), second in tree()) {
        let root = folder("prop");
        prop_assume!(replace(&root, &first));
        let before = snapshot(&root);
        let store = Store::open(&root).unwrap();
        store.create("prop", 0).unwrap();
        prop_assume!(replace(&root, &second));
        store.restore(CheckpointNumber::FIRST).unwrap();
        prop_assert_eq!(snapshot(&root), before);
        fs::remove_dir_all(&root).unwrap();
    }
}
