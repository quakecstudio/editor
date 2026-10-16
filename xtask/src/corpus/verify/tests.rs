// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::collections::BTreeMap;
use std::path::Path;

use super::super::manifest::{Entry, Manifest, Source};
use super::super::{MANIFEST, tracked_files};
use super::{archive_url, check, check_online, check_repo, raw_url};
use crate::download::Net;
use crate::oracles::sha256::hex_digest;

const H1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const H2: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn real() -> Manifest {
    Manifest::load(&root().join(MANIFEST)).unwrap()
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn codebase(id: &str, kind: &str) -> Entry {
    Entry {
        id: id.to_owned(),
        kind: kind.to_owned(),
        source: Some("src".to_owned()),
        path: Some(".".to_owned()),
        license: "GPL-2.0-or-later".to_owned(),
        use_: "vendor".to_owned(),
        evidence: strings(&["LICENSE"]),
        ..Entry::default()
    }
}

/// A minimal manifest that passes every offline check.
fn base() -> Manifest {
    let mut checksums = map(&[("LICENSE", H1)]);
    for i in 0..20 {
        checksums.insert(format!("m{i}.map"), format!("{i:064x}"));
    }
    let mut m = Manifest {
        schema: Some(1),
        never: strings(&["quakec-1.06", "id1-paks"]),
        blocked_urls: strings(&["https://github.com/maddes-b/QuakeC-releases"]),
        source: vec![Source {
            id: "src".to_owned(),
            url: "https://github.com/o/r".to_owned(),
            commit: COMMIT.to_owned(),
            checksums,
        }],
        codebase: vec![Entry {
            id: "quakec-1.06".to_owned(),
            kind: "id".to_owned(),
            license: "NONE".to_owned(),
            use_: "never".to_owned(),
            reason: "no license".to_owned(),
            ..Entry::default()
        }],
        asset: vec![
            Entry {
                id: "lq".to_owned(),
                source: Some("src".to_owned()),
                url: "https://github.com/o/r/releases/download/v1/lite.zip".to_owned(),
                sha256: H2.to_owned(),
                license: "BSD-3-Clause".to_owned(),
                use_: "fetch-at-test".to_owned(),
                evidence: strings(&["LICENSE"]),
                covers: strings(&["rerelease"]),
                strip: "lite/".to_owned(),
                game_dir: "id1".to_owned(),
                members: map(&[("lite/id1/pak0.pak", H2), ("lite/id1/docs/LICENSE", H1)]),
                stand_ins: map(&[("sound/a.wav", "sound/b.wav")]),
                ..Entry::default()
            },
            Entry {
                id: "id1-paks".to_owned(),
                license: "LicenseRef-id-Quake-EULA".to_owned(),
                use_: "never".to_owned(),
                reason: "commercial game data".to_owned(),
                ..Entry::default()
            },
        ],
        ..Manifest::default()
    };
    for id in ["id-1.01", "rerelease"] {
        m.codebase.push(codebase(id, "id"));
    }
    for i in 0..10 {
        m.codebase.push(codebase(&format!("mod{i}"), "mod"));
    }
    for i in 0..20 {
        m.map.push(Entry {
            id: format!("map{i}"),
            source: Some("src".to_owned()),
            path: Some(format!("m{i}.map")),
            license: "BSD-3-Clause".to_owned(),
            use_: "vendor".to_owned(),
            evidence: strings(&["LICENSE"]),
            ..Entry::default()
        });
    }
    m
}

fn cb<'m>(m: &'m mut Manifest, id: &str) -> &'m mut Entry {
    m.codebase.iter_mut().find(|e| e.id == id).unwrap()
}

fn asset<'m>(m: &'m mut Manifest, id: &str) -> &'m mut Entry {
    m.asset.iter_mut().find(|e| e.id == id).unwrap()
}

fn never(e: &mut Entry) {
    e.use_ = "never".to_owned();
    e.reason = "r".to_owned();
    e.source = None;
    e.path = None;
}

fn errs(m: &Manifest) -> String {
    check(m).join("\n")
}

#[track_caller]
fn assert_has(text: &str, part: &str) {
    assert!(text.contains(part), "{part:?} not in:\n{text}");
}

#[test]
fn the_real_manifest_passes() {
    assert_eq!(check(&real()), Vec::<String>::new());
}

#[test]
fn the_repository_tracks_no_corpus_files() {
    let files = tracked_files(root()).unwrap();
    assert!(files.iter().any(|f| f == MANIFEST));
    let read = |f: &str| std::fs::read(root().join(f)).map_err(|e| e.to_string());
    assert_eq!(check_repo(&real(), &files, &read), Vec::<String>::new());
}

// LIC-5: QuakeC 1.06 is listed as never and names no source.
#[test]
fn quakec_106_is_never_in_the_real_manifest() {
    let mut m = real();
    assert!(m.never.iter().any(|n| n == "quakec-1.06"));
    let e = cb(&mut m, "quakec-1.06");
    assert_eq!(e.use_, "never");
    assert!(e.source.is_none());
}

#[test]
fn every_usable_real_entry_has_evidence() {
    let m = real();
    for e in m.codebase.iter().chain(&m.map).filter(|e| e.usable()) {
        assert!(!e.evidence.is_empty(), "{}", e.id);
    }
}

// LIC-4: id's paks are never; LibreQuake stands in for them and covers the rerelease.
#[test]
fn the_real_manifest_pins_librequake_and_never_id_paks() {
    let mut m = real();
    assert!(m.never.iter().any(|n| n == "id1-paks"));
    assert_eq!(asset(&mut m, "id1-paks").use_, "never");
    assert!(
        asset(&mut m, "librequake-lite")
            .covers
            .iter()
            .any(|c| c == "rerelease")
    );
    assert!(m.asset.iter().all(|e| e.use_ != "vendor"));
}

#[test]
fn an_unknown_field_is_a_parse_error() {
    let text = "schema = 1\n[[codebase]]\nid = \"x\"\nlicence = \"MIT\"\n";
    assert!(toml::from_str::<Manifest>(text).is_err());
}

#[test]
fn the_base_manifest_passes() {
    assert_eq!(check(&base()), Vec::<String>::new());
}

#[test]
fn schema_must_be_1() {
    let mut m = base();
    m.schema = Some(2);
    assert_has(&errs(&m), "schema must be 1");
    m.schema = None;
    assert_has(&errs(&m), "schema must be 1");
}

#[test]
fn source_fields_are_checked() {
    let mut m = base();
    m.source[0].url = "http://example.com/x".to_owned();
    m.source[0].commit = "main".to_owned();
    m.source[0]
        .checksums
        .insert("LICENSE".to_owned(), "abc".to_owned());
    let out = errs(&m);
    assert_has(&out, "url must be");
    assert_has(&out, "full 40-character SHA");
    assert_has(&out, "LICENSE: bad sha256");
    for url in [
        "https://github.com/o",
        "https://github.com/o/r/x",
        "https://gitlab.com//r",
    ] {
        m.source[0].url = url.to_owned();
        assert_has(&errs(&m), "url must be");
    }
}

#[test]
fn a_source_needs_checksums() {
    let mut m = base();
    m.source.push(Source {
        id: "empty".to_owned(),
        url: "https://github.com/o/e".to_owned(),
        commit: COMMIT.to_owned(),
        checksums: BTreeMap::new(),
    });
    assert_has(&errs(&m), "source empty: no pinned checksums");
}

#[test]
fn duplicate_and_bad_ids_are_reported() {
    let mut m = base();
    m.source.push(m.source[0].clone());
    m.source.push(Source {
        id: "Bad_Src".to_owned(),
        ..m.source[0].clone()
    });
    m.codebase.push(codebase("mod0", "mod"));
    m.codebase.push(codebase("Bad_ID", "mod"));
    let out = errs(&m);
    assert_has(&out, "source src: duplicate id");
    assert_has(&out, "source \"Bad_Src\": bad id");
    assert_has(&out, "codebase mod0: duplicate id");
    assert_has(&out, "codebase Bad_ID: bad id");
}

#[test]
fn a_blocked_url_is_refused() {
    let mut m = base();
    m.source.push(Source {
        id: "q106".to_owned(),
        url: "https://github.com/Maddes-B/QuakeC-releases/".to_owned(),
        commit: COMMIT.to_owned(),
        checksums: map(&[("x", H1)]),
    });
    assert_has(&errs(&m), "is blocked");
}

#[test]
fn a_license_is_required() {
    let mut m = base();
    cb(&mut m, "mod0").license = String::new();
    cb(&mut m, "mod1").license = "GPL v2".to_owned();
    let out = errs(&m);
    assert_has(&out, "codebase mod0: license must be");
    assert_has(&out, "codebase mod1: license must be");
}

#[test]
fn use_and_kind_are_checked() {
    let mut m = base();
    cb(&mut m, "mod0").use_ = "bundle".to_owned();
    cb(&mut m, "mod1").kind = "map".to_owned();
    let out = errs(&m);
    assert_has(&out, "mod0: use must be one of");
    assert_has(&out, "mod1: kind must be one of");
}

#[test]
fn vendor_needs_a_compatible_license() {
    for license in ["GPL-2.0-only", "LicenseRef-no-commercial", "NONE"] {
        let mut m = base();
        cb(&mut m, "mod0").license = license.to_owned();
        assert_has(&errs(&m), "cannot be vendor");
        cb(&mut m, "mod0").use_ = "fetch-at-test".to_owned();
        assert_eq!(check(&m), Vec::<String>::new(), "{license}");
    }
}

#[test]
fn evidence_is_required_and_pinned() {
    let mut m = base();
    cb(&mut m, "mod0").evidence = Vec::new();
    cb(&mut m, "mod1").evidence = strings(&["README.md"]);
    cb(&mut m, "mod2").evidence = strings(&["nosuch:LICENSE"]);
    let out = errs(&m);
    assert_has(&out, "mod0: no license evidence");
    assert_has(&out, "mod1: evidence README.md has no pinned checksum");
    assert_has(&out, "mod2: evidence nosuch:LICENSE: unknown source");
}

#[test]
fn evidence_may_be_in_another_source() {
    let mut m = base();
    m.source.push(Source {
        id: "other".to_owned(),
        url: "https://gitlab.com/o/x".to_owned(),
        commit: COMMIT.to_owned(),
        checksums: map(&[("COPYING", H2)]),
    });
    cb(&mut m, "mod0").evidence = strings(&["other:COPYING"]);
    assert_eq!(check(&m), Vec::<String>::new());
    cb(&mut m, "mod0").evidence = strings(&["other:LICENSE"]);
    assert_has(&errs(&m), "evidence other:LICENSE has no pinned checksum");
}

#[test]
fn an_unknown_source_or_missing_path_is_reported() {
    let mut m = base();
    cb(&mut m, "mod0").source = Some("nosuch".to_owned());
    cb(&mut m, "mod1").path = None;
    let out = errs(&m);
    assert_has(&out, "mod0: unknown source \"nosuch\"");
    assert_has(&out, "mod1: missing path");
}

#[test]
fn a_map_must_be_a_pinned_map_file() {
    let mut m = base();
    m.map[0].path = Some("m0.bsp".to_owned());
    m.map[1].path = Some("unpinned.map".to_owned());
    let out = errs(&m);
    assert_has(&out, "map map0: path must be a .map file");
    assert_has(&out, "map map1: unpinned.map has no pinned checksum");
}

#[test]
fn a_never_entry_has_a_reason_and_no_source() {
    let mut m = base();
    let e = cb(&mut m, "quakec-1.06");
    e.source = Some("src".to_owned());
    e.reason = String::new();
    let out = errs(&m);
    assert_has(&out, "must not name a source");
    assert_has(&out, "needs a reason");
}

#[test]
fn the_never_list_matches_the_never_entries() {
    let mut m = base();
    m.never = strings(&["quakec-1.06", "ghost"]);
    never(cb(&mut m, "mod0"));
    let out = errs(&m);
    assert_has(&out, "never list: ghost has no entry");
    assert_has(&out, "mod0: use = never but missing from the never list");
}

// LIC-5: the never list must name QuakeC 1.06.
#[test]
fn quakec_106_must_be_never() {
    let mut m = base();
    m.never = strings(&["id1-paks"]);
    m.codebase.retain(|e| e.id != "quakec-1.06");
    assert_has(&errs(&m), "must include quakec-1.06");
}

#[test]
fn required_codebases_and_minimums_are_checked() {
    let mut m = base();
    m.codebase.retain(|e| e.id != "rerelease" && e.id != "mod9");
    m.map.pop();
    let out = errs(&m);
    assert_has(&out, "missing required codebase rerelease");
    assert_has(&out, "9 usable mods; need at least 10");
    assert_has(&out, "19 usable maps; need at least 20");
}

#[test]
fn never_entries_do_not_count() {
    let mut m = base();
    never(cb(&mut m, "id-1.01"));
    m.never.push("id-1.01".to_owned());
    assert_has(&errs(&m), "missing required codebase id-1.01");
}

#[test]
fn an_asset_url_must_be_a_release_zip_of_its_source() {
    for url in [
        "https://github.com/other/r/releases/download/v1/lite.zip",
        "https://github.com/o/r/archive/v1.zip",
        "https://github.com/o/r/releases/download/v1/lite.tar.gz",
        "http://github.com/o/r/releases/download/v1/lite.zip",
        "https://github.com/o/r/releases/download//lite.zip",
        "https://github.com/o/r/releases/download/v1/.zip",
    ] {
        let mut m = base();
        asset(&mut m, "lq").url = url.to_owned();
        assert_has(&errs(&m), "url must be a .zip release download");
    }
}

#[test]
fn asset_hashes_are_checked() {
    let mut m = base();
    asset(&mut m, "lq").sha256 = "x".to_owned();
    asset(&mut m, "lq")
        .members
        .insert("lite/id1/pak1.pak".to_owned(), "y".to_owned());
    let out = errs(&m);
    assert_has(&out, "asset lq: bad sha256");
    assert_has(&out, "member lite/id1/pak1.pak: bad sha256");
}

#[test]
fn unsafe_members_are_refused() {
    for name in [
        "lite/../x",
        "/etc/passwd",
        "lite/a\\b",
        "C:/x",
        "lite/",
        "other/x",
        "lite//x",
    ] {
        let mut m = base();
        asset(&mut m, "lq")
            .members
            .insert(name.to_owned(), H2.to_owned());
        assert_has(&errs(&m), "must be a safe path");
    }
}

#[test]
fn an_asset_needs_members() {
    let mut m = base();
    asset(&mut m, "lq").members.clear();
    assert_has(&errs(&m), "no pinned members");
}

#[test]
fn asset_evidence_must_be_in_the_archive() {
    let mut m = base();
    asset(&mut m, "lq").members.remove("lite/id1/docs/LICENSE");
    assert_has(
        &errs(&m),
        "evidence LICENSE is not among the archive's pinned members",
    );
    let mut m = base();
    asset(&mut m, "lq").evidence = strings(&["NOTICE"]);
    assert_has(&errs(&m), "evidence NOTICE has no pinned checksum");
    let mut m = base();
    asset(&mut m, "lq").evidence = Vec::new();
    assert_has(&errs(&m), "asset lq: no license evidence");
}

#[test]
fn an_asset_source_must_exist() {
    let mut m = base();
    asset(&mut m, "lq").source = Some("nosuch".to_owned());
    assert_has(&errs(&m), "asset lq: unknown source \"nosuch\"");
}

#[test]
fn game_dir_must_be_relative() {
    for bad in ["", "../id1", "/id1"] {
        let mut m = base();
        asset(&mut m, "lq").game_dir = bad.to_owned();
        assert_has(&errs(&m), "game_dir must be");
    }
}

#[test]
fn stand_ins_are_relative() {
    let mut m = base();
    asset(&mut m, "lq").stand_ins = map(&[("../x.wav", "sound/b.wav")]);
    assert_has(&errs(&m), "stand-in");
}

#[test]
fn an_asset_cannot_be_vendor() {
    let mut m = base();
    asset(&mut m, "lq").use_ = "vendor".to_owned();
    assert_has(&errs(&m), "use must be fetch-at-test or never");
}

// LIC-4: the never list must name id's game data.
#[test]
fn id_game_data_must_be_never() {
    let mut m = base();
    m.never.retain(|n| n != "id1-paks");
    let out = errs(&m);
    assert_has(&out, "never list must include id1-paks");
    assert_has(
        &out,
        "id1-paks: use = never but missing from the never list",
    );
}

#[test]
fn a_never_asset_needs_a_reason_and_no_source() {
    let mut m = base();
    let e = asset(&mut m, "id1-paks");
    e.reason = String::new();
    e.source = Some("src".to_owned());
    let out = errs(&m);
    assert_has(&out, "asset id1-paks: a never entry needs a reason");
    assert_has(&out, "asset id1-paks: a never entry must not name a source");
}

#[test]
fn covers_names_usable_codebases_and_the_rerelease() {
    let mut m = base();
    asset(&mut m, "lq").covers = strings(&["nosuch"]);
    let out = errs(&m);
    assert_has(&out, "asset lq: covers unknown codebase nosuch");
    assert_has(&out, "no test asset covers the rerelease");
}

#[test]
fn ids_are_unique_across_tables() {
    let mut m = base();
    asset(&mut m, "lq").id = "mod0".to_owned();
    assert_has(&errs(&m), "asset mod0: duplicate id");
}

fn repo(files: &[(&str, &[u8])], fixture_dirs: &[&str]) -> String {
    let mut m = base();
    m.fixture_dirs = strings(fixture_dirs);
    let names: Vec<String> = files.iter().map(|(n, _)| (*n).to_owned()).collect();
    let read = |f: &str| {
        files
            .iter()
            .find(|(n, _)| *n == f)
            .map(|(_, d)| d.to_vec())
            .ok_or_else(|| "missing".to_owned())
    };
    check_repo(&m, &names, &read).join("\n")
}

#[test]
fn a_clean_repository_passes() {
    assert_eq!(
        repo(&[("README.md", b"x"), ("corpus/manifest.toml", b"y")], &[]),
        ""
    );
}

// LIC-4: a pak file is never committed, even among fixtures.
#[test]
fn pak_files_are_never_tracked() {
    let out = repo(&[("tests/fixtures/PAK0.PAK", b"PACK")], &["tests/fixtures"]);
    assert_has(&out, "pak files");
}

// LIC-7: corpus files are fetched, not committed.
#[test]
fn quake_content_must_be_in_a_fixture_dir() {
    assert_has(
        &repo(&[("src/defs.qc", b"float x;")], &[]),
        "src/defs.qc: Quake content outside",
    );
    assert_eq!(
        repo(
            &[("tests/fixtures/a.qc", b"float x;")],
            &["tests/fixtures/"]
        ),
        ""
    );
    assert_eq!(
        repo(&[(".qc", b"x"), ("a.", b"x"), ("Makefile", b"x")], &[]),
        ""
    );
    assert_has(&repo(&[("src/a.qc", b"x")], &["src/a"]), "outside");
}

#[test]
fn a_copy_of_a_corpus_file_is_refused() {
    let data = b"// a corpus map";
    let mut m = base();
    m.source[0]
        .checksums
        .insert("m0.map".to_owned(), hex_digest(data));
    m.fixture_dirs = strings(&["tests/fixtures"]);
    let read = |_: &str| Ok(data.to_vec());
    let out = check_repo(&m, &strings(&["tests/fixtures/x.map"]), &read);
    assert_eq!(
        out,
        ["tests/fixtures/x.map: is a copy of corpus file src:m0.map"]
    );
}

#[test]
fn license_text_is_not_flagged_and_unreadable_files_are() {
    let data = b"GNU GENERAL PUBLIC LICENSE";
    let mut m = base();
    m.source[0]
        .checksums
        .insert("LICENSE".to_owned(), hex_digest(data));
    let read = |_: &str| Ok(data.to_vec());
    assert_eq!(
        check_repo(&m, &strings(&["LICENSE"]), &read),
        Vec::<String>::new()
    );
    m.fixture_dirs = strings(&["f"]);
    let fail = |_: &str| Err("gone".to_owned());
    assert_eq!(
        check_repo(&m, &strings(&["f/a.qc"]), &fail),
        ["f/a.qc: gone"]
    );
}

#[test]
fn raw_and_archive_urls_follow_the_host() {
    assert_eq!(
        raw_url("https://github.com/o/r", COMMIT, "a b/c.qc").unwrap(),
        format!("https://raw.githubusercontent.com/o/r/{COMMIT}/a%20b/c.qc")
    );
    assert_eq!(
        raw_url("https://gitlab.com/o/r", COMMIT, "qcsrc/Makefile").unwrap(),
        format!(
            "https://gitlab.com/api/v4/projects/o%2Fr/repository/files/qcsrc%2FMakefile/raw?ref={COMMIT}"
        )
    );
    assert!(raw_url("https://example.com/o/r", COMMIT, "x").is_err());
    assert_eq!(
        archive_url("https://github.com/o/r", COMMIT).unwrap(),
        format!("https://github.com/o/r/archive/{COMMIT}.tar.gz")
    );
    assert_eq!(
        archive_url("https://gitlab.com/o/r.pk3dir", COMMIT).unwrap(),
        format!(
            "https://gitlab.com/api/v4/projects/o%2Fr.pk3dir/repository/archive.tar.gz?sha={COMMIT}"
        )
    );
    assert!(archive_url("https://example.com/o/r", COMMIT).is_err());
}

/// Serves the same bytes for every URL, or fails every request.
struct Fixed(Result<Vec<u8>, String>);

impl Net for Fixed {
    fn get(&self, _url: &str) -> Result<Vec<u8>, String> {
        self.0.clone()
    }

    fn download(&self, _url: &str, _out: &Path) -> Result<(), String> {
        Err("not used".to_owned())
    }
}

fn online_manifest(data: &[u8]) -> Manifest {
    Manifest {
        source: vec![Source {
            id: "s".to_owned(),
            url: "https://github.com/o/r".to_owned(),
            commit: COMMIT.to_owned(),
            checksums: map(&[("LICENSE", &hex_digest(data))]),
        }],
        ..Manifest::default()
    }
}

#[test]
fn online_checksums_match() {
    let m = online_manifest(b"ok");
    assert_eq!(
        check_online(&m, &Fixed(Ok(b"ok".to_vec()))),
        Vec::<String>::new()
    );
}

#[test]
fn an_online_mismatch_is_reported() {
    let m = online_manifest(b"ok");
    let out = check_online(&m, &Fixed(Ok(b"changed".to_vec())));
    assert_eq!(out.len(), 1);
    assert_has(&out[0], "source s: LICENSE: sha256");
}

#[test]
fn an_online_fetch_failure_is_reported() {
    let m = online_manifest(b"ok");
    let out = check_online(&m, &Fixed(Err("404 Not Found".to_owned())));
    assert_eq!(out, ["source s: LICENSE: fetch failed: 404 Not Found"]);
    let mut m = online_manifest(b"ok");
    m.source[0].url = "https://example.com/o/r".to_owned();
    let out = check_online(&m, &Fixed(Ok(b"ok".to_vec())));
    assert_has(&out[0], "unsupported host");
}
