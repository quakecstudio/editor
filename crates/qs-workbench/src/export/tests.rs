// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::io::Cursor;
use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use qs_vfs::zip::Zip;
use serde_json::json;

use super::{ExportError, ExportParams, Exported, GameNeeded, PROJECT_EXPORT};
use crate::{Config, Workbench, register};

/// A project on disk to export: what it holds beyond its program.
struct ProjectSpec {
    /// Its QuakeC, compiled into progs.dat.
    world: &'static str,
    /// Files under the project, by path, with their bytes.
    files: Vec<(&'static str, Vec<u8>)>,
}

impl Default for ProjectSpec {
    fn default() -> Self {
        Self {
            world: "void() worldspawn = {};\n",
            files: vec![
                ("maps/start.bsp", b"BSP2 start".to_vec()),
                ("maps/start.map", b"// a map source".to_vec()),
                ("sound/misc/hit.wav", b"RIFF hit".to_vec()),
                ("progs/ogre.mdl", b"IDPO ogre".to_vec()),
                ("COPYING", b"GPL text".to_vec()),
                (
                    "ATTRIBUTION.md",
                    b"- maps/start.bsp: mine\n- hit.wav: mine\n".to_vec(),
                ),
                ("notes.txt", b"not shipped".to_vec()),
            ],
        }
    }
}

/// A fresh project for test `name` as `spec` says; its directory, which names the mod.
fn project(name: &str, spec: &ProjectSpec) -> PathBuf {
    let root = std::env::temp_dir()
        .join(format!("qs-export-{name}-{}", std::process::id()))
        .join(format!("{name}mod"));
    let _gone = std::fs::remove_dir_all(root.parent().unwrap());
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    std::fs::write(root.join("world.qc"), spec.world).unwrap();
    for (path, bytes) in &spec.files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    root
}

fn export(root: &Path, params: &ExportParams) -> Result<Exported, ExportError> {
    let mut workbench = Workbench::new(FsLoader, Config::default());
    workbench
        .open_project(root)
        .map_err(ExportError::Workbench)?;
    workbench.export(params)
}

fn member(zip: &[u8], name: &str) -> Option<Vec<u8>> {
    Zip::open(Cursor::new(zip.to_vec()))
        .unwrap()
        .read(name)
        .unwrap()
}

// No spec statement: P4-28, QA-15: the zip holds the mod folder: its program, its own assets
// (no map source, nothing outside the asset folders), its notices and a README saying the game
// it needs and how to install it; an asset the attribution file does not name is listed
#[test]
fn an_export_holds_the_mod_folder() {
    let root = project("plain", &ProjectSpec::default());
    let exported = export(&root, &ExportParams::default()).unwrap();
    assert_eq!(exported.path, root.join("build/plainmod.zip"));
    assert_eq!(exported.mod_name, "plainmod");
    assert_eq!(exported.game, GameNeeded::Original);
    assert_eq!(
        exported.files,
        [
            "plainmod/ATTRIBUTION.md",
            "plainmod/COPYING",
            "plainmod/README.txt",
            "plainmod/maps/start.bsp",
            "plainmod/progs.dat",
            "plainmod/progs/ogre.mdl",
            "plainmod/sound/misc/hit.wav",
        ]
    );
    assert_eq!(exported.unknown_license, ["progs/ogre.mdl"]);
    let zip = std::fs::read(&exported.path).unwrap();
    assert_eq!(
        member(&zip, "plainmod/progs.dat").unwrap(),
        std::fs::read(root.join("build/plainmod/progs.dat")).unwrap()
    );
    let readme = String::from_utf8(member(&zip, "plainmod/README.txt").unwrap()).unwrap();
    for part in [
        "It needs the original Quake (1996)",
        "quakespasm -game plainmod",
        "does not include any of id Software's game data",
        "plainmod/maps/start.bsp",
        "ATTRIBUTION.md, COPYING",
    ] {
        assert!(readme.contains(part), "{part} in {readme}");
    }
}

// No spec statement: P4-28, the acceptance criterion: exporting twice from the same sources
// gives the same zip, packed or not
#[test]
fn exporting_twice_gives_the_same_zip() {
    let root = project("twice", &ProjectSpec::default());
    for pak in [false, true] {
        let params = ExportParams {
            pak,
            ..ExportParams::default()
        };
        let first = std::fs::read(export(&root, &params).unwrap().path).unwrap();
        let second = std::fs::read(export(&root, &params).unwrap().path).unwrap();
        assert_eq!(first, second, "pak: {pak}");
    }
}

// No spec statement: P4-28, PAK-6: with pak, the assets are one pak0.pak in the mod folder,
// each member as it is in the project
#[test]
fn the_assets_can_be_packed() {
    let root = project("packed", &ProjectSpec::default());
    let params = ExportParams {
        pak: true,
        ..ExportParams::default()
    };
    let exported = export(&root, &params).unwrap();
    assert!(exported.files.contains(&"packedmod/pak0.pak".to_owned()));
    assert!(!exported.files.iter().any(|f| f.ends_with("start.bsp")));
    let zip = std::fs::read(&exported.path).unwrap();
    let pak_bytes = member(&zip, "packedmod/pak0.pak").unwrap();
    let pak = qs_pak::Pak::read(&pak_bytes).unwrap();
    let entry = pak.find(b"maps/start.bsp").unwrap();
    assert_eq!(pak.entry_data(&pak_bytes, entry).unwrap(), b"BSP2 start");
}

// No spec statement: P4-28, LIC-4, the acceptance criterion: an export that would include a
// file of the game's id1 PAKs fails with a message naming it, and the zip is not written
#[test]
fn id_data_is_refused() {
    let game = std::env::temp_dir().join(format!("qs-export-game-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&game);
    std::fs::create_dir_all(game.join("id1")).unwrap();
    let pak = qs_pak::write_pak(&[("progs/player.mdl", b"IDPO id's player".as_slice())]).unwrap();
    std::fs::write(game.join("id1/pak0.pak"), pak).unwrap();
    let mut spec = ProjectSpec::default();
    spec.files
        .push(("progs/player.mdl", b"IDPO id's player".to_vec()));
    let root = project("iddata", &spec);
    let params = ExportParams {
        game: Some(game.clone()),
        ..ExportParams::default()
    };
    let error = export(&root, &params).unwrap_err();
    let message = error.to_string();
    assert!(matches!(error, ExportError::IdData { .. }), "{message}");
    assert!(
        message.contains("player.mdl") && message.contains("progs/player.mdl in"),
        "{message}"
    );
    assert!(message.contains("must never include"), "{message}");
    assert!(!root.join("build/iddatamod.zip").exists());
}

// No spec statement: P4-28, ADR-0008, the acceptance criterion: a project whose build/run/
// links point at an install exports no file reached through them, and no link elsewhere is
// followed
#[cfg(unix)]
#[test]
fn links_are_never_followed() {
    let install = std::env::temp_dir().join(format!("qs-export-install-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&install);
    std::fs::create_dir_all(install.join("id1/maps")).unwrap();
    std::fs::write(install.join("id1/maps/e1m1.bsp"), b"id's map").unwrap();
    let root = project("links", &ProjectSpec::default());
    std::fs::create_dir_all(root.join("build/run/original")).unwrap();
    std::os::unix::fs::symlink(install.join("id1"), root.join("build/run/original/id1")).unwrap();
    std::os::unix::fs::symlink(install.join("id1/maps"), root.join("maps/linked")).unwrap();
    let exported = export(&root, &ExportParams::default()).unwrap();
    assert!(
        !exported
            .files
            .iter()
            .any(|f| f.contains("e1m1") || f.contains("run/")),
        "{:?}",
        exported.files
    );
}

// No spec statement: P4-28: a project that does not build exports nothing, naming the module
#[test]
fn a_project_that_does_not_build_is_not_exported() {
    let root = project(
        "broken",
        &ProjectSpec {
            world: "void() worldspawn = { nosuch(); };\n",
            ..ProjectSpec::default()
        },
    );
    let error = export(&root, &ExportParams::default()).unwrap_err();
    assert!(
        matches!(&error, ExportError::NotBuilt(modules) if modules.len() == 1),
        "{error}"
    );
}

// No spec statement: P4-28: the command answers what the method does, so the palette, the CLI
// and the agent export the same way
#[test]
fn the_command_exports() {
    let root = project("command", &ProjectSpec::default());
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute("project.open", json!({ "path": root }))
        .unwrap();
    let answer = session.execute(PROJECT_EXPORT, json!({})).unwrap();
    assert_eq!(answer["mod_name"], "commandmod");
    assert_eq!(answer["game"], "original");
    assert!(root.join("build/commandmod.zip").is_file());
}
