// SPDX-License-Identifier: GPL-3.0-or-later

//! The type declarations `QCC_PR_ParseType`, `QCC_PR_ParseStruct`, `QCC_PR_ParseEntClass` and
//! `QCC_PR_ParseDefs` read (`docs/spec/dialect-fteqcc.md` FTEF-6, FTEF-7, FTEF-9, FTEF-10), in
//! every position a type stands. Each program was read through the pinned FTEQCC
//! (`cargo xtask oracles builtins`, the `types` family) first.

use qc_preproc::{CompilerFlags, FlagEdit, FteFlag};

use crate::ast::AstNode as _;
use crate::{Code, Dialect, ast, parse_preprocessed_flagged, parse_with};

/// The error codes of `source` under the FTEQCC dialect, after checking that the tree gives back
/// every byte.
fn errors(source: &str) -> Vec<Code> {
    let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
    assert_eq!(parsed.syntax().text(), source.as_bytes(), "{source}");
    parsed.errors().map(|d| d.code).collect()
}

/// Every program of `accepted` has no error.
fn accepts(accepted: &[&str]) {
    for source in accepted {
        assert_eq!(errors(source), [], "{source}");
    }
}

// spec FTEF-6.7: a struct's body takes access labels, nested and anonymous types with or without
// a declarator, and members with no name
#[test]
fn a_struct_body_is_read_as_parse_struct_reads_it() {
    accepts(&[
        "struct S { public: float a; private: float b; protected: float c; };",
        "struct S { struct T { float a; }; float b; };",
        "struct S { struct T { float a; } t; float b; };",
        "struct S { enum { A }; float a; };",
        "struct S { struct { float a; }; float b; };",
        "struct S { union { float a; vector v; }; float A; };",
        "struct S { float; };",
        "struct S { float a, *b; };",
        "struct S { float a;; float b; };",
        "struct S {\nfloat a;\nfloat b;\n};",
        "union U {\nfloat a;\nvector v;\n};",
        "struct S { float(float) f; };",
        "struct S { float a; };\nvoid() g = { struct { float b; } x; x.b = 1; };",
    ]);
    for source in [
        "struct S { float a float b; };",
        "struct S { , float a; };",
        "struct S { float a = 1; };",
        "struct S { float (*fp)(float); };",
        "struct S { public float a; };",
        "struct S { public: };",
    ] {
        assert_ne!(errors(source), [], "{source}");
    }
}

// spec FTEF-6.7, QCD-3.43: a body with no member that is not `static` has none
#[test]
fn a_struct_with_no_members_is_refused() {
    for source in [
        "struct S { };",
        "struct S { ;; };",
        "union U { };",
        "struct S { static float a; };",
        "struct : P { };",
    ] {
        assert!(
            errors(source).contains(&Code::E0243) || source == "struct : P { };",
            "{source}"
        );
    }
    assert_eq!(errors("struct S { static float a; float b; };"), []);
}

// spec FTEF-6.8: a parent needs the body and a union has none; a tag alone names the type
#[test]
fn a_struct_head_is_read_as_parse_struct_reads_it() {
    accepts(&[
        "struct P { float a; }; struct S : P { float b; };",
        "struct S;",
        "struct S x;",
        "struct S *p;",
        "struct { float a; } x, *p;",
    ]);
    for source in [
        "struct P { float a; }; struct S : P;",
        "union U : P { float b; };",
        "struct *p;",
        "struct 5 { float a; };",
    ] {
        assert_ne!(errors(source), [], "{source}");
    }
}

// spec FTEF-7.4: a typedef has the declarators of any definition, and `typedef` is a qualifier
// read any number of times, in a body too
#[test]
fn a_typedef_has_declarators() {
    accepts(&[
        "typedef float t, u;",
        "typedef float t, *u;",
        "typedef void() fn, fn2;",
        "typedef struct S { float a; } T, *PT;",
        "typedef enum { A } E, F;",
        "typedef typedef float tt;",
        "typedef class C { float a; } CC;",
        "void() g = { local typedef float t; t x; };",
        "void() g = { typedef float t, u; };",
    ]);
    let source = "typedef float t, *u, v[2];\n";
    let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
    let typedef = parsed
        .syntax()
        .descendants()
        .find_map(ast::TypedefDecl::cast)
        .unwrap();
    let declarators: Vec<_> = typedef.declarators().collect();
    assert_eq!(declarators.len(), 3);
    assert_eq!(
        declarators
            .iter()
            .map(|d| d.name().unwrap().text_lossless())
            .collect::<Vec<_>>(),
        ["t", "u", "v"]
    );
    assert_eq!(
        declarators.get(1).map(ast::Declarator::pointer_depth),
        Some(1)
    );
    assert_ne!(errors("typedef float t = 1;"), []);
}

// spec FTEF-9.8, QCD-3.50: a class is a type wherever a type stands, a name alone names it, and a
// parent needs the body; a class with a body in a function is refused
#[test]
fn a_class_is_a_type() {
    accepts(&[
        "class C; class C x;",
        "class C { float a; } x;",
        "class C { float a; }; class C x;",
        "void() g = { class C x; };",
        "class C; void() g = { class C *p; };",
        "class C; void(class C x) h = #6;",
        "class float x;",
        "class D { float d; }; class C : D { float a; };",
        "void() g = { local float x = sizeof(class C *); };",
        "typedef class t; t x;",
    ]);
    for source in [
        "class C { float a; }; class D : C;",
        "class D : C;",
        "class 5 x;",
        "void() g = { class C { float a; }; };",
    ] {
        assert_ne!(errors(source), [], "{source}");
    }
    assert_eq!(
        errors("void() g = { class C { float a; }; };"),
        [Code::E0253]
    );
}

// spec FTEF-9.9: `protected:` reads two colons, a label is followed by its member, and a `;` is
// no member
#[test]
fn a_class_body_is_read_as_parse_ent_class_reads_it() {
    accepts(&[
        "class C { public: private: float a; };",
        "class C { protected: : float a; };",
        "class C { float a; private: float b; };",
        "class C { virtual: void() m; nonvirtual: void() n; };",
    ]);
    for source in [
        "class C { private: float a; protected: float b; };",
        "class C { protected: float a; };",
        "class C { private: };",
        "class C { float a; ; };",
        "class C { , float a; };",
        "class C { static private: float a; };",
    ] {
        assert_ne!(errors(source), [], "{source}");
    }
}

// spec FTEF-9.11, QCD-3.51: a class member that is a struct or union is refused, a pointer to
// one is not
#[test]
fn a_class_holds_no_struct_member() {
    assert_eq!(errors("class C { struct S { float a; }; };"), [Code::E0254]);
    assert_eq!(errors("class C { struct S s; };"), [Code::E0254]);
    assert_eq!(errors("class C { struct S *s; };"), []);
}

// spec FTEF-10.5: an accessor is a type wherever a type stands
#[test]
fn an_accessor_is_a_type() {
    accepts(&[
        "accessor A : float;",
        "accessor A : float; typedef accessor B : float;",
        "accessor A : float; static accessor A : float;",
        "accessor A : float; A x, y;",
    ]);
}

// spec FTEF-18.33: the body of a `switch` is any statement
#[test]
fn a_switch_body_is_a_statement() {
    accepts(&[
        "void() g = { switch (1) ; };",
        "void() g = { switch (1) break; };",
        "void() g = { switch (1) { case 1: break; } };",
    ]);
}

// spec FTEF-18.34: a body ends at a declaration line only when no `}` stands ahead, and so a
// member or a local at column 0 is one
#[test]
fn a_body_ends_at_a_declaration_line_only_without_its_brace() {
    accepts(&[
        "class C {\nfloat a;\nvoid() m;\n};",
        "void() g = {\nfloat x;\nentity e;\nx = 1;\n};",
        "void() g = { switch (1) {\nfloat x;\ncase 1: break;\n} };",
    ]);
    for source in [
        "struct S { float a;\nfloat b;\n",
        "class C { float a;\nfloat b;\n",
        "void() g = {\nfloat x;\n",
    ] {
        assert_ne!(errors(source), [], "{source}");
    }
}

// spec QC-27.15: nested struct, union, class and accessor types, closed or not, as members, as
// casts and as parameters, stop at the nesting limit with "nesting too deep" on a 2 MiB thread
// and keep every byte
#[test]
fn nested_types_stop_at_the_limit_on_a_small_stack() {
    let n = 50_000;
    let shapes = [
        format!("{}float a;{}", "struct S { ".repeat(n), " };".repeat(n)),
        format!("{}float a;", "struct S { ".repeat(n)),
        format!("{}float a;{}", "class C { ".repeat(n), " };".repeat(n)),
        format!("{}float a;", "class C { struct { ".repeat(n)),
        format!("{}float a;", "struct { union { ".repeat(n)),
        format!("void() g = {{ {}0; }};", "(struct S *".repeat(n)),
        format!("void({}) h = #6;", "struct { ".repeat(n)),
        format!("{}float;", "accessor A : accessor B : ".repeat(n)),
        format!("{}x;", "typedef ".repeat(n)),
    ];
    for source in shapes {
        let bytes = source.clone().into_bytes();
        let (kept, too_deep) = std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || {
                let parsed = parse_with(&bytes, Dialect::Fteqcc);
                (
                    parsed.syntax().text() == bytes,
                    parsed.errors().any(|d| d.code == Code::E0509),
                )
            })
            .unwrap()
            .join()
            .unwrap();
        assert!(kept, "{}", &source[..20]);
        assert!(
            too_deep || source.starts_with("typedef"),
            "{}",
            &source[..20]
        );
    }
}

// spec FTEF-8.5: `enum class` needs its base type after the name, and an enum's body takes
// declarators
#[test]
fn an_enum_class_needs_its_base_type() {
    accepts(&[
        "enum class E : float { A };",
        "enum class E : float { A } x;",
        "enum E { A } x, *p;",
        "enum { A } x;",
    ]);
    for source in ["enum class E { A };", "enum class { A };", "enum class E;"] {
        assert_eq!(errors(source), [Code::E0204], "{source}");
    }
}

// spec FTEF-6.8, FTEF-21.7: with `qcfuncs` on the `*`s after a struct's, class's, enum's or
// accessor's body are the type's, so `*;` declares nothing and `(*x)(float)` is no function
// pointer; with it off the `*` is the declarator's
#[test]
fn the_pointers_after_a_type_declaration_are_the_types() {
    accepts(&[
        "struct S { float a; } *;",
        "struct { float a; } *;",
        "class C { float a; } *;",
        "enum E { A } *;",
        "accessor A : float *;",
        "struct S { float a; } *p, q;",
    ]);
    for source in [
        "struct S { float a; } (*x)(float);",
        "struct S { float a; } * (*x)(float);",
        "union { float a; } (*x)(float);",
        "class C { float a; } (*x)(float);",
    ] {
        assert_ne!(errors(source), [], "{source}");
    }
    // With `qcfuncs` off from the start the `(*x)` is C's function pointer (FTEF-21.7).
    let off = "struct S { float a; } (*x)(float);";
    let edit = FlagEdit::of_flags(
        &[FteFlag::Qcfuncs],
        CompilerFlags::default().with(FteFlag::Qcfuncs, false),
    );
    let parsed =
        parse_preprocessed_flagged(off.as_bytes(), Dialect::Fteqcc, &[(0, edit)], &[], &[]);
    assert_eq!(parsed.errors().map(|d| d.code).collect::<Vec<_>>(), []);
    let two = "struct S { float a; } *p, q;";
    let parsed = parse_with(two.as_bytes(), Dialect::Fteqcc);
    let decl = parsed
        .syntax()
        .descendants()
        .find_map(ast::StructDecl::cast)
        .unwrap();
    assert_eq!(
        decl.declarators()
            .map(|d| d.pointer_depth())
            .collect::<Vec<_>>(),
        [0, 0]
    );
}

// spec FTEF-6.7: a `static` or `virtual` member of a struct takes a body or value, any other
// member's `=` is refused
#[test]
fn a_static_or_virtual_struct_member_takes_a_body() {
    accepts(&[
        "struct S { virtual void() m = {}; };",
        "struct S { float a; static void() m = {}; };",
        "struct S { float a; static void() m {}; };",
        "struct S { float a; static float b = 1; };",
    ]);
    assert_ne!(errors("struct S { float a = 1; };"), []);
    assert_ne!(errors("struct S { void() m = {}; };"), []);
}

// spec FTEF-6.7: a member's missing `;` is reported and the body is read on, to its `}`
#[test]
fn a_missing_semicolon_between_members_does_not_end_the_body() {
    let source = "struct S { float a float b; }; S x;";
    let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
    assert_eq!(
        parsed.errors().map(|d| d.code).collect::<Vec<_>>(),
        [Code::E0204]
    );
    let decl = parsed
        .syntax()
        .descendants()
        .find_map(ast::StructDecl::cast)
        .unwrap();
    assert_eq!(decl.fields().count(), 2);
    assert_eq!(decl.declarators().count(), 0);
}

// spec FTEF-9.11: the struct check reads the member's own type, not the one before it
#[test]
fn the_struct_member_check_is_per_member() {
    assert_eq!(errors("class C { float *p; struct S s; };"), [Code::E0254]);
    assert_eq!(errors("class C { struct S *s; float a; };"), []);
    assert_eq!(
        errors("class C { struct S *s; struct S t; };"),
        [Code::E0254]
    );
}

// spec FTEF-18.34: a block of many declaration lines is read whole whatever its length, each
// line a local, with no error, and one that lost its `}` ends at the first
#[test]
fn many_declaration_lines_in_one_block_are_all_locals() {
    for lines in [1_usize, 100, 2_000] {
        let body = "float a;\n".repeat(lines);
        let source = format!("void() g = {{\n{body}}};");
        let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
        assert_eq!(parsed.syntax().text(), source.as_bytes());
        assert_eq!(parsed.errors().count(), 0, "{lines} lines");
        let locals = parsed
            .syntax()
            .descendants()
            .filter(|n| n.kind() == crate::SyntaxKind::LocalDecl)
            .count();
        assert_eq!(locals, lines, "{lines} lines");
        let lost = format!("void() g = {{\n{body}");
        let parsed = parse_with(lost.as_bytes(), Dialect::Fteqcc);
        assert_eq!(parsed.syntax().text(), lost.as_bytes());
        assert_ne!(parsed.errors().count(), 0, "{lines} lines, no brace");
    }
}

// spec FTEF-7.5: a definition whose type is a typedef of a function type takes a body, a state
// header or a builtin, as any function does, through a chain of typedefs too, and a typedef
// whose declarator is `(name)` is refused as any such type is
#[test]
fn a_typedef_of_a_function_type_defines_functions() {
    accepts(&[
        "typedef void() fn; fn g = { return; };",
        "typedef void() fn; fn g = { ; };",
        "typedef void() fn; fn g = { float x; };",
        "typedef void() fn; fn g = { 1; };",
        "typedef void() fn; fn g = { g(); };",
        "typedef void() fn; fn g = #6;",
        "typedef void() fn; fn g : 6;",
        "typedef void() fn; fn g = [1, g] {};",
        "typedef void() fn; fn g = [$a1, g] {};",
        "typedef float(float) fn; fn g = { return 1; };",
        "typedef void(float a) fn; fn g = { a = 1; };",
        "typedef void() fn; static fn g = { return; };",
        "typedef void() fn; var fn g = { return; };",
        "typedef void() fn; fn g = {}, h = { return; };",
        "typedef void() fn; void() g = { fn h = { return; }; };",
        "typedef void() fn; void() g = { local fn h = { return; }; };",
        "typedef void() fn; class C { fn m = { return; }; };",
        "typedef void() fn; class C { virtual fn m = { return; }; };",
        "typedef void() fn; class C { static fn m = { return; }; };",
        "typedef void() fn; class C { fn m; }; fn C::m = { return; };",
        "typedef void() fn; struct S { float a; static fn m = { return; }; };",
        "typedef void() fn; extern \"C\" { fn g = { return; }; }",
        "typedef void() fn; extern \"QC\" { fn g = { return; }; }",
        "typedef void() fn; typedef fn fn2; fn2 g = { return; };",
        "void() g = { typedef void() fn; fn h = { return; }; };",
        "typedef void() fn; fn g = {};",
    ]);
    assert_ne!(errors("typedef void() fn; fn g = { 1 };"), []);
}
