// SPDX-License-Identifier: GPL-3.0-or-later

//! Where a reference to a definition is: the definition, the words into it the checker knows,
//! and an index that is known only as the program runs. FTEQCC keeps the same pair on every
//! reference (`QCC_ref_t::base` and `QCC_ref_t::index`) and every subscript, member and
//! component adds to the index (`QCC_PR_ParseRefArrayPointer`); here the pair is read from a
//! value in one place, [`Value::place`], so that a read, a store, an address and an increment
//! of a reference agree on where it is (FTEF-31.28, FTEF-31.32).

use crate::check::reference::eval_int;
use crate::check::{MemberStart, Reference, Sym, Value};
use crate::ir;
use crate::symbol::DefId;

/// Where a reference to a definition is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Place<'a> {
    /// The definition.
    def: DefId,
    /// Whether the definition is a constant (`sym->constant`).
    constant: bool,
    /// The words into the definition that are known: a member's word, an element's at a
    /// constant index, and those of what either is in.
    words: u32,
    /// The index that is no constant, in words after `words`.
    index: Option<&'a Value>,
}

impl<'a> Place<'a> {
    /// The index that is no constant, if the place has one.
    pub(crate) const fn index(&self) -> Option<&'a Value> {
        self.index
    }

    /// The words into the definition that are known.
    pub(crate) const fn words(&self) -> u32 {
        self.words
    }

    /// The definition from its first word, `ref->base` of a reference with an index.
    pub(crate) const fn whole(&self) -> ir::Name {
        ir::Name::Def(self.def, 0)
    }

    /// The definition at the known words, which an index that is no constant counts from.
    pub(crate) const fn start(&self) -> ir::Name {
        ir::Name::Def(self.def, self.words)
    }

    /// The word of the definition the place is, when no part of it waits for the program to
    /// run.
    pub(crate) fn word(&self) -> Option<ir::Name> {
        self.index.is_none().then(|| self.start())
    }

    /// The symbol of that word, as a value read from it has it.
    pub(crate) fn sym(&self) -> Option<Sym> {
        self.index
            .is_none()
            .then_some(Sym::Def(self.def, self.constant, self.words))
    }
}

impl Value {
    /// Where the value is in its definition, when it refers to one: a definition, a member of
    /// one, an element or a component of one, or an array named bare. `None` for a temporary,
    /// an immediate, and what is reached through a pointer, a field or an accessor.
    pub(crate) fn definition_place(&self) -> Option<Place<'_>> {
        let Sym::Def(def, constant, before) = self.sym else {
            return None;
        };
        let (words, index) = match &self.reference {
            Reference::Global | Reference::ArrayHead(None) => (0, None),
            Reference::Element(element) => match element.index.known().and_then(eval_int) {
                Some(words) => (u32::from_le_bytes(words.to_le_bytes()), None),
                None => (0, Some(&element.index)),
            },
            Reference::ArrayHead(Some(member)) if member.through.is_none() => match &member.start {
                MemberStart::Word(word) => (*word, None),
                MemberStart::Index(index) => (0, Some(index.as_ref())),
            },
            Reference::ArrayHead(Some(_))
            | Reference::Field(_)
            | Reference::Pointer(_)
            | Reference::Character(_)
            | Reference::Accessor(_) => return None,
        };
        Some(Place {
            def,
            constant,
            words: before.saturating_add(words),
            index,
        })
    }
}
