// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the lossless `.map` reader and to the model of id's qbsp (MAP-1 to
//! MAP-7), each as given and as text built from the format's tokens, so the fuzzer reaches
//! brushes and faces. Fails on a panic, and on a map that does not write back unchanged.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qs_map::{Compiler, MapFile, qbsp};

/// Tokens of a `.map`, which each input byte picks from.
const VOCABULARY: [&str; 24] = [
    "{",
    "}",
    "(",
    ")",
    "[",
    "]",
    "0",
    "1",
    "-16",
    "0.5",
    "64",
    "\"classname\"",
    "\"worldspawn\"",
    "\"wad\"",
    "\"origin\"",
    "\"0 0 8\"",
    "wall",
    "+0lava",
    "*water",
    "sky1",
    "//",
    "\"",
    ";",
    "\\",
];

/// Text of the tokens `data` picks, one byte each; a high byte ends the line.
fn words(data: &[u8]) -> Vec<u8> {
    let mut text = Vec::new();
    for &byte in data {
        text.extend_from_slice(VOCABULARY[usize::from(byte) % VOCABULARY.len()].as_bytes());
        text.push(if byte >= 0xE0 { b'\n' } else { b' ' });
    }
    text
}

fn check(text: &[u8]) {
    if let Ok(map) = MapFile::parse(text) {
        assert_eq!(map.to_bytes(), text);
        let _diagnostics = map.check();
        let _models = map.brush_models();
        let _paths = map.wad_paths(Compiler::EricwTools);
        for entity in &map.entities {
            let _origin = entity.origin();
            for brush in entity.brushes() {
                for face in &brush.faces {
                    let _plane = face.plane();
                    let _vecs = face.texvecs();
                }
            }
        }
    }
    if let Ok(mut map) = qbsp::load_map_file(text) {
        map.number_models();
        let _lump = map.entity_lump();
        let _slots = map.texture_slots(&[b"WALL", b"+1LAVA"]);
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    check(&words(data));
});
