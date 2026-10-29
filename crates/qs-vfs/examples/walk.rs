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
//! `.spr` is loaded as `Mod_LoadSpriteModel` does (SPR-1, SPR-2), every `.wav` under
//! `sound/` as `S_LoadSound` does (WAV-1 to WAV-3), every `.md5mesh` with its `.md5anim` (MD5-1 to
//! MD5-3), every `.mdl` as `Mod_LoadAliasModel` does (MDL-1 to MDL-5), and every `.bsp` as
//! `Mod_LoadBrushModel` does (BSP-1 to BSP-17), and its entity text read and written back by
//! `qs-map` (BSP-2, MAP-1, MAP-2); the counts are printed. A BSP that a lookup reaches has its
//! `.lit` chosen as QuakeSpasm's `Mod_LoadLighting` chooses it, game directory priority
//! included (BSP-18); a `.lit` it ignores never stops the map loading (BSP-18.8), so its
//! message is printed as a `warning:` line. Any file that cannot be read or decoded is
//! printed as a `failed:` line, and makes the run fail. A `.wav` outside `sound/` is never
//! loaded as a sound (WAV-3.5), so it is only counted, on a `not sounds:` line.

#![expect(
    clippy::print_stdout,
    reason = "a command-line example: its output is the report"
)]

use std::env;

use qs_bsp::lit::companion_name;
use qs_bsp::{Bsp, BspLoader, FoundLit, LitChoice, PathId, mod_load_lighting};
use qs_map::MapFile;
use qs_mdl::md5::md5_anim_path;
use qs_mdl::{Md5Anim, Md5Mesh, Mdl, Sprite};
use qs_vfs::{Element, FileRef, StdFs, Vfs};
use qs_wad::{WAD_GFX_NAME, Wad};
use qs_wav::{s_load_sound, sound_name};

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
    models: u64,
    bsps: u64,
    lits: u64,
    lower_priority_lits: u64,
    not_sounds: u64,
}

/// The path id QuakeSpasm gives each element's game directory, as its
/// `COM_AddGameDirectory` does: each game directory added doubles it, so a later, higher
/// priority one has a larger id, and its archives share its directory's id (BSP-18.4).
fn path_ids(vfs: &Vfs<StdFs>) -> Vec<PathId> {
    let mut ids = Vec::new();
    let mut id: Option<PathId> = None;
    for element in vfs.elements().iter().rev() {
        if let Element::Directory(_) = element {
            id = Some(id.map_or(PathId::FIRST, PathId::next));
        }
        // An archive is added after its game directory, so one always comes first.
        ids.push(id.unwrap_or(PathId::FIRST));
    }
    ids.reverse();
    ids
}

/// Loads a BSP and checks that its entity text reads and writes back unchanged; when a lookup
/// reaches this BSP, chooses its `.lit` as the default loader does and prints a `warning:`
/// line for one QuakeSpasm ignores with a message.
fn decode_bsp(
    vfs: &Vfs<StdFs>,
    path_ids: &[PathId],
    file: &FileRef,
    data: &[u8],
    decoded: &mut Decoded,
) -> Result<(), String> {
    let name = file.name.as_slice();
    let bsp = Bsp::read(data, BspLoader::default()).map_err(|error| error.to_string())?;
    let entities = MapFile::parse(bsp.entities()).map_err(|error| format!("entities: {error}"))?;
    if entities.to_bytes() != bsp.entities() {
        return Err("the entity text does not write back unchanged".to_owned());
    }
    decoded.bsps = decoded.bsps.saturating_add(1);
    if vfs.find(name).as_ref() != Some(file) {
        return Ok(());
    }
    let path_id = |element: usize| path_ids.get(element).copied().unwrap_or(PathId::FIRST);
    let lit_name = companion_name(&String::from_utf8_lossy(name), ".lit");
    let lit_bytes = match vfs.find(lit_name.as_bytes()) {
        Some(lit) => Some((
            vfs.read(&lit)
                .map_err(|error| format!("{lit_name}: {error}"))?,
            path_id(lit.element),
        )),
        None => None,
    };
    let found = lit_bytes.as_ref().map(|(bytes, lit_path_id)| FoundLit {
        bytes,
        path_id: *lit_path_id,
    });
    let choice = mod_load_lighting(
        BspLoader::default(),
        path_id(file.element),
        bsp.lighting.len(),
        found,
    );
    match choice {
        LitChoice::Used(_) => decoded.lits = decoded.lits.saturating_add(1),
        LitChoice::LowerPriority => {
            decoded.lower_priority_lits = decoded.lower_priority_lits.saturating_add(1);
        }
        LitChoice::Ignored(_) | LitChoice::NotRead | LitChoice::Missing => {}
    }
    if let Some(message) = choice.message(&lit_name) {
        println!("warning: {}: {message}", name.escape_ascii());
    }
    Ok(())
}

/// Decodes the contents `data` of `file` by the extension of its name; `Err` says why it
/// failed.
fn decode(
    vfs: &Vfs<StdFs>,
    path_ids: &[PathId],
    file: &FileRef,
    data: &[u8],
    decoded: &mut Decoded,
) -> Result<(), String> {
    let name = file.name.as_slice();
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(b".bsp") {
        decode_bsp(vfs, path_ids, file, data, decoded)?;
    } else if lower.ends_with(b".mdl") {
        Mdl::read(data).map_err(|error| error.to_string())?;
        decoded.models = decoded.models.saturating_add(1);
    } else if lower.ends_with(b".spr") {
        Sprite::read(data).map_err(|error| error.to_string())?;
        decoded.sprites = decoded.sprites.saturating_add(1);
    } else if lower.ends_with(b".wav") {
        if sound_name(name).is_some() {
            s_load_sound(data).map_err(|error| error.to_string())?;
            decoded.sounds = decoded.sounds.saturating_add(1);
        } else {
            decoded.not_sounds = decoded.not_sounds.saturating_add(1);
        }
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
    let path_ids = path_ids(&vfs);
    for file in vfs.files() {
        match vfs.read(&file) {
            Ok(data) => {
                files = files.saturating_add(1);
                bytes = bytes.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
                digest = fnv1a(fnv1a(digest, &file.name), &data);
                if let Err(error) = decode(&vfs, &path_ids, &file, &data, &mut decoded) {
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
        "decoded {} sprites, {} sounds, {} MD5 meshes, {} MD5 animations, {} alias models, \
         {} BSPs, {} .lit files ({} from a game directory of lower priority, ignored)",
        decoded.sprites,
        decoded.sounds,
        decoded.meshes,
        decoded.anims,
        decoded.models,
        decoded.bsps,
        decoded.lits,
        decoded.lower_priority_lits
    );
    println!(
        "not sounds: {} .wav files outside sound/, which S_LoadSound never loads",
        decoded.not_sounds
    );
    println!("read {files} files, {bytes} bytes, digest {digest:016x}");
    if failed > 0 {
        return Err(format!("{failed} files could not be read or decoded"));
    }
    Ok(())
}
