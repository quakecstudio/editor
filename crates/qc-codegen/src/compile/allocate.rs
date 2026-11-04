// SPDX-License-Identifier: GPL-3.0-or-later

//! Temporary allocation, a pass over a body of the intermediate layer
//! (`docs/design/back-end.md`, "Allocation"): each value a statement makes gets a slot of its
//! own, never reused, taken in the order the statements make them (QC-21.5), a block of words
//! before the statements that write it. Today's FTEQCC profile allocates the same way; an
//! FTEQCC-exact mode that reuses slots as `QCC_GetTemp` does would be another pass here
//! (ADR-0020).

use std::collections::HashMap;

use qc_sema::ir::BodyStatement;

use crate::Internal;
use crate::program::Program;

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

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use qc_preproc::Op;
    use qc_sema::ir::{BodyStatement, Name};

    use super::CreationOrder;
    use crate::program::Program;

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
