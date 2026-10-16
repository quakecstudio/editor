// SPDX-License-Identifier: GPL-3.0-or-later

//! Builds the search path of an install, then lists and reads every file on it, and decodes
//! every sprite, sound and MD5 model: the check P1-30 runs on a real id1 and rerelease
//! install.
//!
//! ```text
//! cargo run --release -p qs-vfs --example walk -- -basedir <install> [-game <dir>] ...
//! ```
//!
//! The arguments are the engine's (FS-1, FS-2). It prints each element of the search path,
//! with each archive's entry count, directory CRC and whether it marks the game as modified,
//! then what `gfx.wad` holds, the number of files and bytes read, and a 64-bit FNV-1a digest
//! of every name and its contents, so two runs on the same install can be compared. Every
//! `.spr` is loaded as `Mod_LoadSpriteModel` does (SPR-1, SPR-2), every `.wav` as
//! `S_LoadSound` does (WAV-1 to WAV-3), and every `.md5mesh` with its `.md5anim` (MD5-1 to
//! MD5-3), and the counts are printed. Any file that cannot be read or decoded is printed as
//! a `failed:` line, and makes the run fail.

#![expect(
    clippy::print_stdout,
    reason = "a command-line example: its output is the report"
)]

use std::env;

use qs_mdl::md5::md5_anim_path;
use qs_mdl::{Md5Anim, Md5Mesh, Sprite};
use qs_vfs::{Element, StdFs, Vfs};
use qs_wad::{WAD_GFX_NAME, Wad};
use qs_wav::s_load_sound;

/// FNV-1a, 64 bits.
fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

/// Counts of the files [`decode`] read, by kind.
#[derive(Default)]
struct Decoded {
    sprites: u64,
    sounds: u64,
    meshes: u64,
    anims: u64,
}

/// Decodes `data` by the extension of `name`; `Err` says why it failed.
fn decode(vfs: &Vfs<StdFs>, name: &[u8], data: &[u8], decoded: &mut Decoded) -> Result<(), String> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(b".spr") {
        Sprite::read(data).map_err(|error| error.to_string())?;
        decoded.sprites = decoded.sprites.saturating_add(1);
    } else if lower.ends_with(b".wav") {
        s_load_sound(data).map_err(|error| error.to_string())?;
        decoded.sounds = decoded.sounds.saturating_add(1);
    } else if lower.ends_with(b".md5mesh") {
        let mesh = Md5Mesh::read(data).map_err(|error| error.to_string())?;
        decoded.meshes = decoded.meshes.saturating_add(1);
        let anim_name = md5_anim_path(&String::from_utf8_lossy(name));
        let loaded = vfs
            .load(anim_name.as_bytes())
            .map_err(|error| format!("{anim_name}: {error}"))?;
        if let Some(file) = loaded {
            let anim =
                Md5Anim::read(file.bytes()).map_err(|error| format!("{anim_name}: {error}"))?;
            anim.check_mesh(&mesh)
                .map_err(|error| format!("{anim_name}: {error}"))?;
            decoded.anims = decoded.anims.saturating_add(1);
        }
    }
    Ok(())
}

fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let vfs = Vfs::new(StdFs, &args, ".").map_err(|error| error.to_string())?;
    println!("game directory: {}", vfs.game_dir());
    println!(
        "registered: {}, modified: {}",
        vfs.is_registered(),
        vfs.is_modified()
    );
    for element in vfs.elements() {
        match element {
            Element::Archive(archive) => println!(
                "archive {}: {} entries, directory CRC {}, modified {}",
                archive.name(),
                archive.pak().entries().len(),
                archive.pak().directory_crc(),
                archive.pak().is_modified()
            ),
            Element::Directory(directory) => println!("directory {}", directory.name()),
        }
    }
    match vfs.load_wad(WAD_GFX_NAME) {
        Ok(bytes) => {
            let wad = Wad::read(&bytes).map_err(|error| error.to_string())?;
            let conchars = wad.find(b"conchars").map(|lump| lump.filepos);
            println!(
                "{WAD_GFX_NAME}: {} lumps, conchars at {conchars:?}",
                wad.lumps().len()
            );
        }
        Err(error) => println!("{error}"),
    }
    let mut files = 0_u64;
    let mut bytes = 0_u64;
    let mut failed = 0_u64;
    let mut digest = 0xCBF2_9CE4_8422_2325_u64;
    let mut decoded = Decoded::default();
    for file in vfs.files() {
        match vfs.read(&file) {
            Ok(data) => {
                files = files.saturating_add(1);
                bytes = bytes.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
                digest = fnv1a(fnv1a(digest, &file.name), &data);
                if let Err(error) = decode(&vfs, &file.name, &data, &mut decoded) {
                    failed = failed.saturating_add(1);
                    println!("failed: {}: {error}", file.name.escape_ascii());
                }
            }
            Err(error) => {
                failed = failed.saturating_add(1);
                println!("failed: {}: {error}", file.name.escape_ascii());
            }
        }
    }
    println!(
        "decoded {} sprites, {} sounds, {} MD5 meshes, {} MD5 animations",
        decoded.sprites, decoded.sounds, decoded.meshes, decoded.anims
    );
    println!("read {files} files, {bytes} bytes, digest {digest:016x}");
    if failed > 0 {
        return Err(format!("{failed} files could not be read or decoded"));
    }
    Ok(())
}
