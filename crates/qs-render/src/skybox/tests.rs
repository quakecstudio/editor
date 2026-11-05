// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::{SKY_SIDES, SkyKeys, Skybox, sky_parse_worldspawn};
use crate::picture::tests::{PcxSpec, TgaSpec, colour, files, pcx, solid, tga};

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
