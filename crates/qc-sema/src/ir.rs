// SPDX-License-Identifier: GPL-3.0-or-later

//! The intermediate layer (`docs/design/back-end.md`): the statements the checker lowers a
//! construct to, with operands that are values rather than global offsets, for the code
//! generator to lay out and write. An opcode the target lacks is already emulated here
//! (FTEF-27), once, where the checker also raises the warnings the emulation gives.
//!
//! So far it holds the binary operators: each one's [`Fragment`], the statements
//! `QCC_PR_StatementFlags` emits for it, its operands' conversions included (FTEF-23.9).

use std::collections::HashMap;

use qc_preproc::Op;

use crate::DefId;
use crate::check::etype::Etype;
use crate::check::opcodes::row as opcode_row;

/// What a statement reads or what a fragment's value is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Operand {
    /// One of the fragment's inputs, the operands of its operator, as the code generator holds
    /// them.
    Input(Input),
    /// One of the fragment's temporaries.
    Temp(TempId),
    /// An immediate constant (FTEF-26): its kind and its words.
    Immediate(Immediate),
    /// One word of a vector input, temporary or immediate: `var.ofs++` (FTE-9.3).
    Word(Whole, u8),
    /// A call's parameter slot by its number, `OFS_PARM0 + 3 * n` (QC-18.3).
    Parameter(u8),
    /// A definition, by its identity: the function a call calls.
    Def(DefId),
    /// The return global after a call (`OFS_RETURN`, QC-18.4), the call's value.
    Return,
}

/// An operand of the operator a fragment is lowered from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Input {
    /// The left operand.
    Left,
    /// The right operand.
    Right,
    /// One of a construct's other inputs, by its number: a vector constructor's components.
    Argument(u8),
}

/// A temporary of a fragment: its number in [`Fragment::temps`], the order the lowering took
/// them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TempId(u16);

impl TempId {
    /// The temporary of index `n`.
    #[must_use]
    pub const fn new(n: u16) -> Self {
        Self(n)
    }

    /// Its index in [`Fragment::temps`].
    #[must_use]
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// A value whose words a [`Operand::Word`] names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Whole {
    /// One of the fragment's inputs.
    Input(Input),
    /// One of the fragment's temporaries.
    Temp(TempId),
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
            Self::Word(..) | Self::Parameter(_) | Self::Def(_) | Self::Return => return None,
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
    Word(TempId, u8),
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
    /// The opcode.
    pub op: Op,
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
    /// The constant each input was when a fold consumed it, left then right: an input no
    /// statement or value names, which the fragment then depends on being that constant. A
    /// writer whose inputs are other values does not hold the fragment.
    pub inputs: [Option<Immediate>; 2],
}

/// What an operand of a whole body's statement names (`docs/design/back-end.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Name {
    /// A word of a definition: the definition and the word.
    Def(DefId, u32),
    /// An immediate constant.
    Immediate(Immediate),
    /// The value of the body's statement at this index.
    Made(u32),
    /// A word of the vector the body's statement at this index made: the index and the word.
    MadeWord(u32, u32),
    /// The return global, a call's value (QC-18.4).
    Return,
    /// A word after the first of the return global, a component of a call's vector.
    ReturnWord(u32),
    /// A call's parameter slot by its number, `OFS_PARM0 + 3 * n` (QC-18.3), and past the
    /// eighth FTEQCC's `$parmN` global (FTE-18.6).
    Parameter(u8),
    /// A word of the variadic function's `__va_list` (FTE-18.6).
    VaList(u32),
    /// The default value of this parameter of the function the next `CALL` calls (FTE-18.7).
    Default(u8),
    /// The function's `ret*` local, which a bare `return` returns (FTE-18.10).
    RetLocal,
    /// The function a `__wrap` body wraps, its `prior` (FTEF-15.11).
    Prior,
    /// The body's statement at this index, a jump's target; the body's length is its end.
    Jump(u32),
    /// The string immediate of the body's string literal at this index
    /// (`TypedBody::string`).
    String(u32),
    /// The array definition's getter, `ArrayGet*NAME` (FTE-13.8).
    ArrayGet(DefId),
    /// The array definition's setter, `ArraySet*NAME` (FTE-13.11).
    ArraySet(DefId),
}

/// What a body makes without a statement of its own that reads it (`TypedBody::effects`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// What the name names, made the first time it is named: a local declared, a fold's
    /// immediate, or what a dropped statement named.
    Make(Name),
    /// A temporary of this many words a dropped statement took (QC-21.5).
    Temp(u32),
}

impl Name {
    /// Whether it names something the compiler makes the first time it is named: a
    /// definition, an immediate, an array function, an extra parameter slot, the `ret*` local.
    #[must_use]
    pub const fn is_made_once(&self) -> bool {
        match self {
            Self::Def(..)
            | Self::Immediate(_)
            | Self::String(_)
            | Self::ArrayGet(_)
            | Self::ArraySet(_)
            | Self::RetLocal => true,
            Self::Parameter(slot) => *slot >= 8,
            Self::Made(_)
            | Self::MadeWord(..)
            | Self::Return
            | Self::ReturnWord(_)
            | Self::Jump(_)
            | Self::VaList(_)
            | Self::Default(_)
            | Self::Prior => false,
        }
    }
}

/// One statement of a whole body as the semantic model generated it: its opcode and what each
/// operand names, `None` where the statement has no such operand or the model does not name
/// it yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyStatement {
    /// The opcode.
    pub op: Op,
    /// Operand a.
    pub a: Option<Name>,
    /// Operand b.
    pub b: Option<Name>,
    /// Operand c.
    pub c: Option<Name>,
}

impl BodyStatement {
    /// The statement `op` with no operand named yet.
    #[must_use]
    pub const fn bare(op: Op) -> Self {
        Self {
            op,
            a: None,
            b: None,
            c: None,
        }
    }

    /// Whether the opcode reads or writes each operand, which a writer needs named: `a` and `b`
    /// where its row in `pr_opcodes` types them, `c` where the row makes a value of its own, a
    /// jump's offset (`b` of a conditional jump, `a` of `GOTO`), and none of `RETURN`, whose
    /// value a bare `return` leaves 0.
    #[must_use]
    pub fn uses(&self) -> [bool; 3] {
        let Some(row) = opcode_row(self.op) else {
            return [false; 3];
        };
        let [a, b, _] = row.types.map(|ty| ty.etype() != Etype::Void);
        let c = row.makes_value();
        match self.op {
            Op::Goto => [true, false, false],
            Op::IfI | Op::IfnotI | Op::IfS | Op::IfnotS | Op::IfF | Op::IfnotF => [a, true, c],
            Op::Return | Op::Done => [false; 3],
            _ => [a, b, c],
        }
    }

    /// The words of the temporary this statement, at `index`, takes as it is generated, used or
    /// not (QC-21.5): its result's when its opcode makes a value, whatever a fold then makes `c`,
    /// and a copy's for a store into a value of its own, `b`.
    #[must_use]
    pub fn temp_words(&self, index: u32) -> Option<u32> {
        let row = opcode_row(self.op)?;
        let etype = if row.makes_value() {
            row.types[2].etype()
        } else if self.b == Some(Name::Made(index)) {
            row.types[1].etype()
        } else {
            return None;
        };
        Some(match etype {
            Etype::Vector => 3,
            Etype::Int64 | Etype::Uint64 | Etype::Double => 2,
            _ => 1,
        })
    }

    /// The words of the value this statement, at `index`, makes: its result's for a value of
    /// its own, `c`, and a copy's for a store into a value of its own, `b`.
    #[must_use]
    pub fn made_words(&self, index: u32) -> Option<u32> {
        let made = Some(Name::Made(index));
        let etype = if self.c == made {
            opcode_row(self.op)?.types[2].etype()
        } else if self.b == made {
            opcode_row(self.op)?.types[1].etype()
        } else {
            return None;
        };
        Some(match etype {
            Etype::Vector => 3,
            Etype::Int64 | Etype::Uint64 | Etype::Double => 2,
            _ => 1,
        })
    }

    /// Moves each value this statement names that a statement from `from` on made `by`
    /// statements on, as when statements are moved after others.
    pub(crate) fn shift_made(&mut self, from: u32, by: u32) {
        for name in [&mut self.a, &mut self.b, &mut self.c]
            .into_iter()
            .flatten()
        {
            if let Name::Made(at) | Name::MadeWord(at, _) = name
                && *at >= from
            {
                *at = at.saturating_add(by);
            }
        }
    }
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
        op: Op,
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
        Some(Operand::Temp(TempId(temp)))
    }

    /// Takes a vector temporary before the next statement, whose words statements then write
    /// ([`Destination::Word`]); its number.
    pub(crate) fn vector(&mut self) -> Option<TempId> {
        let (Ok(index), Ok(temp)) = (
            u16::try_from(self.statements.len()),
            u16::try_from(self.temps.len()),
        ) else {
            self.held = false;
            return None;
        };
        self.temps.push(Temp::Vector(index));
        Some(TempId(temp))
    }

    /// Records an immediate the lowering made.
    pub(crate) fn made(&mut self, immediate: Immediate) {
        self.immediates.push(immediate);
    }

    /// Marks the fragment as one the layer does not hold.
    pub(crate) const fn give_up(&mut self) {
        self.held = false;
    }

    /// The fragment, valued `value`, if every step was held; of the constants `inputs` the
    /// inputs were, those of the inputs it does not name, which a fold consumed.
    pub(crate) fn finish(
        self,
        value: Option<Operand>,
        inputs: [Option<Immediate>; 2],
    ) -> Option<Fragment> {
        let names = |side: Input| {
            let named = |operand: &Option<Operand>| matches!(operand, Some(Operand::Input(n) | Operand::Word(Whole::Input(n), _)) if *n == side);
            named(&value) || self.statements.iter().any(|s| named(&s.a) || named(&s.b))
        };
        let [left, right] = inputs;
        let inputs = [
            left.filter(|_| !names(Input::Left)),
            right.filter(|_| !names(Input::Right)),
        ];
        self.held.then_some(Fragment {
            statements: self.statements,
            immediates: self.immediates,
            temps: self.temps,
            value,
            inputs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Builder, Destination, Fragment, Input, Operand, Statement, Temp, TempId};
    use qc_preproc::Op;

    // A statement's value is the next temporary and a vector taken before its words is one too;
    // a statement writing nothing or a word names none; and an operand the fragment cannot name
    // leaves it unheld.
    #[test]
    fn a_builder_numbers_temporaries_and_refuses_an_unnamed_operand() {
        let mut builder = Builder::new();
        let inputs = [
            Some(Some(Operand::Input(Input::Left))),
            Some(Some(Operand::Input(Input::Right))),
        ];
        let made = builder.push(Op::AddF, inputs, Destination::Value);
        assert_eq!(made, Some(Operand::Temp(TempId(0))));
        assert_eq!(builder.vector(), Some(TempId(1)));
        assert_eq!(
            builder.push(Op::BitandF, inputs, Destination::Word(TempId(1), 2)),
            None
        );
        assert_eq!(
            builder.push(
                Op::StoreF,
                [made.map(Some), inputs[1]],
                Destination::Nothing
            ),
            None
        );
        let statement = |op, a, b, c| Statement {
            op,
            a: Some(a),
            b: Some(b),
            c,
        };
        assert_eq!(
            builder
                .clone()
                .finish(Some(Operand::Temp(TempId(1))), [None, None]),
            Some(Fragment {
                statements: vec![
                    statement(
                        Op::AddF,
                        Operand::Input(Input::Left),
                        Operand::Input(Input::Right),
                        Destination::Value
                    ),
                    statement(
                        Op::BitandF,
                        Operand::Input(Input::Left),
                        Operand::Input(Input::Right),
                        Destination::Word(TempId(1), 2)
                    ),
                    statement(
                        Op::StoreF,
                        Operand::Temp(TempId(0)),
                        Operand::Input(Input::Right),
                        Destination::Nothing
                    ),
                ],
                immediates: Vec::new(),
                temps: vec![Temp::Made(0), Temp::Vector(1)],
                value: Some(Operand::Temp(TempId(1))),
                inputs: [None, None],
            })
        );
        for reads in [[Some(None), None], [None, Some(None)]] {
            let mut unnamed = builder.clone();
            assert_eq!(unnamed.push(Op::MulF, reads, Destination::Value), None);
            assert_eq!(unnamed.finish(None, [None, None]), None);
        }
        let mut given_up = Builder::new();
        given_up.give_up();
        assert_eq!(given_up.finish(None, [None, None]), None);
    }

    // A word of an input, a temporary or an immediate is named; a word of a word is not.
    #[test]
    fn a_word_is_of_a_whole_value() {
        use super::Whole;
        assert_eq!(
            Operand::Input(Input::Right).word(2),
            Some(Operand::Word(Whole::Input(Input::Right), 2))
        );
        assert_eq!(
            Operand::Temp(TempId(3)).word(0),
            Some(Operand::Word(Whole::Temp(TempId(3)), 0))
        );
        assert_eq!(Operand::Word(Whole::Temp(TempId(3)), 0).word(1), None);
    }
}
