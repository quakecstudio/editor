// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds arbitrary bytes to the BSP reader (BSP-1 to BSP-17), the `.lit` reader (BSP-18) and the
//! entity lump writer (BSP-19), each as given and written over the spec's `base` vector, so the
//! fuzzer reaches every lump. Fails on a panic, on a face whose samples do not lie in the lighting
//! lump, on a visibility row wider than `BSP_MAX_VIS_ROW` or not the width model 0's `visleafs`
//! gives (BSP-6.7), on a Half-Life texture's palette past its lump or longer than its count
//! (BSP-22.2), and on a BSP whose new entity lump does not read back. Each input is also tried as
//! version 30, Half-Life's (BSP-22).

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
    // BSP-22.2: a palette lies in the textures lump and is no longer than its count says.
    let lump = &bsp.lumps[2];
    let start = usize::try_from(lump.fileofs).unwrap_or(0);
    let end = start.saturating_add(usize::try_from(lump.filelen).unwrap_or(0));
    let textures = bytes.get(start..end).unwrap_or_default().as_ptr_range();
    for palette in bsp.textures.iter().flatten().filter_map(|texture| texture.palette) {
        let at = palette.as_ptr_range();
        assert!(palette.is_empty() || (textures.start <= at.start && at.end <= textures.end));
        if let [low, high, colours @ ..] = palette {
            assert!(colours.len() <= 3 * usize::from(u16::from_le_bytes([*low, *high])));
        }
    }
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
    // The same as Half-Life's version 30, read with BSP29's layout (BSP-22.1); only the
    // FitzQuake lineage's loader reads it.
    bytes[..4].copy_from_slice(&30_i32.to_le_bytes());
    check_with(&bytes, BspLoader::FitzQuake);
});
