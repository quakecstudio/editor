// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/pr_comp.c, PR_GetDef, PR_ParseImmediate, PR_Statement
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/qcc.c, InitData, PR_BeginCompilation, CopyString
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_ParseEntClass
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_Make32bitConst, QCC_Make64bitConst
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qccmain.c, QCC_WriteData

//! The program being built: `qcc`'s definition list, global area, string table, statements
//! and functions, as an explicit value rather than id's globals.
//!
//! The semantic model decides which definition a name is (QC-12); this module gives each
//! definition its place the first time the compiler meets it, as the allocating half of
//! `PR_GetDef` does (QC-21), shares literals as `PR_ParseImmediate` does (QC-20), and emits
//! statements as `PR_Statement` does, a new temporary for every result (QC-21.5).

use std::collections::HashMap;

use qc_progs::opcode::Opcode;
use qc_progs::{Function, Statement};
use qc_sema::{DefId, DefKind, FileId, FunctionType, ModuleIndex, SimpleType, Type};
use qc_syntax::c_scalar::CScalar;
use qc_types::Etype;

use crate::Internal;

/// The first global offset a definition may take: 0 is null, 1 to 3 the return value and 4 to
/// 27 the eight parameter slots (QC-21.1, `RESERVED_OFS` in `qcc/qcc.h`).
pub(crate) const RESERVED_OFS: usize = 28;

/// The return value's global offset, `OFS_RETURN` (QC-18.4).
pub(crate) const OFS_RETURN: u32 = 1;

/// The first parameter slot's global offset, `OFS_PARM0`; slot `i` is at `OFS_PARM0 + 3 * i`
/// (QC-18.3).
pub(crate) const OFS_PARM0: u32 = 4;

/// The `_x`, `_y` and `_z` suffixes of a vector's components (QC-11.7).
const COMPONENTS: [&str; 3] = ["_x", "_y", "_z"];

/// FTEQCC's `MEMBERFIELDNAME`, `"__m%s"`: what a class member's entity field is named after its
/// class's `CLASS::`, `CLASS::__mNAME` (FTEF-9.21).
const MEMBER_FIELD_PREFIX: &str = "__m";

/// A type as `qcc`'s expressions carry it (QC-10): one the semantic model gives, or `qcc`'s
/// own `type_floatfield`, the type of a vector field's components, which no type written in the
/// source is (QC-11.12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ValueType {
    /// A type of the semantic model.
    Of(Type),
    /// `type_floatfield`: a field of a float, unequal to every `.float` the source writes.
    FloatField,
}

impl ValueType {
    /// The type of a literal or an operator's result of basic type `etype`: `type_float`,
    /// `type_vector` or `type_string` (QC-16.1, QC-20.1).
    pub(crate) fn simple(etype: Etype) -> Self {
        let simple = match etype {
            Etype::Vector => SimpleType::Vector,
            Etype::String => SimpleType::String,
            Etype::Entity => SimpleType::Entity,
            Etype::Void => SimpleType::Void,
            Etype::Float | Etype::Field | Etype::Function | Etype::Pointer => SimpleType::Float,
        };
        Self::Of(Type::Simple(simple))
    }

    /// The basic type, `type->type` in `qcc` (QC-16.1): what the operator table compares.
    pub(crate) fn etype(&self) -> Etype {
        match self {
            Self::FloatField => Etype::Field,
            Self::Of(ty) => match ty {
                Type::Simple(SimpleType::Float) => Etype::Float,
                Type::Simple(SimpleType::Vector) => Etype::Vector,
                Type::Simple(SimpleType::String) => Etype::String,
                Type::Simple(SimpleType::Entity) => Etype::Entity,
                Type::Field(_) => Etype::Field,
                Type::Function(_) => Etype::Function,
                Type::Pointer(_) => Etype::Pointer,
                Type::Simple(SimpleType::Void) | Type::Unknown | Type::Named(_) => Etype::Void,
            },
        }
    }

    /// The words a value of this type takes, `type_size` (`QC_TYPE_SIZE`, QC-10.1).
    pub(crate) fn words(&self) -> u32 {
        if self.etype() == Etype::Vector { 3 } else { 1 }
    }

    /// For a field type, the type of what the field holds: `aux_type` (QC-16.1).
    pub(crate) fn field_inner(&self) -> Option<Self> {
        match self {
            Self::FloatField => Some(Self::Of(Type::Simple(SimpleType::Float))),
            Self::Of(Type::Field(inner)) => Some(Self::Of(Type::clone(inner))),
            Self::Of(_) => None,
        }
    }

    /// For a function type, the function's type (QC-18.8).
    pub(crate) fn function(&self) -> Option<&FunctionType> {
        match self {
            Self::Of(Type::Function(function)) => Some(function),
            Self::Of(_) | Self::FloatField => None,
        }
    }
}

/// One entry of `qcc`'s definition list, `def_t` (QC-12.2, QC-25.3).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Def {
    /// The name the progs records.
    pub(crate) name: String,
    /// Its type.
    pub(crate) ty: ValueType,
    /// Its global offset (QC-21.2).
    pub(crate) ofs: u32,
    /// The function it belongs to, for a parameter or local (`def->scope`); `None` for a global
    /// and a literal (QC-20.4: "always share immediates").
    pub(crate) scope: Option<DefId>,
    /// Whether it has been initialised so far (QC-11.5): set where the initialiser is met, so a
    /// literal only reuses a constant initialised before it (QC-20.2).
    pub(crate) initialized: bool,
    /// Whether the semantic model makes it a constant (`def->constant`): under FTEQCC only a
    /// constant's word may hold a literal (FTE-20.5).
    pub(crate) constant: bool,
    /// An array's elements, `arraysize`; `None` for no array (FTE-13.10).
    pub(crate) count: Option<u32>,
    /// The semantic model's definition it is; `None` for a literal and one the compiler makes.
    pub(crate) id: Option<DefId>,
}

impl Def {
    /// The words of one element of the array it is: a field array's are what its fields
    /// hold, a global for each (FTEF-31.33), any other's its type's.
    pub(crate) fn element_words(&self, index: &ModuleIndex) -> u32 {
        self.ty.field_inner().map_or_else(
            || type_words(index, &self.ty),
            |held| type_words(index, &held),
        )
    }
}

/// A literal's value as `qcc`'s lexer reads it (QC-4 to QC-6).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Literal {
    /// A float literal or a frame macro reference (QC-4, QC-8.5).
    Float(f32),
    /// A vector literal (QC-5).
    Vector([f32; 3]),
    /// A string literal's bytes, escapes replaced (QC-6).
    String(Vec<u8>),
}

impl Literal {
    /// `pr_immediate_type`: `type_float`, `type_vector` or `type_string`.
    pub(crate) fn ty(&self) -> ValueType {
        ValueType::simple(match self {
            Self::Float(_) => Etype::Float,
            Self::Vector(_) => Etype::Vector,
            Self::String(_) => Etype::String,
        })
    }
}

/// Which definitions a literal may share a word with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LiteralSharing {
    /// Any initialised definition, as id's `qcc`, whose initialised definitions are all
    /// constants (QC-20.2).
    Initialised,
    /// Only a constant: FTEQCC's immediates are shared among themselves (`floatconstdefstable`),
    /// and a constant takes an immediate's place when its value folds (FTE-20.5, FTEF-34.15).
    Constants,
}

/// The first virtual offset of a function's block of private locals (FTE-21.9): parameters,
/// locals and temporaries take offsets from here while the body is compiled, and real ones,
/// after every global, static and immediate, when the module's blocks are laid out.
const BLOCK_BASE: u32 = 1 << 20;

/// The virtual offsets a closed block keeps until the module's blocks are laid out: its own
/// range, so no two functions' definitions or temporaries share a virtual offset.
const BLOCK_SPAN: u32 = 1 << 16;

/// The first virtual offset of the module's shared temporaries (FTE-21.8), just below every
/// block's and above every global the module may have while any wait: real ones, after every
/// block, when they are laid out.
const SHARED_BASE: u32 = BLOCK_BASE - (1 << 16);

/// The first virtual offset of the open block's locked temporaries (FTE-21.8): above its
/// parameters and locals while its body is compiled, moved after them when it closes.
const LOCKED_BASE: u32 = BLOCK_BASE + (1 << 15);

/// A function's block of private locals while its body is compiled (FTE-21.9).
#[derive(Debug)]
struct Block {
    /// The next virtual offset.
    next: u32,
    /// The next virtual offset of its locked temporaries (FTE-21.8).
    locked_next: u32,
    /// The first definition made since the block opened.
    defs_from: usize,
    /// The first statement made since the block opened.
    statements_from: usize,
    /// The words written to the block's offsets, to be stored once they are real.
    inits: Vec<(u32, Vec<u32>)>,
}

/// A function's block of private locals, closed and waiting for its real offsets
/// (FTE-21.9).
#[derive(Debug)]
struct PendingBlock {
    /// The function record whose `parm_start` the block becomes.
    record: usize,
    /// The definitions made while the block was open.
    defs: std::ops::Range<usize>,
    /// The statements made while the block was open.
    statements: std::ops::Range<usize>,
    /// The block's words.
    len: u32,
    /// The words written to the block's offsets while it was open.
    inits: Vec<(u32, Vec<u32>)>,
    /// The first virtual offset the block holds while it waits.
    virtual_base: u32,
}

/// The program as compiled so far.
#[derive(Debug)]
pub(crate) struct Program {
    /// The definition list, in the order the compiler made them (QC-12.2, QC-25.3).
    pub(crate) defs: Vec<Def>,
    /// Where in [`Program::defs`] each definition of the semantic model is, once made.
    places: HashMap<DefId, usize>,
    /// The global area, a word per global offset: its length is `numpr_globals`.
    pub(crate) globals: Vec<u32>,
    /// The string table, without the padding [`crate::write`] adds.
    pub(crate) strings: Vec<u8>,
    /// The statements; statement 0 is the placeholder (QC-25.1).
    pub(crate) statements: Vec<Statement>,
    /// Where in the compiled text each statement was emitted from, for the `.lno`'s lines
    /// (QC-27.19): the file and the start of the source statement, `None` for the placeholder.
    pub(crate) origins: Vec<Option<(FileId, usize)>>,
    /// The place [`Program::emit`] and [`Program::emit_result`] record, which the compiler sets
    /// as it reads each statement.
    pub(crate) origin: Option<(FileId, usize)>,
    /// The functions; function 0 is the placeholder (QC-25.1).
    pub(crate) functions: Vec<Function>,
    /// The entity fields allocated so far, `pr.size_fields` (QC-21.3).
    pub(crate) size_fields: u32,
    /// Which definitions a literal may share a word with.
    pub(crate) literal_sharing: LiteralSharing,
    /// The block of the function whose body is being compiled, under FTEQCC (FTE-21.9).
    block: Option<Block>,
    /// The blocks closed so far, laid out by [`Program::resolve_blocks`] (FTE-21.9).
    pending_blocks: Vec<PendingBlock>,
    /// The module's shared temporaries, each its virtual offset and words, in the order they
    /// were made, laid out by [`Program::resolve_shared_temps`] (FTE-21.8).
    shared: Vec<(u32, u32)>,
    /// The statement operands that name a shared temporary: the statement and the operand,
    /// 0 for `a`, 1 for `b`, 2 for `c`.
    shared_sites: Vec<(usize, u8)>,
    /// The translated strings made so far, each with its definition's place
    /// (`stringconstdefstable_trans`, FTEF-32.22).
    translations: Vec<(Vec<u8>, usize)>,
    /// How many `dotranslate_N` definitions there are (`dotranslate_count`).
    translated: u32,
    /// The functions an initialiser being gathered names before they are defined, each with
    /// the word of the initialiser that is to hold its number (FTEF-32.23).
    named_functions: Vec<(u32, usize)>,
    /// The globals that are to hold a function's number once every function is defined, each
    /// with the function's definition (`$relocf`, FTEF-32.23).
    function_relocs: Vec<(u32, usize)>,
}

impl Program {
    /// An empty program, as `InitData` and `PR_BeginCompilation` leave `qcc`: one placeholder
    /// statement and function, the empty string at offset 0, and the reserved globals
    /// (QC-21.1, QC-25.1).
    pub(crate) fn new() -> Self {
        Self {
            defs: Vec::new(),
            places: HashMap::new(),
            globals: vec![0; RESERVED_OFS],
            strings: vec![0],
            statements: vec![Statement::default()],
            origins: vec![None],
            origin: None,
            functions: vec![Function::default()],
            size_fields: 0,
            literal_sharing: LiteralSharing::Initialised,
            block: None,
            pending_blocks: Vec::new(),
            shared: Vec::new(),
            shared_sites: Vec::new(),
            translations: Vec::new(),
            translated: 0,
            named_functions: Vec::new(),
            function_relocs: Vec::new(),
        }
    }

    /// FTE-21.8: a locked temporary of `words` words, in the open block after its locals once
    /// it closes, else the next globals; returns its first offset.
    pub(crate) fn locked_temp(&mut self, words: u32) -> Result<u32, Internal> {
        let Some(block) = &mut self.block else {
            return self.allocate_global(words);
        };
        let ofs = block.locked_next;
        block.locked_next = ofs.checked_add(words).ok_or(Internal::TooLarge)?;
        if block.locked_next > BLOCK_BASE + BLOCK_SPAN {
            return Err(Internal::TooLarge);
        }
        Ok(ofs)
    }

    /// FTE-21.8: operand `operand` (0 `a`, 1 `b`, 2 `c`) of the last statement names a shared
    /// temporary, which [`Program::resolve_shared_temps`] gives its real offset.
    pub(crate) fn mark_shared(&mut self, operand: u8) {
        let at = self.statements.len().saturating_sub(1);
        self.shared_sites.push((at, operand));
    }

    /// FTE-21.8: the module's shared temporaries so far, each its virtual offset and words.
    pub(crate) fn shared_temps(&self) -> &[(u32, u32)] {
        &self.shared
    }

    /// FTE-21.8: a new shared temporary of `words` words after the last, `QCC_GetTemp`'s
    /// `tempsused += size`; returns its virtual offset.
    pub(crate) fn shared_temp(&mut self, words: u32) -> Result<u32, Internal> {
        let ofs = match self.shared.last() {
            Some(&(ofs, size)) => ofs.checked_add(size).ok_or(Internal::TooLarge)?,
            None => SHARED_BASE,
        };
        if ofs.checked_add(words).is_none_or(|end| end > BLOCK_BASE) {
            return Err(Internal::TooLarge);
        }
        self.shared.push((ofs, words));
        Ok(ofs)
    }

    /// FTE-21.8: `QCC_FinaliseTemps`: lays the shared temporaries out after every global,
    /// static, immediate and block made so far, each statement operand among them taking its
    /// real offset.
    pub(crate) fn resolve_shared_temps(&mut self) -> Result<(), Internal> {
        let len = match self.shared.last() {
            Some(&(ofs, size)) => ofs
                .checked_add(size)
                .and_then(|end| end.checked_sub(SHARED_BASE))
                .ok_or(Internal::TooLarge)?,
            None => return Ok(()),
        };
        let base = self.allocate_global(len)?;
        // Only the operands that name one: another operand that holds a number in their range,
        // a jump's offset or a bound, is not a global.
        for (at, operand) in core::mem::take(&mut self.shared_sites) {
            let statement = self.statements.get_mut(at).ok_or(Internal::NoDefinition)?;
            let field = match operand {
                0 => &mut statement.a,
                1 => &mut statement.b,
                _ => &mut statement.c,
            };
            let real = u32::try_from(*field)
                .ok()
                .and_then(|ofs| ofs.checked_sub(SHARED_BASE))
                .and_then(|o| o.checked_add(base))
                .ok_or(Internal::TooLarge)?;
            *field = i32::try_from(real).map_err(|_| Internal::TooLarge)?;
        }
        self.shared.clear();
        Ok(())
    }

    /// FTE-21.9: opens the block of private locals of the function whose body follows: until
    /// [`Program::close_block`], a local, a parameter or a temporary takes a virtual offset.
    pub(crate) fn open_block(&mut self) {
        self.block = Some(Block {
            next: BLOCK_BASE,
            locked_next: LOCKED_BASE,
            defs_from: self.defs.len(),
            statements_from: self.statements.len(),
            inits: Vec::new(),
        });
    }

    /// FTE-21.9: closes the open block as the one of function `record`, to be laid out with
    /// the module's others; returns its words, the record's `locals`.
    pub(crate) fn close_block(&mut self, record: usize) -> Result<u32, Internal> {
        let mut block = self.block.take().ok_or(Internal::NoDefinition)?;
        // FTE-21.8: the locked temporaries follow the parameters and locals, as
        // `QCC_RemapLockedTemps` makes them locals at the function's end.
        let locked = block
            .locked_next
            .checked_sub(LOCKED_BASE)
            .ok_or(Internal::TooLarge)?;
        if locked > 0 {
            let to = block.next;
            for statement in self
                .statements
                .get_mut(block.statements_from..)
                .ok_or(Internal::NoDefinition)?
            {
                for operand in [&mut statement.a, &mut statement.b, &mut statement.c] {
                    if let Ok(ofs) = u32::try_from(*operand)
                        && (LOCKED_BASE..block.locked_next).contains(&ofs)
                    {
                        let real = ofs
                            .checked_sub(LOCKED_BASE)
                            .and_then(|o| o.checked_add(to))
                            .ok_or(Internal::TooLarge)?;
                        *operand = i32::try_from(real).map_err(|_| Internal::TooLarge)?;
                    }
                }
            }
            block.next = to.checked_add(locked).ok_or(Internal::TooLarge)?;
        }
        let len = block
            .next
            .checked_sub(BLOCK_BASE)
            .ok_or(Internal::TooLarge)?;
        if len > BLOCK_SPAN {
            return Err(Internal::TooLarge);
        }
        // The block moves to a range of its own, so the next body's block, which starts at
        // the same virtual offset, shares none with it.
        let slot = u32::try_from(self.pending_blocks.len())
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or(Internal::TooLarge)?;
        let virtual_base = slot
            .checked_mul(BLOCK_SPAN)
            .and_then(|o| o.checked_add(BLOCK_BASE))
            .ok_or(Internal::TooLarge)?;
        let mut pending = PendingBlock {
            record,
            defs: block.defs_from..self.defs.len(),
            statements: block.statements_from..self.statements.len(),
            len,
            inits: block.inits,
            virtual_base,
        };
        self.relocate_block(&mut pending, BLOCK_BASE, virtual_base)?;
        self.pending_blocks.push(pending);
        Ok(len)
    }

    /// Moves the offsets of `block`'s definitions, statement operands and pending words from
    /// `from` up to `to`; an offset below `from` is not the block's.
    fn relocate_block(
        &mut self,
        block: &mut PendingBlock,
        from: u32,
        to: u32,
    ) -> Result<(), Internal> {
        let relocate = |ofs: u32| -> Result<u32, Internal> {
            if ofs < from {
                return Ok(ofs);
            }
            ofs.checked_sub(from)
                .and_then(|o| o.checked_add(to))
                .ok_or(Internal::TooLarge)
        };
        for def in self
            .defs
            .get_mut(block.defs.clone())
            .ok_or(Internal::NoDefinition)?
        {
            def.ofs = relocate(def.ofs)?;
        }
        // A jump's operand is an offset among the statements, not a global (QC-22).
        let relocate_operand = |operand: i32| -> Result<i32, Internal> {
            let Ok(ofs) = u32::try_from(operand) else {
                return Ok(operand);
            };
            i32::try_from(relocate(ofs)?).map_err(|_| Internal::TooLarge)
        };
        for statement in self
            .statements
            .get_mut(block.statements.clone())
            .ok_or(Internal::NoDefinition)?
        {
            statement.a = relocate_operand(statement.a)?;
            statement.b = relocate_operand(statement.b)?;
            statement.c = relocate_operand(statement.c)?;
        }
        for (ofs, _) in &mut block.inits {
            *ofs = relocate(*ofs)?;
        }
        Ok(())
    }

    /// FTE-21.9: the block closed as function `from`'s becomes function `to`'s, as a wrap
    /// moves the record of the body it wraps to the end (FTEF-15.11).
    pub(crate) fn rekey_block(&mut self, from: usize, to: usize) {
        for block in &mut self.pending_blocks {
            if block.record == from {
                block.record = to;
            }
        }
    }

    /// FTE-21.9: `QCC_UnmarshalLocals`: lays out every closed block after every global,
    /// static and immediate made so far, in the order of the function records: each block's
    /// definitions and statement operands take their real offsets and its record its
    /// `parm_start`.
    pub(crate) fn resolve_blocks(&mut self) -> Result<(), Internal> {
        let mut pending = std::mem::take(&mut self.pending_blocks);
        pending.sort_by_key(|block| block.record);
        for mut block in pending {
            let base = self.allocate_global(block.len)?;
            let from = block.virtual_base;
            self.relocate_block(&mut block, from, base)?;
            let record = self
                .functions
                .get_mut(block.record)
                .ok_or(Internal::NoDefinition)?;
            record.parm_start = i32::try_from(base).map_err(|_| Internal::TooLarge)?;
            for (ofs, words) in block.inits {
                self.set_words(ofs, &words)?;
            }
        }
        Ok(())
    }

    /// `numpr_globals`: the next free global offset.
    pub(crate) fn next_global(&self) -> Result<u32, Internal> {
        u32::try_from(self.globals.len()).map_err(|_| Internal::TooLarge)
    }

    /// Takes the next `words` offsets (QC-21.2), of the open block for a local, a parameter or
    /// a temporary (FTE-21.9), else of the globals; returns the first.
    pub(crate) fn allocate(&mut self, words: u32) -> Result<u32, Internal> {
        if let Some(block) = &mut self.block {
            let ofs = block.next;
            block.next = ofs.checked_add(words).ok_or(Internal::TooLarge)?;
            // The locked temporaries' virtual offsets start above the block's own.
            if block.next > LOCKED_BASE {
                return Err(Internal::TooLarge);
            }
            return Ok(ofs);
        }
        self.allocate_global(words)
    }

    /// The offset the next [`Program::allocate`] gives.
    fn next_offset(&self) -> Result<u32, Internal> {
        match &self.block {
            Some(block) => Ok(block.next),
            None => self.next_global(),
        }
    }

    /// Takes the next `words` global offsets (QC-21.2), whatever block is open: a global, a
    /// static, an immediate or a generated definition; returns the first.
    pub(crate) fn allocate_global(&mut self, words: u32) -> Result<u32, Internal> {
        let ofs = self.next_global()?;
        // FTE-21.8: the globals stay below the shared temporaries' virtual offsets while any
        // are waiting to be laid out.
        if !self.shared.is_empty() && ofs.checked_add(words).is_none_or(|end| end > SHARED_BASE) {
            return Err(Internal::TooLarge);
        }
        let words = usize::try_from(words).map_err(|_| Internal::TooLarge)?;
        let len = self
            .globals
            .len()
            .checked_add(words)
            .ok_or(Internal::TooLarge)?;
        self.globals.resize(len, 0);
        Ok(ofs)
    }

    /// The word at global offset `ofs`, 0 past the end of the area.
    pub(crate) fn word(&self, ofs: u32) -> u32 {
        usize::try_from(ofs)
            .ok()
            .and_then(|i| self.globals.get(i))
            .copied()
            .unwrap_or(0)
    }

    /// Writes `words` from global offset `ofs`.
    pub(crate) fn set_words(&mut self, ofs: u32, words: &[u32]) -> Result<(), Internal> {
        // FTE-21.9: a word of the open block is written when the block is laid out.
        if ofs >= BLOCK_BASE {
            let block = self.block.as_mut().ok_or(Internal::TooLarge)?;
            block.inits.push((ofs, words.to_vec()));
            return Ok(());
        }
        let start = usize::try_from(ofs).map_err(|_| Internal::TooLarge)?;
        let end = start.checked_add(words.len()).ok_or(Internal::TooLarge)?;
        let slots = self.globals.get_mut(start..end).ok_or(Internal::TooLarge)?;
        slots.copy_from_slice(words);
        Ok(())
    }

    /// `CopyString`: appends `text` and a NUL to the string table, never shared (QC-25.4), and
    /// returns its offset.
    pub(crate) fn copy_string(&mut self, text: &[u8]) -> Result<i32, Internal> {
        let ofs = i32::try_from(self.strings.len()).map_err(|_| Internal::TooLarge)?;
        self.strings.extend_from_slice(text);
        self.strings.push(0);
        Ok(ofs)
    }

    /// The NUL-terminated string at string table offset `ofs`, `G_STRING`; `None` when `ofs` is
    /// outside the table.
    pub(crate) fn string_at(&self, ofs: u32) -> Option<&[u8]> {
        let start = usize::try_from(ofs).ok()?;
        let rest = self.strings.get(start..)?;
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        rest.get(..end)
    }

    /// Where definition `id` is in the list, once the compiler has made it.
    pub(crate) fn place(&self, id: DefId) -> Option<usize> {
        self.places.get(&id).copied()
    }

    /// The definition at `place` in the list.
    pub(crate) fn def(&self, place: usize) -> Option<&Def> {
        self.defs.get(place)
    }

    /// The allocating half of `PR_GetDef` (QC-21.2, QC-21.3, QC-11.7): makes definition `id` of
    /// `index` at the next free global offset if the compiler has not made it yet, and returns
    /// its place in the list.
    ///
    /// A vector takes the words of the `_x`, `_y` and `_z` definitions made after it, each the
    /// definition the semantic model resolved the component's name to, which may be an earlier
    /// one (QC-11.7). A field's global holds the entity field offset it takes; a vector field's
    /// components are fields of `type_floatfield` taking an offset each (QC-11.12, QC-21.3).
    pub(crate) fn get_def(&mut self, index: &ModuleIndex, id: DefId) -> Result<usize, Internal> {
        self.get_def_typed(index, id, None)
    }

    fn get_def_typed(
        &mut self,
        index: &ModuleIndex,
        id: DefId,
        ty: Option<ValueType>,
    ) -> Result<usize, Internal> {
        if let Some(place) = self.place(id) {
            return Ok(place);
        }
        let definition = index.def(id).ok_or(Internal::NoDefinition)?;
        // FTE-15.6: a typedef's or an enum's name stands for its type; a struct keeps its name,
        // by which its layout is found.
        let written = &definition.ty;
        let ty = ty.unwrap_or_else(|| {
            ValueType::Of(match index.alias(written) {
                Some(alias) if !index.struct_shape(written).is_some() => alias.clone(),
                _ => written.clone(),
            })
        });
        // FTEF-9.21: a class's data or virtual member is the entity field `CLASS::__mNAME`
        // (`QCC_PR_ParseEntClass`, `MEMBERFIELDNAME`), which names the entity field `NAME` when
        // one is defined (`d = QCC_PR_GetDef(NULL, parmname, ...); if (d) basicindex = 0;`).
        let class = definition
            .member_of
            .filter(|_| definition.kind == DefKind::Field && ty.field_inner().is_none())
            .and_then(|class| index.def(class));
        if let (Some(class), ValueType::Of(held)) = (class, &ty) {
            return self.member_field(index, id, &class.name, held.clone());
        }
        // FTE-21.9: a function's own definition takes the open block's offsets, a global the
        // globals' whatever block is open.
        let in_block = definition.owner.is_some() && self.block.is_some();
        let ofs = if in_block {
            self.next_offset()?
        } else {
            self.next_global()?
        };
        let place = self.defs.len();
        let count = index.array_len(id);
        // FTEF-9.21: a nonvirtual or static member is the global `CLASS::NAME`.
        let name = match definition.member_of.and_then(|class| index.def(class)) {
            Some(class) => format!("{}::{}", class.name, definition.name),
            None => definition.name.clone(),
        };
        self.defs.push(Def {
            name,
            ty: ty.clone(),
            ofs,
            scope: definition.owner,
            initialized: false,
            constant: index.is_constant(id),
            count,
            id: Some(id),
        });
        self.places.insert(id, place);
        // FTE-13.10: an array takes its elements' words in turn, a vector array's with no
        // component definitions; FTE-14.5: a struct takes its layout's words.
        if let Some(count) = count {
            if ty.field_inner().is_some() {
                return Err(Internal::Unsupported);
            }
            let words = type_words(index, &ty)
                .checked_mul(count)
                .ok_or(Internal::TooLarge)?;
            if in_block {
                self.allocate(words)?;
            } else {
                self.allocate_global(words)?;
            }
            return Ok(place);
        }
        if ty.etype() == Etype::Vector {
            for suffix in COMPONENTS {
                let component = component(index, id, suffix).ok_or(Internal::NoDefinition)?;
                self.get_def_typed(index, component, None)?;
            }
        } else {
            // FTEF-19.81: a field of a C scalar of two words has a word for each, as FTEQCC
            // sizes a field's global by what it holds.
            let held = ty
                .field_inner()
                .filter(|inner| inner.etype() != Etype::Vector)
                .map(|inner| type_words(index, &inner));
            let words = held.unwrap_or_else(|| type_words(index, &ty));
            if in_block {
                self.allocate(words)?;
            } else {
                self.allocate_global(words)?;
            }
        }
        if let Some(inner) = ty.field_inner() {
            // Each word of the global is the offset of the field's word.
            let held = if inner.etype() == Etype::Vector {
                1
            } else {
                type_words(index, &inner)
            };
            let offsets: Vec<u32> = (0..held)
                .map(|word| self.size_fields.saturating_add(word))
                .collect();
            self.set_words(ofs, &offsets)?;
            if inner.etype() == Etype::Vector {
                for suffix in COMPONENTS {
                    let component = component(index, id, suffix).ok_or(Internal::NoDefinition)?;
                    self.get_def_typed(index, component, Some(ValueType::FloatField))?;
                }
            } else {
                self.size_fields = self
                    .size_fields
                    .checked_add(type_words(index, &inner))
                    .ok_or(Internal::TooLarge)?;
            }
        }
        Ok(place)
    }

    /// FTEF-9.21: the member `id` of the class `class`, holding `held`, as the entity field
    /// `CLASS::__mNAME`: the entity field `NAME`'s offset when one is defined, else a new one.
    fn member_field(
        &mut self,
        index: &ModuleIndex,
        id: DefId,
        class: &str,
        held: Type,
    ) -> Result<usize, Internal> {
        let definition = index.def(id).ok_or(Internal::NoDefinition)?;
        let name = definition.name.clone();
        // QC-11.12: a vector member's component is a float field at the vector's field and its
        // word, `_x`, `_y` or `_z`.
        // Only a generated component, which a vector member makes, is looked up.
        let generated = COMPONENTS
            .iter()
            .zip(0_u32..)
            .filter(|_| definition.generated);
        let component = generated.into_iter().find_map(|(suffix, word)| {
            let vector = name.strip_suffix(suffix)?;
            let (vector, _) = index.definitions().find(|(_, d)| {
                d.name == vector
                    && d.member_of == definition.member_of
                    && d.kind == DefKind::Field
                    && !d.generated
            })?;
            Some((vector, word))
        });
        if let Some((vector, word)) = component {
            let vector_place = self.get_def(index, vector)?;
            let vector_ofs = self
                .defs
                .get(vector_place)
                .ok_or(Internal::NoDefinition)?
                .ofs;
            let field = self
                .word(vector_ofs)
                .checked_add(word)
                .ok_or(Internal::TooLarge)?;
            let ofs = self.allocate_global(1)?;
            let place = self.defs.len();
            self.defs.push(Def {
                name: format!("{class}::{MEMBER_FIELD_PREFIX}{name}"),
                ty: ValueType::FloatField,
                ofs,
                scope: None,
                initialized: false,
                // FTEF-9.25: as its vector, which a member given 0 is no constant.
                constant: !index.def(vector).is_some_and(|d| d.initialized),
                count: None,
                id: Some(id),
            });
            self.places.insert(id, place);
            self.set_words(ofs, &[field])?;
            return Ok(place);
        }
        // A member a parent class has is the parent's field (`basicindex = pp[i].ofs; found =
        // true;`), for which FTEQCC makes no definition.
        let parent_member = index
            .class_members(&Type::Named(class.into()))
            .into_iter()
            .find(|&other| {
                index.def(other).is_some_and(|d| {
                    d.name == name
                        && d.member_of != definition.member_of
                        && d.kind == DefKind::Field
                })
            });
        if let Some(parent_member) = parent_member {
            let place = self.get_def(index, parent_member)?;
            self.places.insert(id, place);
            return Ok(place);
        }
        let entity_field = index
            .definitions()
            .find(|(_, d)| d.name == name && d.member_of.is_none() && d.kind == DefKind::Field)
            .map(|(other, _)| other);
        let shared = match entity_field {
            Some(other) => {
                let place = self.get_def(index, other)?;
                let ofs = self.defs.get(place).ok_or(Internal::NoDefinition)?.ofs;
                Some(self.word(ofs))
            }
            None => None,
        };
        // FTEF-9.25: the field is a constant, which the field table lists, unless the member
        // is given 0 (`(isnull?0:GDF_CONST)`): a member with a value that is no function's
        // body.
        let bodied = index
            .virtual_function(id)
            .and_then(|function| index.def(function))
            .is_some_and(|function| function.initialized);
        let null = definition.initialized && !bodied;
        let ty = ValueType::Of(Type::Field(held.into()));
        // FTEF-31.33: an array of fields has a global for each word of each element, the
        // offset of that word's field, as a pool of FTEQCC's has (`::vector[N]`); any other
        // member has its field's one global.
        let count = index.array_len(id);
        // An array whose size the model did not read cannot be laid out.
        if index.is_unsized_array(id) {
            return Err(Internal::Unsupported);
        }
        let inner = ty.field_inner().ok_or(Internal::Unsupported)?;
        let words = match count {
            Some(count) => type_words(index, &inner)
                .checked_mul(count)
                .ok_or(Internal::TooLarge)?,
            None => type_words(index, &ty),
        };
        let ofs = self.allocate_global(words)?;
        let place = self.defs.len();
        self.defs.push(Def {
            name: format!("{class}::{MEMBER_FIELD_PREFIX}{name}"),
            ty,
            ofs,
            scope: None,
            initialized: false,
            constant: !null,
            count,
            id: Some(id),
        });
        self.places.insert(id, place);
        let field = match shared {
            Some(word) => word,
            None => {
                let field = self.size_fields;
                let taken = type_words(index, &inner)
                    .checked_mul(count.unwrap_or(1))
                    .ok_or(Internal::TooLarge)?;
                self.size_fields = self
                    .size_fields
                    .checked_add(taken)
                    .ok_or(Internal::TooLarge)?;
                field
            }
        };
        let offsets: Vec<u32> = match count {
            Some(_) => (0..words).map(|word| field.saturating_add(word)).collect(),
            None => vec![field],
        };
        self.set_words(ofs, &offsets)?;
        Ok(place)
    }

    /// FTEF-19.76: makes the field at `place` name the entity field the field at `other`
    /// names: its word becomes `other`'s, a vector field's components each their
    /// counterpart's, and the field space `get_def` allocated for it, when it was the last
    /// allocated, is given back, since FTEQCC allocates none for a field defined with a value.
    pub(crate) fn alias_field(
        &mut self,
        index: &ModuleIndex,
        place: usize,
        other: usize,
    ) -> Result<(), Internal> {
        let def = self.defs.get(place).ok_or(Internal::NoDefinition)?;
        let inner = def.ty.field_inner().ok_or(Internal::Unsupported)?;
        let (own, id) = (self.word(def.ofs), def.id);
        let source = self.defs.get(other).ok_or(Internal::NoDefinition)?;
        if source.ty != def.ty {
            return Err(Internal::Unsupported);
        }
        let (target, source_id) = (self.word(source.ofs), source.id);
        if inner.etype() == Etype::Vector {
            let (id, source_id) = id.zip(source_id).ok_or(Internal::NoDefinition)?;
            for suffix in COMPONENTS {
                let mine = component(index, id, suffix).ok_or(Internal::NoDefinition)?;
                let theirs = component(index, source_id, suffix).ok_or(Internal::NoDefinition)?;
                let (mine, theirs) = (self.get_def(index, mine)?, self.get_def(index, theirs)?);
                let word = self.word(self.def(theirs).ok_or(Internal::NoDefinition)?.ofs);
                self.initialise_words(mine, &[word])?;
            }
        }
        if own.checked_add(type_words(index, &inner)) == Some(self.size_fields) {
            self.size_fields = own;
        }
        self.initialise_words(place, &[target])
    }

    /// Marks the definition at `place` initialised and stores `literal` in its words, as
    /// `PR_ParseDefs` copies `pr_immediate` (QC-11.6, QC-11.10). A string's text is copied to
    /// the string table first and the definition holds its offset, where id's `qcc` stores a
    /// stale word (QC-27.12).
    pub(crate) fn initialise(&mut self, place: usize, literal: &Literal) -> Result<(), Internal> {
        let words = self.literal_words(literal)?;
        self.initialise_words(place, &words)
    }

    /// FTE-15.9: the `IMMEDIATE` definition FTEQCC's `QCC_PR_ParseImmediate` makes for a
    /// literal that an initialised constant at `place` holds, a second record at its offset.
    pub(crate) fn alias_immediate(&mut self, place: usize) -> Result<(), Internal> {
        let def = self.defs.get(place).ok_or(Internal::NoDefinition)?;
        let (ty, ofs) = (def.ty.clone(), def.ofs);
        self.defs.push(Def {
            name: "IMMEDIATE".to_owned(),
            ty,
            ofs,
            scope: None,
            initialized: true,
            constant: true,
            count: None,
            id: None,
        });
        Ok(())
    }

    /// FTE-15.9: the definition `id` as a name for the global of the definition at `at`, an
    /// enum item for the `IMMEDIATE` of its value: a record at that offset, allocating nothing,
    /// initialised and constant. Returns its place.
    pub(crate) fn alias_def(
        &mut self,
        index: &ModuleIndex,
        id: DefId,
        at: usize,
    ) -> Result<usize, Internal> {
        if let Some(place) = self.place(id) {
            return Ok(place);
        }
        let definition = index.def(id).ok_or(Internal::NoDefinition)?;
        let shared = self.defs.get(at).ok_or(Internal::NoDefinition)?;
        let (ty, ofs) = (shared.ty.clone(), shared.ofs);
        let place = self.defs.len();
        self.defs.push(Def {
            name: definition.name.clone(),
            ty,
            ofs,
            scope: definition.owner,
            initialized: true,
            constant: true,
            count: None,
            id: Some(id),
        });
        self.places.insert(id, place);
        Ok(place)
    }

    /// Makes a definition the compiler generates, `name` of type `ty` in the next `words`
    /// globals, that the program names nowhere: an array's getter (FTE-13.8) or its index
    /// parameter.
    pub(crate) fn generated(
        &mut self,
        name: &str,
        ty: ValueType,
        words: u32,
    ) -> Result<usize, Internal> {
        let ofs = self.allocate_global(words)?;
        let place = self.defs.len();
        self.defs.push(Def {
            name: name.to_owned(),
            ty,
            ofs,
            scope: None,
            initialized: false,
            constant: false,
            count: None,
            id: None,
        });
        Ok(place)
    }

    /// [`Program::generated`] of an array of `count` elements of `ty`, FTEQCC's `__va_list`
    /// (FTE-18.6).
    pub(crate) fn generated_array(
        &mut self,
        name: &str,
        ty: ValueType,
        count: u32,
    ) -> Result<usize, Internal> {
        let words = ty.words().checked_mul(count).ok_or(Internal::TooLarge)?;
        let place = self.generated(name, ty, words)?;
        self.defs
            .get_mut(place)
            .ok_or(Internal::NoDefinition)?
            .count = Some(count);
        Ok(place)
    }

    /// Initialises definition `place` with `words`, the value its initialiser folded to.
    pub(crate) fn initialise_words(&mut self, place: usize, words: &[u32]) -> Result<(), Internal> {
        let def = self.defs.get_mut(place).ok_or(Internal::NoDefinition)?;
        def.initialized = true;
        let ofs = def.ofs;
        // FTEF-32.23: a word that names a function not yet defined is filled in at the end.
        for (word, function) in std::mem::take(&mut self.named_functions) {
            let at = ofs.checked_add(word).ok_or(Internal::TooLarge)?;
            self.function_relocs.push((at, function));
        }
        self.set_words(ofs, words)
    }

    /// FTEF-32.23: the function defined at `function` is named by the initialiser being
    /// gathered, before its body; the word it is named at is given by
    /// [`Program::named_functions_at`].
    pub(crate) fn name_function(&mut self, function: usize) {
        self.named_functions.push((0, function));
    }

    /// How many functions the initialiser being gathered has named so far.
    pub(crate) const fn named_functions(&self) -> usize {
        self.named_functions.len()
    }

    /// Puts the functions named since there were `before` of them at word `word` of the
    /// initialiser.
    pub(crate) fn named_functions_at(&mut self, before: usize, word: u32) {
        for (at, _) in self.named_functions.iter_mut().skip(before) {
            *at = word;
        }
    }

    /// FTEF-32.23: every global an initialiser gave a function not yet defined takes the
    /// function's number, now that each has one.
    pub(crate) fn resolve_function_relocs(&mut self) -> Result<(), Internal> {
        for (at, function) in std::mem::take(&mut self.function_relocs) {
            let ofs = self.defs.get(function).ok_or(Internal::NoDefinition)?.ofs;
            let number = self.word(ofs);
            self.set_words(at, &[number])?;
        }
        Ok(())
    }

    /// The words `literal` is stored as; a string's text is copied to the string table.
    pub(crate) fn literal_words(&mut self, literal: &Literal) -> Result<Vec<u32>, Internal> {
        Ok(match literal {
            Literal::Float(value) => vec![value.to_bits()],
            Literal::Vector(v) => v.iter().map(|c| c.to_bits()).collect(),
            Literal::String(text) => {
                let ofs = self.copy_string(text)?;
                vec![u32::from_ne_bytes(ofs.to_ne_bytes())]
            }
        })
    }

    /// `PR_ParseImmediate` (QC-20): the global of `literal`. The first initialised definition
    /// of the literal's type whose words hold an equal value is reused (QC-20.2, QC-20.3);
    /// otherwise a new `IMMEDIATE` definition is made, a string's text copied into the string
    /// table (QC-20.4, QC-20.5).
    pub(crate) fn immediate(&mut self, literal: &Literal) -> Result<usize, Internal> {
        let ty = literal.ty();
        // The value is compared before the type: it rules out nearly every definition with a
        // word or two, where comparing types walks them (#966). Both are pure, so the first
        // match is the one id's scan finds.
        let found = self.defs.iter().position(|d| {
            d.initialized
                && (d.constant || self.literal_sharing == LiteralSharing::Initialised)
                && d.count.is_none()
                && self.holds(d.ofs, literal)
                && d.ty == ty
        });
        if let Some(place) = found {
            return Ok(place);
        }
        // FTEF-3.6: FTEQCC's float constants are made by `QCC_Make32bitConst` too, so one
        // shares an integer's immediate of its bits.
        if let (Literal::Float(value), LiteralSharing::Constants) = (literal, self.literal_sharing)
            && let Some(place) = self.shared_immediate(&[value.to_bits()])
        {
            return Ok(place);
        }
        let ofs = self.allocate_global(ty.words())?;
        let place = self.defs.len();
        self.defs.push(Def {
            name: "IMMEDIATE".to_owned(),
            ty,
            ofs,
            scope: None,
            initialized: true,
            constant: true,
            count: None,
            id: None,
        });
        let words = self.literal_words(literal)?;
        self.set_words(ofs, &words)?;
        Ok(place)
    }

    /// The name of the next translated string's definition, `dotranslate_N`, the N counting
    /// from 1 (`++dotranslate_count`).
    fn next_translation(&mut self) -> Result<String, Internal> {
        self.translated = self.translated.checked_add(1).ok_or(Internal::TooLarge)?;
        Ok(format!("dotranslate_{}", self.translated))
    }

    /// FTEF-32.22: the `dotranslate_N` definition FTEQCC makes for a global at `place` that
    /// `_("...")` initialises, a second record at its offset, a constant string.
    pub(crate) fn alias_translation(&mut self, place: usize) -> Result<(), Internal> {
        let def = self.defs.get(place).ok_or(Internal::NoDefinition)?;
        let (ty, ofs) = (def.ty.clone(), def.ofs);
        let name = self.next_translation()?;
        self.defs.push(Def {
            name,
            ty,
            ofs,
            scope: None,
            initialized: true,
            constant: true,
            count: None,
            id: None,
        });
        Ok(())
    }

    /// `QCC_MakeTranslateStringConst`: the global of the translated string `text`, a string
    /// named `dotranslate_N` that is no constant, the N counting from 1; a string translated
    /// before names the global it made then (FTEF-32.22).
    pub(crate) fn translated(&mut self, text: &[u8]) -> Result<usize, Internal> {
        if let Some(&(_, place)) = self.translations.iter().find(|(made, _)| made == text) {
            return Ok(place);
        }
        let literal = Literal::String(text.to_vec());
        let ofs = self.allocate_global(literal.ty().words())?;
        let place = self.defs.len();
        let name = self.next_translation()?;
        self.defs.push(Def {
            name,
            ty: literal.ty(),
            ofs,
            scope: None,
            initialized: true,
            constant: false,
            count: None,
            id: None,
        });
        let words = self.literal_words(&literal)?;
        self.set_words(ofs, &words)?;
        self.translations.push((text.to_vec(), place));
        Ok(place)
    }

    /// `QCC_Make32bitConst` and `QCC_Make64bitConst`'s search of the constants they made
    /// (FTEF-3.6): the `IMMEDIATE` of a float, an integer of either sign, or of 64 bits, or a
    /// double, that has as many words as `words` and holds them, whatever its type (`if
    /// (cn->type->size == type->size) if (((QCC_eval_t*)cn->symboldata)->_uint == value)`).
    fn shared_immediate(&self, words: &[u32]) -> Option<usize> {
        let one_word = [CScalar::Integer, CScalar::Uint];
        let two_words = [CScalar::Int64, CScalar::Uint64, CScalar::Double];
        self.defs.iter().position(|d| {
            let sized = match &d.ty {
                ValueType::Of(Type::Simple(SimpleType::Float)) => words.len() == 1,
                ValueType::Of(Type::Named(name)) => {
                    let named = |scalars: &[CScalar]| {
                        scalars.iter().any(|scalar| scalar.name() == name.as_ref())
                    };
                    (words.len() == 1 && named(&one_word))
                        || (words.len() == 2 && named(&two_words))
                }
                ValueType::Of(_) | ValueType::FloatField => false,
            };
            sized
                && d.name == "IMMEDIATE"
                && d.initialized
                && d.constant
                && d.count.is_none()
                && (0_u32..)
                    .zip(words)
                    .all(|(at, word)| self.word(d.ofs.saturating_add(at)) == *word)
        })
    }

    /// `QCC_MakeIntConst`: the global of an integer constant of type `ty`, whose one word is
    /// `bits`; an `IMMEDIATE` of that type holding the word is reused.
    pub(crate) fn int_immediate(&mut self, bits: u32, ty: ValueType) -> Result<usize, Internal> {
        if let Some(place) = self.shared_immediate(&[bits]) {
            return Ok(place);
        }
        let ofs = self.allocate_global(1)?;
        let place = self.defs.len();
        self.defs.push(Def {
            name: "IMMEDIATE".to_owned(),
            ty,
            ofs,
            scope: None,
            initialized: true,
            constant: true,
            count: None,
            id: None,
        });
        self.set_words(ofs, &[bits])?;
        Ok(place)
    }

    /// `QCC_MakeInt64Const` and `QCC_MakeDoubleConst`: the global of a constant of two words of
    /// type `ty`, the low word first; an `IMMEDIATE` of that type holding them is reused.
    pub(crate) fn wide_immediate(
        &mut self,
        words: [u32; 2],
        ty: ValueType,
    ) -> Result<usize, Internal> {
        if let Some(place) = self.shared_immediate(&words) {
            return Ok(place);
        }
        let ofs = self.allocate_global(2)?;
        let place = self.defs.len();
        self.defs.push(Def {
            name: "IMMEDIATE".to_owned(),
            ty,
            ofs,
            scope: None,
            initialized: true,
            constant: true,
            count: None,
            id: None,
        });
        self.set_words(ofs, &words)?;
        Ok(place)
    }

    /// Whether the words from `ofs` hold `literal`'s value, compared as `PR_ParseImmediate`
    /// compares: floats and each vector component with `==`, strings by their text at the
    /// stored offset (QC-20.2). FTEQCC shares a number's immediate by its bits
    /// (`QCC_Make32bitConst`, `QCC_Make96bitConst`, FTE-20.5), so `-0` and `0` are two.
    #[expect(
        clippy::float_cmp,
        reason = "spec QC-20.2: qcc compares the floats with =="
    )]
    fn holds(&self, ofs: u32, literal: &Literal) -> bool {
        let word = |i: u32| self.word(ofs.saturating_add(i));
        let same = |i: u32, value: f32| match self.literal_sharing {
            LiteralSharing::Initialised => f32::from_bits(word(i)) == value,
            LiteralSharing::Constants => word(i) == value.to_bits(),
        };
        match literal {
            Literal::Float(value) => same(0, *value),
            Literal::Vector(v) => same(0, v[0]) && same(1, v[1]) && same(2, v[2]),
            Literal::String(text) => self.string_at(self.word(ofs)) == Some(text.as_slice()),
        }
    }

    /// `PR_Statement` for an opcode with no result: an assignment, `IF`, `IFNOT`, `GOTO`,
    /// `RETURN`, `STATE`, a call or `DONE` (QC-21.5); returns its index.
    pub(crate) fn emit(&mut self, op: Opcode, a: i32, b: i32) -> usize {
        self.origins.push(self.origin);
        self.statements.push(Statement {
            op: u32::from(op.number()),
            a,
            b,
            c: 0,
        });
        self.statements.len().saturating_sub(1)
    }

    /// `PR_Statement` for an opcode with a result: its operand c is a new temporary of the
    /// result type's size, never reused (QC-21.5). Returns the temporary's offset.
    pub(crate) fn emit_result(
        &mut self,
        op: Opcode,
        a: u32,
        b: u32,
        result: Etype,
    ) -> Result<u32, Internal> {
        let words = if result == Etype::Vector { 3 } else { 1 };
        let c = self.allocate(words)?;
        self.emit_into(op, a, b, c)?;
        Ok(c)
    }

    /// `QCC_PR_SimpleStatement`: `op` of `a` and `b` into the global `c` the caller chose.
    /// Emits `op` with the operands as written, a jump's offset among them, at the origin.
    /// `PR_Statement` of the opcode numbered `op`, one of FTE's beyond id's among them
    /// (FTEF-25), with operands already given.
    pub(crate) fn emit_number(&mut self, op: u32, [a, b, c]: [i32; 3]) {
        self.origins.push(self.origin);
        self.statements.push(Statement { op, a, b, c });
    }

    pub(crate) fn emit_into(&mut self, op: Opcode, a: u32, b: u32, c: u32) -> Result<(), Internal> {
        self.origins.push(self.origin);
        self.statements.push(Statement {
            op: u32::from(op.number()),
            a: operand(a)?,
            b: operand(b)?,
            c: operand(c)?,
        });
        Ok(())
    }
}

/// The words a value of `ty` takes: a struct's layout's (FTE-14.5), a named type's record's,
/// else `type_size`.
pub(crate) fn type_words(index: &ModuleIndex, ty: &ValueType) -> u32 {
    match ty {
        ValueType::Of(t) => index
            .struct_shape(t)
            .map(|shape| shape.words)
            .or_else(|| index.record(t).map(|record| record.words))
            .unwrap_or_else(|| ty.words()),
        ValueType::FloatField => ty.words(),
    }
}

/// The type number of a definition's record of type `ty`: a named type's own, which the model
/// gives (`ModuleIndex::record`), else its basic type's.
pub(crate) fn type_number(index: &ModuleIndex, ty: &ValueType) -> u16 {
    let record = match ty {
        ValueType::Of(t) => index.record(t),
        ValueType::FloatField => None,
    };
    record.map_or_else(|| ty.etype().number(), |record| record.number)
}

/// A global offset as a statement operand.
pub(crate) fn operand(ofs: u32) -> Result<i32, Internal> {
    i32::try_from(ofs).map_err(|_| Internal::TooLarge)
}

/// The component `suffix` of vector definition `vector`: the one the semantic model made with
/// the vector, at its name and in its function (QC-11.7); else the first with that name that
/// is a global or belongs to the same function (QC-12.2).
pub(crate) fn component(index: &ModuleIndex, vector: DefId, suffix: &str) -> Option<DefId> {
    let base = index.def(vector)?;
    let name = format!("{}{suffix}", base.name);
    let named = |d: &&qc_sema::Definition| d.name == name && d.member_of.is_none();
    // QC-11.7: the components are made at the vector's name, so a local vector's are its own,
    // not a global's of the same names, such as a field `velocity`'s `velocity_z`.
    let made_with = index
        .definitions()
        .find(|(_, d)| named(d) && d.location == base.location && d.owner == base.owner);
    made_with
        .or_else(|| {
            index
                .definitions()
                .find(|(_, d)| named(d) && (d.owner.is_none() || d.owner == base.owner))
        })
        .map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use qc_progs::Statement;

    use super::{BLOCK_BASE, Program, SHARED_BASE};
    use crate::Internal;

    // spec FTE-21.8: the shared temporaries are laid out after every global made so far, each
    // operand marked as naming one taking its real offset; any other keeps its own, whatever
    // its number, and a module with none lays out nothing
    #[test]
    fn shared_temporaries_are_laid_out_after_the_globals() {
        let mut program = Program::new();
        program.resolve_shared_temps().unwrap();
        let globals = program.globals.len();
        assert_eq!(program.shared_temp(1).unwrap(), SHARED_BASE);
        assert_eq!(program.shared_temp(3).unwrap(), SHARED_BASE + 1);
        let shared = i32::try_from(SHARED_BASE).unwrap();
        program.statements.push(Statement {
            op: 1,
            a: shared + 1,
            b: shared,
            c: shared,
        });
        program.mark_shared(0);
        program.mark_shared(2);
        program.resolve_shared_temps().unwrap();
        let base = i32::try_from(globals).unwrap();
        let statement = program.statements.last().unwrap();
        assert_eq!(
            (statement.a, statement.b, statement.c),
            (base + 1, shared, base),
            "only the operands marked"
        );
        assert_eq!(program.globals.len(), globals + 4);
        assert!(program.shared_temps().is_empty());
    }

    // spec FTE-21.8, FTE-21.9: a locked temporary is laid out after the block's locals, even one
    // declared after it
    #[test]
    fn locked_temporaries_follow_the_locals() {
        let mut program = Program::new();
        program.open_block();
        let a = program.allocate(1).unwrap();
        let locked = program.locked_temp(1).unwrap();
        let b = program.allocate(1).unwrap();
        for ofs in [a, b, locked] {
            let ofs = i32::try_from(ofs).unwrap();
            program.statements.push(Statement {
                op: 1,
                a: ofs,
                b: 0,
                c: 0,
            });
        }
        assert_eq!(
            program.close_block(0).unwrap(),
            3,
            "two locals and the locked one"
        );
        let at: Vec<i32> = program.statements.iter().skip(1).map(|s| s.a).collect();
        let differences: Vec<i32> = at.iter().map(|ofs| ofs - at.first().unwrap()).collect();
        assert_eq!(differences, [0, 1, 2], "{at:?}");
    }

    // spec FTE-21.8: the shared temporaries end below the first block's virtual offset, and
    // while any wait the globals stay below theirs
    #[test]
    fn shared_temporaries_and_globals_keep_to_their_ranges() {
        let mut program = Program::new();
        program.shared_temp(BLOCK_BASE - SHARED_BASE).unwrap();
        assert_eq!(program.shared_temp(1), Err(Internal::TooLarge));
        let room = SHARED_BASE - u32::try_from(program.globals.len()).unwrap();
        assert_eq!(program.allocate_global(room + 1), Err(Internal::TooLarge));
        assert!(program.allocate_global(room).is_ok());
    }
}
