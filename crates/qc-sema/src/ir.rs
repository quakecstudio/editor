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
    /// The value statement `n` of the fragment made.
    Temp(u16),
    /// An immediate constant (FTEF-26): its kind and its words.
    Immediate(Immediate),
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

/// One statement: its opcode's number (`pr_opcodes`), its operands, and whether it makes a
/// value, which later statements and the fragment name as [`Operand::Temp`] by its index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Statement {
    /// The opcode's number.
    pub op: u16,
    /// Operand a.
    pub a: Option<Operand>,
    /// Operand b.
    pub b: Option<Operand>,
    /// Whether the statement writes a value to a temporary of its own (operand c).
    pub makes: bool,
}

/// The statements a construct lowers to, in emission order, and the operand its value is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fragment {
    /// The statements.
    pub statements: Vec<Statement>,
    /// The construct's value: an input, a temporary a statement made, or a constant.
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
    held: bool,
}

impl Builder {
    /// A builder holding nothing yet.
    pub(crate) const fn new() -> Self {
        Self {
            statements: Vec::new(),
            held: true,
        }
    }

    /// Adds a statement; the operand its value is, when it makes one.
    pub(crate) fn push(
        &mut self,
        op: u16,
        a: Option<Operand>,
        b: Option<Operand>,
        makes: bool,
    ) -> Option<Operand> {
        let index = u16::try_from(self.statements.len()).ok();
        if index.is_none() {
            self.held = false;
        }
        self.statements.push(Statement { op, a, b, makes });
        index.filter(|_| makes).map(Operand::Temp)
    }

    /// Marks the fragment as one the layer does not hold.
    pub(crate) const fn give_up(&mut self) {
        self.held = false;
    }

    /// The fragment, valued `value`, if every step was held.
    pub(crate) fn finish(self, value: Option<Operand>) -> Option<Fragment> {
        self.held.then_some(Fragment {
            statements: self.statements,
            value,
        })
    }
}
