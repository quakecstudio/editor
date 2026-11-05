// SPDX-License-Identifier: GPL-3.0-or-later

//! The writer of a function's code from its checked body (`TypedBody::generated`), the
//! intermediate layer's whole body (`docs/design/back-end.md`): each local declared before the
//! statement the body declares it at, each value a statement makes given a slot of its own by
//! the allocation pass, in the order the statements make them (QC-21.5), and each name
//! resolved to its global. A body the semantic model did not follow in full, or that names
//! something the writer cannot resolve, is an internal error: there is no other way to lower
//! it.

use qc_diag::Span;
use qc_progs::opcode::Opcode;
use qc_sema::ir::{BodyStatement, Effect, Name};
use qc_sema::lower::{Qualifier, declares};
use qc_sema::{DefId, FileId, TypedBody};
use qc_syntax::ast::{self, AstNode as _};

use std::collections::HashMap;
use std::rc::Rc;

use super::allocate::{CreationOrder, Overlapping, Slots};
use super::{At, Compiler, Lowered, Written, literal_of};
use crate::Internal;
use crate::back_end::Allocation;
use crate::program::{Literal, OFS_RETURN, operand, type_words};

impl Compiler<'_> {
    /// Writes `body`'s statements as the code of the function being compiled, `named` the span
    /// of its name, the origin of the statements before its first source statement.
    ///
    /// [`Internal::Unfollowed`] for a body [`writable`] refuses, and
    /// [`Internal::DefaultUnknown`] for one whose calls pass a default the semantic model did
    /// not know of.
    pub(super) fn write_body(
        &mut self,
        body: &TypedBody,
        syntax: &ast::FunctionBody,
        named: Span,
    ) -> Lowered<()> {
        // A bare `return` names the `ret*` local under FTEQCC (FTE-18.10); id's `qcc` writes
        // `RETURN` of nothing whatever the function returns (QC-13.8).
        let ret_local = self.returns_value() && self.dialect.has_fteqcc_extensions();
        if !writable(body, ret_local) {
            return Err(Internal::Unfollowed).at(named);
        }
        if !self.defaults_kept(body) {
            return Err(Internal::DefaultUnknown).at(named);
        }
        let syntax = Rc::new(BodySyntax::of(syntax));
        self.writing = Some(Rc::clone(&syntax));
        let written = self.write_statements(body, &syntax, named);
        self.writing = None;
        written
    }

    /// The statements of `body`, whose syntax is `syntax`, written in turn with what the body
    /// makes between them.
    fn write_statements(
        &mut self,
        body: &TypedBody,
        syntax: &BodySyntax,
        named: Span,
    ) -> Lowered<()> {
        let generated = body.generated();
        // QC-21.5: the allocation pass the profile chooses (ADR-0020).
        let mut slots = match self.back_end.allocation {
            Allocation::CreationOrder => Slots::CreationOrder(CreationOrder::default()),
            Allocation::Overlapping => {
                Slots::Overlapping(Overlapping::new(generated, body.effects()))
            }
        };
        // The global the first parameter slot was last stored from, which a precache records.
        let mut first_argument = None;
        let mut effects = body.effects().iter().peekable();
        // How many array functions there were as each `for` loop open here began.
        let mut loops = Vec::new();
        for (index, statement) in (0_u32..).zip(generated) {
            while let Some(&&(at, effect)) = effects.peek()
                && at <= index
            {
                self.effect(
                    effect,
                    index,
                    (body, &mut slots, &mut loops),
                    (syntax, named),
                )?;
                effects.next();
            }
            let at = usize::try_from(index)
                .map_err(|_| Internal::TooLarge)
                .at(named)?;
            let origin = body.origin(at).unwrap_or(named.start);
            self.program.origin = Some((self.file, origin));
            slots
                .statement(&mut self.program, statement, index)
                .at(named)?;
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
            self.effect(effect, end, (body, &mut slots, &mut loops), (syntax, named))?;
        }
        Ok(())
    }

    /// The global holding `expr` of file `file` with no statement: a literal's immediate, made now
    /// if there is none (QC-20), or a name's definition, as a parameter's default and an
    /// initialiser's constant read them.
    pub(super) fn constant_place(
        &mut self,
        file: FileId,
        expr: &ast::Expr,
        span: Span,
    ) -> Lowered<usize> {
        match expr {
            ast::Expr::Literal(_) => {
                let literal = literal_of(expr, self.dialect).at(span)?;
                self.program.immediate(&literal).at(span)
            }
            ast::Expr::Name(name) => {
                let token = name.name().at(span)?;
                let id = self.names.def(file, token.span(), None).at(span)?;
                self.program.get_def(self.index, id).at(span)
            }
            _ => Err(Internal::Unlowered).at(span),
        }
    }

    /// FTEF-18.2: the nested function the local `local` of the body `syntax` is given, numbered
    /// and kept to compile after this one: the immediate of its number when the local stores
    /// it each time the function runs, else a `static` local's value, set now.
    fn nested_local(
        &mut self,
        syntax: &BodySyntax,
        local: DefId,
        span: Span,
    ) -> Lowered<Option<usize>> {
        let at = self.index.def(local).at(span)?.location.span.start;
        let declarator = syntax.declarators.get(&at).cloned().at(span)?;
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

    /// Makes what `effect`, before the statement at `index` of `body`, makes without a statement
    /// reading it (`TypedBody::effects`): a local declared, after which the locals end, a
    /// definition, immediate or array function, a temporary a dropped statement took, a block
    /// taken before its words are written, a `for` loop's bounds, or a local's or a cvar's value
    /// set from the body `syntax`.
    fn effect(
        &mut self,
        effect: Effect,
        index: u32,
        (body, slots, loops): (&TypedBody, &mut Slots, &mut Vec<usize>),
        (syntax, span): (&BodySyntax, Span),
    ) -> Lowered<()> {
        let name = match effect {
            Effect::Make(name) => name,
            Effect::Temp(words) => return slots.dropped(&mut self.program, words).at(span),
            // A block taken before its words are written, the value of this statement; one
            // whose statements were dropped is still taken (QC-21.5).
            Effect::Block(words) => return slots.block(&mut self.program, index, words).at(span),
            Effect::ForBegin => {
                loops.push(self.array_functions.len());
                return Ok(());
            }
            // FTEF-19.78: the local arrays' functions the loop made are forgotten.
            Effect::ForEnd => {
                let mark = loops.pop().unwrap_or(self.array_functions.len());
                self.forget_local_array_functions(mark);
                return Ok(());
            }
            Effect::Constant(local) => return self.constant_local(syntax, local, span),
            Effect::Autocvar(cvar) => return self.autocvar_default(syntax, cvar, span),
            Effect::Nested(Name::Def(local, _)) => {
                return self.nested_local(syntax, local, span).map(|_| ());
            }
            Effect::Nested(_) => return Ok(()),
        };
        self.resolve(name, index, (body, slots), span)?;
        if let Name::Def(id, _) = name
            && self.index.def(id).is_some_and(|d| d.owner.is_some())
        {
            self.locals_end = self.program.next_global().at(span)?;
        }
        Ok(())
    }

    /// FTEF-32.13, QC-11.10: the local `local` of the body `syntax` holds its initialiser's
    /// words at compile time, as a global does: a constant's aggregate, or under id's `qcc` any
    /// local's literal.
    fn constant_local(&mut self, syntax: &BodySyntax, local: Name, span: Span) -> Lowered<()> {
        let Name::Def(id, _) = local else {
            return Err(Internal::Unlowered).at(span);
        };
        let at = self.index.def(id).at(span)?.location.span.start;
        let initializer = syntax
            .declarators
            .get(&at)
            .and_then(ast::Declarator::initializer)
            .at(span)?;
        let place = self.program.get_def(self.index, id).at(span)?;
        let aggregate = match initializer {
            ast::Initializer::Aggregate(aggregate) => aggregate,
            ast::Initializer::Value(expr) => {
                let literal = literal_of(&expr, self.dialect).at(span)?;
                self.program.initialise(place, &literal).at(span)?;
                return Ok(());
            }
            ast::Initializer::Body(_) | ast::Initializer::Builtin(_) => {
                return Err(Internal::Unlowered).at(span);
            }
        };
        let def = self.program.def(place).at(span)?;
        let (ty, count) = (def.ty.clone(), def.count);
        let words = self.aggregate_words(&ty, count, &aggregate)?;
        self.program.initialise_words(place, &words).at(span)?;
        Ok(())
    }

    /// FTEF-32.12: the `autocvar_NAME` global `cvar` that an `autocvar(NAME, default)` call of the
    /// body `syntax` reads holds the call's default when nothing initialised it before. Which of
    /// the body's calls of `NAME` gives it does not matter: a call whose default differs from
    /// the value the cvar already has is an error (QCD-4.69).
    fn autocvar_default(&mut self, syntax: &BodySyntax, cvar: Name, span: Span) -> Lowered<()> {
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
        let default = syntax.autocvars.get(&name).cloned().at(span)?;
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
        (body, slots): (&TypedBody, &Slots),
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
            Name::Made(made) => slots.slot(made).at(span)?,
            Name::MadeWord(made, word) => slots.slot(made).at(span)?.checked_add(word).at(span)?,
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
                let place = self.constant_place(file, &default, span)?;
                self.program.def(place).at(span)?.ofs
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

/// What the writer looks up in a body's syntax, read in one pass over it: each declarator by
/// where its name starts, and the default of the first `autocvar(NAME, default)` call of each
/// `NAME`.
pub(super) struct BodySyntax {
    declarators: HashMap<usize, ast::Declarator>,
    autocvars: HashMap<String, ast::Expr>,
}

impl BodySyntax {
    /// The declarators and `autocvar` defaults of `body`, nested functions' included.
    fn of(body: &ast::FunctionBody) -> Self {
        let mut declarators = HashMap::new();
        let mut autocvars = HashMap::new();
        for node in body.syntax().descendants() {
            if let Some(declarator) = ast::Declarator::cast(node.clone()) {
                if let Some(name) = declarator.name() {
                    declarators.entry(name.span().start).or_insert(declarator);
                }
                continue;
            }
            let Some(call) = ast::CallExpr::cast(node) else {
                continue;
            };
            let is_autocvar = matches!(call.callee(), Some(ast::Expr::Name(callee))
                if callee.name().is_some_and(|t| t.text_lossless() == "autocvar"));
            let args: Vec<ast::Expr> = call.args().map(|a| a.args().collect()).unwrap_or_default();
            if let (true, Some(ast::Expr::Name(first)), Some(default)) =
                (is_autocvar, args.first(), args.get(1))
                && let Some(token) = first.name()
            {
                autocvars
                    .entry(token.text_lossless())
                    .or_insert_with(|| default.clone());
            }
        }
        Self {
            declarators,
            autocvars,
        }
    }
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
