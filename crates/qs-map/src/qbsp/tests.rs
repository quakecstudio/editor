// SPDX-License-Identifier: GPL-3.0-or-later

//! The `.map` vectors of `docs/spec/vectors/map/`, each giving the result its README records
//! for id's qbsp.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test; exact values are the point"
)]

use super::{
    MAP_LUMP_LINE, MAP_MAX_ENTITIES, MAP_MAX_KEY, MAP_MAX_TEXINFO, MAP_MAX_TEXTURE_NAME,
    MAP_MAX_VALUE, MAP_TEX_SPECIAL, QbspError, QbspMap, QbspWarning, TextureSlot, is_special,
    load_map_file,
};
use core::f32::consts::FRAC_1_SQRT_2;

use crate::lexer::LexError;

/// A face of the README table: normal, dist, miptex, flags, s and t.
type FaceRow = ([f32; 3], f32, usize, i32, [f32; 4], [f32; 4]);

const STANDARD: &[u8] = include_bytes!("../../../../docs/spec/vectors/map/standard.txt");
const VALVE220: &[u8] = include_bytes!("../../../../docs/spec/vectors/map/valve220.txt");

fn vector(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../docs/spec/vectors/map/{name}.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).unwrap()
}

fn error(name: &str) -> String {
    load_map_file(&vector(name)).unwrap_err().to_string()
}

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-6)
}

fn standard() -> QbspMap {
    load_map_file(STANDARD).unwrap()
}

// spec MAP-1.3, MAP-2.1, MAP-2.2, MAP-3.1, MAP-3.5, MAP-6.1: `standard` reads as 3 entities and
// 2 brushes, its comments skipped, with one duplicate-plane warning for the door's seventh face.
#[test]
fn standard_loads() {
    let map = standard();
    assert_eq!(map.entities.len(), 3);
    let brushes: usize = map.entities.iter().map(|entity| entity.brushes.len()).sum();
    assert_eq!(brushes, 2);
    assert_eq!(map.warnings, [QbspWarning::DuplicatePlane { line: 37 }]);
    assert_eq!(
        map.warnings[0].to_string(),
        "WARNING: brush with duplicate plane"
    );
    assert_eq!(
        map.entities[0].value_for_key(b"classname"),
        Some(&b"worldspawn"[..])
    );
    assert_eq!(map.entities[2].brushes[0].len(), 6);
}

// spec MAP-3.4, MAP-4.1, MAP-4.2, MAP-4.3, MAP-4.4, MAP-4.5, MAP-4.7: the world brush's planes
// and texinfo, the five texture names and seven texinfo in first-use order.
#[test]
fn standard_planes_and_texinfo() {
    let map = standard();
    let names: Vec<&[u8]> = map.miptex.iter().map(Vec::as_slice).collect();
    assert_eq!(
        names,
        [&b"wall"[..], b"floor", b"+0lava", b"*water", b"sky1"]
    );
    assert_eq!(map.texinfo.len(), 7);
    let expected: [FaceRow; 6] = [
        (
            [-1.0, 0.0, 0.0],
            64.0,
            0,
            0,
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0],
        ),
        (
            [0.0, -1.0, 0.0],
            64.0,
            0,
            0,
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0],
        ),
        (
            [0.0, 0.0, -1.0],
            16.0,
            1,
            0,
            [0.0, 2.0, 0.0, 16.0],
            [0.5, 0.0, 0.0, -8.0],
        ),
        (
            [0.0, 0.0, 1.0],
            16.0,
            2,
            0,
            [FRAC_1_SQRT_2, FRAC_1_SQRT_2, 0.0, 0.0],
            [FRAC_1_SQRT_2, -FRAC_1_SQRT_2, 0.0, 0.0],
        ),
        (
            [0.0, 1.0, 0.0],
            64.0,
            3,
            1,
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0],
        ),
        (
            [1.0, 0.0, 0.0],
            64.0,
            4,
            1,
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0],
        ),
    ];
    let faces = &map.entities[0].brushes[0];
    assert_eq!(faces.len(), 6);
    for (face, (normal, dist, miptex, flags, s, t)) in faces.iter().zip(expected) {
        assert_eq!(face.plane.normal.map(|v| v + 0.0), normal);
        assert_eq!(face.plane.dist, dist);
        let texinfo = map.texinfo[face.texinfo];
        assert_eq!(texinfo.miptex, miptex);
        assert_eq!(texinfo.flags, flags);
        assert!(close(texinfo.vecs[0], s), "{:?}", texinfo.vecs);
        assert!(close(texinfo.vecs[1], t), "{:?}", texinfo.vecs);
    }
    let door: Vec<usize> = map.entities[2].brushes[0]
        .iter()
        .map(|face| face.texinfo)
        .collect();
    assert_eq!(door, [0, 1, 6, 6, 1, 0]);
    assert_eq!(
        map.texinfo[6].vecs,
        [[1.0, 0.0, 0.0, 0.0], [0.0, -1.0, 0.0, 0.0]]
    );
    let planes: Vec<([f32; 3], f32)> = map.entities[2].brushes[0]
        .iter()
        .map(|face| (face.plane.normal.map(|v| v + 0.0), face.plane.dist + 0.0))
        .collect();
    assert_eq!(
        planes,
        [
            ([-1.0, 0.0, 0.0], 0.0),
            ([0.0, -1.0, 0.0], 0.0),
            ([0.0, 0.0, -1.0], 0.0),
            ([0.0, 0.0, 1.0], 8.0),
            ([0.0, 1.0, 0.0], 8.0),
            ([1.0, 0.0, 0.0], 8.0),
        ]
    );
}

// spec MAP-2.5, MAP-2.6, MAP-2.7: duplicate keys are all kept, looked up by the last, and
// written to the lump in reverse; the light's origin is (0, 0, 8); the lump is 199 bytes.
#[test]
fn standard_entity_lump() {
    let map = standard();
    let light = &map.entities[1];
    assert_eq!(light.pairs.len(), 4);
    assert_eq!(light.value_for_key(b"light"), Some(&b"300"[..]));
    assert_eq!(light.origin, [0.0, 0.0, 8.0]);
    assert_eq!(map.entities[0].origin, [0.0, 0.0, 0.0]);
    let lump = map.entity_lump().unwrap();
    let expected = b"{\n\"sounds\" \"2\"\n\"message\" \"The Test Map\"\n\"wad\" \"gfx/base.wad\"\n\
\"classname\" \"worldspawn\"\n}\n{\n\"light\" \"300\"\n\"light\" \"200\"\n\"origin\" \"0 0 8\"\n\
\"classname\" \"light\"\n}\n{\n\"angle\" \"-1\"\n\"classname\" \"func_door\"\n}\n\0";
    assert_eq!(lump, expected);
    assert_eq!(lump.len(), 199);
}

// spec MAP-2.6: an entity with no pairs is not written to the lump.
#[test]
fn entity_without_pairs_is_skipped() {
    let map = load_map_file(b"{\n}\n{\n\"a\" \"b\"\n}\n").unwrap();
    assert_eq!(map.entity_lump().unwrap(), b"{\n\"a\" \"b\"\n}\n\0");
    assert_eq!(load_map_file(b"").unwrap().entity_lump().unwrap(), b"\0");
}

// spec MAP-6.2: brush entities after the world are numbered from 1 and their `model` key
// replaced; entities without brushes keep their keys.
#[test]
fn models_are_numbered() {
    let mut map = standard();
    map.number_models();
    assert_eq!(map.entities[2].value_for_key(b"model"), Some(&b"*1"[..]));
    assert_eq!(map.entities[0].value_for_key(b"model"), None);
    assert_eq!(map.entities[1].value_for_key(b"model"), None);
    let text = b"{\n{\n}\n}\n{\n\"model\" \"x\"\n{\n}\n}\n{\n}\n{\n{\n}\n}\n";
    let mut map = load_map_file(text).unwrap();
    map.number_models();
    assert_eq!(map.entities[1].value_for_key(b"model"), Some(&b"*1"[..]));
    assert_eq!(map.entities[1].pairs.len(), 1);
    assert_eq!(map.entities[2].value_for_key(b"model"), None);
    assert_eq!(map.entities[3].value_for_key(b"model"), Some(&b"*2"[..]));
}

// spec MAP-5.5: id's qbsp takes the `[` of a Valve 220 face as a shift and fails.
#[test]
fn valve220_fails_in_qbsp() {
    assert_eq!(load_map_file(VALVE220), Err(QbspError::ParsingBrush));
    assert_eq!(QbspError::ParsingBrush.to_string(), "parsing brush");
}

// spec MAP-2.1: a token other than `{` where an entity starts.
#[test]
fn no_open_brace() {
    assert_eq!(error("no-open-brace"), "ParseEntity: { not found");
}

// spec MAP-2.2: the end of the file inside an entity.
#[test]
fn eof_in_entity() {
    assert_eq!(
        error("eof-in-entity"),
        "ParseEntity: EOF without closing brace"
    );
}

// spec MAP-1.4: the end of the file inside a quoted token.
#[test]
fn eof_in_quote() {
    assert_eq!(error("eof-in-quote"), "EOF inside quoted token");
    assert_eq!(
        load_map_file(b"{ \"a"),
        Err(QbspError::Lex(LexError::EofInQuote { line: 1 }))
    );
}

// spec MAP-1.7, MAP-2.3: a value on the next line, and a comment between key and value.
#[test]
fn line_incomplete() {
    assert_eq!(error("value-on-next-line"), "Line 2 is incomplete");
    assert_eq!(error("comment-mid-line"), "Line 2 is incomplete");
    assert_eq!(
        load_map_file(b"{\n\"a\""),
        Err(QbspError::LineIncomplete { line: 2 })
    );
}

// spec MAP-2.4: a 30-byte key loads and a 31-byte key fails; likewise values at 1022 and 1023
// bytes, which only qbsp's own buffer could hold, are checked by their length.
#[test]
fn key_length() {
    assert_eq!(MAP_MAX_KEY, 32);
    assert_eq!(MAP_MAX_VALUE, 1024);
    let map = load_map_file(&vector("key-30")).unwrap();
    assert_eq!(map.entities[0].pairs[0].0.len(), 30);
    assert_eq!(error("key-31"), "ParseEpar: token too long");
}

// spec MAP-1.6, MAP-2.10: a 127-byte value parses but overflows the lump line; 128 bytes fail.
#[test]
fn token_length() {
    let map = load_map_file(&vector("token-127")).unwrap();
    assert_eq!(map.entities[0].pairs[0].1.len(), 127);
    assert!(matches!(
        map.entity_lump(),
        Err(QbspError::PairTooLong { .. })
    ));
    assert_eq!(error("token-128"), "Token too large on line 2");
    let mut text = b"{\n".to_vec();
    text.extend([b'k'; 128]);
    assert_eq!(
        load_map_file(&text).unwrap_err().to_string(),
        "Token too large on line 2"
    );
}

// spec MAP-2.10: key and value together at 121 bytes fit the lump line; 122 do not.
#[test]
fn lump_line_limit() {
    assert_eq!(MAP_LUMP_LINE, 128);
    for (len, fits) in [(116, true), (117, false)] {
        let mut text = b"{\n\"kkkkk\" \"".to_vec();
        text.extend(core::iter::repeat_n(b'v', len));
        text.extend_from_slice(b"\"\n}\n");
        let map = load_map_file(&text).unwrap();
        assert_eq!(map.entity_lump().is_ok(), fits, "{len}");
    }
}

// spec MAP-1.4: a backslash is an ordinary byte, kept and written as read.
#[test]
fn escape_is_kept() {
    let map = load_map_file(&vector("escape")).unwrap();
    assert_eq!(
        map.entities[0].value_for_key(b"message"),
        Some(&b"a\\nb"[..])
    );
    assert_eq!(
        map.entity_lump().unwrap(),
        b"{\n\"message\" \"a\\nb\"\n}\n\0"
    );
}

// spec MAP-3.2: a face's third point may start a new line.
#[test]
fn point_split() {
    let map = load_map_file(&vector("point-split")).unwrap();
    let face = &map.entities[0].brushes[0][0];
    assert_eq!(face.plane.normal.map(|v| v + 0.0), [-1.0, 0.0, 0.0]);
    assert_eq!(face.plane.dist + 0.0, 0.0);
}

// spec MAP-3.3, MAP-4.1: points and shifts are read with atoi, so fractions are dropped.
#[test]
fn float_points() {
    let map = load_map_file(&vector("float-points")).unwrap();
    let face = &map.entities[0].brushes[0][0];
    assert_eq!(
        face.points,
        [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    );
    assert_eq!(face.plane.normal.map(|v| v + 0.0), [-1.0, 0.0, 0.0]);
    assert_eq!(face.plane.dist + 0.0, 0.0);
    let texinfo = map.texinfo[face.texinfo];
    assert_eq!((texinfo.vecs[0][3], texinfo.vecs[1][3]), (0.0, -1.0));
}

// spec MAP-3.6: collinear points warn, end the brush, and the rest is read as keys.
#[test]
fn no_normal() {
    assert_eq!(error("no-normal"), "Line 4 is incomplete");
    let map = load_map_file(b"{\n{\n( 0 0 0 ) ( 1 1 1 ) ( 2 2 2 ) w 0 0 0 1 1\n}\n").unwrap();
    assert_eq!(map.warnings, [QbspWarning::NoNormal { line: 3 }]);
    assert_eq!(
        QbspWarning::NoNormal { line: 3 }.to_string(),
        "WARNING: brush plane with no normal"
    );
    assert!(map.entities[0].brushes[0].is_empty());
}

// spec MAP-1.2: CRLF line ends and the bytes 0xC2 0xA0 are whitespace.
#[test]
fn crlf_highbyte() {
    let bytes = [
        0x7b, 0x0d, 0x0a, 0x22, 0x63, 0x6c, 0x61, 0x73, 0x73, 0x6e, 0x61, 0x6d, 0x65, 0x22, 0xc2,
        0xa0, 0x22, 0x77, 0x6f, 0x72, 0x6c, 0x64, 0x73, 0x70, 0x61, 0x77, 0x6e, 0x22, 0x0d, 0x0a,
        0x7d, 0x0d, 0x0a,
    ];
    let map = load_map_file(&bytes).unwrap();
    assert_eq!(map.entities.len(), 1);
    assert_eq!(
        map.entities[0].pairs,
        [(b"classname".to_vec(), b"worldspawn".to_vec())]
    );
}

// spec MAP-1.1: the text ends at the first NUL.
#[test]
fn nul_ends_the_text() {
    let map = load_map_file(b"{\n\"a\" \"b\"\n}\n\0garbage").unwrap();
    assert_eq!(map.entities.len(), 1);
    assert_eq!(
        load_map_file(b"{\n\"a\"\0 \"b\"\n}"),
        Err(QbspError::LineIncomplete { line: 2 })
    );
}

// spec MAP-1.9: qbsp's line numbers skip the LF that ends a comment.
#[test]
fn qbsp_line_numbers_skip_comment_lines() {
    let text = b"// one\n// two\n{\n\"a\"\n\"b\"\n}\n";
    assert_eq!(
        load_map_file(text),
        Err(QbspError::LineIncomplete { line: 2 })
    );
}

// spec MAP-2.8: the 1025th entity fails.
#[test]
fn too_many_entities() {
    let mut text = Vec::new();
    for _ in 0..MAP_MAX_ENTITIES {
        text.extend_from_slice(b"{\n}\n");
    }
    assert_eq!(load_map_file(&text).unwrap().entities.len(), 1024);
    text.extend_from_slice(b"{\n}\n");
    assert_eq!(
        load_map_file(&text).unwrap_err().to_string(),
        "num_entities == MAX_MAP_ENTITIES"
    );
}

// spec MAP-4.6: a texture name of 15 bytes loads and one of 16 fails.
#[test]
fn texture_name_length() {
    assert_eq!(MAP_MAX_TEXTURE_NAME, 15);
    for (len, ok) in [(15, true), (16, false)] {
        let name = "t".repeat(len);
        let text = format!("{{\n{{\n( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) {name} 0 0 0 1 1\n}}\n}}\n");
        let result = load_map_file(text.as_bytes());
        assert_eq!(result.is_ok(), ok, "{len}");
        if !ok {
            assert!(matches!(result, Err(QbspError::TextureNameTooLong { .. })));
        }
    }
}

/// A map whose one brush has `count` faces, each with its own texture or its own shift.
fn many_faces(count: usize, distinct_names: bool) -> Vec<u8> {
    let mut text = b"{\n{\n".to_vec();
    for index in 0..count {
        let (name, shift) = if distinct_names {
            (format!("t{index}"), 0)
        } else {
            ("t".to_owned(), index)
        };
        let z = index;
        text.extend_from_slice(
            format!("( 0 0 {z} ) ( 0 1 {z} ) ( 1 0 {z} ) {name} {shift} 0 0 1 1\n").as_bytes(),
        );
    }
    text.extend_from_slice(b"}\n}\n");
    text
}

// spec MAP-4.7: at most 4096 texture names and 4096 texinfo.
#[test]
fn texinfo_limits() {
    assert_eq!(MAP_MAX_TEXINFO, 4096);
    assert_eq!(
        load_map_file(&many_faces(4096, true)).unwrap().miptex.len(),
        4096
    );
    assert_eq!(
        load_map_file(&many_faces(4097, true)),
        Err(QbspError::TooManyMiptex)
    );
    assert_eq!(
        load_map_file(&many_faces(4096, false))
            .unwrap()
            .texinfo
            .len(),
        4096
    );
    assert_eq!(
        load_map_file(&many_faces(4097, false))
            .unwrap_err()
            .to_string(),
        "numtexinfo == MAX_MAP_TEXINFO"
    );
}

// spec MAP-4.5: `*` and `sky` in any case are special.
#[test]
fn special_textures() {
    assert_eq!(MAP_TEX_SPECIAL, 1);
    for name in [&b"*water"[..], b"sky1", b"SKY", b"Sky4"] {
        assert!(is_special(name));
    }
    for name in [&b"sk"[..], b"wall", b"+0sky", b"s*"] {
        assert!(!is_special(name));
    }
}

// spec MAP-6.3: `_wad`, else `wad`, else none.
#[test]
fn wad_path() {
    assert_eq!(standard().wad_path(), Some(&b"gfx/base.wad"[..]));
    let map = load_map_file(b"{\n\"_wad\" \"\"\n\"wad\" \"a.wad\"\n}\n").unwrap();
    assert_eq!(map.wad_path(), Some(&b"a.wad"[..]));
    let map = load_map_file(b"{\n\"wad\" \"a.wad\"\n\"_wad\" \"b.wad\"\n}\n").unwrap();
    assert_eq!(map.wad_path(), Some(&b"b.wad"[..]));
    let map = load_map_file(b"{\n\"classname\" \"worldspawn\"\n}\n").unwrap();
    assert_eq!(map.wad_path(), None);
    assert_eq!(
        map.texture_slots(&[]).unwrap(),
        (Vec::new(), vec![QbspWarning::NoWadFile])
    );
    assert_eq!(
        QbspWarning::NoWadFile.to_string(),
        "WARNING: no wadfile specified"
    );
}

// spec MAP-6.8: the WAD's other frames of an animation are added only when the map spells the
// name as the upper-cased WAD names do, and a texture the WAD lacks gets an empty slot.
#[test]
fn texture_slots() {
    let map = standard();
    let lumps: [&[u8]; 5] = [b"wall", b"+0lava", b"+1lava", b"*water", b"FLOOR"];
    let (slots, warnings) = map.texture_slots(&lumps).unwrap();
    let found: Vec<(&[u8], bool)> = slots
        .iter()
        .map(|slot: &TextureSlot| (slot.name.as_slice(), slot.found))
        .collect();
    assert_eq!(
        found,
        [
            (&b"wall"[..], true),
            (b"floor", true),
            (b"+0lava", true),
            (b"*water", true),
            (b"sky1", false)
        ]
    );
    assert_eq!(warnings[0].to_string(), "WARNING: texture sky1 not found");
    let text = b"{\n\"wad\" \"w\"\n{\n( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) +0LAVA 0 0 0 1 1\n}\n}\n";
    let map = load_map_file(text).unwrap();
    let lumps: [&[u8]; 4] = [b"+0lava", b"+1lava", b"+alava", b"+2water"];
    let (slots, warnings) = map.texture_slots(&lumps).unwrap();
    let names: Vec<&[u8]> = slots.iter().map(|slot| slot.name.as_slice()).collect();
    assert_eq!(names, [&b"+0LAVA"[..], b"+1LAVA", b"+ALAVA"]);
    assert!(warnings.is_empty());
}
