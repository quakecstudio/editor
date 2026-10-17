// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed index is a failed test"
)]

use std::fmt::Write as _;

use proptest::prelude::*;

use super::{Code, LineIndex, Punct, QC_MAX_TOKEN, Span, TokenKind, atof, lex};

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

// spec QCD-1.1: a code is E or W, four digits, the first two a group from 01 to 05
#[test]
fn code_format() {
    for code in Code::ALL {
        let text = code.as_str();
        let (letter, digits) = text.split_at(1);
        assert_eq!(letter == "E", code.is_error(), "{text}");
        assert!(letter == "E" || letter == "W", "{text}");
        assert_eq!(digits.len(), 4, "{text}");
        assert!(digits.bytes().all(|b| b.is_ascii_digit()), "{text}");
        assert!(
            ["01", "02", "03", "04", "05"].contains(&&digits[..2]),
            "{text}"
        );
    }
    let mut sorted = Code::ALL.map(Code::as_str).to_vec();
    sorted.dedup();
    assert_eq!(sorted.len(), Code::ALL.len());
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
