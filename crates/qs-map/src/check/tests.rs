// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::float_cmp_const,
    reason = "test code: a failed unwrap is a failed test; exact values are the point"
)]

use crate::{DiagnosticKind, MAP_MAX_SOUNDS, MapFile};

const STANDARD: &[u8] = include_bytes!("../../../../docs/spec/vectors/map/standard.txt");
const VALVE220: &[u8] = include_bytes!("../../../../docs/spec/vectors/map/valve220.txt");

const WORLD: &str = "{\n\"classname\" \"worldspawn\"\n\"wad\" \"a.wad\"\n";

/// The diagnostics of a world with `body` added, and the lines they are on.
fn check(body: &str) -> Vec<(u32, DiagnosticKind)> {
    let text = format!("{WORLD}{body}");
    MapFile::parse(text.as_bytes())
        .unwrap()
        .check()
        .into_iter()
        .map(|diagnostic| (diagnostic.line, diagnostic.kind))
        .collect()
}

const FACE: &str = "( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 )";

// spec MAP-5.4: a Valve 220 face with a scale of 0 is reported.
#[test]
fn valve_scale_zero() {
    let found = check(&format!(
        "{{\n{FACE} a [ 0 1 0 0 ] [ 0 0 -1 0 ] 0 1 0\n}}\n}}\n"
    ));
    assert_eq!(
        found,
        [
            (5, DiagnosticKind::Valve220),
            (5, DiagnosticKind::ValveScaleZero)
        ]
    );
}

// spec MAP-3.5, MAP-6.5, MAP-6.6: the standard vector has one duplicate plane, and its world
// a message without % and a CD track in range.
#[test]
fn standard_vector() {
    let map = MapFile::parse(STANDARD).unwrap();
    let kinds: Vec<(u32, DiagnosticKind)> = map
        .check()
        .into_iter()
        .map(|diagnostic| (diagnostic.line, diagnostic.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            (
                24,
                DiagnosticKind::DuplicateKey {
                    key: b"light".to_vec()
                }
            ),
            (37, DiagnosticKind::DuplicatePlane)
        ]
    );
}

// spec MAP-5.5: every Valve 220 face is reported.
#[test]
fn valve220_faces() {
    let map = MapFile::parse(VALVE220).unwrap();
    let valve = map
        .check()
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::Valve220)
        .count();
    assert_eq!(valve, 6);
}

// spec MAP-1.6, MAP-2.4, MAP-2.10: lengths id's qbsp cannot take.
#[test]
fn lengths() {
    let key30 = "k".repeat(30);
    assert!(check(&format!("\"{key30}\" \"v\"\n}}\n")).is_empty());
    let key31 = "k".repeat(31);
    assert_eq!(
        check(&format!("\"{key31}\" \"v\"\n}}\n")),
        [(4, DiagnosticKind::KeyTooLong)]
    );
    let value = "v".repeat(127);
    assert_eq!(
        check(&format!("\"m\" \"{value}\"\n}}\n")),
        [(4, DiagnosticKind::PairTooLong)]
    );
    let value = "v".repeat(128);
    assert_eq!(
        check(&format!("\"m\" \"{value}\"\n}}\n")),
        [
            (4, DiagnosticKind::TokenTooLarge),
            (4, DiagnosticKind::PairTooLong)
        ]
    );
    let value = "v".repeat(1023);
    assert!(check(&format!("\"m\" \"{value}\"\n}}\n")).contains(&(4, DiagnosticKind::KeyTooLong)));
    let value = "v".repeat(120);
    assert!(check(&format!("\"m\" \"{value}\"\n}}\n")).is_empty());
}

// spec MAP-1.8: a `;` outside quotes and a quoted token ending in a backslash.
#[test]
fn ericw_tokens() {
    assert_eq!(check("a;b c\n}\n"), [(4, DiagnosticKind::EricwComment)]);
    assert!(check("\"a;b\" \"c\"\n}\n").is_empty());
    assert_eq!(
        check("\"a\\\" \"c\"\n}\n"),
        [(4, DiagnosticKind::EricwEscape)]
    );
    assert!(check("\"a\" \"c\\\"\n}\n").is_empty());
    assert_eq!(
        check("\"a\" \"c\\\"}\n"),
        [(4, DiagnosticKind::EricwEscape)]
    );
}

// spec MAP-2.5, MAP-2.9: duplicated keys and keys with whitespace at an end.
#[test]
fn keys() {
    assert_eq!(
        check("\"wad\" \"b.wad\"\n}\n"),
        [(
            4,
            DiagnosticKind::DuplicateKey {
                key: b"wad".to_vec()
            }
        )]
    );
    assert_eq!(
        check("\" k\" \"v\"\n}\n"),
        [(4, DiagnosticKind::KeyWhitespace)]
    );
    assert_eq!(
        check("\"k\t\" \"v\"\n}\n"),
        [(4, DiagnosticKind::KeyWhitespace)]
    );
    assert_eq!(
        check("\"k\" \"a\nb\"\n}\n"),
        [(4, DiagnosticKind::LineBreakInToken)]
    );
}

// spec MAP-2.8: the 1025th entity.
#[test]
fn entity_count() {
    let mut body = "}\n".to_owned();
    for _ in 1..1024 {
        body.push_str("{\n}\n");
    }
    assert!(check(&body).is_empty());
    body.push_str("{\n}\n");
    assert_eq!(check(&body), [(2051, DiagnosticKind::TooManyEntities)]);
}

// spec MAP-3.3, MAP-4.1, MAP-3.6, MAP-4.6: faces qbsp reads otherwise, collinear points, and
// long texture names.
#[test]
fn faces() {
    assert!(check(&format!("{{\n{FACE} w 0 0 0 1 1\n}}\n}}\n")).is_empty());
    assert!(check(&format!("{{\n{FACE} w 0 0 0 0.5 1.25\n}}\n}}\n")).is_empty());
    assert_eq!(
        check(&format!("{{\n{FACE} w 0.5 0 0 1 1\n}}\n}}\n")),
        [(5, DiagnosticKind::QbspNumber)]
    );
    assert_eq!(
        check("{\n( 0 0 0 ) ( 0 1 0 ) ( 0 0 1.5 ) w 0 0 0 1 1\n}\n}\n"),
        [(5, DiagnosticKind::QbspNumber)]
    );
    assert_eq!(
        check("{\n( 0 0 0 ) ( 1 1 1 ) ( 2 2 2 ) w 0 0 0 1 1\n}\n}\n"),
        [(5, DiagnosticKind::NoNormal)]
    );
    let name = "t".repeat(16);
    assert_eq!(
        check(&format!("{{\n{FACE} {name} 0 0 0 1 1\n}}\n}}\n")),
        [(5, DiagnosticKind::TextureNameTooLong)]
    );
    // spec MAP-4.6: a name of exactly MAP_MAX_TEXTURE_NAME (15) is accepted; the warning is
    // for names longer than the limit, so the boundary does not warn.
    let name = "t".repeat(15);
    assert!(check(&format!("{{\n{FACE} {name} 0 0 0 1 1\n}}\n}}\n")).is_empty());
    assert_eq!(
        check(&format!(
            "{{\n{FACE} w 0 0 0 1 1\n( 0 0 1 ) ( 0 1 1 ) ( 0 0 0 ) w 0 0 0 1 1\n}}\n}}\n"
        )),
        [(6, DiagnosticKind::DuplicatePlane)]
    );
}

// spec MAP-4.7: more than 4096 texture names.
#[test]
fn texture_count() {
    let mut body = "{\n".to_owned();
    for index in 0..4097 {
        body.push_str(&format!(
            "( 0 0 {index} ) ( 0 1 {index} ) ( 1 0 {index} ) t{index} 0 0 0 1 1\n"
        ));
    }
    body.push_str("}\n}\n");
    assert_eq!(check(&body), [(0, DiagnosticKind::TooManyTextures)]);
}

// spec MAP-4.7: 4097 identical faces have one qbsp texinfo, so qbsp does not overflow and no
// TooManyTextures is reported even though the face count passes 4096. This separates the
// faces-count test from qbsp actually overflowing (the two must both hold).
#[test]
fn many_duplicate_faces_do_not_overflow() {
    let mut body = "{\n".to_owned();
    for _ in 0..4097 {
        body.push_str(&format!("{FACE} shared 0 0 0 1 1\n"));
    }
    body.push_str("}\n}\n");
    assert!(
        !check(&body)
            .iter()
            .any(|(_, kind)| *kind == DiagnosticKind::TooManyTextures)
    );
}

// spec MAP-6.3, MAP-6.5, MAP-6.6: the world's WAD, message and CD track.
#[test]
fn world_keys() {
    let world = |body: &str| {
        let text = format!("{{\n{body}}}\n");
        MapFile::parse(text.as_bytes())
            .unwrap()
            .check()
            .into_iter()
            .map(|diagnostic| diagnostic.kind)
            .collect::<Vec<_>>()
    };
    assert_eq!(world(""), [DiagnosticKind::NoWad]);
    assert!(world("\"_wad\" \"x\"\n").is_empty());
    assert_eq!(
        world("\"wad\" \"x\"\n\"message\" \"100%\"\n"),
        [DiagnosticKind::MessagePercent]
    );
    assert_eq!(MAP_MAX_SOUNDS, 255.0);
    for (sounds, ok) in [
        ("0", true),
        ("255", true),
        ("2.9", true),
        ("256", false),
        ("-1", false),
    ] {
        let found = world(&format!("\"wad\" \"x\"\n\"sounds\" \"{sounds}\"\n"));
        assert_eq!(found.is_empty(), ok, "{sounds}");
        if !ok {
            assert_eq!(found, [DiagnosticKind::SoundsRange]);
        }
    }
    assert!(MapFile::new().check().is_empty());
    assert!(!DiagnosticKind::NoWad.to_string().is_empty());
}
