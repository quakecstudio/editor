// SPDX-License-Identifier: GPL-3.0-or-later

//! Braced initialisers read against their type (`docs/spec/dialect-fteqcc.md` FTEF-19.47).
//! Each program was read through the pinned FTEQCC first.

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

/// Every diagnostic of the FTEQCC file `source`, its code and message, in order.
fn messages(source: &str) -> Vec<(Code, String)> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(
        FileId(0),
        PathBuf::from("t.qc"),
        format!("{source}\n").into_bytes(),
    );
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FileId(0)],
        command_line: CommandLine::new::<&str>(&[]),
        ..ModuleInput::default()
    });
    analysis
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect()
}

fn d(code: Code, message: &str) -> (Code, String) {
    (code, message.to_owned())
}

fn missing_semicolon() -> (Code, String) {
    d(Code::W0203, "Missing semicolon at end of definition")
}

// spec FTEF-19.47: an element past an array's size or a struct's members is left to what reads
// on: at a global's top level not a type, inside braces "expected }", at a local's a statement
#[test]
fn an_element_past_the_end_is_left_over() {
    let not_a_type = |token: &str| {
        vec![
            missing_semicolon(),
            d(Code::E0203, &format!("\"{token}\" is not a type")),
        ]
    };
    assert_eq!(messages("float x[2] = {1, 2, 3};"), not_a_type("3"));
    assert_eq!(messages("float x[2] = {1, 2, 3, 4};"), not_a_type("3"));
    assert_eq!(
        messages("struct S { float a; }; S x = {1, 2};"),
        not_a_type("2")
    );
    assert_eq!(
        messages("struct S { float a, b; } x = { 1, 2, 3 };"),
        not_a_type("3")
    );
    assert_eq!(
        messages("float x[2][2] = {{1, 2, 3}, {3, 4}};"),
        [d(Code::E0204, "expected }, found 3")]
    );
    assert_eq!(
        messages("struct S { float a; vector v; } x = { 1, {1, 2, 3, 4} };"),
        [d(Code::E0204, "expected }, found 4")]
    );
    assert_eq!(
        messages("struct S { float a; }; void() g = { local S x = {1, 2}; };"),
        [
            missing_semicolon(),
            d(Code::W0204, "Statement does not do anything"),
            d(Code::E0204, "expected ;, found }"),
        ]
    );
    for source in [
        "float x[2] = {1, 2,};",
        "struct S { float a, b; } x = { 1 };",
        "string x[] = {\"a\", \"b\"};",
        "struct S { float a; vector v; } x = { 1, {1, 2, 3} };",
        "struct S { float a[2]; } x = { {1, 2} };",
        "struct S { float a; }; struct T { S s; } x = { {1} };",
        "struct P { float a; }; struct S : P { float b; } x = { };",
    ] {
        assert_eq!(messages(source), [], "{source}");
    }
}

// spec FTEF-19.47: each value is read as a definition's value, cast to its element's type
#[test]
fn each_element_value_is_cast() {
    assert_eq!(
        messages("float x[2] = {\"a\", 1};"),
        [d(Code::E0213, "Cannot cast from string to float")]
    );
    assert_eq!(
        messages("void() g = { local float x[2] = {1, \"a\"}; };"),
        [d(Code::E0213, "Cannot cast from string to float")]
    );
    let lax = d(Code::W0220, "Implicit cast from float to vector");
    assert_eq!(
        messages("vector x[2] = {1, 2};"),
        [lax.clone(), lax.clone()]
    );
    assert_eq!(
        messages("struct S { float a; vector v; } x = { 1, 1, 2, 3 };"),
        [
            lax,
            missing_semicolon(),
            d(Code::E0203, "\"2\" is not a type")
        ]
    );
    assert_eq!(
        messages("struct S { string s; } x = { 1 };"),
        [d(Code::E0213, "Cannot cast from float to string")]
    );
    assert_eq!(
        messages("float y; float x[2] = {y, 1};"),
        [d(Code::E0214, "initializer for 'x' is not constant")]
    );
    assert_eq!(
        messages("float y = 2; void() g = { local float x[2] = {y, 1}; };"),
        []
    );
}

// spec FTEF-19.47: an array member takes its own braces, a struct member is cast from a value,
// a struct with a parent takes designated members only, and a union one element
// spec QCD-3.59: E0354 "inherited structs must use designated initialisers"
// spec QCD-3.60: E0355 "member U.M was already ininitialised"
#[test]
fn members_are_read_by_their_kind() {
    assert_eq!(
        messages("struct S { float a[2]; } x = { 1, 2 };"),
        [d(Code::E0204, "expected {, found 1")]
    );
    assert_eq!(
        messages("struct S { float a; }; struct T { S s; } x = { 1 };"),
        [d(Code::E0213, "Cannot cast from float to struct S")]
    );
    assert_eq!(
        messages("struct P { float a; }; struct S : P { float b; } x = { 1, 2 };"),
        [d(
            Code::E0354,
            "inherited structs must use designated initialisers"
        )]
    );
    assert_eq!(
        messages("union U { float a; vector v; } x = { 1, 2 };"),
        [d(Code::E0355, "member U.v was already ininitialised")]
    );
}

// spec FTEF-6.12: a struct is cast to and from nothing else, named `struct NAME` in the message
#[test]
fn a_struct_cast_names_the_struct() {
    assert_eq!(
        messages("struct S { float a; }; S x = 1;"),
        [d(Code::E0213, "Cannot cast from float to struct S")]
    );
    assert_eq!(
        messages("struct S { float a; }; S s; float x = s;"),
        [d(Code::E0213, "Cannot cast from struct S to float")]
    );
}

// spec FTEF-6.16: a struct's static member is the global `S::NAME`, and an array of structs has
// only its length
#[test]
fn a_struct_static_member_is_a_global() {
    assert_eq!(
        messages("struct S { static float a; float b; }; void() g = { S::a = 1; };"),
        []
    );
    assert_eq!(
        messages("struct S { static float a; float b; }; float x = S::a;"),
        [d(Code::E0214, "initializer for 'x' is not constant")]
    );
    assert_eq!(
        messages("struct S { float b; }; void() g = { S::b = 1; };"),
        [d(Code::E0301, "Unknown value \"S::b\"")]
    );
    assert_eq!(
        messages("struct S { float a; }; void() g = { local S x[2]; local float n = x.length; };"),
        []
    );
    assert_eq!(
        messages("struct S { float a; }; S x[2]; void() g = { local float n = x.a; };"),
        [d(Code::E0204, "expected length, found a")]
    );
}
