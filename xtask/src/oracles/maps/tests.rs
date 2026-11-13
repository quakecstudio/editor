// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::cell::RefCell;
use std::fs;
use std::path::Path;

use super::super::testing::{fresh_dir, root};
use super::{Archive, MAPS, fetch_archive, members, read_pins, tree_digest, unpack, work_dir};
use crate::corpus::archive::tests::tar_gz;
use crate::download::Net;

/// A [`Net`] that serves one body for every URL and counts the requests.
struct Serve {
    body: Vec<u8>,
    asked: RefCell<usize>,
}

impl Net for Serve {
    fn get(&self, _url: &str) -> Result<Vec<u8>, String> {
        let asked = *self.asked.borrow();
        *self.asked.borrow_mut() = asked.saturating_add(1);
        Ok(self.body.clone())
    }

    fn download(&self, url: &str, out: &Path) -> Result<(), String> {
        let bytes = self.get(url)?;
        fs::write(out, bytes).map_err(|e| e.to_string())
    }
}

const COMMIT: &str = "36eec1da2a194467e6baac2f444c2dd04b57d266";

fn archive(tree: &str) -> Archive {
    Archive {
        id: "tool".to_owned(),
        repo: "someone/tool".to_owned(),
        commit: COMMIT.to_owned(),
        tree: tree.to_owned(),
        into: String::new(),
    }
}

// No spec statement: P6-03: the repository's pins name full commits and whole checksums, with
// ericw-tools first at the top of its tree
#[test]
fn the_pins_are_whole() {
    let pins = read_pins(&root()).unwrap();
    assert_eq!(pins.qutils.repo, "id-Software/Quake-Tools");
    assert!(pins.qutils.files.keys().all(|f| f.starts_with("qutils/")));
    assert_eq!(pins.archive.first().unwrap().id, "ericw-tools");
    assert!(
        pins.archive
            .iter()
            .skip(1)
            .all(|a| a.into.starts_with("3rdparty/"))
    );
}

// No spec statement: P6-03: a short commit, a short checksum or a submodule first is refused
#[test]
fn pins_that_are_not_whole_are_refused() {
    let text = fs::read_to_string(root().join(MAPS)).unwrap();
    let cases = [
        (
            text.replace(COMMIT, "36eec1da"),
            "not pinned to a full commit",
        ),
        (
            text.replace(
                "fdc275f499781bd5fe45c37a7afdd17e1911f6b392bf056eab7900fea036fed1",
                "FDC275F499781BD5FE45C37A7AFDD17E1911F6B392BF056EAB7900FEA036FED1",
            ),
            "no whole tree SHA-256",
        ),
        (
            text.replacen("into = \"\"", "into = \"x\"", 1),
            "the first archive",
        ),
    ];
    for (index, (pins, why)) in cases.into_iter().enumerate() {
        let dir = fresh_dir(&format!("maps-pins-{index}"));
        fs::create_dir_all(dir.join("oracles")).unwrap();
        fs::write(dir.join(MAPS), pins).unwrap();
        let error = read_pins(&dir).unwrap_err();
        assert!(error.contains(why), "{error}");
    }
}

// No spec statement: P6-03: an archive whose files differ from the pinned tree is refused and
// not kept, not even in part; one cached with its tree is not fetched again
#[test]
fn archives_are_checked_and_cached() {
    let body = tar_gz(&[("tool-abc/README", b"hello")]);
    let net = Serve {
        body: body.clone(),
        asked: RefCell::new(0),
    };
    let cache = fresh_dir("maps-archives");
    fs::create_dir_all(cache.join("archives")).unwrap();
    let wrong = archive(&"0".repeat(64));
    let error = fetch_archive(&wrong, &cache, &net).unwrap_err();
    assert!(error.contains("tree"), "{error}");
    assert!(!cache.join("archives/tool.tar.gz").exists());
    assert!(!cache.join("archives/tool.tar.partial").exists());
    let inside = members(&body).unwrap();
    let right = archive(&tree_digest(&inside));
    assert_eq!(fetch_archive(&right, &cache, &net).unwrap(), inside);
    assert_eq!(fetch_archive(&right, &cache, &net).unwrap(), inside);
    assert_eq!(
        *net.asked.borrow(),
        2,
        "the second fetch of the right archive came from the cache"
    );
}

// No spec statement: P6-03: an archive is unpacked without its top directory, and one naming
// a member outside it is refused; the tree digest is of paths and contents, in any order
#[test]
fn archives_unpack_without_their_top_directory() {
    let body = tar_gz(&[
        ("tool-abc/src/a.c", b"int a;"),
        ("tool-abc/CMakeLists.txt", b"x"),
    ]);
    let into = fresh_dir("maps-unpack").join("3rdparty/tool");
    let inside = members(&body).unwrap();
    let reversed: Vec<_> = inside.iter().rev().cloned().collect();
    assert_eq!(tree_digest(&inside), tree_digest(&reversed));
    unpack(inside, &into).unwrap();
    assert_eq!(fs::read(into.join("src/a.c")).unwrap(), b"int a;");
    assert_eq!(fs::read(into.join("CMakeLists.txt")).unwrap(), b"x");
    assert!(members(b"not a gzip").is_err());
    let escaping = tar_gz(&[("tool-abc/../../x", b"y")]);
    let error = members(&escaping).unwrap_err();
    assert!(error.contains("not a path inside"), "{error}");
    let changed = members(&tar_gz(&[("tool-abc/src/a.c", b"int b;")])).unwrap();
    let original = members(&tar_gz(&[("tool-abc/src/a.c", b"int a;")])).unwrap();
    assert_ne!(tree_digest(&changed), tree_digest(&original));
}

/// On CI's checkout path, which holds no `quake`, qbsp's search for the first `quake` in the
/// map's path (`SetQdirFromPath`) lands on the work directory, with `id1` as its game directory.
#[test]
fn qbsp_finds_quake_and_a_game_directory_in_the_work_path() {
    let work = work_dir(Path::new("/home/runner/work/editor/editor"));
    let path = work
        .join("id")
        .join("room.map")
        .to_string_lossy()
        .to_ascii_lowercase();
    let quake = path.find("quake").unwrap();
    assert_eq!(path.get(quake..), Some("quake/id1/id/room.map"));
    assert!(work.starts_with("/home/runner/work/editor/editor/target/oracles"));
}
