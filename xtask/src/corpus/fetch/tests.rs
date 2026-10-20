// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of `corpus fetch`, and the [`FakeNet`] and temporary directories the asset tests use
//! too. Nothing here touches the network.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::super::archive::tests::tar_gz;
use super::super::manifest::{Entry, Manifest, Source, Tree};
use super::{cmd_fetch, codebase_files, write_codebase, write_tree};
use crate::download::Net;
use crate::oracles::sha256::hex_digest;

pub(crate) const COMMIT: &str = "cccccccccccccccccccccccccccccccccccccccc";

/// Serves canned bodies by URL and records every request; an unknown URL fails.
#[derive(Default)]
pub(crate) struct FakeNet {
    pub(crate) bodies: BTreeMap<String, Vec<u8>>,
    pub(crate) requests: RefCell<Vec<String>>,
}

impl FakeNet {
    pub(crate) fn serving(url: &str, body: &[u8]) -> Self {
        let mut net = Self::default();
        net.bodies.insert(url.to_owned(), body.to_vec());
        net
    }

    pub(crate) fn calls(&self) -> usize {
        self.requests.borrow().len()
    }
}

impl Net for FakeNet {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        self.requests.borrow_mut().push(url.to_owned());
        self.bodies
            .get(url)
            .cloned()
            .ok_or_else(|| "404 Not Found".to_owned())
    }

    fn download(&self, url: &str, out: &Path) -> Result<(), String> {
        let body = self.get(url);
        let parent = out.parent().ok_or("no parent directory")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        // A failed download leaves part of a body behind, as curl can.
        let written = fs::write(out, body.as_deref().unwrap_or(b"partial"));
        written.map_err(|e| e.to_string())?;
        body.map(|_| ())
    }
}

/// A fresh, empty directory under the system temporary directory, removed when dropped.
pub(crate) struct TempDir(pub(crate) PathBuf);

impl TempDir {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("qs-xtask-corpus-{}-{n}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Best effort: a leftover temporary directory is harmless.
        if fs::remove_dir_all(&self.0).is_err() {}
    }
}

pub(crate) const DEFS: &[u8] = b"void(string s) precache_model = #20;\n";

/// A repository archive as GitHub serves it: every path under `r-<commit>/`.
pub(crate) fn repo_archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let named: Vec<(String, &[u8])> = files
        .iter()
        .map(|(n, d)| (format!("r-{COMMIT}/{n}"), *d))
        .collect();
    let refs: Vec<(&str, &[u8])> = named.iter().map(|(n, d)| (n.as_str(), *d)).collect();
    tar_gz(&refs)
}

/// One source `s` at [`COMMIT`] pinning `qc/defs.qc` and a map, codebase `rr` in `qc/`, map
/// `m1`, and the never entry `quakec-1.06`.
pub(crate) fn manifest(defs_sum: &str) -> Manifest {
    let map_sum = hex_digest(b"{ \"classname\" \"worldspawn\" }");
    Manifest {
        source: vec![Source {
            id: "s".to_owned(),
            url: "https://github.com/o/r".to_owned(),
            commit: COMMIT.to_owned(),
            checksums: [
                ("qc/defs.qc".to_owned(), defs_sum.to_owned()),
                ("COPYING".to_owned(), "0".repeat(64)),
                ("maps/m1.map".to_owned(), map_sum),
            ]
            .into(),
        }],
        codebase: vec![
            Entry {
                id: "rr".to_owned(),
                source: Some("s".to_owned()),
                path: Some("qc".to_owned()),
                use_: "vendor".to_owned(),
                ..Entry::default()
            },
            Entry {
                id: "quakec-1.06".to_owned(),
                use_: "never".to_owned(),
                reason: "no license".to_owned(),
                ..Entry::default()
            },
        ],
        map: vec![Entry {
            id: "m1".to_owned(),
            source: Some("s".to_owned()),
            path: Some("maps/m1.map".to_owned()),
            use_: "vendor".to_owned(),
            ..Entry::default()
        }],
        ..Manifest::default()
    }
}

pub(crate) fn archive_url() -> String {
    format!("https://github.com/o/r/archive/{COMMIT}.tar.gz")
}

fn map_url() -> String {
    format!("https://raw.githubusercontent.com/o/r/{COMMIT}/maps/m1.map")
}

/// The archive of source `s`: the codebase in `qc/`, a sibling directory, and a top-level
/// file.
pub(crate) fn standard_archive() -> Vec<u8> {
    repo_archive(&[
        ("COPYING", b"GPL"),
        ("qc/defs.qc", DEFS),
        ("qc/sub/world.qc", b"void() main = {};"),
        ("qc/progs.src", b"defs.qc"),
        ("qcother/x.qc", b"elsewhere"),
    ])
}

fn written(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path.strip_prefix(dir).unwrap();
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn a_codebase_is_written_from_its_directory_only() {
    let tmp = TempDir::new();
    let net = FakeNet::serving(&archive_url(), &standard_archive());
    let (dir, count) = write_codebase(&manifest(&hex_digest(DEFS)), "rr", &tmp.0, &net).unwrap();
    assert_eq!(dir, tmp.0.join("codebase").join("rr"));
    assert_eq!(count, 3);
    assert_eq!(written(&dir), ["defs.qc", "progs.src", "sub/world.qc"]);
    assert_eq!(fs::read(dir.join("defs.qc")).unwrap(), DEFS);
    assert_eq!(*net.requests.borrow(), [archive_url()]);
}

// From Quake-Tools only qcc/v101qc becomes corpus: the compiler beside it is not extracted.
#[test]
fn from_quake_tools_only_v101qc_is_extracted() {
    let tmp = TempDir::new();
    let archive = repo_archive(&[
        ("qcc/COPYING", b"GPL"),
        ("qcc/qcc.c", b"int main() {}"),
        ("qcc/pr_comp.c", b"/* compiler */"),
        ("qcc/v101qc/defs.qc", DEFS),
        ("qcc/v101qc/progs.src", b"../progs.dat\ndefs.qc"),
        ("qcc/v101qcextra/x.qc", b"not this"),
        ("WinQuake/pr_exec.c", b"/* engine */"),
    ]);
    let mut m = manifest(&hex_digest(DEFS));
    m.source[0].checksums = [("qcc/v101qc/defs.qc".to_owned(), hex_digest(DEFS))].into();
    m.codebase[0].path = Some("qcc/v101qc".to_owned());
    let net = FakeNet::serving(&archive_url(), &archive);
    let (dir, _) = write_codebase(&m, "rr", &tmp.0, &net).unwrap();
    assert_eq!(written(&dir), ["defs.qc", "progs.src"]);
    assert_eq!(
        written(&tmp.0.join("codebase")),
        ["rr/defs.qc", "rr/progs.src"]
    );
}

#[test]
fn a_whole_repository_codebase_has_path_dot() {
    let tmp = TempDir::new();
    let mut m = manifest(&hex_digest(DEFS));
    m.codebase[0].path = Some(".".to_owned());
    m.source[0].checksums = [
        ("qc/defs.qc".to_owned(), hex_digest(DEFS)),
        ("COPYING".to_owned(), hex_digest(b"GPL")),
    ]
    .into();
    let net = FakeNet::serving(&archive_url(), &standard_archive());
    let files = codebase_files(&m, "rr", &tmp.0, &net).unwrap();
    assert!(files.contains_key("COPYING") && files.contains_key("qc/defs.qc"));
    assert_eq!(files.len(), 5);
}

// Works offline from cache: the archive is downloaded once, then read from the cache.
#[test]
fn a_cached_archive_is_used_offline() {
    let tmp = TempDir::new();
    let m = manifest(&hex_digest(DEFS));
    let net = FakeNet::serving(&archive_url(), &standard_archive());
    write_codebase(&m, "rr", &tmp.0, &net).unwrap();
    let offline = FakeNet::default();
    let (dir, count) = write_codebase(&m, "rr", &tmp.0, &offline).unwrap();
    assert_eq!(count, 3);
    assert_eq!(offline.calls(), 0);
    assert_eq!(fs::read(dir.join("defs.qc")).unwrap(), DEFS);
    let cached = tmp.0.join("downloads").join(format!("s-{COMMIT}.tar.gz"));
    assert!(cached.is_file());
}

#[test]
fn a_corrupt_cached_archive_is_downloaded_again() {
    let tmp = TempDir::new();
    let m = manifest(&hex_digest(DEFS));
    let cached = tmp.0.join("downloads").join(format!("s-{COMMIT}.tar.gz"));
    fs::create_dir_all(cached.parent().unwrap()).unwrap();
    fs::write(&cached, b"truncated").unwrap();
    let net = FakeNet::serving(&archive_url(), &standard_archive());
    write_codebase(&m, "rr", &tmp.0, &net).unwrap();
    assert_eq!(net.calls(), 1);
    assert_eq!(fs::read(&cached).unwrap(), standard_archive());
}

#[test]
fn a_pinned_checksum_mismatch_writes_nothing() {
    let tmp = TempDir::new();
    let m = manifest(&"1".repeat(64));
    let net = FakeNet::serving(&archive_url(), &standard_archive());
    let err = write_codebase(&m, "rr", &tmp.0, &net).unwrap_err();
    assert!(err.contains("s:qc/defs.qc: sha256"), "{err}");
    assert!(!tmp.0.join("codebase").exists());
    assert_eq!(written(&tmp.0.join("downloads")), Vec::<String>::new());
    let missing = repo_archive(&[("qc/progs.src", b"x")]);
    let net = FakeNet::serving(&archive_url(), &missing);
    let err = write_codebase(&manifest(&hex_digest(DEFS)), "rr", &tmp.0, &net).unwrap_err();
    assert!(err.contains("missing from the archive"), "{err}");
}

#[test]
fn a_failed_download_leaves_no_partial_file() {
    let tmp = TempDir::new();
    let err = write_codebase(
        &manifest(&hex_digest(DEFS)),
        "rr",
        &tmp.0,
        &FakeNet::default(),
    )
    .unwrap_err();
    assert!(err.contains("failed: 404 Not Found"), "{err}");
    assert_eq!(written(&tmp.0.join("downloads")), Vec::<String>::new());
    let net = FakeNet::serving(&archive_url(), b"not a tarball");
    let err = write_codebase(&manifest(&hex_digest(DEFS)), "rr", &tmp.0, &net).unwrap_err();
    assert!(err.contains("not a gzip stream"), "{err}");
    assert_eq!(written(&tmp.0.join("downloads")), Vec::<String>::new());
}

#[test]
fn an_unsafe_path_in_the_archive_is_refused() {
    let tmp = TempDir::new();
    let evil = repo_archive(&[("qc/defs.qc", DEFS), ("qc/../../evil.qc", b"x")]);
    let net = FakeNet::serving(&archive_url(), &evil);
    let err = write_codebase(&manifest(&hex_digest(DEFS)), "rr", &tmp.0, &net).unwrap_err();
    assert!(err.contains("not a safe relative path"), "{err}");
    assert!(!tmp.0.join("evil.qc").exists());
}

// The unsafe member sorts after the safe ones, and still no file of the archive is written.
#[test]
fn an_archive_with_an_unsafe_member_writes_no_file() {
    let tmp = TempDir::new();
    let evil = repo_archive(&[("qc/defs.qc", DEFS), ("qc/zz/../../evil.qc", b"x")]);
    let net = FakeNet::serving(&archive_url(), &evil);
    let err = write_codebase(&manifest(&hex_digest(DEFS)), "rr", &tmp.0, &net).unwrap_err();
    assert!(
        err.contains("\"zz/../../evil.qc\" is not a safe relative path"),
        "{err}"
    );
    assert!(!tmp.0.join("codebase").exists());
    assert!(!tmp.0.join("evil.qc").exists());
}

#[test]
fn a_tree_with_an_unsafe_directory_writes_no_codebase() {
    let tmp = TempDir::new();
    let mut m = tree_manifest();
    m.tree[0].dirs = [
        ("src".to_owned(), "rr".to_owned()),
        ("z/../../up".to_owned(), "rr".to_owned()),
    ]
    .into();
    let net = FakeNet::serving(&archive_url(), &standard_archive());
    let err = write_tree(&m, "t", &tmp.0, &net).unwrap_err();
    assert!(
        err.contains("\"z/../../up\" is not a safe relative path"),
        "{err}"
    );
    assert!(!tmp.0.join("tree").exists());
}

// The second codebase's archive member is unsafe; the first codebase is not written either.
#[test]
fn a_tree_with_an_unsafe_member_in_a_later_codebase_writes_nothing() {
    let tmp = TempDir::new();
    let mut m = tree_manifest();
    m.tree[0].dirs = [
        ("a".to_owned(), "rr".to_owned()),
        ("b".to_owned(), "other".to_owned()),
    ]
    .into();
    let evil = repo_archive(&[("qc/defs.qc", DEFS), ("qcother/zz/../../x.qc", b"x")]);
    let net = FakeNet::serving(&archive_url(), &evil);
    let err = write_tree(&m, "t", &tmp.0, &net).unwrap_err();
    assert!(
        err.contains("codebase other: \"zz/../../x.qc\" is not a safe relative path"),
        "{err}"
    );
    assert!(!tmp.0.join("tree").exists());
}

// Refuses entries marked never, from any table, and unknown ids.
#[test]
fn never_and_unknown_entries_are_refused() {
    let tmp = TempDir::new();
    let m = manifest(&hex_digest(DEFS));
    let net = FakeNet::default();
    let errs = cmd_fetch(
        &m,
        &tmp.0,
        &["quakec-1.06".to_owned(), "nosuch".to_owned()],
        &net,
    );
    assert_eq!(errs.len(), 2);
    assert!(
        errs[0].contains("quakec-1.06: use = never, so it is never fetched"),
        "{}",
        errs[0]
    );
    assert!(errs[1].contains("nosuch: no such entry"), "{}", errs[1]);
    assert_eq!(net.calls(), 0);
    let err = write_codebase(&m, "m1", &tmp.0, &net).unwrap_err();
    assert!(err.contains("is a map, not a codebase"), "{err}");
    let mut m = m;
    m.codebase[0].source = Some("gone".to_owned());
    let err = write_codebase(&m, "rr", &tmp.0, &net).unwrap_err();
    assert!(err.contains("unknown source"), "{err}");
}

#[test]
fn a_map_is_fetched_once_and_checked() {
    let tmp = TempDir::new();
    let m = manifest(&hex_digest(DEFS));
    let body = b"{ \"classname\" \"worldspawn\" }";
    let net = FakeNet::serving(&map_url(), body);
    assert_eq!(
        cmd_fetch(&m, &tmp.0, &["m1".to_owned()], &net),
        Vec::<String>::new()
    );
    assert_eq!(fs::read(tmp.0.join("map/m1.map")).unwrap(), body);
    let offline = FakeNet::default();
    assert_eq!(
        cmd_fetch(&m, &tmp.0, &["m1".to_owned()], &offline),
        Vec::<String>::new()
    );
    assert_eq!(offline.calls(), 0);

    let other = TempDir::new();
    let wrong = FakeNet::serving(&map_url(), b"changed");
    let errs = cmd_fetch(&m, &other.0, &["m1".to_owned()], &wrong);
    assert!(errs[0].contains("manifest says"), "{}", errs[0]);
    assert!(!other.0.join("map/m1.map").exists());
    let errs = cmd_fetch(&m, &other.0, &["m1".to_owned()], &FakeNet::default());
    assert!(errs[0].contains("404"), "{}", errs[0]);
}

#[test]
fn no_ids_fetches_every_usable_codebase_and_map() {
    let tmp = TempDir::new();
    let m = manifest(&hex_digest(DEFS));
    let mut net = FakeNet::serving(&archive_url(), &standard_archive());
    net.bodies
        .insert(map_url(), b"{ \"classname\" \"worldspawn\" }".to_vec());
    assert_eq!(cmd_fetch(&m, &tmp.0, &[], &net), Vec::<String>::new());
    assert!(tmp.0.join("codebase/rr/defs.qc").is_file());
    assert!(tmp.0.join("map/m1.map").is_file());
    assert!(!tmp.0.join("codebase/quakec-1.06").exists());
}

/// [`manifest`] with a second codebase `other` in `qcother/`, and tree `t` laying `rr` out at
/// `src` and `other` at `game/src`.
fn tree_manifest() -> Manifest {
    let mut m = manifest(&hex_digest(DEFS));
    m.codebase.push(Entry {
        id: "other".to_owned(),
        source: Some("s".to_owned()),
        path: Some("qcother".to_owned()),
        use_: "vendor".to_owned(),
        ..Entry::default()
    });
    m.tree.push(Tree {
        id: "t".to_owned(),
        dirs: [
            ("src".to_owned(), "rr".to_owned()),
            ("game/src".to_owned(), "other".to_owned()),
        ]
        .into(),
        ..Tree::default()
    });
    m
}

#[test]
fn a_tree_writes_each_codebase_to_its_directory() {
    let tmp = TempDir::new();
    let m = tree_manifest();
    let net = FakeNet::serving(&archive_url(), &standard_archive());
    assert_eq!(
        cmd_fetch(&m, &tmp.0, &["t".to_owned()], &net),
        Vec::<String>::new()
    );
    assert_eq!(
        written(&tmp.0.join("tree/t")),
        [
            "game/src/x.qc",
            "src/defs.qc",
            "src/progs.src",
            "src/sub/world.qc",
        ]
    );
    assert_eq!(net.calls(), 1);
    assert!(!tmp.0.join("codebase").exists());
    let (dir, count) = write_tree(&m, "t", &tmp.0, &FakeNet::default()).unwrap();
    assert_eq!((dir, count), (tmp.0.join("tree/t"), 4));
}

// No ids fetches every tree too; a tree's failures name the tree.
#[test]
fn a_tree_is_fetched_with_everything_and_fails_by_name() {
    let tmp = TempDir::new();
    let mut m = tree_manifest();
    let mut net = FakeNet::serving(&archive_url(), &standard_archive());
    net.bodies
        .insert(map_url(), b"{ \"classname\" \"worldspawn\" }".to_vec());
    assert_eq!(cmd_fetch(&m, &tmp.0, &[], &net), Vec::<String>::new());
    assert!(tmp.0.join("tree/t/game/src/x.qc").is_file());

    let other = TempDir::new();
    let err = write_tree(&m, "t", &other.0, &FakeNet::default()).unwrap_err();
    assert!(err.starts_with("tree t: codebase "), "{err}");
    let err = write_tree(&m, "nosuch", &other.0, &FakeNet::default()).unwrap_err();
    assert!(err.contains("tree nosuch: no such tree"), "{err}");
    m.tree[0].dirs = [("../up".to_owned(), "rr".to_owned())].into();
    let err = write_tree(&m, "t", &other.0, &net).unwrap_err();
    assert!(
        err.contains("\"../up\" is not a safe relative path"),
        "{err}"
    );
    assert!(!other.0.join("up").exists());
}

// A codebase's project file from this repository becomes its quakec.toml.
#[test]
fn a_project_file_is_written_as_quakec_toml() {
    let tmp = TempDir::new();
    let mut m = manifest(&hex_digest(DEFS));
    m.dir = tmp.0.join("repo");
    fs::create_dir_all(m.dir.join("projects")).unwrap();
    fs::write(m.dir.join("projects/rr.toml"), b"[[module]]\n").unwrap();
    m.codebase[0].project = Some("projects/rr.toml".to_owned());
    let cache = tmp.0.join("cache");
    let net = FakeNet::serving(&archive_url(), &standard_archive());
    let (dir, count) = write_codebase(&m, "rr", &cache, &net).unwrap();
    assert_eq!(count, 4);
    assert_eq!(fs::read(dir.join("quakec.toml")).unwrap(), b"[[module]]\n");

    m.codebase[0].project = Some("projects/gone.toml".to_owned());
    let err = write_codebase(&m, "rr", &cache, &net).unwrap_err();
    assert!(
        err.contains("codebase rr: ") && err.contains("gone.toml"),
        "{err}"
    );
}

// From GitLab only the codebase's directory is downloaded, and cached under its own name.
#[test]
fn a_gitlab_codebase_downloads_only_its_directory() {
    let tmp = TempDir::new();
    let mut m = manifest(&hex_digest(DEFS));
    m.source[0].url = "https://gitlab.com/o/r".to_owned();
    m.codebase[0].path = Some("qc/".to_owned());
    let url = format!(
        "https://gitlab.com/api/v4/projects/o%2Fr/repository/archive.tar.gz?sha={COMMIT}&path=qc"
    );
    let scoped = repo_archive(&[("qc/defs.qc", DEFS), ("qc/progs.src", b"defs.qc")]);
    let net = FakeNet::serving(&url, &scoped);
    let (dir, count) = write_codebase(&m, "rr", &tmp.0, &net).unwrap();
    assert_eq!(count, 2);
    assert_eq!(written(&dir), ["defs.qc", "progs.src"]);
    assert_eq!(*net.requests.borrow(), [url]);
    let cached = tmp
        .0
        .join("downloads")
        .join(format!("s-{COMMIT}-qc.tar.gz"));
    assert!(cached.is_file());
}
