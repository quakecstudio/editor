// SPDX-License-Identifier: GPL-3.0-or-later

//! Adversarial tests from the P2-R2 readiness review (TEST-9): the FTEQCC profile at the
//! nesting limit, written by a reviewer who did not write the parser.

use super::{Program, errors};
use crate::QC_MAX_NESTING;
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
