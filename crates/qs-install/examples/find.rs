// SPDX-License-Identifier: GPL-3.0-or-later

//! Finds Quake on this machine, or looks at the folders given, and checks every install
//! found: the manual check of #43, run on real installs in P1-30.
//!
//! ```text
//! cargo run --release -p qs-install --example find [-- <folder>...]
//! ```
//!
//! With no folders it searches the stores' usual places for the running OS; with folders it
//! examines each one as a folder the player chose (a steamcmd download, innoextract's output,
//! or any folder whose `id1` holds `pak0.pak`). For each install it prints how it was found,
//! its base directories with their `id1`, `pak1.pak`, game directories and `QuakeEX.kpf`,
//! then what each archive turned out to be (with the size and MD5 of an unrecognised one),
//! the editions present and the files missing. It fails when nothing is found.

#![expect(
    clippy::print_stdout,
    reason = "a command-line example: its output is the report"
)]

use std::env;
use std::path::PathBuf;

use qs_install::{FileStatus, GameBase, Install, examine_folder, find_installs, size_and_md5};
use qs_vfs::StdFs;

fn print_base(label: &str, base: Option<&GameBase>) {
    let Some(base) = base else {
        println!("  {label}: none");
        return;
    };
    println!("  {label}: {}", base.path.display());
    println!("    id1: {}", base.id1.display());
    match &base.pak1 {
        Some(pak1) => println!("    pak1: {}", pak1.display()),
        None => println!("    pak1: none"),
    }
    for dir in &base.game_dirs {
        println!("    game dir {}: {}", dir.name, dir.path.display());
    }
    if let Some(kpf) = &base.kpf {
        println!("    kpf: {}", kpf.display());
    }
}

fn print_install(install: &Install) {
    println!("{:?} install at {}", install.source, install.root.display());
    print_base("original", install.original.as_ref());
    print_base("rerelease", install.rerelease.as_ref());
    let validation = install.validate(&StdFs);
    for file in &validation.files {
        let status = match file.status {
            FileStatus::Known(known) => format!("known: {} ({})", known.description, known.md5),
            FileStatus::Unrecognised { entries, modified } => {
                let checksum = size_and_md5(&StdFs, &file.path).map_or_else(
                    |error| format!("could not be hashed: {error}"),
                    |(size, md5)| format!("{size} bytes, MD5 {md5}"),
                );
                format!("unrecognised PAK: {entries} entries, modified {modified}, {checksum}")
            }
            FileStatus::Unusable(error) => format!("unusable: {error}"),
        };
        println!("  file {}: {status}", file.path.display());
    }
    println!("  editions: {:?}", validation.editions);
    for missing in &validation.missing {
        println!("  missing {}: {:?}", missing.file, missing.reason);
    }
}

fn main() -> Result<(), String> {
    let folders: Vec<PathBuf> = env::args_os().skip(1).map(PathBuf::from).collect();
    let installs: Vec<Install> = if folders.is_empty() {
        find_installs()
    } else {
        folders
            .iter()
            .filter_map(|folder| {
                let install = examine_folder(folder);
                if install.is_none() {
                    println!("no game in {}", folder.display());
                }
                install
            })
            .collect()
    };
    for install in &installs {
        print_install(install);
    }
    if installs.is_empty() {
        return Err("no Quake install found".to_owned());
    }
    println!("found {} install(s)", installs.len());
    Ok(())
}
