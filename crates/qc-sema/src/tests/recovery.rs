// SPDX-License-Identifier: GPL-3.0-or-later

//! Lowering and resolution of code the parser read only in part (QCD-1.4): the parser keeps
//! going after an error, and every name it did read is still resolved, so the editor reports
//! it while the code is being typed.

use super::fte_index_of;

/// The names an FTEQCC module of one file reports as "Unknown value", in order.
fn unknown(source: &str) -> Vec<String> {
    fte_index_of(source)
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.message.clone())
        .collect()
}

// spec QCD-1.4: a statement missing its condition, its body or a clause is still lowered, and
// the names it has are resolved.
#[test]
fn partial_statements_resolve_what_they_have() {
    for (source, names) in [
        ("void() f = { if () u1 = 1; };\n", &["u1"][..]),
        ("void() f = { if (u1) };\n", &["u1"]),
        ("void() f = { while () u1 = 1; };\n", &["u1"]),
        ("void() f = { while (u1) };\n", &["u1"]),
        ("void() f = { do while (u1); };\n", &["u1"]),
        ("void() f = { do u1 = 1; };\n", &["u1"]),
        ("void() f = { for (u1;;) };\n", &["u1"]),
        ("void() f = { switch () { case u1: } };\n", &["u1"]),
        ("void() f = { switch (u1) };\n", &["u1"]),
        ("void() f = { switch (u1) { u2 = 1; } };\n", &["u1", "u2"]),
        ("void() f = { local float ; u1 = 1; };\n", &["u1"]),
        ("float ;\nvoid() f = { u1 = 1; };\n", &["u1"]),
    ] {
        let expected: Vec<_> = names
            .iter()
            .map(|name| format!("Unknown value \"{name}\""))
            .collect();
        assert_eq!(unknown(source), expected, "{source:?}");
    }
}

// spec QCD-1.4: an expression missing an operand, an argument list, a selector or an index is
// still lowered, and the names it has are resolved.
#[test]
fn partial_expressions_resolve_what_they_have() {
    for (source, names) in [
        ("void() f = { u1 = !; };\n", &["u1"][..]),
        ("void() f = { u1 = (); };\n", &["u1"]),
        ("void() f = { u1 = u2 +; };\n", &["u1", "u2"]),
        ("void() f = { u1(; };\n", &["u1"]),
        ("void() f = { u1.; };\n", &["u1"]),
        ("void() f = { (u1 ? : u2); };\n", &["u1", "u2"]),
        ("void() f = { u1 = -; };\n", &["u1"]),
        ("void() f = { u1 = u2[]; };\n", &["u1", "u2"]),
        ("void() f = { u1 = (float); };\n", &["u1"]),
        ("void() f = { u1++; ++; };\n", &["u1"]),
        ("void() f = { u1 = []; };\n", &["u1"]),
        ("void() f = { .x; };\n", &[]),
    ] {
        let expected: Vec<_> = names
            .iter()
            .map(|name| format!("Unknown value \"{name}\""))
            .collect();
        assert_eq!(unknown(source), expected, "{source:?}");
    }
}

// spec QCD-1.4: a class, typedef, accessor, member, enumerator or function with no name or no
// body declares nothing and stops nothing after it.
#[test]
fn nameless_declarations_declare_nothing() {
    for source in [
        "class { float x; };\n",
        "typedef float;\n",
        "accessor : float;\n",
        "class A { float; };\n",
        "enum { = 1 };\n",
        "void(float a) f = ;\n",
        "void() f = [1, ] { };\n",
    ] {
        let after = format!("{source}void() g = {{ u1 = 1; }};\n");
        assert_eq!(unknown(&after), ["Unknown value \"u1\""], "{source:?}");
    }
}
