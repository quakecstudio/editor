// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap or index is a failed test; exact values are the point"
)]

use proptest::prelude::*;

use super::{Brush, Compiler, Entity, Face, Item, MapError, MapFile, Pair, TexDef, is_tool_key};
use crate::lexer::LexError;
use crate::qbsp::load_map_file;

const STANDARD: &[u8] = include_bytes!("../../../../docs/spec/vectors/map/standard.txt");
const VALVE220: &[u8] = include_bytes!("../../../../docs/spec/vectors/map/valve220.txt");

fn vector(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../docs/spec/vectors/map/{name}.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).unwrap()
}

fn round_trip(bytes: &[u8]) -> MapFile {
    let map = MapFile::parse(bytes).unwrap();
    assert_eq!(map.to_bytes(), bytes);
    map
}

fn standard_face(points: [[f64; 3]; 3], texture: &[u8]) -> Face {
    Face::new(
        points,
        texture,
        TexDef::Standard {
            shift: [0.0, 0.0],
            rotate: 0.0,
            scale: [1.0, 1.0],
        },
    )
    .unwrap()
}

// spec MAP-1.3, MAP-2.1, MAP-2.2, MAP-3.1, MAP-6.1: every vector both compilers read comes back
// byte for byte, comments and all, with its entities, pairs and brushes in file order.
#[test]
fn vectors_round_trip() {
    for name in [
        "standard",
        "valve220",
        "escape",
        "float-points",
        "key-30",
        "key-31",
        "no-normal",
        "point-split",
        "token-127",
        "token-128",
    ] {
        round_trip(&vector(name));
    }
    let map = round_trip(STANDARD);
    assert_eq!(map.entities.len(), 3);
    assert_eq!(map.world().unwrap().classname(), Some(&b"worldspawn"[..]));
    let door = &map.entities[2];
    assert_eq!(door.pairs().count(), 2);
    assert_eq!(door.brushes().next().unwrap().faces.len(), 7);
    assert!(matches!(door.items[2], Item::Brush(_)));
    assert_eq!(door.line(), 27);
    assert_eq!(door.brushes().next().unwrap().line(), 30);
}

// spec MAP-1.2: CRLF, 0xC2 0xA0 between key and value, and the NUL tail are kept.
#[test]
fn crlf_highbyte_round_trips() {
    let bytes = b"{\r\n\"classname\"\xc2\xa0\"worldspawn\"\r\n}\r\n";
    let map = round_trip(bytes);
    assert_eq!(map.world().unwrap().classname(), Some(&b"worldspawn"[..]));
    let map = round_trip(b"{\n}\n\0{ junk \xff");
    assert_eq!(map.entities.len(), 1);
}

// spec MAP-1.4, MAP-1.6: a quoted token holds any byte but `"`, a LF included; no token limit
// applies, and the file may not end inside quotes.
#[test]
fn tokens() {
    let map = round_trip(b"{\n\"k\" \"a\nb\"\n\"\" \"\"\n}\n");
    let pairs: Vec<&Pair> = map.entities[0].pairs().collect();
    assert_eq!(pairs[0].value(), b"a\nb");
    assert_eq!(pairs[1].key(), b"");
    let map = round_trip(&vector("token-128"));
    assert_eq!(map.entities[0].value(b"message").unwrap().len(), 128);
    assert_eq!(
        MapFile::parse(&vector("eof-in-quote")),
        Err(MapError::Lex(LexError::EofInQuote { line: 2 }))
    );
}

// spec MAP-2.1, MAP-2.2: a quoted `{` or `}` opens or closes as a bare one does, and is
// written back as it was.
#[test]
fn quoted_braces() {
    let map = round_trip(
        b"\"{\" \"k\" \"v\"\n\"{\"\n( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) w 0 0 0 1 1\n\"}\" \"}\"\n",
    );
    assert_eq!(map.entities[0].value(b"k"), Some(&b"v"[..]));
    assert_eq!(map.entities[0].brushes().next().unwrap().faces.len(), 1);
    let map = round_trip(b"{\xa8/\xa8\xff\x8a\"\"\"}\"\0\x02!!\"\"");
    assert_eq!(map.entities[0].value(b"/"), Some(&b""[..]));
}

// spec MAP-1.5: unquoted words are tokens up to whitespace, `(0` included.
#[test]
fn unquoted_words() {
    let map = round_trip(b"{\nclassname worldspawn\n(0 x}\n}\n");
    let world = &map.entities[0];
    assert_eq!(world.classname(), Some(&b"worldspawn"[..]));
    assert_eq!(world.value(b"(0"), Some(&b"x}"[..]));
}

// spec MAP-1.7, MAP-2.3: a value must be on its key's line.
#[test]
fn value_on_next_line() {
    assert_eq!(
        MapFile::parse(&vector("value-on-next-line")),
        Err(MapError::LineIncomplete { line: 2 })
    );
    assert_eq!(
        MapFile::parse(&vector("comment-mid-line")),
        Err(MapError::LineIncomplete { line: 2 })
    );
}

// spec MAP-2.1, MAP-2.2: what is not an entity, and an entity the file ends inside.
#[test]
fn entity_errors() {
    assert_eq!(
        MapFile::parse(&vector("no-open-brace")),
        Err(MapError::NoOpenBrace { line: 1 })
    );
    assert_eq!(
        MapFile::parse(&vector("eof-in-entity")),
        Err(MapError::EofWithoutBrace { line: 1 })
    );
    assert_eq!(
        MapFile::parse(b"{\n{\n( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) w 0 0 0 1 1\n"),
        Err(MapError::EofWithoutBrace { line: 1 })
    );
    assert_eq!(MapFile::parse(b"").unwrap().entities.len(), 0);
    assert_eq!(
        MapError::NoOpenBrace { line: 1 }.to_string(),
        "line 1: ParseEntity: { not found"
    );
}

// spec MAP-2.5: every pair is kept and a key is looked up by its last occurrence.
#[test]
fn duplicate_keys() {
    let map = round_trip(STANDARD);
    let light = &map.entities[1];
    assert_eq!(
        light.pairs().filter(|pair| pair.key() == b"light").count(),
        2
    );
    assert_eq!(light.value(b"light"), Some(&b"300"[..]));
}

// spec MAP-2.7: origin is three %lf numbers, missing ones 0.
#[test]
fn origin() {
    let map = round_trip(STANDARD);
    assert_eq!(map.entities[1].origin(), [0.0, 0.0, 8.0]);
    let mut entity = Entity::new();
    for (value, origin) in [
        (&b"1.5 -2 3e1"[..], [1.5, -2.0, 30.0]),
        (b"  4", [4.0, 0.0, 0.0]),
        (b"x 1 2", [0.0, 0.0, 0.0]),
        (b"1 x 2", [1.0, 0.0, 0.0]),
        (b"", [0.0, 0.0, 0.0]),
    ] {
        entity.set(b"origin", value).unwrap();
        assert_eq!(entity.origin(), origin);
    }
}

// spec MAP-3.2, MAP-3.4: points, the third on its own line, and the plane they give.
#[test]
fn faces_and_planes() {
    let map = round_trip(&vector("point-split"));
    let face = &map.entities[0].brushes().next().unwrap().faces[0];
    assert_eq!(
        face.points(),
        &[[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    );
    let plane = face.plane().unwrap();
    assert_eq!(plane.normal.map(|v| v + 0.0), [-1.0, 0.0, 0.0]);
    assert_eq!(plane.dist + 0.0, 0.0);
    assert_eq!(face.texture(), b"wall");
    assert_eq!(face.line(), 3);
    assert_eq!(
        MapFile::parse(b"{\n{\n( 0 0 0 ( 0 1 0 ) ( 0 0 1 ) w 0 0 0 1 1\n}\n}\n"),
        Err(MapError::ParsingBrush { line: 3 })
    );
    assert_eq!(
        MapFile::parse(b"{\n{\nwall\n}\n}\n"),
        Err(MapError::ParsingBrush { line: 3 })
    );
    assert_eq!(
        MapFile::parse(b"{\n{\n( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) w 0 0 x 1 1\n}\n}\n"),
        Err(MapError::BadNumber {
            line: 3,
            text: b"x".to_vec()
        })
    );
}

// spec MAP-3.3: points are read as doubles.
#[test]
fn fractional_points() {
    let map = round_trip(&vector("float-points"));
    let face = &map.entities[0].brushes().next().unwrap().faces[0];
    assert_eq!(
        face.points(),
        &[[0.5, 0.0, 0.0], [0.5, 1.9, 0.0], [0.5, 0.0, 1.0]]
    );
    assert!(face.qbsp_differs());
    assert_eq!(
        face.texdef(),
        &TexDef::Standard {
            shift: [0.7, -1.5],
            rotate: 0.0,
            scale: [1.0, 1.0]
        }
    );
}

// spec MAP-3.6: a face with collinear points is kept, with no plane.
#[test]
fn no_normal_face_is_kept() {
    let map = round_trip(&vector("no-normal"));
    let faces = &map.entities[0].brushes().next().unwrap().faces;
    assert_eq!(faces.len(), 2);
    assert!(faces[0].plane().is_none());
    assert!(faces[0].texvecs().is_none());
    assert!(faces[1].plane().is_some());
}

// spec MAP-4.1, MAP-4.2, MAP-4.3, MAP-4.4: standard faces give the texinfo qbsp gives.
#[test]
fn standard_texvecs_match_qbsp() {
    let map = round_trip(STANDARD);
    let qbsp = load_map_file(STANDARD).unwrap();
    let ours = &map.entities[0].brushes().next().unwrap().faces;
    let theirs = &qbsp.entities[0].brushes[0];
    for (face, expected) in ours.iter().zip(theirs) {
        let vecs = face.texvecs().unwrap();
        let expected = qbsp.texinfo[expected.texinfo].vecs;
        for (row, expected) in vecs.iter().zip(expected) {
            for (value, expected) in row.iter().zip(expected) {
                assert!((value - f64::from(expected)).abs() < 1e-6, "{vecs:?}");
            }
        }
    }
}

// spec MAP-5.1, MAP-5.2, MAP-5.3: Valve 220 faces, chosen face by face; the third face of the
// vector gives s (2, 0, 0) offset 16 and t (0, -0.5, 0) offset -8.
#[test]
fn valve220() {
    let map = round_trip(VALVE220);
    let faces = &map.entities[0].brushes().next().unwrap().faces;
    assert_eq!(faces.len(), 6);
    assert!(faces.iter().all(Face::qbsp_differs));
    assert_eq!(
        faces[2].texdef(),
        &TexDef::Valve {
            s: [1.0, 0.0, 0.0, 16.0],
            t: [0.0, -1.0, 0.0, -8.0],
            rotate: 0.0,
            scale: [0.5, 2.0]
        }
    );
    assert_eq!(
        faces[2].texvecs().unwrap(),
        [[2.0, 0.0, 0.0, 16.0], [0.0, -0.5, 0.0, -8.0]]
    );
    // The rotation is not applied.
    let mixed = b"{\n{\n( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) a [ 0 1 0 4 ] [ 0 0 -1 2 ] 45 1 1\n\
( 0 0 0 ) ( 0 0 1 ) ( 1 0 0 ) b 0 0 0 1 1\n}\n}\n";
    let map = round_trip(mixed);
    let faces = &map.entities[0].brushes().next().unwrap().faces;
    assert_eq!(
        faces[0].texvecs().unwrap(),
        [[0.0, 1.0, 0.0, 4.0], [0.0, 0.0, -1.0, 2.0]]
    );
    assert!(matches!(faces[1].texdef(), TexDef::Standard { .. }));
    assert_eq!(map.world().unwrap().value(b"mapversion"), None);
    assert_eq!(
        MapFile::parse(
            b"{\n{\n( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) a [ 0 1 0 4 [ 0 0 -1 2 ] 0 1 1\n}\n}\n"
        ),
        Err(MapError::Valve220 { line: 3 })
    );
}

// spec MAP-5.4: a Valve 220 scale of 0 is an error: a face read with one has no texinfo, and
// one cannot be written.
#[test]
fn valve_scale_zero() {
    let map =
        round_trip(b"{\n{\n( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) a [ 0 1 0 0 ] [ 0 0 -1 0 ] 0 0 1\n}\n}\n");
    assert!(
        map.entities[0].brushes().next().unwrap().faces[0]
            .texvecs()
            .is_none()
    );
    let valve = TexDef::Valve {
        s: [1.0, 0.0, 0.0, 0.0],
        t: [0.0, 1.0, 0.0, 0.0],
        rotate: 0.0,
        scale: [1.0, 0.0],
    };
    assert_eq!(
        Face::new([[0.0; 3]; 3], b"a", valve),
        Err(MapError::ValveScaleZero { line: 0 })
    );
}

// spec MAP-6.2: brush entities after the world are models 1, 2, ...
#[test]
fn brush_models() {
    let map = round_trip(STANDARD);
    assert_eq!(map.brush_models(), [(2, 1)]);
    assert!(map.entities[2].has_brushes());
    assert!(!map.entities[1].has_brushes());
}

// spec MAP-6.3: qbsp reads one WAD path, ericw-tools a list split at `;`.
#[test]
fn wad_paths() {
    let map = round_trip(b"{\n\"_wad\" \"\"\n\"wad\" \"a.wad;b/c.wad;\"\n}\n");
    assert_eq!(map.wad_paths(Compiler::Qbsp), [b"a.wad;b/c.wad;".to_vec()]);
    assert_eq!(
        map.wad_paths(Compiler::EricwTools),
        [b"a.wad".to_vec(), b"b/c.wad".to_vec()]
    );
    let map = round_trip(b"{\n\"_wad\" \"x.wad\"\n\"wad\" \"a.wad\"\n}\n");
    assert_eq!(map.wad_paths(Compiler::Qbsp), [b"x.wad".to_vec()]);
    assert!(MapFile::new().wad_paths(Compiler::Qbsp).is_empty());
    assert!(
        round_trip(b"{\n}\n")
            .wad_paths(Compiler::EricwTools)
            .is_empty()
    );
}

// spec MAP-6.7: keys starting with `_` are the tools'.
#[test]
fn tool_keys() {
    assert!(is_tool_key(b"_wad"));
    assert!(is_tool_key(b"_"));
    assert!(!is_tool_key(b"wad"));
    assert!(!is_tool_key(b""));
}

// spec MAP-7.1, MAP-7.2, MAP-7.3: a map built from scratch is written in the form both
// compilers read: quoted pairs and faces on lines of their own, single spaces, LF line ends,
// integer numbers; and qbsp reads it as the reader does.
#[test]
fn new_maps_use_the_common_form() {
    let mut map = MapFile::new();
    let mut world = Entity::new();
    world.set(b"classname", b"worldspawn").unwrap();
    world.set(b"wad", b"gfx/base.wad").unwrap();
    let mut brush = Brush::new();
    brush.faces.push(standard_face(
        [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        b"wall",
    ));
    brush.faces.push(
        Face::new(
            [[8.0, 8.0, 8.0], [8.0, 8.0, 9.0], [8.0, 9.0, 8.0]],
            b"*water",
            TexDef::Standard {
                shift: [16.0, -8.0],
                rotate: 90.0,
                scale: [0.5, 2.0],
            },
        )
        .unwrap(),
    );
    world.items.push(Item::Brush(brush));
    map.push_entity(world);
    let mut light = Entity::new();
    light.set(b"classname", b"light").unwrap();
    light.set(b"origin", b"0 0 8").unwrap();
    map.push_entity(light);
    let text = map.to_bytes();
    assert_eq!(
        String::from_utf8(text.clone()).unwrap(),
        "{\n\"classname\" \"worldspawn\"\n\"wad\" \"gfx/base.wad\"\n{\n\
( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) wall 0 0 0 1 1\n\
( 8 8 8 ) ( 8 8 9 ) ( 8 9 8 ) *water 16 -8 90 0.5 2\n}\n}\n\
{\n\"classname\" \"light\"\n\"origin\" \"0 0 8\"\n}\n"
    );
    let reread = round_trip(&text);
    assert!(reread.check().is_empty(), "{:?}", reread.check());
    let qbsp = load_map_file(&text).unwrap();
    assert!(qbsp.warnings.is_empty());
    assert_eq!(qbsp.entities[0].brushes[0].len(), 2);
    assert_eq!(
        Pair::new(b"a\"b", b"c"),
        Err(MapError::Unwritable {
            text: b"a\"b".to_vec()
        })
    );
    assert!(
        Face::new(
            [[0.0; 3]; 3],
            b"two words",
            TexDef::Standard {
                shift: [0.0; 2],
                rotate: 0.0,
                scale: [1.0; 2],
            }
        )
        .is_err()
    );
    assert!(
        Face::new(
            [[f64::NAN, 0.0, 0.0], [0.0; 3], [0.0; 3]],
            b"w",
            TexDef::Standard {
                shift: [0.0; 2],
                rotate: 0.0,
                scale: [1.0; 2],
            }
        )
        .is_err()
    );
}

// spec MAP-7.4: an edit leaves Valve 220 faces, fractions and comments as they were.
#[test]
fn edits_keep_the_rest() {
    let mut map = round_trip(VALVE220);
    map.entities[0].set(b"message", b"Hello").unwrap();
    map.entities[0].set(b"wad", b"other.wad").unwrap();
    let text = String::from_utf8(map.to_bytes()).unwrap();
    let original = String::from_utf8(VALVE220.to_vec()).unwrap();
    let expected = original.replace(
        "\"wad\" \"gfx/base.wad\"",
        "\"wad\" \"other.wad\"\n\"message\" \"Hello\"",
    );
    assert_eq!(text, expected);
    let mut map = round_trip(&vector("float-points"));
    let Item::Brush(brush) = &mut map.entities[0].items[0] else {
        panic!("the first item is a brush");
    };
    let texdef = *brush.faces[0].texdef();
    brush.faces[0]
        .set(
            [[0.5, 0.0, 0.0], [0.5, 2.0, 0.0], [0.5, 0.0, 1.0]],
            b"wall",
            texdef,
        )
        .unwrap();
    assert_eq!(
        map.to_bytes(),
        b"{\n{\n( 0.5 0 0 ) ( 0.5 2 0 ) ( 0.5 0 1 ) wall 0.7 -1.5 0 1 1\n}\n}\n"
    );
}

// spec MAP-2.3: setting a key keeps the text around it; removing takes every occurrence.
#[test]
fn set_and_remove() {
    let mut map = round_trip(b"{\n  \"a\"   \"1\" // note\n\"a\" \"2\"\n}\n");
    map.entities[0].set(b"a", b"3").unwrap();
    assert_eq!(
        map.to_bytes(),
        b"{\n  \"a\"   \"1\" // note\n\"a\" \"3\"\n}\n"
    );
    let mut map = round_trip(b"{\n  a   1 // note\n}\n");
    map.entities[0].set(b"a", b"two words").unwrap();
    assert_eq!(map.to_bytes(), b"{\n  a   \"two words\" // note\n}\n");
    assert_eq!(map.entities[0].remove(b"a"), 1);
    assert_eq!(map.entities[0].remove(b"a"), 0);
    assert_eq!(map.to_bytes(), b"{ // note\n}\n");
    let mut entity = Entity::default();
    entity.items.push(Item::Brush(Brush::new()));
    entity.set(b"k", b"v").unwrap();
    assert!(matches!(entity.items[0], Item::Pair(_)));
}

// spec MAP-2.3: a pair keeps its value's raw bytes, including the quotes, and gives them back.
#[test]
fn pair_value_raw_keeps_the_quotes() {
    let pair = Pair::new(b"k", b"hello world").unwrap();
    assert_eq!(pair.value_raw(), b"\"hello world\"");
    assert_eq!(pair.value(), b"hello world");
}

// spec MAP-1.8, MAP-2.3: set_value reports an ericw escape when the value ends in a backslash;
// a value with no trailing backslash and a key without one does not.
#[test]
fn set_value_reports_an_ericw_escape() {
    let mut pair = Pair::new(b"k", b"v").unwrap();
    pair.set_value(b"ends\\").unwrap();
    assert!(pair.ericw_escape());
    assert_eq!(pair.value_raw(), b"\"ends\\\"");
    let mut plain = Pair::new(b"k", b"v").unwrap();
    plain.set_value(b"plain").unwrap();
    assert!(!plain.ericw_escape());
}

// spec MAP-3.3, MAP-4.1: a face number token must be used in full; a token with trailing
// non-number bytes is a BadNumber, not a silently truncated value.
#[test]
fn face_number_must_consume_the_whole_token() {
    assert_eq!(
        MapFile::parse(b"{\n{\n( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) w 0 0 0 1x 1\n}\n}\n"),
        Err(MapError::BadNumber {
            line: 3,
            text: b"1x".to_vec()
        })
    );
}

// spec MAP-3.3: the number formatter writes a whole number with no fraction and keeps the sign
// of a real -0.0 as the plus it rounds to (value + 0.0 normalises -0.0 to +0.0).
#[test]
fn number_formats_whole_and_signed_zero() {
    assert_eq!(super::number(0.0).unwrap(), "0");
    assert_eq!(super::number(-0.0).unwrap(), "0");
    assert_eq!(super::number(42.0).unwrap(), "42");
    assert_eq!(super::number(-42.0).unwrap(), "-42");
    assert_eq!(super::number(1.5).unwrap(), "1.5");
}

// spec MAP-3.3, MAP-4.1: a face qbsp_differs flag is the OR of three independent reasons. An
// all-integer standard face matches qbsp (false); a fractional point makes it differ (true);
// a Valve 220 face always differs because qbsp has no Valve form (true).
#[test]
fn face_qbsp_differs_covers_each_reason() {
    let integer = standard_face([[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], b"wall");
    assert!(!integer.qbsp_differs());
    let fractional = standard_face([[0.0, 0.0, 1.5], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], b"wall");
    assert!(fractional.qbsp_differs());
    let valve = Face::new(
        [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        b"wall",
        TexDef::Valve {
            s: [0.0, 1.0, 0.0, 0.0],
            t: [0.0, 0.0, -1.0, 0.0],
            rotate: 0.0,
            scale: [1.0, 1.0],
        },
    )
    .unwrap();
    assert!(valve.qbsp_differs());
}

/// A key, value or texture name that both readers take as one token.
fn word() -> impl Strategy<Value = String> {
    "[a-z_*+][a-z0-9_*+]{0,8}"
}

fn number() -> impl Strategy<Value = String> {
    prop_oneof![
        (-512_i32..512).prop_map(|value| value.to_string()),
        (-512.0_f64..512.0).prop_map(|value| format!("{value}")),
    ]
}

/// One line of trivia: spaces, tabs, a comment, CR.
fn gap() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(" ".to_owned()),
        Just("\t ".to_owned()),
        Just(" // note ".to_owned()),
        Just("\r".to_owned()),
    ]
}

fn face_text() -> impl Strategy<Value = String> {
    (
        proptest::collection::vec(number(), 9),
        word(),
        proptest::collection::vec(number(), 5),
        any::<bool>(),
    )
        .prop_map(|(points, texture, numbers, valve)| {
            let points = points.join(" ");
            let mut points = points.split(' ');
            let mut text = String::new();
            for _ in 0..3 {
                let point: Vec<&str> = points.by_ref().take(3).collect();
                text.push_str(&format!("( {} ) ", point.join(" ")));
            }
            text.push_str(&texture);
            if valve {
                text.push_str(&format!(
                    " [ 1 0 0 {} ] [ 0 1 0 {} ] {} 1 1",
                    numbers[0], numbers[1], numbers[2]
                ));
            } else {
                text.push(' ');
                text.push_str(&numbers.join(" "));
            }
            text
        })
}

fn map_text() -> impl Strategy<Value = String> {
    let pair = (word(), word(), "[ \t]{1,3}")
        .prop_map(|(key, value, gap)| format!("\"{key}\"{gap}\"{value}\""));
    let brush = proptest::collection::vec((face_text(), gap()), 0..4).prop_map(|faces| {
        let mut text = "{\n".to_owned();
        for (face, gap) in faces {
            text.push_str(&face);
            text.push_str(&gap);
            text.push('\n');
        }
        text.push('}');
        text
    });
    let item = prop_oneof![pair, brush];
    let entity =
        (proptest::collection::vec((item, gap()), 0..5), gap()).prop_map(|(items, gap)| {
            let mut text = format!("{gap}\n{{\n");
            for (item, gap) in items {
                text.push_str(&item);
                text.push_str(&gap);
                text.push('\n');
            }
            text.push_str("}\n");
            text
        });
    proptest::collection::vec(entity, 0..4).prop_map(|entities| entities.concat())
}

proptest! {
    // spec MAP-1.3, MAP-2.2, MAP-3.2: any map of pairs, standard and Valve 220 faces, comments
    // and whitespace reads and writes back byte for byte.
    #[test]
    fn generated_maps_round_trip(text in map_text()) {
        let map = MapFile::parse(text.as_bytes()).unwrap();
        prop_assert_eq!(map.to_bytes(), text.as_bytes());
    }

    // spec MAP-1.1: arbitrary bytes never panic, and whatever reads writes back unchanged.
    #[test]
    fn arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        if let Ok(map) = MapFile::parse(&bytes) {
            prop_assert_eq!(map.to_bytes(), bytes.clone());
            let _diagnostics = map.check();
        }
        let _qbsp = load_map_file(&bytes);
    }

    // spec MAP-7.3: a face written from numbers reads back as the same numbers.
    #[test]
    fn written_faces_read_back(values in proptest::collection::vec(-1.0e6_f64..1.0e6, 14)) {
        let points = [
            [values[0], values[1], values[2]],
            [values[3], values[4], values[5]],
            [values[6], values[7], values[8]],
        ];
        let texdef = TexDef::Standard {
            shift: [values[9], values[10]],
            rotate: values[11],
            scale: [values[12], values[13]],
        };
        let mut map = MapFile::new();
        let mut entity = Entity::new();
        let mut brush = Brush::new();
        brush.faces.push(Face::new(points, b"w", texdef).unwrap());
        entity.items.push(Item::Brush(brush));
        map.push_entity(entity);
        let reread = MapFile::parse(&map.to_bytes()).unwrap();
        let face = &reread.entities[0].brushes().next().unwrap().faces[0];
        prop_assert_eq!(face.points(), &points);
        prop_assert_eq!(face.texdef(), &texdef);
    }
}
