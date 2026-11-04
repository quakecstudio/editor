// SPDX-License-Identifier: GPL-3.0-or-later

//! Temporary allocation, a pass over a fragment of the intermediate layer
//! (`docs/design/back-end.md`, "Allocation"): every temporary gets a slot of its own, never
//! reused, taken in the order the lowering made the temporaries (QC-21.5). Today's FTEQCC
//! profile allocates the same way; an FTEQCC-exact mode that reuses slots as `QCC_GetTemp`
//! does would be another pass here (ADR-0020).

use qc_sema::ir;
use qc_types::Etype;

use super::Step;
use crate::Internal;
use crate::program::Program;

/// A temporary's slot: its first global and its basic type.
pub(super) type Slot = (u32, Etype);

/// Each of `fragment`'s temporaries a new slot from `program`, in order: a vector taken before
/// its words are written three words, a statement's value the words of its row's result.
pub(super) fn creation_order(
    fragment: &ir::Fragment,
    steps: &[Step],
    program: &mut Program,
) -> Result<Vec<Slot>, Internal> {
    let mut slots = Vec::with_capacity(fragment.temps.len());
    for temp in &fragment.temps {
        let etype = match *temp {
            ir::Temp::Vector(_) => Etype::Vector,
            ir::Temp::Made(index) => match steps.get(usize::from(index)) {
                Some(Step::Row(row)) => row.c,
                Some(Step::Parameter | Step::Call(_)) | None => return Err(Internal::Unlowered),
            },
        };
        let words = if etype == Etype::Vector { 3 } else { 1 };
        slots.push((program.allocate(words)?, etype));
    }
    Ok(slots)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use qc_progs::opcode::Opcode;
    use qc_sema::ir::{Fragment, Temp};
    use qc_types::Etype;

    use super::{Step, creation_order};
    use crate::ops;
    use crate::program::Program;

    // spec QC-21.5: each temporary gets a new slot, never reused, in the order the lowering
    // took them: a vector taken before its words are written three words, a value its row's
    // result's
    #[test]
    fn each_temporary_takes_a_new_slot_in_creation_order() {
        let row = |opcode| {
            ops::ROWS
                .iter()
                .find(|row| row.opcode == opcode)
                .copied()
                .unwrap()
        };
        let steps = [
            Step::Row(row(Opcode::MulVf)),
            Step::Row(row(Opcode::BitAnd)),
        ];
        let fragment = Fragment {
            temps: vec![Temp::Made(0), Temp::Vector(1), Temp::Made(1)],
            ..Fragment::default()
        };
        let mut program = Program::new();
        let first = program.allocate(0).unwrap();
        let slots = creation_order(&fragment, &steps, &mut program).unwrap();
        assert_eq!(
            slots,
            [
                (first, Etype::Vector),
                (first + 3, Etype::Vector),
                (first + 6, Etype::Float),
            ]
        );
        assert_eq!(program.allocate(0).unwrap(), first + 7, "no slot is reused");
        let call = [Step::Call(Opcode::Call2)];
        let made = Fragment {
            temps: vec![Temp::Made(0)],
            ..Fragment::default()
        };
        assert!(creation_order(&made, &call, &mut program).is_err());
    }
}
