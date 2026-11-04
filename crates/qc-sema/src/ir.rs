// SPDX-License-Identifier: GPL-3.0-or-later

//! The intermediate layer (`docs/design/back-end.md`): each checked body's statements as the
//! semantic model generates them, in order ([`BodyStatement`]), with operands that name values
//! rather than global offsets ([`Name`]), and what the body makes without a statement that reads
//! it ([`Effect`]), for the code generator's passes to allocate, lay out and write. An opcode the
//! target lacks is already emulated here (FTEF-27), once, where the checker also raises the
//! warnings the emulation gives.

use qc_preproc::Op;

use crate::DefId;
use crate::check::etype::Etype;
use crate::check::opcodes::row as opcode_row;

/// A constant a statement names: the words of its value and the kind of immediate that holds
/// them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Immediate {
    /// The kind of immediate.
    pub kind: ImmediateKind,
    /// Its words: one, or three for a vector.
    pub words: [u32; 3],
}

/// The kinds of immediate a statement names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImmediateKind {
    /// A float.
    Float,
    /// A vector.
    Vector,
    /// An integer (FTEF-26.3).
    Integer,
}

/// What an operand of a whole body's statement names (`docs/design/back-end.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Name {
    /// A word of a definition: the definition and the word.
    Def(DefId, u32),
    /// An immediate constant.
    Immediate(Immediate),
    /// A word of a vector immediate, a constant vector's component (FTEF-31.9): the
    /// immediate and the word.
    ImmediateWord(Immediate, u32),
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
    /// The immediate holding the number of the nested function the local definition is given,
    /// which the code generator numbers (FTEF-18.2).
    Nested(DefId),
}

/// What a body makes without a statement of its own that reads it (`TypedBody::effects`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// What the name names, made the first time it is named: a local declared, a fold's
    /// immediate, or what a dropped statement named.
    Make(Name),
    /// A temporary of this many words a dropped statement took (QC-21.5).
    Temp(u32),
    /// A temporary of this many words taken before its words are written (QC-21.5), a vector's
    /// or a struct's, the value of the statement it is made at: the words the statements after
    /// it write, `MadeWord` of that statement.
    Block(u32),
    /// The constant local the name names is given its aggregate initialiser's words at compile
    /// time, as a global is (FTEF-32.13).
    Constant(Name),
    /// The `static` local the name names is given the number of the nested function its
    /// declaration defines (FTEF-18.2).
    Nested(Name),
    /// The `autocvar_NAME` global the name names, which `autocvar(NAME, default)` reads, given
    /// the default as its value when nothing initialised it before (FTEF-32.12).
    Autocvar(Name),
    /// A `for` loop begins, whose end forgets the local arrays' functions made in it.
    ForBegin,
    /// The `for` loop begun last ends: the getters and setters of local arrays made since it
    /// began are forgotten, and a later use makes new ones (FTEF-19.78).
    ForEnd,
}

impl Name {
    /// Word `word` of what it names, if it names something by words: a definition's, a
    /// value's, an immediate's or the return global's.
    #[must_use]
    pub const fn word(self, word: u32) -> Option<Self> {
        Some(match self {
            Self::Def(def, at) => Self::Def(def, at.saturating_add(word)),
            Self::Made(made) => Self::MadeWord(made, word),
            Self::MadeWord(made, at) => Self::MadeWord(made, at.saturating_add(word)),
            Self::Immediate(immediate) => Self::ImmediateWord(immediate, word),
            Self::ImmediateWord(immediate, at) => {
                Self::ImmediateWord(immediate, at.saturating_add(word))
            }
            Self::Return => Self::ReturnWord(word),
            Self::ReturnWord(at) => Self::ReturnWord(at.saturating_add(word)),
            Self::Parameter(_)
            | Self::VaList(_)
            | Self::Default(_)
            | Self::RetLocal
            | Self::Prior
            | Self::Jump(_)
            | Self::String(_)
            | Self::ArrayGet(_)
            | Self::ArraySet(_)
            | Self::Nested(_) => return None,
        })
    }

    /// Whether it names something the compiler makes the first time it is named: a
    /// definition, an immediate, an array function, an extra parameter slot, the `ret*` local.
    #[must_use]
    pub const fn is_made_once(&self) -> bool {
        match self {
            Self::Def(..)
            | Self::Immediate(_)
            | Self::ImmediateWord(..)
            | Self::String(_)
            | Self::ArrayGet(_)
            | Self::ArraySet(_)
            | Self::Nested(_)
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
        // A word of a block an `Effect::Block` took takes none of its own.
        if matches!(self.c, Some(Name::MadeWord(..))) {
            return None;
        }
        // FTE-18.11: a field argument is loaded straight into its parameter slot, with no
        // temporary.
        let load = matches!(
            self.op,
            Op::LoadF | Op::LoadV | Op::LoadS | Op::LoadEnt | Op::LoadFld | Op::LoadFnc
        );
        if load && matches!(self.c, Some(Name::Parameter(_))) {
            return None;
        }
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
        // The first word of a block an `Effect::Block` took, which is the block, written as a
        // result or stored.
        if self.c == Some(Name::MadeWord(index, 0)) || self.b == Some(Name::MadeWord(index, 0)) {
            return Some(3);
        }
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

#[cfg(test)]
mod tests {
    use qc_preproc::Op;

    use super::{BodyStatement, Immediate, ImmediateKind, Name};
    use crate::DefId;

    // spec FTE-18.11, QC-21.5: a field argument loaded straight into its parameter slot takes no
    // temporary; the same load into a value of its own takes its result's words, and so does a
    // value a store then folded into a parameter
    #[test]
    fn a_load_into_a_parameter_takes_no_temporary() {
        let load = |c| BodyStatement {
            c: Some(c),
            ..BodyStatement::bare(Op::LoadV)
        };
        assert_eq!(load(Name::Parameter(0)).temp_words(4), None);
        assert_eq!(load(Name::Made(4)).temp_words(4), Some(3));
        let folded = BodyStatement {
            c: Some(Name::Parameter(1)),
            ..BodyStatement::bare(Op::AddF)
        };
        assert_eq!(folded.temp_words(5), Some(1));
    }

    // A word of a definition, a value, an immediate or the return global is named, offset from
    // the word already named; a parameter slot or a jump has no words
    #[test]
    fn a_name_names_its_words() {
        let def = DefId::new(4);
        let immediate = Immediate {
            kind: ImmediateKind::Vector,
            words: [1, 2, 3],
        };
        assert_eq!(Name::Def(def, 1).word(2), Some(Name::Def(def, 3)));
        assert_eq!(Name::Made(5).word(2), Some(Name::MadeWord(5, 2)));
        assert_eq!(Name::MadeWord(5, 1).word(1), Some(Name::MadeWord(5, 2)));
        assert_eq!(
            Name::Immediate(immediate).word(1),
            Some(Name::ImmediateWord(immediate, 1))
        );
        assert_eq!(Name::Return.word(2), Some(Name::ReturnWord(2)));
        assert_eq!(Name::Parameter(0).word(1), None);
        assert_eq!(Name::Jump(3).word(1), None);
    }
}
