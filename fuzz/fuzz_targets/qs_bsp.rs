// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the BSP reader (BSP-1 to BSP-17), the `.lit` reader (BSP-18) and
//! the entity lump writer (BSP-19), each as given and written over the spec's `base` vector, so
//! the fuzzer reaches every lump. Fails on a panic, on a face whose samples do not lie in the
//! lighting lump, on a visibility row wider than `BSP_MAX_VIS_ROW` or not the width model 0's
//! `visleafs` gives (BSP-6.7), and on a BSP whose new entity lump does not read back.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qs_bsp::{BSP_MAX_VIS_ROW, Bsp, BspLoader, Lit, replace_entities};

/// The `base` vector of `docs/spec/vectors/bsp/`, an `xxd` dump.
const BASE: &str = include_str!("../../docs/spec/vectors/bsp/base.txt");

fn base() -> Vec<u8> {
    let mut out = Vec::new();
    for line in BASE.lines() {
        let Some((_, rest)) = line.split_once(": ") else {
            continue;
        };
        for group in rest.get(..39).unwrap_or(rest).split_whitespace() {
            for pair in group.as_bytes().chunks(2) {
                let pair = core::str::from_utf8(pair).unwrap();
                out.push(u8::from_str_radix(pair, 16).unwrap());
            }
        }
    }
    out
}

fn check(bytes: &[u8]) {
    let _lit = Lit::read(bytes, bytes.len() / 3);
    for loader in [BspLoader::WinQuake, BspLoader::GlQuake, BspLoader::FitzQuake] {
        check_with(bytes, loader);
    }
}

fn check_with(bytes: &[u8], loader: BspLoader) {
    let Ok(bsp) = Bsp::read(bytes, loader) else {
        return;
    };
    for face in &bsp.faces {
        let _vertices = bsp.face_vertices(face);
        if face.lightofs >= 0 {
            assert!(bsp.face_samples(face).is_some());
        }
    }
    // A row is model 0's `visleafs` bits, which the file may make as wide as BSP_MAX_VIS_ROW
    // (BSP-6.2, BSP-6.7): many leaves of a narrow row are checked with every loader, and one
    // leaf of a wide row with one loader, since the width does not depend on the loader.
    let row = bsp.vis_row_size();
    assert!(row <= BSP_MAX_VIS_ROW);
    let leaves = match (row <= 1 << 16, loader) {
        (true, _) => 256,
        (false, BspLoader::FitzQuake) => 1,
        (false, BspLoader::WinQuake | BspLoader::GlQuake) => 0,
    };
    for leaf in 0..bsp.leafs.len().min(leaves) {
        if let Ok(pvs) = bsp.leaf_pvs(leaf) {
            assert_eq!(pvs.len(), row);
        }
    }
    let _hull = bsp.hull0();
    let edited =
        replace_entities(bytes, b"{\n\"classname\" \"worldspawn\"\n}\n", loader).unwrap();
    assert_eq!(
        Bsp::read(&edited, loader).unwrap().entities(),
        b"{\n\"classname\" \"worldspawn\"\n}\n"
    );
}

fuzz_target!(|data: &[u8]| {
    check(data);
    // The first two bytes pick where the rest is written over the base.
    let mut bytes = base();
    if let [high, low, rest @ ..] = data {
        let at = usize::from(u16::from_le_bytes([*low, *high])) % bytes.len();
        for (offset, byte) in rest.iter().enumerate() {
            let index = (at + offset) % bytes.len();
            bytes[index] = *byte;
        }
    }
    check(&bytes);
});
