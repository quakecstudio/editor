// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the constant types and FTEQCC's implicit casts.

use std::rc::Rc;

use super::{ConstType, Literal, Source, refused, type_name};
use crate::ty::{FunctionType, SimpleType, Type};

fn simple(ty: SimpleType) -> Type {
    Type::Simple(ty)
}

fn named(name: &str) -> Type {
    Type::Named(Rc::from(name))
}

// spec FTEF-3.1: `QCC_PR_LexNumber` types a number by its fraction and suffix, a `d` being a
// digit of a hex number and `l` with `u` in either order a 64-bit unsigned
#[test]
fn numbers_are_typed_by_their_suffix() {
    for (text, ty) in [
        ("5", ConstType::Float),
        ("1.5", ConstType::Float),
        ("1.5f", ConstType::Float),
        ("1.5d", ConstType::Double),
        ("1d", ConstType::Double),
        ("0x1d", ConstType::Float),
        ("0x1.5d", ConstType::Double),
        ("5i", ConstType::Integer),
        ("5u", ConstType::Uint),
        ("5l", ConstType::Int64),
        ("5ul", ConstType::Uint64),
        ("5lu", ConstType::Uint64),
        ("5ll", ConstType::Int64),
        ("010", ConstType::Float),
        ("-5i", ConstType::Integer),
    ] {
        assert_eq!(Literal::of_fteqcc_number(text).ty, ty, "{text}");
    }
}

// spec FTEF-19.2: `QCC_SRef_IsNull` holds for an integer, unsigned or float 0, written in any
// base and with `-`, and for no double, 64-bit or nonzero value, nor `-0.0`
#[test]
fn null_is_an_integer_or_float_zero() {
    for (text, null) in [
        ("0", true),
        ("-0", true),
        ("0.0", true),
        ("0x0", true),
        ("0i", true),
        ("0u", true),
        ("00", true),
        ("0d", false),
        ("0l", false),
        ("-0.0", false),
        ("1", false),
        ("0.5", false),
        ("0x10", false),
    ] {
        assert_eq!(Literal::of_fteqcc_number(text).null, null, "{text}");
    }
}

fn literal(text: &str) -> Source<'static> {
    Source::Literal(Literal::of_fteqcc_number(text))
}

fn constant(ty: ConstType) -> Source<'static> {
    Source::Literal(Literal::of(ty))
}

// spec FTEF-19.2: a float casts to the numbers and to a vector, and to nothing else unless it
// is null
#[test]
fn a_float_casts_to_numbers_and_vectors() {
    assert!(!refused(literal("1"), &simple(SimpleType::Float)));
    assert!(!refused(literal("1"), &simple(SimpleType::Vector)));
    assert!(!refused(literal("1"), &named("int")));
    for to in [
        simple(SimpleType::String),
        simple(SimpleType::Entity),
        simple(SimpleType::Void),
        Type::Field(Rc::new(simple(SimpleType::Float))),
        Type::Function(Rc::new(FunctionType {
            ret: simple(SimpleType::Void),
            params: Vec::new(),
            variadic: false,
        })),
    ] {
        assert!(refused(literal("1"), &to), "{to:?}");
        assert!(!refused(literal("0"), &to), "{to:?}");
    }
}

// spec FTEF-19.2: an integer casts to a string, an entity, a function and a vector, a double
// and a 64-bit integer to no string, and a uint to no vector
#[test]
fn integers_cast_where_cast_tries_them() {
    for to in [
        simple(SimpleType::String),
        simple(SimpleType::Entity),
        simple(SimpleType::Vector),
        simple(SimpleType::Float),
    ] {
        assert!(!refused(literal("1i"), &to), "{to:?}");
    }
    assert!(refused(
        literal("1i"),
        &Type::Field(Rc::new(simple(SimpleType::Float)))
    ));
    assert!(refused(literal("1d"), &simple(SimpleType::String)));
    assert!(refused(literal("1d"), &simple(SimpleType::Vector)));
    assert!(refused(literal("0d"), &simple(SimpleType::Entity)));
    assert!(refused(literal("5l"), &simple(SimpleType::String)));
    assert!(refused(literal("5u"), &simple(SimpleType::Vector)));
    assert!(!refused(literal("5u"), &simple(SimpleType::String)));
}

// spec FTEF-19.2: a vector casts only to a vector, a string to the integers, strings and
// functions, and a type a program names is not checked
#[test]
fn vectors_strings_and_named_types() {
    assert!(!refused(
        constant(ConstType::Vector),
        &simple(SimpleType::Vector)
    ));
    for to in [
        simple(SimpleType::Float),
        simple(SimpleType::String),
        simple(SimpleType::Entity),
        named("int"),
    ] {
        assert!(refused(constant(ConstType::Vector), &to), "{to:?}");
    }
    assert!(!refused(constant(ConstType::String), &named("int")));
    assert!(refused(
        constant(ConstType::String),
        &simple(SimpleType::Entity)
    ));
    assert!(!refused(constant(ConstType::String), &named("Player")));
    assert!(!refused(constant(ConstType::Vector), &Type::Unknown));
}

// spec FTEF-19.2: a named definition casts as its type does, a field to another field type
// never, and to its own type always
#[test]
fn named_definitions_cast_by_their_type() {
    let field = Type::Field(Rc::new(simple(SimpleType::Float)));
    let other = Type::Field(Rc::new(simple(SimpleType::Vector)));
    assert!(!refused(Source::Typed(&field), &field));
    assert!(refused(Source::Typed(&field), &other));
    assert!(refused(Source::Typed(&field), &simple(SimpleType::Float)));
    let entity = simple(SimpleType::Entity);
    assert!(refused(Source::Typed(&entity), &simple(SimpleType::Float)));
    assert!(!refused(Source::Typed(&entity), &named("int")));
}

// spec FTEF-19.2: the message spells a type as FTEQCC's `TypeName` does
#[test]
fn type_names_are_fteqcc_spellings() {
    assert_eq!(type_name(&named("int")), "__int32");
    assert_eq!(
        type_name(&Type::Field(Rc::new(simple(SimpleType::Float)))),
        ".float"
    );
    let func = Type::Function(Rc::new(FunctionType {
        ret: simple(SimpleType::Void),
        params: vec![simple(SimpleType::Float)],
        variadic: true,
    }));
    assert_eq!(type_name(&func), "void(float, ...)");
    assert_eq!(
        type_name(&Type::Pointer(Rc::new(simple(SimpleType::Entity)))),
        "entity*"
    );
}
