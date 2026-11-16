// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's classes' spawn functions (FTEF-9.22).
//!
//! A class's spawn function, `spawnfunc_CLASS`, that the program gives no body has one the
//! semantic model writes (`qc_sema::ModuleIndex::spawn_functions`); FTEQCC writes it after every
//! file, and this writes it after the array functions, from its statements as any body.

use qc_diag::Span;
use qc_lexer::latin1_encode;
use qc_progs::Function;
use qc_sema::{DefId, FileId, Location};

use super::{Abort, At, Compiler, Lowered, operand_index};
use crate::Internal;
use crate::program::operand;

impl Compiler<'_> {
    /// Each class's spawn function the semantic model wrote a body for (FTEF-9.22).
    pub(crate) fn emit_class_functions(&mut self) -> Result<(), (Internal, FileId, Span)> {
        for &id in self.index.spawn_functions() {
            let Some(location) = self.index.def(id).map(|d| d.location) else {
                continue;
            };
            if let Err(Abort::Internal(internal, at)) = self.spawn_function(id, location) {
                return Err((internal, location.file, at));
            }
        }
        Ok(())
    }

    /// The spawn function `id`, declared at `location`: its body's statements, then its record,
    /// with no parameters (FTEF-9.22).
    fn spawn_function(&mut self, id: DefId, Location { file, span }: Location) -> Lowered<()> {
        self.file = file;
        self.program.origin = Some((file, span.start));
        let place = self.program.get_def(self.index, id).at(span)?;
        let typed = self
            .index
            .body(id)
            .ok_or(Abort::Internal(Internal::Unfollowed, span))?;
        let parm_start = self.program.next_global().at(span)?;
        let first_statement = operand_index(self.program.statements.len()).at(span)?;
        self.scope = Some(id);
        let written = self.write_body(typed, None, span);
        self.scope = None;
        written?;
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
        let s_name = self.program.copy_string(&latin1_encode(&name)).at(span)?;
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
}
