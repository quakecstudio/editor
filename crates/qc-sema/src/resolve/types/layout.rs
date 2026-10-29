// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_ParseStruct
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qccmain.c, QCC_PR_NewType

//! The size of a type as FTEQCC lays it out (FTEF-6.13): a basic type's words from `type_size`,
//! the 8- and 16-bit integers' bits, and a struct's or union's members placed bit by bit with
//! their alignment, bitfields packed.

use qc_syntax::c_scalar::CScalar;

use crate::check::etype::Etype;
use crate::lower::StructMember;
use crate::resolve::{MAX_CLASS_DEPTH, Resolver};
use crate::ty::Type;

/// The bits of a word.
const WORD_BITS: u32 = 32;
/// The bits of a word's offset within it, `31`.
const WORD_MASK: u32 = 31;
/// The shift from bits to words, `>>5`.
const WORD_SHIFT: u32 = 5;

/// A type's size, `type->size`, `type->bits` and `type->align`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Layout {
    /// The words it takes, `size`.
    pub(crate) words: u32,
    /// Its bits when it is narrower than its words or not word-aligned, `bits`; 0 otherwise.
    pub(crate) bits: u32,
    /// Its alignment in bits, `align`.
    pub(crate) align: u32,
}

impl Layout {
    /// A type of `words` words, word-aligned (`QCC_PR_NewType`).
    const fn words(words: u32) -> Self {
        Self {
            words,
            bits: 0,
            align: WORD_BITS,
        }
    }

    /// An integer of `bits` bits in a word, aligned to its bits (`type_sint8` and the like).
    const fn narrow(bits: u32) -> Self {
        Self {
            words: 1,
            bits,
            align: bits,
        }
    }
}

/// `x` rounded up to a multiple of `to`, a power of two.
fn align_up(x: u32, to: u32) -> u32 {
    let mask = to.wrapping_sub(1);
    x.wrapping_add(mask) & !mask
}

impl Resolver<'_> {
    /// The layout of `ty`, `None` for a type whose size the model does not follow: one it does
    /// not know, a struct with no body, or one whose array sizes or bitfield widths are no
    /// literals.
    pub(in crate::resolve) fn layout(&self, ty: &Type) -> Option<Layout> {
        self.layout_at(ty, MAX_CLASS_DEPTH)
    }

    fn layout_at(&self, ty: &Type, depth: usize) -> Option<Layout> {
        let depth = depth.checked_sub(1)?;
        let ty = self.resolved(ty);
        match ty.fteqcc_scalar() {
            Some(CScalar::Int8 | CScalar::Uint8) => return Some(Layout::narrow(8)),
            Some(CScalar::Int16 | CScalar::Uint16) => return Some(Layout::narrow(16)),
            _ => {}
        }
        let shape = self.shape(&ty)?;
        let words = match shape.etype {
            Etype::Vector | Etype::Variant => 3,
            Etype::Int64 | Etype::Uint64 | Etype::Double => 2,
            Etype::Struct | Etype::Union => return self.struct_layout(&ty, depth),
            Etype::Enum => {
                return match shape.holds? {
                    Etype::Vector | Etype::Variant => Some(Layout::words(3)),
                    Etype::Int64 | Etype::Uint64 | Etype::Double => Some(Layout::words(2)),
                    _ => Some(Layout::words(1)),
                };
            }
            Etype::Void
            | Etype::String
            | Etype::Float
            | Etype::Entity
            | Etype::Field
            | Etype::Function
            | Etype::Pointer
            | Etype::Integer
            | Etype::Uint
            | Etype::Boolean => 1,
        };
        Some(Layout::words(words))
    }

    /// `QCC_PR_ParseStruct`'s layout of the struct or union `ty`: the parent's words first;
    /// each member that is no `static` and no member of the parent again placed in turn, a
    /// struct's after the last, aligned to its type (a bitfield only where it would cross its
    /// alignment, any other to a word), a union's all at 0; then the tail padded to the largest
    /// alignment.
    fn struct_layout(&self, ty: &Type, depth: usize) -> Option<Layout> {
        self.struct_placement(ty, depth, None)
            .map(|(layout, _)| layout)
    }

    /// The bit a struct or union member `name` starts at from the struct's start, through the
    /// parents, as [`Self::struct_layout`] places it.
    pub(in crate::resolve) fn member_position(&self, ty: &Type, name: &str) -> Option<u32> {
        self.struct_placement(&self.resolved(ty), MAX_CLASS_DEPTH, Some(name))
            .and_then(|(_, at)| at)
    }

    /// [`Self::struct_layout`], and where the member `find` starts, in bits from the struct's
    /// start, when it names one.
    fn struct_placement(
        &self,
        ty: &Type,
        depth: usize,
        find: Option<&str>,
    ) -> Option<(Layout, Option<u32>)> {
        let Type::Named(name) = ty else {
            return None;
        };
        let key = self.type_key(self.struct_type_name(name)?);
        let def = self.struct_defs.get(&key)?;
        if !def.defined {
            return None;
        }
        let (parent, mut found) = match &def.parent {
            Some(parent) => {
                let parent = Type::Named(parent.as_str().into());
                match find {
                    Some(_) => self.struct_placement(&parent, depth, find)?,
                    None => (self.layout_at(&parent, depth)?, None),
                }
            }
            None => (Layout::words(0), None),
        };
        // `unsigned int bitsize`, whose arithmetic wraps as C's does (FTEF-6.27).
        let mut bitsize = parent.words.wrapping_mul(WORD_BITS);
        let mut bitalign = 0;
        for member in &def.members {
            if member.is_static
                || def
                    .parent
                    .as_ref()
                    .is_some_and(|p| self.parent_has_member(p, member))
            {
                continue;
            }
            let layout = self.member_layout(member, depth)?;
            let count = array_count(member)?;
            bitalign = if layout.align == 0 {
                WORD_BITS
            } else {
                bitalign.max(layout.align)
            };
            let span = if layout.bits == 0 {
                WORD_BITS.wrapping_mul(layout.words).wrapping_mul(count)
            } else {
                layout.bits.wrapping_mul(count)
            };
            let wanted =
                found.is_none() && find.is_some_and(|n| self.same_member_name(&member.name, n));
            if def.union {
                if wanted {
                    found = Some(parent.words.wrapping_mul(WORD_BITS));
                }
                bitsize = bitsize.max(span);
                continue;
            }
            if layout.bits == 0 {
                bitsize = align_up(bitsize, WORD_BITS);
            } else if member.bits.is_some() && layout.bits < layout.align {
                // A bitfield is realigned only where it would cross its parent's alignment.
                let within = bitsize & layout.align.checked_sub(1)?;
                if within.wrapping_add(layout.bits) > layout.align {
                    bitsize = align_up(bitsize, layout.align);
                }
            } else {
                bitsize = align_up(bitsize, layout.align);
            }
            if wanted {
                found = Some(bitsize);
            }
            bitsize = bitsize.wrapping_add(span);
        }
        if bitalign != 0 {
            bitsize = align_up(bitsize, bitalign);
        }
        let layout = Layout {
            words: bitsize.wrapping_add(WORD_MASK) >> WORD_SHIFT,
            bits: if bitalign & WORD_MASK == 0 {
                0
            } else {
                bitsize
            },
            align: bitalign,
        };
        Some((layout, found))
    }

    /// A member's type's layout, a bitfield's `ev_bitfld` of its width: the size and alignment
    /// of its integer, an `int` or `unsigned` wider than 32 bits made 64 bits.
    fn member_layout(&self, member: &StructMember, depth: usize) -> Option<Layout> {
        let layout = self.layout_at(&member.ty, depth)?;
        let Some(width) = member.bits else {
            return Some(layout);
        };
        let bits = width?;
        // `if (bits > 32 && ...)` of the signed `int bits`.
        let words = if i32::from_le_bytes(bits.to_le_bytes()) > 32 && layout.words == 1 {
            2
        } else {
            layout.words
        };
        Some(Layout {
            words,
            bits,
            align: layout.align,
        })
    }
}

/// The elements of a member, `arraysize`, 1 for no array: the product of its sizes, each a
/// number.
fn array_count(member: &StructMember) -> Option<u32> {
    member.array.iter().try_fold(1_u32, |count, size| {
        let size = size.trim();
        let size = size.strip_prefix('[').unwrap_or(size);
        let size = size.strip_suffix(']').unwrap_or(size);
        count.checked_mul(size.trim().parse::<u32>().ok()?)
    })
}
