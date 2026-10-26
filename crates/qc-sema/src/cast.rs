// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_TryEvaluateCast, QCC_EvaluateCast, QCC_SRef_IsNull, QCC_SupplyConversion, QCC_ShouldConvert
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_cmdlib.c, basictypenames
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_LexNumber, TypeName

//! The types of constants and FTEQCC's implicit casts.
//!
//! A literal initialiser has a type, which FTEQCC's `QCC_PR_LexNumber` gives a number by its
//! digits and suffix (FTEF-3), and `QCC_EvaluateCast` casts it, or a named constant, to the
//! type it initialises, or refuses with "Cannot cast from A to B" (FTEF-19.31). This module
//! holds those types, the cast rule of `QCC_TryEvaluateCast` with `implicit` set, and the
//! spelling of a type in FTEQCC's messages. Written as a clean port: the rule is one table of
//! the types' kinds, not the C's chain of conditions.

use qc_lexer::{LexFlag, LexFlags};
use qc_preproc::{CompilerFlags, FteFlag};
use qc_syntax::c_scalar::CScalar;

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
    /// whose word is 0 (FTEF-19.31).
    pub null: bool,
}

/// FTEF-19.24: the type `assumeint` gives a number with no suffix, by its magnitude as
/// `QCC_PR_LexNumber` reads it: past `0x7fffffffffffffff` `__uint64`, past `0xffffffff`
/// `__int64`, past `0x7fffffff` `__uint32`, else `__int32`.
fn assumed_int(base: u32, digits: &str) -> ConstType {
    let mut num: u64 = 0;
    for digit in digits.chars().map_while(|c| c.to_digit(base)) {
        num = num
            .wrapping_mul(u64::from(base))
            .wrapping_add(u64::from(digit));
    }
    if num > 0x7fff_ffff_ffff_ffff {
        ConstType::Uint64
    } else if num > 0xffff_ffff {
        ConstType::Int64
    } else if num > 0x7fff_ffff {
        ConstType::Uint
    } else {
        ConstType::Integer
    }
}

impl Literal {
    /// The type and nullness `QCC_PR_LexNumber` gives the number token `text` (FTEF-3.1):
    /// a sign, a `0x` or leading-`0` base, the digits of that base, then a fraction, which
    /// makes a float (or, with a `d`, a double), or a suffix: `f` float, `d` double where it is
    /// no digit of the base, and `i`, `u` and `l` in either order an integer of that sign and
    /// width. With none the number is a float (FTEF-3.3, `flag_assume_integer` off).
    #[must_use]
    pub fn of_fteqcc_number(text: &str, flags: LexFlags) -> Self {
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
            // FTEF-19.24: `pr_immediate_type = flag_assume_double?type_double:type_float;`, then
            // an `f` suffix makes it a float and a `d` a double.
            let suffix = fraction.trim_start_matches(|c: char| c.is_ascii_digit());
            if suffix.starts_with(['f', 'F']) {
                ConstType::Float
            } else if suffix.starts_with(['d', 'D']) || flags.has(LexFlag::AssumeDouble) {
                ConstType::Double
            } else {
                ConstType::Float
            }
        } else {
            match rest.chars().next() {
                Some('d' | 'D') => ConstType::Double,
                // FTEF-19.24: an `f` suffix is a float before `assumeint` is read.
                Some('f' | 'F') => ConstType::Float,
                Some('i' | 'I' | 'u' | 'U' | 'l' | 'L') => {
                    // `if (islong >= (flag_ILP32?2:1))`: one `l` is 64 bits, or two under
                    // `ILP32` (FTEF-12.12).
                    let longs = rest.chars().filter(|c| matches!(c, 'l' | 'L')).count();
                    let long = longs >= if flags.has(LexFlag::Ilp32) { 2 } else { 1 };
                    let unsigned = rest.contains(['u', 'U']);
                    match (long, unsigned) {
                        (true, true) => ConstType::Uint64,
                        (true, false) => ConstType::Int64,
                        (false, true) => ConstType::Uint,
                        (false, false) => ConstType::Integer,
                    }
                }
                Some(_) | None if flags.has(LexFlag::AssumeInt) => assumed_int(base, digits),
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

/// The kind of a type, FTEQCC's `etype_t`, as its cast rule and redeclaration checks read it;
/// `Opaque` for a type the semantic model does not follow (FTEF-17.13), on which no check is
/// made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `void`.
    Void,
    /// `string`.
    String,
    /// `float`.
    Float,
    /// `vector`.
    Vector,
    /// `entity`.
    Entity,
    /// A field type.
    Field,
    /// A function type.
    Function,
    /// A pointer type.
    Pointer,
    /// `__int32`: `int`, `integer`, `signed`.
    Integer,
    /// `__uint32`: `unsigned`.
    Uint,
    /// `__int64`: `long`.
    Int64,
    /// `__uint64`: `unsigned long`.
    Uint64,
    /// `__double`: `double`.
    Double,
    /// A type the model does not follow: one a program names, a narrower integer, `_Bool`.
    Opaque,
}

/// The scalar a [`Type::Named`] names: one FTEQCC registers by that name, as lowering names
/// the type C type words make (FTEF-2.8, FTEF-20.9).
fn c_scalar(name: &str) -> Option<CScalar> {
    CScalar::named(name)
}

impl Kind {
    /// The kind of a type the model holds. C type words, and the type names FTEQCC registers,
    /// are the type `QCC_PR_ParseType` makes of them (FTEF-2.8); an 8- or 16-bit integer,
    /// `__bint` and a type a program names are opaque.
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
            Type::Named(name) => Self::of_c_words(name),
            Type::Unknown => Self::Opaque,
        }
    }

    /// The kind the registered type name `name` names (FTEF-2.8), or `Opaque` when it names
    /// none.
    fn of_c_words(name: &str) -> Self {
        let Some(scalar) = c_scalar(name) else {
            return Self::Opaque;
        };
        match scalar {
            CScalar::Float => Self::Float,
            CScalar::Double => Self::Double,
            CScalar::Integer => Self::Integer,
            CScalar::Uint => Self::Uint,
            CScalar::Int64 => Self::Int64,
            CScalar::Uint64 => Self::Uint64,
            CScalar::Int16
            | CScalar::Uint16
            | CScalar::Int8
            | CScalar::Uint8
            | CScalar::Bool
            | CScalar::BFloat => Self::Opaque,
        }
    }

    /// Whether a value of this kind is a number: FTEQCC's float, integer and double types.
    #[must_use]
    pub fn is_numeric(self) -> bool {
        matches!(
            self,
            Self::Float | Self::Integer | Self::Uint | Self::Int64 | Self::Uint64 | Self::Double
        )
    }

    /// The name of this kind in FTEQCC's messages (`basictypenames`): `float`, `__int32`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Void => "void",
            Self::String => "string",
            Self::Float => "float",
            Self::Vector => "vector",
            Self::Entity => "entity",
            Self::Field => "field",
            Self::Function => "function",
            Self::Pointer => "pointer",
            Self::Integer => "__int32",
            Self::Uint => "__uint32",
            Self::Int64 => "__int64",
            Self::Uint64 => "__uint64",
            Self::Double => "__double",
            Self::Opaque => "__variant",
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

    /// The name `basictypenames` gives this kind, which `QCC_SupplyConversion`'s messages write
    /// (FTEF-22.3): `integer`, `uint`, `long`, `ulong`, `double`; `variant` for a type the model
    /// does not follow.
    #[must_use]
    pub fn basic_name(self) -> &'static str {
        match self {
            Self::Void => "void",
            Self::String => "string",
            Self::Float => "float",
            Self::Vector => "vector",
            Self::Entity => "entity",
            Self::Field => "field",
            Self::Function => "function",
            Self::Pointer => "pointer",
            Self::Integer => "integer",
            Self::Uint => "uint",
            Self::Int64 => "long",
            Self::Uint64 => "ulong",
            Self::Double => "double",
            Self::Opaque => "variant",
        }
    }

    /// `QCC_ShouldConvert(from, wanted)` of a value of this kind that is no pointer: none needed
    /// for the same kind, among the 32-bit and among the 64-bit integers, and from an integer to
    /// a function or a pointer; a conversion among the numbers but between a double and a
    /// 32-bit integer, and from a float to a vector; else none possible.
    fn conversion_to(self, wanted: Self) -> Conversion {
        use Kind::{Double, Float, Function, Int64, Integer, Pointer, Uint, Uint64, Vector};
        let int32 = |k: Self| matches!(k, Integer | Uint);
        let int64 = |k: Self| matches!(k, Int64 | Uint64);
        if self == wanted
            || (self == Integer && matches!(wanted, Function | Pointer))
            || (int32(self) && int32(wanted))
            || (int64(self) && int64(wanted))
        {
            return Conversion::None;
        }
        let converts = matches!(
            (self, wanted),
            (Float, Integer | Uint | Int64 | Uint64 | Double | Vector)
                | (Integer | Uint | Int64 | Uint64 | Double, Float)
                | (Int64 | Uint64, Integer | Uint | Double)
                | (Integer | Uint | Double, Int64 | Uint64)
        );
        if converts {
            Conversion::Op
        } else {
            Conversion::Impossible
        }
    }
}

/// What `QCC_ShouldConvert` finds for a value and a wanted kind (FTEF-22.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Conversion {
    /// 0: the value is of the kind already, or needs no instruction to be.
    None,
    /// An opcode converts it.
    Op,
    /// -1: nothing converts it.
    Impossible,
}

/// What `QCC_EvaluateCast(src, type, true)` makes of a value cast to a type (FTEF-19.31,
/// FTEF-22.1, FTEF-22.2, FTEF-22.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cast {
    /// Made, with no word; also a cast between types the model does not follow.
    Silent,
    /// Made, with the warning `W0220` "Implicit cast from A to B" (`WARN_LAXCAST`).
    Implicit,
    /// Made, with the non-fatal error `E0237` "pointer<->string casts were disabled."
    /// (`ERR_BADEXTENSION`): a string to a pointer under `flag_undefwordsize`.
    StringPointerDisabled,
    /// Refused by `QCC_TryEvaluateCast` and made under `lax`, with the warning `W0229`
    /// "Implicit lax cast from A to B" (warning 0, `WARN_DEBUGGING`).
    Lax,
    /// Refused: the error `E0213` "Cannot cast from A to B".
    Refused,
}

/// What `QCC_SupplyConversion(var, wanted, true)` makes of an enum's value or a state frame
/// converted to a number (FTEF-22.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Supply {
    /// The value is converted, or needs no conversion.
    Converted,
    /// The fatal error `E0236` "Automatic type conversions disabled. Needed A, got B." under
    /// `typeexplicit`, for any value that is not of the kind already.
    Explicit,
    /// Under `lax`, the warning `W0230` "Implicit type mismatch. Needed A, got B."
    /// (`WARN_LAXCAST`), and the value kept unconverted.
    Lax,
    /// The fatal error `E0227` "Implicit type mismatch. Needed A, got B.".
    Mismatch,
}

/// What a value being cast is: a literal, or a named definition of a type.
#[derive(Clone, Copy, Debug)]
pub enum Source<'a> {
    /// A literal (FTEF-3).
    Literal(Literal),
    /// A definition's value, of its type; a named constant is never null.
    Typed(&'a Type),
}

/// Whether two pointers' targets `a` and `b` are one type to `typecmp_lax`, which follows
/// pointers to what they point to: a pointer and a pointer agree by their targets, anything
/// else as [`Type::fteqcc_same`] says.
fn pointees_agree(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::Pointer(a), Type::Pointer(b)) => pointees_agree(a, b),
        (Type::Pointer(_), other) | (other, Type::Pointer(_)) => {
            other.fteqcc_kind() == Kind::Opaque
        }
        _ => a.fteqcc_same(b),
    }
}

/// What a pointer type points to, as `QCC_TryEvaluateCast` tells pointers apart (FTEF-22.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pointee {
    /// `void`: any pointer, function and string casts to and from a pointer to it.
    Void,
    /// An 8-bit integer, `__int8` or `__uint8`: a string casts to and from a pointer to it,
    /// "string and char* are the same type".
    Byte,
    /// Anything else.
    Other,
}

impl Pointee {
    /// What `ty` points to, `None` when it is no pointer.
    fn of(ty: &Type) -> Option<Self> {
        let Type::Pointer(inner) = ty else {
            return None;
        };
        Some(match &**inner {
            Type::Simple(SimpleType::Void) => Self::Void,
            Type::Named(_)
                if matches!(inner.fteqcc_scalar(), Some(CScalar::Int8 | CScalar::Uint8)) =>
            {
                Self::Byte
            }
            Type::Unknown
            | Type::Simple(_)
            | Type::Field(_)
            | Type::Function(_)
            | Type::Pointer(_)
            | Type::Named(_) => Self::Other,
        })
    }
}

impl Source<'_> {
    /// `QCC_EvaluateCast(src, to, true)` with the flags `flags` where the value stands
    /// (FTEF-19.31, FTEF-22): `QCC_TryEvaluateCast`'s cast, or when it refuses, under `lax` the
    /// value taken as of the type with a warning, else "Cannot cast".
    #[must_use]
    pub fn cast(self, to: &Type, flags: CompilerFlags) -> Cast {
        match self.try_cast(to, flags) {
            Some(cast) => cast,
            None if flags.has(FteFlag::Laxcasts) => Cast::Lax,
            None => Cast::Refused,
        }
    }

    /// `QCC_TryEvaluateCast(src, to, true)`, its branches in order; `None` when it fails.
    /// `qccx` adds a float to an entity or a string and back to the casts "acceptable but
    /// probably an error"; `flag_undefwordsize` makes a string to a pointer the error of
    /// [`Cast::StringPointerDisabled`], before the `char*` and `void*` branches.
    fn try_cast(self, to: &Type, flags: CompilerFlags) -> Option<Cast> {
        use Kind::{Entity, Field, Float, Function, Integer, Pointer, String, Uint, Vector};
        let to_kind = Kind::of(to);
        let (from_kind, null, from_ty) = match self {
            Source::Literal(literal) => (Kind::of_const(literal.ty), literal.null, None),
            Source::Typed(ty) => (Kind::of(ty), false, Some(ty)),
        };
        // `typecmp`: a type casts to itself. A redeclaration compares types structurally
        // (QC-10.6), and so does this. A type the model does not follow is not checked.
        if from_ty == Some(to) || from_kind == Kind::Opaque || to_kind == Kind::Opaque {
            return Some(Cast::Silent);
        }
        // A literal of the type's own kind is the type, and so is a definition of a simple
        // type's kind; fields, functions and pointers differ in what they hold.
        if from_kind == to_kind && !matches!(from_kind, Field | Function | Pointer) {
            return Some(Cast::Silent);
        }
        // A null casts to any type but a struct or union, which are opaque here; the numbers
        // convert among themselves.
        if null || (from_kind.is_numeric() && to_kind.is_numeric()) {
            return Some(Cast::Silent);
        }
        if to_kind == Vector && matches!(from_kind, Float | Integer) {
            return Some(Cast::Implicit);
        }
        if flags.has(FteFlag::Undefwordsize) && to_kind == Pointer && from_kind == String {
            return Some(Cast::StringPointerDisabled);
        }
        let from_pointee = from_ty.and_then(Pointee::of);
        let to_pointee = Pointee::of(to);
        let pointerish = |k: Kind| matches!(k, Pointer | Function | String);
        if (to_pointee == Some(Pointee::Byte) && from_kind == String)
            || (to_kind == String && from_pointee == Some(Pointee::Byte))
            || (to_pointee == Some(Pointee::Void) && pointerish(from_kind))
            || (from_pointee == Some(Pointee::Void) && pointerish(to_kind))
        {
            return Some(Cast::Silent);
        }
        if let (Type::Pointer(to_inner), Some(Type::Pointer(from_inner))) = (to, from_ty) {
            // `typecmp_lax` of what they point to, a type the model does not follow agreeing.
            return Some(if pointees_agree(from_inner, to_inner) {
                Cast::Silent
            } else {
                Cast::Implicit
            });
        }
        // `typecmp` of two functions: the same type part by part is a cast to itself.
        if let (Type::Function(_), Some(from @ Type::Function(_))) = (to, from_ty)
            && from.fteqcc_same(to)
        {
            return Some(Cast::Silent);
        }
        let wordish = |k: Kind| matches!(k, Pointer | String | Integer | Uint | Function);
        let qccx = flags.has(FteFlag::Qccx);
        let acceptable = (wordish(from_kind) && wordish(to_kind))
            || (to_kind == Entity && from_kind == Integer)
            || (to_kind == Integer && from_kind == Entity)
            || (qccx
                && matches!(
                    (from_kind, to_kind),
                    (Float, Entity | String) | (Entity | String, Float)
                ));
        // `typecmp_lax` differs for two kinds, and for two functions of different types; the
        // two integer kinds were cast above.
        acceptable.then_some(Cast::Implicit)
    }

    /// `QCC_SupplyConversion(var, wanted, true)` of this value with the flags where it stands
    /// (FTEF-22.3): no conversion needed, or else under `typeexplicit` the error of
    /// [`Supply::Explicit`]; one that `QCC_ShouldConvert` finds none for is under `lax` a
    /// warning and else "Implicit type mismatch"; a type the model does not follow is not
    /// checked. A pointer converts only to an integer or a float by what it points to.
    #[must_use]
    pub fn supply(self, wanted: Kind, flags: CompilerFlags) -> Supply {
        let conversion = match self {
            Source::Literal(literal) => Kind::of_const(literal.ty).conversion_to(wanted),
            Source::Typed(Type::Pointer(inner)) => match (Kind::of(inner), wanted) {
                (_, Kind::Pointer) => Conversion::None,
                (Kind::Float, Kind::Integer) | (Kind::Integer, Kind::Float) => Conversion::Op,
                _ => Conversion::Impossible,
            },
            Source::Typed(ty) => Kind::of(ty).conversion_to(wanted),
        };
        if self.kind() == Kind::Opaque || wanted == Kind::Opaque {
            return Supply::Converted;
        }
        match conversion {
            Conversion::None => Supply::Converted,
            Conversion::Op | Conversion::Impossible if flags.has(FteFlag::Typeexplicit) => {
                Supply::Explicit
            }
            Conversion::Op => Supply::Converted,
            Conversion::Impossible if flags.has(FteFlag::Laxcasts) => Supply::Lax,
            Conversion::Impossible => Supply::Mismatch,
        }
    }

    /// The kind of this value's type.
    #[must_use]
    pub fn kind(self) -> Kind {
        match self {
            Source::Literal(literal) => Kind::of_const(literal.ty),
            Source::Typed(ty) => Kind::of(ty),
        }
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
                Kind::Opaque => name.to_string(),
                kind => kind.name().to_owned(),
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

    /// The C scalar this type is, when it is C type words or a registered type name
    /// (FTEF-2.8).
    #[must_use]
    pub fn fteqcc_scalar(&self) -> Option<CScalar> {
        match self {
            Type::Named(name) => c_scalar(name),
            Type::Simple(_)
            | Type::Field(_)
            | Type::Function(_)
            | Type::Pointer(_)
            | Type::Unknown => None,
        }
    }

    /// The kind of this type, FTEQCC's `etype_t`. Two types of one kind are what
    /// `WARN_SAMENAMEASGLOBAL` compares (FTEF-19.1), and an enum's base is numeric by it
    /// (FTEF-19.10).
    #[must_use]
    pub fn fteqcc_kind(&self) -> Kind {
        Kind::of(self)
    }

    /// Whether a redeclaration with type `other` declares the same type as this one under
    /// FTEQCC's `typecmp` (FTEF-17.13): equal, a C type word compared by the kind it names, a
    /// field, function or pointer compared part by part, and any type the model does not follow
    /// agreeing with any other.
    #[must_use]
    pub fn fteqcc_same(&self, other: &Self) -> bool {
        if self == other {
            return true;
        }
        match (self, other) {
            (Self::Field(a), Self::Field(b)) => a.fteqcc_same(b),
            (Self::Function(a), Self::Function(b)) => {
                a.variadic == b.variadic
                    && a.params.len() == b.params.len()
                    && a.ret.fteqcc_same(&b.ret)
                    && a.params
                        .iter()
                        .zip(&b.params)
                        .all(|(x, y)| x.fteqcc_same(y))
            }
            // FTEF-17.13: two pointers are the same when what they point to is.
            (Self::Pointer(a), Self::Pointer(b)) => a.fteqcc_same(b),
            _ => {
                // A type a program names is not followed, so it agrees with anything, a pointer
                // included; the rest agree when they are of one kind, as `int` and `integer`.
                let (a, b) = (self.fteqcc_kind(), other.fteqcc_kind());
                a == Kind::Opaque || b == Kind::Opaque || a == b
            }
        }
    }
}

#[cfg(test)]
mod tests;
