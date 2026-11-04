// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/pr_comp.c, PR_GetDef, PR_ParseImmediate, PR_Statement
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/qcc.c, InitData, PR_BeginCompilation, CopyString

//! The program being built: `qcc`'s definition list, global area, string table, statements
//! and functions, as an explicit value rather than id's globals.
//!
//! The semantic model decides which definition a name is (QC-12); this module gives each
//! definition its place the first time the compiler meets it, as the allocating half of
//! `PR_GetDef` does (QC-21), shares literals as `PR_ParseImmediate` does (QC-20), and emits
//! statements as `PR_Statement` does, a new temporary for every result (QC-21.5).

use std::collections::HashMap;
use std::rc::Rc;

use qc_progs::opcode::Opcode;
use qc_progs::{Function, Statement};
use qc_sema::{DefId, FileId, FunctionType, ModuleIndex, SimpleType, Type};
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

    /// A pointer to a value of this type, `QCC_PR_PointerType` (FTEF-31.4).
    pub(crate) fn pointer_to(&self) -> Self {
        let inner = match self {
            Self::FloatField => Type::Field(Rc::new(Type::Simple(SimpleType::Float))),
            Self::Of(ty) => ty.clone(),
        };
        Self::Of(Type::Pointer(Rc::new(inner)))
    }

    /// For a pointer type, the type of what it points to (FTEF-6.18).
    pub(crate) fn pointee(&self) -> Option<Self> {
        match self {
            Self::Of(Type::Pointer(inner)) => Some(Self::Of(Type::clone(inner))),
            Self::Of(_) | Self::FloatField => None,
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

/// The first virtual offset of a function's block of private locals (FTE-21.7): parameters,
/// locals and temporaries take offsets from here while the body is compiled, and real ones,
/// after every global, static and immediate, when the module's blocks are laid out.
const BLOCK_BASE: u32 = 1 << 20;

/// The virtual offsets a closed block keeps until the module's blocks are laid out: its own
/// range, so no two functions' definitions or temporaries share a virtual offset.
const BLOCK_SPAN: u32 = 1 << 16;

/// A function's block of private locals while its body is compiled (FTE-21.7).
#[derive(Debug)]
struct Block {
    /// The next virtual offset.
    next: u32,
    /// The first definition made since the block opened.
    defs_from: usize,
    /// The first statement made since the block opened.
    statements_from: usize,
    /// The words written to the block's offsets, to be stored once they are real.
    inits: Vec<(u32, Vec<u32>)>,
}

/// A function's block of private locals, closed and waiting for its real offsets
/// (FTE-21.7).
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
    /// The block of the function whose body is being compiled, under FTEQCC (FTE-21.7).
    block: Option<Block>,
    /// The blocks closed so far, laid out by [`Program::resolve_blocks`] (FTE-21.7).
    pending_blocks: Vec<PendingBlock>,
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
        }
    }

    /// FTE-21.7: opens the block of private locals of the function whose body follows: until
    /// [`Program::close_block`], a local, a parameter or a temporary takes a virtual offset.
    pub(crate) fn open_block(&mut self) {
        self.block = Some(Block {
            next: BLOCK_BASE,
            defs_from: self.defs.len(),
            statements_from: self.statements.len(),
            inits: Vec::new(),
        });
    }

    /// FTE-21.7: closes the open block as the one of function `record`, to be laid out with
    /// the module's others; returns its words, the record's `locals`.
    pub(crate) fn close_block(&mut self, record: usize) -> Result<u32, Internal> {
        let block = self.block.take().ok_or(Internal::NoDefinition)?;
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

    /// FTE-21.7: the block closed as function `from`'s becomes function `to`'s, as a wrap
    /// moves the record of the body it wraps to the end (FTEF-15.11).
    pub(crate) fn rekey_block(&mut self, from: usize, to: usize) {
        for block in &mut self.pending_blocks {
            if block.record == from {
                block.record = to;
            }
        }
    }

    /// FTE-21.7: `QCC_UnmarshalLocals`: lays out every closed block after every global,
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
    /// a temporary (FTE-21.7), else of the globals; returns the first.
    pub(crate) fn allocate(&mut self, words: u32) -> Result<u32, Internal> {
        if let Some(block) = &mut self.block {
            let ofs = block.next;
            block.next = ofs.checked_add(words).ok_or(Internal::TooLarge)?;
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
        // FTE-21.7: a word of the open block is written when the block is laid out.
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
        // FTE-21.7: a function's own definition takes the open block's offsets, a global the
        // globals' whatever block is open.
        let in_block = definition.owner.is_some() && self.block.is_some();
        let ofs = if in_block {
            self.next_offset()?
        } else {
            self.next_global()?
        };
        let place = self.defs.len();
        let count = index.array_len(id);
        self.defs.push(Def {
            name: definition.name.clone(),
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
        } else if in_block {
            self.allocate(type_words(index, &ty))?;
        } else {
            self.allocate_global(type_words(index, &ty))?;
        }
        if let Some(inner) = ty.field_inner() {
            self.set_words(ofs, &[self.size_fields])?;
            if inner.etype() == Etype::Vector {
                for suffix in COMPONENTS {
                    let component = component(index, id, suffix).ok_or(Internal::NoDefinition)?;
                    self.get_def_typed(index, component, Some(ValueType::FloatField))?;
                }
            } else {
                self.size_fields = self
                    .size_fields
                    .checked_add(inner.words())
                    .ok_or(Internal::TooLarge)?;
            }
        }
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
        if own.checked_add(inner.words()) == Some(self.size_fields) {
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
        self.set_words(ofs, words)
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
        let found = self.defs.iter().position(|d| {
            d.initialized
                && (d.constant || self.literal_sharing == LiteralSharing::Initialised)
                && d.count.is_none()
                && d.ty == ty
                && self.holds(d.ofs, literal)
        });
        if let Some(place) = found {
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

    /// `QCC_MakeIntConst`: the global of an integer constant of type `ty`, whose one word is
    /// `bits`; an `IMMEDIATE` of that type holding the word is reused.
    pub(crate) fn int_immediate(&mut self, bits: u32, ty: ValueType) -> Result<usize, Internal> {
        let found = self.defs.iter().position(|d| {
            d.initialized
                && d.constant
                && d.count.is_none()
                && d.ty == ty
                && self.word(d.ofs) == bits
        });
        if let Some(place) = found {
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
    pub(crate) fn emit_operands(&mut self, op: Opcode, [a, b, c]: [i32; 3]) {
        self.origins.push(self.origin);
        self.statements.push(Statement {
            op: u32::from(op.number()),
            a,
            b,
            c,
        });
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

/// The words a value of `ty` takes: a struct's layout's (FTE-14.5), else `type_size`.
pub(crate) fn type_words(index: &ModuleIndex, ty: &ValueType) -> u32 {
    match ty {
        ValueType::Of(t) => index
            .struct_shape(t)
            .map_or_else(|| ty.words(), |s| s.words),
        ValueType::FloatField => ty.words(),
    }
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
