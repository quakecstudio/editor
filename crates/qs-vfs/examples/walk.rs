// SPDX-License-Identifier: GPL-3.0-or-later

//! Builds the search path of an install, then lists and reads every file on it: the check
//! the acceptance criterion of #42 asks for on a real id1 and rerelease install.
//!
//! ```text
//! cargo run --release -p qs-vfs --example walk -- -basedir <install> [-game <dir>] ...
//! ```
//!
//! The arguments are the engine's (FS-1, FS-2). It prints each element of the search path,
//! with each archive's entry count, directory CRC and whether it marks the game as modified,
//! then what `gfx.wad` holds, the number of files and bytes read, and a 64-bit FNV-1a digest
//! of every name and its contents, so two runs on the same install can be compared. Any file
//! that cannot be read is printed, and makes the run fail.

#![expect(
    clippy::print_stdout,
    reason = "a command-line example: its output is the report"
)]

use std::env;

use qs_vfs::{Element, StdFs, Vfs};
use qs_wad::{WAD_GFX_NAME, Wad};

/// FNV-1a, 64 bits.
fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
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
    for file in vfs.files() {
        match vfs.read(&file) {
            Ok(data) => {
                files = files.saturating_add(1);
                bytes = bytes.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
                digest = fnv1a(fnv1a(digest, &file.name), &data);
            }
            Err(error) => {
                failed = failed.saturating_add(1);
                println!("failed: {}: {error}", file.name.escape_ascii());
            }
        }
    }
    println!("read {files} files, {bytes} bytes, digest {digest:016x}");
    if failed > 0 {
        return Err(format!("{failed} files could not be read"));
    }
    Ok(())
}
