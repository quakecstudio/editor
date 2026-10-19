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
