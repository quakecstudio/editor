// SPDX-License-Identifier: GPL-3.0-or-later

//! Adversarial tests from the P2-R3 readiness review (TEST-9), written by a reviewer who did
//! not write the semantic model.

use std::path::PathBuf;
use std::rc::Rc;

use super::{fte_index_of, index_of, module_index_of, nth_offset};
use crate::db::Db;
use crate::symbol::FileId;
use crate::ty::{SimpleType, Type};

// spec QC-12.5: go-to-definition covers a use's bytes `[start, end)`: the first and last byte
// of the name hit, the byte before and the byte after do not
#[test]
fn reviewer_goto_definition_covers_exactly_the_name() {
    let source = "float health;\nvoid() f = { health = 1; };";
    let index = index_of(source);
    let start = nth_offset(source, "health", 1);
    let end = start + "health".len();
    let decl = nth_offset(source, "health", 0);
    for offset in [start, end - 1] {
        let (_, location) = index.definition_at(FileId(0), offset).unwrap();
        assert_eq!(location.span.start, decl, "{offset}");
    }
    assert!(index.definition_at(FileId(0), start - 1).is_none());
    assert!(index.definition_at(FileId(0), end).is_none());
}

// spec QC-12.5: a position is looked up in its own file only: the same offset in another
// file of the module does not hit a use or a definition of the first
#[test]
fn reviewer_goto_definition_is_per_file() {
    let first = "float health;\nvoid() f = { health = 1; };";
    let second = "void() g = { };";
    let index = module_index_of(&[first, second]);
    let offset = nth_offset(first, "health", 1);
    assert!(index.definition_at(FileId(0), offset).is_some());
    assert!(index.definition_at(FileId(1), offset).is_none());
    assert!(index.references_at(FileId(1), offset, true).is_empty());
}

// spec QC-10.6: function types compare structurally with parameter names excluded, also
// when a parameter is itself a function, so a prototype and a body naming them differently
// declare one function
#[test]
fn reviewer_nested_function_types_compare_without_names() {
    let source = "void(void(float x) cb, .float fl) run;\n\
                  void(void(float y) other, .float g) run = { };";
    let index = index_of(source);
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    let run: Vec<_> = index.defs.iter().filter(|d| d.name == "run").collect();
    assert_eq!(run.len(), 1);
    let Type::Function(func) = &run[0].ty else {
        panic!("run is not a function: {:?}", run[0].ty);
    };
    assert!(matches!(&func.params[0], Type::Function(inner)
        if inner.params == [Type::Simple(SimpleType::Float)]));
    assert_eq!(
        func.params[1],
        Type::Field(Rc::new(Type::Simple(SimpleType::Float)))
    );
}

// spec FTEF-17.3: the type a value names goes through one pointer to a named type, and no
// other type names one
#[test]
fn reviewer_type_name_goes_through_a_pointer() {
    let named = Type::Named(Rc::from("Thing"));
    assert_eq!(named.type_name(), Some("Thing"));
    assert_eq!(Type::Pointer(Rc::new(named)).type_name(), Some("Thing"));
    for ty in [
        Type::Unknown,
        Type::Simple(SimpleType::Entity),
        Type::Field(Rc::new(Type::Named(Rc::from("Thing")))),
        Type::Pointer(Rc::new(Type::Simple(SimpleType::Float))),
    ] {
        assert_eq!(ty.type_name(), None, "{ty:?}");
    }
}

// (P2-R3 review) a file put in the Db with its path keeps it, and one given only text has none
#[test]
fn reviewer_file_paths_are_kept() {
    let mut db = Db::new();
    db.set_file(
        FileId(0),
        PathBuf::from("src/defs.qc"),
        b"float a;".to_vec(),
    );
    db.set_file_text(FileId(1), b"float b;".to_vec());
    assert_eq!(
        db.file_path(FileId(0)),
        Some(PathBuf::from("src/defs.qc").as_path())
    );
    assert_eq!(db.file_path(FileId(1)), None);
    assert_eq!(db.file_text(FileId(1)), b"float b;");
}

// spec FTEF-17.1: under the FTEQCC profile a member function's body finds the class's
// members before a global of the same name
#[test]
fn reviewer_member_before_global() {
    let source = "float hp;\nclass c { float hp; void() hurt = { hp = 1; }; };";
    let index = fte_index_of(source);
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    let use_at = nth_offset(source, "hp", 2);
    let (def, _) = index.definition_at(FileId(0), use_at).unwrap();
    assert!(index.def(def).unwrap().member_of.is_some());
}

/// `void(void(...void(float p0) p1...) p{depth-1}) run`, the innermost parameter typed `inner`
/// and every parameter named with `prefix`.
fn nested_prototype(depth: usize, prefix: &str, inner: &str) -> String {
    let mut ty = format!("void({inner} {prefix}0)");
    for level in 1..depth {
        ty = format!("void({ty} {prefix}{level})");
    }
    format!("{ty} run")
}

// spec QC-10.6: function types nested 200 deep compare by their parameter types at every
// level, names excluded: a prototype and a body naming every parameter differently are one
// function, and a body whose innermost parameter has another type is a mismatch
#[test]
fn reviewer_deeply_nested_function_types_compare_at_every_level() {
    let depth = 200;
    let same = format!(
        "{};\n{} = {{ }};",
        nested_prototype(depth, "a", "float"),
        nested_prototype(depth, "b", "float")
    );
    let index = index_of(&same);
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    assert_eq!(index.defs.iter().filter(|d| d.name == "run").count(), 1);
    let other = format!(
        "{};\n{} = {{ }};",
        nested_prototype(depth, "a", "float"),
        nested_prototype(depth, "b", "vector")
    );
    assert!(!index_of(&other).diagnostics.is_empty());
}

// spec QC-27.16: the FTEQCC forms nested past the limit that the parser stops, an aggregate,
// an initialiser list, a `?:` in its middle and an `=` chain, each 100000 deep, resolve through
// the semantic model with every diagnostic inside the file
#[test]
fn reviewer_deep_fteqcc_trees_resolve() {
    let n = 100_000;
    let in_body = |body: String| format!("float a;\nvoid() f = {{\n{body}\n}};\n");
    for source in [
        format!("{}float x;{};\n", "struct { ".repeat(n), "} s;".repeat(n)),
        format!("float x[1] = {}1{};\n", "{".repeat(n), "}".repeat(n)),
        in_body(format!("a = {}1{};", "a ? ".repeat(n), " : 1".repeat(n))),
        in_body(format!("{}1;", "a = ".repeat(n))),
    ] {
        let index = fte_index_of(&source);
        for d in &index.diagnostics {
            assert_eq!(d.file, FileId(0));
            assert!(d.diagnostic.span.end <= source.len(), "{d:?}");
        }
    }
}

// spec QC-12.1: a function with 2000 locals resolves each use to its own local, with no
// diagnostic, under both profiles
#[test]
fn reviewer_many_locals_each_resolve_to_their_own() {
    let n = 2000;
    let mut body = String::new();
    for i in 0..n {
        body.push_str(&format!("local float l{i};\n"));
    }
    for i in 0..n {
        body.push_str(&format!("l{i} = {i};\n"));
    }
    let source = format!("void() f = {{\n{body}}};\n");
    for index in [index_of(&source), fte_index_of(&source)] {
        assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
        for i in [0, 1000, n - 1] {
            let name = format!("l{i}");
            let use_at = nth_offset(&source, &format!("{name} ="), 0);
            let (def, _) = index.definition_at(FileId(0), use_at).unwrap();
            assert_eq!(index.def(def).unwrap().name, name);
        }
    }
}
