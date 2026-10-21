// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's file loader (FTEF-14.54): the text `QCC_LoadFile` hands the lexer, and the
//! preprocessor and the project graph reading it.

use std::path::Path;

use proptest::prelude::*;
use qc_lexer::{Dialect, Span};

use super::{codes, tokens};
use crate::fte_directive::qcc_load_file;
use crate::{
    Code, FileOutput, FileStart, FteFileReader, MemoryLoader, ModuleState, Preprocessor, TextOrigin,
};

/// `text` as UTF-16LE after its byte order mark.
fn utf16le(text: &str) -> Vec<u8> {
    let mut bytes = b"\xff\xfe".to_vec();
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

/// `files` read through a loader, `a.qc` compiled under `dialect`.
fn compile(files: &[(&str, &[u8])], dialect: Dialect) -> (Preprocessor<MemoryLoader>, FileOutput) {
    let mut loader = MemoryLoader::new();
    for (path, bytes) in files {
        loader.insert(*path, bytes.to_vec());
    }
    let mut pp = Preprocessor::new(loader).with_dialect(dialect);
    let out = pp.preprocess_file(Path::new("a.qc")).unwrap();
    (pp, out)
}

/// The tokens and codes of `a.qc` of `bytes` under FTEQCC.
fn fte(bytes: &[u8]) -> (String, Vec<Code>) {
    let (_, out) = compile(&[("a.qc", bytes)], Dialect::Fteqcc);
    (tokens(&out), codes(&out))
}

/// The text the loader makes of `bytes`.
fn loaded(bytes: &[u8]) -> Vec<u8> {
    qcc_load_file(bytes.to_vec()).0
}

// spec FTEF-14.54: a file with no byte order mark is its bytes, each NUL a space
#[test]
fn a_plain_file_is_its_bytes_with_each_nul_a_space() {
    assert_eq!(
        qcc_load_file(b"a\0b\0".to_vec()),
        (b"a b ".to_vec(), TextOrigin::Shifted(0))
    );
    assert_eq!(loaded(b""), b"");
    assert_eq!(loaded(b"\xe9\xff"), b"\xe9\xff");
}

// spec FTEF-14.54: a UTF-8 byte order mark is skipped, the text standing 3 bytes into the file,
// and only a whole one
#[test]
fn a_utf8_byte_order_mark_is_skipped() {
    assert_eq!(
        qcc_load_file(b"\xef\xbb\xbfx\0".to_vec()),
        (b"x ".to_vec(), TextOrigin::Shifted(3))
    );
    assert_eq!(loaded(b"\xef\xbb\xbf"), b"");
    assert_eq!(loaded(b"\xef\xbbx"), b"\xef\xbbx");
    assert_eq!(loaded(b" \xef\xbb\xbfx"), b" \xef\xbb\xbfx");
    assert_eq!(loaded(b"\xef\xbb\xbf\xef\xbb\xbfx"), b"\xef\xbb\xbfx");
}

// spec FTEF-14.54: UTF-16 and UTF-32 after their byte order marks are decoded to UTF-8, a NUL
// among them a space, and a UTF-32LE mark, which starts as UTF-16LE's does, is UTF-32LE's
#[test]
fn wide_files_are_decoded_to_utf8() {
    let utf8 = "a\u{e9}\u{20ac}\u{1f600}".as_bytes();
    assert_eq!(loaded(&utf16le("a\u{e9}\u{20ac}\u{1f600}")), utf8);
    let mut be = b"\xfe\xff".to_vec();
    be.extend(
        "a\u{e9}\u{20ac}\u{1f600}"
            .encode_utf16()
            .flat_map(u16::to_be_bytes),
    );
    assert_eq!(loaded(&be), utf8);
    let chars = || "a\u{e9}\u{20ac}\u{1f600}".chars().map(u32::from);
    let mut le32 = b"\xff\xfe\x00\x00".to_vec();
    le32.extend(chars().flat_map(u32::to_le_bytes));
    assert_eq!(loaded(&le32), utf8);
    let mut be32 = b"\x00\x00\xfe\xff".to_vec();
    be32.extend(chars().flat_map(u32::to_be_bytes));
    assert_eq!(loaded(&be32), utf8);
    assert_eq!(loaded(b"\xff\xfe\x00\x00a\x00\x00\x00"), b"a");
    assert_eq!(loaded(b"\xff\xfea\x00\x00\x00b\x00"), b"a b");
    assert_eq!(loaded(b"\xff\xfe"), b"");
    assert_eq!(loaded(b"\xfe\xff"), b"");
}

// spec FTEF-14.54: `decodeUTF` reads whole units only, joins a high surrogate to a low one after
// it, encodes any other surrogate in three bytes, and makes a UTF-32 value past U+10FFFF U+FFFD
#[test]
fn decode_utf_keeps_its_edges() {
    assert_eq!(loaded(b"\xff\xfea\x00b"), b"a");
    assert_eq!(loaded(b"\xff\xfe\x3d\xd8\x00\xde"), "\u{1f600}".as_bytes());
    assert_eq!(loaded(b"\xff\xfe\x3d\xd8"), b"\xed\xa0\xbd");
    assert_eq!(loaded(b"\xff\xfe\x3d\xd8a\x00"), b"\xed\xa0\xbda");
    assert_eq!(loaded(b"\xff\xfe\x00\xdea\x00"), b"\xed\xb8\x80a");
    assert_eq!(
        loaded(b"\xff\xfe\x00\xde\x3d\xd8"),
        b"\xed\xb8\x80\xed\xa0\xbd"
    );
    assert_eq!(loaded(b"\xff\xfe\xff\x07"), b"\xdf\xbf");
    assert_eq!(loaded(b"\xff\xfe\x80\x00"), b"\xc2\x80");
    assert_eq!(loaded(b"\xff\xfe\x7f\x00"), b"\x7f");
    assert_eq!(
        loaded(b"\xff\xfe\x00\x00\x00\x00\x11\x00"),
        "\u{fffd}".as_bytes()
    );
    assert_eq!(
        loaded(b"\xff\xfe\x00\x00\xff\xff\x10\x00"),
        "\u{10ffff}".as_bytes()
    );
    assert_eq!(
        loaded(b"\x00\x00\xfe\xff\xff\xff\xff\xffx"),
        "\u{fffd}".as_bytes()
    );
}

// spec FTEF-14.54: `decodeUTF`'s ranges at their edges: the surrogates are 0xd800 to 0xdbff
// before 0xdc00 to 0xdfff, and UTF-8 takes one byte to U+007F, two to U+07FF, three to U+FFFF
#[test]
fn decode_utf_ranges_end_where_they_should() {
    assert_eq!(loaded(b"\xff\xfe\xff\xdb\xff\xdf"), "\u{10ffff}".as_bytes());
    assert_eq!(loaded(b"\xff\xfe\x00\xd8\x00\xdc"), "\u{10000}".as_bytes());
    assert_eq!(
        loaded(b"\xff\xfe\xff\xd7\x00\xdc"),
        b"\xed\x9f\xbf\xed\xb0\x80"
    );
    assert_eq!(
        loaded(b"\xff\xfe\xff\xdb\x00\xe0"),
        b"\xed\xaf\xbf\xee\x80\x80"
    );
    assert_eq!(
        loaded(b"\xff\xfe\x00\xdc\x00\xdc"),
        b"\xed\xb0\x80\xed\xb0\x80"
    );
    assert_eq!(loaded(b"\xff\xfe\x00\x08"), "\u{800}".as_bytes());
    assert_eq!(loaded(b"\xff\xfe\xff\xff"), "\u{ffff}".as_bytes());
    assert_eq!(
        loaded(b"\x00\x00\xfe\xff\x00\x01\x00\x00"),
        "\u{10000}".as_bytes()
    );
}

// spec FTEF-14.54: a span of a decoded text maps to the code units of the file it came from,
// and one past the text to the end of the units read
#[test]
fn a_decoded_span_maps_to_the_file() {
    let (pp, out) = compile(&[("a.qc", &utf16le("\u{e9} zz\n"))], Dialect::Fteqcc);
    let file = pp.sources().file(out.tokens[0].span.file).unwrap();
    assert_eq!(file.text.as_ref(), "\u{e9} zz\n".as_bytes());
    // `zz` is at bytes 3 to 5 of the text and code units 2 to 4 of the file, after the mark.
    assert_eq!(file.file_span(Span::new(3, 5)), Span::new(6, 10));
    assert_eq!(file.file_span(Span::new(0, 2)), Span::new(2, 4));
    assert_eq!(file.file_span(Span::new(1, 99)), Span::new(2, 12));
    let (pp, out) = compile(&[("a.qc", b"\xef\xbb\xbfzz")], Dialect::Fteqcc);
    let file = pp.sources().file(out.tokens[0].span.file).unwrap();
    assert_eq!(file.file_span(out.tokens[0].span.span), Span::new(3, 5));
    let (pp, out) = compile(&[("a.qc", b"zz")], Dialect::Vanilla);
    let file = pp.sources().file(out.tokens[0].span.file).unwrap();
    assert_eq!(file.file_span(out.tokens[0].span.span), Span::new(0, 2));
}

// spec FTEF-14.54: under FTEQCC a NUL is a space, so the text goes on past it, in a line of
// code, a directive's body and a skipped side
#[test]
fn the_preprocessor_reads_past_a_nul() {
    assert_eq!(
        fte(b"void() f = #5;\n\0zz zz zz\n").0,
        "void ( ) f = # 5 ; zz zz zz"
    );
    assert_eq!(
        fte(b"#define N #5\0zz\nvoid() f = N;\n").0,
        "void ( ) f = # 5 zz ;"
    );
    assert_eq!(
        fte(b"#ifdef Q\nzz\0\n#endif\nvoid() f = #5;\n"),
        ("void ( ) f = # 5 ;".to_owned(), vec![])
    );
}

// spec FTEF-14.54: a NUL first in a compiled file is a space `QCC_PR_NewLine` passes over, and
// first in an included file a byte before the `#`, so no directive starts there
#[test]
fn a_leading_nul_is_a_space() {
    assert_eq!(fte(b"\0#define N 5\nN\n").0, "5");
    let files: [(&str, &[u8]); 2] = [
        ("a.qc", b"#include \"b.qh\"\n"),
        ("b.qh", b"\0#define N 5\nN\n"),
    ];
    let (_, out) = compile(&files, Dialect::Fteqcc);
    assert_eq!(tokens(&out), "# define N 5 N");
}

// spec FTEF-14.54: after a UTF-8 byte order mark a directive starts the compiled file, after
// spaces too, and an included one at its first byte
#[test]
fn a_directive_follows_a_utf8_byte_order_mark() {
    assert_eq!(fte(b"\xef\xbb\xbf#define N #5\nvoid() f = N;\n").1, vec![]);
    assert_eq!(
        fte(b"\xef\xbb\xbf  #define N #5\nvoid() f = N;\n").0,
        "void ( ) f = # 5 ;"
    );
    let files: [(&str, &[u8]); 2] = [
        ("a.qc", b"#include \"b.qh\"\nN\n"),
        ("b.qh", b"\xef\xbb\xbf#define N 5\n"),
    ];
    let (_, out) = compile(&files, Dialect::Fteqcc);
    assert_eq!(tokens(&out), "5");
}

// spec FTEF-14.54: a UTF-16 file is read as its decoded text, its directives run
#[test]
fn a_utf16_file_is_preprocessed() {
    assert_eq!(
        fte(&utf16le("#define N #5\nvoid() f = N;\n")),
        ("void ( ) f = # 5 ;".to_owned(), vec![])
    );
}

// spec QC-1.1: the vanilla dialect keeps a file's bytes, its text ending at a NUL, and decodes
// no UTF-16
#[test]
fn the_vanilla_dialect_reads_the_bytes() {
    let (_, out) = compile(&[("a.qc", b"a\0b")], Dialect::Vanilla);
    assert_eq!(tokens(&out), "a");
    let (pp, out) = compile(&[("a.qc", &utf16le("a"))], Dialect::Vanilla);
    let file = pp.sources().files().first().unwrap();
    assert_eq!(file.text.as_ref(), b"\xff\xfea\x00");
    assert_eq!(file.origin, TextOrigin::Shifted(0));
    assert_eq!(tokens(&out), "a");
}

/// The `#include`s the graph's reader meets in `bytes`, a compiled file.
fn graph_includes(bytes: &[u8]) -> Vec<String> {
    let mut state = ModuleState::fteqcc([]);
    let mut reader = FteFileReader::new(bytes.to_vec(), FileStart::Compiled);
    let mut names = Vec::new();
    while let Some(event) = reader.next(&mut state) {
        if let crate::FteEvent::Include { name, line } = event {
            names.push(format!("{} @{line}", String::from_utf8_lossy(&name)));
        }
    }
    names
}

// spec FTEF-14.54: the project graph reads past a NUL, after a UTF-8 byte order mark and a UTF-16
// file's decoded text, as the preprocessor does (PRJ-5.13)
#[test]
fn the_graph_reads_the_loaded_text() {
    assert_eq!(
        graph_includes(b"float g;\0\n#include \"b.qh\"\n"),
        ["b.qh @2"]
    );
    assert_eq!(
        graph_includes(b"#ifdef Q\nzz\0\n#endif\n#include \"b.qh\"\n"),
        ["b.qh @4"]
    );
    assert_eq!(
        graph_includes(b"\xef\xbb\xbf#include \"b.qh\"\n"),
        ["b.qh @1"]
    );
    assert_eq!(
        graph_includes(&utf16le("\n#include \"b.qh\"\n")),
        ["b.qh @2"]
    );
    assert_eq!(graph_includes(b"\0#include \"b.qh\"\n"), ["b.qh @1"]);
}

/// `text` after `mark`, each code point as UTF-32 or UTF-16 code units, little- or big-endian.
fn wide(mark: &[u8], text: &str, utf32: bool, little: bool) -> Vec<u8> {
    let mut bytes = mark.to_vec();
    if utf32 {
        for c in text.chars().map(u32::from) {
            bytes.extend(if little {
                c.to_le_bytes()
            } else {
                c.to_be_bytes()
            });
        }
    } else {
        for u in text.encode_utf16() {
            bytes.extend(if little {
                u.to_le_bytes()
            } else {
                u.to_be_bytes()
            });
        }
    }
    bytes
}

proptest! {
    // spec FTEF-14.54: any text written in UTF-16 or UTF-32 after its mark decodes to its UTF-8,
    // each NUL a space, each byte mapped in order to a code unit of the file
    #[test]
    fn wide_text_decodes_to_its_utf8(text in "\\PC*") {
        let want: Vec<u8> = text.bytes().map(|b| if b == 0 { b' ' } else { b }).collect();
        let marks: [(&[u8], bool, bool); 4] = [
            (b"\xff\xfe\x00\x00", true, true),
            (b"\x00\x00\xfe\xff", true, false),
            (b"\xff\xfe", false, true),
            (b"\xfe\xff", false, false),
        ];
        for (mark, utf32, little) in marks {
            // A UTF-16LE NUL right after the mark makes the UTF-32LE mark.
            if !utf32 && little && text.starts_with('\0') {
                continue;
            }
            let bytes = wide(mark, &text, utf32, little);
            let (loaded, origin) = qcc_load_file(bytes.clone());
            prop_assert_eq!(&loaded, &want);
            let TextOrigin::Decoded(offsets) = origin else {
                return Err(TestCaseError::fail("not decoded"));
            };
            prop_assert_eq!(offsets.len(), loaded.len() + 1);
            prop_assert!(offsets.windows(2).all(|w| w[0] <= w[1]));
            prop_assert_eq!(offsets.last().copied(), Some(bytes.len()));
        }
    }

    // spec FTEF-14.54: any bytes load, with no NUL left and a file offset for every byte of
    // the text
    #[test]
    fn any_bytes_load(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        let (loaded, origin) = qcc_load_file(bytes.clone());
        prop_assert!(!loaded.contains(&0));
        match origin {
            TextOrigin::Shifted(skipped) => {
                prop_assert_eq!(loaded.len() + skipped, bytes.len());
            }
            TextOrigin::Decoded(offsets) => {
                prop_assert_eq!(offsets.len(), loaded.len() + 1);
                prop_assert!(offsets.iter().all(|&o| o <= bytes.len()));
            }
        }
    }
}
