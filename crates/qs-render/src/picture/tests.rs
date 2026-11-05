// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test; the pictures are small"
)]

use super::{load_image, read_pcx, read_tga};
use crate::mesh::Image;
use crate::mesh::tests::palette;

/// A TGA test picture.
pub(crate) struct TgaSpec {
    pub(crate) width: u16,
    pub(crate) height: u16,
    /// Its pixels, rows from the top, four bytes each.
    pub(crate) rgba: Vec<u8>,
    /// Whether the file stores its rows from the top (attribute bit 0x20), else the bottom.
    pub(crate) top_down: bool,
}

impl Default for TgaSpec {
    fn default() -> Self {
        Self {
            width: 2,
            height: 2,
            rgba: [
                [1, 2, 3, 255],
                [4, 5, 6, 255],
                [7, 8, 9, 255],
                [10, 11, 12, 255],
            ]
            .concat(),
            top_down: false,
        }
    }
}

/// An uncompressed 32-bit true-colour TGA (image type 2).
pub(crate) fn tga(spec: &TgaSpec) -> Vec<u8> {
    let mut out = vec![0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    out.extend(spec.width.to_le_bytes());
    out.extend(spec.height.to_le_bytes());
    out.push(32);
    out.push(if spec.top_down { 0x28 } else { 0x08 });
    let row = usize::from(spec.width) * 4;
    let mut rows: Vec<&[u8]> = spec.rgba.chunks(row).collect();
    if !spec.top_down {
        rows.reverse();
    }
    for pixel in rows.concat().chunks(4) {
        out.extend([pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    out
}

/// A one-colour TGA of `size`.
pub(crate) fn solid(size: [u16; 2], colour: [u8; 4]) -> Vec<u8> {
    let [width, height] = size;
    tga(&TgaSpec {
        width,
        height,
        rgba: colour.repeat(usize::from(width) * usize::from(height)),
        top_down: true,
    })
}

/// A PCX test picture.
pub(crate) struct PcxSpec {
    version: u8,
    /// `xmax - xmin + 1` and `ymax - ymin + 1`, with the minimums at 3 and 5.
    size: [u16; 2],
    bytes_per_line: u16,
    /// The run-length data after the header.
    data: Vec<u8>,
}

impl Default for PcxSpec {
    fn default() -> Self {
        Self {
            version: 5,
            size: [3, 2],
            bytes_per_line: 4,
            // Row 0: a run of 3 of index 2, then the padding byte; row 1: 1, 0xc5 (a literal
            // run of one), 3, then the padding.
            data: vec![0xc3, 2, 9, 1, 0xc1, 0xc5, 3, 9],
        }
    }
}

/// A PCX of `spec` whose palette entry `i` is `(i, 2i, 255 - i)`.
pub(crate) fn pcx(spec: &PcxSpec) -> Vec<u8> {
    let mut header = vec![0_u8; 128];
    header[0] = 0x0a;
    header[1] = spec.version;
    header[2] = 1;
    header[3] = 8;
    let [w, h] = spec.size;
    for (at, value) in [(4, 3), (6, 5), (8, 3 + w - 1), (10, 5 + h - 1)] {
        header[at..at + 2].copy_from_slice(&u16::to_le_bytes(value));
    }
    header[65] = 1;
    header[66..68].copy_from_slice(&spec.bytes_per_line.to_le_bytes());
    let mut out = header;
    out.extend(&spec.data);
    out.push(0x0c);
    out.extend((0..=255_u8).flat_map(|i| [i, i.wrapping_mul(2), 255 - i]));
    out
}

pub(crate) fn colour(i: u8) -> [u8; 4] {
    [i, i.wrapping_mul(2), 255 - i, 255]
}

/// A reader of `files`, each a path and its bytes.
pub(crate) fn files(files: Vec<(String, Vec<u8>)>) -> impl Fn(&str) -> Option<Vec<u8>> {
    move |name| {
        files
            .iter()
            .find(|(path, _)| path == name)
            .map(|(_, bytes)| bytes.clone())
    }
}

// spec REND-8.2: a TGA's rows are read from the top whichever way the file stores them
#[test]
fn tga_rows_come_from_the_top() {
    let spec = TgaSpec::default();
    for top_down in [false, true] {
        let image = read_tga(&tga(&TgaSpec {
            top_down,
            ..TgaSpec::default()
        }))
        .unwrap();
        assert_eq!(
            image,
            Image {
                width: 2,
                height: 2,
                rgba: spec.rgba.clone(),
            }
        );
    }
    assert!(read_tga(b"").is_none());
}

// spec REND-8.2: a PCX's runs decode through the palette at its end; each row keeps its
// first `xmax - xmin + 1` of `bytes_per_line` pixels
#[test]
fn pcx_runs_decode_through_its_palette() {
    let image = read_pcx(&pcx(&PcxSpec::default())).unwrap();
    assert_eq!((image.width, image.height), (3, 2));
    let expected = [
        colour(2),
        colour(2),
        colour(2),
        colour(1),
        colour(0xc5),
        colour(3),
    ];
    assert_eq!(image.rgba, expected.concat());
}

// spec REND-8.2: a run past the line's end is dropped with the padding, and a line shorter
// than the picture is refused
#[test]
fn pcx_lines_are_cut_and_padded_to_the_picture() {
    let long = read_pcx(&pcx(&PcxSpec {
        size: [2, 1],
        bytes_per_line: 2,
        data: vec![0xc5, 4],
        ..PcxSpec::default()
    }))
    .unwrap();
    assert_eq!(long.rgba, [colour(4), colour(4)].concat());
    let short = read_pcx(&pcx(&PcxSpec {
        size: [3, 1],
        bytes_per_line: 2,
        data: vec![6, 6],
        ..PcxSpec::default()
    }));
    assert_eq!(short, None, "a line shorter than the picture");
    let empty = read_pcx(&pcx(&PcxSpec {
        size: [65_530, 65_530],
        bytes_per_line: 0,
        data: Vec::new(),
        ..PcxSpec::default()
    }));
    assert_eq!(
        empty, None,
        "no line makes no 65530 by 65530 picture from no data"
    );
}

// spec REND-8.2: a PCX that is not 8-bit, one-plane, run-length version 5, or that ends
// before its pixels, is not read
#[test]
fn other_pcx_files_are_refused() {
    assert!(
        read_pcx(&pcx(&PcxSpec {
            version: 3,
            ..PcxSpec::default()
        }))
        .is_none()
    );
    for (at, value) in [(0, 0x0b), (2, 0), (3, 4), (65, 3)] {
        let mut bytes = pcx(&PcxSpec::default());
        bytes[at] = value;
        assert!(read_pcx(&bytes).is_none(), "byte {at}");
    }
    let mut short = pcx(&PcxSpec::default());
    short.truncate(128 + 4);
    assert!(
        read_pcx(&short).is_none(),
        "the palette overlaps the pixels"
    );
    assert!(read_pcx(&[0x0a; 100]).is_none(), "no header");
    let mut inverted = pcx(&PcxSpec::default());
    inverted[8..10].copy_from_slice(&1_u16.to_le_bytes());
    assert!(read_pcx(&inverted).is_none(), "xmax before xmin");
}

/// A 1 by 1 picture lump of palette index `index`.
fn lmp(index: u8) -> Vec<u8> {
    let mut out = 1_i32.to_le_bytes().to_vec();
    out.extend(1_i32.to_le_bytes());
    out.push(index);
    out
}

// spec REND-12.4: a skin is found as a TGA, then a PNG, a JPEG and a PCX, as it is, then
// under `textures/`, then its last part under `textures/`, and at last as an LMP through
// the palette; the first file found decides, though it does not read
#[test]
fn images_are_found_in_quakespasm_spikeds_order() {
    let red = |shade: u8| solid([1, 1], [shade, 0, 0, 255]);
    let first = |paths: Vec<(&str, Vec<u8>)>| {
        let read = files(paths.into_iter().map(|(p, b)| (p.to_owned(), b)).collect());
        load_image("progs/ogre_00_00", &read, &palette()).map(|image| image.rgba)
    };
    assert_eq!(first(vec![]), None);
    assert_eq!(
        first(vec![
            ("progs/ogre_00_00.pcx", pcx(&PcxSpec::default())),
            ("progs/ogre_00_00.tga", red(1)),
        ]),
        Some(vec![1, 0, 0, 255])
    );
    assert_eq!(
        first(vec![
            ("textures/progs/ogre_00_00.tga", red(2)),
            ("progs/ogre_00_00.pcx", pcx(&PcxSpec::default())),
        ])
        .unwrap()[..4],
        colour(2),
        "every extension as it is before textures/"
    );
    assert_eq!(
        first(vec![
            ("textures/ogre_00_00.tga", red(3)),
            ("textures/progs/ogre_00_00.tga", red(4)),
        ]),
        Some(vec![4, 0, 0, 255])
    );
    assert_eq!(
        first(vec![("textures/ogre_00_00.tga", red(3))]),
        Some(vec![3, 0, 0, 255])
    );
    assert_eq!(
        first(vec![
            ("progs/ogre_00_00.tga", b"bad".to_vec()),
            ("progs/ogre_00_00.png", red(6)),
            ("progs/ogre_00_00.lmp", lmp(5)),
        ]),
        None,
        "the first file found decides, though it does not read"
    );
    assert_eq!(
        first(vec![("progs/ogre_00_00.lmp", lmp(5))]),
        Some(palette().rgba(5).to_vec()),
        "the LMP when no other file is found"
    );
}
