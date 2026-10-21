// SPDX-License-Identifier: GPL-3.0-or-later

//! Definitions as id's `PR_ParseDefs` and `PR_GetDef` and FTEQCC's `QCC_PR_ParseDefs` and
//! `QCC_PR_GetDef` make them: initialiser types and casts, constness, parameters, array sizes,
//! typedefs, enums and the redeclaration rules (QC-11, QC-13, FTEF-19). Each program here is
//! also a case of the definitions family of `cargo xtask oracles builtins`, which compares the
//! outcome with id's qcc and the pinned FTEQCC.

use std::path::PathBuf;

use qc_diag::Code;
use qc_syntax::Dialect;

use super::{ModuleSpec, fte_index_of, index_of, resolve};
use crate::db::Db;
use crate::expand::ModuleInput;
use crate::resolve::ModuleIndex;
use crate::symbol::FileId;

/// Every diagnostic of `index`, as its code and message.
fn diagnostics(index: &ModuleIndex) -> Vec<(Code, String)> {
    index
        .diagnostics
        .iter()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect()
}

/// The codes of `index`'s diagnostics, in order.
fn codes(index: &ModuleIndex) -> Vec<Code> {
    index
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.code)
        .collect()
}

/// The codes of the diagnostics of the vanilla program `source`.
fn vanilla(source: &str) -> Vec<Code> {
    codes(&index_of(source))
}

/// The codes of the diagnostics of the FTEQCC program `source`.
fn fte(source: &str) -> Vec<Code> {
    codes(&fte_index_of(source))
}

/// The diagnostics, as code and message, of the FTEQCC program `source`.
fn fte_messages(source: &str) -> Vec<(Code, String)> {
    diagnostics(&fte_index_of(source))
}

/// The one diagnostic of the FTEQCC program `source`.
fn fte_only(source: &str) -> (Code, String) {
    let all = fte_messages(source);
    assert_eq!(all.len(), 1, "{source:?}: {all:?}");
    all.into_iter().next().unwrap()
}

/// The codes of the diagnostics of a module of FTEQCC files, each a unit of its own.
fn fte_module(files: &[&str]) -> Vec<Code> {
    codes(&resolve(&ModuleSpec {
        files,
        dialect: Dialect::Fteqcc,
    }))
}

// spec QC-11.3: a literal of another type than the definition's, or any literal for an entity,
// void or field, is "wrong immediate type"; a frame macro is a float
// spec QCD-3.7: the error is E0207 "wrong immediate type for NAME", at the initialiser
#[test]
fn vanilla_literals_must_have_the_definitions_type() {
    for source in [
        "float x = \"a\";\n",
        "entity e = 1;\n",
        ".float fl = 1;\n",
        "string s = 1;\n",
        "vector v = 1;\n",
        "void vv = 1;\n",
        "float x = '1 2 3';\n",
        "$frame a\nstring x = $a;\n",
        "void() g = { local entity e = 1; };\n",
        "void() g = { local string s = 1; };\n",
    ] {
        assert_eq!(vanilla(source), [Code::E0207], "{source:?}");
    }
    for source in [
        "string s = \"\";\n",
        "float x = -1;\n",
        "vector v = '0 0 0';\n",
        "$frame a\nfloat x = $a;\n",
        "void() g = { local float x = 1; };\n",
    ] {
        assert_eq!(vanilla(source), [], "{source:?}");
    }
    let index = index_of("float x = \"a\";\n");
    let d = &index.diagnostics[0].diagnostic;
    assert_eq!(d.message, "wrong immediate type for x");
    assert_eq!(d.span.start, "float x = ".len());
}

// spec QC-13.5: a parameter whose global lies below the one before it is "bad parm order";
// a vector and its `_x` component share their word
// spec QCD-5.4: the error is E0404 "bad parm order", at the parameter
#[test]
fn parameters_below_the_one_before_are_bad_parm_order() {
    for source in [
        "float b;\nfloat a;\nvoid(float a, float b) g = {};\n",
        "float a;\nvoid(float b, float a) g = {};\n",
        "float a;\nvoid(vector b, float a) g = {};\n",
        "vector v;\nvoid(float v_y, float v_x) g = {};\n",
    ] {
        let found = vanilla(source);
        assert!(found.contains(&Code::E0404), "{source:?}: {found:?}");
    }
    for source in [
        "float a;\nfloat b;\nvoid(float a, float b) g = {};\n",
        "void(float a, float a) g = {};\n",
        "vector v;\nvoid(float v_x, vector v) g = {};\n",
        "vector v;\nvoid(vector v, float v_x) g = {};\n",
        "vector v;\nvoid(float v_x, float v_y) g = {};\n",
    ] {
        let found = vanilla(source);
        assert!(!found.contains(&Code::E0404), "{source:?}: {found:?}");
    }
    let index = index_of("float b;\nfloat a;\nvoid(float a, float b) g = {};\n");
    let bad = index
        .diagnostics
        .iter()
        .find(|d| d.diagnostic.code == Code::E0404)
        .unwrap();
    assert_eq!(bad.diagnostic.message, "bad parm order");
    assert_eq!(
        bad.diagnostic.span.start,
        "float b;\nfloat a;\nvoid(float a, float ".len()
    );
}

// spec QC-11.11: a vector's initialiser does not initialise its components under the vanilla
// profile
// spec FTEF-19.5: under FTEQCC the components have the vector's words
#[test]
fn a_vectors_components_and_its_initialiser() {
    assert_eq!(vanilla("vector v = '1 2 3';\nfloat v_x = 2;\n"), []);
    assert_eq!(vanilla("vector v = '1 2 3';\nfloat v_x = 1;\n"), []);
    assert_eq!(fte("vector v = '1 2 3';\nfloat v_x = 1;\n"), []);
    assert_eq!(fte("vector v = '1 2 3';\nfloat v_x = 2;\n"), [Code::E0206]);
    assert_eq!(fte("float x = 0x10;\nfloat x = 16;\n"), []);
}

// spec QC-11.12: a field component is of a type no `.float` written in the source is, in
// either order, while a `float` component is a `float`
#[test]
fn field_components_have_their_own_type() {
    assert_eq!(vanilla(".vector fv;\n.float fv_x;\n"), [Code::E0205]);
    assert_eq!(vanilla(".float fv_x;\n.vector fv;\n"), [Code::E0205]);
    assert_eq!(vanilla(".vector fv;\n.vector fv;\n"), []);
    assert_eq!(vanilla("vector v;\nfloat v_x;\n"), []);
    // FTEQCC compares types structurally.
    assert_eq!(fte(".vector fv;\n.float fv_x;\n"), []);
    assert_eq!(fte(".float fv_x;\n.vector fv;\n"), []);
}

// spec QC-11.13: a local, or the global it is, is initialised once
#[test]
fn locals_are_initialised_once() {
    for source in [
        "float x = 1;\nvoid() g = { local float x = 2; };\n",
        "float x;\nvoid() g = { local float x = 2; };\nvoid() h = { local float x = 3; };\n",
        "void() g = { local float y = 1; local float y = 2; };\n",
    ] {
        assert!(vanilla(source).contains(&Code::E0206), "{source:?}");
    }
    assert!(!vanilla("float x;\nvoid() g = { local float x = 1; };\n").contains(&Code::E0206));
    // FTEQCC keeps no plain local initialised; a `const` or `static` one it compares.
    assert_eq!(
        fte("void() g = { float x = 1; float x = 2; };\n"),
        [Code::W0207]
    );
    assert_eq!(
        fte("void() g = { static float x = 1; static float x = 2; };\n"),
        [Code::E0206]
    );
    assert_eq!(
        fte("void() g = { const float x = 1; const float x = 2; };\n"),
        [Code::W0207, Code::E0206]
    );
}

// spec FTEF-19.1: a parameter given twice, or as an earlier vector parameter's component, is
// E0215; a vector after its component's name is no error
// spec QCD-3.15: "Duplicate definition of NAME." at the parameter
#[test]
fn parameters_given_twice() {
    assert_eq!(
        fte_only("void(float a, float a) g = {};\n"),
        (Code::E0215, "Duplicate definition of a.".to_owned())
    );
    assert_eq!(fte("void(vector v, float v_x) g = {};\n"), [Code::E0215]);
    assert_eq!(fte("void(float v_x, vector v) g = {};\n"), []);
    assert_eq!(vanilla("void(float a, float a) g = {};\n"), []);
}

// spec FTEF-19.1: a local declared again is a warning, a static one again none, and a local
// that hides a global of its kind of type a warning
// spec QCD-7.32: W0207 "NAME duplicate definition ignored" or "nonstatic redeclaration of NAME
// ignored"
// spec QCD-7.33: W0208 "Local \"NAME\" hides global with same name and type"
#[test]
fn locals_declared_again_or_hiding_globals() {
    assert_eq!(
        fte_only("void() g = { local float x; local float x; };\n"),
        (Code::W0207, "x duplicate definition ignored".to_owned())
    );
    assert_eq!(
        fte_only("void() g = { static float x; local float x; };\n"),
        (
            Code::W0207,
            "nonstatic redeclaration of x ignored".to_owned()
        )
    );
    assert_eq!(fte("void() g = { static float x; static float x; };\n"), []);
    assert_eq!(
        fte_only("void(float a) g = { local float a; };\n"),
        (Code::W0207, "a duplicate definition ignored".to_owned())
    );
    assert_eq!(
        fte_only("float x = 1;\nvoid() g = { float x = 2; };\n"),
        (
            Code::W0208,
            "Local \"x\" hides global with same name and type".to_owned()
        )
    );
    assert_eq!(fte("float x = 1;\nvoid(float x) g = {};\n"), [Code::W0208]);
    assert_eq!(
        fte("float x = 1;\nvoid() g = { vector x = '0 0 0'; };\n"),
        []
    );
}

// spec FTEF-19.1: a parameter with no name of a function with a body is a warning; a lone
// `void` is no parameter
// spec QCD-7.48: W0222 "Parameter N of NAME is not named"
#[test]
fn parameters_with_no_name() {
    assert_eq!(
        fte_only("void(float) g = {};\n"),
        (Code::W0222, "Parameter 1 of g is not named".to_owned())
    );
    assert_eq!(
        fte_only("void(float a, float) g = {};\n"),
        (Code::W0222, "Parameter 2 of g is not named".to_owned())
    );
    assert_eq!(fte("void(void) g = {};\n"), []);
    assert_eq!(fte("void(float) g = #6;\n"), []);
}

// spec FTEF-19.1: in a function body `shared`, `nosave`, `inline` and the `__` words start no
// definition, and `var`, `noref`, `static`, `extern`, `const` and `typedef` do
#[test]
fn the_words_that_start_a_local_definition() {
    for word in ["shared", "nosave", "inline", "__inline"] {
        let source = format!("void() g = {{ {word} float x; }};\n");
        assert!(
            fte(&source).iter().any(|code| code.is_error()),
            "{source:?}"
        );
    }
    for word in ["var", "noref", "static", "extern", "const"] {
        let source = format!("void() g = {{ {word} float x; }};\n");
        assert_eq!(fte(&source), [], "{source:?}");
    }
    assert_eq!(fte("void() g = { typedef float t; t x; };\n"), []);
}

// spec FTEF-19.2: a literal is cast as `QCC_TryEvaluateCast` casts it, a null to any type
// spec QCD-3.13: E0213 "Cannot cast from TYPE to TYPE", at the value
// spec QCD-7.46: W0220 "Implicit cast from TYPE to TYPE"
#[test]
fn literal_initialisers_are_cast() {
    for (source, message) in [
        ("float x = \"a\";\n", "Cannot cast from string to float"),
        ("entity e = 1;\n", "Cannot cast from float to entity"),
        ("void() g = 5;\n", "Cannot cast from float to void()"),
        ("string s = 5l;\n", "Cannot cast from __int64 to string"),
        ("vector v = 5u;\n", "Cannot cast from __uint32 to vector"),
        ("entity e = 0d;\n", "Cannot cast from __double to entity"),
        (".float fl = 1i;\n", "Cannot cast from __int32 to .float"),
        ("int i = '1 2 3';\n", "Cannot cast from vector to __int32"),
    ] {
        assert_eq!(
            fte_messages(source)[0],
            (Code::E0213, message.to_owned()),
            "{source:?}"
        );
    }
    for source in [
        "entity e = 0;\n",
        "string s = -0;\n",
        "void() g = 0;\n",
        "float x = 1i;\n",
        "int i = 1;\n",
        "float x = 5l;\n",
    ] {
        assert_eq!(fte(source), [], "{source:?}");
    }
    for (source, message) in [
        ("vector v = 1;\n", "Implicit cast from float to vector"),
        ("string s = 1i;\n", "Implicit cast from __int32 to string"),
        ("entity e = 1i;\n", "Implicit cast from __int32 to entity"),
        ("void() g = \"a\";\n", "Implicit cast from string to void()"),
    ] {
        assert_eq!(
            fte_only(source),
            (Code::W0220, message.to_owned()),
            "{source:?}"
        );
    }
    let index = fte_index_of("float x = \"a\";\n");
    assert_eq!(
        index.diagnostics[0].diagnostic.span.start,
        "float x = ".len()
    );
}

// spec FTEF-19.2: a name is cast as its definition's type, and in a member function a field's
// name is the field's value
// spec QCD-3.32: E0232 "_() intrinsic accepts only a string immediate"
#[test]
fn named_initialisers_are_cast() {
    assert_eq!(fte("string y = \"a\";\nfloat x = y;\n"), [Code::E0213]);
    assert_eq!(fte("entity e;\nfloat x = e;\n"), [Code::E0213]);
    assert_eq!(fte(".float fl;\nfloat x = fl;\n"), [Code::E0213]);
    assert_eq!(fte(".float fl;\n.float g = fl;\n"), []);
    assert_eq!(
        fte("class C { void() m; };\n.vector origin;\nvoid() C::m = { vector v = origin; };\n"),
        []
    );
    assert_eq!(fte("string s = _(\"a\");\n"), []);
    assert_eq!(
        fte_only("string s = _(5);\n"),
        (
            Code::E0232,
            "_() intrinsic accepts only a string immediate".to_owned()
        )
    );
}

// spec FTE-23.16: a number a macro gives a function after `#` is FTEQCC's "Cannot cast"
#[test]
fn a_macro_number_for_a_function_cannot_be_cast() {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(
        FileId(0),
        PathBuf::from("t.qc"),
        b"#define N 5\nvoid() f = #N;\n".to_vec(),
    );
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FileId(0)],
        ..ModuleInput::default()
    });
    let found: Vec<String> = analysis
        .diagnostics()
        .map(|d| d.diagnostic.message.clone())
        .collect();
    // The function it would have been is left with no body.
    assert_eq!(
        found,
        [
            "Cannot cast from float to void()",
            "function f was not defined"
        ]
    );
}

// spec FTEF-19.3: a name initialising a global or a static is a constant; constness is the
// first declaration's
// spec QCD-3.14: E0214 "initializer for 'NAME' is not constant"
// spec QCD-7.37: W0212 on a redeclaration that would change the constness
// spec QCD-7.45: W0219 for a constant with no value yet, not for a field or a function
#[test]
fn constants_initialise_constants() {
    assert_eq!(
        fte_only("float y;\nfloat x = y;\n"),
        (
            Code::E0214,
            "initializer for 'x' is not constant".to_owned()
        )
    );
    assert_eq!(fte("var float y = 2;\nfloat x = y;\n"), [Code::E0214]);
    assert_eq!(
        fte("float y;\nfloat y = 2;\nfloat x = y;\n"),
        [Code::W0212, Code::E0214]
    );
    assert_eq!(fte("float y = 2;\nfloat x = y;\n"), []);
    assert_eq!(fte("float y = 2;\nvar float x = y;\n"), []);
    assert_eq!(
        fte("void() g = { local float y; local float x = y; };\n"),
        []
    );
    assert_eq!(
        fte("void() g = { local float y; const float x = y; };\n"),
        []
    );
    assert_eq!(
        fte("void() g = { static float y; static float x = y; };\n"),
        [Code::E0214]
    );
    assert_eq!(
        fte_only("const float y;\nfloat x = y;\n"),
        (
            Code::W0219,
            "initializer for x is not initialised yet, y will be treated as 0".to_owned()
        )
    );
    assert_eq!(fte("void() h;\nvoid() g = h;\nvoid() h = {};\n"), []);
    assert_eq!(
        fte_only("float y;\nfloat y = 2;\n"),
        (
            Code::W0212,
            "Redeclaration of y would change to const.".to_owned()
        )
    );
    assert_eq!(
        fte_only("enum { A };\nfloat A;\n"),
        (Code::W0212, "Redeclaration of A changes const.".to_owned())
    );
}

// spec FTEF-19.4: a body for a definition already initialised is E0216, but for anglemod and
// crossproduct, and wrap and accumulate their own rules
// spec QCD-3.16: E0216 "redeclaration of function body"
// spec QCD-3.22: E0222 "wrapper function does not wrap anything"
// spec QCD-3.23: E0223 "Wrapper function \"NAME\" does not refer to its prior function"
// spec QCD-7.42: W0217 "NAME already declared as a builtin"
// spec QCD-7.43: W0218 "NAME redeclared to accumulate after initial declaration"
#[test]
fn bodies_for_initialised_definitions() {
    assert_eq!(
        fte("void() g = {};\nvoid() g = {};\n"),
        [Code::E0216, Code::E0206]
    );
    assert_eq!(
        fte("void() g = #6;\nvoid() g = {};\n"),
        [Code::E0216, Code::E0206]
    );
    assert_eq!(
        fte("extern void() g;\nvoid() g = {};\n"),
        [Code::E0216, Code::E0206]
    );
    assert_eq!(
        fte_only("void() anglemod = #6;\nvoid() anglemod = {};\n"),
        (
            Code::W0217,
            "anglemod already declared as a builtin".to_owned()
        )
    );
    assert_eq!(
        fte_messages("__wrap void() g = {};\n")[0],
        (
            Code::E0222,
            "wrapper function does not wrap anything".to_owned()
        )
    );
    assert_eq!(
        fte_only("void() g = {};\n__wrap void() g = {};\n"),
        (
            Code::E0223,
            "Wrapper function \"g\" does not refer to its prior function".to_owned()
        )
    );
    assert_eq!(fte("__weak void() g = {};\nvoid() g = {};\n"), []);
    assert_eq!(
        fte("__accumulate void() g = {};\n__accumulate void() g = {};\n"),
        []
    );
    assert_eq!(
        fte_only("void() g = {};\n__accumulate void() g = {};\n"),
        (
            Code::W0218,
            "g redeclared to accumulate after initial declaration".to_owned()
        )
    );
}

// spec FTEF-19.5: a constant's name gives its value, and parseentitydata may be renumbered
// spec QCD-7.49: W0223 for parseentitydata given another builtin after #608 or #613
#[test]
fn values_compare_through_names() {
    assert_eq!(fte("float y = 2;\nfloat x = y;\nfloat x = 2;\n"), []);
    assert_eq!(
        fte("float y = 2;\nfloat x = y;\nfloat x = 3;\n"),
        [Code::E0206]
    );
    let (code, message) =
        fte_only("void() parseentitydata = #608;\nvoid() parseentitydata = #613;\n");
    assert_eq!(code, Code::W0223);
    assert_eq!(
        message,
        "incompatible redeclaration. Please validate builtin numbers. parseentitydata is #613"
    );
    assert_eq!(
        fte("void() parseentitydata = #609;\nvoid() parseentitydata = #613;\n"),
        [Code::E0206]
    );
}

// spec FTEF-19.6: a default value is a constant with a value, of the parameter's type, and two
// default values must agree
// spec QCD-3.17: E0217 "Default value not a constant"
// spec QCD-3.18: E0218 "Default value not initialized yet"
#[test]
fn default_values() {
    // The declaration ends at the error, leaving `g` with no body.
    assert_eq!(
        fte_messages("float y;\nvoid(float a = y) g = #6;\n")[0],
        (Code::E0217, "Default value not a constant".to_owned())
    );
    assert_eq!(
        fte_messages("const float y;\nvoid(float a = y) g = #6;\n")[0],
        (Code::E0218, "Default value not initialized yet".to_owned())
    );
    assert_eq!(fte("float y = 1;\nvoid(float a = y) g = #6;\n"), []);
    assert_eq!(
        fte("void(float a = \"s\") g = #6;\n"),
        [Code::E0213, Code::E0211]
    );
    assert_eq!(
        fte("void(float a = 1) g;\nvoid(float a = 2) g = #6;\n"),
        [Code::E0205, Code::E0211]
    );
    assert_eq!(fte("void(float a = 1) g;\nvoid(float a) g = #6;\n"), []);
    assert_eq!(fte("void(float a = 1) g;\nvoid(float a = 1) g = #6;\n"), []);
    assert_eq!(
        fte("float(float a) g;\nfloat(float a, float b = 1) g = #6;\n"),
        [Code::E0205, Code::E0211]
    );
}

// spec FTEF-19.7: a difference in optional arguments only is a warning, a misdeclared builtin
// keeps its first type, a noref variable is replaced, and `T(...)` is no difference
// spec QCD-7.34: W0209 "Optional arguments differ on redeclaration of NAME. A, should be B"
// spec QCD-7.35: W0210 "NAME builtin was wrongly redefined as TYPE. ignoring later definition"
// spec QCD-7.36: W0211 "Type redeclaration of TYPE NAME replaces existing variable"
#[test]
fn redeclarations_of_another_type() {
    assert_eq!(
        fte_only("float(float a, optional float b) g;\nfloat(float a) g = #6;\n"),
        (
            Code::W0209,
            "Optional arguments differ on redeclaration of g. float(float), should be \
             float(float, float)"
                .to_owned()
        )
    );
    assert_eq!(
        fte("float(float a) g;\nfloat(float a, optional float b) g = #6;\n"),
        [Code::W0209]
    );
    assert_eq!(fte("float(float a, ...) g;\nfloat(float a) g = #6;\n"), []);
    assert_eq!(fte("float(float a) g;\nfloat(...) g = #6;\n"), []);
    assert_eq!(
        fte("float(...) g;\nfloat(float a) g = #6;\n"),
        [Code::E0205, Code::E0211]
    );
    assert_eq!(
        fte("float(float a) g;\nfloat(float a, float b) g = #6;\n"),
        [Code::E0205, Code::E0211]
    );
    assert_eq!(
        fte_only("void() droptofloor = #6;\nfloat(float a) droptofloor;\n"),
        (
            Code::W0210,
            "droptofloor builtin was wrongly redefined as float(float). ignoring later \
             definition"
                .to_owned()
        )
    );
    // A builtin a body replaced is unused and unreferenced (FTEF-19.4).
    assert_eq!(
        fte_messages(
            "void() anglemod = #6;\nvoid() anglemod = {};\nfloat(float a) anglemod = #7;\n"
        )[1],
        (
            Code::W0211,
            "Type redeclaration of float(float) anglemod replaces existing variable".to_owned()
        )
    );
    // A `noref` mark is a reference too (`def->referenced = true;`).
    assert_eq!(fte("noref float x;\nvector x = '0 0 0';\n"), [Code::E0205]);
    assert_eq!(
        fte("noref float x;\nvoid() g = { x = 1; };\nvector x;\n"),
        [Code::E0205]
    );
    assert_eq!(
        fte_messages("float x;\nvector x;\n"),
        [(
            Code::E0205,
            "Type mismatch on redeclaration of x. vector, should be float".to_owned()
        )]
    );
}

// spec FTEF-19.7: a function first declared in a file named `*extensions.qc` keeps its type
// there
#[test]
fn an_extensions_file_keeps_the_first_declaration() {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(
        FileId(0),
        PathBuf::from("defs.qc"),
        b"void() spin = #6;\n".to_vec(),
    );
    db.set_file(
        FileId(1),
        PathBuf::from("fteextensions.qc"),
        b"float(float a) spin;\n".to_vec(),
    );
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FileId(0), FileId(1)],
        ..ModuleInput::default()
    });
    let found: Vec<(Code, String)> = analysis
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect();
    assert_eq!(
        found,
        [(
            Code::W0210,
            "spin builtin was redefined as float(float). ignoring alternative definition"
                .to_owned()
        )]
    );
}

// spec FTEF-19.8: a redeclaration's array size is the definition's, an unsized one takes it
// spec QCD-3.20: E0220 "Array sizes for redecleration of NAME do not match (A->B)"
#[test]
fn array_sizes_of_redeclarations() {
    assert_eq!(
        fte_only("float x[2];\nfloat x[3];\n"),
        (
            Code::E0220,
            "Array sizes for redecleration of x do not match (2->3)".to_owned()
        )
    );
    assert_eq!(fte("float x[2];\nfloat x;\n"), [Code::E0220]);
    assert_eq!(fte("float x;\nfloat x[2];\n"), [Code::E0220]);
    assert_eq!(fte("float x[2];\nfloat x[2];\n"), []);
    assert_eq!(fte("float x[];\nfloat x[2];\n"), []);
    assert_eq!(fte("float x[2];\nfloat x[];\n"), []);
    assert_eq!(fte("float x[] = {1,2};\nfloat x[2];\n"), [Code::W0212]);
    assert_eq!(fte("float x[] = {1,2};\nfloat x[3];\n"), [Code::E0220]);
    assert_eq!(
        fte_only("void() g = { float x[2]; float x[3]; };\n").1,
        "Array sizes for redecleration of x do not match (2 -> 3)"
    );
}

// spec FTEF-19.9: a typedef names its type again, a typedef's name is its type, a storage
// qualifier is an error and a typedef with no name a warning
// spec FTEF-17.12: a redeclaration reads a typedef's name as the type it names
// spec QCD-3.19: E0219 "Cannot redeclare typedef NAME from TYPE to TYPE"
// spec QCD-3.24: E0224 "bad combination of modifiers with typedef (defining NAME)"
// spec QCD-7.40: W0215 "typedef defines no types"
#[test]
fn typedefs() {
    assert_eq!(
        fte_only("typedef float t;\ntypedef vector t;\n"),
        (
            Code::E0219,
            "Cannot redeclare typedef t from float to vector".to_owned()
        )
    );
    assert_eq!(fte("typedef float t;\ntypedef int t;\n"), [Code::E0219]);
    assert_eq!(fte("typedef float t;\ntypedef float t;\n"), []);
    assert_eq!(fte("typedef float t;\ntypedef t t;\n"), []);
    assert_eq!(fte("typedef float t;\nt x;\nfloat x;\n"), []);
    assert_eq!(fte("typedef float t;\nt x;\nvector x;\n"), [Code::E0205]);
    assert_eq!(
        fte_only("typedef const float t;\n"),
        (
            Code::E0224,
            "bad combination of modifiers with typedef (defining t)".to_owned()
        )
    );
    for source in ["typedef;\n", "typedef float;\n"] {
        assert_eq!(
            fte_only(source),
            (Code::W0215, "typedef defines no types".to_owned()),
            "{source:?}"
        );
    }
}

// spec FTEF-19.10: an enum's values, under enumflags doubling, and their checks
// spec QCD-3.27: E0227 "Implicit type mismatch. Needed TYPE, got TYPE."
// spec QCD-3.28: E0228 "enum - NAME is not a compile-time constant"
// spec QCD-3.29: E0229 "values for enums of this type must be initialised"
// spec QCD-7.50: W0224 "legacy enum base type. ..."
// spec QCD-7.51: W0225 "enumflags - ..."
#[test]
fn enum_values() {
    assert_eq!(fte("enum { A, B };\nfloat A = 0;\n"), []);
    assert_eq!(fte("enum { A, B };\nfloat A = 1;\n"), [Code::E0206]);
    assert_eq!(
        fte("enumflags { A, B, C };\nfloat x = C;\nfloat x = 4;\n"),
        []
    );
    assert_eq!(
        fte("enumflags { A = 0, B };\nfloat x = B;\nfloat x = 1;\n"),
        []
    );
    assert_eq!(fte("enum { A = 5, B };\nfloat x = B;\nfloat x = 6;\n"), []);
    assert_eq!(
        fte("enum { A = 1.5, B };\nfloat x = B;\nfloat x = 2.5;\n"),
        []
    );
    assert_eq!(fte("float y = 2;\nenum { A = y };\n"), []);
    assert_eq!(
        fte_only("enum { A = \"s\" };\n"),
        (
            Code::E0227,
            "Implicit type mismatch. Needed float, got string.".to_owned()
        )
    );
    assert_eq!(
        fte_only("enum : string { A = 1 };\n").1,
        "Implicit type mismatch. Needed string, got float."
    );
    assert_eq!(
        fte_only("float y;\nenum { A = y };\n"),
        (
            Code::E0228,
            "enum - y is not a compile-time constant".to_owned()
        )
    );
    assert_eq!(fte("enum { A = y };\n"), [Code::E0301]);
    assert_eq!(
        fte_only("enum : vector { A };\n"),
        (
            Code::E0229,
            "values for enums of this type must be initialised".to_owned()
        )
    );
    assert_eq!(
        fte_only("enum float { A };\n"),
        (
            Code::W0224,
            "legacy enum base type. Use \"enum [class] [name_e]:type\" instead".to_owned()
        )
    );
    assert_eq!(fte_only("enum E { A };\nenum E { B };\n").0, Code::W0224);
    assert_eq!(
        fte_only("enumflags { A = 3 };\n"),
        (
            Code::W0225,
            "enumflags - 0x3(3) has multiple bits set".to_owned()
        )
    );
    assert_eq!(
        fte_only("enumflags { A = 1.5 };\n").1,
        "enumflags - 1.500000 not an integer value"
    );
    assert_eq!(fte("enumflags { A = 1, B = 4 };\n"), []);
    assert_eq!(
        fte_only("enum class E : float { A };\nfloat x = E::A;\n").1,
        "Cannot cast from enum E:float to float"
    );
    assert_eq!(
        fte("enum class E : float { A };\nfloat x = A;\n"),
        [Code::E0301]
    );
}

// spec FTEF-19.11: a shared definition is initialised by its declaration
// spec QCD-3.21: E0221 "shared values may not be assigned an initial value"
// spec QCD-7.39: W0214 for extern on what is no function and the other ignored qualifiers
#[test]
fn shared_and_extern() {
    assert_eq!(
        fte_only("shared float x = 1;\n"),
        (
            Code::E0221,
            "shared values may not be assigned an initial value".to_owned()
        )
    );
    assert_eq!(fte("shared float x;\nfloat x = 1;\n"), [Code::E0221]);
    assert_eq!(fte("shared float x;\nshared float x;\n"), []);
    assert_eq!(
        fte_only("extern float x;\n"),
        (
            Code::W0214,
            "extern keyword may only apply to qc functions.".to_owned()
        )
    );
    assert_eq!(fte("extern void() g;\n"), []);
}

// spec FTEF-19.12: a static global is visible in its unit only, and static on another file's
// name is an error
// spec QCD-3.31: E0231 "can't redefine non-static as static"
#[test]
fn statics_are_their_units() {
    assert_eq!(
        fte_module(&["static float w;\n", "void() g = { w = 1; };\n"]),
        [Code::E0301]
    );
    assert_eq!(
        fte_module(&[
            "static float w;\n",
            "static float w;\nvoid() g = { w = 1; };\n"
        ]),
        []
    );
    assert_eq!(
        fte_module(&["static float w;\nvoid() g = { w = 1; };\n"]),
        []
    );
    assert_eq!(
        fte_module(&["float w;\n", "static float w;\n"]),
        [Code::E0231]
    );
    assert_eq!(fte("float x;\nstatic float x;\n"), []);
}

// spec FTEF-19.13: a use of a deprecated definition is a warning, with its reason
// spec QCD-7.44: W0314 "Variable \"NAME\" is deprecated"
#[test]
fn deprecated_definitions() {
    assert_eq!(
        fte_only("__deprecated float x;\nvoid() g = { x = 1; };\n"),
        (Code::W0314, "Variable \"x\" is deprecated".to_owned())
    );
    assert_eq!(
        fte_only("__deprecated(\"old\") float x;\nvoid() g = { x = 1; };\n").1,
        "Variable \"x\" is deprecated: old"
    );
}

// spec FTEF-19.14: a state header's frame is read before its next function is declared, and
// its literals are typed
// spec QCD-4.9: E0309 "Type mismatch: TYPE, should be void()"
#[test]
fn state_headers() {
    assert_eq!(
        fte("void() g = [h, h] {};\nvoid() h = {};\n"),
        [Code::E0301]
    );
    assert_eq!(
        fte("void() h;\nvoid() g = [1, h] {};\nvoid() h = {};\n"),
        []
    );
    assert_eq!(fte("void() g = [1, 0] {};\n"), []);
    assert_eq!(
        fte_only("void() g = [1, \"s\"] {};\n"),
        (
            Code::E0309,
            "Type mismatch: string, should be void()".to_owned()
        )
    );
    assert_eq!(
        fte_only("void() g = [\"s\", h] {};\nvoid() h = {};\n").1,
        "Implicit type mismatch. Needed float, got string."
    );
    // Under the vanilla profile the frame is a literal and the name is declared after it too.
    assert_eq!(vanilla("void() g = [1, h] {};\nvoid() h = {};\n"), []);
}

// spec FTEF-19.15: `Scope::name` of a class member the class lacks is a warning, of a struct an
// error, and of what is neither an error
// spec QCD-3.25: E0225 "NAME is not a class"
// spec QCD-3.26: E0226 "NAME is not part of class CLASS"
// spec QCD-7.41: W0216 "NAME is not part of class CLASS"
#[test]
fn scoped_definitions() {
    assert_eq!(
        fte_only("class C { float a; };\nvoid() C::b = {};\n"),
        (Code::W0216, "C::b is not part of class C".to_owned())
    );
    assert_eq!(fte("class C { void() a; };\nvoid() C::a = {};\n"), []);
    assert_eq!(
        fte_only("struct S { float a; };\nfloat S::a;\n"),
        (Code::E0226, "S::a is not part of class S".to_owned())
    );
    assert_eq!(
        fte("struct S { float a; };\nfloat S::a;\nfloat S::a;\n"),
        [Code::E0226]
    );
    for source in [
        "float D::b;\n",
        "enum E { A };\nfloat E::B;\n",
        "typedef float T;\nfloat T::b;\n",
    ] {
        assert_eq!(fte(source), [Code::E0225], "{source:?}");
    }
    assert_eq!(fte_only("float D::b;\n").1, "D is not a class");
}

// spec FTEF-19.17: an initialiser that is an expression is compared by its text and not typed,
// an `extern` function is no error whatever the target, and an attribute before the type is
// refused, as the pinned FTEQCC refuses it
#[test]
fn what_the_profile_does_not_model() {
    assert_eq!(fte("float x = 1 + 1;\nfloat x = 2;\n"), [Code::E0206]);
    assert_eq!(fte("float x = 1 + 1;\nfloat x = 1 + 1;\n"), []);
    assert_eq!(fte("string s = 1 + 1;\n"), []);
    assert_eq!(fte("extern void() g;\n"), []);
    let parse = qc_syntax::parse_with(b"[[alias(\"y\")]] float x;\n", Dialect::Fteqcc);
    let parsed: Vec<Code> = parse.diagnostics().iter().map(|d| d.code).collect();
    assert_eq!(parsed, [Code::E0203]);
}

// spec FTEF-19.16: autocvars, keyword names, `accumulate` on what is no function and the
// qualifiers FTEQCC ignores
// spec QCD-4.10: E0310 "Cannot use 'this' outside of an OO function"
// spec QCD-4.11: E0311 "Cannot use 'super' outside of an OO function"
// spec QCD-4.12: E0312 "class NAME has no super"
// spec QCD-3.30: E0230 "accumulate applies only to functions, not NAME"
// spec QCD-7.38: W0213 "Autocvar ..."
// spec QCD-7.47: W0221 "\"NAME\" keyword used as variable name - coexisting"
#[test]
fn intrinsics_autocvars_keywords_and_qualifiers() {
    let messages = |source: &str| -> Vec<String> {
        fte_messages(source).into_iter().map(|(_, m)| m).collect()
    };
    assert_eq!(
        messages("void() g = { local entity e; e = this; e = super; };\n"),
        [
            "Cannot use 'this' outside of an OO function",
            "Cannot use 'super' outside of an OO function"
        ]
    );
    assert_eq!(
        fte_only("void() g = { local float autocvar_x; };\nvoid() h = { autocvar_x = 1; };\n"),
        (
            Code::W0213,
            "Autocvar \"autocvar_x\" defined with local scope. promoting to global.".to_owned()
        )
    );
    assert_eq!(
        fte_only("const float autocvar_x = 1;\n").1,
        "Autocvar \"autocvar_x\" defined as constant. attempting to correct that for you."
    );
    assert_eq!(
        fte_only("float var = 1;\n"),
        (
            Code::W0221,
            "\"var\" keyword used as variable name - coexisting".to_owned()
        )
    );
    assert_eq!(fte("void() break = {};\n"), []);
    assert_eq!(
        fte_messages("__accumulate float x;\n"),
        [
            (Code::W0214, "function modifier on non-function.".to_owned()),
            (
                Code::E0230,
                "accumulate applies only to functions, not x".to_owned()
            )
        ]
    );
    assert_eq!(
        fte_only("const var float x = 1;\n").1,
        "Conflicting constness."
    );
    assert_eq!(
        fte_only("inline float x = 1;\n").1,
        "function modifier on non-function."
    );
    assert_eq!(
        messages("extern static void() g;\n"),
        ["Conflicting storage classes."]
    );
}
