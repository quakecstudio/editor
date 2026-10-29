// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolution and diagnostics tests (QC-11, QC-12, QC-13.6; QCD-3, QCD-4.1, QCD-7.3).

use std::rc::Rc;

use qc_diag::Code;
use qc_syntax::Dialect;

use super::{index_of, module_index_of};
use crate::lower::lower_file;
use crate::resolve::{ResolveOptions, resolve_module};
use crate::symbol::{DefId, DefKind};
use crate::ty::{FunctionType, ParamLabels, SimpleType, Type};

/// The codes of the diagnostics an index reports, in order.
fn codes(source: &str) -> Vec<Code> {
    index_of(source)
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.code)
        .collect()
}

// spec QC-11.1: a file is a sequence of declarations; each name becomes a definition.
#[test]
fn file_level_names_become_definitions() {
    let index = index_of("float a;\nvector b;\nstring c;");
    let names: Vec<&str> = index.defs.iter().map(|d| d.name.as_str()).collect();
    // The vector's components follow it (QC-11.7).
    assert_eq!(names, ["a", "b", "b_x", "b_y", "b_z", "c"]);
    assert!(index.defs.iter().all(|d| d.kind == DefKind::Global));
}

// spec QC-11.7: a vector global declares float globals `v_x`, `v_y` and `v_z`, which a body
// uses by name.
#[test]
fn vector_global_declares_its_components() {
    let index = index_of("vector v;\nvoid() f = { v_x = v_y + v_z; };");
    let float = Type::Simple(SimpleType::Float);
    for name in ["v_x", "v_y", "v_z"] {
        let def = index.defs.iter().find(|d| d.name == name).unwrap();
        assert_eq!(def.ty, float);
        assert_eq!(def.kind, DefKind::Global);
    }
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
}

// spec QC-11.7: a `.vector` field declares `.float` fields `f_x`, `f_y` and `f_z`.
#[test]
fn vector_field_declares_float_fields() {
    let index = index_of(".vector o;\nentity e;\nvoid() f = { e.o_z = 1; };");
    let def = index.defs.iter().find(|d| d.name == "o_z").unwrap();
    assert_eq!(def.kind, DefKind::Field);
    let float_field = Type::Field(Rc::new(Type::Simple(SimpleType::Float)));
    assert_eq!(def.ty, float_field);
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
}

// spec QC-11.7: a vector parameter and a vector local have components in the function's
// scope, which no other function sees (QC-12.4).
#[test]
fn vector_params_and_locals_declare_components() {
    let index = index_of(
        "void(vector org) f = { local vector d; d_x = org_y; };\n\
         void() g = { org_x = 1; };",
    );
    let unknown: Vec<&str> = index
        .diagnostics
        .iter()
        .filter(|d| d.diagnostic.code == Code::E0301)
        .map(|d| d.diagnostic.message.as_str())
        .collect();
    assert_eq!(unknown, ["Unknown value \"org_x\""]);
    let param_x = index.defs.iter().find(|d| d.name == "org_x").unwrap();
    assert_eq!(param_x.kind, DefKind::Param);
    let local_x = index.defs.iter().find(|d| d.name == "d_x").unwrap();
    assert_eq!(local_x.kind, DefKind::Local);
}

// spec QC-11.7: a component is an ordinary name, so a float of the same name declared before
// is the component, and one of another type is a type mismatch (QC-11.5).
#[test]
fn a_component_name_follows_redeclaration_rules() {
    assert!(codes("float v_x;\nvector v;").is_empty());
    assert_eq!(codes("string v_x;\nvector v;"), [Code::E0205]);
}

// spec QC-11.7: a vector local that reuses a vector global (QC-12.3) adds no components, and
// its components, the global's, raise no "local is the global" warning of their own.
#[test]
fn a_reused_vector_adds_no_components() {
    let index = index_of("vector v;\nvoid() f = { local vector v; v_x = 1; };");
    assert_eq!(index.defs.iter().filter(|d| d.name == "v_x").count(), 1);
    let warnings: Vec<Code> = index
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.code)
        .collect();
    assert_eq!(warnings, [Code::W0201]);
}

// spec QC-11.8: a global of function type is a function, not a variable.
#[test]
fn function_type_global_is_a_function() {
    let index = index_of("void() f = {};");
    assert_eq!(index.defs[0].kind, DefKind::Function);
}

// spec QC-11.9: a field declared at file level is a field definition.
#[test]
fn field_at_file_level_is_a_field() {
    let index = index_of(".float health;");
    assert_eq!(index.defs[0].kind, DefKind::Field);
}

// spec QC-11.5: a name may be declared repeatedly with the same type and refer to one
// definition (prototype then body).
#[test]
fn prototype_then_body_is_one_definition() {
    let index = index_of("void() f;\nvoid() f = {};\nvoid() f;");
    let fs: Vec<_> = index.defs.iter().filter(|d| d.name == "f").collect();
    assert_eq!(fs.len(), 1);
    assert!(fs[0].initialized);
    assert!(index.diagnostics.is_empty());
}

// spec QCD-3.5, QC-11.5: redeclaring a name with another type is a type mismatch.
#[test]
fn redeclaration_with_other_type_is_e0205() {
    assert_eq!(codes("float a;\nvector a;"), [Code::E0205]);
}

// spec QCD-3.6, QC-11.5: initialising a definition twice is "NAME redeclared".
#[test]
fn second_initialiser_is_e0206() {
    assert_eq!(codes("float a = 1;\nfloat a = 2;"), [Code::E0206]);
}

// spec QCD-3.11, QC-11.8: a global function never defined is reported after all files.
#[test]
fn undefined_function_is_e0211() {
    assert_eq!(codes("void() f;"), [Code::E0211]);
}

// spec QC-11.8: a function defined by a body is not reported as undefined.
#[test]
fn defined_function_is_not_reported() {
    assert!(index_of("void() f = {};").diagnostics.is_empty());
}

// spec QC-11.8: a function defined by a builtin number is defined (it has an initialiser).
#[test]
fn builtin_function_is_defined() {
    assert!(index_of("void() f = #1;").diagnostics.is_empty());
}

// spec QC-12.1, QC-12.2: two scopes; the definition list is in declaration order and a
// parameter or local of a function belongs to that function.
#[test]
fn params_and_locals_belong_to_their_function() {
    let index = index_of("void(float p) f = { local float l; };");
    let func = index.defs.iter().position(|d| d.name == "f").unwrap();
    let param = index.defs.iter().find(|d| d.name == "p").unwrap();
    let local = index.defs.iter().find(|d| d.name == "l").unwrap();
    assert_eq!(param.kind, DefKind::Param);
    assert_eq!(local.kind, DefKind::Local);
    assert_eq!(param.owner, Some(DefId(func)));
    assert_eq!(local.owner, Some(DefId(func)));
}

// spec QC-12.3, QCD-7.3: a local that names a global declared before it reuses the global
// and warns; it does not create a new definition.
#[test]
fn local_reuses_earlier_global() {
    let source = "float x;\nvoid() f = { local float x; x = 1; };";
    let index = index_of(source);
    // No new definition named x inside f: only the one global x.
    let xs: Vec<_> = index.defs.iter().filter(|d| d.name == "x").collect();
    assert_eq!(xs.len(), 1);
    assert_eq!(xs[0].kind, DefKind::Global);
    // The warning is raised.
    assert!(
        index
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == Code::W0201)
    );
}

// spec QC-12.3: a parameter that names an earlier global reuses it.
#[test]
fn parameter_reuses_earlier_global() {
    let index = index_of("float x;\nvoid(float x) f = { x = 1; };");
    let xs: Vec<_> = index.defs.iter().filter(|d| d.name == "x").collect();
    assert_eq!(xs.len(), 1);
    assert_eq!(xs[0].kind, DefKind::Global);
}

// spec QC-12.4: a local of one function is invisible to another; two functions may use the
// same local name independently.
#[test]
fn locals_are_private_to_their_function() {
    let source = "void() f = { local float t; t = 1; };\nvoid() g = { local float t; t = 2; };";
    let index = index_of(source);
    let ts: Vec<_> = index.defs.iter().filter(|d| d.name == "t").collect();
    assert_eq!(ts.len(), 2);
    assert!(ts.iter().all(|d| d.kind == DefKind::Local));
    // No diagnostics: each `t` resolves to its own function's local.
    assert!(index.diagnostics.is_empty());
}

// spec QC-12.5, QCD-4.1: a name with no definition in scope is "Unknown value".
#[test]
fn unknown_name_is_e0301() {
    let index = index_of("void() f = { zz = 1; };");
    assert!(
        index
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == Code::E0301)
    );
}

// spec QC-12.5: a function body sees only definitions made before it; a global declared
// after a function is not visible inside it.
#[test]
fn body_sees_only_earlier_definitions() {
    // `later` is declared after f, so f cannot see it.
    let index = index_of("void() f = { later = 1; };\nfloat later;");
    assert!(
        index
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == Code::E0301)
    );
}

// spec QC-12.5: a global declared before a function is visible in it.
#[test]
fn body_sees_earlier_global() {
    let index = index_of("float early;\nvoid() f = { early = 1; };");
    assert!(index.diagnostics.is_empty());
}

// spec QC-11.9, QCD-3.8: a local field or function is "Fields and functions must be
// global".
#[test]
fn local_field_is_e0208() {
    let index = index_of("void() f = { local .float g; };");
    assert!(
        index
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == Code::E0208)
    );
}

// spec QC-13.6, QC-12.6: a state header declares the next-think name on the spot as a global
// void() if new.
#[test]
fn next_think_name_is_declared_void_function() {
    let source = "$frame s1\nvoid() th1 = [$s1, th2] { };\nvoid() th2 = {};";
    let index = module_index_of(&[source]);
    let th2 = index.defs.iter().find(|d| d.name == "th2").unwrap();
    assert_eq!(th2.kind, DefKind::Function);
    assert_eq!(th2.owner, None);
    assert_eq!(
        th2.ty,
        Type::Function(Rc::new(FunctionType {
            ret: Type::Simple(SimpleType::Void),
            params: Vec::new(),
            variadic: false,
            labels: ParamLabels::default(),
        }))
    );
    assert_eq!(th2.location.span.start, source.find("th2").unwrap());
    // th2 is defined by its own body, so no "not defined" error.
    assert!(
        index
            .diagnostics
            .iter()
            .all(|d| d.diagnostic.code != Code::E0211)
    );
}

// spec QC-13.6, QC-11.8: a next-think name never defined is reported as undefined.
#[test]
fn undefined_next_think_is_reported() {
    let index = index_of("$frame s1\nvoid() th1 = [$s1, th3] { };");
    assert!(
        index
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == Code::E0211)
    );
}

// spec QC-13.6, QCD-3.5: a next-think name that exists with another type is a type
// mismatch.
#[test]
fn next_think_type_mismatch_is_e0205() {
    let index = index_of("float th3;\n$frame s1\nvoid() th1 = [$s1, th3] { };");
    assert!(
        index
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == Code::E0205)
    );
}

// spec QC-23.3: files compile in order as one program; a later file sees an earlier file's
// definitions.
#[test]
fn later_file_sees_earlier_file_definitions() {
    let defs = "float shared;";
    let user = "void() f = { shared = 1; };";
    let index = module_index_of(&[defs, user]);
    assert!(index.diagnostics.is_empty());
}

// spec QC-23.3: the reverse order fails, since the user file is compiled first.
#[test]
fn earlier_file_cannot_see_later_file_definitions() {
    let defs = "float shared;";
    let user = "void() f = { shared = 1; };";
    let index = module_index_of(&[user, defs]);
    assert!(
        index
            .diagnostics
            .iter()
            .any(|d| d.diagnostic.code == Code::E0301)
    );
}

// spec QC-11.7: a vector local whose component name is a global reuses that global without
// the warning of QCD-7.3, since the source never names the component; the others are new.
#[test]
fn a_vector_local_component_reuses_a_global_silently() {
    let index = index_of("float v_x;\nvoid() f = { local vector v; v_x = v_y; };\n");
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    let v_x: Vec<_> = index.defs.iter().filter(|d| d.name == "v_x").collect();
    assert_eq!(v_x.len(), 1);
    assert_eq!(v_x[0].kind, DefKind::Global);
    let v_y = index.defs.iter().find(|d| d.name == "v_y").unwrap();
    assert_eq!(v_y.kind, DefKind::Local);
}

// spec QC-12.5: every part of an `if`, `if`-`else`, `while` and `do`-`while` statement and a
// `return` value names values, resolved in source order.
#[test]
fn statement_parts_are_resolved() {
    assert_eq!(
        unknown_values(
            "float() f = {\n\tif (u1) u2 = 1; else u3 = 1;\n\twhile (u4) u5 = 2;\n\
             \tdo u6 = 3; while (u7);\n\tif (u8) return u9;\n\treturn;\n};\n"
        ),
        ["u1", "u2", "u3", "u4", "u5", "u6", "u7", "u8", "u9"]
            .map(|name| format!("Unknown value \"{name}\""))
    );
}

// spec QC-13.11: a parameter of function type writes its parameters' names over the body's
#[test]
fn a_function_parameter_renames_the_body_parameters() {
    assert_eq!(
        unknown_values("float r;\nvoid(float b, void(float q) cb) f = { r = b; r = q; cb(r); };\n"),
        ["Unknown value \"b\""]
    );
}

/// The messages of the "Unknown value" errors of a vanilla module of one file.
fn unknown_values(source: &str) -> Vec<String> {
    index_of(source)
        .diagnostics
        .iter()
        .filter(|d| d.diagnostic.code == Code::E0301)
        .map(|d| d.diagnostic.message.clone())
        .collect()
}

// spec QC-12.5: in vanilla the name after `.` is a name used as a value, so a field name with
// no definition is "Unknown value", even one FTEQCC would read as a vector member.
#[test]
fn a_vanilla_field_selector_is_a_value() {
    assert_eq!(
        unknown_values("entity e;\nvoid() f = { e.x = 1; };\n"),
        ["Unknown value \"x\""]
    );
}

// spec QC-12.5: in vanilla `autocvar` is an ordinary name, so every argument of a call of it
// is a value that must be defined.
#[test]
fn a_vanilla_autocvar_call_resolves_every_argument() {
    assert_eq!(
        unknown_values(
            "float(float a, float b) autocvar = #1;\nvoid() f = { autocvar(nope, 1); };\n"
        ),
        ["Unknown value \"nope\""]
    );
}

// spec QC-11.5: a redeclaration whose type is unknown, the type of a declaration the parser
// read only in part, is no type mismatch in vanilla, whichever of the two is unknown.
#[test]
fn an_unknown_type_is_no_vanilla_mismatch() {
    let parse = qc_syntax::parse_with(b"float x;\nfloat x;\n", Dialect::Vanilla);
    let lowered = lower_file(&parse, Dialect::Vanilla);
    for unknown in [0, 1] {
        let mut file = lowered.clone();
        file.decls[unknown].ty = Type::Unknown;
        let index = resolve_module(&[file], ResolveOptions::default());
        assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    }
    let mut file = lowered;
    file.decls[1].ty = Type::Simple(SimpleType::String);
    let index = resolve_module(&[file], ResolveOptions::default());
    let codes: Vec<_> = index
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.code)
        .collect();
    assert_eq!(codes, [Code::E0205]);
}

// spec QC-12.5: resolution follows the dialect it is asked for: under vanilla a scoped name
// lowered from FTEQCC source is looked up by its name alone, so `super::` excuses nothing.
#[test]
fn vanilla_resolution_reads_a_scoped_name_as_its_name() {
    let parse = qc_syntax::parse_with(b"void() f = { super::nothing(); };\n", Dialect::Fteqcc);
    let lowered = lower_file(&parse, Dialect::Fteqcc);
    let index = resolve_module(&[lowered], ResolveOptions::default());
    let messages: Vec<_> = index
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.message.as_str())
        .collect();
    assert_eq!(messages, ["Unknown value \"super::nothing\""]);
}
