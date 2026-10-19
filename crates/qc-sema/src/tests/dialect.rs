// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolution of the FTEQCC dialect binding forms
//! (`docs/spec/dialect-fteqcc-rerelease.md`): `const`/`var`/`nosave` globals, `enum` names,
//! `typedef` type names, `for`-loop locals and the new expression forms resolve with no
//! error under the FTEQCC profile (P2-03).

use super::fte_index_of;

/// The diagnostics of resolving `source` under the FTEQCC dialect.
fn fte_errors(source: &str) -> Vec<String> {
    fte_index_of(source)
        .diagnostics
        .iter()
        .map(|d| format!("{} {}", d.diagnostic.code.as_str(), d.diagnostic.message))
        .collect()
}

// spec FTE-10.1: a `const` global is a definition a later use resolves to.
// spec FTE-11.1: a `var` global is a definition.
// spec FTE-12.1: a `nosave` global is a definition.
#[test]
fn qualified_globals_resolve() {
    let source = "const float A = 1;\n\
                  var float num = 0;\n\
                  nosave float flag;\n\
                  void() f = { num = A; flag = num; };\n";
    assert_eq!(fte_errors(source), Vec::<String>::new());
}

// spec FTE-10.2: a `const` may initialise another `const`, which resolves to the first.
#[test]
fn const_initialises_const() {
    let source = "const float A = 2;\nconst float B = A;\nvoid() f = { A;\nB; };\n";
    assert_eq!(fte_errors(source), Vec::<String>::new());
}

// spec FTE-15.1: `enum` names become float constants that a later use resolves to.
// spec FTE-15.2: an explicit value does not change that the names are constants.
#[test]
fn enum_names_resolve() {
    let source = "enum { IT_AXE, IT_SHOTGUN, IT_KEY1 = 10, IT_KEY2 };\n\
                  float have;\n\
                  void() f = { have = IT_AXE; have = IT_KEY2; };\n";
    assert_eq!(fte_errors(source), Vec::<String>::new());
}

// spec FTE-2.2: a `for`-loop local declared in the init clause resolves in the loop.
#[test]
fn for_loop_local_resolves() {
    let source = "float n, sum;\n\
                  void() f = { for (float i = 0; i < n; i++) sum = sum + i; };\n";
    assert_eq!(fte_errors(source), Vec::<String>::new());
}

// spec FTE-9.1: a vector member `.z` is not a global field, so it is not an unknown value.
// spec FTE-13.3: `name.length` is not a global field either.
#[test]
fn member_access_is_not_an_unknown_value() {
    let source = "vector v;\nfloat arr[3];\nfloat n;\n\
                  void() f = { v.z = 1; n = arr.length; };\n";
    assert_eq!(fte_errors(source), Vec::<String>::new());
}

// spec FTE-13.2: an array subscript resolves its base and index names.
#[test]
fn array_subscript_resolves() {
    let source = "float arr[3];\nfloat i, out;\n\
                  void() f = { for (i = 0; i < 3; i++) out = arr[i]; };\n";
    assert_eq!(fte_errors(source), Vec::<String>::new());
}

// spec FTE-14.1: a `typedef` type name is a definition, so using the type resolves.
// spec FTE-14.2: a struct field access through an array element is a member, not a global.
#[test]
fn typedef_struct_resolves() {
    let source = "typedef struct { string mdl; float frame; } poseInfo;\n\
                  poseInfo poses[2];\n\
                  string s;\nfloat i;\n\
                  void() f = { s = poses[i].mdl; };\n";
    assert_eq!(fte_errors(source), Vec::<String>::new());
}

// spec FTE-8.1: the condition and both arms of a ternary resolve.
#[test]
fn ternary_operands_resolve() {
    let source = "float a, b, c, out;\n\
                  void() f = { out = a ? b : c; };\n";
    assert_eq!(fte_errors(source), Vec::<String>::new());
}

// spec FTE-3.1: a `switch` scrutinee, `case` values and body statements resolve.
#[test]
fn switch_resolves() {
    let source = "const float ZERO = 0;\nfloat x, out;\n\
                  void() f = { switch (x) { case ZERO: out = 1; break; default: out = 2; } };\n";
    assert_eq!(fte_errors(source), Vec::<String>::new());
}

// spec FTE-6.1: a compound assignment resolves its lvalue and right side.
#[test]
fn compound_assignment_resolves() {
    let source = "float a, b;\nvoid() f = { a += b; a *= 2; };\n";
    assert_eq!(fte_errors(source), Vec::<String>::new());
}
