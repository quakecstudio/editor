// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::cell::RefCell;
use std::fs;
use std::path::Path;

use super::super::testing::{fresh_dir, root};
use super::{Archive, MAPS, fetch_archive, read_pins, unpack};
use crate::corpus::archive::tests::tar_gz;
use crate::download::Net;
use crate::oracles::sha256;

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

fn archive(sha256: &str) -> Archive {
    Archive {
        id: "tool".to_owned(),
        repo: "someone/tool".to_owned(),
        commit: COMMIT.to_owned(),
        sha256: sha256.to_owned(),
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
                "62a27c5e1832e26c09bea2369c00a7449e6a0b58bd15606c595680c331814a77",
                "62a27c5e",
            ),
            "no whole SHA-256",
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

// No spec statement: P6-03: an archive whose checksum differs is refused and not kept; one
// cached with its checksum is not fetched again
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
    assert!(error.contains("sha256"), "{error}");
    assert!(!cache.join("archives/tool.tar.gz").exists());
    let right = archive(&sha256::hex_digest(&body));
    assert_eq!(fetch_archive(&right, &cache, &net).unwrap(), body);
    assert_eq!(fetch_archive(&right, &cache, &net).unwrap(), body);
    assert_eq!(
        *net.asked.borrow(),
        2,
        "the second fetch of the right archive came from the cache"
    );
}

// No spec statement: P6-03: an archive is unpacked without its top directory
#[test]
fn archives_unpack_without_their_top_directory() {
    let body = tar_gz(&[
        ("tool-abc/src/a.c", b"int a;"),
        ("tool-abc/CMakeLists.txt", b"x"),
    ]);
    let into = fresh_dir("maps-unpack").join("3rdparty/tool");
    unpack(&body, &into).unwrap();
    assert_eq!(fs::read(into.join("src/a.c")).unwrap(), b"int a;");
    assert_eq!(fs::read(into.join("CMakeLists.txt")).unwrap(), b"x");
    assert!(unpack(b"not a gzip", &into).is_err());
}
