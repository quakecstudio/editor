// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the constant types and FTEQCC's implicit casts.

use std::rc::Rc;

use super::{ConstType, Kind, Literal, Source};
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
    assert!(!literal("1").refused(&simple(SimpleType::Float)));
    assert!(!literal("1").refused(&simple(SimpleType::Vector)));
    assert!(!literal("1").refused(&named("int")));
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
        assert!(literal("1").refused(&to), "{to:?}");
        assert!(!literal("0").refused(&to), "{to:?}");
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
        assert!(!literal("1i").refused(&to), "{to:?}");
    }
    assert!(literal("1i").refused(&Type::Field(Rc::new(simple(SimpleType::Float)))));
    assert!(literal("1d").refused(&simple(SimpleType::String)));
    assert!(literal("1d").refused(&simple(SimpleType::Vector)));
    assert!(literal("0d").refused(&simple(SimpleType::Entity)));
    assert!(literal("5l").refused(&simple(SimpleType::String)));
    assert!(literal("5u").refused(&simple(SimpleType::Vector)));
    assert!(!literal("5u").refused(&simple(SimpleType::String)));
}

// spec FTEF-19.2: a vector casts only to a vector, a string to the integers, strings and
// functions, and a type a program names is not checked
#[test]
fn vectors_strings_and_named_types() {
    assert!(!constant(ConstType::Vector).refused(&simple(SimpleType::Vector)));
    for to in [
        simple(SimpleType::Float),
        simple(SimpleType::String),
        simple(SimpleType::Entity),
        named("int"),
    ] {
        assert!(constant(ConstType::Vector).refused(&to), "{to:?}");
    }
    assert!(!constant(ConstType::String).refused(&named("int")));
    assert!(constant(ConstType::String).refused(&simple(SimpleType::Entity)));
    assert!(!constant(ConstType::String).refused(&named("Player")));
    assert!(!constant(ConstType::Vector).refused(&Type::Unknown));
}

// spec FTEF-19.2: a named definition casts as its type does, a field to another field type
// never, and to its own type always
#[test]
fn named_definitions_cast_by_their_type() {
    let field = Type::Field(Rc::new(simple(SimpleType::Float)));
    let other = Type::Field(Rc::new(simple(SimpleType::Vector)));
    assert!(!Source::Typed(&field).refused(&field));
    assert!(Source::Typed(&field).refused(&other));
    assert!(Source::Typed(&field).refused(&simple(SimpleType::Float)));
    let entity = simple(SimpleType::Entity);
    assert!(Source::Typed(&entity).refused(&simple(SimpleType::Float)));
    assert!(!Source::Typed(&entity).refused(&named("int")));
}

// spec FTEF-19.2: C type words and registered type names are the type FTEF-2.8 makes, an 8-
// or 16-bit integer none the model follows
#[test]
fn c_words_name_their_kinds() {
    for (words, kind) in [
        ("int", Kind::Integer),
        ("__signed int", Kind::Integer),
        ("integer", Kind::Integer),
        ("unsigned", Kind::Uint),
        ("int unsigned", Kind::Uint),
        ("__long", Kind::Int64),
        ("__long __long", Kind::Int64),
        ("__long int", Kind::Integer),
        ("unsigned __long", Kind::Uint64),
        ("__long unsigned", Kind::Uint64),
        ("__double", Kind::Double),
        ("__long __double", Kind::Double),
        ("unsigned float", Kind::Float),
        ("__int32", Kind::Integer),
        ("__uint", Kind::Uint),
        ("__int64", Kind::Int64),
        ("__uint64", Kind::Uint64),
        ("__char", Kind::Opaque),
        ("__short", Kind::Opaque),
        ("_Bool", Kind::Opaque),
        ("int int", Kind::Opaque),
        ("long", Kind::Opaque),
        ("Player", Kind::Opaque),
    ] {
        assert_eq!(named(words).fteqcc_kind(), kind, "{words}");
    }
    assert_eq!(named("unsigned __long").fteqcc_name(), "__uint64");
    assert_eq!(named("__long __double").fteqcc_name(), "__double");
    assert_eq!(named("Player").fteqcc_name(), "Player");
    assert!(Kind::Double.is_numeric());
    assert!(!Kind::Vector.is_numeric());
}

// spec FTEF-19.2: a pointer casts to a pointer and a string, integers among themselves, and
// the casts that warn are those "acceptable but probably an error" and to a vector
#[test]
fn pointers_and_warnings() {
    let pointer = |ty: SimpleType| Type::Pointer(Rc::new(simple(ty)));
    let float_ptr = pointer(SimpleType::Float);
    assert!(!Source::Typed(&float_ptr).refused(&pointer(SimpleType::Vector)));
    assert!(!Source::Typed(&float_ptr).refused(&simple(SimpleType::String)));
    assert!(Source::Typed(&float_ptr).refused(&simple(SimpleType::Float)));
    assert!(Source::Typed(&float_ptr).refused(&simple(SimpleType::Entity)));
    let int = named("int");
    let entity = simple(SimpleType::Entity);
    let string = simple(SimpleType::String);
    assert!(!Source::Typed(&int).warns(&named("unsigned")));
    assert!(Source::Typed(&int).warns(&entity));
    assert!(Source::Typed(&entity).warns(&int));
    assert!(Source::Typed(&string).warns(&int));
    assert!(!Source::Typed(&string).warns(&string));
    assert!(!Source::Typed(&int).warns(&simple(SimpleType::Float)));
    assert!(literal("1").warns(&simple(SimpleType::Vector)));
    assert!(!literal("0").warns(&simple(SimpleType::Vector)));
    assert!(!literal("1").warns(&simple(SimpleType::String)));
    let f = |param: SimpleType| {
        Type::Function(Rc::new(FunctionType {
            ret: simple(SimpleType::Void),
            params: vec![simple(param)],
            variadic: false,
        }))
    };
    assert!(Source::Typed(&f(SimpleType::Float)).warns(&f(SimpleType::Vector)));
    assert!(!Source::Typed(&f(SimpleType::Float)).warns(&f(SimpleType::Float)));
}

// spec FTEF-17.12: two types are FTEQCC's same type part by part, a C word by its kind
#[test]
fn types_are_the_same_part_by_part() {
    let f = |ret: Type, params: Vec<Type>, variadic: bool| {
        Type::Function(Rc::new(FunctionType {
            ret,
            params,
            variadic,
        }))
    };
    let float = simple(SimpleType::Float);
    assert!(named("int").fteqcc_same(&named("integer")));
    assert!(!named("int").fteqcc_same(&float));
    assert!(named("Player").fteqcc_same(&float));
    assert!(Type::Pointer(Rc::new(float.clone())).fteqcc_same(&simple(SimpleType::Vector)));
    assert!(!Type::Field(Rc::new(float.clone())).fteqcc_same(&Type::Field(Rc::new(named("int")))));
    assert!(f(float.clone(), vec![named("int")], false).fteqcc_same(&f(
        float.clone(),
        vec![named("integer")],
        false
    )));
    assert!(!f(float.clone(), vec![], true).fteqcc_same(&f(float.clone(), vec![], false)));
    assert!(
        !f(float.clone(), vec![float.clone()], false).fteqcc_same(&f(float.clone(), vec![], false))
    );
    assert!(!f(float.clone(), vec![], false).fteqcc_same(&f(named("int"), vec![], false)));
}

// spec FTEF-19.2: the message spells a type as FTEQCC's `TypeName` does
#[test]
fn type_names_are_fteqcc_spellings() {
    assert_eq!(named("int").fteqcc_name(), "__int32");
    assert_eq!(
        Type::Field(Rc::new(simple(SimpleType::Float))).fteqcc_name(),
        ".float"
    );
    let func = Type::Function(Rc::new(FunctionType {
        ret: simple(SimpleType::Void),
        params: vec![simple(SimpleType::Float)],
        variadic: true,
    }));
    assert_eq!(func.fteqcc_name(), "void(float, ...)");
    assert_eq!(
        Type::Pointer(Rc::new(simple(SimpleType::Entity))).fteqcc_name(),
        "entity*"
    );
}
