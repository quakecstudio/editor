// SPDX-License-Identifier: GPL-3.0-or-later

//! The terms FTEQCC reads before an operator: the prefix operators, truth tests, `?:`, casts,
//! the vector constructor, `sizeof` and `_length` (`docs/spec/dialect-fteqcc.md` FTEF-30). Each
//! case's diagnostics are the pinned FTEQCC's on the standard target (2026-10-27); a warning its
//! settings make an error is `E0125` (F313).

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

const FILE: FileId = FileId(0);

fn diags(source: &str) -> Vec<(Code, String)> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FILE, PathBuf::from("t.qc"), source.as_bytes().to_vec());
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FILE],
        command_line: CommandLine::new::<&str>(&[]),
        ..ModuleInput::default()
    });
    analysis
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect()
}

fn body(statement: &str) -> Vec<(Code, String)> {
    diags(&format!(
        "const float k = 3; entity world;\nvoid() g = {{ local float a; local vector v; \
         local string s; local entity e; local int i; {statement} }};\n"
    ))
}

fn d(code: Code, message: &str) -> (Code, String) {
    (code, message.to_owned())
}

// spec FTEF-30.1: `!` and `~` take the types their opcodes do; `~` of an entity is the fatal
// error "Type mismatch: ~TYPE"
// spec QCD-4.34: E0328 for a type `!` or `~` does not take
#[test]
fn logical_and_bitwise_not() {
    for quiet in [
        "a = !a;", "a = !e;", "a = !s;", "a = !v;", "a = ~s;", "a = !i;",
    ] {
        assert_eq!(body(quiet), [], "{quiet}");
    }
    assert_eq!(body("a = ~e;"), [d(Code::E0328, "Type mismatch: ~entity")]);
}

// spec FTEF-30.1: a void value's test is the fatal error, and under `lax` a warning
// spec QCD-7.90: W0248 for a test of a void value under lax
#[test]
fn a_void_test() {
    let source = |lax: &str| format!("{lax}void() g = {{ local float a; a = !(void)a; }};\n");
    assert_eq!(diags(&source("")), [d(Code::E0328, "Type mismatch: !void")]);
    assert_eq!(
        diags(&source("#pragma flag enable lax\n")),
        [d(Code::W0248, "Type mismatch: !void")]
    );
}

// spec FTEF-30.4: `-` and `+` take numbers and vectors; any other type is "type mismatch for -"
// or "+", a fatal error
// spec QCD-4.2: E0302 "type mismatch for -" for a prefix operator
#[test]
fn negation_and_plus() {
    for quiet in ["v = -v;", "a = -(-3);", "a = +a;"] {
        assert_eq!(body(quiet), [], "{quiet}");
    }
    assert_eq!(body("a = -s;"), [d(Code::E0302, "type mismatch for -")]);
    assert_eq!(body("a = -e;"), [d(Code::E0302, "type mismatch for -")]);
    assert_eq!(body("a = +s;"), [d(Code::E0302, "type mismatch for +")]);
    // `0 - i` on the standard target needs `SubInt`, then the temporary is converted.
    assert_eq!(
        body("a = -i;")[..2],
        [
            d(
                Code::W0235,
                "SubInt function not defined: cannot emulate int-int"
            ),
            d(
                Code::E0320,
                "Opcode \"-|SUB_I\" not valid for target. Consider the use of: #pragma target fte"
            ),
        ]
    );
}

// spec FTEF-27.2: `~` of an integer with no `SubInt` refuses `OP_SUB_I`, the opcode it became
#[test]
fn bitwise_not_of_an_integer() {
    assert_eq!(
        body("a = ~i;")[..2],
        [
            d(
                Code::W0235,
                "SubInt function not defined: cannot emulate ~int"
            ),
            d(
                Code::E0320,
                "Opcode \"-|SUB_I\" not valid for target. Consider the use of: #pragma target fte"
            ),
        ]
    );
}

// spec FTEF-30.4: `++` and `--` of a read-only value or a type with no `+` are fatal errors;
// the value is the operand stored to
// spec QCD-4.35: E0329 for a prefix `++` or `--` refused
#[test]
fn prefix_steps() {
    assert_eq!(body("a = ++a;"), []);
    assert_eq!(
        body("a = --k;"),
        [d(Code::E0329, "-- operator on read-only value")]
    );
    assert_eq!(
        body("a = ++s;"),
        [d(Code::E0329, "++ operator on unsupported type")]
    );
    assert_eq!(
        body("a = --i;"),
        [
            d(
                Code::W0235,
                "AddInt function not defined: cannot emulate int+int"
            ),
            d(
                Code::E0320,
                "Opcode \"+|ADD_I\" not valid for target. Consider the use of: #pragma target fte"
            ),
            d(Code::E0125, "itof emulation: denormals are unsafe i"),
        ]
    );
}

// spec FTEF-30.5: `*` of anything but a pointer, a string or a function is a fatal error
// spec QCD-4.36: E0330 for a dereference refused
#[test]
fn dereference() {
    assert_eq!(
        body("e = *e;"),
        [d(Code::E0330, "Unable to dereference non-pointer type.")]
    );
    let source = "void() g = { local float *p; *p = 1; };\n";
    assert_eq!(diags(source), []);
}

// spec FTEF-30.6: a written cast refuses only what no cast takes, and an entity and a float
// convert either way
#[test]
fn written_casts() {
    assert_eq!(
        body("a = (float)s;"),
        [d(Code::E0213, "Cannot cast from string to float")]
    );
    assert_eq!(
        body("a = (float)v;"),
        [d(Code::E0213, "Cannot cast from vector to float")]
    );
    assert_eq!(
        body("e = (entity)a;"),
        [d(Code::E0125, "ftoi emulation: denormals are unsafe")]
    );
    assert_eq!(
        body("a = (float)i;"),
        [d(Code::E0125, "itof emulation: denormals are unsafe i")]
    );
}

// spec FTEF-30.7: a vector constructor casts each component to a float, and an integer one is
// stored with `OP_STORE_IF`
#[test]
fn vector_constructors() {
    assert_eq!(body("v = [1, 2, 3];"), []);
    assert_eq!(
        body("v = [a, s, 1];"),
        [d(Code::E0213, "Cannot cast from string to float")]
    );
    assert_eq!(
        body("v = [a, e, 1];"),
        [d(Code::E0213, "Cannot cast from entity to float")]
    );
    assert_eq!(
        body("v = [i, a, 1];"),
        [d(Code::E0125, "itof emulation: denormals are unsafe i")]
    );
}

// spec FTEF-30.3: arms of types `typecmp_lax` tells apart warn, and an integer arm is made a
// float on the standard target
// spec QCD-7.86: W0244 for a type mismatch on `?:`
#[test]
fn ternaries() {
    for quiet in [
        "a = s ? 1 : 2;",
        "a = v ? 1 : 2;",
        "a = a ?: 2;",
        "e = a ? world : e;",
    ] {
        assert_eq!(body(quiet), [], "{quiet}");
    }
    assert_eq!(
        body("a = a ? s : 2;"),
        [
            d(
                Code::W0244,
                "Type mismatch on ternary operator: string vs float"
            ),
            d(Code::E0213, "Cannot cast from string to float"),
        ]
    );
    assert_eq!(
        body("a = a ? i : 2;"),
        [d(Code::E0125, "itof emulation: denormals are unsafe i")]
    );
}

// spec FTEF-30.2: a condition that is a string warns under F322, off by default
// spec QCD-7.87: W0245 for a string condition
#[test]
fn a_string_condition() {
    assert_eq!(body("a = s ? 1 : 2;"), []);
    let warned = diags(
        "#pragma warning enable F322\nvoid() g = { local float a; local string s; \
         a = s ? 1 : 2; };\n",
    );
    assert_eq!(
        warned,
        [d(Code::W0245, "if (string) tests for null, not empty")]
    );
}

// spec FTEF-30.2: with `ifvector` off a vector condition warns that only its first element is
// tested
// spec QCD-7.88: W0246 for a vector condition without ifvector
#[test]
fn a_vector_condition() {
    let source = "#pragma flag disable ifvector\nvoid() g = { local float a; local vector v; \
                  a = v ? 1 : 2; };\n";
    assert_eq!(
        diags(source),
        [d(
            Code::W0246,
            "if (vector) tests only the first element with the current compiler flags"
        )]
    );
}

// spec FTEF-30.8: `sizeof` of a type or value is a constant; `_length` of a non-constant string
// or a float is a fatal error
// spec QCD-4.38: E0334 for `_length` refused
#[test]
fn sizes_and_lengths() {
    for quiet in [
        "a = sizeof(float);",
        "a = sizeof(v);",
        "a = _length(\"ab\");",
    ] {
        assert_eq!(body(quiet), [], "{quiet}");
    }
    assert_eq!(
        body("a = _length(s);"),
        [d(
            Code::E0334,
            "_length(string) requires an initialised constant"
        )]
    );
    assert_eq!(
        body("a = _length(a);"),
        [d(
            Code::E0334,
            "_length() unsupported argument type for intrinsic"
        )]
    );
}

// spec FTEF-30.8: under `undefwordsize` `sizeof` is an error
// spec QCD-4.32: E0326 for `sizeof` under undefwordsize
#[test]
fn sizeof_under_undefwordsize() {
    let source = "#pragma flag enable undefwordsize\nvoid() g = { local float a; \
                  a = sizeof(float); };\n";
    assert_eq!(
        diags(source),
        [d(
            Code::E0326,
            "sizeof was disabled, use array.length instead."
        )]
    );
}

// spec FTEF-30.3: two function types `typecmp_lax` tells apart only warn; two classes it takes
// as one, but `typecmp` does not, are the fatal error
// spec QCD-4.37: E0332 for arms of mismatching types
#[test]
fn ternary_type_agreement() {
    let functions = "void() f1 = {}; void(float) f2 = {};\n\
                     void() g = { local float a; local void() h; h = a ? f1 : f2; };\n";
    assert_eq!(
        diags(functions),
        [
            d(Code::W0222, "Parameter 1 of f2 is not named"),
            d(
                Code::W0244,
                "Type mismatch on ternary operator: void() vs void(float)"
            ),
        ]
    );
    let classes = "class A {}; class B {};\nvoid() g = { local float a; local A x; local B y; \
                   local entity e; e = a ? x : y; };\n";
    assert_eq!(
        diags(classes),
        [d(Code::E0332, "Ternary operator with mismatching types")]
    );
}

// spec FTEF-30.2: a condition of a type with no test, `void`, warns "conditional type mismatch"
// spec QCD-7.89: W0247 for a condition of no testable type
#[test]
fn a_void_condition() {
    assert_eq!(
        diags("void() g = { local float a; a = (void)a ? 1 : 2; };\n"),
        [d(Code::W0247, "conditional type mismatch: void")]
    );
}

// spec FTEF-6.13: a struct's members are laid out bit by bit, each aligned to its type, a
// bitfield only where it would cross its alignment, a union's all at 0, the tail padded to the
// largest alignment; `sizeof` is the bits over 8 when they are no whole words, else 4 a word
#[test]
fn struct_layouts() {
    for (st, size) in [
        ("struct s {float a; vector b;}", 16),
        ("struct s {__int8 a; __int8 b; float c;}", 8),
        ("struct s {__int8 a; __int16 b;}", 4),
        ("struct s {int a:3; int b:5;}", 4),
        ("struct s {int a:30; int b:5;}", 8),
        ("union s {float a; vector b;}", 12),
        ("struct s {float a[4]; __int8 c[3];}", 20),
        ("struct s {int a:40;}", 8),
        ("struct s {__int16 a; int b:20;}", 8),
        ("struct s {__int8 a; int b:20;}", 4),
        ("struct s {__int8 a[3];}", 3),
    ] {
        let source =
            format!("{st}; float arr[1]; void() g = {{ local float n; n = arr[sizeof(s)]; }};\n");
        let message = format!("(constant) array index out of bounds (0 <= {size} < 1)");
        assert_eq!(diags(&source), [d(Code::W0249, &message)], "{st}");
    }
}

// spec FTEF-6.12: a struct is cast to and from nothing but itself, in a body as in an
// initialiser: an operand, a value stored and a written cast
// spec FTEF-30.2: a struct's test is "conditional type mismatch"
#[test]
fn struct_values_in_bodies() {
    let source = |statement: &str| {
        format!("float a; struct st {{float x, y;}}; st t;\nvoid() g = {{ {statement} }};\n")
    };
    let cannot = d(Code::E0213, "Cannot cast from struct st to float");
    assert_eq!(
        diags(&source("a = t + a;")),
        [
            d(Code::E0302, "type mismatch for + (struct st and float)"),
            cannot.clone()
        ]
    );
    assert_eq!(diags(&source("a = t;")), std::slice::from_ref(&cannot));
    assert_eq!(
        diags(&source("a = (float)t;")),
        std::slice::from_ref(&cannot)
    );
    assert_eq!(
        diags(&source("t = 0;")),
        [d(Code::E0213, "Cannot cast from float to struct st")]
    );
    assert_eq!(diags(&source("local st u = t; u = t;")), []);
    for test in ["a = (t ? 1 : 2);", "if (t) a = 1;", "while (t) a = 1;"] {
        assert_eq!(
            diags(&source(test)),
            [d(Code::W0247, "conditional type mismatch: struct")],
            "{test}"
        );
    }
}

// spec FTEF-31.13: under qccx `&` of a float is a float read through it as an address
#[test]
fn a_qccx_address_of_a_float_is_read_through() {
    let found: Vec<String> =
        diags("#pragma flag enable qccx\nfloat f; string x_s; void() g = { x_s = &f; };\n")
            .into_iter()
            .map(|(_, message)| message)
            .collect();
    assert_eq!(
        found,
        [
            "memgetval function not defined: cannot emulate OP_LOADP_*",
            "Opcode \"=|LOADP_F\" not valid for target. Consider the use of: #pragma target fte",
            "Implicit cast from float to string",
        ]
    );
}

// spec FTEF-30.3: arms of function types compare by their return types, parameters, counts and
// variadic marks, parameters of a class or entity by their names; two structs `typecmp`
// tells apart and `typecmp_lax` does not end FTEQCC, and two classes of the same members too
// spec QCD-7.86: W0244 for a type mismatch on `?:`
#[test]
fn ternaries_of_functions_structs_and_classes() {
    let defs = "class C : entity { float m; }; class D : entity { float m; }; struct S { float a; };\n\
                struct T { float a; }; float(float a) f1; float(string a) f2;\n\
                float(float a, float b) f3; float(float b) f4; float(...) f5; string(float a) f6;\n\
                void(entity e) g1; void(C e) g2; void(D e) g3; S s1; T t1; float c; entity e;\n\
                C cc; D dd;\n";
    let found = |statement: &str| -> Vec<(Code, String)> {
        diags(&format!("{defs}void() f = {{ {statement} }};\n"))
            .into_iter()
            .filter(|(_, m)| !m.ends_with("has no body"))
            .collect()
    };
    let mismatch = |a: &str, b: &str| {
        vec![d(
            Code::W0244,
            &format!("Type mismatch on ternary operator: {a} vs {b}"),
        )]
    };
    assert_eq!(
        found("local float(float) r = c ? f1 : f2;"),
        mismatch("float(float a)", "float(string a)")
    );
    assert_eq!(
        found("local float(float) r = c ? f1 : f3;"),
        mismatch("float(float a)", "float(float a, float b)")
    );
    assert!(found("local float(float) r = c ? f1 : f4;").is_empty());
    assert_eq!(
        found("local float(float) r = c ? f1 : f5;"),
        mismatch("float(float a)", "float(...)")
    );
    assert_eq!(
        found("local float(float) r = c ? f1 : f6;"),
        mismatch("float(float a)", "string(float a)")
    );
    assert_eq!(
        found("local void(entity) r = c ? g1 : g2;"),
        mismatch("void(entity e)", "void(class C e)")
    );
    assert_eq!(
        found("local void(C) r = c ? g2 : g3;"),
        mismatch("void(class C e)", "void(class D e)")
    );
    let fatal = vec![d(Code::E0332, "Ternary operator with mismatching types")];
    assert_eq!(found("local S r = c ? s1 : t1;"), fatal);
    assert_eq!(found("local C r = c ? cc : dd;"), fatal);
    assert_eq!(
        found("local entity r = c ? cc : e;"),
        mismatch("class C", "entity")
    );
}
