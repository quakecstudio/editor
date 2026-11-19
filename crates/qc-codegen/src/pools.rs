// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_ParseEntClass

//! Where FTEQCC holds each class's members (FTEF-9.29): a member a class above has is that
//! member's field, one named as an entity field is that field, and any other is laid into a
//! pool of fields that every class's members of its basic type share, at the first word no
//! member of that type of its class or a class above it takes. Classes apart from each other
//! so hold their members in the same fields.

use std::collections::HashMap;
use std::ops::Range;

use qc_sema::{DefId, DefKind, ModuleIndex, SimpleType, Type};
use qc_types::Etype;

use crate::Internal;
use crate::program::{ValueType, type_number, type_words};

/// How many classes a class may have above it before the walk stops, against a cycle.
const MAX_CLASS_DEPTH: usize = 64;

/// FTEQCC's `basictypenames`, by a basic type's number: what a pool's definitions are named
/// after, `::float[0]`.
const BASIC_TYPE_NAMES: [&str; 14] = [
    "void", "string", "float", "vector", "entity", "field", "function", "pointer", "integer",
    "uint", "long", "ulong", "double", "variant",
];

/// Where a class's member is held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    /// In the field of the member of its name and basic type that a class above has, or that
    /// its own class declared before it (`basicindex = pp[i].ofs; found = true;`).
    Member(DefId),
    /// In the entity field of its name (`d = QCC_PR_GetDef(NULL, parmname, NULL, 0, 0,
    /// GDF_CONST); if (d) basicindex = 0;`).
    EntityField(DefId),
    /// Nowhere: a member the model cannot size, or of a type FTEQCC has no pool for (it
    /// reads `*basictypes[newparm->type]` of a null for an accessor's, a truth value's or a
    /// narrow integer's, and stops there).
    Unsupported,
    /// In a pool.
    Pooled {
        /// Which of [`Pools::pools`].
        pool: usize,
        /// Its first word in the pool, `basicindex`.
        word: u32,
        /// The elements `::TYPE[N]` the pool grew by to hold it.
        grown: Range<u32>,
    },
}

/// Who made a class's field member.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Made {
    /// The program, which declares it.
    Declared,
    /// The compiler: a vector member's `_x`, `_y` or `_z`.
    Component,
}

/// A pool of fields, `::*TYPE`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pool {
    /// The basic type's name.
    pub(crate) name: &'static str,
    /// The type of one element's field.
    pub(crate) element: Type,
    /// The words of one element, `d->type->size`.
    pub(crate) element_words: u32,
    /// `d->arraysize`, which counts words and grows an element at a time.
    length: u32,
}

impl Pool {
    /// The fields the pool takes, and the globals that hold their offsets: `d->symbolsize =
    /// (d->arraysize?d->arraysize:1) * d->type->size` in `QCC_PR_FinishFieldDef`, which counts
    /// an element's words twice over.
    pub(crate) fn words(&self) -> Option<u32> {
        self.length.checked_mul(self.element_words)
    }
}

/// A member as a class lists it, `struct QCC_typeparam_s`.
struct Param<'a> {
    name: &'a str,
    /// Its basic type's number.
    number: u16,
    /// `ofs`: its first word among its class's members of its basic type.
    word: u32,
    /// The word after its last.
    end: u32,
    def: DefId,
}

/// Every class's members' slots, and the pools they are laid into in the order FTEQCC makes
/// them.
#[derive(Debug, Default)]
pub(crate) struct Pools {
    slots: HashMap<DefId, Slot>,
    pub(crate) pools: Vec<Pool>,
    /// The members that have a slot, in declaration order.
    pub(crate) members: Vec<DefId>,
    /// Each class's field members by name and by who made them.
    named: HashMap<(DefId, String, Made), DefId>,
}

impl Pools {
    /// The layout of the classes of `index`, each member taken in declaration order as
    /// `QCC_PR_ParseEntClass` reads it.
    ///
    /// # Errors
    ///
    /// [`Internal::TooLarge`] for a pool past what an offset holds.
    pub(crate) fn of(index: &ModuleIndex) -> Result<Self, Internal> {
        let mut layout = Self::default();
        let mut classes: HashMap<DefId, Vec<Param<'_>>> = HashMap::new();
        let mut numbers: HashMap<u16, usize> = HashMap::new();
        // The first entity field of each name, which a member declared after it is.
        let mut fields: HashMap<&str, DefId> = HashMap::new();
        for (id, definition) in index.definitions() {
            let Some(class) = definition.member_of else {
                if definition.kind == DefKind::Field {
                    fields.entry(definition.name.as_str()).or_insert(id);
                }
                continue;
            };
            if definition.kind == DefKind::Field {
                let made = if definition.generated {
                    Made::Component
                } else {
                    Made::Declared
                };
                let key = (class, definition.name.clone(), made);
                layout.named.entry(key).or_insert(id);
            }
            let Some(held) = member_type(index, id) else {
                continue;
            };
            let number = basic_number(index, &held);
            let name = BASIC_TYPE_NAMES
                .get(usize::from(number))
                .copied()
                .filter(|_| number != Etype::Void.number() && !index.is_unsized_array(id));
            let Some(name) = name else {
                layout.slots.insert(id, Slot::Unsupported);
                layout.members.push(id);
                continue;
            };
            let element_words = type_words(index, &ValueType::Of(held.clone()));
            let count = index.array_len(id).unwrap_or(1);
            // `for(pc = newt; pc && !found; pc = pc->parentclass)`: the first word past every
            // member of the type in the class and the classes above it, or the word of the
            // one that has the name.
            let mut word = 0;
            let mut found = None;
            let mut current = Some(class);
            'classes: for _ in 0..MAX_CLASS_DEPTH {
                let Some(at) = current else {
                    break;
                };
                for param in classes.get(&at).map_or(&[][..], Vec::as_slice) {
                    if param.number != number {
                        continue;
                    }
                    if param.name == definition.name {
                        word = param.word;
                        found = Some(param.def);
                        break 'classes;
                    }
                    word = word.max(param.end);
                }
                current = index.class_parent(at);
            }
            let end = element_words
                .checked_mul(count)
                .and_then(|words| words.checked_add(word))
                .ok_or(Internal::TooLarge)?;
            classes.entry(class).or_default().push(Param {
                name: &definition.name,
                number,
                word,
                end,
                def: id,
            });
            let slot = if let Some(member) = found {
                Slot::Member(member)
            } else if let Some(&field) = fields.get(definition.name.as_str()) {
                Slot::EntityField(field)
            } else {
                let pool = *numbers.entry(number).or_insert_with(|| {
                    layout.pools.push(Pool {
                        name,
                        element: element_type(index, &held),
                        element_words,
                        length: 0,
                    });
                    layout.pools.len().saturating_sub(1)
                });
                let grown = layout.grow(pool, word, count)?;
                Slot::Pooled { pool, word, grown }
            };
            layout.slots.insert(id, slot);
            layout.members.push(id);
        }
        Ok(layout)
    }

    /// `if (d->arraysize < basicindex+(arraysize?arraysize:1))`: the pool grown an element at
    /// a time until it reaches the member's first word and its count of elements, which is no
    /// count of words (`while(d->arraysize < newsize) { ... d->arraysize+=d->type->size; }`).
    /// The elements made.
    fn grow(&mut self, pool: usize, word: u32, count: u32) -> Result<Range<u32>, Internal> {
        let pool = self.pools.get_mut(pool).ok_or(Internal::NoDefinition)?;
        let size = word.checked_add(count).ok_or(Internal::TooLarge)?;
        let element = |length: u32| length.checked_div(pool.element_words);
        let first = element(pool.length).ok_or(Internal::Unsupported)?;
        while pool.length < size {
            pool.length = pool
                .length
                .checked_add(pool.element_words)
                .ok_or(Internal::TooLarge)?;
        }
        let last = element(pool.length).ok_or(Internal::Unsupported)?;
        Ok(first..last)
    }

    /// Where the member `id` is held, if it is one a pool's layout counts.
    pub(crate) fn slot(&self, id: DefId) -> Option<&Slot> {
        self.slots.get(&id)
    }

    /// The field member of `class` named `name` that is `made` so.
    pub(crate) fn member(&self, class: DefId, name: &str, made: Made) -> Option<DefId> {
        self.named.get(&(class, name.to_owned(), made)).copied()
    }
}

/// The type the class member `id` holds, a typedef's or an enum's read through as a
/// definition's is (FTE-15.6): `None` for what is no data or virtual member.
fn member_type(index: &ModuleIndex, id: DefId) -> Option<Type> {
    let definition = index.def(id)?;
    if definition.kind != DefKind::Field || definition.generated {
        return None;
    }
    let written = &definition.ty;
    Some(match index.alias(written) {
        Some(alias) if index.struct_shape(written).is_none() => alias.clone(),
        _ => written.clone(),
    })
}

/// Whether `held` is a class, which is an entity: a type that names one, and no pointer to it.
fn is_class(index: &ModuleIndex, held: &Type) -> bool {
    matches!(held, Type::Named(_)) && index.class_of(held).is_some()
}

/// `newparm->type`: the number of the basic type of `held`, a class an entity.
fn basic_number(index: &ModuleIndex, held: &Type) -> u16 {
    if is_class(index, held) {
        Etype::Entity.number()
    } else {
        type_number(index, &ValueType::Of(held.clone()))
    }
}

/// The type of a pool's element that holds `held`: an entity for a class.
fn element_type(index: &ModuleIndex, held: &Type) -> Type {
    if is_class(index, held) {
        Type::Simple(SimpleType::Entity)
    } else {
        held.clone()
    }
}
