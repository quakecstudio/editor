// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's compiler flags as the lexer reads them (FTEF-12.12). Each case was compiled with the
//! pinned FTEQCC (`fteqcc -Tid`), which gave the same tokens, values and messages.

#![expect(
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed index or panic is a failed test"
)]

use crate::{Code, Dialect, LexFlag, LexFlags, Lexed, Punct, TokenKind, lex_flagged, lex_with};

fn with(flag: LexFlag) -> LexFlags {
    LexFlags::default().with(flag, true)
}

fn lexed(source: &[u8], flags: LexFlags) -> Lexed {
    lex_flagged(source, Dialect::Fteqcc, flags, &|_| None)
}

fn kinds(source: &[u8], flags: LexFlags) -> Vec<TokenKind> {
    lexed(source, flags)
        .tokens
        .into_iter()
        .map(|t| t.kind)
        .collect()
}

fn messages(lexed: &Lexed) -> Vec<(Code, &str)> {
    lexed
        .diagnostics
        .iter()
        .map(|d| (d.code, d.message.as_str()))
        .collect()
}

fn string(bytes: &[u8]) -> TokenKind {
    TokenKind::String(bytes.to_vec())
}

/// The bits of the one float `source` lexes to.
fn float_bits(source: &[u8], flags: LexFlags) -> u32 {
    match kinds(source, flags).as_slice() {
        [TokenKind::Float(value)] => value.to_bits(),
        other => panic!("not one float: {other:?}"),
    }
}

#[test]
fn flags_set_and_clear() {
    let flags = LexFlags::NONE.with(LexFlag::Qccx, true);
    assert!(flags.has(LexFlag::Qccx) && !flags.has(LexFlag::QcFuncs));
    assert!(!flags.with(LexFlag::Qccx, false).has(LexFlag::Qccx));
    assert!(LexFlags::default().has(LexFlag::QcFuncs));
    for flag in [
        LexFlag::QcFuncs,
        LexFlag::Qccx,
        LexFlag::Utf8Strings,
        LexFlag::Ilp32,
        LexFlag::Ssp,
        LexFlag::AssumeInt,
        LexFlag::MacroInStrings,
    ] {
        let alone = LexFlags::NONE.with(flag, true);
        assert!(alone.has(flag));
        assert_eq!(alone.with(flag, false), LexFlags::NONE);
    }
}

// spec FTEF-12.12: with `qcfuncs` off `\0` to `\7` are C's octal escapes of up to three digits,
// and `\8` or `\9` is "Bad character code"; on, the default, they are Quake's yellow digits
#[test]
fn qcfuncs_off_reads_octal_escapes() {
    let off = LexFlags::NONE;
    assert_eq!(kinds(br##""\101\7x""##, off), [string(b"A\x07x")]);
    assert_eq!(kinds(br##""\1018""##, off), [string(b"A8")]);
    let bad = lexed(br##""\8""##, off);
    assert_eq!(messages(&bad), [(Code::E0103, "Bad character code \\8")]);
    assert_eq!(
        messages(&lexed(br##""\9""##, off)),
        [(Code::E0103, "Bad character code \\9")]
    );
    // `0xe012 + c - '0'`, written as Quake's byte 0x13 for `\1`.
    assert_eq!(kinds(br##""\1""##, LexFlags::default()), [string(b"\x13")]);
    assert_eq!(
        kinds(br##""\101""##, LexFlags::default()),
        [string(b"\x1301")]
    );
}

// spec FTEF-12.12: under `qccx` a `:` after a string part writes a NUL into the string, and a
// `:` elsewhere is the punctuation
#[test]
fn qccx_colon_joins_string_parts_with_a_nul() {
    let qccx = with(LexFlag::Qccx);
    assert_eq!(
        kinds(br##""a" /* c */ : "b""##, qccx),
        [string(b"a"), string(b"\0"), string(b"b")]
    );
    assert_eq!(
        kinds(br##"x : "a""##, qccx),
        [
            TokenKind::Name("x".to_owned()),
            TokenKind::Punct(Punct::Colon),
            string(b"a")
        ]
    );
    assert_eq!(
        kinds(br##""a" : "b""##, LexFlags::default()),
        [string(b"a"), TokenKind::Punct(Punct::Colon), string(b"b")]
    );
}

// spec FTEF-12.12: under `qccx` a character constant is a float whose bits are its code, with
// "char constant: denormal", unless `assumeint` makes it an integer first
// spec QCD-7.64: W0121 is FTEQCC's F313 with its text
#[test]
fn qccx_character_constants_are_bits() {
    let qccx = with(LexFlag::Qccx);
    assert_eq!(float_bits(b"'a'", qccx), 97);
    assert_eq!(
        messages(&lexed(b"'a'", qccx)),
        [(Code::W0121, "char constant: denormal")]
    );
    // A UTF-8 sequence's code: U+00E9.
    assert_eq!(float_bits("'\u{e9}'".as_bytes(), qccx), 0xe9);
    assert_eq!(
        messages(&lexed("'\u{e9}'".as_bytes(), qccx)),
        [(Code::W0121, "char constant: denormal")]
    );
    // An escaped one stays the float of its code, with no warning.
    assert_eq!(kinds(br"'\n'", qccx), [TokenKind::Float(10.0)]);
    assert!(lexed(br"'\n'", qccx).diagnostics.is_empty());
    let both = qccx.with(LexFlag::AssumeInt, true);
    assert_eq!(kinds(b"'a'", both), [TokenKind::Float(97.0)]);
    assert!(lexed(b"'a'", both).diagnostics.is_empty());
    assert_eq!(kinds(b"'a'", LexFlags::default()), [TokenKind::Float(97.0)]);
}

// spec FTEF-12.12: under `qccx` `%` before a digit or `-` is "denormalized immediate", the
// float whose bits are `atoi` of what `QCC_PR_LexInteger` reads, so a hex one is 0
#[test]
fn qccx_percent_immediates() {
    let qccx = with(LexFlag::Qccx);
    assert_eq!(float_bits(b"%5", qccx), 5);
    assert_eq!(
        messages(&lexed(b"%5", qccx)),
        [(Code::W0121, "denormalized immediate")]
    );
    assert_eq!(float_bits(b"%0x5", qccx), 0);
    assert_eq!(float_bits(b"%12ab", qccx), 12);
    assert_eq!(float_bits(b"%-3", qccx), (-3_i32).cast_unsigned());
    // `do ... while`: the byte after `0x` is taken whatever it is.
    assert_eq!(kinds(b"%0xz;", qccx).len(), 2);
    assert_eq!(kinds(b"5 % 2", qccx)[1], TokenKind::Punct(Punct::Percent));
    assert_eq!(
        kinds(b"%5", LexFlags::default()),
        [TokenKind::Punct(Punct::Percent), TokenKind::Float(5.0)]
    );
}

// spec FTEF-12.12: under `qccx` a hex number with no suffix is a float whose bits are its low
// 32 bits, "numerical overflow" when those are not the number and are no negative `int`;
// `assumeint` comes first and gives no float's warning
#[test]
fn qccx_and_assumeint_hex_numbers() {
    let qccx = with(LexFlag::Qccx);
    assert_eq!(float_bits(b"0x10", qccx), 16);
    assert!(lexed(b"0x10", qccx).diagnostics.is_empty());
    assert_eq!(float_bits(b"0x100000001", qccx), 1);
    assert_eq!(
        messages(&lexed(b"0x100000001", qccx)),
        [(Code::W0111, "numerical overflow")]
    );
    assert_eq!(float_bits(b"0xffffffffffffffff", qccx), u32::MAX);
    assert!(lexed(b"0xffffffffffffffff", qccx).diagnostics.is_empty());
    // A decimal number is the float of its value.
    assert_eq!(kinds(b"16", qccx), [TokenKind::Float(16.0)]);
    let assume = with(LexFlag::AssumeInt);
    assert_eq!(float_bits(b"0x100000001", assume), 2_f32.powi(32).to_bits());
    assert!(lexed(b"0x100000001", assume).diagnostics.is_empty());
    assert!(
        lexed(b"0x100000001", qccx.with(LexFlag::AssumeInt, true))
            .diagnostics
            .is_empty()
    );
    assert_eq!(
        messages(&lexed(b"0x100000001", LexFlags::default())),
        [(
            Code::W0111,
            "numerical overflow 4294967297 will be rounded to 4294967296.000000"
        )]
    );
}

// spec FTEF-12.12: under `utf8` a plain string is read and written as UTF-8
#[test]
fn utf8_strings() {
    let utf8 = with(LexFlag::Utf8Strings);
    assert_eq!(kinds(br##""\{233}""##, utf8), [string("\u{e9}".as_bytes())]);
    assert_eq!(
        kinds(br##""\{233}""##, LexFlags::default()),
        [string(b"\xe9")]
    );
    assert_eq!(
        messages(&lexed(b"\"\xff\"", utf8)),
        [(Code::E0103, "Input string is not valid utf-8")]
    );
    assert!(
        lexed(b"\"\xff\"", LexFlags::default())
            .diagnostics
            .is_empty()
    );
}

// spec FTEF-12.12: under `ILP32` a number needs `ll` to be 64 bits
#[test]
fn ilp32_needs_two_ls() {
    let ilp32 = with(LexFlag::Ilp32);
    assert_eq!(
        messages(&lexed(b"0x100000000l", ilp32)),
        [(Code::W0111, "numerical overflow")]
    );
    assert!(lexed(b"0x100000000ll", ilp32).diagnostics.is_empty());
    assert!(
        lexed(b"0x100000000l", LexFlags::default())
            .diagnostics
            .is_empty()
    );
}

// spec FTEF-12.12: under `ssp` `**` is the token `*^`, with FTEQCC's F321
// spec QCD-7.61: W0118 has FTEQCC's text
#[test]
fn ssp_double_star() {
    let ssp = with(LexFlag::Ssp);
    assert_eq!(
        kinds(b"2**3", ssp),
        [
            TokenKind::Float(2.0),
            TokenKind::Punct(Punct::StarCaret),
            TokenKind::Float(3.0)
        ]
    );
    assert_eq!(
        messages(&lexed(b"2**3", ssp)),
        [(Code::W0118, "** is unsafe around pointers, use *^ instead.")]
    );
    assert_eq!(
        kinds(b"**", LexFlags::default()),
        [TokenKind::Punct(Punct::Star), TokenKind::Punct(Punct::Star)]
    );
    // The vanilla dialect reads no flag.
    let vanilla = lex_flagged(b"**", Dialect::Vanilla, ssp, &|_| None);
    assert_eq!(vanilla.tokens.len(), 2);
}

/// FTEQCC's macros for the `macroinstrings` cases: `FOO` is `bar` and two spaces, `BAR` names
/// `FOO`, `SELF` names itself and `BIG` is 200 bytes.
fn macro_text(name: &[u8]) -> Option<Vec<u8>> {
    match name {
        b"FOO" => Some(b"bar  ".to_vec()),
        b"BAR" => Some(b"FOO".to_vec()),
        b"SELF" => Some(b"SELF".to_vec()),
        b"BIG" => Some(vec![b'b'; 200]),
        b"EMPTY" => Some(Vec::new()),
        _ => None,
    }
}

fn in_strings(source: &[u8]) -> Lexed {
    lex_flagged(
        source,
        Dialect::Fteqcc,
        with(LexFlag::MacroInStrings),
        &macro_text,
    )
}

// spec FTEF-12.12: under `macroinstrings` `#NAME` in a string is the text the name's macros
// end on, trailing spaces and tabs dropped; a name of no macro leaves the `#`
// spec QCD-7.65: W0122 has FTEQCC's two texts
#[test]
fn macro_names_in_strings() {
    let found = in_strings(br##""a#BAR.c""##);
    assert_eq!(found.tokens[0].kind, string(b"abar.c"));
    assert_eq!(
        messages(&found),
        [(Code::W0122, "Macro BAR expansion in string")]
    );
    let missing = in_strings(br##""x#nope y""##);
    assert_eq!(missing.tokens[0].kind, string(b"x#nope y"));
    assert_eq!(
        messages(&missing),
        [(Code::W0122, "Unable to expand string macro nope")]
    );
    let bare = in_strings(br##""#""##);
    assert_eq!(bare.tokens[0].kind, string(b"#"));
    assert!(bare.diagnostics.is_empty());
    assert_eq!(in_strings(br##""#EMPTY.x""##).tokens[0].kind, string(b".x"));
    // The name stops at FTEQCC's stop bytes, `#` among them.
    assert_eq!(
        in_strings(br##""#FOO#FOO""##).tokens[0].kind,
        string(b"barbar")
    );
    // The `#` of no macro is written with no mask; without the flag `\s` masks it.
    assert_eq!(
        in_strings(br##""\s#nope""##).tokens[0].kind,
        string(b"#\xee\xef\xf0\xe5")
    );
    assert_eq!(kinds(br##""\s#""##, LexFlags::default()), [string(b"\xa3")]);
    assert_eq!(
        kinds(br##""#FOO""##, LexFlags::default()),
        [string(b"#FOO")]
    );
}

// spec FTEF-12.12: the lookup of a macro whose text names itself stops after 64 macros, and a
// string the text would take to 8191 bytes is "String length exceeds 8191"
#[test]
fn macro_names_in_strings_are_bounded() {
    assert_eq!(in_strings(br##""#SELF""##).tokens[0].kind, string(b"SELF"));
    let long = format!("\"{}\" \"#BIG\"", "s".repeat(7991));
    let over = in_strings(long.as_bytes());
    assert_eq!(
        messages(&over),
        [
            (Code::W0122, "Macro BIG expansion in string"),
            (Code::E0502, "String length exceeds 8191")
        ]
    );
    let fits = format!("\"{}\" \"#BIG\"", "s".repeat(7990));
    assert_eq!(
        messages(&in_strings(fits.as_bytes())),
        [(Code::W0122, "Macro BIG expansion in string")]
    );
}

// spec FTEF-13.24: FTEQCC's punctuation is `pr_punctuation[]` in its order, with `<=>`, `*^`,
// `><`, `@` and `^^`
#[test]
fn fteqcc_punctuation_table() {
    let puncts = |source: &[u8]| -> Vec<Punct> {
        lex_with(source, Dialect::Fteqcc)
            .tokens
            .into_iter()
            .filter_map(|t| match t.kind {
                TokenKind::Punct(p) => Some(p),
                _ => None,
            })
            .collect()
    };
    assert_eq!(puncts(b"a<=>b"), [Punct::Spaceship]);
    assert_eq!(puncts(b"a><b"), [Punct::Cross]);
    assert_eq!(puncts(b"a*^b"), [Punct::StarCaret]);
    assert_eq!(puncts(b"@a"), [Punct::At]);
    assert_eq!(puncts(b"a^^b"), [Punct::CaretCaret]);
    assert_eq!(puncts(b"a^b"), [Punct::Caret]);
    assert_eq!(puncts(b"a<=b"), [Punct::LessEq]);
    assert_eq!(puncts(b"a>>=b"), [Punct::ShrEq]);
    assert_eq!(puncts(b"a...b"), [Punct::Ellipsis]);
    let vanilla: Vec<_> = lex_with(b"a<=>b", Dialect::Vanilla)
        .tokens
        .into_iter()
        .map(|t| t.kind)
        .collect();
    assert_eq!(vanilla[1], TokenKind::Punct(Punct::LessEq));
    for punct in Punct::FTEQCC_TABLE {
        let text = punct.as_str();
        assert_eq!(puncts(text.as_bytes()), [punct], "{text}");
    }
}

// spec FTEF-12.12: flags that change at a byte hold from the first token at or after it, the text
// lexed in one pass, so a string part read after the change joins the one before it
#[test]
fn marked_flags_change_in_one_pass() {
    let source = b"\"a\"\n:\"b\" 'a'";
    let qccx = with(LexFlag::Qccx);
    let lexed = crate::lex_marked(source, Dialect::Fteqcc, &[(4, qccx)], &|_| None);
    let kinds: Vec<TokenKind> = lexed.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(kinds[..3], [string(b"a"), string(b"\0"), string(b"b")]);
    assert_eq!(kinds[3], TokenKind::Float(f32::from_bits(97)));
    // Before the mark the defaults hold, and a mark past the text changes nothing.
    let lexed = crate::lex_marked(source, Dialect::Fteqcc, &[(100, qccx)], &|_| None);
    assert_eq!(lexed.tokens[1].kind, TokenKind::Punct(Punct::Colon));
    assert_eq!(lexed.tokens[3].kind, TokenKind::Float(97.0));
    // A token that starts mid-line after a mark is no line start.
    let lexed = crate::lex_marked(b"a b", Dialect::Fteqcc, &[(2, qccx)], &|_| None);
    assert!(!lexed.tokens[1].line_start);
}
