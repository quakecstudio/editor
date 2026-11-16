// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_EmitClassFromFunction, QCC_PR_EmitClassFunctionTable

//! FTEQCC's classes (FTEF-9.21, FTEF-9.22).
//!
//! A class is an entity type whose data and virtual members are entity fields. Each class has
//! a spawn function, `spawnfunc_CLASS`, that `spawn(CLASS)` calls with the new entity as
//! `self` and a map spawns the class by: when the program writes no body for it, FTEQCC writes
//! one after every file, which names the entity's class, fills each virtual member's field with
//! its function and calls each constructor.

use qc_diag::Span;
use qc_progs::Function;
use qc_progs::opcode::Opcode;
use qc_sema::{DefId, DefKind, FileId, Location, Type};
use qc_types::Etype;

use super::{Abort, At, Compiler, Lowered, latin1, operand_index};
use crate::Internal;
use crate::program::operand;

/// The prefix of a class's spawn function's name, before the class's (FTEF-9.5).
const SPAWNFUNC: &str = "spawnfunc_";

impl Compiler<'_> {
    /// `QCC_PR_EmitClassFromFunction` for each class's spawn function the program writes no
    /// body for, after every file and the array functions (FTEF-9.22).
    pub(crate) fn emit_class_functions(&mut self) -> Result<(), (Internal, FileId, Span)> {
        let spawnfuncs: Vec<(DefId, String, Location)> = self
            .index
            .definitions()
            .filter(|(_, d)| d.generated && d.kind == DefKind::Function && d.member_of.is_none())
            .filter_map(|(id, d)| {
                Some((id, d.name.strip_prefix(SPAWNFUNC)?.to_owned(), d.location))
            })
            .collect();
        for (id, class, location) in spawnfuncs {
            if let Err(Abort::Internal(internal, at)) = self.emit_spawnfunc(id, &class, location) {
                return Err((internal, location.file, at));
            }
        }
        Ok(())
    }

    /// FTEF-9.22: the body of `spawnfunc_CLASS`, `spawnfunc`, for the class `class`:
    /// `self.classname = "CLASS"` when the program has a `classname` field; then for each
    /// class from the root down, each virtual member no class below it declares again stored
    /// its function, `self.__mNAME = CLASS::NAME` (`QCC_PR_EmitClassFunctionTable`); then each
    /// constructor `CLASS::CLASS` called, the class's first and its parents' after; then `DONE`.
    fn emit_spawnfunc(
        &mut self,
        spawnfunc: DefId,
        class: &str,
        Location { file, span }: Location,
    ) -> Lowered<()> {
        self.file = file;
        self.program.origin = Some((file, span.start));
        let place = self.program.get_def(self.index, spawnfunc).at(span)?;
        if self.program.def(place).at(span)?.initialized {
            return Ok(());
        }
        let members = self.index.class_members(&Type::Named(class.into()));
        // The class, then each parent: the classes the members belong to, in their order.
        let mut chain: Vec<DefId> = Vec::new();
        for member in &members {
            if let Some(owner) = self.index.def(*member).and_then(|d| d.member_of)
                && !chain.contains(&owner)
            {
                chain.push(owner);
            }
        }
        let this = self
            .global_place("self", span)?
            .ok_or(Abort::Internal(Internal::Unsupported, span))?;
        let parm_start = self.program.next_global().at(span)?;
        let first_statement = operand_index(self.program.statements.len()).at(span)?;
        let classname = self
            .names
            .global("classname")
            .filter(|&id| self.index.def(id).is_some_and(|d| d.kind == DefKind::Field));
        if let Some(classname) = classname {
            let field = self.def_ofs(classname, span)?;
            let name = self.string_immediate(latin1(class), span)?;
            self.store_through(this, field, name, span)?;
        }
        // `QCC_PR_EmitClassFunctionTable` recurses to the root before it reads a class.
        for (depth, owner) in chain.iter().enumerate().rev() {
            let below = chain.get(..depth).unwrap_or_default();
            for &member in &members {
                let Some(def) = self.index.def(member) else {
                    continue;
                };
                let virtual_function = def.member_of == Some(*owner)
                    && def.kind == DefKind::Field
                    && matches!(def.ty, Type::Function(_));
                if !virtual_function {
                    continue;
                }
                // A child class that declares the member again fills the field itself.
                let overridden = members.iter().any(|&other| {
                    self.index.def(other).is_some_and(|o| {
                        o.name == def.name && o.member_of.is_some_and(|c| below.contains(&c))
                    })
                });
                // "Member function %s was not defined": a member with no body is left empty.
                let Some(function) = self.program.member_function_made(member) else {
                    continue;
                };
                if overridden {
                    continue;
                }
                let field = self.def_ofs(member, span)?;
                let function = self.program.def(function).at(span)?.ofs;
                self.store_through(this, field, function, span)?;
            }
        }
        for owner in &chain {
            let Some(owner_name) = self.index.def(*owner).map(|d| d.name.clone()) else {
                continue;
            };
            let constructor = members.iter().copied().find(|&member| {
                self.index.def(member).is_some_and(|d| {
                    d.member_of == Some(*owner)
                        && d.kind == DefKind::Function
                        && d.name == owner_name
                })
            });
            if let Some(constructor) = constructor {
                let ofs = self.def_ofs(constructor, span)?;
                self.program.emit(Opcode::Call0, operand(ofs).at(span)?, 0);
            }
        }
        self.program.emit(Opcode::Done, 0, 0);
        let locals = self
            .program
            .next_global()
            .at(span)?
            .checked_sub(parm_start)
            .at(span)?;
        let number = operand_index(self.program.functions.len()).at(span)?;
        self.program
            .initialise_words(place, &[u32::from_ne_bytes(number.to_ne_bytes())])
            .at(span)?;
        let name = self.program.def(place).at(span)?.name.clone();
        let s_name = self.program.copy_string(&latin1(&name)).at(span)?;
        let s_file = self.s_files.get(&file).copied().at(span)?;
        self.program.functions.push(Function {
            first_statement,
            parm_start: operand(parm_start).at(span)?,
            locals: operand(locals).at(span)?,
            profile: 0,
            s_name,
            s_file,
            numparms: 0,
            parm_size: Default::default(),
        });
        Ok(())
    }

    /// `point = OP_ADDRESS(entity, field); OP_STOREP_FNC(value, point)`, as FTEQCC stores a
    /// spawn function's words, a string's included.
    fn store_through(&mut self, entity: u32, field: u32, value: u32, span: Span) -> Lowered<()> {
        let point = self
            .program
            .emit_result(Opcode::Address, entity, field, Etype::Pointer)
            .at(span)?;
        self.program.emit(
            Opcode::StorepFnc,
            operand(value).at(span)?,
            operand(point).at(span)?,
        );
        Ok(())
    }

    /// The offset of the global `name`, made now if it is new; `None` when none is defined.
    fn global_place(&mut self, name: &str, span: Span) -> Lowered<Option<u32>> {
        match self.names.global(name) {
            Some(id) => self.def_ofs(id, span).map(Some),
            None => Ok(None),
        }
    }

    /// The offset of the definition `id`, made now if it is new.
    fn def_ofs(&mut self, id: DefId, span: Span) -> Lowered<u32> {
        let place = self.program.get_def(self.index, id).at(span)?;
        Ok(self.program.def(place).at(span)?.ofs)
    }
}
