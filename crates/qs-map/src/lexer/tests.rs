// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap, index or panic is a failed test"
)]

use proptest::prelude::*;

use super::{
    Gap, LexError, Lexed, Lexer, LineBreak, MAP_MAX_TOKEN, Token, TokenLimit, is_map_space,
    map_text,
};
use crate::cnum::{c_atof, c_atoi, c_strtod, scan_vector};

fn tokens(text: &[u8], limit: TokenLimit) -> Vec<Token<'_>> {
    let mut lexer = Lexer::new(text, limit);
    let mut out = Vec::new();
    loop {
        match lexer.next_token().unwrap() {
            Lexed::Token(token) => out.push(token),
            Lexed::End(_) => return out,
        }
    }
}

fn texts(text: &[u8]) -> Vec<&[u8]> {
    tokens(text, TokenLimit::Qbsp)
        .into_iter()
        .map(|token| token.text)
        .collect()
}

// spec MAP-1.1: the text ends at its first NUL.
#[test]
fn nul_ends_the_text() {
    assert_eq!(map_text(b"ab\0cd"), b"ab");
    assert_eq!(map_text(b"abc"), b"abc");
    assert_eq!(texts(b"a b\0c d"), [&b"a"[..], b"b"]);
}

// spec MAP-1.2: bytes 0x01 to 0x20 and 0x80 to 0xFF separate tokens.
#[test]
fn whitespace() {
    for byte in 1..=0x20_u8 {
        assert!(is_map_space(byte));
    }
    for byte in 0x80..=0xFF_u8 {
        assert!(is_map_space(byte));
    }
    for byte in 0x21..0x80_u8 {
        assert!(!is_map_space(byte));
    }
    assert_eq!(texts(b"a\xc2\xa0b\r\nc"), [&b"a"[..], b"b", b"c"]);
}

// spec MAP-1.3: `//` comments run to the end of the line, only at the start of a token.
#[test]
fn comments() {
    assert_eq!(texts(b"// x\na // y z\nb"), [&b"a"[..], b"b"]);
    assert_eq!(texts(b"a//b /x"), [&b"a//b"[..], b"/x"]);
    assert_eq!(texts(b"a // no end"), [&b"a"[..]]);
}

// spec MAP-1.4: quoted tokens, empty or holding LFs and backslashes.
#[test]
fn quoted() {
    let found = tokens(b"\"\" \"a\nb\" \"c\\\"d", TokenLimit::Qbsp);
    assert_eq!(found[0].text, b"");
    assert!(found[0].quoted);
    assert_eq!((found[0].start, found[0].end), (0, 2));
    assert_eq!(found[1].text, b"a\nb");
    assert_eq!(found[2].text, b"c\\");
    assert_eq!(found[3].text, b"d");
    assert_eq!(found[3].line, 2);
    let mut lexer = Lexer::new(b"\n \"abc", TokenLimit::Qbsp);
    assert_eq!(lexer.next_token(), Err(LexError::EofInQuote { line: 2 }));
    assert_eq!(
        LexError::EofInQuote { line: 2 }.to_string(),
        "EOF inside quoted token"
    );
}

// spec MAP-1.5: an unquoted token runs to whitespace; brackets do not end it.
#[test]
fn unquoted() {
    assert_eq!(texts(b"(0 a{b} \"q\"r"), [&b"(0"[..], b"a{b}", b"q", b"r"]);
    assert!(
        Token {
            start: 0,
            end: 1,
            text: b"{",
            quoted: true,
            line: 1,
            gap_break: None
        }
        .is(b"{")
    );
}

// spec MAP-1.6: 127 bytes is the longest token; without the limit any length reads.
#[test]
fn token_limit() {
    assert_eq!(MAP_MAX_TOKEN, 128);
    for quote in [false, true] {
        let make = |len: usize| {
            let mut text = b"\n".to_vec();
            if quote {
                text.push(b'"');
            }
            text.extend(core::iter::repeat_n(b'x', len));
            if quote {
                text.push(b'"');
            }
            text
        };
        assert_eq!(tokens(&make(127), TokenLimit::Qbsp)[0].text.len(), 127);
        let long = make(128);
        let mut lexer = Lexer::new(&long, TokenLimit::Qbsp);
        let error = lexer.next_token().unwrap_err();
        assert_eq!(
            error,
            LexError::TokenTooLarge {
                line: 2,
                qbsp_line: 2
            }
        );
        assert_eq!(error.to_string(), "Token too large on line 2");
        assert_eq!(tokens(&make(500), TokenLimit::Unlimited)[0].text.len(), 500);
    }
}

// spec MAP-1.7, MAP-1.9: the first break in a gap and qbsp's line number for it, which skips
// LFs that end comments.
#[test]
fn line_breaks() {
    let found = tokens(b"a b\n c // x\n// y\nd", TokenLimit::Qbsp);
    assert_eq!(found[1].gap_break, None);
    assert_eq!(
        found[2].gap_break,
        Some(LineBreak {
            line: 1,
            qbsp_line: 1
        })
    );
    assert_eq!(
        found[3].gap_break,
        Some(LineBreak {
            line: 2,
            qbsp_line: 2
        })
    );
    assert_eq!(found[3].line, 4);
    let mut lexer = Lexer::new(b"a // x\n", TokenLimit::Qbsp);
    lexer.next_token().unwrap();
    assert_eq!(
        lexer.skip(),
        Gap::End(LineBreak {
            line: 1,
            qbsp_line: 1
        })
    );
    let mut lexer = Lexer::new(b"a", TokenLimit::Qbsp);
    lexer.next_token().unwrap();
    assert_eq!(lexer.offset(), 1);
    match lexer.next_token().unwrap() {
        Lexed::End(line_break) => assert_eq!(line_break.line, 1),
        Lexed::Token(token) => panic!("{token:?}"),
    }
}

// spec MAP-3.3, MAP-4.1: atoi and atof as C89 reads them.
#[test]
fn c_numbers() {
    for (text, value) in [
        (&b"12"[..], 12),
        (b" -7x", -7),
        (b"+3", 3),
        (b"0.9", 0),
        (b"-0.9", 0),
        (b"x", 0),
        (b"", 0),
        (b"99999999999", i32::MAX),
        (b"-99999999999", i32::MIN),
        (b"1e3", 1),
    ] {
        assert_eq!(c_atoi(text), value, "{}", text.escape_ascii());
    }
    for (text, value, used) in [
        (&b"1.5"[..], 1.5, 3),
        (b" -2e1x", -20.0, 5),
        (b".5", 0.5, 2),
        (b"5.", 5.0, 2),
        (b"1e", 1.0, 1),
        (b"1e+", 1.0, 1),
        (b"x", 0.0, 0),
        (b".", 0.0, 0),
        (b"-", 0.0, 0),
        (b"inf", 0.0, 0),
        (b"nan", 0.0, 0),
        (b"0x10", 0.0, 1),
    ] {
        let (parsed, length) = c_strtod(text);
        assert_eq!(
            (parsed.to_bits(), length),
            (f64::to_bits(value), used),
            "{}",
            text.escape_ascii()
        );
    }
    assert_eq!(c_atof(b"0.25"), 0.25);
    assert_eq!(scan_vector(b"1 2 3 4"), [1.0, 2.0, 3.0]);
}

proptest! {
    // spec MAP-3.3: atoi reads any int written in decimal.
    #[test]
    fn atoi_reads_ints(value in any::<i32>()) {
        prop_assert_eq!(c_atoi(value.to_string().as_bytes()), value);
    }

    // spec MAP-2.7: strtod reads any finite double Rust writes.
    #[test]
    fn strtod_reads_doubles(value in -1.0e300_f64..1.0e300) {
        let text = format!("{value}");
        prop_assert_eq!(c_strtod(text.as_bytes()), (value, text.len()));
    }

    // spec MAP-1.4, MAP-1.5: arbitrary bytes never panic the lexer, and tokens tile the text
    // in order.
    #[test]
    fn arbitrary_text(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
        let mut lexer = Lexer::new(&bytes, TokenLimit::Qbsp);
        let mut last = 0;
        while let Ok(Lexed::Token(token)) = lexer.next_token() {
            prop_assert!(token.start >= last);
            prop_assert!(token.end > token.start);
            last = token.end;
        }
    }
}
