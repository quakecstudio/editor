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
