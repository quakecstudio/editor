// SPDX-License-Identifier: GPL-3.0-or-later

//! Temporary allocation, a pass over a body of the intermediate layer
//! (`docs/design/back-end.md`, "Allocation"), as the dialect's profile chooses it (ADR-0020):
//! id's `qcc` gives each value a statement makes a slot of its own, never reused, taken in the
//! order the statements make them (QC-21.5); FTEQCC's `opt_overlaptemps`, on by default, reuses
//! a slot once nothing reads the value in it, from the module's shared temporaries, and gives a
//! value still read after a call a slot of the function's own block (FTE-21.8).

use std::collections::{HashMap, HashSet};

use qc_sema::ir::{BodyStatement, Effect, Name};

use super::body::call_count;

use crate::Internal;
use crate::program::Program;

/// The allocation pass of one body, as the profile chooses it.
#[derive(Debug)]
pub(super) enum Slots {
    /// A new slot for each value (QC-21.5).
    CreationOrder(CreationOrder),
    /// Slots reused once their values are dead (FTE-21.8).
    Overlapping(Overlapping),
}

impl Slots {
    /// The temporary `statement`, at `index`, takes as it is generated, if it takes one.
    pub(super) fn statement(
        &mut self,
        program: &mut Program,
        statement: &BodyStatement,
        index: u32,
    ) -> Result<(), Internal> {
        match self {
            Self::CreationOrder(order) => order.statement(program, statement, index),
            Self::Overlapping(overlapping) => match statement.temp_words(index) {
                Some(words) => overlapping.take(program, index, words),
                None => Ok(()),
            },
        }
    }

    /// A block of `words` words taken before the statement at `index`, whose value it is.
    pub(super) fn block(
        &mut self,
        program: &mut Program,
        index: u32,
        words: u32,
    ) -> Result<(), Internal> {
        match self {
            Self::CreationOrder(order) => order.block(program, index, words),
            Self::Overlapping(overlapping) => overlapping.take(program, index, words),
        }
    }

    /// The temporary of `words` words a dropped statement took before the statement at
    /// `index`: a slot under id's allocation (QC-21.5); under reuse a shared one of its size,
    /// taken and freed as FTEQCC took it before rewinding, which nothing reads (FTE-21.8).
    pub(super) fn dropped(
        &mut self,
        program: &mut Program,
        index: u32,
        words: u32,
    ) -> Result<(), Internal> {
        match self {
            Self::CreationOrder(_) => program.allocate(words).map(|_| ()),
            Self::Overlapping(overlapping) => overlapping
                .shared_place(program, index, words, None)
                .map(|_| ()),
        }
    }

    /// Whether the value the statement at `made` made is in one of the module's shared
    /// temporaries, which are laid out last (FTE-21.8).
    pub(super) fn is_shared(&self, made: u32) -> bool {
        match self {
            Self::CreationOrder(_) => false,
            Self::Overlapping(overlapping) => overlapping.shared.contains(&made),
        }
    }

    /// The first global of the value the statement at `made` made.
    pub(super) fn slot(&self, made: u32) -> Option<u32> {
        match self {
            Self::CreationOrder(order) => order.slot(made),
            Self::Overlapping(overlapping) => overlapping.slots.get(&made).copied(),
        }
    }
}

/// The slots of the values a body's statements make, by the index of the statement that makes
/// each, taken from the program in creation order.
#[derive(Debug, Default)]
pub(super) struct CreationOrder {
    slots: HashMap<u32, u32>,
}

impl CreationOrder {
    /// The temporary `statement`, at `index`, takes as it is generated, if it takes one: a new
    /// slot of its value's words.
    pub(super) fn statement(
        &mut self,
        program: &mut Program,
        statement: &BodyStatement,
        index: u32,
    ) -> Result<(), Internal> {
        if let Some(words) = statement.temp_words(index) {
            let slot = program.allocate(words)?;
            self.slots.insert(index, slot);
        }
        Ok(())
    }

    /// A block of `words` words taken before the statement at `index`, whose value it is.
    pub(super) fn block(
        &mut self,
        program: &mut Program,
        index: u32,
        words: u32,
    ) -> Result<(), Internal> {
        let slot = program.allocate(words)?;
        self.slots.insert(index, slot);
        Ok(())
    }

    /// The first global of the value the statement at `made` made.
    pub(super) fn slot(&self, made: u32) -> Option<u32> {
        self.slots.get(&made).copied()
    }
}

/// FTE-21.8: the slots of a body's values when they are reused, from where each value is made
/// to the last statement that names it.
#[derive(Debug)]
pub(super) struct Overlapping {
    /// The last statement that names each value, by the statement that makes it, a loop's
    /// back jump for a value live where the loop starts again.
    last: HashMap<u32, u32>,
    /// The values still to be read after a call: `QCC_LockActiveTemps` keeps them in the
    /// function's own block.
    locked: Vec<u32>,
    /// Up to which statement each of the module's shared slots, by its place in
    /// [`Program::shared_temps`], is held in this body.
    held: Vec<Option<u32>>,
    /// The slot each value was given.
    slots: HashMap<u32, u32>,
    /// The values given a shared slot.
    shared: HashSet<u32>,
}

impl Overlapping {
    /// The live ranges of the values `generated` makes, `effects` its blocks among them: each
    /// from its statement to the last that names it, then, until nothing changes, to the back
    /// jump of each loop it is live at the start of; a value whose range has a call strictly
    /// inside it is locked.
    pub(super) fn new(generated: &[BodyStatement], effects: &[(u32, Effect)]) -> Self {
        let mut last: HashMap<u32, u32> = HashMap::new();
        for (index, statement) in (0_u32..).zip(generated) {
            if statement.temp_words(index).is_some() {
                last.entry(index).or_insert(index);
            }
        }
        for &(at, effect) in effects {
            if let Effect::Block(_) = effect {
                last.entry(at).or_insert(at);
            }
        }
        let mut loops = Vec::new();
        let mut calls = Vec::new();
        for (index, statement) in (0_u32..).zip(generated) {
            for name in [statement.a, statement.b, statement.c]
                .into_iter()
                .flatten()
            {
                match name {
                    Name::Made(made) | Name::MadeWord(made, _) => {
                        let end = last.entry(made).or_insert(index);
                        *end = (*end).max(index);
                    }
                    Name::Jump(target) if target <= index => loops.push((target, index)),
                    _ => {}
                }
            }
            if call_count(statement.op).is_some() {
                calls.push(index);
            }
        }
        // A value live where a loop starts again is read on the next pass: it lives to the
        // loop's back jump.
        let mut changed = true;
        while changed {
            changed = false;
            for &(start, back) in &loops {
                for (&made, end) in &mut last {
                    if made < start && *end >= start && *end < back {
                        *end = back;
                        changed = true;
                    }
                }
            }
        }
        // `calls` is in statement order: the first after a value's statement decides.
        let mut locked: Vec<u32> = last
            .iter()
            .filter(|&(&made, &end)| {
                let first = calls.partition_point(|&call| call <= made);
                calls.get(first).is_some_and(|&call| call < end)
            })
            .map(|(&made, _)| made)
            .collect();
        locked.sort_unstable();
        Self {
            last,
            locked,
            held: Vec::new(),
            slots: HashMap::new(),
            shared: HashSet::new(),
        }
    }

    /// The slot of `words` words the value made at `index` takes. Each takes the first of the
    /// module's shared slots of its size that no value of this body holds at `index`, the
    /// operands of the statement that makes it included, or a new shared one after the last
    /// (`QCC_GetTemp`), and holds it to its last statement; a locked value then takes a slot
    /// of the function's block after its locals instead, as `QCC_RemapLockedTemps` moves it
    /// once the function ends, its shared one still taken.
    fn take(&mut self, program: &mut Program, index: u32, words: u32) -> Result<(), Internal> {
        let end = self.last.get(&index).copied().unwrap_or(index);
        let shared = self.shared_place(program, index, words, Some(end))?;
        let slot = if self.locked.binary_search(&index).is_ok() {
            program.locked_temp(words)?
        } else {
            self.shared.insert(index);
            shared
        };
        self.slots.insert(index, slot);
        Ok(())
    }

    /// The first of the module's shared slots of `words` words that no value of this body holds
    /// at `index`, or a new one after the last, held to `until` when given; returns its virtual
    /// offset.
    fn shared_place(
        &mut self,
        program: &mut Program,
        index: u32,
        words: u32,
        until: Option<u32>,
    ) -> Result<u32, Internal> {
        let shared = program.shared_temps();
        self.held.resize(shared.len(), None);
        let free = shared
            .iter()
            .zip(&self.held)
            .position(|(&(_, size), held)| size == words && held.is_none_or(|end| end < index));
        let (place, slot) = match free.and_then(|p| shared.get(p).map(|&(slot, _)| (p, slot))) {
            Some(found) => found,
            None => {
                let place = shared.len();
                let slot = program.shared_temp(words)?;
                self.held.push(None);
                (place, slot)
            }
        };
        if let (Some(until), Some(held)) = (until, self.held.get_mut(place)) {
            *held = Some(until);
        }
        Ok(slot)
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use qc_preproc::Op;
    use qc_sema::ir::{BodyStatement, Effect, Name};

    use super::{CreationOrder, Overlapping, Slots};
    use crate::program::Program;

    /// `op` making the value of the statement at `index` from `a` and `b`.
    fn makes(op: Op, index: u32, a: Option<Name>, b: Option<Name>) -> BodyStatement {
        BodyStatement {
            a,
            b,
            c: Some(Name::Made(index)),
            ..BodyStatement::bare(op)
        }
    }

    /// `op` reading `a` and making nothing of its own.
    fn reads(op: Op, a: Option<Name>) -> BodyStatement {
        BodyStatement {
            a,
            ..BodyStatement::bare(op)
        }
    }

    /// The slots `body` gives each statement's value, written into `program` with `effects`.
    fn allocate(
        program: &mut Program,
        body: &[BodyStatement],
        effects: &[(u32, Effect)],
    ) -> Vec<Option<u32>> {
        let mut slots = Slots::Overlapping(Overlapping::new(body, effects));
        for (index, statement) in (0_u32..).zip(body) {
            for &(at, effect) in effects {
                if let (true, Effect::Block(words)) = (at == index, effect) {
                    slots.block(program, index, words).unwrap();
                }
            }
            slots.statement(program, statement, index).unwrap();
        }
        (0_u32..).take(body.len()).map(|i| slots.slot(i)).collect()
    }

    // spec FTE-21.8: a value takes the first shared slot of its size no value holds where it is
    // made, its own statement's operands still held there; a slot of another size is not taken
    #[test]
    fn a_dead_values_slot_is_taken_again() {
        let mut program = Program::new();
        let body = [
            makes(Op::MulF, 0, None, None),
            makes(Op::AddF, 1, Some(Name::Made(0)), None),
            reads(Op::StoreF, Some(Name::Made(1))),
            makes(Op::MulF, 3, None, None),
            makes(Op::MulVf, 4, Some(Name::Made(3)), None),
            reads(Op::StoreV, Some(Name::Made(4))),
            makes(Op::MulVf, 6, None, None),
        ];
        let slots = allocate(&mut program, &body, &[]);
        assert_ne!(slots[1], slots[0], "the operand is still held");
        assert_eq!(slots[3], slots[0], "the first value is dead");
        assert!(
            slots[4].is_some() && slots[4] != slots[1],
            "a vector takes no float's slot"
        );
        assert_eq!(
            slots[6], slots[4],
            "a dead vector's slot is a vector's again"
        );
        let shared: Vec<u32> = program
            .shared_temps()
            .iter()
            .map(|&(_, words)| words)
            .collect();
        assert_eq!(shared, [1, 1, 3], "three shared slots in all");
    }

    // spec FTE-21.8: a value still read after a call is read from a slot of the function's own
    // block, not a shared one, as QCC_LockActiveTemps and QCC_RemapLockedTemps keep it
    #[test]
    fn a_value_read_after_a_call_is_the_functions_own() {
        let mut program = Program::new();
        let body = [
            makes(Op::MulF, 0, None, None),
            makes(Op::MulF, 1, None, None),
            reads(Op::Call0, None),
            reads(Op::StoreF, Some(Name::Made(0))),
            reads(Op::StoreF, Some(Name::Made(1))),
        ];
        let first = program.allocate(0).unwrap();
        let slots = allocate(&mut program, &body, &[]);
        assert_eq!(slots[0], Some(first), "a new slot of the program's own");
        assert_eq!(slots[1], Some(first + 1));
        let shared: Vec<u32> = program.shared_temps().iter().map(|&(ofs, _)| ofs).collect();
        assert_eq!(
            shared.len(),
            2,
            "each still takes a shared slot, as QCC_GetTemp did"
        );
        assert!(slots.iter().flatten().all(|slot| !shared.contains(slot)));
        let before = [
            makes(Op::MulF, 0, None, None),
            reads(Op::StoreF, Some(Name::Made(0))),
            reads(Op::Call0, None),
        ];
        let slots = allocate(&mut program, &before, &[]);
        assert_eq!(
            slots[0],
            program.shared_temps().first().map(|&(ofs, _)| ofs),
            "read before the call: shared"
        );
    }

    // spec FTE-21.8: a value live where a loop starts again is held to the loop's back jump
    #[test]
    fn a_value_live_at_a_loops_start_is_held_through_the_loop() {
        let mut program = Program::new();
        let body = [
            makes(Op::MulF, 0, None, None),
            reads(Op::StoreF, Some(Name::Made(0))),
            makes(Op::MulF, 2, None, None),
            reads(Op::StoreF, Some(Name::Made(2))),
            reads(Op::Goto, Some(Name::Jump(1))),
        ];
        let slots = allocate(&mut program, &body, &[]);
        assert_ne!(
            slots[2], slots[0],
            "the first value is read again on the next pass"
        );
        let straight = [
            makes(Op::MulF, 0, None, None),
            reads(Op::StoreF, Some(Name::Made(0))),
            makes(Op::MulF, 2, None, None),
        ];
        let slots = allocate(&mut program, &straight, &[]);
        assert_eq!(slots[2], slots[0], "with no loop it is dead");
    }

    // spec FTE-21.8: a block's value is held to the last statement that names a word of it, and
    // the shared slots are the module's: a later body takes them again from the first
    #[test]
    fn blocks_are_held_and_the_shared_slots_are_the_modules() {
        let mut program = Program::new();
        let word = |w| Some(Name::MadeWord(0, w));
        let body = [
            BodyStatement {
                c: word(0),
                ..BodyStatement::bare(Op::StoreF)
            },
            makes(Op::MulVf, 1, None, None),
            reads(Op::StoreF, word(2)),
        ];
        let slots = allocate(&mut program, &body, &[(0, Effect::Block(3))]);
        assert!(
            slots[0].is_some() && slots[1] != slots[0],
            "the block is still held"
        );
        let next = allocate(&mut program, &[makes(Op::MulVf, 0, None, None)], &[]);
        assert_eq!(next[0], slots[0], "a new body starts with every slot free");
    }

    // spec FTE-21.8: a temporary a dropped statement took is taken and freed among the shared
    // slots, as FTEQCC took it before rewinding, so the next value may take it again; under
    // id's allocation it is a new slot (QC-21.5)
    #[test]
    fn a_dropped_temporary_is_taken_and_freed() {
        let mut program = Program::new();
        let first = program.allocate(0).unwrap();
        let mut overlapping = Slots::Overlapping(Overlapping::new(&[], &[]));
        overlapping.dropped(&mut program, 0, 3).unwrap();
        assert_eq!(program.allocate(0).unwrap(), first, "no global of its own");
        let shared: Vec<u32> = program.shared_temps().iter().map(|&(_, w)| w).collect();
        assert_eq!(shared, [3], "a shared slot of its size");
        overlapping.dropped(&mut program, 0, 3).unwrap();
        assert_eq!(
            program.shared_temps().len(),
            1,
            "freed: the next takes it again"
        );
        let mut order = Slots::CreationOrder(CreationOrder::default());
        order.dropped(&mut program, 0, 3).unwrap();
        assert_eq!(program.allocate(0).unwrap(), first + 3);
    }

    // spec QC-21.5: each value a statement makes takes a new slot, never reused, in the order
    // the statements make them: a vector's three words, a float's one, a block its words taken
    // before the statements that write it; a statement that makes no value takes none
    #[test]
    fn each_value_takes_a_new_slot_in_creation_order() {
        let made = |op, index| BodyStatement {
            c: Some(Name::Made(index)),
            ..BodyStatement::bare(op)
        };
        let mut program = Program::new();
        let first = program.allocate(0).unwrap();
        let mut order = CreationOrder::default();
        order
            .statement(&mut program, &made(Op::MulVf, 0), 0)
            .unwrap();
        order.block(&mut program, 1, 2).unwrap();
        let store = BodyStatement {
            b: Some(Name::MadeWord(1, 0)),
            ..BodyStatement::bare(Op::StoreF)
        };
        order.statement(&mut program, &store, 1).unwrap();
        order
            .statement(&mut program, &made(Op::AddF, 2), 2)
            .unwrap();
        assert_eq!(
            [order.slot(0), order.slot(1), order.slot(2)],
            [Some(first), Some(first + 3), Some(first + 5)]
        );
        assert_eq!(program.allocate(0).unwrap(), first + 6, "no slot is reused");
        order
            .statement(&mut program, &BodyStatement::bare(Op::Goto), 3)
            .unwrap();
        assert_eq!(order.slot(3), None);
    }
}
