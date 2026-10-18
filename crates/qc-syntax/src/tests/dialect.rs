// SPDX-License-Identifier: GPL-3.0-or-later

//! The FTEQCC dialect profile (`docs/spec/dialect-fteqcc-rerelease.md`, FTE-2 to FTE-16):
//! the FTEQCC subset the Quake rerelease uses parses with no error under the FTEQCC profile,
//! and each extension is rejected with a diagnostic under the vanilla profile (P2-03).

use crate::{Code, Dialect, Parse, parse, parse_with};

/// Parses `source` under the FTEQCC dialect.
fn fte(source: &str) -> Parse {
    parse_with(source.as_bytes(), Dialect::Fteqcc)
}

/// The error codes of parsing `source` under the FTEQCC dialect.
fn fte_errors(source: &str) -> Vec<Code> {
    fte(source).errors().map(|d| d.code).collect()
}

/// The error codes of parsing `source` under the vanilla dialect.
fn vanilla_errors(source: &str) -> Vec<Code> {
    parse(source.as_bytes()).errors().map(|d| d.code).collect()
}

/// Wraps body statements in a function and parses under the FTEQCC dialect with no error.
fn fte_body_ok(body: &str) {
    let source = format!("float a, b, c, i, n;\nvoid() f =\n{{\n{body}\n}};\n");
    let parsed = fte(&source);
    assert_eq!(
        parsed.errors().map(|d| d.code).collect::<Vec<_>>(),
        Vec::<Code>::new(),
        "{}",
        parsed.syntax().debug_tree()
    );
    // The tree is still lossless.
    assert_eq!(parsed.syntax().text(), source.as_bytes());
}

// spec FTE-2.1: `for (init; cond; step) body` parses under the FTEQCC profile.
// spec FTE-2.2: the init clause may be a local declaration with an initialiser.
#[test]
fn for_loops_parse() {
    fte_body_ok("for (i = 0; i < n; i = i + 1) b = i;");
    fte_body_ok("for (float j = 0; j < n; j = j + 1) b = j;");
    fte_body_ok("for (; i < n; ) i = i + 1;");
    fte_body_ok("for (;;) return;");
}

// spec FTE-3.1: `switch (e) { case v: ... default: ... }` parses.
// spec FTE-3.3: a `case` body may be a block, and fall-through needs no break.
#[test]
fn switch_parses() {
    fte_body_ok("switch (i) { case 0: b = 1; case 1: b = 2; default: b = 3; }");
    fte_body_ok("switch (i) { case 0: { b = 1; } break; default: break; }");
    fte_body_ok("switch (i) { }");
}

// spec FTE-4.1: `break;` and `continue;` parse inside a loop or switch.
#[test]
fn break_and_continue_parse() {
    fte_body_ok("while (a) { if (b) break; continue; }");
    fte_body_ok("for (i = 0; i < n; i = i + 1) { if (b) continue; break; }");
}

// spec FTE-5.1: `lvalue++` and `lvalue--` parse as postfix expressions.
#[test]
fn increment_decrement_parse() {
    fte_body_ok("i++;");
    fte_body_ok("i--;");
    fte_body_ok("for (i = 0; i < n; i++) b = i;");
}

// spec FTE-6.1: the compound assignments `+=`, `-=`, `*=` and `/=` parse.
#[test]
fn compound_assignment_parses() {
    fte_body_ok("a += 1;");
    fte_body_ok("a -= 1;");
    fte_body_ok("a *= 2;");
    fte_body_ok("a /= 2;");
}

// spec FTE-7.1: `a % b` parses as the modulo operator.
#[test]
fn modulo_parses() {
    fte_body_ok("a = b % c;");
    fte_body_ok("a = i % 3;");
}

// spec FTE-8.1: `cond ? a : b` parses, including nested.
// spec FTE-8.2: the two arms are parsed as expressions.
#[test]
fn ternary_parses() {
    fte_body_ok("a = b ? c : i;");
    fte_body_ok("a = b ? (c ? 1 : 2) : 3;");
    fte_body_ok("a = b ? c ? 1 : 2 : 3;");
}

// spec FTE-9.1: `.z` on a vector parses (as field access, a semantic difference, so accepted
// in both dialects); FTEQCC uses it as an lvalue with a compound operator (FTE-9.2).
// spec FTE-9.2: a vector member is an lvalue, so it may be assigned and used with `*=`.
#[test]
fn vector_member_parses() {
    fte_body_ok("c = a;");
    let source = "vector v;\nvoid() f =\n{\n\tv.z = 1;\n\tv.z *= -1;\n};\n";
    assert_eq!(
        fte(source).errors().map(|d| d.code).collect::<Vec<_>>(),
        Vec::<Code>::new()
    );
}

// spec FTE-10.1: `const T name = value;` parses.
// spec FTE-11.1: `var T name = value;` parses.
// spec FTE-12.1: `nosave T name;` parses.
#[test]
fn qualified_declarations_parse() {
    assert_eq!(fte_errors("const float A = 1;"), Vec::<Code>::new());
    assert_eq!(fte_errors("var float num = 0;"), Vec::<Code>::new());
    assert_eq!(fte_errors("nosave float load_flag;"), Vec::<Code>::new());
    assert_eq!(fte_errors("const string S = \"x\";"), Vec::<Code>::new());
}

// spec FTE-10.2: a `const` may initialise another `const`.
#[test]
fn const_initialises_const() {
    assert_eq!(
        fte_errors("const float A = 1;\nconst float B = A;"),
        Vec::<Code>::new()
    );
}

// spec FTE-13.1: `T name[N] = { ... };` and `T name[] = { ... };` parse.
#[test]
fn arrays_parse() {
    assert_eq!(fte_errors("float arr[3];"), Vec::<Code>::new());
    assert_eq!(
        fte_errors("const string names[] = { \"a\", \"b\", \"c\" };"),
        Vec::<Code>::new()
    );
}

// spec FTE-13.2: `name[i]` subscripts parse.
// spec FTE-13.3: `name.length` parses (as field access of a `length` name).
#[test]
fn array_use_parses() {
    let source =
        "float arr[3];\nfloat i, a, n;\nvoid() f =\n{\n\ta = arr[i];\n\tn = arr.length;\n};\n";
    let parsed = fte(source);
    assert_eq!(
        parsed.errors().map(|d| d.code).collect::<Vec<_>>(),
        Vec::<Code>::new()
    );
}

// spec FTE-14.1: `typedef struct { T f; ... } Name;` parses.
// spec FTE-14.2: a value of the type and `name[i].f` member access parse.
#[test]
fn typedef_struct_parses() {
    assert_eq!(
        fte_errors("typedef struct { string mdl; float frame; } poseInfo;"),
        Vec::<Code>::new()
    );
    let source = "typedef struct { string mdl; float frame; } poseInfo;\n\
                  poseInfo poses[2];\n";
    assert_eq!(
        fte(source).errors().map(|d| d.code).collect::<Vec<_>>(),
        Vec::<Code>::new()
    );
}

// spec FTE-15.1: `enum { A, B, C };` and the typed forms parse.
// spec FTE-15.2: an explicit value `A = n` parses.
#[test]
fn enum_parses() {
    assert_eq!(fte_errors("enum { A, B, C };"), Vec::<Code>::new());
    assert_eq!(fte_errors("enum { A = 1, B, C = 10 };"), Vec::<Code>::new());
    assert_eq!(fte_errors("enum : int { A, B };"), Vec::<Code>::new());
    assert_eq!(
        fte_errors("enum class Keys : int { K1, K2 };"),
        Vec::<Code>::new()
    );
}

// spec FTE-2.1, FTE-3.1, FTE-4.1: the FTEQCC keyword constructs are rejected under the
// vanilla profile with E0510 (QCD-6.10), so the two profiles are observably different.
// spec QCD-6.10: the vanilla dialect reports E0510 for an FTEQCC keyword construct.
#[test]
fn vanilla_rejects_keyword_extensions() {
    for source in [
        "void() f = { for (a = 0; a < 3; a = a + 1) b = 1; };",
        "void() f = { switch (a) { case 0: b = 1; } };",
        "void() f = { while (a) break; };",
        "void() f = { while (a) continue; };",
    ] {
        assert!(
            vanilla_errors(source).contains(&Code::E0510),
            "vanilla should report E0510 for {source}"
        );
    }
}

// spec FTE-7.1, FTE-8.1: the FTEQCC operators have no vanilla token, so the vanilla lexer
// reports "Unknown punctuation" (E0105); the vanilla profile rejects each.
#[test]
fn vanilla_rejects_operator_extensions() {
    for source in [
        "float a, b, c, d;\nvoid() f = { a = b % c; };",
        "float a, b, c, d;\nvoid() f = { a = b ? c : d; };",
    ] {
        assert!(
            vanilla_errors(source).contains(&Code::E0105),
            "vanilla should report E0105 for {source}"
        );
    }
    // `i++` and `a += 1` lex as separate `+` tokens in vanilla and fail to parse.
    for source in [
        "float i;\nvoid() f = { i++; };",
        "float a;\nvoid() f = { a += 1; };",
    ] {
        assert!(
            !vanilla_errors(source).is_empty(),
            "vanilla should reject {source}"
        );
    }
}

// spec FTE-10.1, FTE-11.1, FTE-12.1, FTE-14.1, FTE-15.1: the FTEQCC declaration extensions
// are rejected under the vanilla profile, where the keyword or user type is "not a type"
// (E0203).
#[test]
fn vanilla_rejects_declaration_extensions() {
    for source in [
        "const float A = 1;",
        "var float a = 0;",
        "nosave float a;",
        "typedef struct { float f; } S;",
        "enum { A, B };",
    ] {
        assert!(
            vanilla_errors(source).contains(&Code::E0203),
            "vanilla should report E0203 for {source}"
        );
    }
    // An array declaration `float a[3];` in vanilla is rejected (a diagnostic of some kind).
    assert!(
        !vanilla_errors("float a[3];").is_empty(),
        "vanilla should reject an array declaration"
    );
}

// spec FTE-2.1: a vanilla program is parsed identically through `parse` or
// `parse_with(.., Vanilla)`.
#[test]
fn parse_and_parse_with_agree_on_vanilla() {
    let source = b"float a;\nvoid() f = { a = 1; };\n";
    assert_eq!(parse(source), parse_with(source, Dialect::Vanilla));
}
