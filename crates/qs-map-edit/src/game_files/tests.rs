// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::{Path, PathBuf};

use super::{FileOrigin, GameFile, GameFileKind};

/// A folder of its own for `name`, with `files` written in it, empty.
fn folder(name: &str, files: &[&str]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("qs-game-files-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    for file in files {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }
    dir
}

fn names(files: &[GameFile]) -> Vec<&str> {
    files.iter().map(|file| file.name.as_str()).collect()
}

const PROJECT: [&str; 8] = [
    "maps/start.bsp",
    "maps/b_bh25.bsp",
    "maps/sub/odd.bsp",
    "maps/start.map",
    "progs/ogre.mdl",
    "progs/s_light.spr",
    "sound/doors/drclos4.wav",
    ".git/sound/hidden.wav",
];

// spec MAP-8.11: a level is named as `map` takes it, from `maps/<map>.bsp`; an item's box
// model and a file in a folder under `maps/` are no level
#[test]
fn levels_are_named_as_map_takes_them() {
    let root = folder("levels", &PROJECT);
    let files = GameFileKind::Level.files(Some(&root), None);
    assert_eq!(names(&files), ["start"]);
    assert_eq!(
        files.first().map(|file| file.origin),
        Some(FileOrigin::Project)
    );
}

// spec MAP-8.11: a model is named by its whole path, an alias model, a brush model or a
// sprite
#[test]
fn models_are_named_by_their_path() {
    let root = folder("models", &PROJECT);
    let files = GameFileKind::Model.files(Some(&root), None);
    assert_eq!(
        names(&files),
        [
            "maps/b_bh25.bsp",
            "maps/start.bsp",
            "maps/sub/odd.bsp",
            "progs/ogre.mdl",
            "progs/s_light.spr"
        ]
    );
}

// spec MAP-8.11: a sound is named by its path under `sound/`; a folder starting with `.` is not
// read
#[test]
fn sounds_are_named_under_the_sound_folder() {
    let root = folder("sounds", &PROJECT);
    let files = GameFileKind::Sound.files(Some(&root), None);
    assert_eq!(names(&files), ["doors/drclos4.wav"]);
}

// spec MAP-8.11: the game's search path is read too, and the project's file takes the game's
// place for a name both have
#[test]
fn the_project_takes_the_games_place() {
    let base = folder(
        "base",
        &["id1/sound/doors/drclos4.wav", "id1/sound/misc/null.wav"],
    );
    let root = folder("over", &["sound/doors/drclos4.wav"]);
    let files = GameFileKind::Sound.files(Some(&root), Some(&base));
    assert_eq!(
        files,
        [
            GameFile {
                name: "doors/drclos4.wav".to_owned(),
                origin: FileOrigin::Project,
            },
            GameFile {
                name: "misc/null.wav".to_owned(),
                origin: FileOrigin::Game,
            },
        ]
    );
}

#[test]
fn nothing_is_listed_with_no_folder_or_a_missing_one() {
    assert!(GameFileKind::Level.files(None, None).is_empty());
    let missing = Path::new("/no/such/folder/for/qs-game-files");
    assert!(
        GameFileKind::Level
            .files(Some(missing), Some(missing))
            .is_empty()
    );
}
