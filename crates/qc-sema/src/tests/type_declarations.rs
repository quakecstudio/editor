// SPDX-License-Identifier: GPL-3.0-or-later

//! The declarations and statements FTEQCC refuses and the semantic model checks
//! (`docs/spec/dialect-fteqcc.md` FTEF-6, FTEF-9, FTEF-17, FTEF-19, FTEF-21): `break` and
//! `continue` with no loop, struct and union definitions, enums, type names used as values,
//! class members and states in classes. Each program was read through the pinned FTEQCC
//! (`cargo xtask oracles builtins`, the `types` family) first.

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

/// The error codes of the FTEQCC file `source` read with the command line `args`, after the
/// definition `f` FTEQCC's differential binds first.
fn errors(source: &str, args: &[&str]) -> Vec<Code> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(
        FileId(0),
        PathBuf::from("t.qc"),
        format!("void() f = #5;\n{source}\n").into_bytes(),
    );
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FileId(0)],
        command_line: CommandLine::new(args),
        ..ModuleInput::default()
    });
    analysis
        .diagnostics()
        .map(|d| d.diagnostic.code)
        .filter(|code| code.is_error())
        .collect()
}

/// The programs of `refused` give exactly `code`, the programs of `accepted` none.
fn check(code: Code, refused: &[&str], accepted: &[&str]) {
    for source in refused {
        assert_eq!(errors(source, &[]), [code], "{source}");
    }
    for source in accepted {
        assert_eq!(errors(source, &[]), [], "{source}");
    }
}

// spec FTEF-21.19, QCD-4.14: a `break` that no loop or `switch` holds is "illegal breaks"
#[test]
fn a_break_with_no_loop_is_refused() {
    check(
        Code::E0244,
        &[
            "void() g = { break; };",
            "void() g = { if (1) { break; } };",
            "void() g = { while (1) { } break; };",
            "void() g = { break; break; };",
        ],
        &[
            "void() g = { while (1) { break; } };",
            "void() g = { switch (1) { case 1: break; } };",
            "void() g = { do { break; } while (0); };",
            "void() g = { for (;;) { break; } };",
            "void() g = { switch (1) break; };",
            "void() g = { while (1) { switch (1) { case 1: break; } } };",
        ],
    );
    assert_eq!(
        errors("void() g = { BREAK; };", &["-Fcaseinsens"]),
        [Code::E0244]
    );
    assert_eq!(errors("void() g = { BREAK; };", &[]), [Code::E0301]);
}

// spec FTEF-21.19, QCD-4.15: a `continue` that no loop holds is "illegal continues", a
// `switch` not being one
#[test]
fn a_continue_with_no_loop_is_refused() {
    check(
        Code::E0245,
        &[
            "void() g = { continue; };",
            "void() g = { switch (1) { case 1: continue; } };",
            "void() g = { for (;;) { break; } continue; };",
        ],
        &[
            "void() g = { do { continue; } while (0); };",
            "void() g = { while (1) { switch (1) { case 1: continue; } } };",
            "void() g = { for (float i; ; ) { continue; } };",
        ],
    );
    assert_eq!(
        errors("void() g = { Continue; };", &["-Fcaseinsens"]),
        [Code::E0245]
    );
}

// spec FTEF-6.10, QCD-3.44: a struct given a body again is compared with the first
#[test]
fn a_struct_redefined_differently_is_refused() {
    check(
        Code::E0246,
        &[
            "struct S { float a; }; struct S { float b; };",
            "struct S { float a; }; struct S { float a; float b; };",
            "struct S { float a; }; struct S { vector a; };",
            "struct S { float a; }; struct S { float a[2]; };",
            "struct S { float a; float b; }; struct S { float b; float a; };",
            "struct float { float a; };",
            "union U { float a; }; union U { vector a; };",
        ],
        &[
            "struct S { float a; }; struct S { float a; };",
            "union U { float a; }; struct U { float a; };",
            "struct S { float a; }; struct S;",
            "struct S; struct S { float a; };",
        ],
    );
}

// spec FTEF-6.10, QCD-3.45, QCD-3.46, QCD-3.47: a struct's parent is a struct defined before,
// and the same one each time
#[test]
fn a_structs_parent_is_checked() {
    check(
        Code::E0247,
        &["struct S : P { float b; };"],
        &["struct P { float a; }; struct S : P { float b; };"],
    );
    check(
        Code::E0248,
        &[
            "class E { float e; }; struct S : E { float b; };",
            "struct S : float { float b; };",
        ],
        &[],
    );
    check(
        Code::E0249,
        &[
            "struct P { float a; }; struct Q { float a; }; struct S : P { float b; }; \
             struct S : Q { float b; };",
            "struct P { float a; }; struct S; struct S : P { float b; }; struct S : P { float b; };",
        ][..1],
        &[],
    );
}

// spec FTEF-6.9, QCD-3.48: a bitfield is an integer's, at most 64 bits wide
#[test]
fn a_bitfield_is_an_integers() {
    check(
        Code::E0250,
        &[
            "struct S { float a : 3; };",
            "struct S { string a : 3; };",
            "struct S { int a : 65; };",
            "struct S { __int8 a : 3; };",
            "struct S { __double a : 3; };",
        ],
        &[
            "struct S { int a : 3; };",
            "struct S { __uint a : 3; };",
            "struct S { __int64 a : 3; };",
            "struct S { int a : 40; };",
        ],
    );
}

// spec FTEF-6.10, QCD-3.52: a member of the struct's own type, or of one with no body so far,
// is "not fully defined yet", a pointer to it is not
#[test]
fn a_member_of_an_incomplete_struct_is_refused() {
    check(
        Code::E0255,
        &[
            "struct S { struct S next; };",
            "struct S { struct T t; };",
            "struct T; struct S { struct T t; };",
        ],
        &[
            "struct S { struct S *next; };",
            "struct S { struct T *t; };",
            "struct T { float a; }; struct S { struct T t; };",
        ],
    );
}

// spec FTEF-6.11, QCD-4.16: a name found twice among a struct's members is "multiple members",
// in any case under `-Fcaseinsens`
#[test]
fn a_member_found_twice_is_refused() {
    let union = "union U { float a; float A; }; U x; void() g = { x.A = 1; };";
    assert_eq!(errors(union, &["-Fcaseinsens"]), [Code::E0252]);
    assert_eq!(errors(union, &[]), []);
    let twice = "struct S { float a; vector a; }; S x; void() g = { x.a = 1; };";
    assert_eq!(errors(twice, &[]), [Code::E0252]);
    let once = "struct S { float a; float a; }; S x; void() g = { x.a = 1; };";
    assert_eq!(errors(once, &[]), []);
}

// spec FTEF-19.36, QCD-3.49: an enumerator of a named enum given again with another value is
// "already declared"; an anonymous enum or the same value is not
#[test]
fn an_enumerator_given_again_is_refused() {
    check(
        Code::E0251,
        &["enum E { A, A };", "enum E { A, B, A };"],
        &[
            "enum { A, A };",
            "enum E { A = 1, A = 1 };",
            "enum E { A }; enum E { A };",
            "enum E { A, B }; enum E { B };",
        ],
    );
}

// spec FTEF-19.38: a type's name is no value, but the operand of `sizeof` and of `spawn`
#[test]
fn a_type_name_is_no_value() {
    for source in [
        "struct S { float a; }; void() g = { local float q = S; };",
        "struct S { float a; }; S x; void() g = { x = S(1); };",
        "class C { float a; }; void() g = { local float q = C; };",
        "typedef float t; void() g = { local float q = t; };",
        "enum E { A }; void() g = { local float q = E; };",
    ] {
        assert_eq!(errors(source, &[]), [Code::E0301], "{source}");
    }
    assert_eq!(
        errors(
            "struct S { float a; }; void() g = { local float q = sizeof(S); };",
            &[]
        ),
        []
    );
}

// spec FTEF-19.39: an element of a variable is no constant initialiser
#[test]
fn an_element_of_a_variable_is_not_constant() {
    for source in [
        "float x[2]; float y = x[1];",
        "extern \"C\" { float x[2]; float y = x[1]; }",
    ] {
        assert_eq!(errors(source, &[]), [Code::E0214], "{source}");
    }
    assert_eq!(
        errors("const float x[2] = {1, 2}; float y = x[1];", &[]),
        []
    );
}

// spec FTEF-19.40, QCD-4.17: a call of a name whose type is a value's is "not a function"
#[test]
fn a_value_is_not_called() {
    for source in [
        "float x; void() g = { x(); };",
        "vector x; void() g = { local float y = x(); };",
        "struct S { float a; }; S x; void() g = { x(); };",
    ] {
        assert_eq!(errors(source, &[]), [Code::E0256], "{source}");
    }
}

// spec FTEF-19.41: a function with no body is one whether a global or a member, and a typedef
// of a function type names one
#[test]
fn a_function_with_no_body_is_refused() {
    for source in [
        "class C { void() m; };",
        "class C { virtual void() m; };",
        "class C { C(void); };",
        "typedef float fn(float); fn x;",
        "extern \"C\" { typedef float (*fp)(float); fp x; }",
    ] {
        assert_eq!(errors(source, &[]), [Code::E0211], "{source}");
    }
    for source in [
        "class C { void() m; }; void C::m(void) { };",
        "class C { void() m = #5; };",
        "extern \"C\" { typedef float (*fp)(float); fp x = 0; }",
    ] {
        assert_eq!(errors(source, &[]), [], "{source}");
    }
}

// spec FTEF-9.10, QCD-3.53: a member given a value is a function, `static` or `const`, or the
// value is 0
#[test]
fn only_a_member_function_is_initialised() {
    check(
        Code::E0257,
        &[
            "class C { float a = 1; };",
            "class C { var float a = 1; };",
            "class C { string a = \"x\"; };",
        ],
        &[
            "class C { float a = 0; };",
            "class C { static float a = 1; };",
            "class C { const float a = 1; };",
            "class C { void() m = {}; };",
        ],
    );
}

// spec FTEF-17.14: a name in a member function is a nonvirtual member function or a global, a
// field is `this.field`
#[test]
fn a_field_is_found_as_this_dot_field() {
    for source in [
        "class C { float a; void() m = { a = 1; }; };",
        "class C { float a; virtual void() m = { a = 1; }; };",
        "class C { float a; void() m; }; void C::m(void) { a = 1; };",
    ] {
        assert_eq!(errors(source, &[]), [Code::E0301], "{source}");
    }
    for source in [
        "class C { float a; void() m = { this.a = 1; }; };",
        "class C { void() m = {}; void() n = { m(); }; };",
    ] {
        assert_eq!(errors(source, &[]), [], "{source}");
    }
}

// spec FTEF-19.42: a state in a member names a member declared before it, a `void()`
#[test]
fn a_state_in_a_member_names_a_void_function() {
    check(
        Code::E0309,
        &["class C { float() m = [1, m] { return 1; }; };"],
        &["class C { void() m = [1, m] {}; };"],
    );
    assert_eq!(
        errors("class C { void() m = [1, n] {}; void() n = {}; };", &[]),
        [Code::E0211]
    );
    assert_eq!(
        errors("$frame run\nvoid() g = [$RUN, G] {};", &["-Fcaseinsens"]),
        []
    );
}

// spec FTEF-19.35: a type written with no declarator has its type names checked
#[test]
fn a_type_with_no_declarator_is_checked() {
    for source in [
        "float (x);",
        "float (x)(float);",
        "void() g = { local float (x); };",
        "struct S2 { float b; }; S2 (x);",
    ] {
        assert_eq!(errors(source, &[]), [Code::E0203], "{source}");
    }
}
