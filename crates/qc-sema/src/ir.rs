// SPDX-License-Identifier: GPL-3.0-or-later

//! The intermediate layer (`docs/design/back-end.md`): the statements the checker lowers a
//! construct to, with operands that are values rather than global offsets, for the code
//! generator to lay out and write. An opcode the target lacks is already emulated here
//! (FTEF-27), once, where the checker also raises the warnings the emulation gives.
//!
//! So far it holds the binary operators: each one's [`Fragment`], the statements
//! `QCC_PR_StatementFlags` emits for it, its operands' conversions included (FTEF-23.9).

use std::collections::HashMap;

/// What a statement reads or what a fragment's value is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Operand {
    /// The fragment's input `n`: the left (0) or right (1) operand of its operator, as the code
    /// generator holds it.
    Input(u8),
    /// The fragment's temporary `n`, in the order the lowering took them.
    Temp(u16),
    /// An immediate constant (FTEF-26): its kind and its words.
    Immediate(Immediate),
    /// One word of a vector input, temporary or immediate: `var.ofs++` (FTE-9.3).
    Word(Whole, u8),
}

/// A value whose words a [`Operand::Word`] names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Whole {
    /// The fragment's input `n`.
    Input(u8),
    /// The fragment's temporary `n`.
    Temp(u16),
    /// An immediate.
    Immediate(Immediate),
}

impl Operand {
    /// Word `word` of this operand, a vector.
    #[must_use]
    pub const fn word(self, word: u8) -> Option<Self> {
        let whole = match self {
            Self::Input(n) => Whole::Input(n),
            Self::Temp(n) => Whole::Temp(n),
            Self::Immediate(immediate) => Whole::Immediate(immediate),
            Self::Word(..) => return None,
        };
        Some(Self::Word(whole, word))
    }
}

/// Where a statement writes, its operand c.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Destination {
    /// Nowhere: a store, a jump, an opcode of no result.
    Nothing,
    /// A new temporary of its opcode's result type, the fragment's next.
    Value,
    /// A word of a vector temporary of the fragment, by the temporary's number and the
    /// word's, taken before (`QCC_GetTemp(type_vector)`, [`Temp::Vector`]).
    Word(u16, u8),
}

/// A constant a fragment names: the words of its value and the kind of immediate that holds
/// them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Immediate {
    /// The kind of immediate.
    pub kind: ImmediateKind,
    /// Its words: one, or three for a vector.
    pub words: [u32; 3],
}

/// The kinds of immediate a fragment names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImmediateKind {
    /// A float.
    Float,
    /// A vector.
    Vector,
    /// An integer (FTEF-26.3).
    Integer,
}

/// One statement: its opcode's number (`pr_opcodes`), its operands, and where it writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Statement {
    /// The opcode's number.
    pub op: u16,
    /// Operand a.
    pub a: Option<Operand>,
    /// Operand b.
    pub b: Option<Operand>,
    /// Operand c.
    pub c: Destination,
}

/// A temporary of a fragment: one a statement made, or a vector taken before its words are
/// written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Temp {
    /// The value of the statement at this index.
    Made(u16),
    /// A vector taken (`QCC_GetTemp(type_vector)`) before the statement at this index.
    Vector(u16),
}

/// The statements a construct lowers to, in emission order, the immediates its lowering made,
/// and the operand its value is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fragment {
    /// The statements.
    pub statements: Vec<Statement>,
    /// The immediates the lowering made, in order: a constant an emulation names
    /// (`QCC_MakeFloatConst`) and each folded value, which FTEQCC makes a global of whether or
    /// not a statement reads it (FTEF-26.2).
    pub immediates: Vec<Immediate>,
    /// The temporaries, in the order the lowering took them, which [`Operand::Temp`] numbers.
    pub temps: Vec<Temp>,
    /// The construct's value: an input, a temporary, or a constant.
    pub value: Option<Operand>,
}

/// A function body's fragments, by the span of the token each was lowered at.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fragments {
    by_span: HashMap<(usize, usize), Fragment>,
}

impl Fragments {
    /// The fragment lowered at `span`, if one was.
    #[must_use]
    pub fn at(&self, span: qc_diag::Span) -> Option<&Fragment> {
        self.by_span.get(&(span.start, span.end))
    }

    pub(crate) fn insert(&mut self, span: qc_diag::Span, fragment: Fragment) {
        self.by_span.insert((span.start, span.end), fragment);
    }
}

/// A fragment being lowered: the statements so far, or nothing when a step was one the layer
/// does not hold yet, which the code generator then lowers itself.
#[derive(Clone, Debug, Default)]
pub(crate) struct Builder {
    statements: Vec<Statement>,
    immediates: Vec<Immediate>,
    temps: Vec<Temp>,
    held: bool,
}

impl Builder {
    /// A builder holding nothing yet.
    pub(crate) const fn new() -> Self {
        Self {
            statements: Vec::new(),
            immediates: Vec::new(),
            temps: Vec::new(),
            held: true,
        }
    }

    /// Adds the statement `op` of the operands it `reads`, each `None` when the opcode has no
    /// such operand and `Some(None)` when the fragment cannot name it, which leaves the
    /// fragment one the layer does not hold, writing to `c`; the operand of the temporary it
    /// makes, when it makes one.
    pub(crate) fn push(
        &mut self,
        op: u16,
        reads: [Option<Option<Operand>>; 2],
        c: Destination,
    ) -> Option<Operand> {
        let [a, b] = reads;
        let (Ok(index), Ok(temp)) = (
            u16::try_from(self.statements.len()),
            u16::try_from(self.temps.len()),
        ) else {
            self.held = false;
            return None;
        };
        if a == Some(None) || b == Some(None) {
            self.held = false;
            return None;
        }
        self.statements.push(Statement {
            op,
            a: a.flatten(),
            b: b.flatten(),
            c,
        });
        if c != Destination::Value {
            return None;
        }
        self.temps.push(Temp::Made(index));
        Some(Operand::Temp(temp))
    }

    /// Takes a vector temporary before the next statement, whose words statements then write
    /// ([`Destination::Word`]); its number.
    pub(crate) fn vector(&mut self) -> Option<u16> {
        let (Ok(index), Ok(temp)) = (
            u16::try_from(self.statements.len()),
            u16::try_from(self.temps.len()),
        ) else {
            self.held = false;
            return None;
        };
        self.temps.push(Temp::Vector(index));
        Some(temp)
    }

    /// Records an immediate the lowering made.
    pub(crate) fn made(&mut self, immediate: Immediate) {
        self.immediates.push(immediate);
    }

    /// Marks the fragment as one the layer does not hold.
    pub(crate) const fn give_up(&mut self) {
        self.held = false;
    }

    /// The fragment, valued `value`, if every step was held.
    pub(crate) fn finish(self, value: Option<Operand>) -> Option<Fragment> {
        self.held.then_some(Fragment {
            statements: self.statements,
            immediates: self.immediates,
            temps: self.temps,
            value,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Builder, Destination, Fragment, Operand, Statement, Temp};

    // A statement's value is the next temporary and a vector taken before its words is one too;
    // a statement writing nothing or a word names none; and an operand the fragment cannot name
    // leaves it unheld.
    #[test]
    fn a_builder_numbers_temporaries_and_refuses_an_unnamed_operand() {
        let mut builder = Builder::new();
        let inputs = [Some(Some(Operand::Input(0))), Some(Some(Operand::Input(1)))];
        let made = builder.push(6, inputs, Destination::Value);
        assert_eq!(made, Some(Operand::Temp(0)));
        assert_eq!(builder.vector(), Some(1));
        assert_eq!(builder.push(64, inputs, Destination::Word(1, 2)), None);
        assert_eq!(
            builder.push(31, [made.map(Some), inputs[1]], Destination::Nothing),
            None
        );
        let statement = |op, a, b, c| Statement {
            op,
            a: Some(a),
            b: Some(b),
            c,
        };
        assert_eq!(
            builder.clone().finish(Some(Operand::Temp(1))),
            Some(Fragment {
                statements: vec![
                    statement(6, Operand::Input(0), Operand::Input(1), Destination::Value),
                    statement(
                        64,
                        Operand::Input(0),
                        Operand::Input(1),
                        Destination::Word(1, 2)
                    ),
                    statement(
                        31,
                        Operand::Temp(0),
                        Operand::Input(1),
                        Destination::Nothing
                    ),
                ],
                immediates: Vec::new(),
                temps: vec![Temp::Made(0), Temp::Vector(1)],
                value: Some(Operand::Temp(1)),
            })
        );
        for reads in [[Some(None), None], [None, Some(None)]] {
            let mut unnamed = builder.clone();
            assert_eq!(unnamed.push(1, reads, Destination::Value), None);
            assert_eq!(unnamed.finish(None), None);
        }
        let mut given_up = Builder::new();
        given_up.give_up();
        assert_eq!(given_up.finish(None), None);
    }

    // A word of an input, a temporary or an immediate is named; a word of a word is not.
    #[test]
    fn a_word_is_of_a_whole_value() {
        use super::Whole;
        assert_eq!(
            Operand::Input(1).word(2),
            Some(Operand::Word(Whole::Input(1), 2))
        );
        assert_eq!(
            Operand::Temp(3).word(0),
            Some(Operand::Word(Whole::Temp(3), 0))
        );
        assert_eq!(Operand::Word(Whole::Temp(3), 0).word(1), None);
    }
}
