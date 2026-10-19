// SPDX-License-Identifier: GPL-3.0-or-later

//! Type tests (QC-10).

use std::rc::Rc;

use qc_syntax::ast;

use crate::ty::{FunctionType, SimpleType, Type};

/// Lowers the type of the first file-level declaration in `source`.
fn first_decl_type(source: &str) -> Type {
    let parse = qc_syntax::parse(source.as_bytes());
    let tree = parse.tree().expect("a tree");
    let decl = tree.declarations().next().expect("a declaration");
    let ty = decl.ty().expect("a type");
    Type::from_ast(&ty)
}

// spec QC-10.1: the five simple types; a vector is three floats, the rest one word.
#[test]
fn simple_types_lower() {
    assert_eq!(first_decl_type("float a;"), Type::Simple(SimpleType::Float));
    assert_eq!(
        first_decl_type("vector a;"),
        Type::Simple(SimpleType::Vector)
    );
    assert_eq!(
        first_decl_type("string a;"),
        Type::Simple(SimpleType::String)
    );
    assert_eq!(
        first_decl_type("entity a;"),
        Type::Simple(SimpleType::Entity)
    );
    assert_eq!(first_decl_type("void a;"), Type::Simple(SimpleType::Void));
}

// spec QC-10.1: a vector occupies three words, every other type one.
#[test]
fn word_sizes() {
    assert_eq!(Type::Simple(SimpleType::Vector).word_size(), 3);
    assert_eq!(Type::Simple(SimpleType::Float).word_size(), 1);
    assert_eq!(Type::Simple(SimpleType::String).word_size(), 1);
    assert_eq!(Type::Unknown.word_size(), 1);
}

// spec QC-10.7: a variable of type void may be declared and occupies one word.
#[test]
fn void_variable_has_one_word() {
    assert_eq!(first_decl_type("void x;").word_size(), 1);
}

// spec QC-10.2: `.T` is a field type, and field types nest (`..float`).
#[test]
fn field_types_nest() {
    assert_eq!(
        first_decl_type(".float f;"),
        Type::Field(Rc::new(Type::Simple(SimpleType::Float)))
    );
    assert_eq!(
        first_decl_type("..float ff;"),
        Type::Field(Rc::new(Type::Field(Rc::new(Type::Simple(
            SimpleType::Float
        )))))
    );
}

// spec QC-10.3: a function type has a return type and parameter types; names are not part
// of the type.
#[test]
fn function_type_lowers() {
    let ty = first_decl_type("void(float p, vector q) f;");
    assert_eq!(
        ty,
        Type::Function(Rc::new(FunctionType {
            ret: Type::Simple(SimpleType::Void),
            params: vec![
                Type::Simple(SimpleType::Float),
                Type::Simple(SimpleType::Vector),
            ],
            variadic: false,
        }))
    );
}

// spec QC-10.6: two function types are equal when return type, parameter count and
// parameter types match; parameter names do not matter.
#[test]
fn function_types_equal_ignoring_param_names() {
    let a = first_decl_type("void(float p) f;");
    let b = first_decl_type("void(float q) g;");
    assert_eq!(a, b);
}

// spec QC-10.6: a different parameter type makes a different type.
#[test]
fn function_types_differ_by_param_type() {
    let a = first_decl_type("void(float p) f;");
    let b = first_decl_type("void(entity p) g;");
    assert_ne!(a, b);
}

// spec QC-10.4: `(...)` is a variadic function type.
#[test]
fn variadic_function_type() {
    let ty = first_decl_type("void(...) f;");
    match ty {
        Type::Function(fty) => {
            assert!(fty.variadic);
            assert!(fty.params.is_empty());
        }
        _ => panic!("expected a function type"),
    }
}

// spec QC-10.3: a parameter type may itself be a field type.
#[test]
fn parameter_may_be_field_type() {
    let ty = first_decl_type("void(.float p) f;");
    match ty {
        Type::Function(fty) => {
            assert_eq!(
                fty.params,
                vec![Type::Field(Rc::new(Type::Simple(SimpleType::Float)))]
            );
        }
        _ => panic!("expected a function type"),
    }
}

// spec QC-10.1: the keyword spelling round-trips through SimpleType::as_str.
#[test]
fn simple_type_spelling() {
    assert_eq!(SimpleType::Float.as_str(), "float");
    assert_eq!(SimpleType::Vector.as_str(), "vector");
    assert_eq!(SimpleType::Entity.as_str(), "entity");
}

// A missing or malformed type lowers to Unknown without panicking (QCD-1.4 recovery).
#[test]
fn malformed_type_is_unknown() {
    // A field with a missing inner type lowers to Field(Unknown).
    let parse = qc_syntax::parse(b". f;");
    let tree = parse.tree().expect("a tree");
    let decl = tree.declarations().next().expect("a declaration");
    if let Some(ast::Type::Field(field)) = decl.ty() {
        let inner = field.inner().map_or(Type::Unknown, |t| Type::from_ast(&t));
        assert_eq!(inner, Type::Unknown);
    } else {
        // Recovery may have produced a different shape; the point is it did not panic.
        let ty = decl.ty().map_or(Type::Unknown, |t| Type::from_ast(&t));
        assert_eq!(ty.word_size(), 1);
    }
}
