// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::{wad_files, wad_key};

// spec WAD3-4.1: the wad key is the first entity's first `wad` or `_wad` key, its trailing
// spaces removed; quoted and bare words, comments and brushes' braces read as COM_Parse reads
// them; text that ends first, or a world with none, gives none
#[test]
fn the_wad_key_is_read_as_mod_load_wad_files_reads_it() {
    let world = |text: &str| wad_key(text.as_bytes()).map(|v| String::from_utf8(v).unwrap());
    assert_eq!(
        world("{\n\"classname\" \"worldspawn\"\n\"wad\" \"halflife.wad;decals.wad\"\n}"),
        Some("halflife.wad;decals.wad".to_owned())
    );
    assert_eq!(
        world("{ \"_wad\" \"a.wad\" \"wad\" \"b.wad\" }"),
        Some("a.wad".to_owned())
    );
    assert_eq!(
        world("{ \"wad  \" \"spaced.wad\" }"),
        Some("spaced.wad".to_owned())
    );
    assert_eq!(
        world("// c\n{ /* x */ wad bare.wad }"),
        Some("bare.wad".to_owned())
    );
    assert_eq!(
        world("{ \"classname\" \"worldspawn\" }\n{ \"wad\" \"later.wad\" }"),
        None
    );
    assert_eq!(world("{ \"wad\""), None, "the value is past the end");
    assert_eq!(world("\"wad\" \"x\""), None, "no opening brace");
    assert_eq!(world(""), None);
}

// spec WAD3-4.2: each name between `;`s is its base with `.wad` added unless it ends in it,
// then under `gfx/`; a
// base of fewer than 2 bytes is `?model?`; an empty name ends the list
#[test]
fn each_name_gives_its_two_files() {
    let files = |value: &str| -> Vec<[String; 2]> {
        wad_files(value.as_bytes())
            .into_iter()
            .map(|pair| pair.map(|f| String::from_utf8(f).unwrap()))
            .collect()
    };
    assert_eq!(
        files("\\sierra\\half-life\\valve\\halflife.wad;C:/maps/decals.WAD"),
        [
            ["halflife.wad".to_owned(), "gfx/halflife.wad".to_owned()],
            ["decals.wad".to_owned(), "gfx/decals.wad".to_owned()],
        ]
    );
    assert_eq!(files("x.wad")[0][0], "?model?.wad");
    // A base already ending in `.wad` (`COM_AddExtension`, case kept), and one ending in a dot.
    assert_eq!(files("textures/foo.wad.bak")[0][0], "foo.wad");
    assert_eq!(files("foo.WAD.bak")[0][0], "foo.WAD.wad");
    assert_eq!(files("foo.wad..x")[0][0], "foo.wad..wad");
    assert_eq!(
        files("dir.v2/name")[0][0],
        "?model?.wad",
        "the last dot is in a folder"
    );
    assert_eq!(files("a.wad;;b.wad").len(), 1);
    assert!(files("").is_empty());
}
