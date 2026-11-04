// SPDX-License-Identifier: GPL-3.0-or-later

//! The writer of a function's code from its checked body (`TypedBody::generated`), the
//! intermediate layer's whole body (`docs/design/back-end.md`): each local declared before the
//! statement the body declares it at, each value a statement makes given a slot of its own by
//! the allocation pass, in the order the statements make them (QC-21.5), and each name
//! resolved to its global. A body the semantic model did not follow in full, or that names
//! something the writer cannot resolve, is left to the syntax walk.

use std::collections::HashMap;

use qc_diag::Span;
use qc_progs::opcode::Opcode;
use qc_sema::ir::{BodyStatement, Effect, Name};
use qc_sema::lower::{Qualifier, declares};
use qc_sema::{DefId, FileId, TypedBody};
use qc_syntax::ast::{self, AstNode as _};

use super::{At, Compiler, Lowered, Written};
use crate::Internal;
use crate::program::{Literal, OFS_RETURN, operand, type_words};

impl Compiler<'_> {
    /// Writes `body`'s statements as the code of the function being compiled, `named` the span
    /// of its name, the origin of the statements before its first source statement. Whether it
    /// wrote them: a body [`writable`] refuses is left to the syntax walk.
    pub(super) fn write_body(
        &mut self,
        body: &TypedBody,
        syntax: &ast::FunctionBody,
        named: Span,
    ) -> Lowered<bool> {
        // A bare `return` names the `ret*` local under FTEQCC (FTE-18.10); id's `qcc` writes
        // `RETURN` of nothing whatever the function returns (QC-13.8).
        let ret_local = self.returns_value() && self.dialect.has_fteqcc_extensions();
        if !writable(body, ret_local) || !self.defaults_kept(body) {
            return Ok(false);
        }
        self.writing = Some(syntax.clone());
        let written = self.write_statements(body, syntax, named);
        self.writing = None;
        written.map(|()| true)
    }

    /// The statements of `body`, whose syntax is `syntax`, written in turn with what the body
    /// makes between them.
    fn write_statements(
        &mut self,
        body: &TypedBody,
        syntax: &ast::FunctionBody,
        named: Span,
    ) -> Lowered<()> {
        let generated = body.generated();
        let mut slots: HashMap<u32, u32> = HashMap::new();
        // The global the first parameter slot was last stored from, which a precache records.
        let mut first_argument = None;
        let mut effects = body.effects().iter().peekable();
        // How many array functions there were as each `for` loop open here began.
        let mut loops = Vec::new();
        for (index, statement) in (0_u32..).zip(generated) {
            while let Some(&&(at, effect)) = effects.peek()
                && at <= index
            {
                // QC-21.5: a vector taken before its words are written, the value of this
                // statement, which writes its first word.
                match effect {
                    Effect::Block(words) => {
                        let slot = self.program.allocate(words).at(named)?;
                        slots.insert(index, slot);
                    }
                    Effect::ForBegin => loops.push(self.array_functions.len()),
                    // FTEF-19.78: the local arrays' functions the loop made are forgotten.
                    Effect::ForEnd => {
                        let mark = loops.pop().unwrap_or(self.array_functions.len());
                        self.forget_local_array_functions(mark);
                    }
                    Effect::Constant(local) => self.constant_local(syntax, local, named)?,
                    Effect::Autocvar(cvar) => self.autocvar_default(syntax, cvar, named)?,
                    Effect::Nested(Name::Def(local, _)) => {
                        self.nested_local(syntax, local, named)?;
                    }
                    Effect::Make(_) | Effect::Temp(_) | Effect::Nested(_) => {
                        self.effect(effect, index, (body, &slots), named)?;
                    }
                }
                effects.next();
            }
            let at = usize::try_from(index)
                .map_err(|_| Internal::TooLarge)
                .at(named)?;
            let origin = body.origin(at).unwrap_or(named.start);
            self.program.origin = Some((self.file, origin));
            if let Some(words) = statement.temp_words(index) {
                let slot = self.program.allocate(words).at(named)?;
                slots.insert(index, slot);
            }
            let op = u32::try_from(statement.op.number())
                .ok()
                .and_then(Opcode::from_number)
                .at(named)?;
            let mut operands = [0_i32; 3];
            for (word, name) in operands
                .iter_mut()
                .zip([statement.a, statement.b, statement.c])
            {
                if let Some(name) = name {
                    *word = self.resolve(name, index, (body, &slots), named)?;
                }
            }
            self.program.emit_operands(op, operands);
            // FTE-18.4: a precache builtin's first argument is recorded.
            let [a, _, _] = operands;
            match (statement.b, statement.c) {
                (Some(Name::Parameter(0)), _) => first_argument = u32::try_from(a).ok(),
                (_, Some(Name::Parameter(0))) => first_argument = None,
                _ => {}
            }
            if let Some(Name::Def(callee, _)) = statement.a
                && is_call(op)
            {
                if let (Some(def), Some(ofs)) = (self.index.def(callee), first_argument) {
                    self.precaches.record(&def.name, &self.program, ofs);
                }
                first_argument = None;
            }
        }
        let end = u32::try_from(generated.len())
            .map_err(|_| Internal::TooLarge)
            .at(named)?;
        for &(_, effect) in effects {
            match effect {
                Effect::ForBegin => loops.push(self.array_functions.len()),
                Effect::ForEnd => {
                    let mark = loops.pop().unwrap_or(self.array_functions.len());
                    self.forget_local_array_functions(mark);
                }
                Effect::Constant(local) => self.constant_local(syntax, local, named)?,
                Effect::Autocvar(cvar) => self.autocvar_default(syntax, cvar, named)?,
                Effect::Nested(Name::Def(local, _)) => {
                    self.nested_local(syntax, local, named)?;
                }
                Effect::Make(_) | Effect::Temp(_) | Effect::Block(_) | Effect::Nested(_) => {
                    self.effect(effect, end, (body, &slots), named)?;
                }
            }
        }
        Ok(())
    }

    /// FTEF-18.2: the nested function the local `local` of the body `syntax` is given, numbered
    /// and kept to compile after this one: the immediate of its number when the local stores
    /// it each time the function runs, else a `static` local's value, set now.
    fn nested_local(
        &mut self,
        syntax: &ast::FunctionBody,
        local: DefId,
        span: Span,
    ) -> Lowered<Option<usize>> {
        let at = self.index.def(local).at(span)?.location.span.start;
        let declarator = syntax
            .syntax()
            .descendants()
            .filter_map(ast::Declarator::cast)
            .find(|declarator| {
                declarator
                    .name()
                    .is_some_and(|name| name.span().start == at)
            })
            .at(span)?;
        let name = declarator.name().at(span)?;
        let decl = declarator
            .syntax()
            .parent()
            .and_then(ast::LocalDecl::cast)
            .at(span)?;
        let Some(ast::Initializer::Body(nested_body)) = declarator.initializer() else {
            return Err(Internal::Unlowered).at(span);
        };
        let nested = !declares(decl.syntax(), &Qualifier::Static);
        let written = Written {
            ty: decl.ty(),
            wrap: false,
            nested,
        };
        let place = self.program.get_def(self.index, local).at(span)?;
        let word =
            self.register_nested(written, &declarator, &name, (local, place), &nested_body)?;
        if !nested {
            let ofs = self.program.def(place).at(span)?.ofs;
            self.program.set_words(ofs, &[word]).at(span)?;
            return Ok(None);
        }
        // `tmp = QCC_MakeIntConst(f - functions)`, stored as any local's initialiser.
        let constant = self
            .program
            .immediate(&Literal::Float(f32::from_bits(word)))
            .at(span)?;
        Ok(Some(constant))
    }

    /// Makes what `effect` makes without a statement reading it (`TypedBody::effects`): a
    /// local declared, after which the locals end, a definition, immediate or array function,
    /// or a temporary a dropped statement took.
    fn effect(
        &mut self,
        effect: Effect,
        index: u32,
        made: (&TypedBody, &HashMap<u32, u32>),
        span: Span,
    ) -> Lowered<()> {
        let name = match effect {
            Effect::Make(name) => name,
            Effect::Temp(words) => {
                self.program.allocate(words).at(span)?;
                return Ok(());
            }
            // A block whose statements were dropped is still taken (QC-21.5).
            Effect::Block(words) => {
                self.program.allocate(words).at(span)?;
                return Ok(());
            }
            Effect::ForBegin
            | Effect::ForEnd
            | Effect::Constant(_)
            | Effect::Autocvar(_)
            | Effect::Nested(_) => {
                return Ok(());
            }
        };
        self.resolve(name, index, made, span)?;
        if let Name::Def(id, _) = name
            && self.index.def(id).is_some_and(|d| d.owner.is_some())
        {
            self.locals_end = self.program.next_global().at(span)?;
        }
        Ok(())
    }

    /// FTEF-32.13: the constant local `local` of the body `syntax` holds its aggregate
    /// initialiser's words at compile time, as a global does.
    fn constant_local(
        &mut self,
        syntax: &ast::FunctionBody,
        local: Name,
        span: Span,
    ) -> Lowered<()> {
        let Name::Def(id, _) = local else {
            return Err(Internal::Unlowered).at(span);
        };
        let at = self.index.def(id).at(span)?.location.span.start;
        let aggregate = syntax
            .syntax()
            .descendants()
            .filter_map(ast::Declarator::cast)
            .find(|declarator| {
                declarator
                    .name()
                    .is_some_and(|name| name.span().start == at)
            })
            .and_then(|declarator| match declarator.initializer() {
                Some(ast::Initializer::Aggregate(aggregate)) => Some(aggregate),
                _ => None,
            })
            .at(span)?;
        let place = self.program.get_def(self.index, id).at(span)?;
        let def = self.program.def(place).at(span)?;
        let (ty, count) = (def.ty.clone(), def.count);
        let words = self.aggregate_words(&ty, count, &aggregate)?;
        self.program.initialise_words(place, &words).at(span)?;
        Ok(())
    }

    /// FTEF-32.12: the `autocvar_NAME` global `cvar` that an `autocvar(NAME, default)` call of the
    /// body `syntax` reads holds the call's default when nothing initialised it before.
    fn autocvar_default(
        &mut self,
        syntax: &ast::FunctionBody,
        cvar: Name,
        span: Span,
    ) -> Lowered<()> {
        let Name::Def(id, _) = cvar else {
            return Err(Internal::Unlowered).at(span);
        };
        let place = self.program.get_def(self.index, id).at(span)?;
        let def = self.program.def(place).at(span)?;
        if def.initialized {
            return Ok(());
        }
        let ty = def.ty.clone();
        let name = def.name.strip_prefix("autocvar_").at(span)?.to_owned();
        let default = syntax
            .syntax()
            .descendants()
            .filter_map(ast::CallExpr::cast)
            .filter(|call| {
                matches!(call.callee(), Some(ast::Expr::Name(callee))
                    if callee.name().is_some_and(|t| t.text_lossless() == "autocvar"))
            })
            .filter_map(|call| call.args().map(|args| args.args().collect::<Vec<_>>()))
            .find(|args| {
                matches!(args.first(), Some(ast::Expr::Name(first))
                    if first.name().is_some_and(|t| t.text_lossless() == name))
            })
            .and_then(|args| args.get(1).cloned())
            .at(span)?;
        let words = self.value_words(&ty, &default)?;
        self.program.initialise_words(place, &words).at(span)?;
        Ok(())
    }

    /// Whether `body` passes the parameter defaults this compiler keeps (FTE-18.7), which under
    /// autoprototyping the semantic model does not know (FTE-18.8): each it passes is kept and
    /// read with no statement of its own, and no call stops before a parameter whose default is
    /// kept.
    fn defaults_kept(&self, body: &TypedBody) -> bool {
        let generated = body.generated();
        generated.iter().enumerate().all(|(at, statement)| {
            let passed = [statement.a, statement.b, statement.c]
                .into_iter()
                .flatten()
                .all(|name| match name {
                    Name::Default(slot) => self.default_of(generated, at, slot).is_some(),
                    _ => true,
                });
            let stops = match (call_count(statement.op), statement.a) {
                (Some(count), Some(Name::Def(callee, _))) => self
                    .defaults
                    .get(&callee)
                    .and_then(|defaults| defaults.get(count))
                    .is_some_and(Option::is_some),
                _ => false,
            };
            passed && !stops
        })
    }

    /// The default value of parameter `slot` of the function the first `CALL` from the
    /// statement at `at` calls, with the file it is written in, when this compiler keeps one
    /// that is a literal or a name, its casts taken off: a cast within its basic type makes no
    /// statement (FTEF-30.6), and the default is stored by its words.
    fn default_of(
        &self,
        generated: &[BodyStatement],
        at: usize,
        slot: u8,
    ) -> Option<(FileId, ast::Expr)> {
        let call = generated
            .get(at..)?
            .iter()
            .find(|s| call_count(s.op).is_some())?;
        let Some(Name::Def(callee, _)) = call.a else {
            return None;
        };
        let (file, mut default) = self
            .defaults
            .get(&callee)?
            .get(usize::from(slot))?
            .clone()?;
        while let ast::Expr::Cast(cast) = &default {
            default = cast.operand()?;
        }
        matches!(default, ast::Expr::Literal(_) | ast::Expr::Name(_)).then_some((file, default))
    }

    /// Whether the function being compiled returns a value, which a bare `return` of
    /// [`Name::RetLocal`] needs.
    fn returns_value(&self) -> bool {
        self.body_return
            .as_ref()
            .is_some_and(|ty| ty.etype() != qc_types::Etype::Void)
    }

    /// The operand `name` is in the statement at `index`: a global, a slot the allocation
    /// pass gave a value, or a jump's offset.
    fn resolve(
        &mut self,
        name: Name,
        index: u32,
        (body, slots): (&TypedBody, &HashMap<u32, u32>),
        span: Span,
    ) -> Lowered<i32> {
        let ofs = match name {
            Name::Def(id, word) => {
                let place = self.program.get_def(self.index, id).at(span)?;
                let def = self.program.def(place).at(span)?;
                // FTE-13.6: a word past an array's elements is not lowered.
                if let Some(count) = def.count {
                    let words = count
                        .checked_mul(type_words(self.index, &def.ty))
                        .at(span)?;
                    if word >= words {
                        return Err(Internal::Unlowered).at(span);
                    }
                }
                def.ofs.checked_add(word).at(span)?
            }
            Name::Immediate(immediate) => {
                let place = self.immediate_place(immediate, span)?;
                self.program.def(place).at(span)?.ofs
            }
            Name::ImmediateWord(immediate, word) => {
                let place = self.immediate_place(immediate, span)?;
                self.program
                    .def(place)
                    .at(span)?
                    .ofs
                    .checked_add(word)
                    .at(span)?
            }
            Name::String(string) => {
                let bytes = body.string(string).at(span)?.to_vec();
                let place = self.program.immediate(&Literal::String(bytes)).at(span)?;
                self.program.def(place).at(span)?.ofs
            }
            Name::Made(made) => *slots.get(&made).at(span)?,
            Name::MadeWord(made, word) => slots.get(&made).at(span)?.checked_add(word).at(span)?,
            Name::Return => OFS_RETURN,
            Name::ReturnWord(word) => OFS_RETURN.checked_add(word).at(span)?,
            Name::Parameter(slot) => self.parameter_slot(usize::from(slot), span)?,
            Name::VaList(word) => {
                let list = self.va_list.at(span)?;
                self.program
                    .def(list)
                    .at(span)?
                    .ofs
                    .checked_add(word)
                    .at(span)?
            }
            Name::RetLocal => self.bare_return(span)?,
            Name::Prior => {
                let prior = self.prior.at(span)?;
                self.program.def(prior).at(span)?.ofs
            }
            Name::ArrayGet(id) | Name::ArraySet(id) => {
                let place = self.array_accessor(id, matches!(name, Name::ArraySet(_)), span)?;
                self.program.def(place).at(span)?.ofs
            }
            Name::Jump(target) => {
                let target = i64::from(target);
                let offset = target.checked_sub(i64::from(index)).at(span)?;
                return i32::try_from(offset)
                    .map_err(|_| Internal::TooLarge)
                    .at(span);
            }
            // FTEF-18.2: the nested function's number, its immediate made as it is stored.
            Name::Nested(local) => {
                let syntax = self.writing.clone().at(span)?;
                let place = self.nested_local(&syntax, local, span)?.at(span)?;
                self.program.def(place).at(span)?.ofs
            }
            Name::Default(slot) => {
                let at = usize::try_from(index)
                    .map_err(|_| Internal::TooLarge)
                    .at(span)?;
                let (file, default) = self.default_of(body.generated(), at, slot).at(span)?;
                let here = std::mem::replace(&mut self.file, file);
                let value = self.expr(&default);
                self.file = here;
                let value = value?;
                self.operand_of(&value)
            }
        };
        operand(ofs).at(span)
    }
}

/// Whether the writer can write `body`: the checker followed every statement it generates,
/// each statement names every operand its opcode uses, each value read is one a statement
/// makes, and a bare `return` of a function that
/// `returns_value` names its `ret*` local.
fn writable(body: &TypedBody, returns_value: bool) -> bool {
    if !body.followed() {
        return false;
    }
    let generated = body.generated();
    let makes = |made: u32| {
        usize::try_from(made)
            .ok()
            .and_then(|at| generated.get(at))
            .and_then(|statement| statement.made_words(made))
            .is_some()
            || body
                .effects()
                .iter()
                .any(|&(at, effect)| at == made && matches!(effect, Effect::Block(_)))
    };
    generated.iter().all(|statement| {
        let named = [statement.a, statement.b, statement.c];
        let used = statement
            .uses()
            .iter()
            .zip(named)
            .all(|(&uses, name)| !uses || name.is_some());
        let reads = named.into_iter().flatten().all(|name| match name {
            Name::Made(made) | Name::MadeWord(made, _) => makes(made),
            _ => true,
        });
        used && reads && !bare_return(statement, returns_value)
    })
}

/// The arguments a call the semantic model generates passes, `CALL0` to `CALL8`; `None` for
/// any other opcode.
fn call_count(op: qc_preproc::Op) -> Option<usize> {
    use qc_preproc::Op;
    Some(match op {
        Op::Call0 => 0,
        Op::Call1 => 1,
        Op::Call2 => 2,
        Op::Call3 => 3,
        Op::Call4 => 4,
        Op::Call5 => 5,
        Op::Call6 => 6,
        Op::Call7 => 7,
        Op::Call8 => 8,
        _ => return None,
    })
}

/// Whether `op` is a call, `CALL0` to `CALL8`.
fn is_call(op: Opcode) -> bool {
    matches!(
        op,
        Opcode::Call0
            | Opcode::Call1
            | Opcode::Call2
            | Opcode::Call3
            | Opcode::Call4
            | Opcode::Call5
            | Opcode::Call6
            | Opcode::Call7
            | Opcode::Call8
    )
}

/// Whether `statement` is a `RETURN` of nothing in a function that `returns_value`.
fn bare_return(statement: &BodyStatement, returns_value: bool) -> bool {
    statement.op == qc_preproc::Op::Return && statement.a.is_none() && returns_value
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::indexing_slicing,
        reason = "test code: a bad index is a failed test"
    )]

    use qc_progs::opcode::Opcode;

    use crate::tests::{body, fte_progs, global};

    // spec QC-21.5: a body written from the semantic model gives each value a statement makes a
    // slot of its own in the order the statements make them, and a store folds into the last
    #[test]
    fn a_written_body_takes_each_temporary_in_turn() {
        let p = fte_progs("float a, b, c;\nvoid() t = { a = b * c + b * c; };\n");
        let t = body(&p, "t");
        let (a, b, c) = (global(&p, "a"), global(&p, "b"), global(&p, "c"));
        assert_eq!(t[0].op, u32::from(Opcode::MulF.number()));
        assert_eq!((t[0].a, t[0].b), (b, c));
        assert_eq!(t[1].c, t[0].c + 1, "the second product takes the next slot");
        assert_eq!(t[2].op, u32::from(Opcode::AddF.number()));
        assert_eq!((t[2].a, t[2].b, t[2].c), (t[0].c, t[1].c, a));
    }
}
