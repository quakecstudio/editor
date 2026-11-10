// SPDX-License-Identifier: GPL-3.0-or-later

//! PAK archives our writer makes (PAK-6) load through the game filesystem the preview host
//! reads its game from (FS-1, FS-2), a mod's archive over `id1`'s (P4-27).

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::fs;
use std::path::PathBuf;

use qs_pak::write_pak;
use qs_vfs::{FS_POP_PATH, FS_POP_TABLE, StdFs, Vfs};

/// A `gfx/pop.lmp` holding id's `pop` table, which marks the game registered (FS-4.2), so a
/// mod may load (FS-4.3).
fn pop_lmp() -> Vec<u8> {
    FS_POP_TABLE
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect()
}

/// A fresh game root under the system's temporary directory.
fn game_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("qs-pak-written-{name}-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    fs::create_dir_all(root.join("id1")).unwrap();
    fs::create_dir_all(root.join("mymod")).unwrap();
    root
}

// spec PAK-6.2: archives the writer makes load on the game's search path as id's do, a mod's
// archive found before id1's
#[test]
fn written_archives_load_on_the_search_path() {
    let root = game_root("search");
    let pop = pop_lmp();
    let base = write_pak(&[
        ("progs.dat", b"base progs".as_slice()),
        ("gfx/palette.lmp", b"palette".as_slice()),
        (FS_POP_PATH, pop.as_slice()),
    ])
    .unwrap();
    let mine = write_pak(&[("progs.dat", b"mod progs".as_slice())]).unwrap();
    fs::write(root.join("id1/pak0.pak"), base).unwrap();
    fs::write(root.join("mymod/pak0.pak"), mine).unwrap();
    let basedir = root.display().to_string();
    let vfs = Vfs::new(
        StdFs,
        &["-basedir", basedir.as_str(), "-game", "mymod"],
        ".",
    )
    .unwrap();
    let progs = vfs.load(b"progs.dat").unwrap().unwrap();
    assert_eq!(progs.bytes(), b"mod progs");
    let palette = vfs.load(b"gfx/palette.lmp").unwrap().unwrap();
    assert_eq!(palette.bytes(), b"palette");
    fs::remove_dir_all(&root).unwrap();
}
