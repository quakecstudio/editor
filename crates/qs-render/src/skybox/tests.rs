// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test; the pictures are small"
)]

use super::{SKY_SIDES, SkyKeys, Skybox, read_pcx, read_tga, sky_parse_worldspawn};
use crate::mesh::Image;

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
struct PcxSpec {
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
fn pcx(spec: &PcxSpec) -> Vec<u8> {
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

fn colour(i: u8) -> [u8; 4] {
    [i, i.wrapping_mul(2), 255 - i, 255]
}

// spec REND-8.2: `sky`, `skyname` and `qlsky` each name the skybox, the last one read
// winning; a key's leading `_` and trailing spaces are dropped; only the first entity counts
#[test]
fn the_worldspawn_names_the_skybox() {
    let name = |text: &str| sky_parse_worldspawn(text.as_bytes()).name;
    assert_eq!(name(r#"{ "classname" "worldspawn" }"#), "");
    assert_eq!(name(r#"{ "sky" "mountain" }"#), "mountain");
    assert_eq!(name(r#"{ "_sky" "a" }"#), "a");
    assert_eq!(name(r#"{ "sky  " "b" }"#), "b");
    assert_eq!(name(r#"{ "skyname" "hl" }"#), "hl");
    assert_eq!(name(r#"{ "qlsky" "ql" }"#), "ql");
    assert_eq!(name(r#"{ "sky" "first" "skyname" "second" }"#), "second");
    assert_eq!(
        name(r#"{ "sky" "x" "sky" "" }"#),
        "",
        "an empty name turns it off"
    );
    assert_eq!(
        name(r#"{ "classname" "worldspawn" } { "sky" "later" }"#),
        ""
    );
    assert_eq!(name(r#"{ "skies" "no" }"#), "");
}

// spec REND-8.1: `skyfog` is read as `atof` reads it, 0.5 when absent
#[test]
fn the_worldspawn_sets_the_skys_fog() {
    let skyfog = |text: &str| sky_parse_worldspawn(text.as_bytes()).skyfog;
    assert_eq!(SkyKeys::default().skyfog, 0.5);
    assert_eq!(skyfog("{ }"), 0.5);
    assert_eq!(skyfog(r#"{ "skyfog" "0.25" }"#), 0.25);
    assert_eq!(skyfog(r#"{ "_skyfog" "1" }"#), 1.0);
    assert_eq!(skyfog(r#"{ "skyfog" "  0.75 and more" }"#), 0.75);
    assert_eq!(skyfog(r#"{ "skyfog" "none" }"#), 0.0);
    assert_eq!(skyfog(r#"{ "skyfog" "" }"#), 0.0);
}

/// A reader of `files`, each a path and its bytes.
fn files(files: Vec<(String, Vec<u8>)>) -> impl Fn(&str) -> Option<Vec<u8>> {
    move |name| {
        files
            .iter()
            .find(|(path, _)| path == name)
            .map(|(_, bytes)| bytes.clone())
    }
}

// spec REND-8.2: the six faces are `gfx/env/<name><side>` in `suf`'s order, a TGA before a
// PCX; a face not read is black, and one of another size is taken at its nearest texels
#[test]
fn a_skybox_reads_its_six_faces() {
    assert_eq!(SKY_SIDES, ["rt", "bk", "lf", "ft", "up", "dn"]);
    let red = |shade: u8| solid([2, 2], [shade, 0, 0, 255]);
    let paths = vec![
        ("gfx/env/boxrt.pcx".to_owned(), pcx(&PcxSpec::default())),
        ("gfx/env/boxbk.tga".to_owned(), red(40)),
        ("gfx/env/boxbk.pcx".to_owned(), pcx(&PcxSpec::default())),
        ("gfx/env/boxlf.tga".to_owned(), red(80)),
        ("gfx/env/boxft.tga".to_owned(), red(120)),
        ("gfx/env/boxup.tga".to_owned(), b"not a picture".to_vec()),
        (
            "gfx/env/boxdn.tga".to_owned(),
            solid([4, 4], [9, 9, 9, 255]),
        ),
    ];
    let read = files(paths);
    let skybox = Skybox::load("box", &read).unwrap();
    // The first face read is rt's PCX, 3 by 2.
    assert_eq!(skybox.size(), [3, 2]);
    let layers = skybox.layers();
    assert_eq!(layers.len(), 6 * 3 * 2 * 4);
    let face = |i: usize| &layers[i * 24..(i + 1) * 24];
    assert_eq!(&face(0)[..4], colour(2));
    assert_eq!(&face(1)[..4], [40, 0, 0, 255], "bk's TGA before its PCX");
    assert_eq!(&face(2)[..4], [80, 0, 0, 255]);
    assert_eq!(&face(3)[..4], [120, 0, 0, 255]);
    assert_eq!(&face(4)[..4], [0, 0, 0, 255], "up is not a picture");
    assert_eq!(
        &face(5)[..4],
        [9, 9, 9, 255],
        "dn's 4 by 4 at its nearest texels"
    );
}

// spec REND-8.2: an empty name or a box with no face read keeps the scrolling layers
#[test]
fn a_skybox_with_no_faces_is_none() {
    let read = files(vec![(
        "gfx/env/xrt.tga".to_owned(),
        solid([1, 1], [1, 1, 1, 1]),
    )]);
    assert!(Skybox::load("", &read).is_none());
    assert!(Skybox::load("missing", &read).is_none());
    assert!(Skybox::load("x", &read).is_some());
}

// spec REND-8.2: a face of another size is taken at its nearest texels
#[test]
fn a_larger_face_is_taken_at_its_nearest_texels() {
    let mut big = Vec::new();
    for y in 0..4_u8 {
        for x in 0..4_u8 {
            big.extend([x, y, 0, 255]);
        }
    }
    let read = files(vec![
        ("gfx/env/srt.tga".to_owned(), solid([2, 2], [0, 0, 0, 255])),
        (
            "gfx/env/sbk.tga".to_owned(),
            tga(&TgaSpec {
                width: 4,
                height: 4,
                rgba: big,
                top_down: true,
            }),
        ),
    ]);
    let layers = Skybox::load("s", &read).unwrap().layers();
    let bk: Vec<[u8; 2]> = layers[16..32].chunks(4).map(|p| [p[0], p[1]]).collect();
    assert_eq!(bk, [[0, 0], [2, 0], [0, 2], [2, 2]]);
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
// than the picture leaves the row's rest zero
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
        bytes_per_line: 1,
        data: vec![6],
        ..PcxSpec::default()
    }))
    .unwrap();
    assert_eq!(short.rgba, [colour(6), [0; 4], [0; 4]].concat());
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
