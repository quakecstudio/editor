// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the constant types and FTEQCC's implicit casts.

use std::rc::Rc;

use qc_syntax::c_scalar::CContext;

use qc_lexer::{LexFlag, LexFlags};

use qc_preproc::{CompilerFlags, FteFlag};

use super::{Cast, ConstType, Kind, Literal, Source, Supply};
use crate::ty::{FunctionType, SimpleType, Type};

fn simple(ty: SimpleType) -> Type {
    Type::Simple(ty)
}

/// The type lowering makes of the type words `words` under FTEQCC's defaults.
fn named(words: &str) -> Type {
    Type::named(words, CContext::default())
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
        assert_eq!(
            Literal::of_fteqcc_number(text, LexFlags::default()).ty,
            ty,
            "{text}"
        );
    }
}

// spec FTEF-19.31: `QCC_SRef_IsNull` holds for an integer, unsigned or float 0, written in any
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
        assert_eq!(
            Literal::of_fteqcc_number(text, LexFlags::default()).null,
            null,
            "{text}"
        );
    }
}

fn literal(text: &str) -> Source<'static> {
    Source::Literal(Literal::of_fteqcc_number(text, LexFlags::default()))
}

fn constant(ty: ConstType) -> Source<'static> {
    Source::Literal(Literal::of(ty))
}

/// Whether FTEQCC's implicit cast of `source` to `to` fails with its default flags.
fn refused(source: Source<'_>, to: &Type) -> bool {
    source.cast(to, CompilerFlags::default()) == Cast::Refused
}

/// Whether FTEQCC makes the implicit cast of `source` to `to` with "Implicit cast", with its
/// default flags.
fn warns(source: Source<'_>, to: &Type) -> bool {
    source.cast(to, CompilerFlags::default()) == Cast::Implicit
}

// spec FTEF-19.31: a float casts to the numbers and to a vector, and to nothing else unless it
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

// spec FTEF-19.31: an integer casts to a string, an entity, a function and a vector, a double
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

// spec FTEF-19.31: a vector casts only to a vector, a string to the integers, strings and
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

// spec FTEF-19.31: a named definition casts as its type does, a field to another field type
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

// spec FTEF-19.31: C type words and registered type names are the type FTEF-2.8 makes, an 8-
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

// spec FTEF-19.31: a pointer casts to a pointer and a string, integers among themselves, and
// the casts that warn are those "acceptable but probably an error" and to a vector
#[test]
fn pointers_and_warnings() {
    let pointer = |ty: SimpleType| Type::Pointer(Rc::new(simple(ty)));
    let float_ptr = pointer(SimpleType::Float);
    assert!(!refused(
        Source::Typed(&float_ptr),
        &pointer(SimpleType::Vector)
    ));
    assert!(!refused(
        Source::Typed(&float_ptr),
        &simple(SimpleType::String)
    ));
    assert!(refused(
        Source::Typed(&float_ptr),
        &simple(SimpleType::Float)
    ));
    assert!(refused(
        Source::Typed(&float_ptr),
        &simple(SimpleType::Entity)
    ));
    let int = named("int");
    let entity = simple(SimpleType::Entity);
    let string = simple(SimpleType::String);
    assert!(!warns(Source::Typed(&int), &named("unsigned")));
    assert!(warns(Source::Typed(&int), &entity));
    assert!(warns(Source::Typed(&entity), &int));
    assert!(warns(Source::Typed(&string), &int));
    assert!(!warns(Source::Typed(&string), &string));
    assert!(!warns(Source::Typed(&int), &simple(SimpleType::Float)));
    assert!(warns(literal("1"), &simple(SimpleType::Vector)));
    assert!(!warns(literal("0"), &simple(SimpleType::Vector)));
    assert!(!warns(literal("1"), &simple(SimpleType::String)));
    let f = |param: SimpleType| {
        Type::Function(Rc::new(FunctionType {
            ret: simple(SimpleType::Void),
            params: vec![simple(param)],
            variadic: false,
        }))
    };
    assert!(warns(
        Source::Typed(&f(SimpleType::Float)),
        &f(SimpleType::Vector)
    ));
    assert!(!warns(
        Source::Typed(&f(SimpleType::Float)),
        &f(SimpleType::Float)
    ));
}

// spec FTEF-17.13: two types are FTEQCC's same type part by part, a C word by its kind
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
    // FTEF-17.13: a pointer is the same as a pointer to the same, and as no other type.
    let pointer = |inner: &Type| Type::Pointer(Rc::new(inner.clone()));
    assert!(!pointer(&float).fteqcc_same(&simple(SimpleType::Vector)));
    assert!(!pointer(&float).fteqcc_same(&float));
    assert!(pointer(&float).fteqcc_same(&pointer(&float)));
    assert!(!pointer(&float).fteqcc_same(&pointer(&simple(SimpleType::String))));
    assert!(pointer(&named("int")).fteqcc_same(&pointer(&named("integer"))));
    assert!(pointer(&float).fteqcc_same(&named("Player")));
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

// spec FTEF-19.31: the message spells a type as FTEQCC's `TypeName` does
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

// spec FTEF-19.24: under `assumeint` a number with no suffix is an integer type by its magnitude,
// and under `assumedouble` a number with a `.` is a double
#[test]
fn assumed_number_types() {
    let int = LexFlags::default().with(LexFlag::AssumeInt, true);
    let double = LexFlags::default().with(LexFlag::AssumeDouble, true);
    for (text, ty) in [
        ("5", ConstType::Integer),
        ("-5", ConstType::Integer),
        ("2147483647", ConstType::Integer),
        ("2147483648", ConstType::Uint),
        ("0xffffffff", ConstType::Uint),
        ("4294967296", ConstType::Int64),
        ("0x7fffffffffffffff", ConstType::Int64),
        ("0x8000000000000000", ConstType::Uint64),
        ("010", ConstType::Integer),
        ("5u", ConstType::Uint),
        ("5d", ConstType::Double),
        ("5.5", ConstType::Float),
    ] {
        assert_eq!(Literal::of_fteqcc_number(text, int).ty, ty, "{text}");
    }
    assert!(Literal::of_fteqcc_number("0", int).null);
    assert_eq!(
        Literal::of_fteqcc_number("5.5", double).ty,
        ConstType::Double
    );
    assert!(!Literal::of_fteqcc_number("0.0", double).null);
    assert_eq!(Literal::of_fteqcc_number("5", double).ty, ConstType::Float);
    assert_eq!(
        Literal::of_fteqcc_number("5i", double).ty,
        ConstType::Integer
    );
    // An `f` suffix is a float under either.
    assert_eq!(Literal::of_fteqcc_number("5f", int).ty, ConstType::Float);
    assert_eq!(Literal::of_fteqcc_number("5F", int).ty, ConstType::Float);
    assert_eq!(
        Literal::of_fteqcc_number("1.5f", double).ty,
        ConstType::Float
    );
    assert_eq!(
        Literal::of_fteqcc_number("1.5d", LexFlags::default()).ty,
        ConstType::Double
    );
}

// spec FTEF-19.24: one `l` is 64 bits, or two under `ILP32` (FTEF-12.12)
#[test]
fn long_suffixes_and_ilp32() {
    let ilp32 = LexFlags::default().with(LexFlag::Ilp32, true);
    let plain = LexFlags::default();
    assert_eq!(Literal::of_fteqcc_number("5l", plain).ty, ConstType::Int64);
    assert_eq!(
        Literal::of_fteqcc_number("5l", ilp32).ty,
        ConstType::Integer
    );
    assert_eq!(Literal::of_fteqcc_number("5ul", ilp32).ty, ConstType::Uint);
    assert_eq!(Literal::of_fteqcc_number("5ll", ilp32).ty, ConstType::Int64);
    assert_eq!(
        Literal::of_fteqcc_number("5llu", ilp32).ty,
        ConstType::Uint64
    );
}

fn flags(set: &[FteFlag]) -> CompilerFlags {
    set.iter().fold(CompilerFlags::default(), |flags, &flag| {
        flags.with(flag, true)
    })
}

// spec FTEF-22.1: under `lax` a refused cast is made with its warning, and a cast that is made
// is unchanged
#[test]
fn lax_casts() {
    let lax = flags(&[FteFlag::Laxcasts]);
    let float = simple(SimpleType::Float);
    assert_eq!(constant(ConstType::String).cast(&float, lax), Cast::Lax);
    assert_eq!(literal("1").cast(&float, lax), Cast::Silent);
    assert_eq!(
        literal("1").cast(&simple(SimpleType::Vector), lax),
        Cast::Implicit
    );
    assert_eq!(
        constant(ConstType::String).cast(&float, CompilerFlags::default()),
        Cast::Refused
    );
}

// spec FTEF-22.2: `qccx` casts a float to an entity or a string and back, with the warning
#[test]
fn qccx_casts() {
    let qccx = flags(&[FteFlag::Qccx]);
    let (float, entity, string) = (
        simple(SimpleType::Float),
        simple(SimpleType::Entity),
        simple(SimpleType::String),
    );
    assert_eq!(literal("1").cast(&entity, qccx), Cast::Implicit);
    assert_eq!(literal("1").cast(&string, qccx), Cast::Implicit);
    assert_eq!(Source::Typed(&entity).cast(&float, qccx), Cast::Implicit);
    assert_eq!(Source::Typed(&string).cast(&float, qccx), Cast::Implicit);
    assert_eq!(literal("1d").cast(&entity, qccx), Cast::Refused);
    assert_eq!(
        constant(ConstType::String).cast(&simple(SimpleType::Vector), qccx),
        Cast::Refused
    );
    assert_eq!(literal("0").cast(&entity, qccx), Cast::Silent);
}

// spec FTEF-22.4: under `flag_undefwordsize` a string to any pointer is disabled, a pointer to a
// string not
#[test]
fn undefined_word_size_casts() {
    let dp = flags(&[FteFlag::Undefwordsize]);
    let pointer = |ty: Type| Type::Pointer(Rc::new(ty));
    for to in [
        pointer(simple(SimpleType::Float)),
        pointer(simple(SimpleType::Void)),
        pointer(named("__uint8")),
    ] {
        assert_eq!(
            constant(ConstType::String).cast(&to, dp),
            Cast::StringPointerDisabled,
            "{to:?}"
        );
    }
    let float_ptr = pointer(simple(SimpleType::Float));
    assert_eq!(
        Source::Typed(&float_ptr).cast(&simple(SimpleType::String), dp),
        Cast::Implicit
    );
    assert_eq!(literal("0").cast(&float_ptr, dp), Cast::Silent);
}

// spec FTEF-19.31: `void*` and pointers to 8-bit integers cast with no warning, other pointers
// with one unless they point to one type
#[test]
fn void_and_byte_pointer_casts() {
    let none = CompilerFlags::default();
    let pointer = |ty: Type| Type::Pointer(Rc::new(ty));
    let void_ptr = pointer(simple(SimpleType::Void));
    let byte_ptr = pointer(named("__int8"));
    let float_ptr = pointer(simple(SimpleType::Float));
    let string = simple(SimpleType::String);
    assert_eq!(
        constant(ConstType::String).cast(&void_ptr, none),
        Cast::Silent
    );
    assert_eq!(
        constant(ConstType::String).cast(&byte_ptr, none),
        Cast::Silent
    );
    assert_eq!(
        constant(ConstType::String).cast(&float_ptr, none),
        Cast::Implicit
    );
    assert_eq!(
        constant(ConstType::String).cast(&pointer(named("__int16")), none),
        Cast::Implicit
    );
    assert_eq!(Source::Typed(&void_ptr).cast(&string, none), Cast::Silent);
    assert_eq!(Source::Typed(&byte_ptr).cast(&string, none), Cast::Silent);
    assert_eq!(
        Source::Typed(&float_ptr).cast(&void_ptr, none),
        Cast::Silent
    );
    assert_eq!(
        Source::Typed(&void_ptr).cast(&pointer(simple(SimpleType::Vector)), none),
        Cast::Silent
    );
    assert_eq!(
        Source::Typed(&float_ptr).cast(&pointer(simple(SimpleType::Vector)), none),
        Cast::Implicit
    );
    assert_eq!(
        Source::Typed(&pointer(named("int"))).cast(&pointer(named("integer")), none),
        Cast::Silent
    );
}

// spec FTEF-22.3: `QCC_SupplyConversion` converts the numbers but a double and a 32-bit integer,
// refuses any conversion under `typeexplicit`, and under `lax` keeps a value nothing converts
#[test]
fn supplied_conversions() {
    let none = CompilerFlags::default();
    let explicit = flags(&[FteFlag::Typeexplicit]);
    let lax = flags(&[FteFlag::Laxcasts]);
    assert_eq!(literal("1i").supply(Kind::Float, none), Supply::Converted);
    assert_eq!(
        literal("1i").supply(Kind::Float, explicit),
        Supply::Explicit
    );
    assert_eq!(
        literal("1").supply(Kind::Float, explicit),
        Supply::Converted
    );
    assert_eq!(
        literal("1u").supply(Kind::Integer, explicit),
        Supply::Converted
    );
    assert_eq!(literal("1d").supply(Kind::Integer, none), Supply::Mismatch);
    assert_eq!(literal("1d").supply(Kind::Int64, none), Supply::Converted);
    assert_eq!(literal("1l").supply(Kind::Integer, none), Supply::Converted);
    assert_eq!(literal("1").supply(Kind::Vector, none), Supply::Converted);
    assert_eq!(
        constant(ConstType::String).supply(Kind::Float, none),
        Supply::Mismatch
    );
    assert_eq!(
        constant(ConstType::String).supply(Kind::Float, lax),
        Supply::Lax
    );
    assert_eq!(
        constant(ConstType::String).supply(Kind::Float, explicit),
        Supply::Explicit
    );
    assert_eq!(
        constant(ConstType::Vector).supply(Kind::Float, none),
        Supply::Mismatch
    );
    let pointer = |ty: Type| Type::Pointer(Rc::new(ty));
    let float_ptr = pointer(simple(SimpleType::Float));
    assert_eq!(
        Source::Typed(&float_ptr).supply(Kind::Integer, none),
        Supply::Converted
    );
    assert_eq!(
        Source::Typed(&float_ptr).supply(Kind::Float, none),
        Supply::Mismatch
    );
    assert_eq!(
        Source::Typed(&float_ptr).supply(Kind::Pointer, explicit),
        Supply::Converted
    );
    assert_eq!(
        Source::Typed(&named("Player")).supply(Kind::Float, explicit),
        Supply::Converted
    );
    for (kind, name) in [
        (Kind::Integer, "integer"),
        (Kind::Uint, "uint"),
        (Kind::Int64, "long"),
        (Kind::Uint64, "ulong"),
        (Kind::Double, "double"),
        (Kind::Opaque, "variant"),
        (Kind::String, "string"),
    ] {
        assert_eq!(kind.basic_name(), name);
    }
}
