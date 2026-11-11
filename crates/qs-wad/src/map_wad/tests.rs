// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    reason = "test code: a failed unwrap or index is a failed test; exact values are the point"
)]

use super::{MapWad, MapWadError, WAD_TYP_MIPTEX_PALETTE, WAD3_MAGIC, WadKind};
use crate::WAD_TYP_MIPTEX;
use crate::fixtures::{LumpSpec, WadSpec, patch};

/// A texture lump: its 40-byte header named `name`, `width` by `height`, four mip levels of
/// `pixel`, then `trailer` (a palette).
fn miptex(name: &[u8], width: u32, height: u32, pixel: u8, trailer: &[u8], ty: u8) -> LumpSpec {
    let mut data = vec![0_u8; 16];
    data[..name.len()].copy_from_slice(name);
    data.extend_from_slice(&width.to_le_bytes());
    data.extend_from_slice(&height.to_le_bytes());
    let area = width * height;
    let mut offset: u32 = 40;
    for level in 0..4 {
        data.extend_from_slice(&offset.to_le_bytes());
        offset += area >> (2 * level);
    }
    data.extend(core::iter::repeat_n(pixel, (area / 64 * 85) as usize));
    data.extend_from_slice(trailer);
    LumpSpec {
        name: name.to_vec(),
        lump_type: ty,
        compression: 0,
        data,
    }
}

/// A full 256-colour palette: the count, then colour n as (n, 255 - n, n / 2).
fn full_palette() -> Vec<u8> {
    let mut out = 256_u16.to_le_bytes().to_vec();
    for n in 0..=255_u8 {
        out.extend_from_slice(&[n, 255 - n, n / 2]);
    }
    out
}

/// The vector `wad3-full`: one 16 by 16 texture `brick` of index 9 with a full palette.
fn wad3_full() -> Vec<u8> {
    WadSpec {
        magic: WAD3_MAGIC,
        lumps: vec![miptex(
            b"BRICK",
            16,
            16,
            9,
            &full_palette(),
            WAD_TYP_MIPTEX_PALETTE,
        )],
    }
    .build()
}

// spec WAD3-1.1: W_AddWadFile takes a file that starts WAD2 or WAD3 and no other; a file shorter
// than its header is not valid
// spec WAD3-1.3: id's gfx.wad loader takes WAD2 only (WAD-1.2), so a WAD3 is not a gfx.wad
#[test]
fn a_map_wad_is_wad2_or_wad3() {
    let wad3 = wad3_full();
    assert_eq!(MapWad::read(&wad3).unwrap().kind(), WadKind::Wad3);
    let mut wad2 = wad3.clone();
    wad2[..4].copy_from_slice(b"WAD2");
    assert_eq!(MapWad::read(&wad2).unwrap().kind(), WadKind::Wad2);
    for magic in [*b"WAD4", *b"wad3", *b"PACK"] {
        let mut bad = wad3.clone();
        bad[..4].copy_from_slice(&magic);
        assert_eq!(MapWad::read(&bad), Err(MapWadError::NotValid), "{magic:?}");
    }
    assert_eq!(MapWad::read(&wad3[..11]), Err(MapWadError::NotValid));
    assert!(crate::Wad::read(&wad3).is_err(), "gfx.wad must be WAD2");
}

// spec WAD3-1.2: a negative lump count or directory offset is not valid; no lumps is ignored
#[test]
fn a_bad_directory_is_not_used() {
    let mut bytes = wad3_full();
    patch(&mut bytes, 4, -1);
    assert_eq!(
        MapWad::read(&bytes),
        Err(MapWadError::BadDirectory {
            numlumps: -1,
            infotableofs: i32::from_le_bytes(bytes[8..12].try_into().unwrap())
        })
    );
    let mut bytes = wad3_full();
    patch(&mut bytes, 8, -4);
    assert!(matches!(
        MapWad::read(&bytes),
        Err(MapWadError::BadDirectory {
            infotableofs: -4,
            ..
        })
    ));
    let empty = WadSpec {
        magic: WAD3_MAGIC,
        lumps: Vec::new(),
    }
    .build();
    assert_eq!(MapWad::read(&empty), Err(MapWadError::NoLumps));
}

// spec WAD3-2.1: the directory is numlumps 32-byte entries at infotableofs
// spec WAD3-2.2: names are cleaned as W_CleanupName does: lowered, NUL-padded
// spec WAD3-3.1: a texture is found by its cleaned name, the first entry that has it
#[test]
fn the_directory_is_read_and_names_cleaned() {
    let bytes = WadSpec {
        magic: WAD3_MAGIC,
        lumps: vec![
            miptex(
                b"Brick",
                16,
                16,
                1,
                &[1, 0, 1, 2, 3],
                WAD_TYP_MIPTEX_PALETTE,
            ),
            miptex(b"BRICK", 16, 16, 2, &[0, 0], WAD_TYP_MIPTEX_PALETTE),
        ],
    }
    .build();
    let wad = MapWad::read(&bytes).unwrap();
    assert_eq!(wad.entries().len(), 2);
    assert_eq!(wad.entries()[0].name.as_bytes(), b"brick");
    assert_eq!(wad.entries()[0].filepos, 12);
    assert_eq!(wad.entries()[0].ty, WAD_TYP_MIPTEX_PALETTE);
    let texture = wad.texture(b"bRiCk").unwrap();
    assert_eq!(texture.pixels[0], 1, "the first entry of the name");
    assert!(wad.texture(b"stone").is_none());
}

// spec WAD3-2.4: only the directory entries the file holds whole are entries
#[test]
fn a_truncated_directory_keeps_its_whole_entries() {
    let mut bytes = WadSpec {
        magic: WAD3_MAGIC,
        lumps: vec![
            miptex(b"a", 16, 16, 1, &[0, 0], WAD_TYP_MIPTEX_PALETTE),
            miptex(b"b", 16, 16, 2, &[0, 0], WAD_TYP_MIPTEX_PALETTE),
        ],
    }
    .build();
    bytes.truncate(bytes.len() - 20);
    let wad = MapWad::read(&bytes).unwrap();
    assert_eq!(wad.entries().len(), 1);
    assert!(wad.texture(b"a").is_some());
    assert!(wad.texture(b"b").is_none());
}

// spec WAD3-2.3: a lump past the file takes its disksize when that fits, else is cut: one that
// starts past the file starts at 0, one that runs past it loses its offset from its size
#[test]
fn lumps_past_the_file_are_fixed_as_w_add_wad_file_does() {
    let base = wad3_full();
    let entry = base.len() - 32;
    let length = i64::try_from(base.len()).unwrap();
    // size past the file, disksize fitting: disksize.
    let mut bytes = base.clone();
    patch(&mut bytes, entry + 8, 100_000);
    patch(&mut bytes, entry + 4, 50);
    assert_eq!(MapWad::read(&bytes).unwrap().entries()[0].size, 50);
    // starts past the file: offset 0, size kept.
    let mut bytes = base.clone();
    patch(&mut bytes, entry, 100_000);
    let fixed = MapWad::read(&bytes).unwrap().entries()[0];
    assert_eq!((fixed.filepos, fixed.size), (0, 40 + 340 + 770));
    // runs past the file: size less the offset.
    let mut bytes = base.clone();
    patch(&mut bytes, entry + 4, 100_000);
    patch(&mut bytes, entry + 8, 100_000);
    let fixed = MapWad::read(&bytes).unwrap().entries()[0];
    assert_eq!((fixed.filepos, fixed.size), (12, 100_000 - 12));
    assert!(fixed.filepos + fixed.size > length);
    // a negative size: offset 0, size 0.
    let mut bytes = base;
    patch(&mut bytes, entry + 8, -5);
    patch(&mut bytes, entry + 4, -5);
    let fixed = MapWad::read(&bytes).unwrap().entries()[0];
    assert_eq!((fixed.filepos, fixed.size), (0, 0));
}

// spec WAD3-3.2: a texture is a miptex lump, or in a WAD3 a palette miptex; a size of 0 or one
// that is not a multiple of 16 is none
#[test]
fn only_aligned_miptex_lumps_are_textures() {
    let build = |magic, lump| {
        WadSpec {
            magic,
            lumps: vec![lump],
        }
        .build()
    };
    let plain = build(*b"WAD2", miptex(b"t", 16, 16, 4, &[], WAD_TYP_MIPTEX));
    let texture = MapWad::read(&plain).unwrap().texture(b"t").unwrap();
    assert_eq!(
        (texture.width, texture.pixels.len(), texture.palette),
        (16, 256, None)
    );
    let pal_in_wad2 = build(
        *b"WAD2",
        miptex(b"t", 16, 16, 4, &[0, 0], WAD_TYP_MIPTEX_PALETTE),
    );
    assert!(MapWad::read(&pal_in_wad2).unwrap().texture(b"t").is_none());
    let miptex_in_wad3 = build(WAD3_MAGIC, miptex(b"t", 16, 16, 4, &[], WAD_TYP_MIPTEX));
    let texture = MapWad::read(&miptex_in_wad3)
        .unwrap()
        .texture(b"t")
        .unwrap();
    assert_eq!(
        texture.palette, None,
        "a plain miptex has no palette in a WAD3 either"
    );
    let pic = build(
        WAD3_MAGIC,
        LumpSpec {
            lump_type: 66,
            ..miptex(b"t", 16, 16, 4, &[], 66)
        },
    );
    assert!(MapWad::read(&pic).unwrap().texture(b"t").is_none());
    for (width, height) in [(0, 16), (16, 0), (24, 16), (16, 8)] {
        let odd = build(
            WAD3_MAGIC,
            miptex(b"t", width, height, 4, &[0, 0], WAD_TYP_MIPTEX_PALETTE),
        );
        assert!(
            MapWad::read(&odd).unwrap().texture(b"t").is_none(),
            "{width} by {height}"
        );
    }
}

// spec WAD3-3.3: a palette texture's palette follows its four mip levels: the count and that
// many RGB triples (`wad3-full`, `wad3-few`)
// spec WAD3-3.6: the texture's name and size are its own header's
#[test]
fn a_texture_keeps_its_palette() {
    let bytes = wad3_full();
    let texture = MapWad::read(&bytes).unwrap().texture(b"brick").unwrap();
    assert_eq!(texture.name, b"BRICK");
    assert_eq!((texture.width, texture.height), (16, 16));
    assert_eq!(texture.pixels, &[9; 256][..]);
    assert_eq!(texture.palette, Some(&full_palette()[..]));
    let few = WadSpec {
        magic: WAD3_MAGIC,
        lumps: vec![miptex(
            b"few",
            16,
            16,
            3,
            &[2, 0, 1, 2, 3, 4, 5, 6, 7, 7],
            WAD_TYP_MIPTEX_PALETTE,
        )],
    }
    .build();
    let texture = MapWad::read(&few).unwrap().texture(b"few").unwrap();
    assert_eq!(
        texture.palette,
        Some(&[2, 0, 1, 2, 3, 4, 5, 6][..]),
        "two colours"
    );
}

// spec WAD3-3.4: a palette past the lump is cut against the lump's size, counted from the lump's
// start (`wad3-past`); a count that lies past the file is 0 colours (WAD3-3.3)
#[test]
fn a_palette_past_the_lump_is_cut() {
    // 256 colours named, 6 bytes of them in the lump.
    let past = WadSpec {
        magic: WAD3_MAGIC,
        lumps: vec![miptex(
            b"past",
            16,
            16,
            5,
            &[0, 1, 1, 2, 3, 4, 5, 6],
            WAD_TYP_MIPTEX_PALETTE,
        )],
    }
    .build();
    let wad = MapWad::read(&past).unwrap();
    let texture = wad.texture(b"past").unwrap();
    // The lump is 40 + 340 + 8 bytes, so the palette is cut to its size less the mip levels,
    // 388 - 340 = 48 bytes, 40 past the lump's end: they are read on into the directory, as
    // FS_fread does, and stop at the file's end, 32 bytes on.
    let size = wad.entries()[0].size;
    assert_eq!(size, 388);
    assert_eq!(past.len(), 12 + 388 + 32);
    let palette = texture.palette.unwrap();
    assert_eq!(palette.len(), 40);
    assert_eq!(&palette[..8], &[0, 1, 1, 2, 3, 4, 5, 6]);
    assert_eq!(&palette[8..], &past[400..432]);
}

// spec WAD3-3.5: a lump that starts before the file, or whose 40-byte header the file does not
// hold whole, is no texture
#[test]
fn an_unreadable_lump_is_no_texture() {
    let base = wad3_full();
    let entry = base.len() - 32;
    // An offset of -8 with a size of 2000: WAD3-2.3 keeps the offset and grows the size.
    let mut bytes = base.clone();
    patch(&mut bytes, entry, -8);
    patch(&mut bytes, entry + 4, 2000);
    patch(&mut bytes, entry + 8, 2000);
    let wad = MapWad::read(&bytes).unwrap();
    assert_eq!(wad.entries()[0].filepos, -8);
    assert!(wad.texture(b"brick").is_none());
    // A header that runs past the file.
    let mut bytes = base;
    let past = i32::try_from(bytes.len() - 20).unwrap();
    patch(&mut bytes, entry, past);
    patch(&mut bytes, entry + 4, 10);
    patch(&mut bytes, entry + 8, 10);
    let wad = MapWad::read(&bytes).unwrap();
    assert!(wad.texture(b"brick").is_none());
}
