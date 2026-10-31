// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test code: a failed index, unwrap or panic is a failed test"
)]

use std::fmt::Write as _;

use proptest::prelude::*;

use super::{Code, LineIndex, Punct, QC_MAX_TOKEN, Span, TokenKind, atof, lex};
use super::{Dialect, FTEQCC_MAX_WORD, lex_with};

/// The token kinds a dialect lexer produces.
fn kinds_in(source: &[u8], dialect: Dialect) -> Vec<TokenKind> {
    lex_with(source, dialect)
        .tokens
        .into_iter()
        .map(|t| t.kind)
        .collect()
}

/// A snapshot of a lex: one line per token, then one per diagnostic.
fn snapshot(source: &[u8]) -> String {
    let lexed = lex(source);
    let mut out = String::new();
    for token in &lexed.tokens {
        let kind = match &token.kind {
            TokenKind::Name(name) => format!("name {name}"),
            TokenKind::Float(value) => format!("float {value}"),
            TokenKind::Vector([x, y, z]) => format!("vector {x} {y} {z}"),
            TokenKind::String(bytes) => format!("string {}", bytes.escape_ascii()),
            TokenKind::Punct(punct) => format!("punct {}", punct.as_str()),
            TokenKind::FrameRef(word) => format!("frameref {word}"),
            TokenKind::Frames(names) => {
                let names: Vec<&str> = names.iter().map(|n| n.name.as_str()).collect();
                format!("frames {}", names.join(" "))
            }
            TokenKind::FrameCommand(command) => format!("command {command:?}"),
        };
        let start = if token.line_start { " ^" } else { "" };
        writeln!(
            out,
            "{kind} @{}..{}{start}",
            token.span.start, token.span.end
        )
        .unwrap_or(());
    }
    for d in &lexed.diagnostics {
        writeln!(
            out,
            "{} {} @{}..{}",
            d.code.as_str(),
            d.message,
            d.span.start,
            d.span.end
        )
        .unwrap_or(());
    }
    out
}

fn codes(source: &[u8]) -> Vec<Code> {
    lex(source).diagnostics.iter().map(|d| d.code).collect()
}

fn kinds(source: &[u8]) -> Vec<TokenKind> {
    lex(source).tokens.into_iter().map(|t| t.kind).collect()
}

fn name(s: &str) -> TokenKind {
    TokenKind::Name(s.to_owned())
}

fn punct(p: Punct) -> TokenKind {
    TokenKind::Punct(p)
}

// spec QC-1.1: the text ends at the first NUL byte
#[test]
fn text_ends_at_nul() {
    assert_eq!(kinds(b"a\0b c"), vec![name("a")]);
    assert_eq!(kinds(b"\0a"), vec![]);
}

// spec QC-1.2: lines count LF bytes; CR is whitespace, so CRLF and LF agree
#[test]
fn lines_count_lf_only() {
    let crlf = b"a\r\nb\r\n\rc";
    let lf = b"a\nb\nc";
    let index = LineIndex::new(crlf);
    let lines: Vec<u32> = lex(crlf)
        .tokens
        .iter()
        .map(|t| index.line(t.span.start))
        .collect();
    assert_eq!(lines, vec![1, 2, 3]);
    let index = LineIndex::new(lf);
    let lines: Vec<u32> = lex(lf)
        .tokens
        .iter()
        .map(|t| index.line(t.span.start))
        .collect();
    assert_eq!(lines, vec![1, 2, 3]);
    assert_eq!(index.line(0), 1);
    assert_eq!(index.line(1), 1, "the LF itself is on the line it ends");
    assert_eq!(index.line(2), 2);
}

// spec QC-1.3: bytes 0x80 to 0xFF outside a string are whitespace
#[test]
fn high_bytes_are_whitespace() {
    assert_eq!(
        kinds(b"float\xa0a;"),
        vec![name("float"), name("a"), punct(Punct::Semicolon)]
    );
    assert_eq!(kinds(b"a\xffb"), vec![name("a"), name("b")]);
}

// spec QC-1.4: bytes 0x80 to 0xFF inside a string are copied unchanged
#[test]
fn high_bytes_in_strings_are_kept() {
    assert_eq!(
        kinds(b"\"\xe9\x80\""),
        vec![TokenKind::String(vec![0xe9, 0x80])]
    );
}

// spec QC-2.1: bytes 0x01 to 0x20 separate tokens and are otherwise ignored
#[test]
fn control_bytes_are_whitespace() {
    assert_eq!(
        kinds(b"a\x01b\x1fc d\te\x0bf"),
        ["a", "b", "c", "d", "e", "f"].map(name)
    );
}

// spec QC-2.2: `//` runs to the end of the line; with no LF it ends the file
#[test]
fn line_comments() {
    assert_eq!(kinds(b"a // b c\nd"), vec![name("a"), name("d")]);
    assert_eq!(
        kinds(b"float a;\n// abc"),
        vec![name("float"), name("a"), punct(Punct::Semicolon)]
    );
    assert!(codes(b"float a;\n// abc").is_empty());
}

// spec QC-2.3: a `/*` comment ends at the first `*/` from its `*`; `/*/` is whole, no nesting
#[test]
fn block_comments() {
    assert_eq!(
        snapshot(b"/*/ float x;\nfloat y;"),
        "name float @4..9\nname x @10..11\npunct ; @11..12\nname float @13..18 ^\nname y @19..20\npunct ; @20..21\n"
    );
    assert_eq!(
        kinds(b"a /* /* */ b */"),
        vec![
            name("a"),
            name("b"),
            punct(Punct::Star),
            punct(Punct::Slash)
        ]
    );
    assert_eq!(kinds(b"a/**/b"), vec![name("a"), name("b")]);
}

// spec QC-2.4, QC-27.2, QCD-6.1: an unterminated `/*` is an error at the `/*`
#[test]
fn unterminated_block_comment() {
    assert_eq!(
        snapshot(b"float a;\n/* abc"),
        "name float @0..5 ^\nname a @6..7\npunct ; @7..8\nE0501 unterminated comment @9..11\n"
    );
    assert_eq!(codes(b"/*"), vec![Code::E0501]);
    assert_eq!(codes(b"/*/"), vec![]);
}

// spec QC-2.5: line breaks inside comments count towards line numbers
#[test]
fn comments_keep_line_numbers() {
    let source = b"/* a\nb\n*/ x // y\nz";
    let index = LineIndex::new(source);
    let lines: Vec<u32> = lex(source)
        .tokens
        .iter()
        .map(|t| index.line(t.span.start))
        .collect();
    assert_eq!(lines, vec![3, 4]);
}

// spec QC-2.6: comments may stand between tokens and vector components; in strings they are text
#[test]
fn comments_between_tokens_not_in_strings() {
    assert_eq!(kinds(b"a/*x*/b//y\nc"), ["a", "b", "c"].map(name));
    assert_eq!(
        kinds(b"'1 /* x */ 2 // y\n 3'"),
        vec![TokenKind::Vector([1.0, 2.0, 3.0])]
    );
    assert_eq!(
        kinds(b"\"a // b /* c\""),
        vec![TokenKind::String(b"a // b /* c".to_vec())]
    );
}

// spec QC-3.1: a name starts with a letter or `_`, continues with letters, digits and `_`
#[test]
fn names() {
    assert_eq!(
        kinds(b"_a1 B_2 a-b 1a"),
        vec![
            name("_a1"),
            name("B_2"),
            name("a"),
            punct(Punct::Minus),
            name("b"),
            TokenKind::Float(1.0),
            name("a"),
        ]
    );
    assert_eq!(kinds(b"Abc abc"), vec![name("Abc"), name("abc")]);
}

// spec QC-3.5: `for`, `break`, `int`, `const` and the rest are plain names
#[test]
fn non_vanilla_words_are_names() {
    let words = [
        "for", "break", "continue", "switch", "case", "goto", "int", "const", "do",
    ];
    assert_eq!(kinds(words.join(" ").as_bytes()), words.map(name));
}

// spec QC-4.1: a float literal is a digit, or `-` and a digit, then digits and dots; atof
#[test]
fn float_literals() {
    assert_eq!(
        kinds(b"0 12 3.5 -7 -0.25 5."),
        vec![
            TokenKind::Float(0.0),
            TokenKind::Float(12.0),
            TokenKind::Float(3.5),
            TokenKind::Float(-7.0),
            TokenKind::Float(-0.25),
            TokenKind::Float(5.0),
        ]
    );
    assert_eq!(kinds(b"0.1"), vec![TokenKind::Float(0.1_f32)]);
    assert_eq!(atof(b"16777217"), 16_777_216.0, "rounded to 32 bits");
    assert_eq!(atof(b" \t+2"), 2.0);
    assert_eq!(atof(b"-"), 0.0);
    assert!(
        atof(b"-").is_sign_positive(),
        "no digits is +0, as strtod returns"
    );
    assert!(atof(b"-0").is_sign_negative());
    assert_eq!(atof(b"."), 0.0);
    assert_eq!(atof(b"-.5"), -0.5);
    assert_eq!(atof(b"x5"), 0.0);
    assert_eq!(atof(b"1e40"), 1.0);
    assert!(
        atof(b"9".repeat(40).as_slice()).is_infinite(),
        "too large for f32"
    );
}

// spec QC-4.2: `-` then a digit is always a negative literal, whatever precedes it
#[test]
fn minus_digit_is_negative_literal() {
    assert_eq!(kinds(b"b-5"), vec![name("b"), TokenKind::Float(-5.0)]);
    assert_eq!(
        kinds(b"b - 5"),
        vec![name("b"), punct(Punct::Minus), TokenKind::Float(5.0)]
    );
    assert_eq!(
        kinds(b"b - -5"),
        vec![name("b"), punct(Punct::Minus), TokenKind::Float(-5.0)]
    );
    assert_eq!(
        kinds(b"- 5"),
        vec![punct(Punct::Minus), TokenKind::Float(5.0)]
    );
}

// spec QC-4.3: atof stops at the second `.`, so `1.2.3` is one token valued 1.2
#[test]
fn second_dot_ends_value_not_token() {
    assert_eq!(snapshot(b"1.2.3;"), "float 1.2 @0..5 ^\npunct ; @5..6\n");
}

// spec QC-4.4: no exponents, hex or suffixes; a literal cannot start with `.`
#[test]
fn no_exponent_hex_or_leading_dot() {
    assert_eq!(kinds(b"1e5"), vec![TokenKind::Float(1.0), name("e5")]);
    assert_eq!(kinds(b"0x10"), vec![TokenKind::Float(0.0), name("x10")]);
    assert_eq!(kinds(b"2f"), vec![TokenKind::Float(2.0), name("f")]);
    assert_eq!(kinds(b".5"), vec![punct(Punct::Dot), TokenKind::Float(5.0)]);
}

// spec QC-5.1: a vector is `'`, three numbers, `'`; each component's first byte is taken as is
#[test]
fn vector_literals() {
    assert_eq!(
        kinds(b"'-1 -2.5 3'"),
        vec![TokenKind::Vector([-1.0, -2.5, 3.0])]
    );
    assert_eq!(kinds(b"'+1 x 3'"), vec![TokenKind::Vector([1.0, 0.0, 3.0])]);
    assert_eq!(kinds(b"' 1 2 3'"), vec![TokenKind::Vector([1.0, 2.0, 3.0])]);
    assert_eq!(
        snapshot(b"'1 2 3';"),
        "vector 1 2 3 @0..7 ^\npunct ; @7..8\n"
    );
}

// spec QC-5.2, QCD-2.4: a byte other than `'` after the third component is "Bad vector"
#[test]
fn bad_vector() {
    assert_eq!(codes(b"'1 2 3 4'"), vec![Code::E0104, Code::E0104]);
    assert_eq!(
        snapshot(b"'1 2"),
        "vector 1 2 0 @0..4 ^\nE0104 Bad vector @0..4\n"
    );
    assert_eq!(codes(b"'"), vec![Code::E0104]);
    // Two blanks make the first component a lone blank, so the components run one early.
    assert_eq!(codes(b"'  1 2 3'"), vec![Code::E0104, Code::E0104]);
}

// spec QC-5.3: line breaks and comments may sit between components and before the quote
#[test]
fn vector_spans_lines() {
    let source = b"'1\n2 /* c\n */ 3\n'\nx";
    let index = LineIndex::new(source);
    let tokens = lex(source).tokens;
    assert_eq!(tokens[0].kind, TokenKind::Vector([1.0, 2.0, 3.0]));
    assert_eq!(index.line(tokens[1].span.start), 5);
}

// spec QC-6.1: a string literal is `"`, its contents, `"`; adjacent literals stay separate
#[test]
fn string_literals() {
    assert_eq!(
        snapshot(b"\"ab\" \"c\""),
        "string ab @0..4 ^\nstring c @5..8\n"
    );
    assert_eq!(kinds(b"\"\""), vec![TokenKind::String(Vec::new())]);
}

// spec QC-6.2, QCD-2.3: `\n` and `\"` are the only escapes; anything else is an error
#[test]
fn string_escapes() {
    assert_eq!(
        kinds(b"\"a\\\"b\\n\""),
        vec![TokenKind::String(b"a\"b\n".to_vec())]
    );
    assert_eq!(
        snapshot(b"\"a\\\\b\""),
        "string ab @0..6 ^\nE0103 Unknown escape char @2..4\n"
    );
    assert_eq!(codes(b"\"\\t\""), vec![Code::E0103]);
    // FTEQCC's bronze toggles `\s` and `\b` (FTEF-13.10) are unknown here and toggle nothing.
    for source in [&b"\"a\\sb\""[..], b"\"a\\bb\""] {
        assert_eq!(
            snapshot(source),
            "string ab @0..6 ^\nE0103 Unknown escape char @2..4\n",
            "{source:?}"
        );
    }
}

// spec QC-6.3, QCD-2.1, QCD-2.2: a line break inside is "newline inside quote", EOF "EOF inside quote"
#[test]
fn unterminated_strings() {
    assert_eq!(
        snapshot(b"\"a\nb\""),
        "string a @0..2 ^\nname b @3..4 ^\nstring  @4..5\nE0102 newline inside quote @0..2\nE0101 EOF inside quote @4..5\n"
    );
    assert_eq!(codes(b"\"ab"), vec![Code::E0101]);
    assert_eq!(codes(b"\"ab\\"), vec![Code::E0101]);
    assert_eq!(
        codes(b"\"a\\\nb\""),
        vec![Code::E0103, Code::E0102, Code::E0101]
    );
}

// spec QC-6.4, QC-27.4, QCD-6.2: contents longer than QC_MAX_TOKEN - 1 bytes are an error
#[test]
fn long_tokens() {
    let fits = "a".repeat(QC_MAX_TOKEN - 1);
    let over = "a".repeat(QC_MAX_TOKEN);
    assert!(codes(format!("\"{fits}\"").as_bytes()).is_empty());
    assert_eq!(codes(format!("\"{over}\"").as_bytes()), vec![Code::E0502]);
    assert!(codes(fits.as_bytes()).is_empty());
    assert_eq!(codes(over.as_bytes()), vec![Code::E0502]);
    assert_eq!(
        codes(format!("1{}", "0".repeat(QC_MAX_TOKEN - 1)).as_bytes()),
        vec![Code::E0502]
    );
    assert_eq!(codes(format!("${over}").as_bytes()), vec![Code::E0502]);
    assert_eq!(
        codes(format!("$frame {over}").as_bytes()),
        vec![Code::E0502, Code::W0101]
    );
}

// spec QC-7.1: punctuation is tried in id's order; the first match wins
#[test]
fn punctuation() {
    let all: Vec<&str> = Punct::ALL.iter().map(|p| p.as_str()).collect();
    assert_eq!(kinds(all.join(" ").as_bytes()), Punct::ALL.map(punct));
    assert_eq!(
        kinds(b"<==>=!=!"),
        vec![
            punct(Punct::LessEq),
            punct(Punct::Eq),
            punct(Punct::GreaterEq),
            punct(Punct::NotEq),
            punct(Punct::Not),
        ]
    );
    assert_eq!(
        kinds(b"....&&&|||"),
        vec![
            punct(Punct::Ellipsis),
            punct(Punct::Dot),
            punct(Punct::AndAnd),
            punct(Punct::And),
            punct(Punct::OrOr),
            punct(Punct::Or),
        ]
    );
}

// spec QC-7.2, QCD-2.5: any other byte is "Unknown punctuation"
#[test]
fn unknown_punctuation() {
    for b in *b"%^~?:@\\`" {
        let source = [b'a', b, b'b'];
        assert_eq!(codes(&source), vec![Code::E0105], "byte {b:#x}");
        assert_eq!(
            kinds(&source),
            vec![name("a"), name("b")],
            "byte {b:#x} is skipped"
        );
    }
    assert_eq!(
        snapshot(b"a%b"),
        "name a @0..1 ^\nname b @2..3\nE0105 Unknown punctuation @1..2\n"
    );
}

// spec QC-7.4: no compound assignment, increment or decrement tokens exist
#[test]
fn no_compound_operators() {
    assert_eq!(
        kinds(b"++ -- += -="),
        vec![
            punct(Punct::Plus),
            punct(Punct::Plus),
            punct(Punct::Minus),
            punct(Punct::Minus),
            punct(Punct::Plus),
            punct(Punct::Eq),
            punct(Punct::Minus),
            punct(Punct::Eq),
        ]
    );
}

// spec QC-8.1, QCD-2.7: `$` and a word up to whitespace, `,` or `;`; `$` alone is "hanging $"
#[test]
fn dollar_words() {
    assert_eq!(
        kinds(b"$ \t walk1,"),
        vec![TokenKind::FrameRef("walk1".to_owned()), punct(Punct::Comma),]
    );
    assert_eq!(
        snapshot(b"x = $  \nfoo"),
        "name x @0..1 ^\npunct = @2..3\nname foo @8..11 ^\nE0107 hanging $ @4..7\n"
    );
    assert_eq!(codes(b"$"), vec![Code::E0107]);
    assert_eq!(
        kinds(b"$;"),
        vec![TokenKind::FrameRef(String::new()), punct(Punct::Semicolon)]
    );
}

// spec QC-8.2: `$frame` takes every word after it on its line
#[test]
fn frame_lines() {
    assert_eq!(
        snapshot(b"$frame a b\tc\nd"),
        "frames a b c @0..12 ^\nname d @13..14 ^\n"
    );
    assert_eq!(snapshot(b"$ frame"), "frames  @0..7 ^\n");
}

// spec QC-8.4: `$cd`, `$origin`, `$base`, `$flags`, `$scale` and `$skin` go with their line
#[test]
fn ignored_commands() {
    let source =
        b"$cd /a/b/c\n$origin 0 0 24\n$base base\n$skin skin\n$flags 8\n$scale 1\nfloat a;";
    assert_eq!(
        kinds(source),
        vec![name("float"), name("a"), punct(Punct::Semicolon)]
    );
    assert!(codes(source).is_empty());
}

// spec QC-27.14: a `,` or `;` on an ignored command's line goes with the rest of it
#[test]
fn ignored_commands_take_commas() {
    assert_eq!(kinds(b"$cd a,b;c\nx"), vec![name("x")]);
    assert!(codes(b"$cd a,b;c\nx").is_empty());
}

// spec QC-8.6: a reference ends only at whitespace, `,` or `;`
#[test]
fn frame_ref_takes_following_bytes() {
    assert_eq!(
        kinds(b"($b)"),
        vec![punct(Punct::LParen), TokenKind::FrameRef("b)".to_owned())]
    );
}

// spec QC-27.5, QCD-6.3: a `,` or `;` on a `$frame` line is an error
#[test]
fn comma_in_frame_list() {
    assert_eq!(
        snapshot(b"$frame a, b;c"),
        "frames a b c @0..13 ^\nE0503 `,` or `;` in a $frame list @8..9\nE0503 `,` or `;` in a $frame list @11..12\n"
    );
}

// spec QC-27.6, QCD-7.1: a frame name over 15 bytes is kept, with a warning
#[test]
fn long_frame_name_warns() {
    assert!(codes(b"$frame abcdefghijklmno").is_empty());
    let lexed = lex(b"$frame abcdefghijklmnop");
    assert_eq!(lexed.diagnostics.len(), 1);
    assert_eq!(lexed.diagnostics[0].code, Code::W0101);
    assert!(!lexed.diagnostics[0].code.is_error());
    assert_eq!(lexed.diagnostics[0].span, Span::new(7, 23));
    assert!(matches!(&lexed.tokens[0].kind, TokenKind::Frames(n) if n[0].name.len() == 16));
}

// spec QC-27.1: an error in the first token of a file is an ordinary error
#[test]
fn error_in_first_token() {
    assert_eq!(
        snapshot(b"%float a;"),
        "name float @1..6\nname a @7..8\npunct ; @8..9\nE0105 Unknown punctuation @0..1\n"
    );
}

#[test]
fn line_start_flag() {
    let tokens = lex(b"  # a\n/* x */ #b\n\t\xa0#").tokens;
    let flags: Vec<bool> = tokens.iter().map(|t| t.line_start).collect();
    assert_eq!(flags, vec![true, false, false, false, true]);
}

#[test]
fn span_helpers() {
    let span = Span::new(2, 5);
    assert_eq!(span.range(), 2..5);
    assert!(span.contains(Span::new(2, 5)));
    assert!(span.contains(Span::new(3, 4)));
    assert!(!span.contains(Span::new(1, 4)));
    assert!(!span.contains(Span::new(3, 6)));
}

// Adversarial tests added by the P1-R2 readiness review (review-55). The reviewer did not
// write this crate; these probe edges found while reading the lexer against the spec.

// spec QC-8.9: id's qcc loops forever on a `,` or `;` in a $frame list; this lexer stops,
// so a $frame line ending in a run of separators terminates with one E0503 for each.
#[test]
fn reviewer_frame_list_separators_terminate() {
    // A trailing run of `,;,;` would hang id's qcc; here lexing ends with the file.
    let lexed = lex(b"$frame a ,;,; b\nfloat c;");
    // The frame token is produced, then the `float c ;` line parses normally.
    assert!(matches!(&lexed.tokens[0].kind, TokenKind::Frames(_)));
    assert_eq!(
        kinds(b"$frame a ,;,; b\nfloat c;")[1..],
        vec![name("float"), name("c"), punct(Punct::Semicolon)]
    );
    // One E0503 per separator byte, and nothing else.
    assert_eq!(
        codes(b"$frame a ,;,; b"),
        vec![Code::E0503, Code::E0503, Code::E0503, Code::E0503]
    );
    // A $frame line that is nothing but separators still terminates.
    assert_eq!(codes(b"$frame ,,,;;;"), vec![Code::E0503; 6]);
}

// spec QC-27.6: a 15-byte frame name is kept silently; 16 bytes is the first that warns,
// the exact boundary of QC_MAX_FRAME_NAME.
#[test]
fn reviewer_frame_name_length_boundary() {
    let fifteen = "a".repeat(15);
    let sixteen = "a".repeat(16);
    assert!(codes(format!("$frame {fifteen}").as_bytes()).is_empty());
    assert_eq!(
        codes(format!("$frame {sixteen}").as_bytes()),
        vec![Code::W0101]
    );
}

// spec QC-5.1: a vector literal with fewer than three components leaves the missing ones
// zero, and the missing closing quote is E0104, without reading past end of file.
#[test]
fn reviewer_short_vector_is_zero_filled() {
    assert_eq!(kinds(b"'1 2"), vec![TokenKind::Vector([1.0, 2.0, 0.0])]);
    assert_eq!(codes(b"'1 2"), vec![Code::E0104]);
    // A bare quote at end of file is a zero vector with one Bad vector.
    assert_eq!(kinds(b"'"), vec![TokenKind::Vector([0.0, 0.0, 0.0])]);
    assert_eq!(codes(b"'"), vec![Code::E0104]);
}

// spec QC-27.4: the token-too-long boundary is exact: QC_MAX_TOKEN - 1 name bytes are fine,
// QC_MAX_TOKEN bytes are E0502, for a name as well as for a string.
#[test]
fn reviewer_name_length_boundary_is_exact() {
    let fits = "a".repeat(QC_MAX_TOKEN - 1);
    let over = "a".repeat(QC_MAX_TOKEN);
    assert!(codes(fits.as_bytes()).is_empty());
    assert_eq!(codes(over.as_bytes()), vec![Code::E0502]);
    // The token is still produced in full, not truncated.
    assert_eq!(kinds(over.as_bytes()), vec![name(&over)]);
}

// spec QC-2.3: `/*` finds its end at the first `*/` from the byte after the `/`, so `/**/`
// is an empty comment, `/*/` is a whole comment (the `*/` is found at once), and a `/*` with
// no later `*/` is unterminated (E0501).
#[test]
fn reviewer_block_comment_boundaries() {
    assert!(kinds(b"/**/x").iter().eq([name("x")].iter()));
    assert!(codes(b"/**/x").is_empty());
    // `/*/` is a whole comment: the search for `*/` starts one byte past the first `/`,
    // so `/*/x` closes the comment at once and leaves `x`.
    assert_eq!(kinds(b"/*/x"), vec![name("x")]);
    assert!(codes(b"/*/x").is_empty());
    // `/*` with no closing `*/` anywhere is unterminated.
    assert_eq!(codes(b"/* x"), vec![Code::E0501]);
    // A comment that ends exactly at end of file with `*/`.
    assert!(codes(b"/* done */").is_empty());
}

// spec QC-7.1: the lexer tries multi-byte punctuation before single bytes, so a lone `&`
// or `|` is And/Or but `&&`/`||` win when they match, and `...` beats `.`.
#[test]
fn reviewer_punctuation_maximal_munch() {
    assert_eq!(kinds(b"&"), vec![punct(Punct::And)]);
    assert_eq!(kinds(b"&&"), vec![punct(Punct::AndAnd)]);
    assert_eq!(kinds(b"&&&"), vec![punct(Punct::AndAnd), punct(Punct::And)]);
    assert_eq!(kinds(b"..."), vec![punct(Punct::Ellipsis)]);
    assert_eq!(
        kinds(b"...."),
        vec![punct(Punct::Ellipsis), punct(Punct::Dot)]
    );
    assert_eq!(kinds(b".."), vec![punct(Punct::Dot), punct(Punct::Dot)]);
}

// spec FTE-5.1: `++` and `--` lex as single FTEQCC punctuators in an FTEQCC dialect.
// spec FTE-6.1: `+=`, `-=`, `*=` and `/=` lex as single compound-assignment punctuators.
// spec FTE-7.1: `%` lexes as the modulo punctuator.
// spec FTE-8.1: `?` and `:` lex as the ternary punctuators.
#[test]
fn fteqcc_punctuators_lex_in_fteqcc_dialect() {
    assert_eq!(
        kinds_in(b"i++", Dialect::Fteqcc),
        vec![name("i"), punct(Punct::PlusPlus)]
    );
    assert_eq!(
        kinds_in(b"i--", Dialect::Fteqcc),
        vec![name("i"), punct(Punct::MinusMinus)]
    );
    assert_eq!(
        kinds_in(b"x += y", Dialect::Fteqcc),
        vec![name("x"), punct(Punct::PlusEq), name("y")]
    );
    assert_eq!(
        kinds_in(b"x -= y", Dialect::Fteqcc),
        vec![name("x"), punct(Punct::MinusEq), name("y")]
    );
    assert_eq!(
        kinds_in(b"x *= y", Dialect::Fteqcc),
        vec![name("x"), punct(Punct::StarEq), name("y")]
    );
    assert_eq!(
        kinds_in(b"x /= y", Dialect::Fteqcc),
        vec![name("x"), punct(Punct::SlashEq), name("y")]
    );
    assert_eq!(
        kinds_in(b"a % b", Dialect::Fteqcc),
        vec![name("a"), punct(Punct::Percent), name("b")]
    );
    assert_eq!(
        kinds_in(b"a ? b : c", Dialect::Fteqcc),
        vec![
            name("a"),
            punct(Punct::Question),
            name("b"),
            punct(Punct::Colon),
            name("c"),
        ]
    );
}

// spec FTE-6.1: a compound FTEQCC operator wins the longest match over the vanilla operator
// it extends, so `++=` is `++` then `=` and two separated `+` stay separate.
#[test]
fn fteqcc_punctuators_take_the_longest_match() {
    assert_eq!(
        kinds_in(b"++=", Dialect::Fteqcc),
        vec![punct(Punct::PlusPlus), punct(Punct::Eq)]
    );
    assert_eq!(
        kinds_in(b"+ +", Dialect::Fteqcc),
        vec![punct(Punct::Plus), punct(Punct::Plus)]
    );
}

// spec FTE-7.1: vanilla has no `%`, `?` or `:`: each is "Unknown punctuation" (QC-7.2), and
// vanilla output is unchanged by the FTEQCC lexer additions (byte for byte).
#[test]
fn vanilla_lexing_is_unchanged_by_the_dialect_additions() {
    // The FTEQCC bytes are vanilla "Unknown punctuation" and are skipped (QC-7.2).
    assert_eq!(codes(b"a % b"), vec![Code::E0105]);
    assert_eq!(codes(b"a ? b : c"), vec![Code::E0105, Code::E0105]);
    // `i++` in vanilla is `i`, `+`, `+`: two separate Plus tokens, not one `++`.
    assert_eq!(
        kinds(b"i++"),
        vec![name("i"), punct(Punct::Plus), punct(Punct::Plus)]
    );
    // `x += y` in vanilla is `x`, `+`, `=`, `y`.
    assert_eq!(
        kinds(b"x += y"),
        vec![name("x"), punct(Punct::Plus), punct(Punct::Eq), name("y")]
    );
    // lex() and lex_with(.., Vanilla) agree.
    assert_eq!(
        kinds(b"i++ x+=y a%b?c:d"),
        kinds_in(b"i++ x+=y a%b?c:d", Dialect::Vanilla)
    );
}

// spec FTE-5.1: a negative-number token is still a single float, so `--`/`-=` lexing does
// not change how `-1` lexes.
#[test]
fn negative_numbers_still_lex_as_floats_in_fteqcc() {
    assert_eq!(
        kinds_in(b"-1", Dialect::Fteqcc),
        vec![TokenKind::Float(-1.0)]
    );
    assert_eq!(
        kinds_in(b"x - 1", Dialect::Fteqcc),
        vec![name("x"), punct(Punct::Minus), TokenKind::Float(1.0)]
    );
}

// spec FTE-19.1: the further FTEQCC punctuators each lex as one token, the longest first.
#[test]
fn further_fteqcc_punctuators_lex() {
    let cases: [(&[u8], Punct); 13] = [
        (b"->", Punct::Arrow),
        (b"~", Punct::Tilde),
        (b"^", Punct::Caret),
        (b"<<", Punct::Shl),
        (b">>", Punct::Shr),
        (b"&=", Punct::AndEq),
        (b"|=", Punct::OrEq),
        (b"^=", Punct::CaretEq),
        (b"&~=", Punct::AndNotEq),
        (b"<<=", Punct::ShlEq),
        (b">>=", Punct::ShrEq),
        (b"(+)", Punct::SetBits),
        (b"(-)", Punct::ClearBits),
    ];
    for (source, expected) in cases {
        assert_eq!(kinds_in(source, Dialect::Fteqcc), vec![punct(expected)]);
        assert_eq!(expected.as_str().as_bytes(), source);
    }
    // The vanilla tokens they extend still lex as before.
    assert_eq!(
        kinds_in(b"< <= & | ( - )", Dialect::Fteqcc),
        vec![
            punct(Punct::Less),
            punct(Punct::LessEq),
            punct(Punct::And),
            punct(Punct::Or),
            punct(Punct::LParen),
            punct(Punct::Minus),
            punct(Punct::RParen),
        ]
    );
    // `(-1)` is a parenthesised number, not `(-)`.
    assert_eq!(
        kinds_in(b"(-1)", Dialect::Fteqcc),
        vec![
            punct(Punct::LParen),
            TokenKind::Float(-1.0),
            punct(Punct::RParen)
        ]
    );
    // In vanilla `~` and `^` are "Unknown punctuation" (QC-7.2), and `->` is `-` and `>`.
    assert_eq!(codes(b"~a ^ b"), vec![Code::E0105, Code::E0105]);
    assert_eq!(
        kinds(b"e->f"),
        vec![
            name("e"),
            punct(Punct::Minus),
            punct(Punct::Greater),
            name("f")
        ]
    );
}

// spec FTE-19.2: `(+)` and `(-)` are single tokens, which the parser reads as `|=` and `&~=`.
#[test]
fn set_and_clear_bits_are_single_tokens() {
    assert_eq!(
        kinds_in(b"a (+) b (-) c", Dialect::Fteqcc),
        vec![
            name("a"),
            punct(Punct::SetBits),
            name("b"),
            punct(Punct::ClearBits),
            name("c"),
        ]
    );
}

// spec FTE-20.1: in FTEQCC a `.` before a digit starts a number; vanilla reads `.` alone.
#[test]
fn leading_dot_numbers_lex() {
    assert_eq!(
        kinds_in(b".1", Dialect::Fteqcc),
        vec![TokenKind::Float(0.1)]
    );
    assert_eq!(
        kinds_in(b"x = .25;", Dialect::Fteqcc),
        vec![
            name("x"),
            punct(Punct::Eq),
            TokenKind::Float(0.25),
            punct(Punct::Semicolon)
        ]
    );
    // `..` and a field access `e.x` are not numbers.
    assert_eq!(
        kinds_in(b"e.x", Dialect::Fteqcc),
        vec![name("e"), punct(Punct::Dot), name("x")]
    );
    assert_eq!(kinds(b".1"), vec![punct(Punct::Dot), TokenKind::Float(1.0)]);
}

// spec FTE-20.2: an `f` or `F` suffix ends an FTEQCC number and is not part of its value.
#[test]
fn float_suffix_lexes() {
    let lexed = lex_with(b"1.0f 2F 3", Dialect::Fteqcc);
    let kinds: Vec<TokenKind> = lexed.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Float(1.0),
            TokenKind::Float(2.0),
            TokenKind::Float(3.0)
        ]
    );
    assert_eq!(lexed.tokens[0].span, Span::new(0, 4));
    assert!(lexed.diagnostics.is_empty());
    // In vanilla the suffix is a separate name.
    assert_eq!(
        kinds_in(b"1.0f", Dialect::Vanilla),
        vec![TokenKind::Float(1.0), name("f")]
    );
}

// spec FTE-20.4: FTEQCC's one-byte escapes are accepted, and are unknown in vanilla.
#[test]
fn fteqcc_string_escapes() {
    let lexed = lex_with(br#""\t\r\a\v\f\#\\\?\'\n\"""#, Dialect::Fteqcc);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    assert_eq!(
        lexed.tokens[0].kind,
        TokenKind::String(b"\t\r\x07\x0b\x0c#\\?'\n\"".to_vec())
    );
    assert_eq!(codes(br#""\t""#), vec![Code::E0103]);
}

/// The string contents and diagnostic codes of one FTEQCC string literal.
fn fte_string(source: &[u8]) -> (Vec<u8>, Vec<Code>) {
    let lexed = lex_with(source, Dialect::Fteqcc);
    let codes = lexed.diagnostics.iter().map(|d| d.code).collect();
    match &lexed.tokens[0].kind {
        TokenKind::String(bytes) => (bytes.clone(), codes),
        other => panic!("not a string: {other:?}"),
    }
}

// spec FTEF-13.11: the Quake character escapes, `\{n}`, `\{xN}`, `\x`, `\u`, `\U` and the
// digits; any other escape stays unknown
// spec FTEF-13.8: a plain string writes a Quake glyph's code as its low byte, and `u8"..."`
// as UTF-8
#[test]
fn full_fteqcc_escapes() {
    let utf8 = |code: u32| char::from_u32(code).unwrap().to_string().into_bytes();
    let u8_string = |source: &[u8]| {
        let mut prefixed = b"u8".to_vec();
        prefixed.extend_from_slice(source);
        fte_string(&prefixed)
    };
    assert_eq!(fte_string(br#""\x41\x4""#), (b"A\x04".to_vec(), vec![]));
    assert_eq!(fte_string(br#""\{65}\{x42}""#), (b"AB".to_vec(), vec![]));
    assert_eq!(fte_string(br#""A\U0000042""#), (b"AB".to_vec(), vec![]));
    assert_eq!(fte_string(br#""\[""#).0, b"\x10".to_vec());
    assert_eq!(u8_string(br#""\[""#).0, utf8(0xe010));
    assert_eq!(fte_string(br#""\]""#).0, b"\x11".to_vec());
    for (escape, code) in [
        (b'.', 0xe01c),
        (b'<', 0xe01d),
        (b'-', 0xe01e),
        (b'>', 0xe01f),
        (b'(', 0xe080),
        (b'=', 0xe081),
        (b')', 0xe082),
        (b'+', 0xe083),
        (b'0', 0xe012),
        (b'9', 0xe01b),
    ] {
        let source = [b'"', b'\\', escape, b'"'];
        let low = vec![u8::try_from(code & 0xff).unwrap()];
        assert_eq!(fte_string(&source), (low, vec![]), "{}", char::from(escape));
        assert_eq!(
            u8_string(&source),
            (utf8(code), vec![]),
            "{}",
            char::from(escape)
        );
    }
    // `\s` and `\b` toggle bronze text, which sets the high bit, and stand for nothing.
    assert_eq!(fte_string(br#""a\bb\sc""#), (b"a\xe2c".to_vec(), vec![]));
    assert_eq!(codes(br#""\b""#), vec![Code::E0103]);
    // A code above a byte is its low byte in a plain string; `\u` is UTF-8 in any string.
    assert_eq!(fte_string(br#""\{200}\{256}""#).0, b"\xc8\x00".to_vec());
    assert_eq!(u8_string(br#""\{200}""#).0, utf8(200));
    assert_eq!(fte_string(b"\"\xc3\xa9\"").0, utf8(0xe9));
    assert_eq!(fte_string(br#""\q""#).1, vec![Code::E0103]);
    // Vanilla has none of them.
    assert_eq!(codes(br#""\x41""#), vec![Code::E0103]);
}

// spec FTEF-13.1: `::` is one token in FTEQCC and two `:` bytes unknown to vanilla.
#[test]
fn scope_operator_lexes() {
    assert_eq!(
        kinds_in(b"a::b", Dialect::Fteqcc),
        vec![
            TokenKind::Name("a".into()),
            TokenKind::Punct(Punct::ColonColon),
            TokenKind::Name("b".into()),
        ]
    );
    assert_eq!(
        kinds_in(b"a: b", Dialect::Fteqcc)[1],
        TokenKind::Punct(Punct::Colon)
    );
    assert_eq!(codes(b"a::b"), vec![Code::E0105, Code::E0105]);
}

// spec FTEF-13.2: a `\` before LF, CR or CR LF joins the lines in FTEQCC; vanilla has no such
// byte.
// spec QC-7.2, QCD-2.5: in vanilla that `\` is "Unknown punctuation", and the line break and
// the name after it are kept.
#[test]
fn line_continuations_are_trivia() {
    for source in [&b"a \\\nb"[..], b"a \\\r\nb", b"a \\\rb"] {
        let lexed = lex_with(source, Dialect::Fteqcc);
        assert!(lexed.diagnostics.is_empty(), "{source:?}");
        assert_eq!(lexed.tokens.len(), 2);
    }
    for source in [&b"a \\\nb"[..], b"a \\\r\nb", b"a \\\rb"] {
        assert_eq!(codes(source), vec![Code::E0105], "{source:?}");
        assert_eq!(kinds(source), vec![name("a"), name("b")], "{source:?}");
    }
    // A `\` before anything else is still unknown.
    assert_eq!(
        lex_with(b"a \\ b", Dialect::Fteqcc).diagnostics[0].code,
        Code::E0105
    );
}

// spec FTEF-13.3: a character constant is a float of its code; a two-component vector has
// z 0; vanilla reads neither.
#[test]
fn character_constants_and_short_vectors() {
    let one = |source: &[u8]| {
        let lexed = lex_with(source, Dialect::Fteqcc);
        // FTEF-13.22's "2d vector" is a warning, which FTEQCC ignores unless told otherwise.
        assert!(
            lexed.diagnostics.iter().all(|d| d.code == Code::W0120),
            "{source:?}: {:?}",
            lexed.diagnostics
        );
        assert_eq!(lexed.tokens.len(), 1, "{source:?}");
        lexed.tokens[0].kind.clone()
    };
    assert_eq!(one(b"'a'"), TokenKind::Float(97.0));
    assert_eq!(one(b"' '"), TokenKind::Float(32.0));
    assert_eq!(one(br"'\n'"), TokenKind::Float(10.0));
    assert_eq!(one(br"'\x41'"), TokenKind::Float(65.0));
    assert_eq!(one("'\u{e9}'".as_bytes()), TokenKind::Float(233.0));
    assert_eq!(one(b"'8 8'"), TokenKind::Vector([8.0, 8.0, 0.0]));
    assert_eq!(one(b"'1 2 3'"), TokenKind::Vector([1.0, 2.0, 3.0]));
    let unclosed = lex_with(br"'\nx", Dialect::Fteqcc);
    assert_eq!(unclosed.diagnostics[0].code, Code::E0104);
    assert_eq!(lex_with(br"'\", Dialect::Fteqcc).tokens.len(), 1);
    assert_eq!(codes(b"'a'"), vec![Code::E0104]);
    assert_eq!(codes(b"'8 8'"), vec![Code::E0104]);
}

// spec FTEF-3.1: a type suffix ends a number and is not part of its value.
// spec FTEF-3.2: `0x` reads base 16 and a leading `0` base 8.
// spec FTEF-13.5: every number is kept as a float of its value.
#[test]
fn full_fteqcc_numbers() {
    let value = |source: &str| {
        let lexed = lex_with(source.as_bytes(), Dialect::Fteqcc);
        // FTEF-13.19's warning for a leading 0 before a digit is no error.
        assert!(
            lexed.diagnostics.iter().all(|d| d.code == Code::W0110),
            "{source}"
        );
        assert_eq!(lexed.tokens.len(), 1, "{source}");
        match lexed.tokens[0].kind {
            TokenKind::Float(v) => v,
            ref other => panic!("{source}: {other:?}"),
        }
    };
    for (source, expected) in [
        ("12i", 12.0_f32),
        ("1u", 1.0),
        ("7l", 7.0),
        ("7ll", 7.0),
        ("7lu", 7.0),
        ("7LLi", 7.0),
        ("7ul", 7.0),
        ("7iLL", 7.0),
        ("3d", 3.0),
        ("3f", 3.0),
        ("3.5d", 3.5),
        ("3.5f", 3.5),
        ("0x10", 16.0),
        ("0XfF", 255.0),
        ("0x00000002i", 2.0),
        ("0x80000000i", 2.0_f32.powi(31)),
        ("010", 8.0),
        ("0", 0.0),
        ("0.25", 0.25),
        ("-0x10", -16.0),
        ("-5", -5.0),
        (".5", 0.5),
        ("0xd", 13.0),
    ] {
        assert_eq!(value(source).to_bits(), expected.to_bits(), "{source}");
    }
    // A `..` after a number is not a fraction.
    let dots = kinds_in(b"1..", Dialect::Fteqcc);
    assert_eq!(dots[0], TokenKind::Float(1.0));
    // Vanilla reads `0x10` as 0 and the name `x10`.
    assert_eq!(
        kinds_in(b"0x10", Dialect::Vanilla),
        vec![TokenKind::Float(0.0), TokenKind::Name("x10".into())]
    );
}

proptest! {
    // Lexing any bytes terminates without panicking, and every span is in bounds,
    // increasing and non-overlapping.
    #[test]
    fn spans_are_ordered_and_in_bounds(source in proptest::collection::vec(any::<u8>(), 0..256)) {
        let lexed = lex(&source);
        let mut end = 0;
        for token in &lexed.tokens {
            prop_assert!(token.span.start >= end);
            prop_assert!(token.span.start < token.span.end);
            prop_assert!(token.span.end <= source.len());
            end = token.span.end;
        }
        for d in &lexed.diagnostics {
            prop_assert!(d.span.start <= d.span.end && d.span.end <= source.len());
        }
    }

    // A token's own text lexes to the same token, for tokens that need no context.
    #[test]
    fn token_text_relexes(source in "[a-z0-9_ .,;(){}\\[\\]+*/=<>!&|#\"'-]{0,64}") {
        let bytes = source.as_bytes();
        let lexed = lex(bytes);
        if lexed.diagnostics.is_empty() {
            for token in &lexed.tokens {
                let text = &bytes[token.span.range()];
                let again = lex(text);
                prop_assert_eq!(again.tokens.len(), 1);
                prop_assert_eq!(&again.tokens[0].kind, &token.kind);
            }
        }
    }

    // Line numbers count the LF bytes before an offset.
    #[test]
    fn line_numbers_count_lfs(source in proptest::collection::vec(any::<u8>(), 0..128), at in 0usize..128) {
        let at = at.min(source.len());
        let index = LineIndex::new(&source);
        let lfs = source[..at].iter().filter(|&&b| b == b'\n').count();
        prop_assert_eq!(index.line(at), u32::try_from(lfs + 1).unwrap_or(u32::MAX));
    }
}

proptest! {
    // P2-R2 review: the FTEQCC lexer keeps the README's invariants too. Lexing any bytes under
    // the FTEQCC profile terminates, and every span is in bounds, increasing and
    // non-overlapping.
    #[test]
    fn reviewer_fteqcc_spans_are_ordered_and_in_bounds(source in proptest::collection::vec(any::<u8>(), 0..256)) {
        let lexed = lex_with(&source, Dialect::Fteqcc);
        let mut end = 0;
        for token in &lexed.tokens {
            prop_assert!(token.span.start >= end);
            prop_assert!(token.span.start < token.span.end);
            prop_assert!(token.span.end <= source.len());
            end = token.span.end;
        }
        for d in &lexed.diagnostics {
            prop_assert!(d.span.start <= d.span.end && d.span.end <= source.len());
        }
    }

    // P2-R2 review: under the FTEQCC profile a token's own text lexes to the same token, over
    // the FTEQCC punctuation, escapes and `$`.
    #[test]
    fn reviewer_fteqcc_token_text_relexes(source in "[a-z0-9_ .,;:?(){}\\[\\]+*/%^~=<>!&|#\"'\\\\$x-]{0,64}") {
        let bytes = source.as_bytes();
        let lexed = lex_with(bytes, Dialect::Fteqcc);
        if lexed.diagnostics.is_empty() {
            for token in &lexed.tokens {
                let text = &bytes[token.span.range()];
                let again = lex_with(text, Dialect::Fteqcc);
                prop_assert_eq!(again.tokens.len(), 1, "{:?} in {:?}", String::from_utf8_lossy(text), source);
                prop_assert_eq!(&again.tokens[0].kind, &token.kind);
            }
        }
    }
}

fn fte_kinds(source: &[u8]) -> Vec<TokenKind> {
    kinds_in(source, Dialect::Fteqcc)
}

fn fte_codes(source: &[u8]) -> Vec<Code> {
    lex_with(source, Dialect::Fteqcc)
        .diagnostics
        .iter()
        .map(|d| d.code)
        .collect()
}

fn frame_ref(word: &str) -> TokenKind {
    TokenKind::FrameRef(word.to_owned())
}

// spec FTEF-18.13: under FTEQCC a frame reference ends at `,`, `;`, `&`, `|`, `(`, `)`, `]` or `..`
#[test]
fn fteqcc_frame_ref_stops_at_more_bytes() {
    assert_eq!(
        fte_kinds(b"($b)"),
        vec![punct(Punct::LParen), frame_ref("b"), punct(Punct::RParen)]
    );
    assert_eq!(
        fte_kinds(b"$a||x"),
        vec![frame_ref("a"), punct(Punct::OrOr), name("x")]
    );
    assert_eq!(
        fte_kinds(b"$a&&x"),
        vec![frame_ref("a"), punct(Punct::AndAnd), name("x")]
    );
    assert_eq!(
        fte_kinds(b"[$a, th]"),
        vec![
            punct(Punct::LBracket),
            frame_ref("a"),
            punct(Punct::Comma),
            name("th"),
            punct(Punct::RBracket)
        ]
    );
    assert_eq!(
        fte_kinds(b"$a]$b..$c;"),
        vec![
            frame_ref("a"),
            punct(Punct::RBracket),
            frame_ref("b"),
            punct(Punct::DotDot),
            frame_ref("c"),
            punct(Punct::Semicolon)
        ]
    );
    // A single `.` stays in the name, as do the other bytes above a space.
    assert_eq!(fte_kinds(b"$a.b+c"), vec![frame_ref("a.b+c")]);
    // Vanilla keeps reading to whitespace, `,` or `;` (QC-8.6).
    assert_eq!(kinds(b"$a||x"), vec![frame_ref("a||x")]);
}

// spec FTEF-18.13: under FTEQCC a `$` followed by whitespace, or by no byte a name can hold, is hanging
#[test]
fn fteqcc_hanging_dollar() {
    assert_eq!(fte_codes(b"$ walk1"), vec![Code::E0107]);
    assert_eq!(fte_codes(b"$)"), vec![Code::E0107]);
    assert_eq!(fte_codes(b"$// c"), vec![Code::E0107]);
    assert_eq!(fte_codes(b"$/* c */"), vec![Code::E0107]);
    // Vanilla skips the blanks to the word (QC-8.1).
    assert_eq!(kinds(b"$ walk1"), vec![frame_ref("walk1")]);
}

// spec FTEF-18.14: `$framesave` defines frame names as `$frame` does, under FTEQCC only
#[test]
fn framesave_defines_frames() {
    let lexed = lex_with(b"$framesave a b\nx", Dialect::Fteqcc);
    let TokenKind::Frames(names) = &lexed.tokens[0].kind else {
        panic!("not a frame list: {:?}", lexed.tokens[0].kind);
    };
    assert_eq!(
        names.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(lexed.tokens[1].kind, name("x"));
    // The frame names end as frame references do (FTEF-18.13).
    let lexed = lex_with(b"$frame a(b c", Dialect::Fteqcc);
    let TokenKind::Frames(names) = &lexed.tokens[0].kind else {
        panic!("not a frame list");
    };
    assert_eq!(names.len(), 1);
    assert_eq!(lexed.tokens[1].kind, punct(Punct::LParen));
    // `,` and `;` stay QCD-6.3 errors under both profiles.
    assert_eq!(fte_codes(b"$frame a, b"), vec![Code::E0503]);
    assert_eq!(
        kinds(b"$framesave a"),
        vec![frame_ref("framesave"), name("a")]
    );
}

// spec FTEF-18.16: FTEQCC's further `$` commands are tokens of their own, with their word
#[test]
fn fteqcc_frame_commands() {
    use super::FrameCommand;
    assert_eq!(
        fte_kinds(b"$flush\nx"),
        vec![TokenKind::FrameCommand(FrameCommand::Flush), name("x")]
    );
    assert_eq!(
        fte_kinds(b"$frame_reset"),
        vec![TokenKind::FrameCommand(FrameCommand::Reset)]
    );
    assert_eq!(
        fte_kinds(b"$framevalue 12"),
        vec![TokenKind::FrameCommand(FrameCommand::Value(
            "12".to_owned()
        ))]
    );
    assert_eq!(
        fte_kinds(b"$framerestore walk1"),
        vec![TokenKind::FrameCommand(FrameCommand::Restore(
            "walk1".to_owned()
        ))]
    );
    assert_eq!(
        fte_kinds(b"$modelname knight"),
        vec![TokenKind::FrameCommand(FrameCommand::ModelName(
            "knight".to_owned()
        ))]
    );
    // With no word the operand is empty, as FTEQCC's `pr_token` is.
    assert_eq!(
        fte_kinds(b"$framevalue\nx"),
        vec![
            TokenKind::FrameCommand(FrameCommand::Value(String::new())),
            name("x")
        ]
    );
    // Under vanilla each is a frame macro reference (QC-8.5), and takes no word after it.
    for command in [
        "flush",
        "frame_reset",
        "framevalue",
        "framerestore",
        "modelname",
    ] {
        let source = format!("${command} w");
        assert_eq!(
            kinds(source.as_bytes()),
            vec![frame_ref(command), name("w")],
            "{source}"
        );
    }
    assert_eq!(
        kinds(b"$framevalue 12"),
        vec![frame_ref("framevalue"), TokenKind::Float(12.0)]
    );
}

// spec FTEF-18.16: an ignored command takes its words up to one that ends at once, not the rest of the line
#[test]
fn fteqcc_ignored_commands_take_words() {
    assert_eq!(fte_kinds(b"$cd id1/models/knight\nx"), vec![name("x")]);
    assert_eq!(fte_kinds(b"$origin 0 0 24 // c\nx"), vec![name("x")]);
    assert_eq!(fte_kinds(b"$cd a,b"), vec![punct(Punct::Comma), name("b")]);
    // Vanilla drops the rest of the line (QC-27.14).
    assert_eq!(kinds(b"$cd a,b\nx"), vec![name("x")]);
}

// spec FTEF-18.18: `..` is one token under FTEQCC, after `...`, and a number ends before it
#[test]
fn dot_dot_token() {
    assert_eq!(
        fte_kinds(b"1..5"),
        vec![
            TokenKind::Float(1.0),
            punct(Punct::DotDot),
            TokenKind::Float(5.0)
        ]
    );
    assert_eq!(
        fte_kinds(b"... .. ."),
        vec![
            punct(Punct::Ellipsis),
            punct(Punct::DotDot),
            punct(Punct::Dot)
        ]
    );
    assert_eq!(
        fte_kinds(b"..float"),
        vec![punct(Punct::DotDot), name("float")]
    );
    // Vanilla has no `..` token.
    assert_eq!(
        kinds(b"a..b"),
        vec![name("a"), punct(Punct::Dot), punct(Punct::Dot), name("b")]
    );
}

/// The string token texts of an FTEQCC lex, in order.
fn fte_strings(source: &[u8]) -> Vec<Vec<u8>> {
    lex_with(source, Dialect::Fteqcc)
        .tokens
        .into_iter()
        .filter_map(|t| match t.kind {
            TokenKind::String(bytes) => Some(bytes),
            _ => None,
        })
        .collect()
}

// spec FTEF-14.38: the text of `#warning`, `#error` and `#message` is the rest of the line as
// written, blanks before it skipped, so a `'` in it opens no vector
#[test]
fn message_directive_text_is_the_raw_line() {
    assert_eq!(
        fte_kinds(b"#warning doesn't work\nx"),
        vec![
            punct(Punct::Hash),
            name("warning"),
            TokenKind::String(b"doesn't work".to_vec()),
            name("x")
        ]
    );
    assert_eq!(fte_strings(b"#error \t bad"), vec![b"bad".to_vec()]);
    assert_eq!(fte_strings(b"#message\thello"), vec![b"hello".to_vec()]);
    assert_eq!(fte_strings(b"  #warning a b\r\nx"), vec![b"a b".to_vec()]);
    assert_eq!(fte_strings(b"#warning a\rx"), vec![b"a".to_vec()]);
    // A line that ends at once has no text.
    assert_eq!(
        fte_kinds(b"#warning\nx"),
        vec![punct(Punct::Hash), name("warning"), name("x")]
    );
    assert_eq!(fte_kinds(b"#warning   ").len(), 2);
    // Blanks between the `#` and the word are allowed.
    assert_eq!(fte_strings(b"# warning a"), vec![b"a".to_vec()]);
}

// spec FTEF-14.38: only a `#warning`, `#error` or `#message` directive reads its line raw:
// another word, a `#` not starting its line, a `#` on an earlier line or the vanilla profile
// lex the line as tokens
#[test]
fn message_text_needs_the_directive() {
    let none: Vec<Vec<u8>> = Vec::new();
    assert_eq!(fte_strings(b"#define a b"), none);
    assert_eq!(fte_strings(b"warning a b"), none);
    assert_eq!(fte_strings(b"+warning a"), none);
    assert_eq!(fte_strings(b"x #warning a"), none);
    assert_eq!(fte_strings(b"#\nwarning a"), none);
    assert_eq!(
        kinds(b"#warning a"),
        vec![punct(Punct::Hash), name("warning"), name("a")]
    );
}

// spec FTEF-3.2: a leading `0` reads base 8, so an `8` or `9` ends the number; `0x` reads base
// 16, so a `g` ends it
#[test]
fn a_digit_outside_the_base_ends_the_number() {
    assert_eq!(
        fte_kinds(b"08"),
        vec![TokenKind::Float(0.0), TokenKind::Float(8.0)]
    );
    assert_eq!(fte_kinds(b"0x1g"), vec![TokenKind::Float(1.0), name("g")]);
    assert_eq!(fte_kinds(b"0xf"), vec![TokenKind::Float(15.0)]);
    assert_eq!(fte_kinds(b"07"), vec![TokenKind::Float(7.0)]);
}

// spec FTE-23.2: a hex number with a fraction is C's `atof` of it, a hexadecimal float, each
// decimal digit after the `.` a sixteenth of the one before; the vanilla dialect has no hex
#[test]
fn hex_numbers_with_a_fraction_are_hex_floats() {
    for (source, value) in [
        (&b"0x10.0"[..], 16.0_f32),
        (b"0x10.", 16.0),
        (b"-0x10.0", -16.0),
        (b"0x1.8", 1.5),
        (b"0X1.08", 1.031_25),
        (b"0x.4", 0.25),
        (b"0x1.0f", 1.0),
        (b"0x1.5d", 1.312_5),
        // The whole part is read in full, not kept to 64 bits.
        (b"0x10000000000000005.0", 1.844_674_4e19),
    ] {
        assert_eq!(
            fte_kinds(source),
            vec![TokenKind::Float(value)],
            "{source:?}"
        );
    }
    // The fraction takes only decimal digits.
    assert_eq!(fte_kinds(b"0x1.a"), vec![TokenKind::Float(1.0), name("a")]);
    assert_eq!(
        kinds(b"0x10.0"),
        vec![
            TokenKind::Float(0.0),
            name("x10"),
            punct(Punct::Dot),
            TokenKind::Float(0.0)
        ]
    );
}

// spec FTEF-13.11: `\u` and `\U` take four and eight hex digits, as many as there are, and are
// "Unicode character terminated unexpectedly" unless they stop one digit short
#[test]
fn short_unicode_escape() {
    let unexpected = vec![Code::E0103];
    let escape = |digits: &str| format!("\"\\{digits}Z\"").into_bytes();
    assert_eq!(
        fte_string(&escape("u0041")),
        (b"AZ".to_vec(), unexpected.clone())
    );
    assert_eq!(fte_string(&escape("u041")), (b"AZ".to_vec(), vec![]));
    assert_eq!(
        fte_string(&escape("u41")),
        (b"AZ".to_vec(), unexpected.clone())
    );
    assert_eq!(fte_string(&escape("U0000041")), (b"AZ".to_vec(), vec![]));
    assert_eq!(
        fte_string(&escape("U00000041")),
        (b"AZ".to_vec(), unexpected.clone())
    );
    // A fifth hex digit is no part of `\u`.
    assert_eq!(fte_string(&escape("u12345")).0, b"\xe1\x88\xb45Z".to_vec());
    assert_eq!(fte_string(br#""\u""#), (b"\x00".to_vec(), unexpected));
}

/// The value of the FTEQCC character constant at the start of `source`, and the codes of
/// the diagnostics of `source`.
fn fte_character(source: &[u8]) -> (f32, Vec<Code>) {
    let lexed = lex_with(source, Dialect::Fteqcc);
    let codes = lexed.diagnostics.iter().map(|d| d.code).collect();
    match &lexed.tokens[0].kind {
        TokenKind::Float(value) => (*value, codes),
        other => panic!("not a character constant: {other:?}"),
    }
}

/// The character constant `'\` `escape` `'`.
fn escaped_character(escape: &str) -> Vec<u8> {
    format!("'\\{escape}'").into_bytes()
}

// spec FTEF-13.27: a character constant's escape is read as a string's (FTEF-13.11): each
// escape's code, and "Unknown escape char" and "Unicode character terminated unexpectedly"
// as errors (E0103)
#[test]
fn character_constant_escapes_are_read_as_in_strings() {
    for (escape, value) in [
        ("n", 10_u16),
        ("r", 13),
        ("t", 9),
        ("a", 7),
        ("v", 11),
        ("f", 12),
        ("#", 35),
        ("\"", 34),
        ("\\", 92),
        ("?", 63),
        ("'", 39),
        ("[", 0xe010),
        ("]", 0xe011),
        (".", 0xe01c),
        ("<", 0xe01d),
        ("-", 0xe01e),
        (">", 0xe01f),
        ("(", 0xe080),
        ("=", 0xe081),
        (")", 0xe082),
        ("+", 0xe083),
        ("0", 0xe012),
        ("9", 0xe01b),
        ("{65}", 65),
        ("{x41}", 65),
        ("x41", 65),
        ("X41", 65),
        ("x4", 4),
        ("u041", 65),
        ("U0000041", 65),
        ("\n", 10),
        ("\r\n", 10),
    ] {
        let source = escaped_character(escape);
        assert_eq!(
            fte_character(&source),
            (f32::from(value), vec![]),
            "{escape:?}"
        );
    }
    for (escape, value) in [
        ("s", 115.0),
        ("b", 98.0),
        ("q", 113.0),
        ("u0041", 65.0),
        ("U00000041", 65.0),
    ] {
        let source = escaped_character(escape);
        assert_eq!(
            fte_character(&source),
            (value, vec![Code::E0103]),
            "{escape:?}"
        );
    }
}

// spec FTEF-13.14: a character constant at a byte of 0x80 or more is the code FTEQCC's
// `utf8_check` reads, surrogates and the modified UTF-8 NUL included, and one byte after the
// sequence is skipped unread; a byte that starts no sequence is 0xFFFD and is skipped alone, so
// the `'` after it opens a vector
#[test]
fn high_byte_character_constants_follow_utf8_check() {
    for (source, value) in [
        (&b"'\xc3\xa9'"[..], 233.0),
        (b"'\xc3\xa9x", 233.0),
        (b"'\xed\xa0\x80'", 55296.0),
        (b"'\xc0\x80'", 0.0),
        (b"'\xf0\x90\x80\x80'", 65536.0),
    ] {
        let lexed = lex_with(source, Dialect::Fteqcc);
        assert!(lexed.diagnostics.is_empty(), "{source:?}");
        assert_eq!(lexed.tokens.len(), 1, "{source:?}");
        assert_eq!(lexed.tokens[0].kind, TokenKind::Float(value), "{source:?}");
    }
    for source in [&b"'\xff'"[..], b"'\xe9'", b"'\xc1\x81'"] {
        let lexed = lex_with(source, Dialect::Fteqcc);
        assert_eq!(
            lexed.tokens[0].kind,
            TokenKind::Float(65533.0),
            "{source:?}"
        );
        assert_eq!(lexed.tokens[0].span.end, 2, "{source:?}");
        assert_eq!(fte_codes(source).last(), Some(&Code::E0104), "{source:?}");
    }
}

// spec FTEF-13.27: a bad `\{` or `\x` is "Bad character code" (E0103), and with no `'` after
// the escape the constant is also "Bad character constant" (E0104); the end of the file
// right after the `\` is "EOF inside quote" (E0101) alone, and after a `\` and CR "Bad
// character constant" (FTEF-13.26)
#[test]
fn character_constant_escape_errors() {
    let both = vec![Code::E0103, Code::E0104];
    assert_eq!(fte_character(br"'\{6z}"), (6.0, both.clone()));
    assert_eq!(fte_character(br"'\{65"), (65.0, both.clone()));
    assert_eq!(fte_character(br"'\xg"), (0.0, both.clone()));
    assert_eq!(fte_character(br"'\x"), (0.0, both));
    assert_eq!(fte_character(br"'\"), (0.0, vec![Code::E0101]));
    assert_eq!(fte_character(b"'\\\r"), (0.0, vec![Code::E0104]));
    assert_eq!(fte_character(br"'\n"), (10.0, vec![Code::E0104]));
}

// spec FTEF-13.10: bronze text sets the high bit of each byte from a space up and leaves a
// control byte and a byte of 0x80 or more as they are
#[test]
fn bronze_text_keeps_high_bytes() {
    assert_eq!(fte_string(b"\"\\b\xe9a\"").0, b"\xe9\xe1".to_vec());
    assert_eq!(fte_string(b"\"\\s\x80\"").0, b"\x80".to_vec());
    assert_eq!(fte_string(b"\"\\b\t \"").0, b"\t\xa0".to_vec());
    assert_eq!(fte_string(b"\"\\b\\t\"").0, b"\t".to_vec());
}

// spec FTEF-13.11: `\{n}` reads digits to the `}`; any other byte, the end of the file
// included, is "Bad character code" (E0103), and the string then goes on
#[test]
fn braced_escape_at_end_of_file() {
    assert_eq!(
        fte_string(br#""\{65"#),
        (b"A".to_vec(), vec![Code::E0103, Code::E0101])
    );
    assert_eq!(fte_string(br#""\{x41}""#), (b"A".to_vec(), vec![]));
    assert_eq!(
        fte_string(br#""\{6z}""#),
        (b"\x06z}".to_vec(), vec![Code::E0103])
    );
    assert_eq!(fte_string(br#""\{xg}""#).1, vec![Code::E0103]);
}

// spec FTEF-13.3: an escaped character constant is the escape's code, and a byte with no
// escape of its own, `\"` among them, is itself
#[test]
fn escaped_character_constants() {
    assert_eq!(fte_kinds(br#"'\"'"#), vec![TokenKind::Float(34.0)]);
    assert_eq!(fte_kinds(br"'\n'"), vec![TokenKind::Float(10.0)]);
    assert_eq!(fte_kinds(br"'\t'"), vec![TokenKind::Float(9.0)]);
    assert_eq!(fte_kinds(br"'\''"), vec![TokenKind::Float(39.0)]);
}

// spec FTEF-13.3: a `'` with nothing after it opens a vector, which is bad
#[test]
fn lone_quote_is_a_bad_vector() {
    let lexed = lex_with(b"'", Dialect::Fteqcc);
    assert_eq!(lexed.tokens.len(), 1);
    assert_eq!(lexed.tokens[0].kind, TokenKind::Vector([0.0; 3]));
    assert_eq!(fte_codes(b"'"), vec![Code::E0104]);
}

// spec FTEF-13.6: a vector component may end in a lowercase `f`, which is not part of its
// value; an `F` is not skipped, so the vector is bad
#[test]
fn vector_components_take_the_float_suffix() {
    let lexed = lex_with(b"'1f 2f 3.5f'", Dialect::Fteqcc);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    assert_eq!(lexed.tokens[0].kind, TokenKind::Vector([1.0, 2.0, 3.5]));
    assert_eq!(fte_codes(b"'1F 2 3'")[0], Code::E0104);
    assert_eq!(fte_codes(b"'1 2 3F'")[0], Code::E0104);
    assert_eq!(codes(b"'1f 2 3'")[0], Code::E0104);
}

// spec FTEF-13.2: a `\` before a CR LF joins the lines, the LF included
#[test]
fn continuation_before_crlf_keeps_the_next_line_start() {
    let lexed = lex_with(b"a \\\r\n\nb", Dialect::Fteqcc);
    assert!(lexed.diagnostics.is_empty());
    assert_eq!(lexed.tokens.len(), 2);
    assert_eq!(lexed.tokens[1].span, Span::new(6, 7));
}

// spec FTEF-18.16: `$flush` and `$frame_reset` take the words after them
#[test]
fn flush_takes_its_words() {
    use super::FrameCommand;
    assert_eq!(
        fte_kinds(b"$flush a b\nx"),
        vec![TokenKind::FrameCommand(FrameCommand::Flush), name("x")]
    );
    assert_eq!(
        fte_kinds(b"$frame_reset a"),
        vec![TokenKind::FrameCommand(FrameCommand::Reset)]
    );
}

// spec FTEF-18.13: a frame macro name ends at a comment, but a `/` before anything else is
// part of the name
#[test]
fn frame_names_and_slashes() {
    let names = |source: &[u8]| match &lex_with(source, Dialect::Fteqcc).tokens[0].kind {
        TokenKind::Frames(names) => names.iter().map(|n| n.name.clone()).collect::<Vec<_>>(),
        other => panic!("not a frame list: {other:?}"),
    };
    assert_eq!(names(b"$frame a /b"), ["a", "/b"]);
    assert_eq!(names(b"$frame a // b c\nd"), ["a"]);
    assert_eq!(names(b"$frame a /* b */ c"), ["a"]);
    assert_eq!(fte_kinds(b"$frame a // b c\nd")[1..], [name("d")]);
}

// spec FTEF-14.55: C's `atoi` reads white space, a sign and decimal digits
#[test]
fn c_atoi_reads_white_space_a_sign_and_digits() {
    assert_eq!(super::c_atoi(b" \t+12x"), 12);
    assert_eq!(super::c_atoi(b"\n-7"), -7);
    assert_eq!(super::c_atoi(b"+-1"), 0);
}

// spec FTEF-14.66: `QCC_PR_LexMacroName` stops the compile at a name's 1024th byte, so a frame
// name of 1023 bytes is read and one of 1024 is "token exceeds 1023 chars"
#[test]
fn a_frame_name_of_1024_bytes_is_too_long() {
    let frame = |len: usize| fte_codes(format!("$frame {}", "a".repeat(len)).as_bytes());
    assert!(!frame(FTEQCC_MAX_WORD).contains(&Code::E0502));
    assert!(frame(FTEQCC_MAX_WORD + 1).contains(&Code::E0502));
}

fn string(bytes: &[u8]) -> TokenKind {
    TokenKind::String(bytes.to_vec())
}

// spec FTEF-13.7: under FTEQCC `R"`, `Q"`, `u"`, `U"` and `u8"` start a string, and `L"` only
// right after another string; vanilla reads each prefix as a name
#[test]
fn prefixed_strings_are_strings() {
    for source in [
        &b"\"ab\""[..],
        b"R\"(ab)\"",
        b"Q\"ab\"",
        b"u\"ab\"",
        b"U\"ab\"",
        b"u8\"ab\"",
    ] {
        let lexed = lex_with(source, Dialect::Fteqcc);
        // FTEF-13.22's warning for `u` and `U`, which FTEQCC ignores unless told otherwise.
        assert!(
            lexed.diagnostics.iter().all(|d| d.code == Code::W0119),
            "{source:?}"
        );
        assert_eq!(fte_kinds(source), [string(b"ab")], "{source:?}");
        assert_eq!(
            lexed.tokens[0].span,
            Span::new(0, source.len()),
            "{source:?}"
        );
    }
    assert_eq!(fte_kinds(b"\"a\" L\"b\""), [string(b"a"), string(b"b")]);
    assert_eq!(
        fte_kinds(b"\"a\" /* c */\nL\"b\""),
        [string(b"a"), string(b"b")]
    );
    assert_eq!(fte_kinds(b"L\"b\""), [name("L"), string(b"b")]);
    assert_eq!(fte_kinds(b"x L\"b\""), [name("x"), name("L"), string(b"b")]);
    assert_eq!(fte_kinds(b"u8 \"b\""), [name("u8"), string(b"b")]);
    assert_eq!(fte_kinds(b"u16\"b\""), [name("u16"), string(b"b")]);
    assert_eq!(kinds(b"u8\"b\""), [name("u8"), string(b"b")]);
    assert_eq!(kinds(b"R\"(b)\"").len(), 2);
}

// spec FTEF-13.8: `Q"..."` reads UTF-8 and writes Quake bytes, `u8"..."`, `u"..."`, `U"..."`
// and `L"..."` read and write UTF-8, a plain string takes its bytes as they are; UTF-8 is read
// as FTEQCC's `utf8_check` reads it and written as its `forceutf8` writes it
#[test]
fn each_prefix_writes_its_bytes() {
    let one = |source: &[u8]| fte_string(source).0;
    assert_eq!(one(b"Q\"\xc3\xa9\""), b"\xe9");
    assert_eq!(one(b"Q\"\xee\x80\x90\""), b"\x10");
    assert_eq!(one(b"Q\"\xff\""), b"\xff");
    assert_eq!(one(b"u8\"\xc3\xa9\""), b"\xc3\xa9");
    assert_eq!(one(b"u\"\\[\""), b"\xee\x80\x90");
    assert_eq!(one(b"U\"\\{200}\""), b"\xc3\x88");
    assert_eq!(one(b"\"\xc3\xa9\\[\""), b"\xc3\xa9\x10");
    // Modified UTF-8's two-byte NUL is a NUL; an overlong form is no sequence.
    assert_eq!(one(b"u8\"\xc0\x80\""), b"\x00");
    assert_eq!(one(b"u8\"\xc1\x81\""), b"\xc1\x81");
    // Surrogates are read and written; above 0x10FFFF is no sequence on input.
    assert_eq!(one(b"u8\"\xed\xa0\x80\""), b"\xed\xa0\x80");
    assert_eq!(one(b"u8\"\xf4\x90\x80\x80\""), b"\xf4\x90\x80\x80");
    assert_eq!(one(b"Q\"\xf4\x90\x80\x80\""), b"\xf4\x90\x80\x80");
    // `\u` and `\U` are UTF-8 even in a plain string, whatever the code.
    assert_eq!(one(br#""\U0010FFFF""#), b"\xf4\x8f\xbf\xbf");
    // Only a code above U+10FFFF is F301 "Bad unicode character code".
    assert!(!fte_string(br#""\U0010FFFF""#).1.contains(&Code::W0119));
    assert!(fte_string(br#""\U00110000""#).1.contains(&Code::W0119));
    assert_eq!(one(br#""\U00110000""#), b"\xf4\x90\x80\x80");
    assert_eq!(one(br#""\uD800""#), b"\xed\xa0\x80");
    assert_eq!(one(br#""\U7FFFFFFF""#), b"\xfd\xbf\xbf\xbf\xbf\xbf");
    assert_eq!(one(br#""\UFFFFFFFF""#), b"\xfe\x83\xbf\xbf\xbf\xbf\xbf");
    // `\x` is a byte even in a UTF-8 string.
    assert_eq!(one(br#"u8"\xe9""#), b"\xe9");
    // Each length of sequence is read whole, and its code is Quake's low byte.
    assert_eq!(one(b"Q\"\xe2\x82\xac\""), b"\xac");
    assert_eq!(one(b"Q\"\xf0\x9f\x98\x81\""), b"\x01");
    assert_eq!(one(b"Q\"\xf4\x8f\xbf\xbf\""), b"\xff");
    assert_eq!(one(b"Q\"\xdf\xbf\""), b"\xff");
    // Overlong forms, a lead byte with no continuation, and a lead of 0xF8 or more are bytes.
    assert_eq!(one(b"Q\"\xe0\x9f\xbf\""), b"\xe0\x9f\xbf");
    assert_eq!(one(b"Q\"\xf0\x8f\xbf\xbf\""), b"\xf0\x8f\xbf\xbf");
    assert_eq!(one(b"Q\"\xe2\x82a\""), b"\xe2\x82a");
    assert_eq!(one(b"Q\"\xe2\xc2\xac\""), b"\xe2\xac");
    assert_eq!(one(b"Q\"\xf0\x9f\x98a\""), b"\xf0\x9f\x98a");
    assert_eq!(one(b"Q\"\xf8\x88\x80\x80\x80\""), b"\xf8\x88\x80\x80\x80");
    assert_eq!(one(b"Q\"\xc2\""), b"\xc2");
    // Every length `forceutf8` writes.
    for (code, bytes) in [
        (0x7f_u32, &b"\x7f"[..]),
        (0x80, b"\xc2\x80"),
        (0x7ff, b"\xdf\xbf"),
        (0x800, b"\xe0\xa0\x80"),
        (0xffff, b"\xef\xbf\xbf"),
        (0x1_0000, b"\xf0\x90\x80\x80"),
        (0x1f_ffff, b"\xf7\xbf\xbf\xbf"),
        (0x20_0000, b"\xf8\x88\x80\x80\x80"),
        (0x3ff_ffff, b"\xfb\xbf\xbf\xbf\xbf"),
        (0x400_0000, b"\xfc\x84\x80\x80\x80\x80"),
        (0x8000_0000, b"\xfe\x82\x80\x80\x80\x80\x80"),
    ] {
        let source = format!("u8\"\\{{x{code:x}}}\"");
        assert_eq!(one(source.as_bytes()), bytes, "{code:#x}");
    }
}

// spec FTEF-13.7: one string token's bytes, an `L` part's included, and nothing for a text
// that is not one whole string part
#[test]
fn a_string_part_gives_its_bytes() {
    use super::{LexFlags, fteqcc_string_bytes};
    let fteqcc_string_bytes_default = |text: &[u8]| fteqcc_string_bytes(text, LexFlags::default());
    assert_eq!(fteqcc_string_bytes_default(b"\"ab\""), Some(b"ab".to_vec()));
    assert_eq!(
        fteqcc_string_bytes_default(b"L\"ab\""),
        Some(b"ab".to_vec())
    );
    assert_eq!(
        fteqcc_string_bytes_default(b"u8\"\\[\""),
        Some(b"\xee\x80\x90".to_vec())
    );
    assert_eq!(fteqcc_string_bytes_default(b"\"ab\" "), None);
    assert_eq!(fteqcc_string_bytes_default(b"ab"), None);
    assert_eq!(fteqcc_string_bytes_default(b""), None);
}

// spec FTEF-13.7: a string on a directive's line is no part an `L` on a later line joins, though
// one on the same line is
#[test]
fn an_l_after_a_directive_s_string_is_a_name() {
    let after = |source: &[u8]| {
        let kinds = fte_kinds(source);
        kinds[kinds.len() - 2..].to_vec()
    };
    assert_eq!(after(b"#include \"a\"\nL\"x\""), [name("L"), string(b"x")]);
    assert_eq!(after(b"#warning hi\nL\"x\""), [name("L"), string(b"x")]);
    assert_eq!(
        after(b"#define X \"a\" L\"b\""),
        [string(b"a"), string(b"b")]
    );
    assert_eq!(after(b"  \"a\"\nL\"b\""), [string(b"a"), string(b"b")]);
}

// spec FTEF-13.9: a raw string runs from its delimiter's `(` to `)`, the delimiter and `"`, its
// bytes as they are, line breaks and backslashes included, a `\` and line break right after
// the `(` skipped; a delimiter of more than 62 bytes, or one with no `(`, is an error
#[test]
fn raw_strings_keep_their_bytes() {
    let one = |source: &[u8]| fte_string(source);
    assert_eq!(one(b"R\"(a\nb)\""), (b"a\nb".to_vec(), vec![]));
    assert_eq!(
        one(b"R\"xy(a)x)\"xy\"b)xy\""),
        (b"a)x)\"xy\"b".to_vec(), vec![])
    );
    assert_eq!(one(b"R\"(\\n\\b\")\""), (b"\\n\\b\"".to_vec(), vec![]));
    for skipped in [&b"\\\n"[..], b"\\\r\n", b"\\\r"] {
        let mut source = b"R\"(".to_vec();
        source.extend_from_slice(skipped);
        source.extend_from_slice(b"a)\"");
        assert_eq!(one(&source), (b"a".to_vec(), vec![]), "{skipped:?}");
    }
    assert_eq!(one(b"R\"(\\\\\na)\"").0, b"\\\\\na");
    let delimited = |len: usize| {
        let delimiter = "d".repeat(len);
        format!("R\"{delimiter}(a){delimiter}\"").into_bytes()
    };
    assert_eq!(one(&delimited(62)), (b"a".to_vec(), vec![]));
    // The rest is lexed again after the error, and its closing `"` opens a string.
    assert_eq!(one(&delimited(63)).1, [Code::E0101, Code::E0101]);
    assert_eq!(one(b"R\"abc").1, [Code::E0101]);
    assert_eq!(one(b"R\"(abc").1, [Code::E0101]);
    // The raw string ends where it ends, and what follows is lexed again.
    assert_eq!(
        fte_kinds(b"R\"(a)\" + 1"),
        [
            string(b"a"),
            TokenKind::Punct(Punct::Plus),
            TokenKind::Float(1.0)
        ]
    );
}

// spec FTEF-13.10: bronze text in a UTF-8 string is the Quake glyph's private-use code, from
// a space up, and a non-ASCII character keeps its bytes
#[test]
fn bronze_text_in_each_kind_of_string() {
    let one = |source: &[u8]| fte_string(source).0;
    assert_eq!(one(br#"u8"\ba""#), b"\xee\x83\xa1");
    assert_eq!(one(br#"Q"\ba""#), b"\xe1");
    assert_eq!(one(b"u8\"\\b\t\""), b"\t");
    assert_eq!(one(b"u8\"\\b\xc3\xa9\""), b"\xc3\xa9");
    assert_eq!(one(br#"u8"\b\.""#), b"\xee\x82\x9c");
    assert_eq!(one(br#""\b\.""#), b"\x9c");
    assert_eq!(one(br#""\.""#), b"\x1c");
    // A space is the first byte bronze changes, 0x7F the last; 0x80 is a character above ASCII.
    assert_eq!(one(b"u8\"\\b \""), b"\xee\x82\xa0");
    assert_eq!(one(b"u8\"\\b\x1f\""), b"\x1f");
    assert_eq!(one(b"u8\"\\b\x7f\""), b"\xee\x83\xbf");
    assert_eq!(one(b"u8\"\\b\xc2\x80\""), b"\xc2\x80");
}

// spec FTEF-13.11: `\x` needs a hex digit, an escaped line break is a line break, and an
// escaped CR is the byte after it
#[test]
fn string_escapes_as_fteqcc_reads_them() {
    assert_eq!(
        fte_string(br#""\xg""#),
        (b"\x00g".to_vec(), vec![Code::E0103])
    );
    assert_eq!(fte_string(b"\"a\\\nb\""), (b"a\nb".to_vec(), vec![]));
    assert_eq!(fte_string(b"\"a\\\r\nb\""), (b"a\nb".to_vec(), vec![]));
    // FTEF-13.25: with FTEQCC's "Hanging \\" warning.
    assert_eq!(
        fte_string(b"\"a\\\rb\""),
        (b"ab".to_vec(), vec![Code::W0123])
    );
    // FTEF-13.26: the end of the input after `\` and CR is one "EOF inside quote".
    assert_eq!(
        fte_string(b"\"a\\\r"),
        (b"a\x00".to_vec(), vec![Code::E0101])
    );
    assert_eq!(fte_string(b"\"a\\"), (b"a".to_vec(), vec![Code::E0101]));
    assert_eq!(fte_string(b"\"a\nb\"").1, [Code::E0102, Code::E0101]);
}

/// The codes and messages of the lexical diagnostics of `source` under FTEQCC.
fn fte_diagnostics(source: &[u8]) -> Vec<(Code, String)> {
    lex_with(source, Dialect::Fteqcc)
        .diagnostics
        .into_iter()
        .map(|d| (d.code, d.message))
        .collect()
}

// spec FTEF-13.15: under FTEQCC only a space, CR, LF, tab and VT are white space; another
// control byte or DEL is "Unknown punctuation" with FTEQCC's text, and reading goes on after it
// spec QCD-8.18: E0105 has FTEQCC's text under an FTEQCC profile
#[test]
fn fteqcc_white_space_is_qcc_iswhite() {
    assert_eq!(
        kinds_in(b"a \r\n\t\x0bb", Dialect::Fteqcc),
        vec![name("a"), name("b")]
    );
    for (byte, text) in [(0x0c_u8, "c"), (0x01, "1"), (0x7f, "7f"), (0x1f, "1f")] {
        let source = [b'a', byte, b'b'];
        let lexed = lex_with(&source, Dialect::Fteqcc);
        assert_eq!(
            lexed
                .tokens
                .iter()
                .map(|t| t.kind.clone())
                .collect::<Vec<_>>(),
            vec![name("a"), name("b")],
            "{byte:#x}"
        );
        assert_eq!(
            fte_diagnostics(&source),
            vec![(Code::E0105, format!("Unknown punctuation: '\\x{text}'"))]
        );
        assert_eq!(lexed.diagnostics[0].span, Span::new(1, 2));
    }
    // The vanilla dialect keeps QC-2.1.
    assert_eq!(kinds(b"a\x0cb"), vec![name("a"), name("b")]);
    assert!(codes(b"a\x0c\x01b").is_empty());
    assert!(Dialect::Fteqcc.is_whitespace(0x0b) && !Dialect::Fteqcc.is_whitespace(0x0c));
    assert!(Dialect::Vanilla.is_whitespace(0x0c) && Dialect::Vanilla.is_whitespace(0xa0));
}

// spec FTEF-13.16: under FTEQCC a byte of 0x80 or more starts and continues a name, read as
// UTF-8; a byte that starts no sequence is "Invalid UTF-8 code sequence", no token is made and
// reading goes on at the next byte that starts one
// spec QCD-2.9: E0123 is FTEQCC's error for it, at the name
#[test]
fn fteqcc_names_are_utf8() {
    assert_eq!(
        kinds_in("é xéy _9é".as_bytes(), Dialect::Fteqcc),
        vec![
            name("\u{c3}\u{a9}"),
            name("x\u{c3}\u{a9}y"),
            name("_9\u{c3}\u{a9}")
        ]
    );
    assert!(fte_diagnostics("é".as_bytes()).is_empty());
    assert_eq!(
        fte_diagnostics(b"x\xc3;"),
        vec![(
            Code::E0123,
            "Invalid UTF-8 code sequence before ;. Lead byte was 0xc3".to_owned()
        )]
    );
    let lexed = lex_with(b"x\xc3;", Dialect::Fteqcc);
    assert_eq!(lexed.diagnostics[0].span, Span::new(0, 2));
    assert_eq!(lexed.tokens.len(), 1);
    assert_eq!(lexed.tokens[0].kind, punct(Punct::Semicolon));
    // The bytes that start no sequence are passed over together; white space before the word.
    assert_eq!(
        fte_diagnostics(b"\xa0\x80\x80 \t x;"),
        vec![(
            Code::E0123,
            "Invalid UTF-8 code sequence before x;. Lead byte was 0xa0".to_owned()
        )]
    );
    for source in [&b"a\xff \r\nb"[..], b"a\xff", b"a\xff\t\n"] {
        assert_eq!(
            fte_diagnostics(source),
            vec![(
                Code::E0123,
                "Invalid UTF-8 code sequence at end of line. Lead byte was 0xff".to_owned()
            )],
            "{}",
            source.escape_ascii()
        );
    }
    // A sequence cut short, an overlong one and one past U+10FFFF.
    for source in [&b"\xe2\x82"[..], b"\xc1\x81", b"\xf4\x90\x80\x80"] {
        assert_eq!(fte_diagnostics(source)[0].0, Code::E0123);
    }
    // The vanilla dialect keeps QC-1.3.
    assert_eq!(kinds(b"a\xc3\xa9b"), vec![name("a"), name("b")]);
}

// spec FTEF-13.17: a letter, digit, `_` or high byte right after an FTEQCC number is "bad suffix
// on number" with the number's `pr_token`, and the bytes after it are read on
// spec QCD-2.10: E0124 is that error, at the number
#[test]
fn fteqcc_bad_number_suffix() {
    for (source, token) in [
        ("5abc", "5"),
        ("5fx", "5f"),
        ("1.5e3", "1.5"),
        ("1.5fx", "1.5"),
        ("0X10g", "0x10"),
        ("-5_", "-5"),
        ("16iz", "16i"),
        ("5\u{e9}", "5"),
    ] {
        let lexed = lex_with(source.as_bytes(), Dialect::Fteqcc);
        let errors: Vec<(Code, String)> = lexed
            .diagnostics
            .iter()
            .map(|d| (d.code, d.message.clone()))
            .collect();
        assert_eq!(
            errors,
            vec![(Code::E0124, format!("bad suffix on number {token}"))],
            "{source}"
        );
        assert_eq!(lexed.diagnostics[0].span.start, 0, "{source}");
        assert!(lexed.tokens.len() >= 2, "{source}");
    }
    let octal = fte_diagnostics(b"08");
    assert_eq!(
        octal.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        vec![Code::W0110, Code::E0124]
    );
    assert_eq!(octal[1].1, "bad suffix on number 0");
    for clean in [
        &b"5 a"[..],
        b"5;",
        b"5f",
        b"1.5d",
        b"16ull",
        b"0x1f",
        b"'1x 2 3'",
    ] {
        assert!(
            fte_diagnostics(clean)
                .iter()
                .all(|(code, _)| *code != Code::E0124),
            "{}",
            clean.escape_ascii()
        );
    }
    // A vanilla number has no such check (QC-4.4).
    assert!(codes(b"5abc").is_empty());
}

// spec FTEF-13.18: `//` reads to the LF in `QCC_PR_LexComment` and to any line ending in
// `QCC_PR_LexWhitespace`, so a lone CR ends a comment only after a line ending since the token
// before it
#[test]
fn fteqcc_comments_end_as_its_readers_end_them() {
    // After a token on its line, the comment reads past the CR to the LF.
    assert_eq!(
        kinds_in(b"a // c\rb\nd", Dialect::Fteqcc),
        vec![name("a"), name("d")]
    );
    // After a line ending, the CR ends it.
    assert_eq!(
        kinds_in(b"a\n// c\rb\nd", Dialect::Fteqcc),
        vec![name("a"), name("b"), name("d")]
    );
    // Or the end of the text.
    assert_eq!(kinds_in(b"a\n// c", Dialect::Fteqcc), vec![name("a")]);
    // The next line's comment is `QCC_PR_LexWhitespace`'s, as `QCC_PR_NewLine` passed over its
    // spaces and tabs and no other white space reset `nl`; after a VT it is read to its LF too.
    assert_eq!(
        kinds_in(b"a // c\n  // e\rb\nd", Dialect::Fteqcc),
        vec![name("a"), name("b"), name("d")]
    );
    assert_eq!(
        kinds_in(b"a // c\n// e\rb\nd", Dialect::Fteqcc),
        vec![name("a"), name("b"), name("d")]
    );
    assert_eq!(
        kinds_in(b"a // c\n \x0b// e\rb\nd", Dialect::Fteqcc),
        vec![name("a"), name("d")]
    );
    // A blank line ends that reading.
    assert_eq!(
        kinds_in(b"a // c\n\n\x0b// e\rb\nd", Dialect::Fteqcc),
        vec![name("a"), name("b"), name("d")]
    );
    // A CR LF is no lone CR.
    assert_eq!(
        kinds_in(b"a\n// c\r\nb", Dialect::Fteqcc),
        vec![name("a"), name("b")]
    );
    // A `\` and a line break start the reading again: a CR then ends no comment.
    assert_eq!(
        kinds_in(b"a\\\r\n// c\rb\nd", Dialect::Fteqcc),
        vec![name("a"), name("d")]
    );
    // The vanilla dialect ends a `//` comment at its LF (QC-2.2).
    assert_eq!(kinds(b"a\n// c\rb\nd"), vec![name("a"), name("d")]);
}

// spec FTEF-13.18: an FTEQCC `/* */` comment's `*/` is looked for from the byte after its `*`,
// a `/*` inside it is a warning and one with no end is "EOF inside comment"
// spec QCD-7.55: W0112 is FTEQCC's F325, at the `/*`
// spec QCD-8.18: E0501 has FTEQCC's text under an FTEQCC profile
#[test]
fn fteqcc_block_comments() {
    assert_eq!(kinds_in(b"/*/ a */b", Dialect::Fteqcc), vec![name("b")]);
    assert_eq!(kinds_in(b"/**/b", Dialect::Fteqcc), vec![name("b")]);
    assert_eq!(kinds(b"/*/ a */b").len(), 4);
    assert_eq!(
        fte_diagnostics(b"/* a /* b */"),
        vec![(Code::W0112, "\"/*\" inside comment".to_owned())]
    );
    let lexed = lex_with(b"/* a /* b */", Dialect::Fteqcc);
    assert_eq!(lexed.diagnostics[0].span, Span::new(5, 7));
    // `/*/*` warns at its second `/*`; `/***/` holds none.
    assert_eq!(fte_diagnostics(b"/*/**/").len(), 1);
    assert!(fte_diagnostics(b"/***/").is_empty());
    assert_eq!(
        fte_diagnostics(b"a /* b"),
        vec![(Code::E0501, "EOF inside comment".to_owned())]
    );
    assert_eq!(
        lex_with(b"a /* b", Dialect::Fteqcc).diagnostics[0].span,
        Span::new(2, 4)
    );
    // A second comment after a token's first, and one in a later line's white space.
    assert_eq!(
        fte_diagnostics(b"a /* */ /* /* */\n/* /* */"),
        vec![
            (Code::W0112, "\"/*\" inside comment".to_owned()),
            (Code::W0112, "\"/*\" inside comment".to_owned())
        ]
    );
}

// spec FTEF-13.19: an FTEQCC number accumulates in a wrapping `long long`; a leading 0 before a
// digit warns; and an `f`, `d` or `i` suffix, or base 16 with none, warns where the value stored
// is not the number
// spec QCD-7.53: W0110 is FTEQCC's warning `octal`, at the `0`
// spec QCD-7.54: W0111 is FTEQCC's F312
#[test]
fn fteqcc_number_values_and_warnings() {
    let one = |source: &str| {
        let lexed = lex_with(source.as_bytes(), Dialect::Fteqcc);
        let TokenKind::Float(value) = lexed.tokens[0].kind else {
            panic!("{source}")
        };
        (value, lexed.diagnostics)
    };
    assert_eq!(one("18446744073709551621").0, 5.0);
    assert_eq!(one("0xffffffffffffffff").0, -1.0);
    assert_eq!(one("-18446744073709551621").0, -5.0);
    assert_eq!(one("9223372036854775813").0, -(2.0_f32.powi(63)));
    let (_, octal) = one("017");
    assert_eq!(octal.len(), 1);
    assert_eq!(octal[0].code, Code::W0110);
    assert_eq!(octal[0].message, "A leading 0 is interpreted as base-8.");
    assert_eq!(octal[0].span, Span::new(0, 1));
    assert!(one("0").1.is_empty() && one("0x10").1.is_empty() && one("0.5").1.is_empty());
    let warned = |source: &str| -> Vec<String> {
        one(source)
            .1
            .into_iter()
            .filter(|d| d.code == Code::W0111)
            .map(|d| d.message)
            .collect()
    };
    assert_eq!(
        warned("0x100000001"),
        vec!["numerical overflow 4294967297 will be rounded to 4294967296.000000"]
    );
    assert_eq!(
        warned("-0x1000001"),
        vec!["numerical overflow -16777217 will be rounded to -16777216.000000"]
    );
    // `2147483648u` keeps the negative `_int` -2147483648, which FTEQCC does not warn about.
    for silent in [
        "16777217",
        "0x1000000",
        "4294967295i",
        "0xffffffffi",
        "2147483648u",
        "5l",
        "16777216f",
    ] {
        assert!(warned(silent).is_empty(), "{silent}");
    }
    for overflow in [
        "16777217f",
        "9007199254740993d",
        "4294967313i",
        "4294967296u",
    ] {
        assert_eq!(warned(overflow), vec!["numerical overflow"], "{overflow}");
    }
    // The vanilla dialect has none of it (QC-4).
    assert!(codes(b"017 0x100000001").is_empty());
}

// spec FTEF-13.20: an FTEQCC string of more than 8191 bytes is "String length exceeds 8191", and
// a name or number of more than 8191 bytes is "token too long"; shorter ones, past id's 2047, are
// tokens
// spec QCD-8.18: E0502 has FTEQCC's text for a string
#[test]
fn fteqcc_token_limits() {
    let string = |len: usize| format!("\"{}\"", "s".repeat(len));
    assert!(fte_diagnostics(string(8191).as_bytes()).is_empty());
    assert_eq!(
        fte_diagnostics(string(8192).as_bytes()),
        vec![(Code::E0502, "String length exceeds 8191".to_owned())]
    );
    // Parts read together count together, the error at the part that passes 8191.
    let joined = |a: usize, b: usize| format!("{} /* c */\n{}", string(a), string(b));
    assert!(fte_diagnostics(joined(4000, 4191).as_bytes()).is_empty());
    let over = lex_with(joined(4000, 4192).as_bytes(), Dialect::Fteqcc);
    assert_eq!(over.diagnostics.len(), 1);
    assert_eq!(over.diagnostics[0].message, "String length exceeds 8191");
    assert_eq!(over.diagnostics[0].span.start, 4011);
    // Only the part that passes 8191 is the error, not the parts after it.
    let three = format!("{} {} {}", string(8000), string(500), string(5));
    assert_eq!(
        lex_with(three.as_bytes(), Dialect::Fteqcc)
            .diagnostics
            .len(),
        1
    );
    assert_eq!(
        fte_diagnostics(format!("{} x {}", string(4000), string(4192)).as_bytes()),
        Vec::new()
    );
    assert!(fte_diagnostics("n".repeat(8191).as_bytes()).is_empty());
    assert_eq!(
        fte_diagnostics("n".repeat(8192).as_bytes()),
        vec![(Code::E0502, "token too long".to_owned())]
    );
    assert!(fte_diagnostics("1".repeat(3000).as_bytes()).is_empty());
    assert_eq!(
        codes("n".repeat(QC_MAX_TOKEN).as_bytes()),
        vec![Code::E0502]
    );
}

// spec FTEF-13.21: a UTF-8 string's byte that starts no sequence, and one bronze cannot mask,
// are errors; an unknown escape names its byte
// spec QCD-8.18: E0103 has FTEQCC's texts under an FTEQCC profile
#[test]
fn fteqcc_string_input_errors() {
    for source in [&b"u8\"a\xffb\""[..], b"Q\"\xc3\"", b"u8\"\x80\""] {
        assert_eq!(
            fte_diagnostics(source),
            vec![(Code::E0103, "Input string is not valid utf-8".to_owned())],
            "{}",
            source.escape_ascii()
        );
    }
    assert!(fte_diagnostics(b"\"a\xffb\"").is_empty());
    assert_eq!(
        fte_diagnostics("Q\"\\b\u{e9}\"".as_bytes()),
        // The sequence's other bytes are read again, one at a time, and start none.
        vec![
            (
                Code::E0103,
                "Unable to mask non-ascii chars. Attempting to mask bytes".to_owned()
            ),
            (Code::E0103, "Input string is not valid utf-8".to_owned())
        ]
    );
    assert_eq!(
        fte_diagnostics(br#""a\qb""#),
        vec![(Code::E0103, "Unknown escape char q".to_owned())]
    );
}

// spec FTEF-13.22: FTEQCC's F301 and F311 warnings, which its settings ignore unless a pragma
// turns them on, are the lexer's to give
// spec QCD-7.62: W0119 is F301 with its text
// spec QCD-7.63: W0120 is F311's "2d vector"
#[test]
fn fteqcc_off_by_default_string_and_vector_warnings() {
    let prefix =
        "char32_t/char16_t/wchar_t strings are not supported, treating as u8 prefix (as utf-8)";
    for source in [&b"u\"a\""[..], b"U\"a\"", b"\"a\" L\"b\""] {
        assert_eq!(
            fte_diagnostics(source),
            vec![(Code::W0119, prefix.to_owned())],
            "{}",
            source.escape_ascii()
        );
    }
    assert!(fte_diagnostics(b"u8\"a\" Q\"b\" R\"(c)\"").is_empty());
    assert_eq!(
        fte_diagnostics("Q\"\u{100}\"".as_bytes()),
        vec![(
            Code::W0119,
            "Cannot convert codepoint 0x100 to quake's charset".to_owned()
        )]
    );
    // A plain string's bytes are written as they are, an escape's code converted.
    assert!(fte_diagnostics("\"\u{100}\"".as_bytes()).is_empty());
    assert_eq!(fte_diagnostics(br#""\{256}""#).len(), 1);
    assert!(
        fte_diagnostics("\"\\[\u{e9}\"".as_bytes())
            .iter()
            .all(|(c, _)| *c == Code::W0119)
    );
    assert!(
        fte_diagnostics(br#""\["#)
            .iter()
            .all(|(c, _)| *c != Code::W0119)
    );
    assert_eq!(
        fte_diagnostics(br#"u8"\U1100000""#),
        vec![(
            Code::W0119,
            "Bad unicode character code - codepoint is above 0x10FFFFu".to_owned()
        )]
    );
    assert_eq!(
        fte_diagnostics(b"'1 2'"),
        vec![(Code::W0120, "2d vector".to_owned())]
    );
    assert!(fte_diagnostics(b"'1 2 3'").is_empty());
}

// spec FTEF-13.23: an FTEQCC vector's components are read with `QCC_PR_LexWhitespace` before
// the first and after each
#[test]
fn fteqcc_vector_white_space_is_lex_whitespace_s() {
    assert_eq!(
        kinds_in(b"'/*c*/1 2 3'", Dialect::Fteqcc),
        vec![TokenKind::Vector([1.0, 2.0, 3.0])]
    );
    assert_eq!(
        kinds_in(b"'1 // c\r2 3'", Dialect::Fteqcc),
        vec![TokenKind::Vector([1.0, 2.0, 3.0])]
    );
    assert!(fte_diagnostics(b"'1 // c\r2 3'").is_empty());
    // id's reader takes the `/` as the first component's first byte (QC-5.1).
    assert!(codes(b"'/*c*/1 2 3'").contains(&Code::E0104));
}

// spec FTEF-13.25: `\` before a lone CR in an FTEQCC string is the byte after the CR, with
// "Hanging \\" and a CR; before a CR and an LF it is the LF
// spec QCD-7.66: W0123 has FTEQCC's text
#[test]
fn fteqcc_hanging_backslash_cr() {
    let lexed = lex_with(b"\"a\\\rb\"", Dialect::Fteqcc);
    assert_eq!(lexed.tokens[0].kind, TokenKind::String(b"ab".to_vec()));
    assert_eq!(
        fte_diagnostics(b"\"a\\\rb\""),
        vec![(Code::W0123, "Hanging \\\\\r".to_owned())]
    );
    let lexed = lex_with(b"\"a\\\r\nb\"", Dialect::Fteqcc);
    assert_eq!(lexed.tokens[0].kind, TokenKind::String(b"a\nb".to_vec()));
    assert!(lexed.diagnostics.is_empty());
}

// spec FTEF-13.26: a string that ends after a `\` and a CR is one "EOF inside quote", and a
// character constant one "Bad character constant"; neither warns "Hanging"
// spec QCD-2.1: E0101 is reported once for an unterminated string
#[test]
fn fteqcc_backslash_cr_at_the_end_of_input() {
    let lexed = lex_with(b"\"a\\\r", Dialect::Fteqcc);
    let codes: Vec<_> = lexed.diagnostics.iter().map(|d| (d.code, d.span)).collect();
    assert_eq!(codes, [(Code::E0101, Span::new(0, 4))]);
    assert_eq!(
        fte_diagnostics(b"'\\\r"),
        vec![(Code::E0104, "Bad character constant".to_owned())]
    );
}
