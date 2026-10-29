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
        "float x[2]; void() g = { static float y = x[1]; };",
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

// spec FTEF-17.14: a field named bare in a member function is `this.field` through the global
// `self`, without which FTEQCC ends; a member function is found either way
#[test]
fn a_field_is_found_through_self() {
    for source in [
        "class C { float a; void() m = { a = 1; }; };",
        "class C { float a; virtual void() m = { a = 1; }; };",
        "class C { float a; void() m; }; void C::m(void) { a = 1; };",
    ] {
        assert_eq!(errors(source, &[]), [Code::E0301], "{source}");
        assert_eq!(
            errors(&format!("entity self; {source}"), &[]),
            [],
            "{source}"
        );
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
// spec FTEF-21.22: `(x)` of a name that is no type is "expected function arg list"
#[test]
fn a_type_with_no_declarator_is_checked() {
    for source in [
        "float (x);",
        "float (x)(float);",
        "void() g = { local float (x); };",
        "struct S2 { float b; }; S2 (x);",
        "struct S { float a; } (x);",
    ] {
        assert_eq!(errors(source, &[]), [Code::E0204], "{source}");
    }
}

// spec FTEF-8.5: the parameter lists after a struct's or an enum's body make the declarators'
// type a function
#[test]
fn a_parameter_list_after_a_body_makes_a_function() {
    let warnings = |source: &str| -> Vec<Code> {
        let mut db = Db::new();
        db.set_dialect(Dialect::Fteqcc);
        db.set_file(FileId(0), PathBuf::from("t.qc"), source.as_bytes().to_vec());
        db.analyze_module(&ModuleInput {
            compiled: vec![FileId(0)],
            command_line: CommandLine::new::<&str>(&[]),
            ..ModuleInput::default()
        })
        .diagnostics()
        .map(|d| d.diagnostic.code)
        .collect()
    };
    assert_eq!(
        warnings("enum { A }(float) h = #1; void() g = { h(1, 2); };\n"),
        [Code::W0252]
    );
    assert_eq!(
        warnings("struct S { float a; } (float) h = #1; void() g = { h(1); };\n"),
        []
    );
}

// spec FTEF-6.12: a struct is cast to and from nothing but itself; an aggregate or a value of
// the struct initialises one
#[test]
fn a_struct_is_cast_to_and_from_nothing() {
    check(
        Code::E0213,
        &[
            "struct S { float a; }; S s = 1;",
            "struct S { float a; }; S s = 0;",
            "struct S { float a; }; S s; float x = s;",
            "struct S { float a; }; void() g = { (struct S)0; };",
        ],
        &[
            "struct S { float a; }; S s = { 1 };",
            "struct S { float a; }; S s = { 1 }; S t = s;",
            "struct S { float a; }; S s; void() g = { local S t = s; };",
        ],
    );
}

// spec FTEF-9.12: a class whose name is a struct's is "Parent class changed", another sized
// type's name is refused too; under `-Fcaseinsens` a class in another case is the class
#[test]
fn a_class_named_as_another_type_is_refused() {
    check(
        Code::E0242,
        &[
            "struct S { float a; }; class S { float b; };",
            "class float { float a; };",
            "typedef float T; class T { float b; };",
            "enum E { A }; class E { float b; };",
        ],
        &["class C { float a; }; class C { float a; };"],
    );
    let other_case = "class C { float a; }; class c { float a; }; c x; C y;";
    assert_eq!(errors(other_case, &["-Fcaseinsens"]), []);
    assert_eq!(errors(other_case, &[]), []);
}

// spec FTEF-10.6, QCD-3.54: an accessor's base is a type and no struct, the same each time, and
// one is needed; another type's name is no accessor's; under `-Fcaseinsens` the accessor is
// found in any case
#[test]
fn an_accessors_base_is_checked() {
    check(
        Code::E0258,
        &[
            "accessor A;",
            "accessor A : float; accessor A;",
            "accessor A : float; accessor A : vector;",
            "accessor A : struct S { float a; };",
            "struct S { float a; }; accessor A : S;",
        ],
        &[
            "accessor A : float; accessor A : float;",
            "accessor A : float *;",
            "float A; accessor A : float;",
        ],
    );
    assert_eq!(errors("accessor A : nosuch;", &[]), [Code::E0203]);
    assert_ne!(errors("typedef float A; accessor A : float;", &[]), []);
    let other_case = "accessor A : float; a x;";
    assert_eq!(errors(other_case, &["-Fcaseinsens"]), []);
    assert_eq!(errors(other_case, &[]), [Code::E0203]);
}

// spec FTEF-19.43: a cast's or `sizeof`'s name that is no type is refused, in any case under
// `-Fcaseinsens`; `struct s` is a tag in either case
#[test]
fn a_casts_type_name_is_a_type() {
    for (source, code) in [
        ("void() g = { (T *)0; };", Code::E0203),
        ("void() g = { local float x = sizeof(T *); };", Code::E0203),
        ("void() g = { local float x = sizeof(T); };", Code::E0301),
    ] {
        assert_eq!(errors(source, &[]), [code], "{source}");
    }
    // FTEF-16.15: a lone name that is no type is a value in parentheses, here an unknown one.
    assert_eq!(
        errors("void() g = { (T)0; };", &[]),
        [Code::E0301, Code::E0204]
    );
    let sizeof_s = "struct S { float a; }; void() g = { local float x = sizeof(s); };";
    assert_eq!(errors(sizeof_s, &["-Fcaseinsens"]), []);
    assert_eq!(errors(sizeof_s, &[]), [Code::E0301]);
    let tag = "struct S { float a; }; void() g = { (struct s *)0; };";
    assert_eq!(errors(tag, &["-Fcaseinsens"]), []);
    assert_eq!(errors(tag, &[]), []);
}

// spec FTEF-21.19, QCD-4.18: a `case` or `default` that no `switch` holds is "illegal cases"
#[test]
fn a_case_with_no_switch_is_refused() {
    check(
        Code::E0259,
        &[
            "void() g = { case 1: ; };",
            "void() g = { default: ; };",
            "void() g = { if (1) { case 1: ; } };",
            "void() g = { switch (1) { case 1: ; } case 2: ; };",
        ],
        &["void() g = { switch (1) { case 1: break; default: break; } };"],
    );
}

// spec FTEF-9.8: under `-Fcaseinsens` a class's name in another case is the class, as a type and
// after `class`; without it a name in another case is no type
#[test]
fn a_class_name_is_found_in_any_case() {
    let by_name = "class C { float a; }; c x;";
    assert_eq!(errors(by_name, &["-Fcaseinsens"]), []);
    assert_eq!(errors(by_name, &[]), [Code::E0203]);
    let after_class = "class C { float a; }; class c x;";
    assert_eq!(errors(after_class, &["-Fcaseinsens"]), []);
    assert_eq!(errors(after_class, &[]), []);
    let parent = "class D { float d; }; class C : d { float a; };";
    assert_eq!(errors(parent, &["-Fcaseinsens"]), []);
    assert_eq!(errors(parent, &[]), [Code::E0240]);
}

// spec FTEF-6.8, FTEF-6.10: under `-Fcaseinsens` a struct's tag in another case is the struct,
// whose body given again in that case is "redeclared differently" (the names differ); without
// it the tag is a new struct, and the name alone no type
#[test]
fn a_struct_tag_is_found_in_any_case() {
    let by_name = "struct S { float a; }; s x;";
    assert_eq!(errors(by_name, &["-Fcaseinsens"]), []);
    assert_eq!(errors(by_name, &[]), [Code::E0203]);
    let by_tag = "struct S { float a; }; struct s x;";
    assert_eq!(errors(by_tag, &["-Fcaseinsens"]), []);
    assert_eq!(errors(by_tag, &[]), []);
    for again in [
        "struct S { float a; }; struct s { float a; };",
        "struct S { float a; }; struct s { float b; };",
    ] {
        assert_eq!(errors(again, &["-Fcaseinsens"]), [Code::E0246], "{again}");
        assert_eq!(errors(again, &[]), [], "{again}");
    }
    let parent = "struct P { float p; }; struct S : p { float a; };";
    assert_eq!(errors(parent, &["-Fcaseinsens"]), []);
    assert_eq!(errors(parent, &[]), [Code::E0247]);
}

// spec FTEF-7.4: under `-Fcaseinsens` a typedef's name in another case is the type
#[test]
fn a_typedef_name_is_found_in_any_case() {
    let by_name = "typedef float t; T x;";
    assert_eq!(errors(by_name, &["-Fcaseinsens"]), []);
    assert_eq!(errors(by_name, &[]), [Code::E0203]);
    assert_eq!(
        errors("typedef float t; typedef float T;", &["-Fcaseinsens"]),
        []
    );
}

// spec FTEF-8.5: an enum's declarators are of its type, an anonymous enum's of its base, and an
// `enum class` value is no float
#[test]
fn an_enums_declarators_are_of_its_type() {
    for source in [
        "enum E { A } x; float y; void() g = { y = x; };",
        "enum { A } x; float y; void() g = { y = x; };",
        "enum E { A } x; E y; void() g = { y = x; };",
    ] {
        assert_eq!(errors(source, &[]), [], "{source}");
    }
    assert_eq!(errors("enum E { A } x; float y = x;", &[]), [Code::E0214]);
    assert_eq!(
        errors("enum class E : float { A } x; float y = x;", &[]),
        [Code::E0213]
    );
}

// spec FTEF-9.13, QCD-3.55: a class given its body again is compared member by member and in
// number; a member's type is a type, and a struct by its name is no member
#[test]
fn a_class_body_given_again_is_compared() {
    check(
        Code::E0260,
        &[
            "class C { float a; }; class C { float b; };",
            "class C { float a; }; class C { vector a; };",
            "class C { float a; }; class C { float a; float b; };",
            "class C { float a; }; class C { };",
            "class C; class C { float a; }; class C { float b; };",
        ],
        &[
            "class C { float a; }; class C { float a; };",
            "class C; class C { float a; };",
            "class C { float a; }; class C { float a; } x;",
        ],
    );
    assert_eq!(
        errors(
            "class C { float a; }; class c { float b; };",
            &["-Fcaseinsens"]
        ),
        [Code::E0260]
    );
    assert_eq!(errors("class O { T x; };", &[]), [Code::E0203]);
    assert_eq!(
        errors("struct S { float a; }; class O { S x; };", &[]),
        [Code::E0254]
    );
    assert_eq!(errors("struct S { float a; }; class O { S *x; };", &[]), []);
    assert_eq!(errors("class C { float a,; };", &[]), [Code::E0201]);
}

// spec FTEF-19.44: an enum whose base is a class or struct, by the legacy form or after `:`,
// has values no number gives
#[test]
fn an_enum_based_on_a_class_is_refused() {
    check(
        Code::E0229,
        &[
            "class E { float e; }; enum E { A };",
            "class E; enum E { A = 1 };",
            "struct E { float a; }; enum E { A };",
            "struct S { float a; }; enum E : S { A };",
            "class E { float e; }; struct S { enum E { A } e; };",
        ],
        &["enum E { A }; enum E { B };", "enum E : float { A };"],
    );
}

// spec FTEF-19.9: a typedef named as a struct, class, accessor or enum is "Cannot redeclare
// typedef", in any case under `-Fcaseinsens`, unless it names that type
#[test]
fn a_typedef_named_as_another_type_is_refused() {
    check(
        Code::E0219,
        &[
            "struct S { float a; }; typedef float S;",
            "class C { float a; }; typedef float C;",
            "accessor A : float; typedef float A;",
            "class E { float e; }; typedef enum { A } E;",
        ],
        &[
            "union U { float a; }; typedef float u;",
            "struct S { float a; }; typedef struct S S;",
        ],
    );
    assert_eq!(
        errors("union U { float a; }; typedef float u;", &["-Fcaseinsens"]),
        [Code::E0219]
    );
    assert_eq!(
        errors(
            "union U { float a; }; typedef float t, *u;",
            &["-Fcaseinsens"]
        ),
        [Code::E0219]
    );
    assert_eq!(
        errors(
            "union U { float a; }; typedef float t, u[2];",
            &["-Fcaseinsens"]
        ),
        []
    );
}

// spec FTEF-10.6: an accessor is declared wherever its type is written, so one with no base
// in a body, a parameter, a member, a cast or a `sizeof` is refused, and a base that is the
// accessor itself is no type
#[test]
fn an_accessor_written_as_a_type_is_declared_there() {
    for source in [
        "void() g = { local accessor A x; };",
        "void(accessor A a) h = #6;",
        "struct O { accessor A x; };",
        "void() g = { local float x = sizeof(accessor A); };",
        "typedef accessor A t;",
        "accessor A : float; void() g = { local accessor A x; };",
    ] {
        assert_eq!(errors(source, &[]), [Code::E0258], "{source}");
    }
    assert_eq!(errors("accessor A : A;", &[]), [Code::E0203]);
    assert_eq!(
        errors(
            "accessor A : float; void(accessor A : float a) h = #6;",
            &[]
        ),
        []
    );
}

// spec FTEF-6.11, QCD-4.19: a member no struct of the chain has is "not a member", and an
// array takes `.length` alone
#[test]
fn a_missing_member_and_an_arrays_member_are_refused() {
    assert_eq!(
        errors("struct S { float a; }; void() g = { S s; s.b = 1; };", &[]),
        [Code::E0261]
    );
    assert_eq!(
        errors(
            "struct P { float a; }; struct S : P { float b; }; void() g = { S s; s.a = 1; };",
            &[]
        ),
        []
    );
    assert_eq!(
        errors(
            "struct S { struct { float a; }; float b; }; void() g = { S s; s.a = 1; };",
            &[]
        ),
        []
    );
    assert_eq!(
        errors(
            "struct S { float a; }; S x[2]; void() g = { x.a = 1; };",
            &[]
        ),
        [Code::E0204]
    );
    assert_eq!(
        errors("float x[2]; void() g = { local float n = x.length; };", &[]),
        []
    );
}

// spec FTEF-6.9, QCD-3.56: a width's names are values and its literal is an integer
#[test]
fn a_bitfields_width_is_an_integer_constant() {
    assert_eq!(
        errors("struct S { int a : b; };", &[]),
        [Code::E0301, Code::E0262]
    );
    assert_eq!(errors("struct S { int a : 1.5; };", &[]), [Code::E0262]);
    assert_eq!(errors("float b = 2; struct S { int a : b; };", &[]), []);
    assert_eq!(errors("struct S { int a : (1 + 2); };", &[]), []);
}

// spec FTEF-19.41: a `virtual` function member of a struct is `Struct::member`, which needs a
// body; a `static` or `virtual` member takes one
#[test]
fn a_virtual_struct_member_needs_a_body() {
    assert_eq!(
        errors("struct S { virtual void() m; };", &[]),
        [Code::E0211]
    );
    for source in [
        "struct S { virtual void() m = {}; };",
        "struct S { float a; static void() m = {}; };",
        "struct S { void() m; };",
    ] {
        assert_eq!(errors(source, &[]), [], "{source}");
    }
}

// spec FTEF-6.10, QCD-3.52: a field of a struct with no body so far is "not fully defined"
#[test]
fn a_field_of_an_incomplete_struct_is_refused() {
    assert_eq!(errors(".struct S fl;", &[]), [Code::E0255]);
    assert_eq!(errors("struct S; .struct S fl;", &[]), [Code::E0255]);
    assert_eq!(errors("struct S { float a; }; .struct S fl;", &[]), []);
}

// spec FTEF-9.12: a class named as a sized type is refused with any body
#[test]
fn a_class_named_as_float_with_no_members_is_refused() {
    assert_eq!(errors("class float { };", &[]), [Code::E0242]);
    assert_eq!(errors("typedef float T; class T;", &[]), []);
}

// spec FTEF-19.43: a literal cast to a type is a value of that type, which the definition's
// type must take
#[test]
fn a_cast_literal_is_a_value_of_its_type() {
    for source in [
        "void() g = { local float x = (entity)0; };",
        "void() g = { local float x = (void)0; };",
        "class C { float a; }; void() g = { local float x = (C)0; };",
        "struct S { float a; }; void() g = { local float x = (struct S)0; };",
    ] {
        assert_eq!(errors(source, &[]), [Code::E0213], "{source}");
    }
    for source in [
        "void() g = { local entity x = (entity)0; };",
        "void() g = { local float x = (float)0; };",
        "void() g = { local float x = (int)0; };",
    ] {
        assert_eq!(errors(source, &[]), [], "{source}");
    }
}

// spec FTEF-6.8: `struct float` names `float`, the type the name finds
#[test]
fn a_struct_keyword_before_a_types_name_names_that_type() {
    assert_eq!(errors("struct float x = 1;", &[]), []);
    assert_eq!(errors("class O { struct float x; };", &[]), []);
}

// spec FTEF-6.10, QCD-3.43: a parent named through a typedef is the struct, and a body whose
// members the parent has already has none of its own
#[test]
fn a_parent_through_a_typedef_and_a_parents_members() {
    assert_eq!(
        errors(
            "typedef struct P { float a; } PP; struct S : PP { float b; };",
            &[]
        ),
        []
    );
    assert_eq!(
        errors("struct P { float a; }; struct S : P { float a; };", &[]),
        [Code::E0243]
    );
    assert_eq!(
        errors(
            "struct P { float a; }; struct S : P { float a; float b; };",
            &[]
        ),
        []
    );
}

// spec FTEF-9.13: the constructor is named exactly as the class, under `-Fcaseinsens` too, and
// declared without a body it is a function with none (FTEF-19.41)
#[test]
fn a_constructor_is_named_exactly_as_the_class() {
    assert_eq!(errors("class C { C(); };", &[]), [Code::E0211]);
    assert_eq!(errors("class C { C(); }; void C::C(void) {};", &[]), []);
    assert_eq!(
        errors("class c { C(); };", &["-Fcaseinsens"]),
        [Code::E0203]
    );
}

// spec FTEF-8.5: a typedef of an enum names the enum's type, an anonymous enum's base, which
// another type's name refuses
#[test]
fn a_typedef_of_an_enum_names_its_type() {
    assert_eq!(errors("typedef enum E { A } E, F; F x;", &[]), []);
    assert_eq!(
        errors("class E { float e; }; typedef enum { A } E;", &[]),
        [Code::E0219]
    );
}

// spec FTEF-9.13: a `static` or `ignore` member and an array size are compared as FTEQCC
// compares them
#[test]
fn a_class_body_compares_array_sizes_and_skips_globals() {
    check(
        Code::E0260,
        &[
            "class C { float a; }; class C { float a[2]; };",
            "class C { float a; }; class C { ignore float a; };",
        ],
        &["class C { float a; }; class C { float a; static float a; };"],
    );
}

// spec FTEF-10.6: an accessor with its base as a class member's type is refused
#[test]
fn an_accessor_with_its_base_in_a_class_is_refused() {
    assert_eq!(
        errors("class O { accessor A : float x; };", &[]),
        [Code::E0258]
    );
    assert_eq!(errors("accessor A : float; class O { A x; };", &[]), []);
}

// spec FTEF-6.11: a struct's name is no scope of its members or nested enum values
#[test]
fn a_struct_name_is_no_scope() {
    assert_eq!(
        errors("struct S { enum { A }; float a; }; float y = S::A;", &[]),
        [Code::E0301]
    );
    assert_eq!(
        errors("struct S { enum { A }; float a; }; float y = A;", &[]),
        []
    );
}

// spec FTEF-6.10: a struct's own name is no parent of it, and a struct a body writes is a type
// from there on
#[test]
fn a_struct_is_no_parent_of_itself() {
    assert_eq!(errors("struct S : S { float a; };", &[]), [Code::E0247]);
    assert_eq!(
        errors(
            "void() g = { struct S { struct T { float a; }; } i; };",
            &[]
        ),
        []
    );
}

// spec FTEF-6.12: a body with no tag is a struct type of its own
#[test]
fn an_anonymous_struct_takes_no_number() {
    assert_eq!(errors("struct { float a; } x = 1;", &[]), [Code::E0213]);
    assert_eq!(errors("union { float a; } x = 1;", &[]), [Code::E0213]);
    assert_eq!(errors("struct { float a; } x = { 1 };", &[]), []);
}

// spec FTEF-15.3, FTEF-16.9: a pointer to a function as a parameter, and any type in a `for`
// initialiser
#[test]
fn a_function_pointer_parameter_and_a_for_initialiser_type() {
    for source in [
        "void(void() *a) h = #6;",
        "void(float(float) *a) h = #6;",
        "void() g = { for (accessor A : float i; ; ) { break; } };",
        "void() g = { for (enumflags E { A } i; ; ) { break; } };",
        "enum { A }(float) h = #6;",
        "void() g = { local float x = (class float)0; };",
    ] {
        assert_eq!(errors(source, &[]), [], "{source}");
    }
}

// spec FTEF-15.5: a `var` of a function type named through a typedef holds a function and
// defines none
#[test]
fn a_var_of_a_typedef_function_type_needs_no_body() {
    assert_eq!(errors("typedef void() fn; var fn x;", &[]), []);
}

// spec FTEF-6.10: a struct a body writes lasts to the body's end; a struct of its name after it
// is another
#[test]
fn a_bodys_struct_ends_with_the_body() {
    assert_eq!(
        errors(
            "void() g = { struct S { float a; } s; }; struct S { float b; }; S x; float y; \
             void() h = { y = x.b; };",
            &[]
        ),
        []
    );
}

// spec FTEF-9.10: the value that makes no "Can only initialise member functions" is `0` or
// `0i`, as written (FTEF-9.17)
#[test]
fn a_null_member_value_is_the_constant_zero() {
    for source in ["class C { float a = 0; };", "class C { float a = 0i; };"] {
        assert_eq!(errors(source, &[]), [], "{source}");
    }
    for source in [
        "class C { float a = 0.0; };",
        "class C { float a = 00; };",
        "class C { float a = 0x0; };",
        "class C { float a = 0.5; };",
    ] {
        assert_eq!(errors(source, &[]), [Code::E0257], "{source}");
    }
}

// spec FTEF-21.7: a struct member written as C's `(*name)(params)` with `qcfuncs` off is a
// function type, the `*` the declarator's, compared so on redeclaration
#[test]
fn a_function_pointer_member_is_a_function_type() {
    let off = "#pragma flag disable qcfuncs\nstruct S { float (*fp)(float); }; \
               struct S { float (*fp)(float); };";
    assert_eq!(errors(off, &[]), []);
}

// spec FTEF-19.36, FTEF-19.33: an enumerator in another case is another name, under
// `-Fcaseinsens` too, while the enum is found in any case
#[test]
fn an_enumerator_in_another_case_is_another_name() {
    assert_eq!(errors("enum E { A = 1, a = 2 };", &["-Fcaseinsens"]), []);
    assert_eq!(
        errors("enum E { A = 1, A = 2 };", &["-Fcaseinsens"]),
        [Code::E0251]
    );
}

// spec FTEF-6.9: a negative width is a width the model does not follow, and no error
#[test]
fn a_negative_width_is_accepted() {
    assert_eq!(errors("struct S { int a : -1; };", &[]), []);
}

// spec FTEF-7.5, FTEF-19.35: a typedef's declarator `(name)` is a parameter that names no type
// spec FTEF-21.22: so the typedef is "expected function arg list"
#[test]
fn a_typedef_with_a_parenthesised_name_is_refused() {
    assert_eq!(errors("typedef float (t);", &[]), [Code::E0204]);
    assert_eq!(
        errors("typedef float t; typedef t (u);", &[]),
        [Code::E0204]
    );
    for source in [
        "typedef void() fn; fn g = { return; };",
        "typedef void() fn; fn g = #6;",
        "typedef void() fn; typedef fn fn2; fn2 g = { return; };",
        "typedef void() fn; fn g = [1, g] {};",
    ] {
        assert_eq!(errors(source, &[]), [], "{source}");
        assert_eq!(errors(source, &["-Fcaseinsens"]), [], "{source}");
    }
    // Through a macro; under `-Fcaseinsens` the macro `FN` takes `fn` too (dc0e5fd).
    assert_eq!(
        errors("#define FN void()\ntypedef FN fn; fn g = { return; };", &[]),
        []
    );
}

// spec FTEF-7.5: a definition of a typedef'd function type takes the typedef's parameters, a
// class member of one is a member function, a static struct member of one is no member
// compared, and a state's next function through one is checked as a `void()`
#[test]
fn a_typedef_function_type_carries_its_parameters_and_members() {
    for source in [
        "typedef void(float a) fn; fn g = { a = 1; };",
        "typedef void(float a) fn; typedef fn fn2; fn2 g = { a = 1; };",
        "typedef void() fn; class C { fn m = { return; }; };",
        "typedef void() fn; class C { virtual fn m = { return; }; };",
        "struct S { float a; }; typedef void() fn; struct S { float a; static fn m = { return; }; };",
        "typedef void() fn; fn g = [1, g] {};",
    ] {
        assert_eq!(errors(source, &[]), [], "{source}");
    }
    assert_eq!(
        errors(
            "#pragma flag disable qcfuncs\ntypedef void() fn; fn g = [1, g] {};",
            &[]
        ),
        [Code::E0309]
    );
    assert_eq!(
        errors("typedef void(float a) fn; fn g = { b = 1; };", &[]),
        [Code::E0301]
    );
}

// spec FTE-17.7: an array of functions is a global holding functions, with no body of its
// own, and an element may be called
#[test]
fn an_array_of_functions_needs_no_body() {
    for source in [
        "void() h = {}; void() g[2] = { h, h }; void() k = { g[0](); };",
        "void() h = {}; typedef void() fn; typedef fn fns[2]; fns g = { h, h };",
        "void() h = {}; var void() g[2] = { h, h };",
    ] {
        assert_eq!(errors(source, &[]), [], "{source}");
        assert_eq!(errors(source, &["-Fcaseinsens"]), [], "{source}");
    }
}

// spec FTE-17.7, FTEF-19.8: an array of functions with no initialiser has no body, each
// dimension of an array takes its own braces, and a state's next think of another function
// type is a mismatch
#[test]
fn arrays_of_functions_are_checked() {
    assert_eq!(errors("void() x[2];", &[]), [Code::E0211]);
    assert_eq!(
        errors("#pragma flag disable qcfuncs\nfloat (*x[2])(float);", &[]),
        []
    );
    assert_eq!(
        errors("void() h = {}; void() g[2][2] = { h, h, h, h };", &[]),
        [Code::E0204]
    );
    assert_eq!(
        errors(
            "void() h = {}; void() g[2][2] = { { h, h }, { h, h } };",
            &[]
        ),
        []
    );
    assert_eq!(
        errors("float x[2][2] = { 1, 2, 3, 4 };", &[]),
        [Code::E0204]
    );
    assert_eq!(
        errors("#pragma flag disable qcfuncs\nvoid() g = [1, g] {};", &[]),
        [Code::E0309]
    );
    assert_eq!(errors("void() g = [1, g] {};", &[]), []);
}

// spec FTEF-15.9: with `qcfuncs` off `(name)` after a function type is a QC parameter list, which
// leaves the typedef no name
#[test]
fn a_parenthesised_name_needs_qcfuncs() {
    assert_eq!(errors("typedef void() (fn); fn g;", &[]), [Code::E0211]);
    assert!(!errors("#pragma flag disable qcfuncs\ntypedef void() (fn);", &[]).is_empty());
}

// spec FTEF-20.15: a body's typedefs and struct tags last to their block's end and are no type
// after the body; its enum is declared as one at file level, divergently
#[test]
fn a_bodys_types_last_to_their_block() {
    for source in [
        "void() g = { { typedef float t; t x; } };",
        "void() g = { struct S { float a; }; S x; };",
        "void() g = { typedef struct S { float a; } T; T x; S y; };",
        "void() g = { enum E { A }; E e = A; };",
    ] {
        assert_eq!(errors(source, &[]), [], "{source}");
    }
    assert!(!errors("void() g = { { typedef float t; } t x; };", &[]).is_empty());
    assert_eq!(
        errors("void() g = { { struct S { float a; }; } S x; };", &[]),
        [Code::E0301, Code::E0204]
    );
    for source in [
        "void() g = { typedef struct S { float a; } T; }; S y;",
        "void() g = { struct S { float a; }; }; S y;",
    ] {
        assert_eq!(errors(source, &[]), [Code::E0203], "{source}");
    }
    assert_eq!(errors("void() g = { enum { A }; }; float x = A;", &[]), []);
}

// spec FTEF-19.48: a global initialised with an unknown name is no constant, but for a scoped one
#[test]
fn an_unknown_initialiser_is_no_constant() {
    assert_eq!(errors("float y = B;", &[]), [Code::E0214, Code::E0301]);
    assert_eq!(
        errors("void() g = { static float x = B; };", &[]),
        [Code::E0214, Code::E0301]
    );
    assert_eq!(
        errors("void() g = { local float x = B; };", &[]),
        [Code::E0301]
    );
}

// spec FTEF-6.11: an anonymous member's members are searched, in any case under `-Fcaseinsens`
#[test]
fn an_anonymous_members_members_are_matched() {
    let program = "struct S { union { float a; vector v; }; float A; }; S x; \
                   void() g = { x.a = 1; };";
    assert_eq!(errors(program, &["-Fcaseinsens"]), [Code::E0252]);
    assert_eq!(errors(program, &[]), []);
}

// spec FTEF-9.7: under `-Fcaseinsens` a basic type's word is no parent either
#[test]
fn a_basic_type_is_no_parent_in_any_case() {
    for parent in ["vector", "string", "void"] {
        let source = format!("class C : {parent} {{ float a; }};");
        assert_eq!(
            errors(&source, &["-Fcaseinsens"]),
            [Code::E0241],
            "{parent}"
        );
        assert_eq!(errors(&source, &[]), [Code::E0241], "{parent}");
    }
}

// spec FTEF-21.23: the illegal jump errors name the function, a member as `CLASS::NAME`
#[test]
fn illegal_jumps_name_their_function() {
    let messages = |source: &str| -> Vec<String> {
        let mut db = Db::new();
        db.set_dialect(Dialect::Fteqcc);
        db.set_file(FileId(0), PathBuf::from("t.qc"), source.as_bytes().to_vec());
        db.analyze_module(&ModuleInput {
            compiled: vec![FileId(0)],
            command_line: CommandLine::new::<&str>(&[]),
            ..ModuleInput::default()
        })
        .diagnostics()
        .filter(|d| matches!(d.diagnostic.code, Code::E0244 | Code::E0245 | Code::E0259))
        .map(|d| d.diagnostic.message.clone())
        .collect()
    };
    assert_eq!(
        messages("void() g = { break; };\n"),
        ["g: function contains illegal breaks"]
    );
    assert_eq!(
        messages("void() g = { { case 1: ; } { case 2: ; } };\n"),
        ["g: function contains illegal cases"]
    );
    assert_eq!(
        messages("class C { void() m = { continue; }; };\n"),
        ["C::m: function contains illegal continues"]
    );
}

// spec FTEF-6.10: a struct's redeclaration with another parent is reported at its tag
#[test]
fn a_structs_redeclaration_stands_at_its_tag() {
    let source = "struct P { float a; }; struct Q { float b; }; struct S : P { float c; }; \
                  struct\nS : Q { float d; };\n";
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FileId(0), PathBuf::from("t.qc"), source.as_bytes().to_vec());
    let found: Vec<(Code, usize)> = db
        .analyze_module(&ModuleInput {
            compiled: vec![FileId(0)],
            command_line: CommandLine::new::<&str>(&[]),
            ..ModuleInput::default()
        })
        .diagnostics()
        .map(|d| {
            let line = source
                .get(..d.diagnostic.span.start)
                .map_or(0, |before| before.matches('\n').count() + 1);
            (d.diagnostic.code, line)
        })
        .collect();
    assert_eq!(found, [(Code::E0249, 2)]);
}

// spec FTEF-21.24: a stripped arm's breaks are forgotten; end messages name CLASS::NAME
#[test]
fn stripped_arms_leave_no_illegal_jumps() {
    let messages = |source: &str| -> Vec<String> {
        let mut db = Db::new();
        db.set_dialect(Dialect::Fteqcc);
        db.set_file(FileId(0), PathBuf::from("t.qc"), source.as_bytes().to_vec());
        db.analyze_module(&ModuleInput {
            compiled: vec![FileId(0)],
            command_line: CommandLine::new::<&str>(&[]),
            ..ModuleInput::default()
        })
        .diagnostics()
        .map(|d| d.diagnostic.message.clone())
        .collect()
    };
    assert!(
        messages("float f; float() g = { if (0) { break; continue; } return 2; };\n").is_empty()
    );
    assert_eq!(
        messages("float f; float() g = { if (1) { break; } return 2; };\n"),
        ["g: function contains illegal breaks"]
    );
    assert_eq!(
        messages("void() g = { continue; break; };\n"),
        ["g: function contains illegal continues"]
    );
    assert_eq!(
        messages(
            "float time; class C : entity { nonvirtual float() m = { if (time) return 1; }; };\n"
        ),
        ["C::m: not all control paths return a value"]
    );
}

// spec FTEF-6.29: a member with no name declares nothing; a virtual member must be a function
// spec QCD-4.70: E0369 "[non]virtual members must be functions"
#[test]
fn struct_members_that_declare_nothing() {
    let messages = |source: &str| -> Vec<(Code, String)> {
        let mut db = Db::new();
        db.set_dialect(Dialect::Fteqcc);
        db.set_file(FileId(0), PathBuf::from("t.qc"), source.as_bytes().to_vec());
        db.analyze_module(&ModuleInput {
            compiled: vec![FileId(0)],
            command_line: CommandLine::new::<&str>(&[]),
            ..ModuleInput::default()
        })
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect()
    };
    assert_eq!(
        messages("struct S { vector; float a; };\n"),
        [(
            Code::W0204,
            "declaration does not declare anything (vector)".to_owned()
        )]
    );
    assert!(messages("struct S { struct { float a; }; };\n").is_empty());
    assert_eq!(
        messages("struct S { virtual float x; };\n"),
        [(
            Code::E0369,
            "[non]virtual members must be functions".to_owned()
        )]
    );
    // A typedef of a function type is a function.
    assert!(messages("typedef void() fn; struct S { virtual fn m = { return; }; };\n").is_empty());
    assert_eq!(
        messages("typedef void() fn; struct S { virtual fn m; };\n"),
        [(Code::E0211, "function S::m has no body".to_owned())]
    );
}
