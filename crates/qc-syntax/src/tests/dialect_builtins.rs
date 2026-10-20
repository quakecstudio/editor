// SPDX-License-Identifier: GPL-3.0-or-later

//! Builtin initialisers as FTEQCC's lexer and parser read them
//! (`docs/spec/dialect-fteqcc-rerelease.md`, FTE-23).

use crate::ast;
use crate::{Code, Dialect, Parse, builtin_binum, parse, parse_with};

fn fte(source: &str) -> Parse {
    parse_with(source.as_bytes(), Dialect::Fteqcc)
}

fn errors(parsed: &Parse) -> Vec<Code> {
    parsed.errors().map(|d| d.code).collect()
}

/// The builtin initialiser of the first declarator of `parsed`.
fn builtin(parsed: &Parse) -> ast::Builtin {
    let declarator = parsed
        .tree()
        .unwrap()
        .declarations()
        .next()
        .unwrap()
        .declarators()
        .next()
        .unwrap();
    let Some(ast::Initializer::Builtin(builtin)) = declarator.initializer() else {
        panic!("no builtin initialiser");
    };
    builtin
}

/// The number the FTEQCC profile binds and the engine name of `void() f = <init>;`, which
/// must parse with no error.
fn binding(init: &str) -> (Option<i32>, Option<String>) {
    let source = format!("void() f = {init};\n");
    let parsed = fte(&source);
    assert_eq!(errors(&parsed), [], "{source}");
    let builtin = builtin(&parsed);
    (
        builtin
            .number()
            .and_then(|t| builtin_binum(&t.text_lossless(), Dialect::Fteqcc)),
        builtin.engine_name(),
    )
}

// spec FTE-23.1: under FTEQCC a `#` is a builtin's only right before `"`, `-` or a digit;
// before a space, a quote, a `.`, a `+` or a name it is a macro use, E0209
#[test]
fn hash_is_a_builtin_only_before_a_quote_minus_or_digit() {
    for init in [
        "# 5", "# \"x\"", "#'a'", "#'\\n'", "#.5", "#.0", "#+5", "# -5", "#\t5", "#/**/5", "#ex_a",
        "#(5)", "#",
    ] {
        let source = format!("void() f = {init};\n");
        assert_eq!(errors(&fte(&source)), [Code::E0209], "{init}");
    }
    assert_eq!(binding("#5"), (Some(5), None));
    assert_eq!(binding("#-5"), (Some(-5), None));
    assert_eq!(binding("#\"x\""), (None, Some("x".to_owned())));
    assert_eq!(binding("#0x10:ex_a"), (Some(16), Some("ex_a".to_owned())));
}

// spec FTE-23.1: `:` has no such rule, and the vanilla `#` takes a number after a space, as id's
// qcc binds `# 5` as 5
#[test]
fn colon_and_the_vanilla_hash_take_any_spacing() {
    assert_eq!(binding(": 5"), (Some(5), None));
    assert_eq!(binding(":'a'"), (Some(97), None));
    assert_eq!(binding(": \"nmr\""), (None, Some("nmr".to_owned())));
    assert_eq!(binding(": ex_a"), (None, Some("ex_a".to_owned())));
    let vanilla = parse(b"void() f = # 5;\n");
    assert_eq!(errors(&vanilla), []);
    let number = builtin(&vanilla).number().unwrap().text_lossless();
    assert_eq!(builtin_binum(&number, Dialect::Vanilla), Some(5));
}

// spec FTE-23.2: C's `atof` reads an FTEQCC hex number with a fraction as a hexadecimal float,
// each digit after the `.` a sixteenth of the one before
#[test]
fn hex_numbers_with_a_fraction_are_hex_floats() {
    assert_eq!(binding("#0x10.0"), (Some(16), None));
    assert_eq!(binding("#-0x10.0"), (Some(-16), None));
    assert_eq!(binding("#0x2."), (Some(2), None));
    assert_eq!(errors(&fte("void() f = #0x1.8;\n")), [Code::E0209]);
}

// spec FTE-23.3: a `d` literal is a double, which no branch of FTEQCC's builtin initialiser
// takes, E0209; in hex `d` is a digit, and an `f` literal is a float
#[test]
fn double_literals_are_bad_builtin_immediates() {
    for init in [
        "#16d",
        "#16.0d",
        "#16D",
        "#0d",
        "#010d",
        "#16.5d",
        "#0x1.5d",
        ":16d",
        ": 16.0d",
        "#16d:ex_a",
    ] {
        let source = format!("void() f = {init};\n");
        assert_eq!(errors(&fte(&source)), [Code::E0209], "{init}");
    }
    for number in ["16d", "16.0d", "16D", "0d", "-16d", "0x1.5d", "0X1.0D"] {
        assert_eq!(builtin_binum(number, Dialect::Fteqcc), None, "{number}");
    }
    assert_eq!(builtin_binum("0x1d", Dialect::Fteqcc), Some(29));
    assert_eq!(builtin_binum("-0x1d", Dialect::Fteqcc), Some(-29));
    assert_eq!(builtin_binum("16f", Dialect::Fteqcc), Some(16));
    assert_eq!(builtin_binum("16.0f", Dialect::Fteqcc), Some(16));
    assert_eq!(binding("#0x10f"), (Some(271), None));
}

// spec FTE-23.4: a `-` is part of the number only right before a digit, so `#- 5` and `#-.5`
// read the `-` alone and are E0209
#[test]
fn minus_belongs_to_the_number_only_before_a_digit() {
    assert_eq!(binding("#-0x10"), (Some(-16), None));
    assert_eq!(binding(":-5i"), (Some(-5), None));
    for init in ["#- 5", "#-.5", ":- 5", ":-.5"] {
        let source = format!("void() f = {init};\n");
        assert_eq!(errors(&fte(&source)), [Code::E0209], "{init}");
    }
}

// spec FTE-23.5: adjacent strings are one name, across spaces, comments and lines; an empty
// string names nothing, so a `:name` may follow it
#[test]
fn adjacent_strings_join_and_an_empty_one_names_nothing() {
    let name = |n: &str| (None, Some(n.to_owned()));
    assert_eq!(binding("#\"ab\" \"cd\""), name("abcd"));
    assert_eq!(binding(":\"x\" \"y\""), name("xy"));
    assert_eq!(binding("#\"ab\"\"cd\" \"ef\""), name("abcdef"));
    assert_eq!(binding("#\"ab\" /* c */ \"cd\""), name("abcd"));
    assert_eq!(binding("#\"ab\" // c\n\"cd\""), name("abcd"));
    assert_eq!(binding("#\"a\\n\" \"b\""), name("a\nb"));
    assert_eq!(binding("#\"\""), (None, None));
    assert_eq!(binding("#\"\" \"\""), (None, None));
    assert_eq!(binding("#\"\":ex_b"), name("ex_b"));
    assert_eq!(binding(":\"\" \"ab\""), name("ab"));
    // A name follows only an empty string.
    assert_eq!(
        errors(&fte("void() f = #\"ab\" \"\":ex_b;\n")),
        [Code::E0203]
    );
}

// spec FTE-23.6: FTEQCC keeps 255 bytes of an engine name, a string's or a name's
#[test]
fn engine_names_keep_255_bytes() {
    let long = "s".repeat(300);
    let kept = Some("s".repeat(255));
    assert_eq!(binding(&format!("#\"{long}\"")).1, kept);
    assert_eq!(
        binding(&format!(":\"{}\" \"{}\"", &long[..200], &long[..100])).1,
        kept
    );
    assert_eq!(binding(&format!(":{long}")).1, kept);
    assert_eq!(binding(&format!("#\"{}\"", "s".repeat(255))).1, kept);
    assert_eq!(
        binding(&format!("#\"{}\"", "s".repeat(254))).1,
        Some("s".repeat(254))
    );
}

// spec FTE-23.6: a string's name ends at its first NUL byte, so one that starts with it names
// nothing and takes the name after `:`; a name keeps its bytes, each one `char`
#[test]
fn engine_names_end_at_a_nul_and_keep_their_bytes() {
    let name = |n: &str| (None, Some(n.to_owned()));
    assert_eq!(binding(r#"#"ab\x00cd""#), name("ab"));
    assert_eq!(binding(r#"#"ab" "\x00cd""#), name("ab"));
    assert_eq!(binding(r#"#"\x00":ex_b"#), name("ex_b"));
    assert_eq!(binding(r#"#"\x00" "cd":ex_b"#), name("ex_b"));
    assert_eq!(binding(r#"#"\x00""#), (None, None));
    assert_eq!(binding(r#"#"\bab""#), name("\u{e1}\u{e2}"));
    assert_eq!(binding(r#"#"\bcd""#), name("\u{e3}\u{e4}"));
}

// spec FTEF-13.7: a prefixed string after `:` or after another string names the builtin, an
// `L"..."` after another part too, with the bytes its prefix gives it; after `#` a prefix is a
// macro use, E0209
#[test]
fn prefixed_strings_name_builtins() {
    let name = |n: &str| (None, Some(n.to_owned()));
    for init in [
        ":u8\"ex_a\"",
        ":Q\"ex_a\"",
        ":u\"ex_a\"",
        ":U\"ex_a\"",
        ":R\"(ex_a)\"",
        ":R\"x(ex_a)x\"",
        "#\"ex_\" u8\"a\"",
        "#\"ex_\" L\"a\"",
        "#\"ex_\" R\"(a)\"",
        "#\"\" u8\"ex_a\"",
    ] {
        assert_eq!(binding(init), name("ex_a"), "{init}");
    }
    assert_eq!(binding(":u8\"\\[\""), name("\u{ee}\u{80}\u{90}"));
    assert_eq!(binding(":\"\\[\""), name("\u{10}"));
    for init in ["#u8\"ex_a\"", "#R\"(ex_a)\"", "#Q\"ex_a\"", ":L\"ex_a\""] {
        let source = format!("void() f = {init};\n");
        assert!(!errors(&fte(&source)).is_empty(), "{init}");
    }
}

// spec FTEF-15.7: under FTEQCC a name of 254 bytes is a name and one of 255 is "name too
// long", in a declaration, after `#number:` and in an expression
#[test]
fn fteqcc_names_are_up_to_254_bytes() {
    let ok = "n".repeat(crate::FTEQCC_MAX_NAME - 2);
    let long = "n".repeat(crate::FTEQCC_MAX_NAME - 1);
    assert_eq!(binding(&format!("#5:{ok}")), (Some(5), Some(ok.clone())));
    let error = |source: String| errors(&fte(&source));
    assert_eq!(error(format!("void() f = #5:{long};\n")), [Code::E0202]);
    assert_eq!(error(format!("void() {ok} = #5;\n")), []);
    assert_eq!(error(format!("void() {long} = #5;\n")), [Code::E0202]);
    assert_eq!(error(format!("float {ok};\n")), []);
    let term = |name: &str| format!("float {ok};\nvoid() g = {{ {ok} = {name}; }};\n");
    assert_eq!(error(term(&ok)), []);
    assert_eq!(error(term(&long)), [Code::E0202]);
    assert_eq!(error(format!("void() g = {{ a = A::{long}; }};\n")), [Code::E0202]);
    assert_eq!(error(format!("void() g = {{ a = A::{ok}; }};\n")), []);
}

// spec QC-3.3: the vanilla dialect keeps id's limit, so a name of 63 bytes is "name too long"
// where FTEQCC takes it
#[test]
fn the_vanilla_name_limit_stays() {
    let name = "n".repeat(crate::LIMIT_QCC_MAX_NAME - 1);
    let source = format!("void() {name} = #5;\n");
    assert_eq!(errors(&parse(source.as_bytes())), [Code::E0202]);
    assert_eq!(errors(&fte(&source)), []);
}
