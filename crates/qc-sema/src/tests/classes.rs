// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC classes as values and their member functions' qualifiers
//! (`docs/spec/dialect-fteqcc.md` FTEF-9.14 to FTEF-9.16). Each program was read through the
//! pinned FTEQCC first.

#![expect(clippy::indexing_slicing, reason = "test code: fixed fixtures")]

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
        format!("entity self;\n{source}\n").into_bytes(),
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

fn one(code: Code, message: &str) -> Vec<(Code, String)> {
    vec![(code, message.to_owned())]
}

// spec FTEF-9.14: an implicit cast between entity types warns when neither the value's class nor
// one it derives from has the target's members; a class with no parent derives from the entity
#[test]
fn a_class_cast_warns_when_no_class_in_the_chain_has_the_members() {
    let warned = [
        (
            "class C { float a; }; entity e; void() h = { C c = e; };",
            "Implicit cast from entity to class C",
        ),
        (
            "class C { float a; }; class D { string b; }; D d; void(C c) k = {}; \
             void() h = { k(d); };",
            "Implicit cast from class D to class C",
        ),
        (
            "class C { float a; }; class D { float a; string b; }; D d; void() h = { C c = d; };",
            "Implicit cast from class D to class C",
        ),
        (
            "class C { float a; virtual void() m; }; class D { float a; nonvirtual void() m; }; \
             D d; void() h = { C c = d; };",
            "Implicit cast from class D to class C",
        ),
    ];
    for (source, message) in warned {
        let found: Vec<_> = messages(source)
            .into_iter()
            .filter(|(code, _)| *code != Code::E0211)
            .collect();
        assert_eq!(found, one(Code::W0220, message), "{source}");
    }
    let silent = [
        "class C { }; entity e; void(C c) k = {}; void() h = { k(e); C c2 = e; };",
        "class C { float a; }; C c; void() h = { entity e = c; };",
        "class C { float a; }; class D : C { float b; }; C c; void() h = { D d = c; };",
        "class C { float a; static float s; }; class D { float a; }; D d; \
         void() h = { C c = d; };",
        "class B { string z; }; class C : B { float a; }; class D : B { float a; }; D d; \
         void() h = { C c = d; };",
        "class C { float a; }; entity e; void() h = { C c = (C)e; };",
        "class C { .C next; }; class D { .C next; }; D d; void() h = { C c = d; };",
    ];
    for source in silent {
        assert_eq!(messages(source), [], "{source}");
    }
}

// spec FTEF-9.14: a global initialised with a cast of a variable is no constant
// spec FTEF-34.9: the types a global initialiser's casts write are read as a body's
#[test]
fn a_global_initialised_with_a_cast_of_a_variable_is_no_constant() {
    assert_eq!(
        messages("class C { }; entity e; C c = (C)e;"),
        one(Code::E0214, "initializer for 'c' is not constant")
    );
    assert_eq!(
        messages("float e; float c = (float)e;"),
        one(Code::E0214, "initializer for 'c' is not constant")
    );
    assert_eq!(messages("const float e = 2; float c = (float)e;"), []);
    assert_eq!(messages("float c = (float)5;"), []);
}

// spec FTEF-30.3: two classes' members are compared on `?:`, by `typecmp_lax`
#[test]
fn ternary_arms_of_two_classes_compare_their_members() {
    let program = |d: &str| {
        format!(
            "class C {{ float a; }}; class D {{ {d} }}; C c; D d; float f; \
             void() h = {{ local entity e = f ? c : d; }};"
        )
    };
    assert_eq!(
        messages(&program("string b;")),
        one(
            Code::W0244,
            "Type mismatch on ternary operator: class C vs class D"
        )
    );
    assert_eq!(
        messages(&program("float b;")),
        one(Code::E0332, "Ternary operator with mismatching types")
    );
}

// spec FTEF-9.15, QCD-3.57: a virtual member a parent has as a nonvirtual function is refused
#[test]
fn a_virtual_member_a_parent_has_as_nonvirtual_is_refused() {
    assert_eq!(
        messages("class C { nonvirtual void() m = {}; }; class D : C { virtual void() m = {}; };"),
        one(
            Code::E0350,
            "m defined as virtual in D, but nonvirtual in C"
        )
    );
    assert_eq!(
        messages("class C { virtual void() m = {}; }; class D : C { virtual void() m = {}; };"),
        []
    );
}

// spec FTEF-9.16, QCD-7.107: an unqualified member function is assumed nonvirtual with a
// warning, unless a `virtual:` or `nonvirtual:` label set the default
#[test]
fn an_unqualified_member_function_warns() {
    assert_eq!(
        messages("class C { void() m = {}; };"),
        one(Code::W0265, "C::m was not qualified. Assuming non-virtual.")
    );
    for source in [
        "class C { virtual: void() m = {}; };",
        "class C { nonvirtual: void() m = {}; };",
        "class C { static void() m = {}; };",
        "class C { float a; };",
    ] {
        assert_eq!(messages(source), [], "{source}");
    }
}

// spec FTEF-9.16, QCD-7.107: a constructor is warned of being static, returning a value and
// taking arguments, each naming the token after its name
#[test]
fn a_constructor_is_warned_of_its_qualifiers_and_type() {
    assert_eq!(
        messages("class C { static float() C = { return 1; }; };"),
        [
            (
                Code::W0265,
                "Constructor C::= should not be static.".to_owned()
            ),
            (
                Code::W0265,
                "Constructor C::= does not return void.".to_owned()
            ),
        ]
    );
    assert_eq!(
        messages("class C { nonvirtual void(float a) C = {}; };"),
        one(
            Code::W0265,
            "Constructor arguments are not supported at this time C::=. Use named fields instead."
        )
    );
    assert_eq!(
        messages("class C { float() C[2]; };"),
        one(Code::W0265, "Constructor C::; does not return void.")
    );
    assert_eq!(messages("class C { nonvirtual void() C = {}; };"), []);
}

// spec FTEF-9.16, QCD-3.58: more than one of `virtual`, `nonvirtual` and `static` or `const` is
// refused
#[test]
fn conflicting_member_qualifiers_are_refused() {
    assert_eq!(
        messages("class C { virtual static void() m = {}; };"),
        one(Code::E0351, "Multiple conflicting qualifiers on C::=.")
    );
    assert_eq!(
        messages("class C { const nonvirtual void() m; };")[0],
        (
            Code::E0351,
            "Multiple conflicting qualifiers on C::;.".to_owned()
        )
    );
}

// spec FTEF-9.16: a member function nothing defines is named by its class, and an array of member
// functions is not reported
#[test]
fn a_member_function_with_no_body_is_named_by_its_class() {
    assert_eq!(
        messages("class C { virtual void() n; };"),
        one(Code::E0211, "function C::n has no body")
    );
    assert_eq!(
        messages("class C { static void() C, n; };"),
        [
            (
                Code::W0265,
                "Constructor C::, should not be static.".to_owned()
            ),
            (Code::E0211, "function C::C has no body".to_owned()),
            (Code::E0211, "function C::n has no body".to_owned()),
        ]
    );
    assert_eq!(messages("class C { virtual void() n[2]; };"), []);
}

// spec FTEF-17.15: a member defined with a type `typecmp` tells from the declared one is a type
// mismatch, a bare constructor being of type `void()`
#[test]
fn a_member_defined_with_another_type_is_refused() {
    let mismatch = |message: &str| one(Code::E0205, message);
    let found = |source: &str| -> Vec<(Code, String)> {
        messages(source)
            .into_iter()
            .filter(|(code, _)| *code != Code::E0211)
            .collect()
    };
    assert_eq!(
        found("class C { nonvirtual void(float x) m; }; void() C::m = {};"),
        mismatch("Type mismatch on redeclaration of C::m. void(), should be void(float x)")
    );
    assert_eq!(
        found("class C { virtual void(float x) m; }; float(float x) C::m = { return 1; };"),
        mismatch("Type mismatch on redeclaration of C::m. float(float x), should be void(float x)")
    );
    assert_eq!(
        found("class C { C(); }; C() C::C = {};"),
        mismatch("Type mismatch on redeclaration of C::C. C(), should be void()")
    );
    assert_eq!(
        found("class C { nonvirtual void(C x) m; }; void(entity x) C::m = {};"),
        mismatch(
            "Type mismatch on redeclaration of C::m. void(entity x), should be void(class C x)"
        )
    );
    for source in [
        "class C { nonvirtual void(float x) m; }; void(float y) C::m = {};",
        "class C { C(); }; void() C::C = {};",
        "typedef void(float) fn; class C { nonvirtual fn m; }; void(float x) C::m = {};",
    ] {
        assert_eq!(messages(source), [], "{source}");
    }
}

// spec FTEF-9.17: a member's `0` is its token's text, a vector's being its last component's
#[test]
fn a_member_zero_is_its_token_text() {
    let refused = |value: &str| {
        messages(&format!("class C {{ float a = {value}; }};"))
            .iter()
            .any(|(code, _)| *code == Code::E0257)
    };
    for value in ["0", "0i"] {
        assert!(!refused(value), "{value}");
    }
    for value in ["0x0", "00", "0.0", "-0", "1"] {
        assert!(refused(value), "{value}");
    }
    let vector = |value: &str| messages(&format!("class C {{ vector v = {value}; }};"));
    assert_eq!(vector("'0 0 0'"), []);
    assert_eq!(vector("'1 0 0'"), []);
    assert_eq!(
        vector("'0 0 1'"),
        one(Code::E0257, "Can only initialise member functions")
    );
    assert_eq!(messages("class C { string s = 0; };"), []);
}

// spec FTEF-9.18: `Class::name` names the member through the class: a field, a static global or
// constant, or the member function
#[test]
fn a_scoped_member_name_is_the_member() {
    assert_eq!(
        messages("class C { float a; }; float x = C::a;"),
        one(Code::E0213, "Cannot cast from .float to float")
    );
    for member in ["static float a;", "static float a = 1;"] {
        assert_eq!(
            messages(&format!("class C {{ {member} }}; float x = C::a;")),
            one(Code::E0214, "initializer for 'x' is not constant"),
            "{member}"
        );
    }
    for source in [
        "class C { const float a = 1; }; float x = C::a;",
        "class C { nonvirtual void() m = {}; }; void() x = C::m;",
        "class C { virtual void() m = {}; }; void() x = C::m;",
        "class C { virtual void() m; }; void() x = C::m; void() C::m = {};",
        "class C { static float a; }; void() g = { local float x = C::a; C::a = 2; };",
    ] {
        assert_eq!(messages(source), [], "{source}");
    }
}

// spec FTEF-9.19: a vector data member has its components' fields, an array of vectors none
#[test]
fn a_vector_member_has_component_fields() {
    assert_eq!(
        messages("class C { vector v; }; C c; void() g = { c.v_x = 1; c.v_y = 2; };"),
        []
    );
    assert_eq!(
        messages("class C { vector v; }; C c; void() g = { c.v_w = 1; };"),
        one(Code::E0319, "Unknown field \"v_w\" in class \"C\"")
    );
    assert_eq!(
        messages("class C { vector v[2]; }; C c; void() g = { c.v_x = 1; };"),
        one(Code::E0319, "Unknown field \"v_x\" in class \"C\"")
    );
}

// spec FTEF-17.16: an inline member body sees only the members declared before it, and `this` is
// the entity as the class
#[test]
fn an_inline_body_sees_earlier_members() {
    assert_eq!(
        messages("class C { void() m = { n(); }; void() n = {}; };")
            .into_iter()
            .filter(|(code, _)| *code != Code::W0265)
            .collect::<Vec<_>>(),
        one(Code::E0301, "Unknown value \"n\".")
    );
    for (source, member) in [
        (
            "class C { nonvirtual void() m = { this.n(); }; nonvirtual void() n = {}; };",
            "n",
        ),
        (
            "class C { nonvirtual void() m = { this.a = 1; }; float a; };",
            "a",
        ),
    ] {
        assert_eq!(
            messages(source),
            one(
                Code::E0319,
                &format!("Unknown field \"{member}\" in class \"C\"")
            ),
            "{source}"
        );
    }
    for source in [
        "class C { float a; nonvirtual void() m = { this.a = 1; a = 2; }; };",
        "class C { nonvirtual void() m; float a; }; void() C::m = { a = 1; this.a = 2; };",
        "class C { nonvirtual void() m = { local entity e = this; local C c = this; }; };",
    ] {
        assert_eq!(messages(source), [], "{source}");
    }
    assert_eq!(
        messages("class C { nonvirtual void() m = { local float f = this; }; };"),
        one(Code::E0213, "Cannot cast from class C to float")
    );
}

// spec FTEF-31.6: an array of fields with no index is its head, whose address the id target
// refuses
#[test]
fn a_field_array_with_no_index_is_its_head() {
    let refused = one(
        Code::E0331,
        "Address-of operator is not supported in this form without extensions. Consider the use \
         of either '#pragma target fte' or '#pragma flag enable brokenarray'",
    );
    for source in [
        "class C { float a[2]; }; C c; void() g = { c.a = 1; };",
        "class C { float a[2]; }; C c; void() g = { local float f = c.a; };",
        ".float a[2]; entity c; void() g = { c.a = 1; };",
    ] {
        assert_eq!(messages(source), refused, "{source}");
    }
    assert_eq!(
        messages(".float a[2]; entity c; void() g = { c.a[0] = 1; local float f = c.a[1]; };"),
        []
    );
}
