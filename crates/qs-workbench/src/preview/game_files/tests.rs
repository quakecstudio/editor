// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::{Path, PathBuf};

use super::{GameFiles, GameState};

/// A folder of empty files at `files`, relative to a fresh directory named for `name`.
fn folder(name: &str, files: &[&str]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("qs-game-files-{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    for file in files {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"PACK").unwrap();
    }
    dir
}

fn check(dir: &Path) -> GameFiles {
    GameFiles::check(Some(dir))
}

// spec FS-6.5: an original base with both paks lacks nothing, and plays
#[test]
fn both_paks_are_found() {
    let dir = folder("found", &["id1/pak0.pak", "id1/pak1.pak"]);
    let files = check(&dir);
    assert_eq!(files.state, GameState::Found);
    assert!(files.missing.is_empty());
    assert_eq!(files.folder.as_deref(), Some(dir.as_path()));
    assert!(files.state.plays());
}

// spec FS-6.5: an original base with no pak1.pak is the shareware game, which plays its maps
#[test]
fn pak0_alone_is_shareware() {
    let dir = folder("shareware", &["id1/pak0.pak"]);
    let files = check(&dir);
    assert_eq!(files.state, GameState::SharewareOnly);
    assert_eq!(files.missing, ["id1/pak1.pak"]);
    assert!(files.state.plays());
}

// spec FS-6.5: a rerelease with no original base lacks the original game's two paks, and does
// not play
#[test]
fn the_rerelease_alone_is_partial() {
    let dir = folder("rerelease", &["QuakeEX.kpf", "id1/pak0.pak"]);
    let files = check(&dir);
    assert_eq!(files.state, GameState::RereleaseOnly);
    assert_eq!(files.missing, ["id1/pak0.pak", "id1/pak1.pak"]);
    assert!(!files.state.plays());
}

// spec FS-6.6: a rerelease with the original game in its dos folder is found
#[test]
fn the_rerelease_with_its_dos_folder_is_found() {
    let dir = folder(
        "rerelease-dos",
        &[
            "QuakeEX.kpf",
            "id1/pak0.pak",
            "dos/id1/pak0.pak",
            "dos/id1/pak1.pak",
        ],
    );
    assert_eq!(check(&dir).state, GameState::Found);
}

// No spec statement: a folder with no game data is missing it, named as the folder chosen
#[test]
fn an_empty_folder_is_missing() {
    let dir = folder("empty", &["readme.txt"]);
    let files = check(&dir);
    assert_eq!(files.state, GameState::Missing);
    assert_eq!(files.folder.as_deref(), Some(dir.as_path()));
    assert_eq!(files.missing, ["id1/pak0.pak", "id1/pak1.pak"]);
    assert!(!files.state.plays());
}
