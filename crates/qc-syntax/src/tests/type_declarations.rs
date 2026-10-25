// SPDX-License-Identifier: GPL-3.0-or-later

//! The type declarations `QCC_PR_ParseType`, `QCC_PR_ParseStruct`, `QCC_PR_ParseEntClass` and
//! `QCC_PR_ParseDefs` read (`docs/spec/dialect-fteqcc.md` FTEF-6, FTEF-7, FTEF-9, FTEF-10), in
//! every position a type stands. Each program was read through the pinned FTEQCC
//! (`cargo xtask oracles builtins`, the `types` family) first.

use crate::ast::AstNode as _;
use crate::{Code, Dialect, ast, parse_with};

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
        assert!(too_deep || source.starts_with("typedef"), "{}", &source[..20]);
    }
}
