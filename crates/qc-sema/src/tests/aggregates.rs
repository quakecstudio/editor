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

// spec FTEF-6.17: a struct member's array size is an integer constant and no 0
// spec QCD-3.61: E0356 "cannot cope with 0-sized arrays"
#[test]
fn a_member_array_size_is_a_nonzero_integer() {
    let zero = [d(Code::E0356, "cannot cope with 0-sized arrays")];
    for size in ["[0]", "[0x0]", "[2][0]"] {
        assert_eq!(
            messages(&format!("struct S {{ float a{size}; }};")),
            zero,
            "{size}"
        );
    }
    let not_integer = d(Code::E0262, "Value is not an integer constant");
    assert_eq!(
        messages("struct S { float a[1.5]; };"),
        std::slice::from_ref(&not_integer)
    );
    let unknown = [d(Code::E0301, "Unknown value \"A\"."), not_integer];
    assert_eq!(messages("struct S { float a[A]; };"), unknown);
    assert_eq!(messages("struct S { int a : A; };"), unknown);
    for source in [
        "float A = 2; struct S { float a[A]; };",
        "struct S { float a[2.0]; };",
        "struct S { float a[-1]; };",
    ] {
        assert_eq!(messages(source), [], "{source}");
    }
}

// spec FTEF-6.26: an error about a braced element stands at its `{`
#[test]
fn a_braced_elements_error_stands_at_its_brace() {
    let lines = |source: &str| -> Vec<usize> {
        let mut db = Db::new();
        db.set_dialect(Dialect::Fteqcc);
        db.set_file(FileId(0), PathBuf::from("t.qc"), source.as_bytes().to_vec());
        db.analyze_module(&ModuleInput {
            compiled: vec![FileId(0)],
            command_line: CommandLine::new::<&str>(&[]),
            ..ModuleInput::default()
        })
        .diagnostics()
        .map(|d| {
            source
                .get(..d.diagnostic.span.start)
                .map_or(0, |before| before.matches('\n').count() + 1)
        })
        .collect()
    };
    assert_eq!(
        lines(
            "struct P { float a; }; struct C : P { vector v; float w; }; C c = { {\n1 }, 2 \
             };\n"
        ),
        [1]
    );
    assert_eq!(
        lines("union U { float a; float b; }; U u = { 1,\n{\n2 } };\n"),
        [2]
    );
}

// spec FTEF-6.26: an element that should be braced is named by its first token, a comment before
// it aside
#[test]
fn an_unbraced_element_is_named_by_its_first_token() {
    let expected = |found: &str| vec![d(Code::E0204, &format!("expected {{, found {found}"))];
    assert_eq!(
        messages("#pragma target fte\nfloat x[2][2] = { /* c */ 1, 2 };"),
        expected("1")
    );
    assert_eq!(
        messages("#pragma target fte\nfloat x[2][2] = { {1,2}, /* c */ 3 };"),
        expected("3")
    );
}

// spec FTEF-5.3: an array with no size has as many elements as its braces hold, braced ones
// included
#[test]
fn an_unsized_array_counts_its_braced_elements() {
    assert_eq!(
        messages(
            "#pragma target fte\nstruct S { float a; }; S sa[] = { {1}, {2} }; float x; void() g = \
             { x = sa[2].a; };"
        ),
        [d(
            Code::W0249,
            "(constant) array index out of bounds (0 <= 2 < 2)"
        )]
    );
}

// spec FTEF-19.53: braces for a scalar are "\"{\" - not a name"
#[test]
fn braces_for_a_scalar_are_no_name() {
    let refused = vec![d(Code::E0201, "\"{\" - not a name")];
    for source in [
        "float x = { 1 };",
        "string sx = { \"a\" };",
        "entity ex = { 1 };",
        "float fy = { };",
        "void() g = { local float x = { 1 }; };",
        ".float fl = { 1 };",
        "int ix = { 1 };",
        "float a[2] = { {1}, 2 };",
    ] {
        assert_eq!(
            messages(&format!("#pragma target fte\n{source}")),
            refused,
            "{source}"
        );
    }
    assert_eq!(messages("#pragma target fte\nvector v = { 1, 2, 3 };"), []);
}

// spec FTEF-19.56: a pointer's braces are a nameless static array whose address it takes
#[test]
fn braced_pointer_initialisers() {
    let messages = |pragma: &str, source: &str| -> Vec<String> {
        messages(&format!("{pragma}\nfloat x;\n{source}"))
            .into_iter()
            .map(|(_, message)| message)
            .collect()
    };
    let fte = "#pragma target fte";
    assert_eq!(
        messages(fte, "float *p = { x };"),
        [
            "initializer for '' is not constant",
            "Unable to generate statements at global scope (GLOBALADDRESS).",
            "initializer for 'p' is not constant"
        ]
    );
    assert_eq!(
        messages(fte, "void() f = { static float *p = { 1 }; };"),
        ["initializer for 'p' is not constant"]
    );
    assert_eq!(
        messages(
            fte,
            "void() f = { local float *p = { x, 1 }; local float *q = { \"a\" }; };"
        ),
        [
            "initializer for '' is not constant",
            "Cannot cast from string to float"
        ]
    );
    assert!(
        messages(
            fte,
            "void() f = { local float *p = { 1 }; const float *q = { 1 }; };"
        )
        .is_empty()
    );
    assert_eq!(
        messages("", "float *p = { 1 };"),
        [
            "Address-of operator is not supported in this form without extensions. Consider the \
             use of: #pragma target fte"
        ]
    );
}

// spec FTEF-19.64: designated braces name a member in every element
// spec QCD-4.73: E0372 for an element that names none
#[test]
fn designated_initialisers_name_every_member() {
    let mixed = d(
        Code::E0372,
        "designated initialisers require all members to be initialised the same way",
    );
    let defs = "#pragma target fte\nstruct S { float a; vector b; };\n";
    assert_eq!(
        messages(&format!("{defs}S st = {{ .a = 1, 2 }};")),
        std::slice::from_ref(&mixed)
    );
    assert_eq!(
        messages(&format!("{defs}S st = {{ .a = 1, .b = '1 2 3', }};")),
        [mixed]
    );
    assert!(messages(&format!("{defs}S st = {{ .a = 1, .b = {{1, 2, 3}} }};")).is_empty());
}

// spec FTEF-19.65: a trailing comma after every member's value leaves the struct's `}` to the
// definition, which is missing its semicolon
#[test]
fn a_trailing_comma_after_every_member() {
    let defs = "struct S { float a; float b; };\n";
    let left = [missing_semicolon(), d(Code::E0203, "\"}\" is not a type")];
    assert_eq!(messages(&format!("{defs}S st = {{ 1, 2, }};")), left);
    assert_eq!(
        messages(&format!(
            "{defs}void() f = {{ local S st = {{ 1, 2, }}; }};"
        )),
        left
    );
    assert!(messages(&format!("{defs}S st = {{ 1, }};")).is_empty());
    assert!(messages(&format!("{defs}S st = {{ 1, 2 }};")).is_empty());
    assert!(messages("union U { float a; float b; };\nU u = { 1, };").is_empty());
}

// spec FTEF-6.30: a member's size or width that names a variable is no integer constant, and an
// unknown name read there is a global `variant`
#[test]
fn a_member_size_names_a_constant() {
    let not_integer = d(Code::E0262, "Value is not an integer constant");
    for source in [
        "float y; struct S { int a : y; };",
        "float y; struct S { float a[y]; };",
    ] {
        assert_eq!(
            messages(source),
            std::slice::from_ref(&not_integer),
            "{source}"
        );
    }
    assert_eq!(
        messages(
            "const int k = 3; enum { E = 2 }; struct S { int a : k; float b[k]; int c : E; };"
        ),
        []
    );
    for source in [
        "struct S { float a[A]; }; float u = A;",
        "struct S { int a : A; }; float u = A;",
    ] {
        let mut found = messages(source);
        found.sort();
        let mut expected = vec![
            d(Code::E0301, "Unknown value \"A\"."),
            not_integer.clone(),
            d(Code::E0214, "initializer for 'u' is not constant"),
        ];
        expected.sort();
        assert_eq!(found, expected, "{source}");
    }
}
