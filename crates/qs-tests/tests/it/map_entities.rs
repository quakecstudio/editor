// SPDX-License-Identifier: GPL-3.0-or-later

//! Editing the entities of a shipped map with no `.map` source (MAP-22): `qs-bsp` reads the
//! entity lump, `qs-map` edits it without losing a byte, and the result goes back into the BSP
//! or into a `.ent` file beside it.

#![expect(
    clippy::expect_used,
    reason = "test code: a failed expect is a failed test with a message"
)]

use std::fs;
use std::path::PathBuf;

use qs_bsp::{Bsp, BspLoader, ent_file_names, replace_entities};
use qs_map::MapFile;

/// The spec's `base` BSP vector, from its `xxd` dump.
fn base_bsp() -> Vec<u8> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/vectors/bsp/base.txt");
    let text = fs::read_to_string(path).expect("the base vector is readable");
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((_, rest)) = line.split_once(": ") else {
            continue;
        };
        for group in rest.get(..39).unwrap_or(rest).split_whitespace() {
            for pair in group.as_bytes().chunks(2) {
                let pair = core::str::from_utf8(pair).expect("hex is ASCII");
                out.push(u8::from_str_radix(pair, 16).expect("hex digits"));
            }
        }
    }
    out
}

// spec BSP-2.1, BSP-2.2, BSP-19.1, BSP-19.3, MAP-2.2: a BSP's entity text reads into the lossless
// document, an edit changes only its pair, and the text goes back into the BSP with a NUL, or
// alone into the `.ent` file QuakeSpasm looks for.
#[test]
fn entities_of_a_shipped_map_are_edited() {
    let bytes = base_bsp();
    let bsp = Bsp::read(&bytes, BspLoader::default()).expect("the base vector loads");
    let mut map = MapFile::parse(bsp.entities()).expect("the entity text reads");
    assert_eq!(map.to_bytes(), bsp.entities());
    map.entities
        .get_mut(1)
        .expect("the door")
        .set(b"model", b"*1")
        .expect("writable");
    map.entities
        .get_mut(0)
        .expect("the world")
        .set(b"message", b"Edited")
        .expect("writable");
    let text = map.to_bytes();
    assert_eq!(
        text,
        b"{\n\"classname\" \"worldspawn\"\n\"wad\" \"gfx/base.wad\"\n\"message\" \"Edited\"\n}\n\
{\n\"classname\" \"func_door\"\n\"model\" \"*1\"\n}\n"
    );
    let edited =
        replace_entities(&bytes, &text, BspLoader::default()).expect("the BSP is rewritten");
    let reread = Bsp::read(&edited, BspLoader::default()).expect("the edited BSP loads");
    assert_eq!(reread.entities(), text.as_slice());
    assert_eq!(reread.faces, bsp.faces);
    let [with_crc, plain] = ent_file_names("maps/base.bsp", bsp.entity_lump);
    assert!(with_crc.starts_with("maps/base@") && with_crc.ends_with(".ent"));
    assert_eq!(plain, "maps/base.ent");
    let ent = MapFile::parse(&text).expect("the .ent file reads");
    assert_eq!(ent.entities.len(), 2);
}
