// SPDX-License-Identifier: GPL-3.0-or-later

//! Everything runs offline against small archives built here; nothing is downloaded.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "test code: a failed unwrap, index, sum or cast is a failed test"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::super::archive::tests::zip;
use super::super::fetch::tests::{DEFS, FakeNet, TempDir, archive_url, manifest, repo_archive};
use super::super::manifest::{Entry, Manifest};
use super::{
    Kind, Precache, check_scenario, cmd_fetch, cmd_precache, coverage, extract, fetch_archive,
    pak_files, precache_calls,
};
use crate::oracles::sha256::hex_digest;

/// A PACK file: header, file data, then the directory.
fn make_pak(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    let mut directory = Vec::new();
    for (name, data) in files {
        let mut field = [0_u8; 56];
        field[..name.len()].copy_from_slice(name.as_bytes());
        directory.extend_from_slice(&field);
        directory.extend_from_slice(&(12 + body.len() as i32).to_le_bytes());
        directory.extend_from_slice(&(data.len() as i32).to_le_bytes());
        body.extend_from_slice(data);
    }
    let mut out = b"PACK".to_vec();
    out.extend_from_slice(&(12 + body.len() as i32).to_le_bytes());
    out.extend_from_slice(&(directory.len() as i32).to_le_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&directory);
    out
}

const SCENARIO: [(&str, &[u8]); 6] = [
    ("gfx/palette.lmp", &[b'p'; 768]),
    ("gfx/colormap.lmp", b"c"),
    ("gfx.wad", b"WAD2"),
    ("maps/start.bsp", b"bsp"),
    ("progs/player.mdl", b"IDPO"),
    ("sound/boss1/death.wav", b"RIFF-boss1"),
];
const LICENSE: &[u8] = b"BSD 3-Clause";
const RELEASE: &str = "https://github.com/o/r/releases/download/v1/lite.zip";

fn pak0() -> Vec<u8> {
    make_pak(&SCENARIO)
}

fn pak1() -> Vec<u8> {
    make_pak(&[("sound/player/slmbrn2.wav", b"RIFF-slime")])
}

fn release() -> Vec<u8> {
    zip(
        &[
            ("lite/id1/pak0.pak", &pak0()),
            ("lite/id1/pak1.pak", &pak1()),
            ("lite/id1/docs/COPYING", LICENSE),
            ("lite/id1/music/track02.ogg", b"not extracted"),
        ],
        true,
    )
}

fn entry() -> Entry {
    Entry {
        id: "lq".to_owned(),
        use_: "fetch-at-test".to_owned(),
        url: RELEASE.to_owned(),
        sha256: hex_digest(&release()),
        strip: "lite/".to_owned(),
        game_dir: "id1".to_owned(),
        members: [
            ("lite/id1/pak0.pak".to_owned(), hex_digest(&pak0())),
            ("lite/id1/pak1.pak".to_owned(), hex_digest(&pak1())),
            ("lite/id1/docs/COPYING".to_owned(), hex_digest(LICENSE)),
        ]
        .into(),
        stand_ins: [
            (
                "sound/boss2/death.wav".to_owned(),
                "sound/boss1/death.wav".to_owned(),
            ),
            (
                "sound/player/slimbrn2.wav".to_owned(),
                "sound/player/slmbrn2.wav".to_owned(),
            ),
        ]
        .into(),
        covers: vec!["rr".to_owned()],
        ..Entry::default()
    }
}

fn net() -> FakeNet {
    FakeNet::serving(RELEASE, &release())
}

fn files_in(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn fetch_extracts_pinned_members_only() {
    let tmp = TempDir::new();
    let archive = fetch_archive(&entry(), &tmp.0, &net()).unwrap();
    let out = extract(&entry(), &archive, &tmp.0).unwrap();
    assert_eq!(out, tmp.0.join("lq"));
    assert_eq!(fs::read(out.join("id1/pak0.pak")).unwrap(), pak0());
    assert_eq!(fs::read(out.join("id1/docs/COPYING")).unwrap(), LICENSE);
    assert!(!out.join("id1/music").exists());
    assert!(!out.join("lite").exists());
}

// Works offline from cache: a cached archive with the right SHA-256 is not downloaded again,
// and extracted members with the right SHA-256 are not read again.
#[test]
fn a_cached_archive_is_not_downloaded_again() {
    let tmp = TempDir::new();
    let net = net();
    fetch_archive(&entry(), &tmp.0, &net).unwrap();
    let archive = fetch_archive(&entry(), &tmp.0, &net).unwrap();
    assert_eq!(net.calls(), 1);
    extract(&entry(), &archive, &tmp.0).unwrap();
    fs::write(&archive, b"gone").unwrap();
    let mut cached = entry();
    cached.sha256 = hex_digest(b"gone");
    assert!(extract(&cached, &archive, &tmp.0).is_ok());
}

#[test]
fn a_corrupt_cache_is_downloaded_again() {
    let tmp = TempDir::new();
    let net = net();
    let archive = fetch_archive(&entry(), &tmp.0, &net).unwrap();
    fs::write(&archive, b"truncated").unwrap();
    fetch_archive(&entry(), &tmp.0, &net).unwrap();
    assert_eq!(net.calls(), 2);
    assert_eq!(fs::read(&archive).unwrap(), release());
}

#[test]
fn an_archive_checksum_mismatch_is_refused() {
    let tmp = TempDir::new();
    let err = fetch_archive(
        &entry(),
        &tmp.0,
        &FakeNet::serving(RELEASE, b"something else"),
    )
    .unwrap_err();
    assert!(err.contains("manifest says"), "{err}");
    assert_eq!(files_in(&tmp.0.join("downloads")), BTreeSet::new());
}

#[test]
fn a_failed_download_leaves_nothing() {
    let tmp = TempDir::new();
    let err = fetch_archive(&entry(), &tmp.0, &FakeNet::default()).unwrap_err();
    assert!(err.contains("failed: 404 Not Found"), "{err}");
    assert_eq!(files_in(&tmp.0.join("downloads")), BTreeSet::new());
}

#[test]
fn member_problems_are_reported() {
    let tmp = TempDir::new();
    let archive = fetch_archive(&entry(), &tmp.0, &net()).unwrap();
    let mut e = entry();
    e.members
        .insert("lite/id1/pak0.pak".to_owned(), "0".repeat(64));
    let err = extract(&e, &archive, &tmp.0).unwrap_err();
    assert!(err.contains("lite/id1/pak0.pak: sha256"), "{err}");

    let mut e = entry();
    e.members
        .insert("lite/id1/pak2.pak".to_owned(), "0".repeat(64));
    let err = extract(&e, &archive, &tmp.0).unwrap_err();
    assert!(err.contains("no member lite/id1/pak2.pak"), "{err}");

    let mut e = entry();
    e.members = [("lite/../../evil".to_owned(), hex_digest(b"x"))].into();
    let err = extract(&e, &archive, &tmp.0).unwrap_err();
    assert!(err.contains("not a safe path"), "{err}");
    e.members = [("other/x".to_owned(), hex_digest(b"x"))].into();
    assert!(extract(&e, &archive, &tmp.0).is_err());

    let fresh = TempDir::new();
    let bad = fresh.0.join("bad.zip");
    fs::write(&bad, b"not a zip").unwrap();
    let err = extract(&entry(), &bad, &fresh.0).unwrap_err();
    assert!(err.contains("bad.zip"), "{err}");
}

fn asset_manifest(e: Entry) -> Manifest {
    let mut m = manifest(&hex_digest(DEFS));
    m.asset = vec![
        e,
        Entry {
            id: "id1-paks".to_owned(),
            use_: "never".to_owned(),
            ..Entry::default()
        },
    ];
    m
}

#[test]
fn fetch_installs_stand_ins_and_skips_never_assets() {
    let tmp = TempDir::new();
    assert_eq!(
        cmd_fetch(&asset_manifest(entry()), &tmp.0, &net()),
        Vec::<String>::new()
    );
    let game = tmp.0.join("lq/id1");
    assert_eq!(
        fs::read(game.join("sound/boss2/death.wav")).unwrap(),
        b"RIFF-boss1"
    );
    assert_eq!(
        fs::read(game.join("sound/player/slimbrn2.wav")).unwrap(),
        b"RIFF-slime"
    );
    assert!(!tmp.0.join("id1-paks").exists());
}

#[test]
fn fetch_reports_missing_scenario_files() {
    let tmp = TempDir::new();
    let pak = make_pak(&[("maps/start.bsp", b"bsp")]);
    let archive = zip(&[("a/id1/pak0.pak", &pak)], false);
    let mut e = entry();
    e.url = RELEASE.to_owned();
    e.strip = "a/".to_owned();
    e.sha256 = hex_digest(&archive);
    e.members = [("a/id1/pak0.pak".to_owned(), hex_digest(&pak))].into();
    e.stand_ins = BTreeMap::new();
    let errs = cmd_fetch(
        &asset_manifest(e),
        &tmp.0,
        &FakeNet::serving(RELEASE, &archive),
    );
    let out = errs.join("\n");
    assert!(out.contains("asset lq: missing gfx/palette.lmp"), "{out}");
    assert!(out.contains("asset lq: no model (progs/*.mdl)"), "{out}");
    assert!(out.contains("asset lq: no sound (sound/*.wav)"), "{out}");
}

#[test]
fn a_stand_in_must_be_in_the_paks() {
    let tmp = TempDir::new();
    let mut e = entry();
    e.stand_ins = [("sound/x.wav".to_owned(), "sound/nosuch.wav".to_owned())].into();
    let errs = cmd_fetch(&asset_manifest(e), &tmp.0, &net());
    assert_eq!(
        errs,
        ["asset lq: stand-in sound/nosuch.wav for sound/x.wav is not in the paks"]
    );
}

#[test]
fn pak_files_index_every_pak_in_order() {
    let tmp = TempDir::new();
    fs::write(
        tmp.0.join("pak0.pak"),
        make_pak(&[("a", b"old"), ("b", b"b")]),
    )
    .unwrap();
    fs::write(tmp.0.join("pak1.pak"), make_pak(&[("a", b"new")])).unwrap();
    fs::write(
        tmp.0.join("pak3.pak"),
        make_pak(&[("z", b"skipped: pak2 is missing")]),
    )
    .unwrap();
    let files = pak_files(&tmp.0).unwrap();
    assert_eq!(files.keys().collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!(files["a"].pak, tmp.0.join("pak1.pak"));
    assert_eq!(files["b"].pak, tmp.0.join("pak0.pak"));
}

#[test]
fn malformed_paks_are_reported() {
    let cases: [(&[u8], &str); 4] = [
        (b"PAC", "pak0.pak"),
        (b"WAD2\0\0\0\0\0\0\0\0", "not a packfile"),
        (b"PACK\x0c\0\0\0\x40\0\0\0", "outside the file"),
        (
            &[
                b"PACK\x0c\0\0\0\x40\0\0\0".as_slice(),
                b"a",
                &[0; 55],
                &[0; 4],
                &999_i32.to_le_bytes(),
                &[0; 56],
            ]
            .concat(),
            "entry 0",
        ),
    ];
    for (bytes, message) in cases {
        let tmp = TempDir::new();
        fs::write(tmp.0.join("pak0.pak"), bytes).unwrap();
        let err = pak_files(&tmp.0).err().unwrap();
        assert!(err.contains(message), "{message}: {err}");
    }
}

#[test]
fn the_scenario_check_names_what_is_missing() {
    let names: BTreeSet<&str> = SCENARIO.iter().map(|(n, _)| *n).collect();
    assert_eq!(check_scenario(&names), Vec::<String>::new());
    let mut names = names;
    names.remove("gfx.wad");
    names.remove("maps/start.bsp");
    assert_eq!(
        check_scenario(&names),
        [
            "missing gfx.wad",
            "missing maps/start.bsp",
            "no map (maps/*.bsp)"
        ]
    );
}

const QC: &str = r#"
// precache_model ("progs/commented.mdl");
/* precache_sound ("misc/commented.wav");
   still a comment */
void() worldspawn =
{
	precache_model ("progs/player.mdl");
	PRECACHE_SOUND("boss1/death.wav");  // a trailing comment
	precache_sound2 ("boss2/death.wav" );
	precache_model2 ("progs/missing.mdl");
	precache_model (self.mdl);
	bprint ("precache_sound (\"fake.wav\") in a string");
	precache_sounds ();
	my_precache_model ("progs/not_a_call.mdl");
	precache_model (strcat("progs/", "x.mdl"));
};
"#;

fn calls() -> Vec<Precache> {
    precache_calls("w.qc", QC)
}

#[test]
fn precache_calls_are_found_outside_strings_and_comments() {
    let calls = calls();
    let got: Vec<(Kind, Option<&str>, usize)> = calls
        .iter()
        .map(|c| (c.kind, c.name.as_deref(), c.line))
        .collect();
    assert_eq!(
        got,
        [
            (Kind::Model, Some("progs/player.mdl"), 7),
            (Kind::Sound, Some("boss1/death.wav"), 8),
            (Kind::Sound, Some("boss2/death.wav"), 9),
            (Kind::Model, Some("progs/missing.mdl"), 10),
            (Kind::Model, None, 11),
        ]
    );
}

#[test]
fn unterminated_strings_and_comments_end_the_scan_safely() {
    assert_eq!(
        precache_calls("a.qc", "\"open\nprecache_model (\"a.mdl\");").len(),
        1
    );
    assert_eq!(precache_calls("a.qc", "\"esc\\").len(), 0);
    assert_eq!(
        precache_calls("a.qc", "/* open precache_model (\"a.mdl\");").len(),
        0
    );
    assert_eq!(precache_calls("a.qc", "precache_model (\"a.mdl\"").len(), 0);
    assert_eq!(precache_calls("a.qc", "precache_mod").len(), 0);
    assert_eq!(precache_calls("a.qc", "").len(), 0);
}

#[test]
fn precached_paths_put_sounds_under_sound() {
    let paths: Vec<Option<String>> = calls().iter().map(Precache::path).collect();
    assert_eq!(
        paths,
        [
            Some("progs/player.mdl".to_owned()),
            Some("sound/boss1/death.wav".to_owned()),
            Some("sound/boss2/death.wav".to_owned()),
            Some("progs/missing.mdl".to_owned()),
            None,
        ]
    );
}

#[test]
fn coverage_splits_present_stand_in_missing_and_dynamic() {
    let names: BTreeSet<&str> = ["progs/player.mdl", "sound/boss1/death.wav"].into();
    let stand_ins: BTreeMap<String, String> = [
        (
            "sound/boss2/death.wav".to_owned(),
            "sound/boss1/death.wav".to_owned(),
        ),
        (
            "progs/missing.mdl".to_owned(),
            "progs/nosuch.mdl".to_owned(),
        ),
    ]
    .into();
    let calls = calls();
    let (missing, standing_in, dynamic) = coverage(&calls, &names, &stand_ins);
    assert_eq!(
        missing.into_iter().collect::<Vec<_>>(),
        ["progs/missing.mdl"]
    );
    assert_eq!(
        standing_in.into_iter().collect::<Vec<_>>(),
        ["sound/boss2/death.wav"]
    );
    assert_eq!(
        dynamic.iter().map(|c| c.arg.as_str()).collect::<Vec<_>>(),
        ["self.mdl"]
    );
}

fn precache_archive() -> Vec<u8> {
    repo_archive(&[
        ("COPYING", b"GPL"),
        ("qc/defs.qc", DEFS),
        ("qc/world.qc", QC.as_bytes()),
        ("qc/progs.src", b"defs.qc"),
        ("qcother/x.qc", b"precache_model (\"progs/elsewhere.mdl\");"),
    ])
}

#[test]
fn precache_reports_files_the_assets_lack() {
    let tmp = TempDir::new();
    let m = asset_manifest(entry());
    assert_eq!(cmd_fetch(&m, &tmp.0, &net()), Vec::<String>::new());
    let source = FakeNet::serving(&archive_url(), &precache_archive());
    let errs = cmd_precache(&m, &tmp.0, &source);
    assert_eq!(
        errs,
        ["asset lq: rr precaches progs/missing.mdl, which is missing"]
    );
    // The codebase archive is cached, so a second run is offline.
    assert_eq!(cmd_precache(&m, &tmp.0, &FakeNet::default()), errs);
}

#[test]
fn precache_before_fetch_or_without_the_codebase_fails() {
    let tmp = TempDir::new();
    let m = asset_manifest(entry());
    let errs = cmd_precache(&m, &tmp.0, &FakeNet::default());
    assert_eq!(errs.len(), 1);
    assert!(
        errs[0].contains("run cargo xtask assets fetch first"),
        "{}",
        errs[0]
    );
    assert_eq!(cmd_fetch(&m, &tmp.0, &net()), Vec::<String>::new());
    let errs = cmd_precache(&m, &tmp.0, &FakeNet::default());
    assert!(errs[0].contains("404"), "{}", errs[0]);
    fs::write(tmp.0.join("lq/id1/pak0.pak"), b"PAC").unwrap();
    let errs = cmd_precache(&m, &tmp.0, &FakeNet::default());
    assert!(errs[0].contains("pak0.pak"), "{}", errs[0]);
}

#[test]
fn a_string_ends_at_its_closing_quote_or_the_line() {
    assert_eq!(super::string_len(b"\"ab\" x\n"), 4);
    assert_eq!(super::string_len(b"\"a\\\"b\" x"), 6);
    assert_eq!(super::string_len(b"\"ab\nc\""), 3);
    assert_eq!(super::string_len(b"\"ab"), 3);
}
