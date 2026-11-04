// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseStatement, QCC_PR_ParseStatement_For, QCC_PR_FinaliseFunction

//! Statements as FTEQCC checks them (FTEF-34): what each returns, the test of each condition,
//! the cases of a `switch`, `thinktime`, and the layout of the jumps they generate, from which the
//! finished function is checked for a missing return and for code no path reaches.

use qc_diag::{Code, Span};
use qc_preproc::{FteFlag, Op, TargetFormat};
use qc_syntax::ast::{self, AstNode as _, Negation};

use crate::check::constant::Constant;
use crate::check::etype::Etype;
use crate::check::expr::{Checked, Checker, Fatal};
use crate::check::flow::{Finding, Mark, Step};
use crate::check::member;
use crate::check::reference::eval_int;
use crate::check::truth::{Jump, Test, is_truth};
use crate::check::{Constness, Reference, Value};
use crate::ir;
use crate::ty::{SimpleType, Type};

/// The words of a vector, `type_vector->size`.
const VECTOR_WORDS: u32 = 3;

/// A `case` of a `switch`: its value, the end of its range, where it is and the statement it
/// jumps to (`pr_casesref`, `pr_casesref2`, `pr_cases`).
pub(super) struct Case {
    value: Option<Value>,
    range_end: Option<Value>,
    span: Span,
    at: Mark,
}

impl Checker<'_> {
    /// Checks `stmt`; a fatal error in one of its expressions ends its checks (FTEF-23.6).
    /// A jump of `step` to `target` at `line` in the layout, and its opcode `op` among the
    /// statements the body generates.
    fn jump(&mut self, step: Step, op: Op, target: Option<Mark>, line: u32) -> usize {
        let made = self.goto_made(op);
        if matches!(op, Op::IfI | Op::IfnotI)
            && let Some(statement) = self.generated.last_mut()
        {
            statement.a = self.branch_condition.take();
        }
        self.layout.push(step, target, line, Some(made))
    }

    /// The index among the generated statements of the next one, which a jump to the next
    /// statement goes to.
    pub(super) fn next_made(&self) -> u32 {
        u32::try_from(self.generated.len()).unwrap_or(u32::MAX)
    }

    /// FTE-18.6: `STORE_V` of each of the 24 parameter slots after the named ones to the
    /// element of the function's `__va_list`.
    pub(super) fn variadic_prologue(&mut self) {
        const VA_COUNT: u8 = 24;
        let named = u8::try_from(self.function.params).unwrap_or(u8::MAX);
        for i in 0..VA_COUNT {
            self.generated.push(ir::BodyStatement {
                a: named.checked_add(i).map(ir::Name::Parameter),
                b: Some(ir::Name::VaList(u32::from(i).saturating_mul(3))),
                ..ir::BodyStatement::bare(Op::StoreV)
            });
        }
    }

    /// The point of the next statement, in the layout and among the generated statements.
    fn here(&self) -> Mark {
        self.layout.mark(self.next_made())
    }

    /// Generates the jump `op`, its operands named once the body is read; returns its index.
    fn goto_made(&mut self, op: Op) -> u32 {
        let made = self.next_made();
        self.generated.push(ir::BodyStatement::bare(op));
        self.last_temp = None;
        made
    }

    /// What the generated statements from `from` on make: each one's operands and the temporary
    /// it took, in order.
    pub(super) fn dropped_effects(&self, from: usize) -> Vec<ir::Effect> {
        let mut effects = Vec::new();
        let statements = self.generated.get(from..).unwrap_or_default();
        for (index, statement) in (u32::try_from(from).unwrap_or(u32::MAX)..).zip(statements) {
            let make =
                |name: Option<ir::Name>| name.filter(ir::Name::is_made_once).map(ir::Effect::Make);
            effects.extend(make(statement.a));
            effects.extend(make(statement.b));
            effects.extend(statement.temp_words(index).map(ir::Effect::Temp));
            effects.extend(make(statement.c));
        }
        effects
    }

    /// `QCC_UngenerateStatements` back to `mark`: drops what the layout and the body generated
    /// after it.
    fn truncate(&mut self, mark: Mark) {
        self.layout.truncate(mark.at());
        // FTE-3.6: a `case` label the dropped statements recorded is forgotten
        // (`if (pr_cases[i] >= newstatementcount)`).
        for cases in &mut self.switches {
            cases.retain(|case| case.at.at() < mark.at());
        }
        let from = usize::try_from(mark.made()).unwrap_or(usize::MAX);
        // What the dropped statements named is still made, and the temporaries they took still
        // taken, in their order.
        let dropped = self.dropped_effects(from);
        self.generated.truncate(from);
        let kept = self.next_made();
        self.effects
            .extend(dropped.into_iter().map(|effect| (kept, effect)));
        self.live_return = None;
        self.copies.retain(|&(call, _)| call < kept);
        // What follows is still recorded where the last statement read started.
        if let Some(&(_, origin)) = self.origins.last() {
            self.origins.retain(|&(at, _)| at < kept);
            self.origins.push((kept, origin));
        }
    }

    /// Names the target of each jump of the body whose target is known, and its condition
    /// once [`Checker::branch`] named it: `b` of `IF`, `IFNOT` and `CASE`, `a` of `GOTO`.
    pub(super) fn name_jumps(&mut self) {
        for (made, target) in self.layout.jumps() {
            let Some(statement) = usize::try_from(made)
                .ok()
                .and_then(|at| self.generated.get_mut(at))
            else {
                continue;
            };
            let target = Some(ir::Name::Jump(target));
            match statement.op {
                Op::Goto => statement.a = target,
                Op::IfI | Op::IfnotI => statement.b = target,
                _ => {}
            }
        }
    }

    /// [`Checker::jump`] of the conditional jump the last [`Checker::branch`] chose.
    fn branch_jump(&mut self, target: Option<Mark>, line: u32) -> usize {
        let op = self.branch_op.take().unwrap_or(Op::IfnotI);
        self.jump(Step::Branch, op, target, line)
    }

    pub(super) fn statement(&mut self, stmt: &ast::Stmt) {
        // The statements it generates are recorded at its start.
        let made = self.next_made();
        self.origins.push((made, stmt.syntax().span().start));
        // A value the statement before left is discarded with it (FTE-18.13).
        self.live_return = None;
        let ended = self.statement_parts(stmt);
        // So is a value the statement itself leaves, which a test after it does not copy.
        self.live_return = None;
        self.live_call = None;
        self.env.temps().release_all();
        // FTEF-24.10: a fatal error before the token ends the statement first.
        if let Some((message, span)) = self.pending.take()
            && ended.is_ok()
        {
            self.env.report(Code::E0204, span, message);
        }
    }

    fn statement_of(&mut self, stmt: Option<ast::Stmt>) {
        if let Some(stmt) = stmt {
            self.statement(&stmt);
        }
    }

    fn statement_parts(&mut self, stmt: &ast::Stmt) -> Result<(), Fatal> {
        let line = self.env.line(stmt.syntax().span());
        self.line_starts.entry(line).or_insert(stmt.syntax().span());
        match stmt {
            ast::Stmt::Block(block) => self.block(block),
            ast::Stmt::Case(label) => {
                // A `case` no switch holds is "illegal cases", the lowering's (FTEF-21.19).
                if let Some(mut cases) = self.switches.pop() {
                    let ended = self.case(label, &mut cases);
                    self.switches.push(cases);
                    ended?;
                }
            }
            ast::Stmt::Expr(s) => {
                let expr = s.expr();
                if expr.as_ref().is_some_and(generates) {
                    self.layout.push(Step::Other, None, line, None);
                }
                // `QCC_PR_DiscardExpression`: the value is discarded, never read.
                self.expr_of(expr)?;
            }
            ast::Stmt::Return(s) => self.return_stmt(s, line)?,
            ast::Stmt::If(s) => self.if_stmt(s, line)?,
            ast::Stmt::While(s) => self.while_stmt(s, line)?,
            ast::Stmt::Do(s) => self.do_stmt(s, line)?,
            ast::Stmt::Loop(s) => {
                let (breaks, continues) = self.layout.marks();
                let top = self.here();
                self.statement_of(s.body());
                self.jump(Step::Goto, Op::Goto, Some(top), line);
                let end = self.here();
                self.layout.patch_breaks(breaks, end);
                self.layout.patch_continues(continues, top);
            }
            ast::Stmt::For(s) => self.for_stmt(s, line)?,
            ast::Stmt::Switch(s) => self.switch(s, line)?,
            ast::Stmt::Local(local) => {
                let initialised = local.declarators().any(|d| d.initializer().is_some());
                if initialised {
                    self.layout.push(Step::Other, None, line, None);
                }
                self.local(local)?;
                for inner in inner_functions(local.syntax()) {
                    self.inner_function(&inner)?;
                }
            }
            ast::Stmt::Break(_) => {
                let made = self.goto_made(Op::Goto);
                self.layout.break_(line, made);
            }
            ast::Stmt::Continue(_) => {
                let made = self.goto_made(Op::Goto);
                self.layout.continue_(line, made);
            }
            ast::Stmt::Label(s) => {
                // Labels and `goto`s are not followed statement by statement.
                self.followed = false;
                if let Some(name) = s.name() {
                    let made = self.next_made();
                    self.layout.label(name.text_lossless(), made);
                }
            }
            ast::Stmt::Goto(s) => {
                self.followed = false;
                let name = s.label().map(|t| t.text_lossless()).unwrap_or_default();
                let made = self.goto_made(Op::Goto);
                self.layout.goto(name, line, made);
            }
            ast::Stmt::Thinktime(s) => self.thinktime(s, line)?,
            ast::Stmt::Exit(_) => {
                self.jump(Step::Done, Op::Done, None, line);
            }
            ast::Stmt::State(_) | ast::Stmt::Asm(_) => {
                self.layout.push(Step::Other, None, line, None);
            }
            ast::Stmt::Empty(_) | ast::Stmt::Using(_) | ast::Stmt::Extern(_) => {}
        }
        Ok(())
    }

    /// FTEF-34.1: `return;`, `return e;` and `return = e;`.
    fn return_stmt(&mut self, s: &ast::ReturnStmt, line: u32) -> Result<(), Fatal> {
        let span = s.syntax().span();
        let ret = self.function.ret.clone();
        let void = self.etype_of(&ret) == Some(Etype::Void);
        let name = self.function.name.clone();
        let ret_name = self.own_name(&ret);
        // A type larger than a vector is returned through a pointer, which `return;` and
        // `return =` do not handle.
        let large = self.returns_large();
        if s.sets() {
            if large {
                let message = format!("'{name}' not supported when returning {ret_name}");
                return self.fatal(Code::E0341, span, message).map(drop);
            }
            self.returned = true;
            self.layout.push(Step::Other, None, line, None);
            self.value(s.value())?;
            return Ok(());
        }
        let Some(expr) = s.value() else {
            if !void && ret != Type::Unknown {
                let message = format!("'{name}' returned nothing, expected {ret_name}");
                if large {
                    return self.fatal(Code::E0341, span, message).map(drop);
                }
                if !self.function.accumulate && !self.returned {
                    self.env.report(Code::W0256, span, message);
                }
                self.returned = true;
            }
            self.jump(Step::Return, Op::Return, None, line);
            // FTE-18.10: in a function that returns a value it returns the `ret*` local.
            if !void && ret != Type::Unknown {
                self.name_last(Op::Return, Some(ir::Name::RetLocal), None);
                self.ret_local = true;
            }
            return Ok(());
        };
        let before = self.emitted;
        let value = self.value(Some(expr.clone()))?;
        self.generated(&expr, &value, line, before);
        // The value is made the return type first (`QCC_EvaluateCast`), then returned.
        let returned = self.returned_as(value, &ret, (void, &name, &ret_name), &expr)?;
        self.jump(Step::Return, Op::Return, None, line);
        self.name_last(Op::Return, self.name_of(&returned), None);
        Ok(())
    }

    /// FTEF-34.1: `value` returned from a function returning `ret`: as it is when the types
    /// agree, it is a null or either is unknown; from a `void` function with the warning that it
    /// returned a value; else cast to `ret`.
    fn returned_as(
        &mut self,
        value: Value,
        ret: &Type,
        (void, name, ret_name): (bool, &str, &str),
        expr: &ast::Expr,
    ) -> Checked {
        if value.ty == Type::Unknown || *ret == Type::Unknown || value.null() {
            return Ok(value);
        }
        if self.etype_of(&value.ty) == self.etype_of(ret) {
            return Ok(value);
        }
        let value_span = expr.syntax().span();
        if void {
            let message = format!(
                "'{name}' returned {}, expected {ret_name}",
                self.own_name(&value.ty)
            );
            self.env.report(Code::W0257, value_span, message);
            return Ok(value);
        }
        self.cast_to(&value, ret, value_span)
    }

    /// A condition of `if`, `while`, `do` or `switch`: an assignment at its top, outside
    /// parentheses, is "suggest parenthesis" (FTEF-29.6).
    fn condition(&mut self, expr: Option<ast::Expr>) -> Result<Value, Fatal> {
        self.in_condition = true;
        let before = self.emitted;
        let checked = self.value(expr.clone());
        self.in_condition = false;
        let value = checked?;
        if let Some(expr) = &expr {
            let line = self.env.line(expr.syntax().span());
            self.generated(expr, &value, line, before);
        }
        Ok(value)
    }

    /// The statements an expression generates, if it generates any: one stands for them all. A
    /// cast of a name or a literal generates only the conversions it emitted since `before`, none
    /// for a cast to the type it has.
    fn generated(&mut self, expr: &ast::Expr, value: &Value, line: u32, before: usize) {
        let mut plain = expr.clone();
        while let ast::Expr::Paren(paren) = &plain {
            let Some(inner) = paren.expr() else {
                break;
            };
            plain = inner;
        }
        let converts_only = matches!(
            &plain,
            ast::Expr::Cast(cast) if cast.operand().is_some_and(|operand| !generates(&operand))
        );
        if converts_only && self.emitted == before {
            return;
        }
        if generates(expr) && value.known().is_none() {
            self.layout.push(Step::Other, None, line, None);
        }
    }

    /// The constant value of a condition, `QCC_SRef_EvalConst`, read as zero or not: a float as
    /// `_float`, anything else as `_int`.
    fn constant_zero(&mut self, value: &Value) -> Option<bool> {
        let constant = value.known()?;
        // A string constant's test is not followed statement by statement.
        if self.etype_of(&value.ty) == Some(Etype::String) {
            self.followed = false;
        }
        Some(if self.etype_of(&value.ty) == Some(Etype::Float) {
            constant.f() == 0.0
        } else {
            constant.i() == 0
        })
    }

    /// FTEF-34.2: `if (e)`, `if not (e)` and `if !(e)`, whose constant condition generates no
    /// test and drops the branch never taken.
    fn if_stmt(&mut self, s: &ast::IfStmt, line: u32) -> Result<(), Fatal> {
        let span = s.syntax().span();
        let negation = s.negation();
        let hexen2 = matches!(
            self.env.target_at(span.start).format(),
            TargetFormat::Hexen2 | TargetFormat::Uhexen2 | TargetFormat::Fteh2
        );
        match negation {
            Some(Negation::Bang) => {
                let message = "if !() is specific to fteqcc".to_owned();
                self.env.report(Code::W0261, span, message);
            }
            Some(Negation::Not) if !hexen2 => {
                let message = "if not() is specific to fteqcc or hexen2".to_owned();
                self.env.report(Code::W0261, span, message);
            }
            Some(Negation::Not) | None => {}
        }
        let mut negate = negation.is_some();
        let condition_span = s.condition().map_or(span, |c| c.syntax().span());
        let mut value = self.condition(s.condition())?;
        if negation == Some(Negation::Bang)
            && matches!(self.etype_of(&value.ty), Some(Etype::String | Etype::Float))
        {
            value = self.test(Test::Not, &value, condition_span, "Type mismatch: !%s")?;
            negate = false;
        }
        let start = self.here();
        let (patch1, strip_truth, strip_false) = match self.constant_zero(&value) {
            Some(zero) => {
                let strip_truth = zero != negate;
                let patch = strip_truth.then(|| self.jump(Step::Goto, Op::Goto, None, line));
                (patch, strip_truth, !strip_truth)
            }
            None => {
                let jump = if negate { Jump::If } else { Jump::IfNot };
                self.branch(&value, jump, condition_span)?;
                (Some(self.branch_jump(None, line)), false, false)
            }
        };
        let labels = self.layout.labels();
        self.statement_of(s.then_branch());
        let (patch1, strip_truth) = if strip_truth && labels == self.layout.labels() {
            self.truncate(start);
            (None, true)
        } else {
            (patch1, false)
        };
        let Some(else_branch) = s.else_branch() else {
            if let Some(patch1) = patch1 {
                let end = self.here();
                self.layout.patch(patch1, end);
            }
            return Ok(());
        };
        let here = self.here();
        let patch2 = (!strip_truth).then(|| self.jump(Step::Goto, Op::Goto, None, line));
        let after_goto = self.here();
        if let Some(patch1) = patch1 {
            self.layout.patch(patch1, after_goto);
        }
        let labels = self.layout.labels();
        self.statement(&else_branch);
        let patch2 = if strip_false && labels == self.layout.labels() {
            self.truncate(here);
            if let Some(patch1) = patch1 {
                self.layout.patch(patch1, here);
            }
            None
        } else {
            patch2
        };
        if let Some(patch2) = patch2 {
            let end = self.here();
            self.layout.patch(patch2, end);
        }
        Ok(())
    }

    /// FTEF-34.3: `while (e)` and `until (e)`: a constant float condition generates no test, or a
    /// `goto` past the loop when it never holds.
    fn while_stmt(&mut self, s: &ast::WhileStmt, line: u32) -> Result<(), Fatal> {
        let (breaks, continues) = self.layout.marks();
        let top = self.here();
        let until = s.until();
        let condition_span = s
            .condition()
            .map_or(s.syntax().span(), |c| c.syntax().span());
        let value = self.condition(s.condition())?;
        let constant = match self.etype_of(&value.ty) {
            Some(Etype::Float) => self.constant_zero(&value),
            _ => None,
        };
        let patch1 = match constant {
            Some(zero) => (zero != until).then(|| self.jump(Step::Goto, Op::Goto, None, line)),
            None => {
                let jump = if until { Jump::If } else { Jump::IfNot };
                self.branch(&value, jump, condition_span)?;
                Some(self.branch_jump(None, line))
            }
        };
        self.statement_of(s.body());
        self.jump(Step::Goto, Op::Goto, Some(top), line);
        let end = self.here();
        if let Some(patch1) = patch1 {
            self.layout.patch(patch1, end);
        }
        self.layout.patch_breaks(breaks, end);
        self.layout.patch_continues(continues, top);
        Ok(())
    }

    /// FTEF-34.3: `do s while (e);` and `do s until (e);`: a constant condition generates no test,
    /// or a `goto` back when it holds.
    fn do_stmt(&mut self, s: &ast::DoStmt, line: u32) -> Result<(), Fatal> {
        let (breaks, continues) = self.layout.marks();
        let top = self.here();
        self.statement_of(s.body());
        let until = s.until();
        let test = self.here();
        let condition_span = s
            .condition()
            .map_or(s.syntax().span(), |c| c.syntax().span());
        let value = self.condition(s.condition())?;
        match value.known() {
            Some(constant) => {
                if (constant.i() == 0) == until {
                    self.jump(Step::Goto, Op::Goto, Some(top), line);
                }
            }
            None => {
                let jump = if until { Jump::IfNot } else { Jump::If };
                self.branch(&value, jump, condition_span)?;
                self.branch_jump(Some(top), line);
            }
        }
        let end = self.here();
        self.layout.patch_breaks(breaks, end);
        self.layout.patch_continues(continues, test);
        Ok(())
    }

    /// FTEF-34.4: `for (init; e; step)`: the condition is tested by `OP_IFNOT_I`, with no test of
    /// its type, and the step follows the body.
    fn for_stmt(&mut self, s: &ast::ForStmt, line: u32) -> Result<(), Fatal> {
        let (breaks, continues) = self.layout.marks();
        // FTEF-19.78: the local arrays' functions the loop makes are forgotten at its end.
        let begin = self.next_made();
        self.effects.push((begin, ir::Effect::ForBegin));
        if let Some(init) = s.init() {
            self.layout.push(Step::Other, None, line, None);
            // The clause's expressions and the locals it declares, in order.
            for child in init.children() {
                if let Some(local) = ast::LocalDecl::cast(child.clone()) {
                    self.local(&local)?;
                } else if let Some(expr) = ast::Expr::cast(child) {
                    self.value(Some(expr))?;
                }
            }
        }
        let top = self.here();
        let patch1 = match s.condition() {
            Some(condition) => {
                let mut last = None;
                for expr in condition.children().filter_map(ast::Expr::cast) {
                    last = Some(self.condition(Some(expr))?);
                }
                // The test is the last condition's `IFNOT`, short as FTEF-34.16 makes it.
                let (op, condition) = match last {
                    Some(value) => match self.drops_not(&value) {
                        Some(operand) => (Op::IfI, operand),
                        None => (Op::IfnotI, self.name_of(&value)),
                    },
                    None => (Op::IfnotI, None),
                };
                self.branch_condition = condition;
                Some(self.jump(Step::Branch, op, None, line))
            }
            None => None,
        };
        let step = s.step();
        // The step is read before the body and its statements emitted after it.
        let before_step = self.generated.len();
        if let Some(step) = &step {
            for expr in step.children().filter_map(ast::Expr::cast) {
                self.value(Some(expr))?;
            }
        }
        let mut step_statements = self
            .generated
            .split_off(before_step.min(self.generated.len()));
        // A vector the step took is made where the step was read, which the moved statements
        // would no longer follow.
        let from = u32::try_from(before_step).unwrap_or(u32::MAX);
        if self
            .effects
            .iter()
            .any(|&(at, effect)| at >= from && matches!(effect, ir::Effect::Block(_)))
        {
            self.followed = false;
        }
        self.statement_of(s.body());
        let next = self.here();
        // The step's statements move after the body's, and the values they make with them.
        let moved = self
            .next_made()
            .saturating_sub(u32::try_from(before_step).unwrap_or(0));
        for statement in &mut step_statements {
            statement.shift_made(u32::try_from(before_step).unwrap_or(0), moved);
        }
        self.generated.extend(step_statements);
        if step.is_some() {
            self.layout.push(Step::Other, None, line, None);
        }
        self.jump(Step::Goto, Op::Goto, Some(top), line);
        let end = self.here();
        if let Some(patch1) = patch1 {
            self.layout.patch(patch1, end);
        }
        self.layout.patch_breaks(breaks, end);
        self.layout.patch_continues(continues, next);
        let after = self.next_made();
        self.effects.push((after, ir::Effect::ForEnd));
        Ok(())
    }

    /// FTEF-34.5: `switch (e)`: the opcode `OP_SWITCH_*` for its type when the target has it,
    /// else a comparison of each `case` with the value after the body; each case value is made
    /// the switch's type, and a type no comparison takes is "Bad switch type".
    fn switch(&mut self, s: &ast::SwitchStmt, line: u32) -> Result<(), Fatal> {
        let span = s.syntax().span();
        let breaks = self.layout.marks().0;
        let value = self.condition(s.scrutinee())?;
        let etype = self.etype_of(&value.ty);
        // The opcode and the comparisons read an enum as its base type, a range does not.
        let compared = match self.env.shape(&value.ty) {
            Some(shape) if shape.etype == Etype::Enum => shape.holds,
            Some(shape) => Some(shape.etype),
            None => None,
        };
        let op = match compared {
            Some(Etype::Float) => Some(Op::SwitchF),
            Some(Etype::Entity) => Some(Op::SwitchE),
            Some(Etype::Vector) => Some(Op::SwitchV),
            Some(Etype::String) => Some(Op::SwitchS),
            Some(Etype::Function) => Some(Op::SwitchFnc),
            _ => None,
        };
        let target = self.env.target_at(span.start);
        let hcstyle = op.is_some_and(|op| target.valid(op));
        let step = if hcstyle { Step::Branch } else { Step::Goto };
        let switch_op = op.filter(|_| hcstyle).unwrap_or(Op::Goto);
        // FTE-18.13: the jump copies a call's value the cases still read.
        self.clobber_return();
        let patch1 = self.jump(step, switch_op, None, line);
        let body = self.here();
        // FTEF-16.16: each `case` read while the body is, at any depth, is this switch's.
        self.switches.push(Vec::new());
        if let Some(block) = s.block() {
            for stmt in block.statements() {
                self.statement(&stmt);
            }
        } else {
            self.statement_of(s.statement());
        }
        let cases = self.switches.pop().unwrap_or_default();
        let here = self.here();
        let jumps_out = match self.layout.last() {
            Some(Step::Return | Step::Done | Step::Goto) => here.at() != body.at(),
            Some(Step::Branch | Step::Other) | None => false,
        };
        // FTE-3.7: a `case` or `default` label at the statement after the body is a jump to it
        // (`QCC_AStatementJumpsTo` reads `pr_cases`).
        let labelled = cases.iter().any(|case| case.at == here);
        let patch2 = (!jumps_out || labelled || self.layout.jumps_to(0, here))
            .then(|| self.jump(Step::Goto, Op::Goto, None, line));
        let table = self.here();
        self.layout.patch(patch1, table);
        self.uncovered_items(&value, &cases, span);
        // FTEF-22.9: the cases are converted once the body is read, where FTEQCC's lexer is at
        // the token after it.
        let after = member::next_token(s.syntax()).map_or(span, |token| token.span());
        let mut default = None;
        for case in cases {
            let Some(min) = case.value else {
                // FTEF-34.10: `if (defaultcase >= 0) QCC_PR_ParseError(ERR_MULTIPLEDEFAULTS, ...)`.
                if default.is_some() {
                    let message = "Duplicated default case".to_owned();
                    return self.fatal(Code::E0352, case.span, message).map(drop);
                }
                default = Some(case.at);
                continue;
            };
            let Some(etype) = etype else {
                // A switch of a type the model does not follow: its cases still jump.
                self.jump(Step::Branch, Op::IfI, Some(case.at), line);
                continue;
            };
            let min = self.read(min, case.span)?;
            let min = self.case_value(min, &value, after)?;
            let max = match case.range_end {
                Some(max) => {
                    let max = self.read(max, case.span)?;
                    Some(self.case_value(max, &value, after)?)
                }
                None => None,
            };
            if hcstyle {
                self.jump(Step::Branch, Op::Case, Some(case.at), line);
                continue;
            }
            match max {
                Some(max) => self.case_range(&value, etype, (&min, &max), case.span, case.at)?,
                None => {
                    let compared = compared.unwrap_or(etype);
                    self.case_match(&value, compared, &min, case.span, case.at)?;
                }
            }
        }
        if let Some(at) = default {
            self.jump(Step::Goto, Op::Goto, Some(at), line);
        }
        let end = self.here();
        if let Some(patch2) = patch2 {
            self.layout.patch(patch2, end);
        }
        self.layout.patch_breaks(breaks, end);
        Ok(())
    }

    /// FTEF-34.5: each item of a strict enum switched on that no case covers warns "ENUM::ITEM
    /// not part of switch", the items in the reverse of their order (FTEQCC prepends each to
    /// `accessors`). A `default`, a range, a value of another type or one that is no constant
    /// covers every item, as FTEQCC cannot verify past it.
    fn uncovered_items(&mut self, value: &Value, cases: &[Case], span: Span) {
        let items = self.env.enum_items(&value.ty);
        for (item, constant) in items.into_iter().rev() {
            let Some(constant) = constant else {
                continue;
            };
            let uncovered = cases
                .iter()
                .all(|case| match (&case.value, &case.range_end) {
                    (Some(min), None) => {
                        min.reference == Reference::Global
                            && min.ty == value.ty
                            && min.known().is_some_and(|known| known != constant)
                    }
                    (Some(_), Some(_)) | (None, _) => false,
                });
            if uncovered {
                let message = format!("{}::{item} not part of switch", self.own_name(&value.ty));
                self.env.report(Code::W0263, span, message);
            }
        }
    }

    /// A `case` label where it stands: its value, which may generate no code, "Case statements may
    /// not use formulas".
    fn case(&mut self, label: &ast::CaseLabel, cases: &mut Vec<Case>) -> Result<(), Fatal> {
        let span = label.syntax().span();
        let at = self.here();
        if label.is_default() {
            cases.push(Case {
                value: None,
                range_end: None,
                span,
                at,
            });
            return Ok(());
        }
        let emitted = self.emitted;
        let value = self.expr_of(label.value())?;
        let range_end = match label.range_end() {
            Some(end) => Some(self.expr(&end)?),
            None => None,
        };
        if self.emitted != emitted {
            let message = "Case statements may not use formulas".to_owned();
            return self.fatal(Code::E0342, span, message).map(drop);
        }
        cases.push(Case {
            value: Some(value),
            range_end,
            span,
            at,
        });
        Ok(())
    }

    /// A case value made the switch's type, `QCC_SupplyConversion(dmin, e.cast->type, true)`.
    fn case_value(&mut self, case: Value, switch: &Value, span: Span) -> Result<Value, Fatal> {
        // FTEF-22.9: `dmin.cast->type != e.cast->type` tells a truth value, `ev_boolean`, from
        // its parent type.
        let same = self.etype_of(&case.ty) == self.etype_of(&switch.ty)
            && is_truth(&case.ty) == is_truth(&switch.ty);
        if case.ty == Type::Unknown || same {
            return Ok(case);
        }
        self.supplied(&case, &switch.ty, span)
    }

    /// A `case` of one value with no `OP_SWITCH_*`: the equality of the switch's type, or for a
    /// constant 0 the test of the value itself; any other type is "Bad switch type".
    fn case_match(
        &mut self,
        value: &Value,
        etype: Etype,
        case: &Value,
        span: Span,
        at: Mark,
    ) -> Result<(), Fatal> {
        let line = self.env.line(span);
        if case.known().is_some_and(|c| c.i() == 0) {
            self.branch(value, Jump::IfNot, span)?;
            self.branch_jump(Some(at), line);
            return Ok(());
        }
        let op = match etype {
            Etype::Float => Op::EqF,
            Etype::Entity => Op::EqE,
            Etype::Vector => Op::EqV,
            Etype::String => Op::EqS,
            Etype::Function | Etype::Field => Op::EqFnc,
            Etype::Pointer => Op::EqP,
            Etype::Integer | Etype::Uint => Op::EqI,
            Etype::Int64 | Etype::Uint64 => Op::EqI64,
            Etype::Double => Op::EqD,
            Etype::Variant
            | Etype::Struct
            | Etype::Union
            | Etype::Void
            | Etype::Boolean
            | Etype::Enum => {
                return self
                    .fatal(Code::E0343, span, "Bad switch type".to_owned())
                    .map(drop);
            }
        };
        self.branch_condition = self.emit(op, value, case, span)?.name();
        self.jump(Step::Branch, Op::IfI, Some(at), line);
        Ok(())
    }

    /// A `case min .. max` with no `OP_CASERANGE`: `value >= min && value <= max` of a number;
    /// any other type warns "switch caserange MUST be a float or integer".
    fn case_range(
        &mut self,
        value: &Value,
        etype: Etype,
        (min, max): (&Value, &Value),
        span: Span,
        at: Mark,
    ) -> Result<(), Fatal> {
        let line = self.env.line(span);
        let (ge, le, and) = match etype {
            Etype::Float => (Op::GeF, Op::LeF, Op::AndF),
            Etype::Double => (Op::GeD, Op::LeD, Op::AndF),
            Etype::Integer => (Op::GeI, Op::LeI, Op::AndI),
            Etype::Uint => (Op::GeU, Op::LeU, Op::AndI),
            Etype::Int64 => (Op::GeI64, Op::LeI64, Op::AndI),
            Etype::Uint64 => (Op::GeU64, Op::LeU64, Op::AndI),
            _ => {
                let message = "switch caserange MUST be a float or integer".to_owned();
                self.env.report(Code::W0260, span, message);
                return Ok(());
            }
        };
        let above = self.emit(ge, value, min, span)?;
        let below = self.emit(le, value, max, span)?;
        let within = self.emit(and, &above, &below, span)?;
        self.branch(&within, Jump::If, span)?;
        self.branch_jump(Some(at), line);
        Ok(())
    }

    /// FTEF-34.7: `thinktime e : t;`: the time made a float, then an entity and a float or the
    /// fatal error "thinktime type mismatch".
    fn thinktime(&mut self, s: &ast::ThinktimeStmt, line: u32) -> Result<(), Fatal> {
        self.layout.push(Step::Other, None, line, None);
        let mut operands = s.operands();
        let entity = self.value(operands.next())?;
        let time_expr = operands.next();
        let time_span = time_expr
            .as_ref()
            .map_or(s.syntax().span(), |e| e.syntax().span());
        let time = self.value(time_expr)?;
        if entity.ty == Type::Unknown || time.ty == Type::Unknown {
            return Ok(());
        }
        let time = self.supplied(&time, &Type::Simple(SimpleType::Float), time_span)?;
        if self.etype_of(&entity.ty) != Some(Etype::Entity)
            || self.etype_of(&time.ty) != Some(Etype::Float)
        {
            let message = "thinktime type mismatch".to_owned();
            return self
                .fatal(Code::E0344, s.syntax().span(), message)
                .map(drop);
        }
        Ok(())
    }

    /// FTEF-34.9: the initialiser `expr` of the global `name` of type `ty`, read at global scope.
    pub(super) fn initialiser(
        &mut self,
        expr: &ast::Expr,
        ty: &Type,
        name: &str,
    ) -> Option<crate::InitialValue> {
        let checked = self.initialiser_value(expr, ty, name);
        if let Some((message, span)) = self.pending.take()
            && checked.is_ok()
        {
            self.env.report(Code::E0204, span, message);
        }
        // FTEF-18.38: the definition ends at the token, where the next one's type is read.
        if let Some((span, token)) = self.stray.take()
            && checked.is_ok()
        {
            let message = "Missing semicolon at end of definition".to_owned();
            self.env.report(Code::W0203, span, message);
            self.env
                .report(Code::E0203, span, format!("\"{token}\" is not a type"));
        }
        checked.ok().flatten().and_then(Constant::initial_value)
    }

    /// FTEF-19.62: `QCC_PR_ParseDefArray`'s size: `QCC_SRef_EvalConst` of the expression, an
    /// integer by `QCC_Eval_Int`, else "Array length is not a constant".
    pub(super) fn array_length(&mut self, expr: &ast::Expr) -> Result<Option<u32>, Fatal> {
        let value = self.value(Some(expr.clone()))?;
        let size = value.known().and_then(eval_int);
        if size.is_none() {
            let message = "Array length is not a constant".to_owned();
            self.env.report(Code::E0366, expr.syntax().span(), message);
        }
        Ok(size.and_then(|n| u32::try_from(n).ok()))
    }

    /// FTEF-19.62: a local array's size that is an expression, folded where the local is
    /// declared; a first dimension a target with `OP_PUSH` allocates as the function runs
    /// (`dynlength`, under `qcfuncs` off, not `static` and with no initialiser) is not.
    pub(super) fn local_size(&mut self, declarator: &ast::Declarator) -> Result<(), Fatal> {
        let Some(name) = declarator.name() else {
            return Ok(());
        };
        let Some(suffix) = declarator.array_suffixes().next() else {
            return Ok(());
        };
        let Some(expr) = suffix.children().find_map(ast::Expr::cast) else {
            return Ok(());
        };
        if matches!(expr, ast::Expr::Literal(_)) {
            return Ok(());
        }
        let at = name.span().start;
        let dynamic = self.env.target_at(at).valid(Op::Push)
            && !self.env.flags_at(at).has(FteFlag::Qcfuncs)
            && !self.env.local_static(at)
            && declarator.initializer().is_none();
        if dynamic {
            return Ok(());
        }
        if let Some(size) = self.array_length(&expr)? {
            self.env.size_local(at, size);
        }
        Ok(())
    }

    /// The initialiser read and cast to the global's type, and what it folds to.
    fn initialiser_value(
        &mut self,
        expr: &ast::Expr,
        ty: &Type,
        name: &str,
    ) -> Result<Option<Constant>, Fatal> {
        let value = self.value(Some(expr.clone()))?;
        if value.ty == Type::Unknown {
            return Ok(None);
        }
        let span = expr.syntax().span();
        let value = self.cast_to(&value, ty, span)?;
        self.constant_initialiser(&value, name, span);
        Ok(value.known())
    }

    /// "initializer for 'NAME' is not constant" when `value` is no constant: neither one the
    /// model folds nor a constant definition (`tmp.sym->constant`, which a field's offset is,
    /// FTEF-18.38).
    pub(super) fn constant_initialiser(&mut self, value: &Value, name: &str, span: Span) {
        let constant_def = value.sym.def().is_some_and(|def| self.env.constant(def));
        if value.constness == Constness::Variable && !constant_def {
            let message = format!("initializer for '{name}' is not constant");
            self.env.report(Code::E0214, span, message);
        }
    }

    /// FTEF-34.12: the end of a function defined inside this one checks every `goto` read since
    /// the last function's end, this one's included, against the labels read since then, its
    /// own included, then forgets both; a `goto` with none is fatal there.
    fn inner_function(&mut self, inner: &ast::FunctionBody) -> Result<(), Fatal> {
        let (labels, gotos) = own_jumps(inner);
        if let Some(name) = self.layout.unlabelled(&labels, &gotos) {
            let message = format!("Goto statement with no matching label \"{name}\"");
            let end = inner
                .block()
                .and_then(|b| b.syntax().token_of(qc_syntax::SyntaxKind::RBrace))
                .map_or_else(|| inner.syntax().span(), |brace| brace.span());
            return self.fatal(Code::E0308, end, message).map(drop);
        }
        self.layout.forget_jumps();
        Ok(())
    }

    /// Whether the function returns a type larger than a vector, through a pointer its caller
    /// passes.
    fn returns_large(&self) -> bool {
        self.is_large(&self.function.ret)
    }

    /// Whether `ty` is larger than a vector, `ty->size > type_vector->size`, so a function
    /// returns it through a pointer its caller passes.
    pub(super) fn is_large(&self, ty: &Type) -> bool {
        self.env.layout(ty).is_some_and(|l| l.words > VECTOR_WORDS)
    }

    /// FTEF-34.8: the start of a function returning a type larger than a vector stores the
    /// pointer its caller passed with `OP_STORE_P`, which a target without it refuses, a fatal
    /// error that ends the function's checks; whether they go on.
    pub(super) fn start_function(&mut self, span: Span) -> bool {
        if !self.returns_large() {
            return true;
        }
        if self.env.target_at(span.start).valid(Op::StoreP) {
            return true;
        }
        self.refuse(Op::StoreP, span);
        false
    }

    /// FTEF-34.6: the checks of the finished function, at the end of its body `span`: "not all
    /// control paths return a value", and "contains unreachable code".
    pub(super) fn finish_function(&mut self, span: Span) {
        if self.env.failed() {
            return;
        }
        // FTEF-34.12: "Goto statement with no matching label", a fatal error at the function's
        // end that ends its checks, before the illegal jumps.
        if let Some(name) = self.layout.unlabelled(&[], &[]).filter(|_| !self.nested) {
            let message = format!("Goto statement with no matching label \"{name}\"");
            self.env.report(Code::E0308, span, message);
            return;
        }
        // FTEF-21.19: a `continue` or `break` no loop took, which a stripped `if` arm does not
        // leave (`QCC_UngenerateStatements`), is "%s: function contains illegal ...".
        let (continues, breaks) = self.layout.left_over();
        for (left, code, what) in [
            (continues, Code::E0245, "continues"),
            (breaks, Code::E0244, "breaks"),
        ] {
            if left {
                let message = format!("{}: function contains illegal {what}", self.function.name);
                self.env.report(code, span, message);
                return;
            }
        }
        if self.function.accumulate || self.function.ret == Type::Unknown {
            return;
        }
        if self.returns_large() {
            // FTEF-34.8: `QCC_PR_ParseInitializerType` once the body is read.
            let message = format!(
                "Function {} returns (large) {}. This is inefficient.",
                self.function.name,
                self.env.type_name(&self.function.ret)
            );
            self.env.report(Code::W0262, span, message);
        }
        let void = self.etype_of(&self.function.ret) == Some(Etype::Void);
        let line = self.env.line(span);
        let layout = std::mem::take(&mut self.layout);
        let name = self.function.name.clone();
        for finding in layout.finish(self.returned, void, line) {
            match finding {
                Finding::MissingReturn => {
                    let message = format!("{name}: not all control paths return a value");
                    self.env.report(Code::W0258, span, message);
                }
                Finding::Unreachable(at) => {
                    let message = format!("{name}: contains unreachable code (line {at})");
                    let at = self.line_starts.get(&at).copied().unwrap_or(span);
                    self.env.report(Code::W0259, at, message);
                }
            }
        }
    }
}

/// The bodies of the functions a local declaration defines, not those defined inside them.
fn inner_functions(node: &qc_syntax::SyntaxNode) -> Vec<ast::FunctionBody> {
    node.descendants()
        .filter_map(ast::FunctionBody::cast)
        .filter(|body| {
            !body
                .syntax()
                .ancestors()
                .skip(1)
                .take_while(|n| n != node)
                .any(|n| n.kind() == qc_syntax::SyntaxKind::FunctionBody)
        })
        .collect()
}

/// The labels and the `goto`s' labels of `body`, not those of a function defined inside it.
fn own_jumps(body: &ast::FunctionBody) -> (Vec<String>, Vec<String>) {
    let mut labels = Vec::new();
    let mut gotos = Vec::new();
    let own = |node: &qc_syntax::SyntaxNode| {
        !node
            .ancestors()
            .take_while(|n| n != body.syntax())
            .any(|n| n.kind() == qc_syntax::SyntaxKind::FunctionBody)
    };
    for node in body.syntax().descendants().filter(own) {
        if let Some(label) = ast::LabelStmt::cast(node.clone()).and_then(|s| s.name()) {
            labels.push(label.text_lossless());
        } else if let Some(goto) = ast::GotoStmt::cast(node).and_then(|s| s.label()) {
            gotos.push(goto.text_lossless());
        }
    }
    (labels, gotos)
}

/// Whether an expression statement generates code: any expression but a name or a literal,
/// which `QCC_PR_DiscardExpression` reads no statement for.
fn generates(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Name(_) | ast::Expr::Literal(_) => false,
        ast::Expr::Paren(paren) => paren.expr().as_ref().is_some_and(generates),
        _ => true,
    }
}

impl Checker<'_> {
    /// FTEF-24.11: a type's own name, `type->name`: a pointer's `ptr` (`QCC_PointerTypeTo`), a
    /// function's `__function` (`type_function->name`), a field's its type's after a `.`
    /// (`QCC_PR_FieldType`), and a struct's, union's, class's or enum's its name without its
    /// keyword or base.
    pub(super) fn own_name(&self, ty: &Type) -> String {
        match ty {
            Type::Pointer(_) => return "ptr".to_owned(),
            Type::Function(_) => return "__function".to_owned(),
            Type::Field(inner) => return format!(".{}", self.own_name(inner)),
            Type::Unknown | Type::Simple(_) | Type::Named(_) => {}
        }
        let type_name = self.env.type_name(ty);
        for keyword in ["struct ", "union ", "class "] {
            if let Some(name) = type_name.strip_prefix(keyword) {
                return name.to_owned();
            }
        }
        match type_name.strip_prefix("enum ") {
            Some(name) => name.split(':').next().unwrap_or(name).to_owned(),
            None => type_name,
        }
    }
}
