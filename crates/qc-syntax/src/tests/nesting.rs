// SPDX-License-Identifier: GPL-3.0-or-later

//! The nesting limit (QC-27.15, QC-27.16) under the FTEQCC profile (FTEF-18.26), and the tree
//! after the limit under both profiles.

use super::Program;
use crate::{Code, Dialect, QC_MAX_NESTING, SyntaxNode, parse_with};

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

/// A program with `body` as the statements of `f`.
fn in_body(body: &str) -> String {
    Program {
        body,
        ..Program::default()
    }
    .source()
}

/// A nesting form of FTEF-18.26: a program with `n` repeats of its nesting part.
struct Form {
    name: &'static str,
    source: fn(usize) -> String,
    /// The most repeats with no error; one more is "nesting too deep".
    limit: usize,
}

/// Every FTEQCC form FTEF-18.26 counts, with its limit: the statement, its expression and an
/// assignment's right side take the levels the vanilla parentheses do, and a declaration's type
/// one (QC-27.15).
fn forms() -> Vec<Form> {
    vec![
        Form {
            name: "prefix -",
            source: |n| in_body(&format!("a = {}a;", "- ".repeat(n))),
            limit: QC_MAX_NESTING - 4,
        },
        Form {
            name: "prefix +",
            source: |n| in_body(&format!("a = {}a;", "+ ".repeat(n))),
            limit: QC_MAX_NESTING - 4,
        },
        Form {
            name: "prefix ~",
            source: |n| in_body(&format!("a = {}a;", "~ ".repeat(n))),
            limit: QC_MAX_NESTING - 4,
        },
        Form {
            name: "prefix &",
            source: |n| in_body(&format!("a = {}a;", "& ".repeat(n))),
            limit: QC_MAX_NESTING - 4,
        },
        Form {
            name: "prefix *",
            source: |n| in_body(&format!("a = {}a;", "* ".repeat(n))),
            limit: QC_MAX_NESTING - 4,
        },
        Form {
            name: "cast",
            source: |n| in_body(&format!("a = {}a;", "(float)".repeat(n))),
            limit: QC_MAX_NESTING - 4,
        },
        Form {
            name: "prefix ++",
            source: |n| in_body(&format!("{}a;", "++".repeat(n))),
            limit: QC_MAX_NESTING - 2,
        },
        Form {
            name: "prefix --",
            source: |n| in_body(&format!("{}a;", "--".repeat(n))),
            limit: QC_MAX_NESTING - 2,
        },
        Form {
            name: "parameter lists",
            source: |n| format!("float{} f;\n", "()".repeat(n)),
            limit: QC_MAX_NESTING - 1,
        },
        Form {
            name: "pointers",
            source: |n| format!("float{} p;\n", "*".repeat(n)),
            limit: QC_MAX_NESTING - 1,
        },
        Form {
            name: "declarator pointers",
            source: |n| format!("float a, {}b;\n", "*".repeat(n)),
            limit: QC_MAX_NESTING,
        },
        Form {
            name: "state frame",
            source: |n| format!("void() f = [{}1, f] {{}};\n", "- ".repeat(n)),
            limit: QC_MAX_NESTING - 1,
        },
        Form {
            name: "parameter default",
            source: |n| format!("void(float a = {}1) f;\n", "- ".repeat(n)),
            limit: QC_MAX_NESTING - 3,
        },
    ]
}

/// The error codes of `source` under the FTEQCC profile.
fn fte_errors(source: &str) -> Vec<Code> {
    parse_with(source.as_bytes(), Dialect::Fteqcc)
        .errors()
        .map(|d| d.code)
        .collect()
}

// spec FTEF-18.26: each prefix operator, cast, `*` and parameter list of a type is one level,
// so each form parses at its limit and is "nesting too deep" one past it
// spec QC-27.15: the error is E0509 at the token where the limit is passed
#[test]
fn fteqcc_nesting_limit() {
    for form in forms() {
        assert_eq!(fte_errors(&(form.source)(form.limit)), [], "{}", form.name);
        assert_eq!(
            fte_errors(&(form.source)(form.limit + 1)),
            [Code::E0509],
            "{}",
            form.name
        );
    }
}

// spec FTEF-18.26: no input of these forms exhausts the stack: 100000 repeats of each, and of
// the unspaced `---...` and `+++...`, which lex as prefix `--` and `++`, keep every byte,
// report E0509 and stay within the limit's depth
// spec QC-27.16: once the limit is passed nothing nests deeper
#[test]
fn deep_fteqcc_input_does_not_overflow() {
    let mut sources: Vec<(String, String)> = forms()
        .into_iter()
        .map(|form| (form.name.to_owned(), (form.source)(100_000)))
        .collect();
    sources.push((
        "unspaced -".to_owned(),
        in_body(&format!("a = {}a;", "-".repeat(100_000))),
    ));
    sources.push((
        "unspaced +".to_owned(),
        in_body(&format!("a = {}a;", "+".repeat(100_000))),
    ));
    sources.push((
        "unspaced state frame".to_owned(),
        format!("void() f = [{}1, f] {{}};\n", "-".repeat(100_000)),
    ));
    sources.push((
        "unspaced parameter default".to_owned(),
        format!("void(float a = {}1) f;\n", "-".repeat(100_000)),
    ));
    for (name, source) in sources {
        let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
        assert_eq!(parsed.syntax().text(), source.as_bytes(), "{name}");
        assert!(
            parsed.errors().any(|d| d.code == Code::E0509),
            "{name}: {:?}",
            parsed.errors().map(|d| d.code).take(3).collect::<Vec<_>>()
        );
        assert!(
            tree_depth(&parsed.syntax()) <= QC_MAX_NESTING + 16,
            "{name}: depth {}",
            tree_depth(&parsed.syntax())
        );
    }
}

// spec QC-27.16: past the limit the constructs a statement unwinds through take no further
// operator, field or call, the rest of the statement is skipped, and the next statement nests
// afresh, under both profiles
#[test]
fn the_next_statement_nests_afresh() {
    let deep = format!("a = {}a{};", "(".repeat(300), " + a).en".repeat(300));
    let fits = format!(
        "b = {}a{};",
        "(".repeat(QC_MAX_NESTING - 4),
        ")".repeat(QC_MAX_NESTING - 4)
    );
    let source = in_body(&format!("{deep}\n{fits}"));
    for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
        let parsed = parse_with(source.as_bytes(), dialect);
        let errors: Vec<Code> = parsed.errors().map(|d| d.code).collect();
        assert_eq!(errors, [Code::E0509], "{dialect:?}");
        assert!(
            tree_depth(&parsed.syntax()) <= QC_MAX_NESTING + 16,
            "{dialect:?}: depth {}",
            tree_depth(&parsed.syntax())
        );
        assert_eq!(parsed.syntax().text(), source.as_bytes());
    }
}
