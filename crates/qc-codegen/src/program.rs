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
        }
    }

    /// `numpr_globals`: the next free global offset.
    pub(crate) fn next_global(&self) -> Result<u32, Internal> {
        u32::try_from(self.globals.len()).map_err(|_| Internal::TooLarge)
    }

    /// Takes the next `words` global offsets (QC-21.2); returns the first.
    fn allocate(&mut self, words: u32) -> Result<u32, Internal> {
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
        let ty = ty.unwrap_or_else(|| ValueType::Of(definition.ty.clone()));
        let ofs = self.next_global()?;
        let place = self.defs.len();
        self.defs.push(Def {
            name: definition.name.clone(),
            ty: ty.clone(),
            ofs,
            scope: definition.owner,
            initialized: false,
        });
        self.places.insert(id, place);
        if ty.etype() == Etype::Vector {
            for suffix in COMPONENTS {
                let component = component(index, id, suffix).ok_or(Internal::NoDefinition)?;
                self.get_def_typed(index, component, None)?;
            }
        } else {
            self.allocate(ty.words())?;
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

    /// Marks the definition at `place` initialised and stores `literal` in its words, as
    /// `PR_ParseDefs` copies `pr_immediate` (QC-11.6, QC-11.10). A string's text is copied to
    /// the string table first and the definition holds its offset, where id's `qcc` stores a
    /// stale word (QC-27.12).
    pub(crate) fn initialise(&mut self, place: usize, literal: &Literal) -> Result<(), Internal> {
        let words = self.literal_words(literal)?;
        self.initialise_words(place, &words)
    }

    /// Initialises definition `place` with `words`, the value its initialiser folded to.
    pub(crate) fn initialise_words(&mut self, place: usize, words: &[u32]) -> Result<(), Internal> {
        let def = self.defs.get_mut(place).ok_or(Internal::NoDefinition)?;
        def.initialized = true;
        let ofs = def.ofs;
        self.set_words(ofs, &words)
    }

    /// The words `literal` is stored as; a string's text is copied to the string table.
    fn literal_words(&mut self, literal: &Literal) -> Result<Vec<u32>, Internal> {
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
        let found = self
            .defs
            .iter()
            .position(|d| d.initialized && d.ty == ty && self.holds(d.ofs, literal));
        if let Some(place) = found {
            return Ok(place);
        }
        let ofs = self.allocate(ty.words())?;
        let place = self.defs.len();
        self.defs.push(Def {
            name: "IMMEDIATE".to_owned(),
            ty,
            ofs,
            scope: None,
            initialized: true,
        });
        let words = self.literal_words(literal)?;
        self.set_words(ofs, &words)?;
        Ok(place)
    }

    /// Whether the words from `ofs` hold `literal`'s value, compared as `PR_ParseImmediate`
    /// compares: floats and each vector component with `==`, strings by their text at the
    /// stored offset (QC-20.2).
    #[expect(
        clippy::float_cmp,
        reason = "spec QC-20.2: qcc compares the floats with =="
    )]
    fn holds(&self, ofs: u32, literal: &Literal) -> bool {
        let float = |i: u32| f32::from_bits(self.word(ofs.saturating_add(i)));
        match literal {
            Literal::Float(value) => float(0) == *value,
            Literal::Vector(v) => float(0) == v[0] && float(1) == v[1] && float(2) == v[2],
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
        self.origins.push(self.origin);
        self.statements.push(Statement {
            op: u32::from(op.number()),
            a: operand(a)?,
            b: operand(b)?,
            c: operand(c)?,
        });
        Ok(c)
    }
}

/// A global offset as a statement operand.
pub(crate) fn operand(ofs: u32) -> Result<i32, Internal> {
    i32::try_from(ofs).map_err(|_| Internal::TooLarge)
}

/// The component `suffix` of vector definition `vector`: the definition `PR_GetDef` finds for
/// the component's name in the vector's scope, the first with that name that is a global or
/// belongs to the same function (QC-12.2), which the semantic model made with the vector or
/// before it (QC-11.7).
fn component(index: &ModuleIndex, vector: DefId, suffix: &str) -> Option<DefId> {
    let base = index.def(vector)?;
    let name = format!("{}{suffix}", base.name);
    index
        .defs
        .iter()
        .position(|d| {
            d.name == name && d.member_of.is_none() && (d.owner.is_none() || d.owner == base.owner)
        })
        .map(DefId)
}
