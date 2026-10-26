// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolution under the FTEQCC profile where FTEQCC differs from vanilla
//! (`docs/spec/dialect-fteqcc-rerelease.md`, FTE-15.4, FTE-17, FTE-18, FTE-19, FTE-21 and
//! FTE-22), each checked against the vanilla resolution of the same source.

use qc_diag::Code;
use qc_syntax::Dialect;

use super::{fte_index_of, index_of};
use crate::db::Db;
use crate::resolve::ModuleIndex;
use crate::symbol::{DefKind, FileId};
use crate::ty::{SimpleType, Type};

/// The error codes of an index, warnings left out.
fn error_codes(index: &ModuleIndex) -> Vec<Code> {
    index
        .diagnostics
        .iter()
        .filter(|d| d.diagnostic.code.is_error())
        .map(|d| d.diagnostic.code)
        .collect()
}

/// The codes of every diagnostic of an index.
fn all_codes(index: &ModuleIndex) -> Vec<Code> {
    index
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.code)
        .collect()
}

/// Resolves several files under the FTEQCC dialect, with or without autoprototyping.
fn fte_module(files: &[&str], autoproto: bool) -> ModuleIndex {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_autoproto(autoproto);
    let ids: Vec<FileId> = files
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let id = FileId(index);
            db.set_file_text(id, text.as_bytes().to_vec());
            id
        })
        .collect();
    db.module_index(&ids)
}

// spec FTE-6.1: both sides of a compound assignment are value uses, so an unknown name in
// either is reported.
#[test]
fn compound_assignment_operands_are_uses() {
    let index = fte_index_of("float a;\nvoid() f = { a += nope; missing |= a; };\n");
    let messages: Vec<&str> = index
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.message.as_str())
        .collect();
    assert_eq!(
        messages,
        ["Unknown value \"nope\"", "Unknown value \"missing\""]
    );
    assert_eq!(index.refs.len(), 2);
}

// spec FTE-17.1: a C-style definition and a vanilla prototype of the same function are one
// definition of one type, which is then defined.
#[test]
fn c_style_definitions_match_vanilla_prototypes() {
    let index = fte_index_of(
        "void() InitLightStyles;\nvoid() worldspawn = { InitLightStyles(); };\n\
         void InitLightStyles() { };\n\
         float(float a, string s) g;\nfloat g(float x, string t) = { return x; };\n",
    );
    assert_eq!(all_codes(&index), Vec::<Code>::new());
    let lights: Vec<_> = index
        .defs
        .iter()
        .filter(|d| d.name == "InitLightStyles")
        .collect();
    assert_eq!(lights.len(), 1);
    assert!(lights[0].initialized);
    assert_eq!(lights[0].kind, DefKind::Function);
    // A C-style declarator of another type is a type mismatch, as in vanilla (QC-11.5), and
    // leaves the prototype undefined (QC-11.8), which a call makes an error (FTEF-17.11).
    let mismatch = fte_index_of("void() h;\nvoid h(float x) { };\nvoid() k = { h(); };\n");
    assert_eq!(error_codes(&mismatch), [Code::E0205, Code::E0211]);
}

// spec FTE-17.7: a body written without `=` defines the function and binds its parameters.
#[test]
fn bodies_without_equals_define_functions() {
    let index = fte_index_of("float out;\nvoid set(float v) { out = v; }\nvoid() g { set(1); }\n");
    assert_eq!(all_codes(&index), Vec::<Code>::new());
    let v = index.defs.iter().find(|d| d.name == "v").unwrap();
    assert_eq!(v.kind, DefKind::Param);
}

// spec FTE-17.4: a function bound to a builtin by name is defined, so it is not reported as
// undefined (QC-11.8).
#[test]
fn named_builtins_define_functions() {
    let index = fte_index_of("void(string s, ...) bprint = #0:ex_bprint;\n");
    assert_eq!(all_codes(&index), Vec::<Code>::new());
    assert!(index.defs[0].initialized);
}

// spec FTE-18.1: a function type with `...` after named parameters is variadic and keeps the
// named ones.
#[test]
fn varargs_after_named_parameters_keep_them() {
    let index = fte_index_of("void(string s, ...) sprint = #0:ex_sprint;\n");
    let Type::Function(func) = &index.defs[0].ty else {
        panic!("not a function type");
    };
    assert!(func.variadic);
    assert_eq!(func.params, [Type::Simple(SimpleType::String)]);
}

// spec FTE-18.2: a parameter with a default value is a parameter like any other.
#[test]
fn defaulted_parameters_are_parameters() {
    let index = fte_index_of("float out;\nvoid(float k = 0) f = { out = k; };\n");
    assert_eq!(all_codes(&index), Vec::<Code>::new());
    assert!(
        index
            .defs
            .iter()
            .any(|d| d.name == "k" && d.kind == DefKind::Param)
    );
}

// spec FTE-18.3: a parameter of a named enum's type resolves in the body.
// spec FTE-15.4: a named enum declares its type name and its constants.
#[test]
fn named_enum_types_and_parameters_resolve() {
    let index = fte_index_of(
        "enum UpgradeType { UT_Health, UT_Shells };\n\
         float GetUpgradeAmount(UpgradeType Type) { if (Type == UT_Health) return 15; return 0; };\n",
    );
    assert_eq!(all_codes(&index), Vec::<Code>::new());
    assert!(index.defs.iter().any(|d| d.name == "UpgradeType"));
    assert!(index.defs.iter().any(|d| d.name == "UT_Shells"));
    assert!(
        index
            .defs
            .iter()
            .any(|d| d.name == "Type" && d.kind == DefKind::Param)
    );
}

// spec FTE-19.4: the operand of a prefix operator is a value use.
// spec FTE-19.6: the operand of a cast is a value use.
#[test]
fn prefix_and_cast_operands_are_uses() {
    let index = fte_index_of("float a;\nvoid() f = { a = -nope; a = (float) gone; a = ~a; };\n");
    let messages: Vec<&str> = index
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.message.as_str())
        .collect();
    assert_eq!(
        messages,
        ["Unknown value \"nope\"", "Unknown value \"gone\""]
    );
}

// spec FTE-19.5: both operands of `a -1` are resolved.
#[test]
fn negative_number_subtraction_resolves_its_operand() {
    let index = fte_index_of("float a;\nvoid() f = { a = nope -1; };\n");
    let messages: Vec<&str> = index
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.message.as_str())
        .collect();
    assert_eq!(messages, ["Unknown value \"nope\""]);
}

// spec FTE-21.1: a local declared without `local`, mid-body, resolves.
// spec FTE-21.2: a function-variable local is not "Fields and functions must be global".
#[test]
fn bare_and_function_locals_resolve() {
    let index = fte_index_of(
        "void() touch = { };\nfloat a;\n\
         void() f = {\n\ta = 1;\n\tfloat x = a;\n\tvoid() t;\n\tt = touch;\n\tt();\n\ta = x;\n};\n",
    );
    assert_eq!(all_codes(&index), Vec::<Code>::new());
    assert!(
        index
            .defs
            .iter()
            .any(|d| d.name == "t" && d.kind == DefKind::Local)
    );
    let vanilla = index_of("void() f = { local void() t; };");
    assert_eq!(error_codes(&vanilla), [Code::E0208]);
}

// spec FTE-22.1: a parameter or local shadows a global of the same name, whatever its type,
// and a use in the body is the function's own.
#[test]
fn locals_shadow_globals() {
    let source = "void() light = { };\n\
                  void(vector org, float light) launch = { if (light) org_x = 0; };\n\
                  .string target;\nvoid(entity target) use = { target.target = \"\"; };\n";
    let index = fte_index_of(source);
    assert_eq!(all_codes(&index), Vec::<Code>::new());
    let param = index
        .defs
        .iter()
        .position(|d| d.name == "light" && d.kind == DefKind::Param)
        .unwrap();
    let use_of_light = index
        .refs
        .iter()
        .find(|r| index.defs[r.def.0].name == "light")
        .unwrap();
    assert_eq!(use_of_light.def.0, param);
    // Vanilla reuses the global, so another type is a type mismatch (QC-12.3).
    assert_eq!(error_codes(&index_of(source)), [Code::E0205, Code::E0205]);
    // A second declaration in one function is still that definition, or a mismatch.
    let twice = fte_index_of("void() f = { local float x; local string x; };\n");
    assert_eq!(error_codes(&twice), [Code::E0205]);
    let same = fte_index_of("void() f = { local float x; local float x; x = 1; };\n");
    assert_eq!(error_codes(&same), Vec::<Code>::new());
}

// spec FTE-22.5: a constant initialised twice with the same literal is accepted; a different
// literal is still "NAME redeclared".
// spec FTEF-17.10: so is an initialiser written differently, and a body is never equal.
#[test]
fn equal_reinitialisation_is_accepted() {
    let index = fte_index_of(
        "float SIGIL_E1 = 1;\nfloat SIGIL_E1 = 1.0;\nstring S = \"a\";\nstring S = \"a\";\n\
         vector V = '1 2 3';\nvector V = '1 2 3';\n",
    );
    assert_eq!(all_codes(&index), Vec::<Code>::new());
    assert_eq!(
        error_codes(&fte_index_of("float A = 1;\nfloat A = 2;\n")),
        [Code::E0206]
    );
    // FTEF-19.5: a constant's name gives its value.
    assert_eq!(
        error_codes(&fte_index_of("float A = 1;\nfloat B = 1;\nfloat A = B;\n")),
        Vec::<Code>::new()
    );
    assert_eq!(
        error_codes(&fte_index_of("float A = 1;\nfloat B = 2;\nfloat A = B;\n")),
        [Code::E0206]
    );
    assert_eq!(
        error_codes(&fte_index_of("void() f = { };\nvoid() f = { };\n")),
        [Code::E0216, Code::E0206]
    );
    assert_eq!(
        error_codes(&index_of("float A = 1;\nfloat A = 1;")),
        [Code::E0206]
    );
}

// spec FTE-22.3: with autoprototyping a body sees every top-level declaration of the module,
// later files included; without it a later declaration is unknown, as in vanilla.
#[test]
fn autoprototyping_sees_later_declarations() {
    let files = [
        "float out;\nvoid() f = { out = CORPSE_FLAG_A; OrbCheckAttack(); };\n",
        "const float CORPSE_FLAG_A = 1;\nvoid() OrbCheckAttack = { };\n",
    ];
    let index = fte_module(&files, true);
    assert_eq!(all_codes(&index), Vec::<Code>::new());
    let without = fte_module(&files, false);
    assert_eq!(error_codes(&without), [Code::E0301, Code::E0301]);
    // Autoprototyping changes only the module index: switching it re-resolves.
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    for (index, text) in files.iter().enumerate() {
        db.set_file_text(FileId(index), text.as_bytes().to_vec());
    }
    let ids = [FileId(0), FileId(1)];
    assert_eq!(error_codes(&db.module_index(&ids)).len(), 2);
    db.set_autoproto(true);
    assert!(error_codes(&db.module_index(&ids)).is_empty());
    let counts = db.counts();
    assert_eq!(counts.parse, 2);
    assert_eq!(counts.module_index, 2);
}

// spec FTE-14.1: a typedef's type name is a definition, and so is a named enum's (FTE-15.4).
#[test]
fn type_names_are_definitions() {
    let index = fte_index_of(
        "typedef struct { string mdl; } poseInfo;\nenum UpgradeType { UT_A };\n\
         void() f = { poseInfo p; UpgradeType u; float poseInfo; };\n",
    );
    let globals: Vec<&str> = index
        .defs
        .iter()
        .filter(|d| d.kind.is_global())
        .map(|d| d.name.as_str())
        .collect();
    assert_eq!(globals, ["poseInfo", "UpgradeType", "UT_A", "f"]);
    // A local of a type the model does not know is never a type mismatch, and a local named
    // like the type shadows it (FTE-22.1).
    assert_eq!(all_codes(&index), Vec::<Code>::new());
    // FTEF-19.20: a name no type has is "not a type", and the local is not made.
    let unknown = fte_index_of("void() f = { poseInfo p; float p; };\n");
    assert_eq!(all_codes(&unknown), [Code::E0203]);
}

// spec FTE-21.1: the names a local's initialiser uses are resolved, a value or an aggregate.
#[test]
fn local_initialisers_are_uses() {
    let index = fte_index_of(
        "float a;\nvoid() f = {\n\tfloat x = nope;\n\tvector v = { a, gone, 0 };\n};\n",
    );
    let messages: Vec<&str> = index
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.message.as_str())
        .collect();
    assert_eq!(
        messages,
        ["Unknown value \"nope\"", "Unknown value \"gone\""]
    );
}

// spec QC-11.5: a local declared again in its function with the same type is that local
// again, with no warning; QCD-7.3 is for a global only.
#[test]
fn a_local_declared_twice_is_one_local() {
    let index = index_of("void() f = { local float x; local float x; x = 1; };");
    assert_eq!(all_codes(&index), Vec::<Code>::new());
    assert_eq!(index.defs.iter().filter(|d| d.name == "x").count(), 1);
}

// spec PRJ-2.8: the database reports the dialect it was set to.
#[test]
fn the_database_reports_its_dialect() {
    let mut db = Db::new();
    assert_eq!(db.dialect(), Dialect::Vanilla);
    db.set_dialect(Dialect::Fteqcc);
    assert_eq!(db.dialect(), Dialect::Fteqcc);
}

// spec QC-11.7: under FTEQCC too, a vector local has components, and they shadow a global
// of the same name (FTE-22.1).
#[test]
fn shadowing_vectors_have_components() {
    let index = fte_index_of("float org_x;\nvoid(vector org) f = { org_x = 1; };\n");
    assert_eq!(all_codes(&index), Vec::<Code>::new());
    let used = index
        .refs
        .iter()
        .find(|r| index.defs[r.def.0].name == "org_x")
        .unwrap();
    assert_eq!(index.defs[used.def.0].kind, DefKind::Param);
}
