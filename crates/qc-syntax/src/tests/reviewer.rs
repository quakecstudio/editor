// SPDX-License-Identifier: GPL-3.0-or-later

//! Adversarial tests from the P2-R2 readiness review (TEST-9): the FTEQCC profile at the
//! nesting limit and its builtin initialisers, written by reviewers who did not write the
//! parser.

use super::{Program, errors};
use crate::QC_MAX_NESTING;
use crate::ast::{AstNode as _, Builtin};
use crate::{Code, Dialect, SyntaxNode, parse_with};

/// The deepest chain of nodes from `root` to a leaf, counted without recursion.
fn tree_depth(root: &SyntaxNode) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(root.clone(), 1_usize)];
    while let Some((node, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        for child in node.children() {
            stack.push((child, depth.saturating_add(1)));
        }
    }
    deepest
}

/// A program with `body` as the statements of `f`, parsed under the FTEQCC profile.
fn fte_body(body: &str) -> crate::Parse {
    let source = Program {
        body,
        ..Program::default()
    }
    .source();
    parse_with(source.as_bytes(), Dialect::Fteqcc)
}

// spec QC-27.15: the FTEQCC profile keeps the vanilla limit for vanilla constructs, so
// parentheses nest exactly as deep under both profiles before "nesting too deep"
#[test]
fn reviewer_fteqcc_keeps_the_vanilla_parenthesis_limit() {
    let parens = |n: usize| format!("a = {}a{};", "(".repeat(n), ")".repeat(n));
    assert_eq!(errors(&fte_body(&parens(QC_MAX_NESTING - 4))), vec![]);
    assert_eq!(
        errors(&fte_body(&parens(QC_MAX_NESTING - 3))),
        vec![(Code::E0509, "nesting too deep".to_owned())]
    );
}

// spec QC-27.15: under the FTEQCC profile a chain of the FTEQCC operators that nest the
// expression before them (`?:`, `?:` with no middle, `->`, `[]`, the comma operator, a cast,
// `sizeof`) is an error past the limit, the tree stays bounded and every byte is kept
#[test]
fn reviewer_fteqcc_operator_chains_stop_at_the_limit() {
    let n = 20_000;
    for body in [
        format!("a = {}1;", "a ? 1 : ".repeat(n)),
        format!("a = {}1;", "a ?: ".repeat(n)),
        format!("e{};", "->en".repeat(n)),
        format!("a{};", "[0]".repeat(n)),
        format!("{}a;", "a, ".repeat(n)),
        format!("a = {}a;", "(float)".repeat(n)),
        format!("a = {}a;", "sizeof ".repeat(n)),
        format!("a = {}a;", "!".repeat(n)),
    ] {
        let parsed = fte_body(&body);
        assert!(
            parsed.errors().any(|d| d.code == Code::E0509),
            "{}",
            &body[..20]
        );
        assert!(
            tree_depth(&parsed.syntax()) <= QC_MAX_NESTING.saturating_mul(4),
            "{}",
            &body[..20]
        );
        let source = Program {
            body: &body,
            ..Program::default()
        }
        .source();
        assert_eq!(parsed.syntax().text(), source.as_bytes());
    }
}

// spec QC-27.15: the FTEQCC statement forms that nest a statement (`do`, `for`, `switch`
// bodies and a labelled statement) are bounded like `if` and `while`
#[test]
fn reviewer_fteqcc_statement_nesting_is_bounded() {
    let n = 20_000;
    for body in [
        format!("{}a = 1;{}", "do ".repeat(n), " while (a);".repeat(n)),
        format!("{}a = 1;", "for (;;) ".repeat(n)),
        format!("{}a = 1;", "if (a) ".repeat(n)),
    ] {
        let parsed = fte_body(&body);
        assert!(
            parsed.errors().any(|d| d.code == Code::E0509),
            "{}",
            &body[..20]
        );
        assert!(
            tree_depth(&parsed.syntax()) <= QC_MAX_NESTING.saturating_mul(4),
            "{}",
            &body[..20]
        );
    }
}

// spec QCD-1.4: a parse under the FTEQCC profile never fails: arbitrary FTEQCC punctuation
// soup gives back exactly its bytes, whatever errors it reports
#[test]
fn reviewer_fteqcc_soup_round_trips() {
    let soup = "class :: -> ?: ... #0: [++ .. --] $frame { ( ) } ; , *& ~ ^ << >>= %= &~= (+) ";
    let source = soup.repeat(200);
    let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
    assert_eq!(parsed.syntax().text(), source.as_bytes());
    assert!(parsed.errors().next().is_some());
    for d in parsed.diagnostics() {
        assert!(d.span.start <= d.span.end && d.span.end <= source.len());
    }
}

// spec QCD-1.4: the FTEQCC profile parses the same bytes to the same tree and diagnostics
// every time, including after an error
#[test]
fn reviewer_fteqcc_parse_is_deterministic() {
    let source = b"float f(float a = 1, ...) { for (;;) { a ?: ; } }; class c : d { };";
    assert_eq!(
        parse_with(source, Dialect::Fteqcc),
        parse_with(source, Dialect::Fteqcc)
    );
}

/// Checks that `source`, parsed under the FTEQCC profile, reports "nesting too deep", keeps its
/// tree within 4 x `QC_MAX_NESTING` levels and gives back every byte.
fn assert_fte_bounded(name: &str, source: &str) {
    let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
    assert!(
        parsed.errors().any(|d| d.code == Code::E0509),
        "{name}: no E0509"
    );
    assert!(
        tree_depth(&parsed.syntax()) <= QC_MAX_NESTING.saturating_mul(4),
        "{name}: tree too deep"
    );
    assert_eq!(parsed.syntax().text(), source.as_bytes(), "{name}");
}

// spec QC-27.15: the FTEQCC aggregates nest as declarations do: a struct or union inside a
// struct or union, 100000 deep, closed or left open, stops at the limit with every byte kept
#[test]
fn reviewer_fteqcc_aggregate_nesting_is_bounded() {
    let n = 100_000;
    for (name, source) in [
        (
            "struct",
            format!("{}float x;{};\n", "struct { ".repeat(n), "} s;".repeat(n)),
        ),
        (
            "union",
            format!("{}float x;{};\n", "union { ".repeat(n), "} s;".repeat(n)),
        ),
        ("unclosed struct", "struct { ".repeat(n)),
    ] {
        assert_fte_bounded(name, &source);
    }
}

// spec QC-27.15: an FTEQCC initialiser list `{ ... }` inside another, 100000 deep, is a level
// each
#[test]
fn reviewer_fteqcc_initialiser_lists_are_bounded() {
    let n = 100_000;
    assert_fte_bounded(
        "global list",
        &format!("float x[1] = {}1{};\n", "{".repeat(n), "}".repeat(n)),
    );
}

// spec QC-27.15: under the FTEQCC profile the operators that nest to the right or after their
// operand are bounded too: `?:` nested in its middle, `=` and `+=` chains, `~`, a prefix `*`
// and a postfix `++` chain, each 100000 deep
#[test]
fn reviewer_fteqcc_right_nesting_chains_are_bounded() {
    let n = 100_000;
    for (name, body) in [
        (
            "?: middle",
            format!("a = {}1{};", "a ? ".repeat(n), " : 1".repeat(n)),
        ),
        ("= chain", format!("{}1;", "a = ".repeat(n))),
        ("+= chain", format!("{}1;", "a += ".repeat(n))),
        ("~", format!("a = {}a;", "~".repeat(n))),
        ("prefix *", format!("a = {}a;", "*".repeat(n))),
        ("postfix ++", format!("a{};", "++".repeat(n))),
    ] {
        let source = Program {
            body: &body,
            ..Program::default()
        }
        .source();
        assert_fte_bounded(name, &source);
    }
}

// spec FTEF-18.26: the expressions FTEQCC reads outside a function body, an enum value, a
// parameter's default and a global's initialiser, count prefix operators and parentheses
// towards the limit like a body's
#[test]
fn reviewer_fteqcc_file_scope_expressions_are_bounded() {
    let n = 100_000;
    for (name, source) in [
        (
            "enum value",
            format!("enum {{ x = {}1{} }};\n", "(".repeat(n), ")".repeat(n)),
        ),
        (
            "parameter default",
            format!("void(float x = {}1{}) g;\n", "(".repeat(n), ")".repeat(n)),
        ),
        ("global -", format!("float x = {}1;\n", "-".repeat(n))),
    ] {
        assert_fte_bounded(name, &source);
    }
}

/// The engine name and FTEQCC `binum` of each builtin initialiser of `source`, parsed under the
/// FTEQCC profile, after checking that it parses with no error.
fn fte_builtins(source: &str) -> Vec<(Option<String>, Option<i32>)> {
    let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
    assert_eq!(errors(&parsed), [], "{source}");
    parsed
        .syntax()
        .descendants()
        .filter_map(Builtin::cast)
        .map(|builtin| {
            let binum = builtin
                .number()
                .and_then(|t| crate::builtin_binum(&t.text_lossless(), Dialect::Fteqcc));
            (builtin.engine_name(qc_lexer::LexFlags::default()), binum)
        })
        .collect()
}

// spec FTE-17.5: the `:` builtin form follows any function declarator, with or without `=`:
// a C-style declarator, `var`, a list, a local, and a space after the `:`. The pinned FTEQCC
// compiles each with 0 warnings, binding the names given
#[test]
fn reviewer_fteqcc_colon_builtins_follow_any_function_declarator() {
    let name = |n: &str| (Some(n.to_owned()), None);
    for (source, expected) in [
        ("void bx() :ex_c;\n", vec![name("ex_c")]),
        ("void bx() = :ex_d;\n", vec![name("ex_d")]),
        (
            "void(float a) bx :ex_e, by :ex_f;\n",
            vec![name("ex_e"), name("ex_f")],
        ),
        ("var void() bx :ex_g;\n", vec![name("ex_g")]),
        ("void() bx : ex_h;\n", vec![name("ex_h")]),
        (
            "void() f = { local void() lf = :nm4; };\n",
            vec![name("nm4")],
        ),
    ] {
        assert_eq!(fte_builtins(source), expected, "{source}");
    }
}

// spec FTE-17.5: a `:` after a field of function type or after a function parameter is no
// builtin initialiser; the pinned FTEQCC rejects both
#[test]
fn reviewer_fteqcc_colon_is_no_builtin_on_a_field_or_parameter() {
    for source in [".void() fth :nm6;\n", "void(void() cb :nm7) bx = #1;\n"] {
        let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
        assert_ne!(errors(&parsed), [], "{source}");
    }
}

// spec FTE-17.6: `:` takes the number forms `#` takes: a character literal is its code, a
// float above 2^24 is `(int)` of its `f32`, and an `i` literal in hex its `int`. The pinned
// FTEQCC binds `:'a'` as 97, `:'\n'` as 10, `:123456789` as 123456792, `#0x10i` as 16 and
// `bx :5` as 5
#[test]
fn reviewer_fteqcc_colon_numbers_bind_as_hash_numbers() {
    for (source, binum) in [
        ("void() bx = :'a';\n", 97),
        ("void() bx = :'\\n';\n", 10),
        ("void() bx = :123456789;\n", 123_456_792),
        ("void() bx = #0x10i;\n", 16),
        ("void() bx :5;\n", 5),
    ] {
        assert_eq!(fte_builtins(source), [(None, Some(binum))], "{source}");
    }
}
