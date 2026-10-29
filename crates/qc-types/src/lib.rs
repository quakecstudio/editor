// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_comp.h, etype_t
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, type_size
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/progs.h, eval_t, EDICT_TO_PROG, PROG_TO_EDICT

//! The eight QuakeC types, the slots their values occupy, and the reserved global slots.
//!
//! A progs.dat stores every global and every entity field as 4-byte slots. This crate names
//! the types a definition can carry (`docs/spec/progs.md`, PROGS-16), says how each type's
//! value sits in its slots (PROGS-17), and gives the fixed slots at the start of the globals
//! that hold a call's return value and parameters (PROGS-18). It does not read files; the
//! progs.dat loader and the VM build on it.

#[cfg(test)]
mod tests;

/// Number of vanilla QC types, `ev_void` to `ev_pointer`.
///
/// `PROGS_TYPE_COUNT` in PROGS-16, from `etype_t` in `WinQuake/pr_comp.h`.
pub const PROGS_TYPE_COUNT: u16 = 8;

/// Slots a vector occupies: x, y and z.
///
/// `PROGS_VECTOR_SLOTS` in PROGS-16, from `type_size[ev_vector]` in `WinQuake/pr_edict.c`.
pub const PROGS_VECTOR_SLOTS: usize = 3;

/// Global slot 0, which nothing defines.
///
/// `PROGS_OFS_NULL` in PROGS-18, from `OFS_NULL` in `WinQuake/pr_comp.h`.
pub const PROGS_OFS_NULL: u16 = 0;

/// First of the three global slots that hold a function's return value.
///
/// `PROGS_OFS_RETURN` in PROGS-18, from `OFS_RETURN` in `WinQuake/pr_comp.h`.
pub const PROGS_OFS_RETURN: u16 = 1;

/// First global slot of call parameter 0.
///
/// `PROGS_OFS_PARM0` in PROGS-18, from `OFS_PARM0` in `WinQuake/pr_comp.h`.
pub const PROGS_OFS_PARM0: u16 = 4;

/// Global slots set aside for each call parameter, so a vector fits.
///
/// `PROGS_PARM_SLOTS` in PROGS-18, from the comment on `OFS_PARM0` in `WinQuake/pr_comp.h`.
pub const PROGS_PARM_SLOTS: u16 = 3;

/// First global slot after the return value and parameters; the system globals start here.
///
/// `PROGS_RESERVED_OFS` in PROGS-18, from `RESERVED_OFS` in `WinQuake/pr_comp.h`.
pub const PROGS_RESERVED_OFS: u16 = 28;

/// Most parameters a QC function can take.
///
/// `LIMIT_MAX_PARMS` in LIMIT-1, from `MAX_PARMS` in `WinQuake/pr_comp.h`.
pub const LIMIT_MAX_PARMS: u16 = 8;

/// First global slot of call parameter `n`, id's `OFS_PARM0` to `OFS_PARM7`.
///
/// Implements PROGS-18.1: parameter `n` starts at `4 + 3n`. Returns `None` for `n` of
/// [`LIMIT_MAX_PARMS`] or more, since a call has no such parameter (LIMIT-1.4).
#[must_use]
pub const fn ofs_parm(n: u16) -> Option<u16> {
    if n >= LIMIT_MAX_PARMS {
        return None;
    }
    match n.checked_mul(PROGS_PARM_SLOTS) {
        Some(offset) => offset.checked_add(PROGS_OFS_PARM0),
        None => None,
    }
}

/// One of the eight vanilla QC types, id's `etype_t`.
///
/// Implements PROGS-16.1. The discriminant is the type number a definition stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Etype {
    /// `ev_void`, type 0: a slot that holds nothing.
    Void = 0,
    /// `ev_string`, type 1: a [`StringRef`].
    String = 1,
    /// `ev_float`, type 2: an `f32`.
    Float = 2,
    /// `ev_vector`, type 3: three `f32`, x first.
    Vector = 3,
    /// `ev_entity`, type 4: an [`EntityRef`].
    Entity = 4,
    /// `ev_field`, type 5: a [`FieldRef`].
    Field = 5,
    /// `ev_function`, type 6: a [`FunctionRef`].
    Function = 6,
    /// `ev_pointer`, type 7: a [`PointerRef`].
    Pointer = 7,
}

impl Etype {
    /// Every type, in type-number order.
    pub const ALL: [Self; PROGS_TYPE_COUNT as usize] = [
        Self::Void,
        Self::String,
        Self::Float,
        Self::Vector,
        Self::Entity,
        Self::Field,
        Self::Function,
        Self::Pointer,
    ];

    /// The type with this number, or `None` above 7 (PROGS-16.1, PROGS-16.3).
    #[must_use]
    pub const fn from_number(number: u16) -> Option<Self> {
        match number {
            0 => Some(Self::Void),
            1 => Some(Self::String),
            2 => Some(Self::Float),
            3 => Some(Self::Vector),
            4 => Some(Self::Entity),
            5 => Some(Self::Field),
            6 => Some(Self::Function),
            7 => Some(Self::Pointer),
            _ => None,
        }
    }

    /// The type number a definition stores for this type (PROGS-16.1).
    #[must_use]
    pub const fn number(self) -> u16 {
        match self {
            Self::Void => 0,
            Self::String => 1,
            Self::Float => 2,
            Self::Vector => 3,
            Self::Entity => 4,
            Self::Field => 5,
            Self::Function => 6,
            Self::Pointer => 7,
        }
    }

    /// Slots a value of this type occupies, id's `type_size`.
    ///
    /// Implements PROGS-16.2: 3 for a vector and 1 for every other type, including the
    /// pointer, as on the 32-bit engine id shipped and in qcc's table.
    #[must_use]
    pub const fn slot_count(self) -> usize {
        match self {
            Self::Vector => PROGS_VECTOR_SLOTS,
            Self::Void
            | Self::String
            | Self::Float
            | Self::Entity
            | Self::Field
            | Self::Function
            | Self::Pointer => 1,
        }
    }

    /// id's name for the type, `ev_void` to `ev_pointer` (PROGS-16.1).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Void => "ev_void",
            Self::String => "ev_string",
            Self::Float => "ev_float",
            Self::Vector => "ev_vector",
            Self::Entity => "ev_entity",
            Self::Field => "ev_field",
            Self::Function => "ev_function",
            Self::Pointer => "ev_pointer",
        }
    }

    /// Whether QuakeC source can declare a definition of this type.
    ///
    /// Implements PROGS-16.4: every type but `ev_pointer`, whose values exist only at run
    /// time.
    #[must_use]
    pub const fn is_declarable(self) -> bool {
        match self {
            Self::Pointer => false,
            Self::Void
            | Self::String
            | Self::Float
            | Self::Vector
            | Self::Entity
            | Self::Field
            | Self::Function => true,
        }
    }
}

/// A definition's type number as a file stores it, with `DEF_SAVEGLOBAL` already removed.
///
/// Implements PROGS-16.3: a number above 7 is kept, not rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefType {
    /// One of the eight vanilla types.
    Known(Etype),
    /// A number id's engine does not know; it prints such a value as `bad type <n>`.
    Unknown(u16),
}

impl DefType {
    /// Classifies a type number (PROGS-16.1, PROGS-16.3).
    #[must_use]
    pub const fn from_number(number: u16) -> Self {
        match Etype::from_number(number) {
            Some(etype) => Self::Known(etype),
            None => Self::Unknown(number),
        }
    }

    /// The type number, unchanged from the one classified.
    #[must_use]
    pub const fn number(self) -> u16 {
        match self {
            Self::Known(etype) => etype.number(),
            Self::Unknown(number) => number,
        }
    }

    /// The vanilla type, if the number names one.
    #[must_use]
    pub const fn etype(self) -> Option<Etype> {
        match self {
            Self::Known(etype) => Some(etype),
            Self::Unknown(_) => None,
        }
    }
}

/// One 4-byte cell of the globals or of an entity's fields, held as its raw bits.
///
/// Implements PROGS-17.1: the same bits read as an `f32` or as an `i32`, like id's `eval_t`
/// union; nothing converts between the two.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Slot(u32);

impl Slot {
    /// The slot whose bits are all 0: the float 0, the integer 0 and every null reference.
    pub const ZERO: Self = Self(0);

    /// The slot holding exactly these bits.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// The slot's bits.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// A slot whose bits are this float's.
    #[must_use]
    pub const fn from_f32(value: f32) -> Self {
        Self(value.to_bits())
    }

    /// A slot whose bits are this integer's two's-complement representation.
    #[must_use]
    pub const fn from_i32(value: i32) -> Self {
        Self(u32::from_ne_bytes(value.to_ne_bytes()))
    }

    /// The slot's bits read as a float, id's `_float`.
    #[must_use]
    pub const fn as_f32(self) -> f32 {
        f32::from_bits(self.0)
    }

    /// The slot's bits read as an integer, id's `_int`.
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        i32::from_ne_bytes(self.0.to_ne_bytes())
    }

    /// The slot as the 4 little-endian bytes a progs.dat stores.
    #[must_use]
    pub const fn to_le_bytes(self) -> [u8; 4] {
        self.0.to_le_bytes()
    }

    /// The slot stored as these 4 little-endian bytes.
    #[must_use]
    pub const fn from_le_bytes(bytes: [u8; 4]) -> Self {
        Self(u32::from_le_bytes(bytes))
    }
}

/// A string value: a byte offset from the start of the string table, id's `string_t`.
///
/// Implements PROGS-17.2.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StringRef(i32);

impl StringRef {
    /// Offset 0, the empty string.
    pub const EMPTY: Self = Self(0);
}

/// An entity value: a byte offset from the start of the engine's edict array.
///
/// Implements PROGS-17.4: the offset is the entity number times the engine's edict size,
/// id's `EDICT_TO_PROG` and `PROG_TO_EDICT`; it is not the entity number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct EntityRef(i32);

impl EntityRef {
    /// Offset 0, the world.
    pub const WORLD: Self = Self(0);

    /// The value of entity `number` in an engine whose edicts are `edict_size` bytes.
    ///
    /// Returns `None` when the offset does not fit an `i32`.
    #[must_use]
    pub const fn from_number(number: i32, edict_size: i32) -> Option<Self> {
        match number.checked_mul(edict_size) {
            Some(offset) => Some(Self(offset)),
            None => None,
        }
    }

    /// The entity number this value names in an engine whose edicts are `edict_size` bytes.
    ///
    /// Returns `None` when `edict_size` is 0 or the offset is not a whole number of edicts,
    /// since such a value names no entity.
    #[must_use]
    pub const fn number(self, edict_size: i32) -> Option<i32> {
        match self.0.checked_rem(edict_size) {
            Some(0) => self.0.checked_div(edict_size),
            Some(_) | None => None,
        }
    }
}

/// A field value: a slot index into an entity's fields.
///
/// Implements PROGS-17.5.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FieldRef(i32);

impl FieldRef {
    /// Component `index` of a vector field: `.origin_y` is `.origin` plus 1 (PROGS-17.5).
    ///
    /// Returns `None` for an index of 3 or more, or when the slot index overflows.
    #[must_use]
    pub fn component(self, index: usize) -> Option<Self> {
        if index >= PROGS_VECTOR_SLOTS {
            return None;
        }
        let offset = i32::try_from(index).ok()?;
        self.0.checked_add(offset).map(Self)
    }
}

/// A function value: an index into the functions table, id's `func_t`.
///
/// Implements PROGS-17.6.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FunctionRef(i32);

impl FunctionRef {
    /// Index 0, the null function.
    pub const NULL: Self = Self(0);
}

/// A pointer value: a byte offset from the start of the engine's edict array to one field
/// slot of one entity.
///
/// Implements PROGS-17.7.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PointerRef(i32);

/// The bits of a reference type, and its conversions to and from the [`Slot`] that holds it:
/// a reference is stored as its 32-bit value, which the VM may reinterpret as any type.
macro_rules! reference_bits {
    ($($name:ident),*) => {$(
        impl $name {
            /// The reference with this 32-bit value.
            #[must_use]
            pub const fn from_bits(bits: i32) -> Self {
                Self(bits)
            }

            /// Its 32-bit value.
            #[must_use]
            pub const fn bits(self) -> i32 {
                self.0
            }
        }

        impl From<$name> for Slot {
            fn from(value: $name) -> Self {
                Self::from_i32(value.0)
            }
        }

        impl From<Slot> for $name {
            fn from(slot: Slot) -> Self {
                Self(slot.as_i32())
            }
        }
    )*};
}

reference_bits!(StringRef, EntityRef, FieldRef, FunctionRef, PointerRef);

/// A typed QC value.
///
/// Implements PROGS-17: each variant is laid out in [`Etype::slot_count`] slots as that
/// statement says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    /// `ev_void`: nothing.
    Void,
    /// `ev_string`.
    String(StringRef),
    /// `ev_float`.
    Float(f32),
    /// `ev_vector`: x, y, z.
    Vector([f32; PROGS_VECTOR_SLOTS]),
    /// `ev_entity`.
    Entity(EntityRef),
    /// `ev_field`.
    Field(FieldRef),
    /// `ev_function`.
    Function(FunctionRef),
    /// `ev_pointer`.
    Pointer(PointerRef),
}

/// Too few slots to hold a value of the type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooFewSlots {
    /// The type being read or written.
    pub etype: Etype,
    /// Slots the type needs.
    pub needed: usize,
    /// Slots there were.
    pub found: usize,
}

impl core::fmt::Display for TooFewSlots {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{} needs {} slots, found {}",
            self.etype.name(),
            self.needed,
            self.found
        )
    }
}

impl core::error::Error for TooFewSlots {}

impl Value {
    /// The value's type.
    #[must_use]
    pub const fn etype(&self) -> Etype {
        match self {
            Self::Void => Etype::Void,
            Self::String(_) => Etype::String,
            Self::Float(_) => Etype::Float,
            Self::Vector(_) => Etype::Vector,
            Self::Entity(_) => Etype::Entity,
            Self::Field(_) => Etype::Field,
            Self::Function(_) => Etype::Function,
            Self::Pointer(_) => Etype::Pointer,
        }
    }

    /// Reads a value of type `etype` from the start of `slots`, reinterpreting the bits as
    /// id's `eval_t` does (PROGS-17.1 to PROGS-17.7).
    ///
    /// # Errors
    ///
    /// [`TooFewSlots`] when `slots` is shorter than [`Etype::slot_count`].
    pub fn read(etype: Etype, slots: &[Slot]) -> Result<Self, TooFewSlots> {
        let needed = etype.slot_count();
        let too_few = TooFewSlots {
            etype,
            needed,
            found: slots.len(),
        };
        let cells = slots.get(..needed).ok_or(too_few)?;
        let first = cells.first().copied().ok_or(too_few)?;
        Ok(match etype {
            Etype::Void => Self::Void,
            Etype::String => Self::String(StringRef(first.as_i32())),
            Etype::Float => Self::Float(first.as_f32()),
            Etype::Vector => match cells {
                [x, y, z] => Self::Vector([x.as_f32(), y.as_f32(), z.as_f32()]),
                _ => return Err(too_few),
            },
            Etype::Entity => Self::Entity(EntityRef(first.as_i32())),
            Etype::Field => Self::Field(FieldRef(first.as_i32())),
            Etype::Function => Self::Function(FunctionRef(first.as_i32())),
            Etype::Pointer => Self::Pointer(PointerRef(first.as_i32())),
        })
    }

    /// Writes the value to the start of `slots`, the inverse of [`Value::read`].
    ///
    /// A void value holds nothing (PROGS-16.1), so writing one leaves its slot unchanged.
    ///
    /// # Errors
    ///
    /// [`TooFewSlots`] when `slots` is shorter than [`Etype::slot_count`]; nothing is written.
    pub fn write(&self, slots: &mut [Slot]) -> Result<(), TooFewSlots> {
        let etype = self.etype();
        let needed = etype.slot_count();
        let too_few = TooFewSlots {
            etype,
            needed,
            found: slots.len(),
        };
        let cells = slots.get_mut(..needed).ok_or(too_few)?;
        let bits = match *self {
            Self::Void => return Ok(()),
            Self::Vector([x, y, z]) => {
                return match cells {
                    [cx, cy, cz] => {
                        *cx = Slot::from_f32(x);
                        *cy = Slot::from_f32(y);
                        *cz = Slot::from_f32(z);
                        Ok(())
                    }
                    _ => Err(too_few),
                };
            }
            Self::Float(value) => Slot::from_f32(value),
            Self::String(StringRef(value))
            | Self::Entity(EntityRef(value))
            | Self::Field(FieldRef(value))
            | Self::Function(FunctionRef(value))
            | Self::Pointer(PointerRef(value)) => Slot::from_i32(value),
        };
        let first = cells.first_mut().ok_or(too_few)?;
        *first = bits;
        Ok(())
    }
}
