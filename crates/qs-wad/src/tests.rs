// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    reason = "test code: a failed unwrap, index, overflow or panic is a failed test"
)]

use proptest::prelude::*;

use crate::fixtures::{LumpSpec, WadSpec, lump, patch};
use crate::{
    Conchars, Lump, MipTexHeader, QPic, WAD_CMP_LZSS, WAD_CMP_NONE, WAD_CONCHARS_SIZE,
    WAD_CONCHARS_TRANSPARENT, WAD_ENTRY_SIZE, WAD_GFX_NAME, WAD_HEADER_SIZE, WAD_MAGIC,
    WAD_MIPLEVELS, WAD_NAME_SIZE, WAD_TYP_LABEL, WAD_TYP_MIPTEX, WAD_TYP_NONE, WAD_TYP_PALETTE,
    WAD_TYP_QPIC, WAD_TYP_QTEX, WAD_TYP_SOUND, Wad, WadError, w_cleanup_name,
};

fn names(wad: &Wad<'_>) -> Vec<Vec<u8>> {
    wad.lumps()
        .iter()
        .map(|lump| lump.name.as_bytes().to_vec())
        .collect()
}

/// The lumps of the spec's WAD-3 vector.
fn vector() -> Vec<u8> {
    WadSpec {
        lumps: vec![
            LumpSpec {
                lump_type: WAD_TYP_MIPTEX,
                ..lump(b"CONCHARS", b"c")
            },
            lump(b"Disc\0junk", b"d"),
            lump(b"x", b"1"),
            lump(b"x", b"2"),
            lump(&[0xC9, b't', 0xC9], b"e"),
        ],
        ..WadSpec::default()
    }
    .build()
}

// spec WAD-1.1: a 12-byte header of `WAD2`, `numlumps` and `infotableofs`.
#[test]
fn header_and_constants() {
    assert_eq!(WAD_MAGIC, [0x57, 0x41, 0x44, 0x32]);
    assert_eq!(WAD_HEADER_SIZE, 12);
    assert_eq!(WAD_GFX_NAME, "gfx.wad");
    let bytes = vector();
    assert_eq!(&bytes[..4], b"WAD2");
    assert_eq!(Wad::read(&bytes).unwrap().lumps().len(), 5);
}

// spec WAD-1.2: only `WAD2` is accepted; `WAD3`, `2DAW` and lower case fail.
#[test]
fn wrong_id_fails() {
    for magic in [*b"WAD3", *b"2DAW", *b"wad2", *b"PACK", [0; 4]] {
        let bytes = WadSpec {
            magic,
            ..WadSpec::default()
        }
        .build();
        assert_eq!(Wad::read(&bytes), Err(WadError::WrongId));
    }
    assert_eq!(
        format!("Wad file gfx.wad {}", WadError::WrongId),
        "Wad file gfx.wad doesn't have WAD2 id"
    );
}

// spec WAD-1.3: fewer than 12 bytes fails to load.
#[test]
fn short_file_fails() {
    let bytes = WadSpec::default().build();
    for len in 0..WAD_HEADER_SIZE {
        assert_eq!(Wad::read(&bytes[..len]), Err(WadError::WrongId));
    }
}

// spec WAD-2.1: 32-byte entries of filepos, disksize, size, type, compression, pad and name.
#[test]
fn directory_entry_decodes() {
    assert_eq!(WAD_ENTRY_SIZE, 32);
    assert_eq!(WAD_NAME_SIZE, 16);
    let mut bytes = WadSpec {
        lumps: vec![LumpSpec {
            lump_type: 7,
            compression: 9,
            ..lump(b"name", b"abcd")
        }],
        ..WadSpec::default()
    }
    .build();
    let entry = bytes.len() - WAD_ENTRY_SIZE;
    patch(&mut bytes, entry + 8, 1234);
    bytes[entry + 14] = 0xAA;
    bytes[entry + 15] = 0xBB;
    let wad = Wad::read(&bytes).unwrap();
    assert_eq!(
        wad.lumps(),
        [Lump {
            filepos: 12,
            disksize: 4,
            size: 1234,
            lump_type: 7,
            compression: 9,
            name: w_cleanup_name(b"name"),
        }]
    );
}

// spec WAD-2.2: `numlumps` 0 loads with no lumps.
#[test]
fn no_lumps() {
    let bytes = WadSpec::default().build();
    assert!(Wad::read(&bytes).unwrap().lumps().is_empty());
}

// spec WAD-2.3: a negative `numlumps` fails.
#[test]
fn negative_lump_count_fails() {
    let mut bytes = WadSpec::default().build();
    for numlumps in [-1, i32::MIN] {
        patch(&mut bytes, 4, numlumps);
        assert_eq!(
            Wad::read(&bytes),
            Err(WadError::NegativeLumpCount { numlumps })
        );
    }
}

// spec WAD-2.4: a directory outside the file fails.
#[test]
fn directory_outside_the_file() {
    let good = vector();
    let infotableofs = i32::try_from(good.len() - 5 * WAD_ENTRY_SIZE).unwrap();
    for (ofs, count) in [
        (-1, 5),
        (i32::MIN, 0),
        (infotableofs + 1, 5),
        (infotableofs, 6),
        (0, i32::MAX),
    ] {
        let mut bytes = good.clone();
        patch(&mut bytes, 4, count);
        patch(&mut bytes, 8, ofs);
        assert_eq!(
            Wad::read(&bytes),
            Err(WadError::DirectoryOutOfBounds {
                infotableofs: ofs,
                numlumps: count
            }),
            "{ofs} {count}"
        );
    }
}

// spec WAD-2.5: names keep bytes up to the first NUL, lower ASCII A-Z only, pad with NUL.
// spec WAD-2.9: lumps keep directory order, and duplicate names are kept.
#[test]
fn spec_vector_names() {
    let bytes = vector();
    let wad = Wad::read(&bytes).unwrap();
    assert_eq!(
        names(&wad),
        [
            b"conchars".to_vec(),
            b"disc".to_vec(),
            b"x".to_vec(),
            b"x".to_vec(),
            vec![0xC9, b't', 0xC9],
        ]
    );
    assert_eq!(wad.lumps()[1].name, w_cleanup_name(b"disc"));
    assert_eq!(
        w_cleanup_name(b"AZaz@[`{09\xC9\xFF"),
        w_cleanup_name(b"azaz@[`{09\xC9\xFF")
    );
    assert_eq!(w_cleanup_name(b"AB\0CD").as_bytes(), b"ab");
}

// spec WAD-2.6: a 16-byte name with no NUL keeps all 16 bytes.
// spec WAD-3.2: only the first 16 bytes of a requested name count.
#[test]
fn sixteen_byte_names() {
    let bytes = WadSpec {
        lumps: vec![lump(b"ABCDEFGHIJKLMNOP", b"z")],
        ..WadSpec::default()
    }
    .build();
    let wad = Wad::read(&bytes).unwrap();
    assert_eq!(names(&wad), [b"abcdefghijklmnop".to_vec()]);
    assert!(wad.find(b"abcdefghijklmnop").is_ok());
    assert!(wad.find(b"ABCDEFGHIJKLMNOPQRSTUV").is_ok());
    assert!(wad.find(b"abcdefghijklmno").is_err());
}

// spec WAD-2.7: type, compression and disksize are not checked; data is returned as stored.
#[test]
fn type_and_compression_are_not_checked() {
    assert_eq!(
        [
            WAD_TYP_NONE,
            WAD_TYP_LABEL,
            WAD_TYP_PALETTE,
            WAD_TYP_QTEX,
            WAD_TYP_QPIC,
            WAD_TYP_SOUND,
            WAD_TYP_MIPTEX
        ],
        [0, 1, 64, 65, 66, 67, 68]
    );
    assert_eq!([WAD_CMP_NONE, WAD_CMP_LZSS], [0, 1]);
    let bytes = WadSpec {
        lumps: vec![
            LumpSpec {
                lump_type: 0xFF,
                compression: WAD_CMP_LZSS,
                ..lump(b"lz", b"stored")
            },
            LumpSpec {
                lump_type: 200,
                compression: 77,
                ..lump(b"odd", b"")
            },
        ],
        ..WadSpec::default()
    }
    .build();
    let wad = Wad::read(&bytes).unwrap();
    assert_eq!(wad.data(wad.find(b"lz").unwrap()).unwrap(), b"stored");
    assert_eq!(wad.data(wad.find(b"odd").unwrap()).unwrap(), b"");
}

// spec WAD-2.8: bad ranges never stop loading or listing.
// spec WAD-3.4: a lump is its `disksize` bytes at `filepos`; a range outside the file is an
// error.
#[test]
fn bad_lump_range_loads_but_does_not_read() {
    let good = vector();
    let len = i32::try_from(good.len()).unwrap();
    let first = good.len() - 5 * WAD_ENTRY_SIZE;
    for (filepos, disksize) in [
        (-1, 1),
        (0, -1),
        (len, 1),
        (len - 1, 2),
        (i32::MAX, i32::MAX),
    ] {
        let mut bytes = good.clone();
        patch(&mut bytes, first, filepos);
        patch(&mut bytes, first + 4, disksize);
        let wad = Wad::read(&bytes).unwrap();
        assert_eq!(wad.lumps().len(), 5);
        assert_eq!(
            wad.data(&wad.lumps()[0]),
            Err(WadError::LumpOutOfBounds { filepos, disksize })
        );
        assert_eq!(wad.data(&wad.lumps()[1]).unwrap(), b"d");
    }
    let mut bytes = good;
    patch(&mut bytes, first + 8, -5);
    let wad = Wad::read(&bytes).unwrap();
    assert_eq!(wad.data(&wad.lumps()[0]).unwrap(), b"c");
}

// spec WAD-3.1: lookups clean the name and return the first match, ignoring ASCII case only.
#[test]
fn spec_vector_lookups() {
    let bytes = vector();
    let wad = Wad::read(&bytes).unwrap();
    let find = |name: &[u8]| wad.data(wad.find(name).unwrap()).unwrap();
    assert_eq!(find(b"conchars"), b"c");
    assert_eq!(find(b"CoNcHaRs"), b"c");
    assert_eq!(find(b"DISC"), b"d");
    assert_eq!(find(b"x"), b"1");
    assert_eq!(wad.find(b"x").unwrap().filepos, 14);
    assert_eq!(find(&[0xC9, b'T', 0xC9]), b"e");
    assert_eq!(find(&[0xC9, b't', 0xC9]), b"e");
    assert!(wad.find(&[0xE9, b't', 0xE9]).is_err());
}

// spec WAD-3.3: a name that matches no lump is `W_GetLumpinfo: <name> not found`.
#[test]
fn missing_lump_is_not_found() {
    let bytes = WadSpec {
        lumps: vec![lump(b"a", b"")],
        ..WadSpec::default()
    }
    .build();
    let wad = Wad::read(&bytes).unwrap();
    let error = wad.find(b"b").unwrap_err();
    assert_eq!(
        error,
        WadError::NotFound {
            name: b"b".to_vec()
        }
    );
    assert_eq!(error.to_string(), "W_GetLumpinfo: b not found");
    assert!(wad.find(b"").is_err());
}

// spec WAD-3.5: indexes 0 to numlumps - 1 are accepted; numlumps is an error.
#[test]
fn lump_by_index() {
    let bytes = vector();
    let wad = Wad::read(&bytes).unwrap();
    assert_eq!(wad.lump(0).unwrap().name.as_bytes(), b"conchars");
    assert_eq!(wad.lump(4).unwrap().name.as_bytes(), [0xC9, b't', 0xC9]);
    assert_eq!(wad.lump(5), Err(WadError::IndexOutOfRange { index: 5 }));
    assert_eq!(
        WadError::IndexOutOfRange { index: 5 }.to_string(),
        "W_GetLumpNum: bad number: 5"
    );
}

/// A picture lump of `width` by `height` with `pixels`.
fn pic(width: i32, height: i32, pixels: &[u8]) -> Vec<u8> {
    let mut data = width.to_le_bytes().to_vec();
    data.extend_from_slice(&height.to_le_bytes());
    data.extend_from_slice(pixels);
    data
}

// spec WAD-4.1: a picture is width and height as little-endian i32, then width * height bytes.
#[test]
fn picture_lumps() {
    let data = pic(3, 2, b"abcdefTRAIL");
    let decoded = QPic::parse(&data).unwrap();
    assert_eq!((decoded.width, decoded.height), (3, 2));
    assert_eq!(decoded.pixels, b"abcdef");
    assert_eq!(QPic::parse(&pic(0, 5, b"")).unwrap().pixels, b"");
    for (width, height, pixels) in [(3, 2, b"abcde".as_slice()), (-1, 1, b"a"), (1, -1, b"a")] {
        assert_eq!(
            QPic::parse(&pic(width, height, pixels)),
            Err(WadError::BadPic { width, height })
        );
    }
    assert_eq!(
        QPic::parse(&pic(i32::MAX, i32::MAX, b"")),
        Err(WadError::BadPic {
            width: i32::MAX,
            height: i32::MAX
        })
    );
    assert_eq!(
        QPic::parse(b"1234567"),
        Err(WadError::LumpTooShort { len: 7, needed: 8 })
    );
}

// spec WAD-4.2: conchars is 128 x 128 with no header; character n is at row n >> 4, column
// n & 15, and index 0 is transparent.
#[test]
fn conchars_glyphs() {
    assert_eq!(WAD_CONCHARS_SIZE, 128);
    assert_eq!(WAD_CONCHARS_TRANSPARENT, 0);
    let mut data = vec![0_u8; 128 * 128];
    for (i, byte) in data.iter_mut().enumerate() {
        let (y, x) = (i / 128, i % 128);
        // Each pixel holds its character number's low bits mixed with its place in the glyph.
        *byte = u8::try_from(((y / 8) * 16 + x / 8) ^ ((y % 8) * 8 + x % 8)).unwrap_or(0);
    }
    let conchars = Conchars::parse(&data).unwrap();
    for n in [0_u8, 1, 15, 16, 0x41, 0xFF] {
        let glyph = conchars.glyph(n);
        let (row, column) = (usize::from(n >> 4), usize::from(n & 15));
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(
                    glyph[y * 8 + x],
                    data[(row * 8 + y) * 128 + column * 8 + x],
                    "{n} {x} {y}"
                );
            }
        }
    }
    assert_eq!(
        Conchars::parse(&data[..128 * 128 - 1]),
        Err(WadError::LumpTooShort {
            len: 16383,
            needed: 16384
        })
    );
}

// spec WAD-4.3: a texture lump starts with a 16-byte name, width, height and four offsets.
#[test]
fn miptex_header() {
    assert_eq!(WAD_MIPLEVELS, 4);
    let mut data = b"wall\0\0\0\0\0\0\0\0\0\0\0\0".to_vec();
    for word in [64_u32, 32, 40, 2088, 2600, u32::MAX] {
        data.extend_from_slice(&word.to_le_bytes());
    }
    assert_eq!(
        MipTexHeader::parse(&data).unwrap(),
        MipTexHeader {
            name: *b"wall\0\0\0\0\0\0\0\0\0\0\0\0",
            width: 64,
            height: 32,
            offsets: [40, 2088, 2600, u32::MAX],
        }
    );
    assert_eq!(
        MipTexHeader::parse(&data[..39]),
        Err(WadError::LumpTooShort {
            len: 39,
            needed: 40
        })
    );
}

#[test]
fn errors_display() {
    let cases = [
        (WadError::NegativeLumpCount { numlumps: -2 }, "has -2 lumps"),
        (
            WadError::DirectoryOutOfBounds {
                infotableofs: 9,
                numlumps: 3,
            },
            "has a directory of 3 lumps at offset 9, outside the file",
        ),
        (
            WadError::LumpOutOfBounds {
                filepos: 4,
                disksize: 8,
            },
            "lump of 8 bytes at offset 4 lies outside the file",
        ),
        (
            WadError::BadPic {
                width: -1,
                height: 2,
            },
            "picture of -1 by 2 pixels does not fit its lump",
        ),
        (
            WadError::LumpTooShort { len: 1, needed: 8 },
            "lump of 1 bytes is shorter than its 8-byte layout",
        ),
        (
            WadError::NotFound {
                name: vec![0xC9, b'a'],
            },
            "W_GetLumpinfo: \\xc9a not found",
        ),
    ];
    for (error, message) in cases {
        assert_eq!(error.to_string(), message);
    }
}

proptest! {
    // spec WAD-1.2: arbitrary bytes never panic the reader or its lookups.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        if let Ok(wad) = Wad::read(&bytes) {
            for lump in wad.lumps() {
                if let Ok(data) = wad.data(lump) {
                    if let Ok(pic) = QPic::parse(data) {
                        prop_assert_eq!(pic.pixels.len(), pic.width * pic.height);
                    }
                    prop_assert_eq!(MipTexHeader::parse(data).is_ok(), data.len() >= 40);
                }
                prop_assert!(wad.find(lump.name.as_bytes()).is_ok());
            }
        }
    }

    // spec WAD-2.4: a header followed by arbitrary bytes never panics the reader.
    #[test]
    fn arbitrary_after_magic_never_panics(
        rest in prop::collection::vec(any::<u8>(), 0..512),
        numlumps in prop_oneof![-4_i32..20, Just(i32::MAX), Just(i32::MIN)],
        infotableofs in prop_oneof![-16_i32..600, Just(i32::MAX)],
    ) {
        let mut bytes = WAD_MAGIC.to_vec();
        bytes.extend_from_slice(&numlumps.to_le_bytes());
        bytes.extend_from_slice(&infotableofs.to_le_bytes());
        bytes.extend_from_slice(&rest);
        if let Ok(wad) = Wad::read(&bytes) {
            for lump in wad.lumps() {
                if let Ok(data) = wad.data(lump) {
                    prop_assert_eq!(Ok(data.len()), usize::try_from(lump.disksize));
                }
            }
        }
    }

    // spec WAD-3.1: every built lump is found by its own name, any case, and reads back.
    #[test]
    fn built_files_read_back(
        lumps in prop::collection::vec(
            (prop::collection::vec(b'a'..=b'z', 1..=16), prop::collection::vec(any::<u8>(), 0..32)),
            0..16,
        ),
    ) {
        let spec = WadSpec {
            lumps: lumps.iter().map(|(name, data)| lump(name, data)).collect(),
            ..WadSpec::default()
        };
        let bytes = spec.build();
        let wad = Wad::read(&bytes).unwrap();
        prop_assert_eq!(wad.lumps().len(), lumps.len());
        for (name, _) in &lumps {
            let first = lumps.iter().find(|(other, _)| other == name).unwrap();
            let upper = name.to_ascii_uppercase();
            prop_assert_eq!(wad.data(wad.find(&upper).unwrap()).unwrap(), first.1.as_slice());
        }
    }

    // spec WAD-2.5: cleaning is idempotent and never longer than 16 bytes.
    #[test]
    fn cleanup_is_idempotent(name in prop::collection::vec(any::<u8>(), 0..40)) {
        let clean = w_cleanup_name(&name);
        prop_assert!(clean.as_bytes().len() <= WAD_NAME_SIZE);
        prop_assert_eq!(w_cleanup_name(clean.as_bytes()), clean);
    }
}
