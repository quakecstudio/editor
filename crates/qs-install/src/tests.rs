// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use qs_pak::PakError;
use qs_paths::Platform;
use qs_vfs::StdFs;

use crate::fixtures::{MemFs, MemRegistry, env, find, known, pak};
use crate::{
    Edition, FS_KNOWN_FILES, FileStatus, Finder, GOG_QUAKE_ENHANCED_ID, GOG_QUAKE_ID, Install,
    InstallFs, LinkError, LinkOutcome, Missing, MissingReason, Release, Source, examine_folder,
    find_installs, identify, link_dir, size_and_md5,
};

const STEAM_ORIGINAL: [&str; 4] = [
    "Id1/PAK0.PAK",
    "Id1/PAK1.PAK",
    "hipnotic/pak0.pak",
    "rogue/pak0.pak",
];

const RERELEASE: [&str; 5] = [
    "id1/pak0.pak",
    "hipnotic/pak0.pak",
    "rogue/pak0.pak",
    "dopa/pak0.pak",
    "mg1/pak0.pak",
];

fn p(path: &str) -> PathBuf {
    PathBuf::from(path)
}

fn names(install: &Install, rerelease: bool) -> Vec<String> {
    let base = if rerelease {
        install.rerelease.as_ref()
    } else {
        install.original.as_ref()
    };
    base.unwrap()
        .game_dirs
        .iter()
        .map(|dir| dir.name.clone())
        .collect()
}

/// A Steam install of the original game with the rerelease in its `rerelease` folder.
fn steam_quake(fs: MemFs, folder: &Path) -> MemFs {
    fs.paks(folder, &STEAM_ORIGINAL)
        .paks(folder.join("rerelease"), &RERELEASE)
        .file(folder.join("rerelease").join("QuakeEX.kpf"), b"PK")
}

fn manifest(installdir: &str) -> Vec<u8> {
    format!("\"AppState\"\n{{\n\t\"appid\"\t\t\"2310\"\n\t\"installdir\"\t\t\"{installdir}\"\n}}\n")
        .into_bytes()
}

// ---------------------------------------------------------------------------------------
// Finding installs

// spec FS-6.1: an original base with a rerelease folder; id1 and paks matched in any case.
// spec FS-5.1: the Steam rerelease is the `rerelease` folder, its id1 holding only pak0.pak.
#[test]
fn linux_steam_library_from_libraryfolders_vdf() {
    let steam = p("/home/q/.local/share/Steam");
    let library = p("/mnt/games/SteamLibrary");
    let quake = library.join("steamapps/common/Quake");
    let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"/home/q/.local/share/Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"/mnt/games/SteamLibrary\"\n\t\t\"apps\" { \"2310\" \"1\" }\n\t}\n}\n";
    let fs = steam_quake(MemFs::default(), &quake)
        .file(steam.join("config/libraryfolders.vdf"), vdf.as_bytes())
        .dir(steam.join("steamapps/common"))
        .file(
            library.join("steamapps/appmanifest_2310.acf"),
            &manifest("Quake"),
        );
    let installs = find(&fs, &MemRegistry::default(), Platform::Linux);
    assert_eq!(installs.len(), 1, "{installs:#?}");
    let install = &installs[0];
    assert_eq!(install.source, Source::Steam);
    assert_eq!(install.root, quake);
    let original = install.original.as_ref().unwrap();
    assert_eq!(original.path, quake);
    assert_eq!(original.id1, quake.join("Id1"));
    assert_eq!(original.pak0, quake.join("Id1/PAK0.PAK"));
    assert_eq!(original.pak1, Some(quake.join("Id1/PAK1.PAK")));
    assert_eq!(original.kpf, None);
    assert_eq!(names(install, false), ["hipnotic", "rogue"]);
    let rerelease = install.rerelease.as_ref().unwrap();
    assert_eq!(rerelease.path, quake.join("rerelease"));
    assert_eq!(rerelease.id1, quake.join("rerelease/id1"));
    assert_eq!(rerelease.pak1, None);
    assert_eq!(rerelease.kpf, Some(quake.join("rerelease/QuakeEX.kpf")));
    assert_eq!(names(install, true), ["dopa", "hipnotic", "mg1", "rogue"]);
    assert_eq!(
        rerelease.game_dirs[1].pak0,
        quake.join("rerelease/hipnotic/pak0.pak")
    );
}

#[test]
fn linux_steam_folder_reached_through_a_link_is_reported_once() {
    let quake = p("/home/q/.local/share/Steam/steamapps/common/Quake");
    let fs = steam_quake(MemFs::default(), &quake)
        .paks(
            "/home/q/.steam/steam/steamapps/common/Quake",
            &STEAM_ORIGINAL,
        )
        .alias("/home/q/.steam/steam", "/home/q/.local/share/Steam");
    let installs = find(&fs, &MemRegistry::default(), Platform::Linux);
    assert_eq!(installs.len(), 1, "{installs:#?}");
    assert_eq!(
        installs[0].root,
        p("/home/q/.steam/steam/steamapps/common/Quake")
    );
}

#[test]
fn old_libraryfolders_format_custom_installdir_and_missing_manifest() {
    let steam = p("/home/q/.steam/steam");
    let vdf = "\"LibraryFolders\"\n{\n\t\"TimeNextStatsReport\"\t\t\"1\"\n\t\"ContentStatsID\"\t\t\"2\"\n\t\"1\"\t\t\"/data/steam\"\n\t\"2\"\t\t\"\"\n}\n";
    let fs = MemFs::default()
        .file(steam.join("steamapps/libraryfolders.vdf"), vdf.as_bytes())
        .paks(steam.join("steamapps/common/Quake"), &["id1/pak0.pak"])
        .file(
            "/data/steam/steamapps/appmanifest_2310.acf",
            &manifest("Quake Custom"),
        )
        .paks(
            "/data/steam/steamapps/common/Quake Custom",
            &["id1/pak0.pak"],
        );
    let roots: Vec<PathBuf> = find(&fs, &MemRegistry::default(), Platform::Linux)
        .into_iter()
        .map(|install| install.root)
        .collect();
    assert_eq!(
        roots,
        [
            steam.join("steamapps/common/Quake"),
            p("/data/steam/steamapps/common/Quake Custom")
        ]
    );
}

#[test]
fn malformed_steam_files_are_ignored() {
    let steam = p("/home/q/.local/share/Steam");
    let fs = MemFs::default()
        .file(
            steam.join("config/libraryfolders.vdf"),
            b"\"libraryfolders\" {",
        )
        .file(
            steam.join("steamapps/appmanifest_2310.acf"),
            b"\"AppState\" { \"installdir\"",
        )
        .paks(steam.join("steamapps/common/Quake"), &["id1/pak0.pak"]);
    let installs = find(&fs, &MemRegistry::default(), Platform::Linux);
    assert_eq!(installs.len(), 1);
    assert_eq!(installs[0].root, steam.join("steamapps/common/Quake"));
}

#[test]
fn a_steam_library_without_quake_is_skipped() {
    let fs = MemFs::default()
        .dir("/home/q/.local/share/Steam/steamapps/common/Other")
        .dir("/home/q/.steam/steam/steamapps");
    assert_eq!(find(&fs, &MemRegistry::default(), Platform::Linux), []);
}

#[test]
fn linux_flatpak_steam() {
    let quake =
        p("/home/q/.var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/common/Quake");
    let fs = steam_quake(MemFs::default(), &quake);
    let installs = find(&fs, &MemRegistry::default(), Platform::Linux);
    assert_eq!(installs.len(), 1);
    assert_eq!(installs[0].root, quake);
    assert!(installs[0].rerelease.is_some());
}

// spec FS-6.1: a base holding QuakeEX.kpf is a rerelease of its own (GOG's Quake Enhanced).
#[test]
fn linux_heroic_gog_installs() {
    let json = format!(
        r#"{{"installed": [
            {{"appName": "1207659013", "install_path": "/home/q/Games/Heroic/Other", "platform": "windows"}},
            {{"appName": "{GOG_QUAKE_ID}", "install_path": "/home/q/Games/Heroic/Quake", "platform": "windows"}},
            {{"appName": "{GOG_QUAKE_ENHANCED_ID}", "install_path": "/home/q/Games/Heroic/Quake Enhanced", "platform": "windows"}},
            {{"appName": "{GOG_QUAKE_ID}"}},
            {{"appName": "{GOG_QUAKE_ID}", "install_path": ""}}
        ]}}"#
    );
    let flatpak = "/home/q/.var/app/com.heroicgameslauncher.hgl/config/heroic";
    let fs = MemFs::default()
        .file(
            "/home/q/.config/heroic/gog_store/installed.json",
            json.as_bytes(),
        )
        .file(format!("{flatpak}/gog_store/installed.json"), b"not json")
        .paks("/home/q/Games/Heroic/Other", &["id1/pak0.pak"])
        .paks(
            "/home/q/Games/Heroic/Quake",
            &["id1/pak0.pak", "id1/pak1.pak"],
        )
        .paks("/home/q/Games/Heroic/Quake Enhanced", &RERELEASE)
        .file("/home/q/Games/Heroic/Quake Enhanced/QuakeEX.kpf", b"PK");
    let installs = find(&fs, &MemRegistry::default(), Platform::Linux);
    assert_eq!(installs.len(), 2, "{installs:#?}");
    assert_eq!(installs[0].source, Source::Heroic);
    assert_eq!(installs[0].root, p("/home/q/Games/Heroic/Quake"));
    assert!(installs[0].original.is_some() && installs[0].rerelease.is_none());
    let enhanced = &installs[1];
    assert_eq!(enhanced.root, p("/home/q/Games/Heroic/Quake Enhanced"));
    assert!(enhanced.original.is_none());
    let rerelease = enhanced.rerelease.as_ref().unwrap();
    assert_eq!(rerelease.path, p("/home/q/Games/Heroic/Quake Enhanced"));
    assert_eq!(names(enhanced, true), ["dopa", "hipnotic", "mg1", "rogue"]);
}

#[test]
fn heroic_files_without_an_installed_list_are_ignored() {
    let fs = MemFs::default()
        .file(
            "/home/q/.config/heroic/gog_store/installed.json",
            br#"{"installed": 3}"#,
        )
        .file(
            "/home/q/.var/app/com.heroicgameslauncher.hgl/config/heroic/gog_store/installed.json",
            b"[]",
        );
    assert_eq!(find(&fs, &MemRegistry::default(), Platform::Linux), []);
}

#[test]
fn windows_steam_from_the_registry() {
    let steam = "c:/program files (x86)/steam";
    let vdf = "\"libraryfolders\"\n{\n\t\"0\" { \"path\" \"c:\\\\program files (x86)\\\\steam\" }\n\t\"1\" { \"path\" \"D:/SteamLibrary\" }\n}\n";
    let registry = MemRegistry::default().with(
        r"HKEY_CURRENT_USER\Software\Valve\Steam",
        "SteamPath",
        steam,
    );
    let fs = MemFs::default()
        .file(format!("{steam}/config/libraryfolders.vdf"), vdf.as_bytes())
        .dir(format!("{steam}/steamapps/common"));
    let fs = steam_quake(fs, &p("D:/SteamLibrary/steamapps/common/Quake"));
    let installs = find(&fs, &registry, Platform::Windows);
    assert_eq!(installs.len(), 1, "{installs:#?}");
    assert_eq!(installs[0].source, Source::Steam);
    assert_eq!(
        installs[0].root,
        p("D:/SteamLibrary/steamapps/common/Quake")
    );
    assert!(installs[0].rerelease.is_some());
}

#[test]
fn windows_gog_from_the_registry() {
    let key = r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\GOG.com\Games";
    let registry = MemRegistry::default()
        .with(
            &format!(r"{key}\{GOG_QUAKE_ID}"),
            "path",
            "D:/GOG Games/Quake/",
        )
        .with(
            &format!(r"{key}\{GOG_QUAKE_ENHANCED_ID}"),
            "path",
            "D:/GOG Games/Quake Enhanced",
        );
    let fs = MemFs::default()
        .paks("D:/GOG Games/Quake", &STEAM_ORIGINAL)
        .paks("D:/GOG Games/Quake Enhanced", &RERELEASE)
        .file("D:/GOG Games/Quake Enhanced/QuakeEX.kpf", b"PK");
    let installs = find(&fs, &registry, Platform::Windows);
    let found: Vec<(Source, PathBuf, bool)> = installs
        .iter()
        .map(|install| {
            (
                install.source,
                install.root.clone(),
                install.rerelease.is_some(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            (Source::Gog, p("D:/GOG Games/Quake"), false),
            (Source::Gog, p("D:/GOG Games/Quake Enhanced"), true),
        ]
    );
}

#[test]
fn windows_gog_galaxy_and_offline_installer_libraries() {
    let stores = env(Platform::Windows);
    let galaxy = stores.gog_libraries()[0].clone();
    let offline = stores.gog_libraries()[1].clone();
    let fs = MemFs::default()
        .paks(galaxy.join("Quake"), &["id1/pak0.pak", "id1/pak1.pak"])
        .file(
            galaxy.join(format!("Quake/goggame-{GOG_QUAKE_ID}.info")),
            b"{}",
        )
        .paks(galaxy.join("Not Quake"), &["id1/pak0.pak"])
        .file(galaxy.join("stray.txt"), b"")
        .paks(offline.join("Quake Enhanced"), &RERELEASE)
        .file(offline.join("Quake Enhanced/QuakeEX.kpf"), b"PK")
        .file(
            offline.join(format!(
                "Quake Enhanced/GOGGAME-{GOG_QUAKE_ENHANCED_ID}.INFO"
            )),
            b"{}",
        );
    let installs = find(&fs, &MemRegistry::default(), Platform::Windows);
    let roots: Vec<PathBuf> = installs
        .iter()
        .map(|install| install.root.clone())
        .collect();
    assert_eq!(
        roots,
        [galaxy.join("Quake"), offline.join("Quake Enhanced")]
    );
    assert!(installs.iter().all(|install| install.source == Source::Gog));
    assert!(installs[1].original.is_none() && installs[1].rerelease.is_some());
}

#[test]
fn windows_heroic() {
    let json = format!(
        r#"{{"installed": [{{"appName": "{GOG_QUAKE_ID}", "install_path": "C:/Users/q/Games/Heroic/Quake"}}]}}"#
    );
    let fs = MemFs::default()
        .file(
            "C:/Users/q/AppData/Roaming/heroic/gog_store/installed.json",
            json.as_bytes(),
        )
        .paks("C:/Users/q/Games/Heroic/Quake", &["id1/pak0.pak"]);
    let installs = find(&fs, &MemRegistry::default(), Platform::Windows);
    assert_eq!(installs.len(), 1);
    assert_eq!(installs[0].source, Source::Heroic);
}

#[test]
fn the_registry_is_read_only_on_windows() {
    let registry = MemRegistry::default()
        .with(
            r"HKEY_CURRENT_USER\Software\Valve\Steam",
            "SteamPath",
            "/steam",
        )
        .with(
            &format!(r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\GOG.com\Games\{GOG_QUAKE_ID}"),
            "path",
            "/gog",
        );
    let fs = MemFs::default()
        .paks("/steam/steamapps/common/Quake", &["id1/pak0.pak"])
        .paks("/gog", &["id1/pak0.pak"]);
    for platform in [Platform::Linux, Platform::MacOs] {
        assert_eq!(find(&fs, &registry, platform), [], "{platform:?}");
    }
    assert_eq!(find(&fs, &registry, Platform::Windows).len(), 2);
}

#[test]
fn windows_registry_paths_that_are_only_separators_are_ignored() {
    let registry = MemRegistry::default().with(
        &format!(r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\GOG.com\Games\{GOG_QUAKE_ID}"),
        "path",
        "\\",
    );
    let fs = MemFs::default().paks("/", &["id1/pak0.pak"]);
    assert_eq!(find(&fs, &registry, Platform::Windows), []);
}

#[test]
fn macos_steam_client_or_steamcmd_default_library() {
    let quake = p("/Users/q/Library/Application Support/Steam/steamapps/common/Quake");
    let fs = steam_quake(MemFs::default(), &quake);
    let installs = find(&fs, &MemRegistry::default(), Platform::MacOs);
    assert_eq!(installs.len(), 1);
    assert_eq!(installs[0].source, Source::Steam);
    assert_eq!(installs[0].root, quake);
}

#[test]
fn nothing_installed_finds_nothing() {
    for platform in [Platform::MacOs, Platform::Linux, Platform::Windows] {
        assert_eq!(
            find(&MemFs::default(), &MemRegistry::default(), platform),
            [],
            "{platform:?}"
        );
    }
}

fn examine(fs: &MemFs, folder: &str) -> Option<Install> {
    Finder::new(
        fs,
        &MemRegistry::default(),
        Platform::MacOs,
        env(Platform::MacOs),
    )
    .examine(Path::new(folder))
}

// spec FS-6.1: a folder downloaded by steamcmd with `force_install_dir` is a base directory.
#[test]
fn macos_steamcmd_download() {
    let fs = steam_quake(MemFs::default(), &p("/Users/q/Downloads/quake")).file(
        "/Users/q/Downloads/quake/steamapps/appmanifest_2310.acf",
        &manifest("Quake"),
    );
    let install = examine(&fs, "/Users/q/Downloads/quake").unwrap();
    assert_eq!(install.source, Source::Steamcmd);
    assert_eq!(install.root, p("/Users/q/Downloads/quake"));
    assert!(install.original.is_some() && install.rerelease.is_some());
}

// spec FS-6.1: innoextract writes the GOG installer's `{app}` files under `app/`.
#[test]
fn macos_innoextract_output() {
    let fs = MemFs::default()
        .paks(
            "/Users/q/Downloads/gog/app",
            &[
                "ID1/PAK0.PAK",
                "ID1/PAK1.PAK",
                "hipnotic/pak0.pak",
                "rogue/pak0.pak",
            ],
        )
        .file(
            format!("/Users/q/Downloads/gog/app/goggame-{GOG_QUAKE_ID}.info"),
            b"{}",
        )
        .dir("/Users/q/Downloads/gog/tmp");
    let install = examine(&fs, "/Users/q/Downloads/gog").unwrap();
    assert_eq!(install.source, Source::Innoextract);
    assert_eq!(install.root, p("/Users/q/Downloads/gog"));
    let original = install.original.as_ref().unwrap();
    assert_eq!(original.path, p("/Users/q/Downloads/gog/app"));
    assert_eq!(original.id1, p("/Users/q/Downloads/gog/app/ID1"));
    assert_eq!(names(&install, false), ["hipnotic", "rogue"]);
}

// spec FS-6.1: any folder whose id1 holds pak0.pak is accepted as the manual override.
#[test]
fn any_folder_with_id1_pak0_is_accepted() {
    let fs = MemFs::default()
        .paks("/Volumes/Games/Quake", &["id1/pak0.pak"])
        .dir("/Volumes/Games/Quake/id1/maps")
        .dir("/Volumes/Games/Quake/emptymod")
        .file("/Volumes/Games/Quake/notes/pak0.pak/x", b"")
        .dir("/Volumes/Games/Quake/rerelease/id1");
    let install = examine(&fs, "/Volumes/Games/Quake").unwrap();
    assert_eq!(install.source, Source::Folder);
    let original = install.original.as_ref().unwrap();
    assert_eq!(original.pak1, None);
    assert!(original.game_dirs.is_empty());
    assert!(install.rerelease.is_none());
}

// spec FS-6.1: without id1/pak0.pak a folder holds no game.
#[test]
fn folders_without_id1_pak0_are_refused() {
    let fs = MemFs::default()
        .dir("/a/id1")
        .file("/b/pak0.pak", &pak("x"))
        .dir("/c/id1/pak0.pak")
        .dir("/d/app")
        .file("/e/steamapps/appmanifest_2310.acf", &manifest("Quake"));
    for folder in ["/a", "/b", "/c", "/d", "/e", "/missing"] {
        assert_eq!(examine(&fs, folder), None, "{folder}");
    }
}

#[test]
fn examining_a_real_folder_without_game_data() {
    let dir = TempDir::new("examine");
    assert_eq!(examine_folder(dir.path()), None);
    fs::create_dir_all(dir.path().join("id1")).unwrap();
    fs::write(dir.path().join("id1").join("PAK0.PAK"), pak("stand-in")).unwrap();
    let install = examine_folder(dir.path()).unwrap();
    assert_eq!(install.source, Source::Folder);
    assert!(install.original.unwrap().pak0.ends_with("PAK0.PAK"));
}

#[test]
fn the_host_filesystem_resolves_links_for_deduplication() {
    let dir = TempDir::new("canonical");
    let missing = dir.path().join("missing");
    assert_eq!(StdFs.canonical(&missing), missing);
    let target = dir.path().join("Steam");
    fs::create_dir_all(&target).unwrap();
    let link = dir.path().join("steam-link");
    link_dir(&target, &link).unwrap();
    assert_eq!(StdFs.canonical(&link), StdFs.canonical(&target));
    assert_eq!(StdFs.canonical(&target), fs::canonicalize(&target).unwrap());
}

#[test]
fn finding_on_this_machine_does_not_fail() {
    let _installs = find_installs();
}

// ---------------------------------------------------------------------------------------
// Checking an install

fn checked(fs: &MemFs, install: &Install, table: &[crate::KnownFile]) -> crate::Validation {
    install.validate_with(fs, table)
}

fn examined(fs: MemFs, folder: &str) -> (MemFs, Install) {
    let install = Finder::new(
        &fs,
        &MemRegistry::default(),
        Platform::Linux,
        env(Platform::Linux),
    )
    .examine(Path::new(folder))
    .unwrap();
    (fs, install)
}

/// A stand-in install at `/q` holding `files`.
fn original(files: &[&str]) -> (MemFs, Install) {
    examined(MemFs::default().paks("/q", files), "/q")
}

// spec FS-6.2: a file whose size and MD5 equal a row is that known file.
// spec FS-6.4: the known pak1.pak makes the registered edition.
#[test]
fn registered_original_game() {
    let (fs, install) = original(&STEAM_ORIGINAL);
    let table = [
        known("id1", "pak0.pak", Release::Original, &pak("Id1/PAK0.PAK")),
        known("id1", "pak1.pak", Release::Original, &pak("Id1/PAK1.PAK")),
        known(
            "hipnotic",
            "pak0.pak",
            Release::Original,
            &pak("hipnotic/pak0.pak"),
        ),
    ];
    let validation = checked(&fs, &install, &table);
    assert_eq!(validation.editions, [Edition::Registered]);
    assert_eq!(validation.missing, []);
    let statuses: Vec<FileStatus> = validation.files.iter().map(|file| file.status).collect();
    assert_eq!(
        statuses,
        [
            FileStatus::Known(table[0]),
            FileStatus::Known(table[1]),
            FileStatus::Known(table[2]),
            FileStatus::Unrecognised {
                entries: 1,
                modified: true
            },
        ]
    );
    assert_eq!(validation.files[3].path, p("/q/rogue/pak0.pak"));
}

// spec FS-6.4: a known original pak0.pak and no pak1.pak is the shareware game.
// spec FS-6.5: the shareware game reports id1/pak1.pak missing.
#[test]
fn shareware_game() {
    let (fs, install) = original(&["id1/pak0.pak"]);
    let table = [known(
        "id1",
        "pak0.pak",
        Release::Original,
        &pak("id1/pak0.pak"),
    )];
    let validation = checked(&fs, &install, &table);
    assert_eq!(validation.editions, [Edition::Shareware]);
    assert_eq!(
        validation.missing,
        [Missing {
            file: "id1/pak1.pak",
            reason: MissingReason::SharewareOnly
        }]
    );
}

// spec FS-6.3: an unpublished pak0.pak is reported unrecognised, not as an error.
// spec FS-6.4: shareware needs the known pak0.pak; registered needs the known pak1.pak.
#[test]
fn unrecognised_original_files_make_no_edition() {
    let (fs, install) = original(&["id1/pak0.pak", "id1/pak1.pak"]);
    let validation = install.validate(&fs);
    assert_eq!(validation.editions, []);
    assert_eq!(validation.missing, []);
    assert!(validation.files.iter().all(|file| matches!(
        file.status,
        FileStatus::Unrecognised {
            entries: 1,
            modified: true
        }
    )));
    let (fs, install) = original(&["id1/pak0.pak"]);
    let validation = install.validate(&fs);
    assert_eq!(validation.editions, []);
    assert_eq!(validation.missing.len(), 1);
}

// spec FS-6.2: the size alone does not identify a file; the checksum must match too.
#[test]
fn same_size_different_checksum_is_unrecognised() {
    let (fs, install) = original(&["id1/pak0.pak"]);
    let mut row = known("id1", "pak0.pak", Release::Original, &pak("id1/pak0.pak"));
    row.md5 = "00000000000000000000000000000000";
    let validation = checked(&fs, &install, &[row]);
    assert_eq!(
        validation.files[0].status,
        FileStatus::Unrecognised {
            entries: 1,
            modified: true
        }
    );
    assert_eq!(validation.editions, []);
}

// spec FS-6.4: a usable rerelease id1/pak0.pak makes the rerelease edition.
// spec FS-6.5: with only the rerelease, the original id1/pak0.pak and pak1.pak are missing.
#[test]
fn rerelease_only_install_reports_the_original_game_missing() {
    let (fs, install) = examined(
        MemFs::default()
            .paks("/epic/Quake", &RERELEASE)
            .file("/epic/Quake/QuakeEX.kpf", b"PK"),
        "/epic/Quake",
    );
    let table = [known(
        "id1",
        "pak0.pak",
        Release::Rerelease,
        &pak("id1/pak0.pak"),
    )];
    let validation = checked(&fs, &install, &table);
    assert_eq!(validation.editions, [Edition::Rerelease]);
    assert_eq!(
        validation.missing,
        [
            Missing {
                file: "id1/pak0.pak",
                reason: MissingReason::RereleaseOnly
            },
            Missing {
                file: "id1/pak1.pak",
                reason: MissingReason::RereleaseOnly
            },
        ]
    );
    assert_eq!(validation.files.len(), 5);
    assert_eq!(validation.files[0].status, FileStatus::Known(table[0]));
}

/// GOG's Quake Enhanced as Justin's install lays it out (FS-6.6): the rerelease at the top,
/// and the original DOS game in `dos`, its names in the case GOG wrote them.
fn gog_quake_enhanced(fs: MemFs, root: &str) -> MemFs {
    fs.paks(root, &RERELEASE)
        .paks(root, &["mg3/pak0.pak", "ctf/pak0.pak"])
        .file(format!("{root}/QuakeEX.kpf"), b"PK")
        .file(
            format!("{root}/goggame-{GOG_QUAKE_ENHANCED_ID}.info"),
            b"{}",
        )
        .dir(format!("{root}/app"))
        .paks(
            format!("{root}/dos"),
            &[
                "id1/PAK0.PAK",
                "id1/PAK1.PAK",
                "hipnotic/pak0.pak",
                "rogue/pak0.pak",
            ],
        )
        .file(format!("{root}/dos/quake.exe"), b"MZ")
}

// spec FS-6.6: GOG's Quake Enhanced has the original DOS game in its `dos` folder, so both
// bases are reported, and the registered and rerelease editions with nothing missing.
#[test]
fn gog_quake_enhanced_holds_the_original_game_in_dos() {
    let (fs, install) = examined(
        gog_quake_enhanced(MemFs::default(), "/q/quake-enhanced-gog"),
        "/q/quake-enhanced-gog",
    );
    assert_eq!(install.source, Source::Folder);
    let original = install.original.as_ref().unwrap();
    assert_eq!(original.path, p("/q/quake-enhanced-gog/dos"));
    assert_eq!(original.id1, p("/q/quake-enhanced-gog/dos/id1"));
    assert_eq!(original.pak0, p("/q/quake-enhanced-gog/dos/id1/PAK0.PAK"));
    assert_eq!(
        original.pak1,
        Some(p("/q/quake-enhanced-gog/dos/id1/PAK1.PAK"))
    );
    assert_eq!(original.kpf, None);
    assert_eq!(names(&install, false), ["hipnotic", "rogue"]);
    let rerelease = install.rerelease.as_ref().unwrap();
    assert_eq!(rerelease.path, p("/q/quake-enhanced-gog"));
    assert_eq!(
        names(&install, true),
        ["ctf", "dopa", "hipnotic", "mg1", "mg3", "rogue"]
    );
    let table = [
        known("id1", "pak0.pak", Release::Original, &pak("id1/PAK0.PAK")),
        known("id1", "pak1.pak", Release::Original, &pak("id1/PAK1.PAK")),
        known("id1", "pak0.pak", Release::Rerelease, &pak("id1/pak0.pak")),
    ];
    let validation = checked(&fs, &install, &table);
    assert_eq!(
        validation.editions,
        [Edition::Registered, Edition::Rerelease]
    );
    assert_eq!(validation.missing, []);
    assert_eq!(validation.files[0].status, FileStatus::Known(table[0]));
    assert_eq!(validation.files[1].status, FileStatus::Known(table[1]));
}

// spec FS-6.6: the GOG layout is found from the store too, and a `dos` folder that is not a
// base directory, or one beside an original game rather than a rerelease, is no original game.
#[test]
fn dos_folder_counts_only_beside_a_rerelease() {
    let root = "C:/GOG Games/Quake Enhanced";
    let fs = gog_quake_enhanced(MemFs::default(), root);
    let registry = MemRegistry::default().with(
        &format!(r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\GOG.com\Games\{GOG_QUAKE_ENHANCED_ID}"),
        "path",
        root,
    );
    let installs = find(&fs, &registry, Platform::Windows);
    assert_eq!(installs.len(), 1, "{installs:#?}");
    assert_eq!(
        installs[0].original.as_ref().unwrap().path,
        p(&format!("{root}/dos"))
    );

    let (_, no_paks) = examined(
        MemFs::default()
            .paks("/q", &RERELEASE)
            .file("/q/QuakeEX.kpf", b"PK")
            .paks("/q/dos", &["hipnotic/pak0.pak"]),
        "/q",
    );
    assert!(no_paks.original.is_none());

    let (_, beside_original) = examined(
        MemFs::default()
            .paks("/q", &STEAM_ORIGINAL)
            .paks("/q/dos", &["id1/pak0.pak"]),
        "/q",
    );
    assert_eq!(beside_original.original.unwrap().path, p("/q"));
    assert!(beside_original.rerelease.is_none());
}

// spec FS-6.3: a file that is not a PAK archive is unusable, with the PAK error.
// spec FS-6.4: an unusable rerelease pak0.pak does not make the rerelease edition.
#[test]
fn damaged_files_are_unusable() {
    let (fs, install) = examined(
        MemFs::default()
            .file("/q/id1/pak0.pak", b"PACK")
            .file("/q/rerelease/id1/pak0.pak", b"not a pak at all")
            .file("/q/rerelease/id1/pak1.pak", &pak("x"))
            .file("/q/rerelease/QuakeEX.kpf", b"PK"),
        "/q",
    );
    let validation = install.validate(&fs);
    assert_eq!(
        validation.files[0].status,
        FileStatus::Unusable(PakError::NotAPackfile)
    );
    assert_eq!(
        validation.files[1].status,
        FileStatus::Unusable(PakError::NotAPackfile)
    );
    assert_eq!(validation.files.len(), 3);
    assert_eq!(validation.editions, []);
    assert_eq!(validation.missing.len(), 1);
}

#[test]
fn size_and_md5_of_a_file() {
    let fs = MemFs::default().file("/q/a", b"abc");
    assert_eq!(
        size_and_md5(&fs, Path::new("/q/a")).unwrap(),
        (3, "900150983cd24fb0d6963f7d28e17f72".to_owned())
    );
    assert_eq!(
        size_and_md5(&fs, Path::new("/q/b")),
        Err(PakError::Io(std::io::ErrorKind::NotFound))
    );
}

// spec FS-6.3: a file that cannot be opened is unusable.
#[test]
fn unreadable_files_are_unusable() {
    let status = identify(&MemFs::default(), Path::new("/nowhere/pak0.pak"));
    assert_eq!(
        status,
        FileStatus::Unusable(PakError::Io(std::io::ErrorKind::NotFound))
    );
}

// spec FS-6.2: the published table; every row a pak, a 32-digit lowercase MD5, no duplicate.
#[test]
fn the_known_files_table_is_well_formed() {
    for (index, row) in FS_KNOWN_FILES.iter().enumerate() {
        assert_eq!(row.md5.len(), 32, "{row:?}");
        assert!(
            row.md5
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "{row:?}"
        );
        assert!(row.name == "pak0.pak" || row.name == "pak1.pak", "{row:?}");
        assert!(
            FS_KNOWN_FILES[index + 1..]
                .iter()
                .all(|other| (other.size, other.md5) != (row.size, row.md5)),
            "{row:?}"
        );
    }
    let registered: Vec<_> = FS_KNOWN_FILES
        .iter()
        .filter(|row| row.is_registered_pak1())
        .collect();
    assert_eq!(registered.len(), 1);
    assert_eq!(registered[0].md5, "d76b3e5678f0b64ac74ce5e340e6a685");
    assert_eq!(
        FS_KNOWN_FILES
            .iter()
            .filter(|row| row.is_original_pak0())
            .count(),
        2
    );
}

// spec FS-6.2: the code's table is the spec's FS_KNOWN_FILES table, row for row.
#[test]
fn the_known_files_table_matches_the_spec() {
    let spec = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/filesystem.md"),
    )
    .unwrap();
    let (_, table) = spec
        .split_once("`FS_KNOWN_FILES`, every file FS-6.2 identifies:")
        .unwrap();
    let rows: Vec<(String, u64, String, String)> = table
        .lines()
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            (
                cells[1].trim_matches('`').to_owned(),
                cells[2].replace(',', "").parse().unwrap(),
                cells[3].trim_matches('`').to_owned(),
                cells[4].to_owned(),
            )
        })
        .collect();
    let expected: Vec<(String, u64, String, String)> = FS_KNOWN_FILES
        .iter()
        .map(|row| {
            let release = match row.release {
                Release::Original => "original",
                Release::Rerelease => "rerelease",
            };
            (
                format!("{}/{}", row.game_dir, row.name),
                row.size,
                row.md5.to_owned(),
                release.to_owned(),
            )
        })
        .collect();
    assert_eq!(rows, expected);
}

#[test]
fn stand_in_files_are_never_identified_by_the_real_table() {
    let fs = MemFs::default().file("/q/id1/pak0.pak", &pak("id1/pak0.pak"));
    assert_eq!(
        identify(&fs, Path::new("/q/id1/pak0.pak")),
        FileStatus::Unrecognised {
            entries: 1,
            modified: true
        }
    );
}

// ---------------------------------------------------------------------------------------
// Directory links (ADR-0008 run roots), on the real filesystem of each OS in CI

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory under the system's temporary folder, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("qs-install-{}-{name}-{n}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

/// A stand-in install: `install/id1/pak0.pak`.
fn stand_in_install(dir: &TempDir) -> PathBuf {
    let id1 = dir.path().join("install").join("id1");
    fs::create_dir_all(&id1).unwrap();
    fs::write(id1.join("pak0.pak"), pak("stand-in")).unwrap();
    id1
}

fn resolves_to(link: &Path, target: &Path) -> bool {
    fs::canonicalize(link).unwrap() == fs::canonicalize(target).unwrap()
}

#[cfg(not(windows))]
#[test]
fn links_are_symbolic_links_on_macos_and_linux() {
    let dir = TempDir::new("symlink");
    let id1 = stand_in_install(&dir);
    let link = dir
        .path()
        .join("build")
        .join("run")
        .join("original")
        .join("id1");
    assert_eq!(link_dir(&id1, &link).unwrap(), LinkOutcome::Created);
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(resolves_to(&link, &id1));
    assert_eq!(fs::read(link.join("pak0.pak")).unwrap(), pak("stand-in"));
    assert_eq!(link_dir(&id1, &link).unwrap(), LinkOutcome::Unchanged);
    assert_eq!(
        fs::read_dir(&id1).unwrap().count(),
        1,
        "nothing written in the install"
    );
}

#[cfg(windows)]
#[test]
fn links_are_junctions_on_windows() {
    let dir = TempDir::new("junction");
    let id1 = stand_in_install(&dir);
    let link = dir
        .path()
        .join("build")
        .join("run")
        .join("original")
        .join("id1");
    assert_eq!(link_dir(&id1, &link).unwrap(), LinkOutcome::Created);
    assert!(junction::exists(&link).unwrap());
    assert!(resolves_to(&link, &id1));
    assert_eq!(fs::read(link.join("pak0.pak")).unwrap(), pak("stand-in"));
    assert_eq!(link_dir(&id1, &link).unwrap(), LinkOutcome::Unchanged);
    assert_eq!(
        fs::read_dir(&id1).unwrap().count(),
        1,
        "nothing written in the install"
    );
}

#[test]
fn a_broken_link_is_repaired() {
    let dir = TempDir::new("broken");
    let id1 = stand_in_install(&dir);
    let old = dir.path().join("old-install");
    fs::create_dir_all(&old).unwrap();
    let link = dir.path().join("run").join("id1");
    assert_eq!(link_dir(&old, &link).unwrap(), LinkOutcome::Created);
    fs::remove_dir(&old).unwrap();
    assert!(fs::canonicalize(&link).is_err(), "the link is broken");
    assert_eq!(link_dir(&id1, &link).unwrap(), LinkOutcome::Repaired);
    assert!(resolves_to(&link, &id1));
}

#[test]
fn a_link_to_another_folder_is_repaired() {
    let dir = TempDir::new("elsewhere");
    let id1 = stand_in_install(&dir);
    let other = dir.path().join("other");
    fs::create_dir_all(&other).unwrap();
    let link = dir.path().join("run").join("id1");
    link_dir(&other, &link).unwrap();
    assert_eq!(link_dir(&id1, &link).unwrap(), LinkOutcome::Repaired);
    assert!(resolves_to(&link, &id1));
    assert!(other.is_dir(), "the old target is left alone");
}

#[test]
fn a_missing_install_is_an_error_and_makes_no_link() {
    let dir = TempDir::new("missing");
    let link = dir.path().join("run").join("id1");
    let target = dir.path().join("uninstalled").join("id1");
    let error = link_dir(&target, &link).unwrap_err();
    assert!(matches!(&error, LinkError::MissingTarget(path) if *path == target));
    assert!(
        error
            .to_string()
            .ends_with("is not a directory that exists")
    );
    assert!(fs::symlink_metadata(&link).is_err());
    let file = dir.path().join("file");
    fs::write(&file, b"x").unwrap();
    assert!(matches!(
        link_dir(&file, &link),
        Err(LinkError::MissingTarget(_))
    ));
}

#[test]
fn a_real_folder_at_the_link_is_never_removed() {
    let dir = TempDir::new("occupied");
    let id1 = stand_in_install(&dir);
    let link = dir.path().join("run").join("id1");
    fs::create_dir_all(&link).unwrap();
    fs::write(link.join("keep.txt"), b"mine").unwrap();
    let error = link_dir(&id1, &link).unwrap_err();
    assert!(matches!(&error, LinkError::Occupied(path) if *path == link));
    assert!(error.to_string().contains("not a link"));
    assert_eq!(fs::read(link.join("keep.txt")).unwrap(), b"mine");
}

#[test]
fn io_errors_name_their_path() {
    let error = LinkError::Io {
        path: p("/x"),
        error: std::io::ErrorKind::PermissionDenied.into(),
    };
    assert!(
        error
            .to_string()
            .starts_with(&format!("{}: ", Path::new("/x").display()))
    );
    assert!(std::error::Error::source(&error).is_some());
    assert!(std::error::Error::source(&LinkError::Occupied(p("/x"))).is_none());
}
