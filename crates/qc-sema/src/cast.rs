// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_TryEvaluateCast, QCC_EvaluateCast, QCC_SRef_IsNull
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_LexNumber, TypeName

//! The types of constants and FTEQCC's implicit casts.
//!
//! A literal initialiser has a type, which FTEQCC's `QCC_PR_LexNumber` gives a number by its
//! digits and suffix (FTEF-3), and `QCC_EvaluateCast` casts it, or a named constant, to the
//! type it initialises, or refuses with "Cannot cast from A to B" (FTEF-19.2). This module
//! holds those types, the cast rule of `QCC_TryEvaluateCast` with `implicit` set, and the
//! spelling of a type in FTEQCC's messages. Written as a clean port: the rule is one table of
//! the types' kinds, not the C's chain of conditions.

use crate::ty::{SimpleType, Type};

/// The type FTEQCC gives a literal (FTEF-3.1, FTEF-3.3, QC-4, QC-5, QC-6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConstType {
    /// `float`: a number with no suffix, an `f` suffix or a fraction, and a frame macro.
    Float,
    /// `__int32`: an `i` suffix.
    Integer,
    /// `__uint32`: a `u` suffix.
    Uint,
    /// `__int64`: an `l` suffix.
    Int64,
    /// `__uint64`: an `l` and a `u` suffix.
    Uint64,
    /// `__double`: a `d` suffix that is no digit of the number's base.
    Double,
    /// `vector`: a vector literal.
    Vector,
    /// `string`: a string literal.
    String,
}

/// A literal's type and whether FTEQCC takes it for the null constant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Literal {
    /// Its type.
    pub ty: ConstType,
    /// Whether it is null: `QCC_SRef_IsNull` holds for an integer, unsigned or float constant
    /// whose word is 0 (FTEF-19.2).
    pub null: bool,
}

impl Literal {
    /// The type and nullness `QCC_PR_LexNumber` gives the number token `text` (FTEF-3.1):
    /// a sign, a `0x` or leading-`0` base, the digits of that base, then a fraction, which
    /// makes a float (or, with a `d`, a double), or a suffix: `f` float, `d` double where it is
    /// no digit of the base, and `i`, `u` and `l` in either order an integer of that sign and
    /// width. With none the number is a float (FTEF-3.3, `flag_assume_integer` off).
    #[must_use]
    pub fn of_fteqcc_number(text: &str) -> Self {
        let unsigned = text.strip_prefix('-').unwrap_or(text);
        let (base, digits) = if let Some(hex) = unsigned
            .strip_prefix("0x")
            .or_else(|| unsigned.strip_prefix("0X"))
        {
            (16, hex)
        } else if unsigned.starts_with('0') {
            (8, unsigned)
        } else {
            (10, unsigned)
        };
        let mut zero = true;
        let mut chars = digits.chars().peekable();
        while let Some(&c) = chars.peek() {
            let Some(digit) = c.to_digit(base) else {
                break;
            };
            zero &= digit == 0;
            chars.next();
        }
        let rest: String = chars.collect();
        let ty = if let Some(fraction) = rest.strip_prefix('.') {
            zero &= fraction
                .chars()
                .take_while(char::is_ascii_digit)
                .all(|c| c == '0');
            // `-0.0` is a float whose word is not 0 (its sign bit), so it is no null.
            zero &= !text.starts_with('-');
            if fraction
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .starts_with(['d', 'D'])
            {
                ConstType::Double
            } else {
                ConstType::Float
            }
        } else {
            match rest.chars().next() {
                Some('d' | 'D') => ConstType::Double,
                Some('i' | 'I' | 'u' | 'U' | 'l' | 'L') => {
                    let long = rest.contains(['l', 'L']);
                    let unsigned = rest.contains(['u', 'U']);
                    match (long, unsigned) {
                        (true, true) => ConstType::Uint64,
                        (true, false) => ConstType::Int64,
                        (false, true) => ConstType::Uint,
                        (false, false) => ConstType::Integer,
                    }
                }
                Some(_) | None => ConstType::Float,
            }
        };
        // `QCC_SRef_IsNull` counts an integer, unsigned or float zero, and no double or 64-bit
        // one.
        let nullable = matches!(ty, ConstType::Float | ConstType::Integer | ConstType::Uint);
        Self {
            ty,
            null: zero && nullable,
        }
    }

    /// A literal of `ty` that is no null.
    #[must_use]
    pub fn of(ty: ConstType) -> Self {
        Self { ty, null: false }
    }
}

/// The kind of a type as FTEQCC's cast rule reads it, its `etype_t`; `Opaque` for a type the
/// semantic model does not follow (FTEF-17.12), which no check is made on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Void,
    String,
    Float,
    Vector,
    Entity,
    Field,
    Function,
    Pointer,
    Integer,
    Uint,
    Int64,
    Uint64,
    Double,
    Opaque,
}

impl Kind {
    /// The kind of a type the model holds. A C type word maps to its `etype_t` (FTEF-2.1 to
    /// FTEF-2.3); a narrower integer, `_Bool` and a type a program names are opaque.
    fn of(ty: &Type) -> Self {
        match ty {
            Type::Simple(SimpleType::Void) => Self::Void,
            Type::Simple(SimpleType::String) => Self::String,
            Type::Simple(SimpleType::Float) => Self::Float,
            Type::Simple(SimpleType::Vector) => Self::Vector,
            Type::Simple(SimpleType::Entity) => Self::Entity,
            Type::Field(_) => Self::Field,
            Type::Function(_) => Self::Function,
            Type::Pointer(_) => Self::Pointer,
            Type::Named(name) => match name.as_ref() {
                "int" | "integer" | "signed" | "signed int" | "int signed" => Self::Integer,
                "unsigned" | "unsigned int" | "int unsigned" => Self::Uint,
                "long" | "long int" | "signed long" | "long long" => Self::Int64,
                "unsigned long" | "long unsigned" => Self::Uint64,
                "double" => Self::Double,
                _ => Self::Opaque,
            },
            Type::Unknown => Self::Opaque,
        }
    }

    fn of_const(ty: ConstType) -> Self {
        match ty {
            ConstType::Float => Self::Float,
            ConstType::Integer => Self::Integer,
            ConstType::Uint => Self::Uint,
            ConstType::Int64 => Self::Int64,
            ConstType::Uint64 => Self::Uint64,
            ConstType::Double => Self::Double,
            ConstType::Vector => Self::Vector,
            ConstType::String => Self::String,
        }
    }

    /// Whether `QCC_TryEvaluateCast`, `implicit` set and `flag_qccx` off, casts a value of a
    /// different type of kind `self` to one of kind `to`: the numeric conversions, a float or
    /// an integer to a vector (with a warning), and the casts among pointers, strings,
    /// integers and functions and between integers and entities, "acceptable but probably an
    /// error".
    fn casts_to(self, to: Self) -> bool {
        use Kind::{
            Double, Entity, Float, Function, Int64, Integer, Pointer, String, Uint, Uint64,
        };
        let numeric = |k: Self| matches!(k, Float | Integer | Uint | Int64 | Uint64 | Double);
        let wordish = |k: Self| matches!(k, Pointer | String | Integer | Uint | Function);
        (numeric(self) && numeric(to))
            || (to == Self::Vector && matches!(self, Float | Integer))
            || (to == Entity && self == Entity)
            || (to == Pointer && self == Pointer)
            || (wordish(self) && wordish(to))
            || (to == Entity && self == Integer)
            || (to == Integer && self == Entity)
    }
}

/// What a value being cast is: a literal, or a named definition of a type.
#[derive(Clone, Copy, Debug)]
pub enum Source<'a> {
    /// A literal (FTEF-3).
    Literal(Literal),
    /// A definition's value, of its type; a named constant is never null.
    Typed(&'a Type),
}

impl Source<'_> {
    /// Whether FTEQCC's implicit cast of this value to `to` fails: "Cannot cast from A to B"
    /// (FTEF-19.2). `false` when it succeeds, or when either type is one the model does not
    /// follow.
    #[must_use]
    pub fn refused(self, to: &Type) -> bool {
        let to_kind = Kind::of(to);
        let (from_kind, null) = match self {
            Source::Literal(literal) => (Kind::of_const(literal.ty), literal.null),
            Source::Typed(ty) => {
                // `typecmp`: a type casts to itself. A redeclaration compares types structurally
                // (QC-10.6), and so does this.
                if ty == to {
                    return false;
                }
                (Kind::of(ty), false)
            }
        };
        if from_kind == Kind::Opaque || to_kind == Kind::Opaque {
            return false;
        }
        if from_kind == to_kind && !matches!(from_kind, Kind::Field) {
            return false;
        }
        // A null casts to any type but a struct or union, which are opaque here.
        if null {
            return false;
        }
        !from_kind.casts_to(to_kind)
    }

    /// Whether FTEQCC makes the implicit cast of this value to `to` with its warning
    /// "Implicit cast from A to B" (`WARN_LAXCAST`, FTEF-19.2): a float or an integer to a
    /// vector, and the casts "acceptable but probably an error" among pointers, strings,
    /// integers and functions and between integers and entities, a null excepted. `false` for
    /// a cast refused or made with no warning, and for a type the model does not follow.
    #[must_use]
    pub fn warns(self, to: &Type) -> bool {
        use Kind::{Entity, Function, Integer, Pointer, String, Uint, Vector};
        let to_kind = Kind::of(to);
        let (from_kind, null, same) = match self {
            Source::Literal(literal) => (Kind::of_const(literal.ty), literal.null, false),
            Source::Typed(ty) => (Kind::of(ty), false, ty == to),
        };
        if same || null || from_kind == Kind::Opaque || to_kind == Kind::Opaque {
            return false;
        }
        // A literal of the type's own kind is the type; two functions differ in their signatures.
        if from_kind == to_kind && from_kind != Function {
            return false;
        }
        if self.refused(to) {
            return false;
        }
        let wordish = |k: Kind| matches!(k, Pointer | String | Integer | Uint | Function);
        let integer = |k: Kind| matches!(k, Integer | Uint);
        (to_kind == Vector && matches!(from_kind, Kind::Float | Integer))
            || (wordish(from_kind) && wordish(to_kind) && !(integer(from_kind) && integer(to_kind)))
            || (to_kind == Entity && from_kind == Integer)
            || (to_kind == Integer && from_kind == Entity)
    }

    /// The name of this value's type in a message.
    #[must_use]
    pub fn name(self) -> String {
        match self {
            Source::Literal(literal) => literal.ty.name().to_owned(),
            Source::Typed(ty) => ty.fteqcc_name(),
        }
    }
}

impl ConstType {
    /// The name FTEQCC's `TypeName` gives this type.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            ConstType::Float => "float",
            ConstType::Integer => "__int32",
            ConstType::Uint => "__uint32",
            ConstType::Int64 => "__int64",
            ConstType::Uint64 => "__uint64",
            ConstType::Double => "__double",
            ConstType::Vector => "vector",
            ConstType::String => "string",
        }
    }
}

impl Type {
    /// This type as FTEQCC's `TypeName` writes it in a message, without parameter names, which
    /// the model's types do not keep (QC-10.3): `.float`, `void(float, vector)`, `entity*`.
    #[must_use]
    pub fn fteqcc_name(&self) -> String {
        let ty = self;
        match ty {
            Type::Unknown => "__variant".to_owned(),
            Type::Simple(simple) => simple.as_str().to_owned(),
            Type::Field(inner) => format!(".{}", inner.fteqcc_name()),
            Type::Pointer(inner) => format!("{}*", inner.fteqcc_name()),
            Type::Named(name) => match Kind::of(ty) {
                Kind::Integer => "__int32".to_owned(),
                Kind::Uint => "__uint32".to_owned(),
                Kind::Int64 => "__int64".to_owned(),
                Kind::Uint64 => "__uint64".to_owned(),
                Kind::Double => "__double".to_owned(),
                Kind::Void
                | Kind::String
                | Kind::Float
                | Kind::Vector
                | Kind::Entity
                | Kind::Field
                | Kind::Function
                | Kind::Pointer
                | Kind::Opaque => name.to_string(),
            },
            Type::Function(func) => {
                let mut params: Vec<String> = func.params.iter().map(Type::fteqcc_name).collect();
                if func.variadic {
                    params.push("...".to_owned());
                }
                format!("{}({})", func.ret.fteqcc_name(), params.join(", "))
            }
        }
    }

    /// The kind of this type, FTEQCC's `etype_t`, by name: `float`, `.`-less `field`,
    /// `__int32`; `None` for a type the model does not follow. Two types of one kind are what
    /// `WARN_SAMENAMEASGLOBAL` compares (FTEF-19.1), and an enum's base is numeric by it
    /// (FTEF-19.10).
    #[must_use]
    pub fn fteqcc_kind(&self) -> Option<&'static str> {
        Some(match Kind::of(self) {
            Kind::Void => "void",
            Kind::String => "string",
            Kind::Float => "float",
            Kind::Vector => "vector",
            Kind::Entity => "entity",
            Kind::Field => "field",
            Kind::Function => "function",
            Kind::Pointer => "pointer",
            Kind::Integer => "__int32",
            Kind::Uint => "__uint32",
            Kind::Int64 => "__int64",
            Kind::Uint64 => "__uint64",
            Kind::Double => "__double",
            Kind::Opaque => return None,
        })
    }
}

#[cfg(test)]
mod tests;
