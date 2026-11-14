// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    reason = "test code: a failed unwrap is a failed test, and the fixture's sizes are small"
)]

use std::path::{Path, PathBuf};

use qs_sim::localization::RRE_LOC_FILE;
use qs_vfs::zip::crc32;
use qs_vfs::{FS_POP_PATH, FS_POP_TABLE, LoadedFile, StdFs, Vfs};

use super::{is_external_brush_model, localization};
use crate::args::{Args, engine_style};

/// A game root removed when the test ends.
struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("quakec-engine-{name}-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn write(&self, path: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _gone = std::fs::remove_dir_all(&self.0);
    }
}

/// A zip of `files`, each stored, as `QuakeEX.kpf` holds its members.
fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let offset = out.len() as u32;
        let mut fixed = Vec::new();
        fixed.extend_from_slice(&20_u16.to_le_bytes());
        fixed.extend_from_slice(&[0; 2]);
        fixed.extend_from_slice(&0_u16.to_le_bytes());
        fixed.extend_from_slice(&[0; 4]);
        fixed.extend_from_slice(&crc32(data).to_le_bytes());
        fixed.extend_from_slice(&(data.len() as u32).to_le_bytes());
        fixed.extend_from_slice(&(data.len() as u32).to_le_bytes());
        fixed.extend_from_slice(&(name.len() as u16).to_le_bytes());
        fixed.extend_from_slice(&[0; 2]);
        out.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
        out.extend_from_slice(&fixed);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        central.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
        central.extend_from_slice(&20_u16.to_le_bytes());
        central.extend_from_slice(&fixed);
        central.extend_from_slice(&[0; 10]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&[0; 2]);
    out
}

fn args(words: &[&str]) -> Args {
    let line = core::iter::once("quakec-engine")
        .chain(words.iter().copied())
        .map(str::to_owned);
    <Args as clap::Parser>::try_parse_from(engine_style(line)).unwrap()
}

fn vfs(args: &Args) -> Vfs<StdFs> {
    Vfs::new(StdFs, &args.engine(), ".").unwrap()
}

fn read(vfs: &Vfs<StdFs>, name: &str) -> Option<Vec<u8>> {
    vfs.load(name.as_bytes())
        .unwrap()
        .map(LoadedFile::into_bytes)
}

fn text(path: &Path) -> String {
    path.display().to_string()
}

// ADR-0008: started with -basedir and -game against a game root, the host reads the mod's
// files over id1's, and the rerelease's strings from the -kpf archive outside the root
#[test]
fn the_mod_and_the_kpf_come_from_the_command_line() {
    let root = Root::new("kpf");
    // A registered game, as `gfx/pop.lmp` marks one (FS-4.2): a mod needs it (FS-4.3).
    let pop: Vec<u8> = FS_POP_TABLE.iter().flat_map(|v| v.to_be_bytes()).collect();
    root.write(&format!("game/id1/{FS_POP_PATH}"), &pop);
    root.write("game/id1/progs.dat", b"id1");
    root.write("game/id1/gfx.wad", b"id1 wad");
    root.write("game/mymod/progs.dat", b"mymod");
    let kpf = root.write(
        "elsewhere/QuakeEX.kpf",
        &zip(&[(RRE_LOC_FILE, b"$qc_test = \"from the kpf\"\n")]),
    );
    let basedir = root.0.join("game");
    let with_kpf = args(&[
        "-basedir",
        &text(&basedir),
        "-game",
        "mymod",
        "-kpf",
        &text(&kpf),
    ]);
    let files = vfs(&with_kpf);
    assert_eq!(read(&files, "progs.dat").as_deref(), Some(&b"mymod"[..]));
    assert_eq!(read(&files, "gfx.wad").as_deref(), Some(&b"id1 wad"[..]));
    let strings = localization(&files, &with_kpf).unwrap().unwrap();
    assert_eq!(strings, b"$qc_test = \"from the kpf\"\n");

    // Without -kpf the root has no QuakeEX.kpf: no strings.
    let without = args(&["-basedir", &text(&basedir), "-game", "mymod"]);
    assert_eq!(localization(&vfs(&without), &without).unwrap(), None);

    // A -kpf file that does not exist is an error, not silence.
    let missing = root.0.join("nowhere.kpf");
    let bad = args(&["-basedir", &text(&basedir), "-kpf", &text(&missing)]);
    assert!(localization(&vfs(&bad), &bad).is_err());

    // The search path's own copy comes first, as LOC_LoadFile reads it (RRE-1.10).
    root.write(
        &format!("game/mymod/{RRE_LOC_FILE}"),
        b"$qc_test = \"loose\"\n",
    );
    let files = vfs(&with_kpf);
    assert_eq!(
        localization(&files, &with_kpf).unwrap().unwrap(),
        b"$qc_test = \"loose\"\n"
    );
}

// spec PHYS-13.6: a save's name stays under the game directory, `.sav` added when it has no
// extension, as `sprintf ("%s/%s", com_gamedir, name)` keeps it
#[test]
fn a_save_stays_in_the_game_directory() {
    let args = args(&["-basedir", "/q", "-game", "mod"]);
    assert_eq!(args.save_path("s1"), Path::new("/q/mod/s1.sav"));
    assert_eq!(args.save_path("s1.txt"), Path::new("/q/mod/s1.txt"));
    let absolute = args.save_path("/etc/x");
    assert!(absolute.starts_with("/q/mod"), "{absolute:?}");
    assert_eq!(absolute, Path::new("/q/mod//etc/x.sav"));
    let plain = self::args(&["-basedir", "/q"]).save_path("s1");
    assert_eq!(plain, Path::new("/q/id1/s1.sav"));
}

// spec REND-1.9: an entity showing a BSP of its own is drawn from it, the health and ammo boxes
// of `maps/b_*.bsp` too; the map's own BSP is the world, and a model or submodel is not a BSP
#[test]
fn boxes_and_barrels_are_external_brush_models() {
    let world = "maps/e4m6.bsp";
    assert!(is_external_brush_model("maps/b_bh25.bsp", world));
    assert!(is_external_brush_model("maps/b_explob.bsp", world));
    assert!(!is_external_brush_model(world, world));
    assert!(!is_external_brush_model("progs/player.mdl", world));
    assert!(!is_external_brush_model("*1", world));
}
