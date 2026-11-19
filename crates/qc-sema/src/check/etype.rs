// SPDX-License-Identifier: GPL-3.0-or-later

//! The basic type of a value as FTEQCC's checker reads it, its `etype_t` (FTEF-23.3).

/// FTEQCC's `etype_t`: what a type is at bottom, the operand types of its opcodes are written
/// in (`progslib.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Etype {
    /// `ev_void`.
    Void,
    /// `ev_string`.
    String,
    /// `ev_float`.
    Float,
    /// `ev_vector`.
    Vector,
    /// `ev_entity`.
    Entity,
    /// `ev_field`.
    Field,
    /// `ev_function`.
    Function,
    /// `ev_pointer`.
    Pointer,
    /// `ev_integer`.
    Integer,
    /// `ev_uint`.
    Uint,
    /// `ev_int64`.
    Int64,
    /// `ev_uint64`.
    Uint64,
    /// `ev_double`.
    Double,
    /// `ev_variant`.
    Variant,
    /// `ev_struct`.
    Struct,
    /// `ev_union`.
    Union,
    /// `ev_enum`.
    Enum,
    /// `ev_boolean`: a truth value of its `parentclass`'s type.
    Boolean,
}

impl Etype {
    /// FTEQCC's `etype_t` from `ev_void`, 0, to `ev_double`, in order.
    const NUMBERED: [Self; 13] = [
        Self::Void,
        Self::String,
        Self::Float,
        Self::Vector,
        Self::Entity,
        Self::Field,
        Self::Function,
        Self::Pointer,
        Self::Integer,
        Self::Uint,
        Self::Int64,
        Self::Uint64,
        Self::Double,
    ];

    /// `ev_variant` in FTEQCC's `etype_t`, after `ev_double` (FTEF-19.84).
    pub(crate) const VARIANT_NUMBER: u16 = 13;

    /// `ev_accessor` in FTEQCC's `etype_t`, after `ev_variant`, `ev_struct` and `ev_union`
    /// (FTEF-10.15).
    pub(crate) const ACCESSOR_NUMBER: u16 = 16;

    /// `ev_boolean` in FTEQCC's `etype_t`, after `ev_accessor`, `ev_enum` and `ev_typedef`
    /// (FTEF-19.84).
    pub(crate) const BOOLEAN_NUMBER: u16 = 19;

    /// `ev_bitfld` in FTEQCC's `etype_t`, the last (FTEF-19.84).
    pub(crate) const BITFLD_NUMBER: u16 = 20;

    /// Its number in a definition's record when it is one of C's scalars: an integer of 32
    /// or 64 bits or a double, `ev_integer` to `ev_double` (FTEF-19.81).
    pub(crate) fn scalar_number(self) -> Option<u16> {
        let scalar = matches!(
            self,
            Self::Integer | Self::Uint | Self::Int64 | Self::Uint64 | Self::Double
        );
        scalar.then(|| self.number()).flatten()
    }

    /// Whether it is one of the numbers `QCC_StoreSRefToRef` converts between (FTEF-29.21): a
    /// float, an integer of 32 or 64 bits, a double or a truth value.
    pub(crate) const fn is_number(self) -> bool {
        matches!(
            self,
            Self::Float
                | Self::Integer
                | Self::Uint
                | Self::Int64
                | Self::Uint64
                | Self::Double
                | Self::Boolean
        )
    }

    /// Its number in FTEQCC's `etype_t` when it is one of the engine's types, `ev_void` to
    /// `ev_double`.
    pub(crate) fn number(self) -> Option<u16> {
        let place = Self::NUMBERED.iter().position(|etype| *etype == self)?;
        u16::try_from(place).ok()
    }

    /// Its name in `basictypenames[]`, which some messages print.
    pub(crate) const fn basic_name(self) -> &'static str {
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
            Self::Variant => "variant",
            Self::Struct => "struct",
            Self::Union => "union",
            Self::Enum => "enum",
            Self::Boolean => "bool",
        }
    }

    /// `QCC_NumericTypeRanking`: how far up the numeric types a type is, `None` for one that is
    /// not numeric (FTEF-23.2).
    pub(crate) const fn numeric_ranking(self) -> Option<i32> {
        match self {
            Self::Double => Some(15),
            Self::Float => Some(10),
            Self::Uint64 => Some(6),
            Self::Int64 => Some(3),
            Self::Uint => Some(1),
            Self::Integer => Some(0),
            Self::Void
            | Self::String
            | Self::Vector
            | Self::Entity
            | Self::Field
            | Self::Function
            | Self::Pointer
            | Self::Variant
            | Self::Struct
            | Self::Union
            | Self::Enum
            | Self::Boolean => None,
        }
    }
}
