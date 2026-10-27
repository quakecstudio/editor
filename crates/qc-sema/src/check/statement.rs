// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseStatement, QCC_PR_ParseStatement_For, QCC_PR_FinaliseFunction

//! Statements as FTEQCC checks them (FTEF-34): what each returns, the test of each condition,
//! the cases of a `switch`, `thinktime`, and the layout of the jumps they generate, from which the
//! finished function is checked for a missing return and for code no path reaches.

use qc_diag::{Code, Span};
use qc_preproc::{Op, TargetFormat};
use qc_syntax::ast::{self, AstNode as _, Negation};

use crate::check::etype::Etype;
use crate::check::expr::{Checker, Fatal};
use crate::check::flow::{Finding, Step};
use crate::check::truth::{Jump, Test};
use crate::check::{Constness, Reference, Value};
use crate::ty::{SimpleType, Type};

/// The words of a vector, `type_vector->size`.
const VECTOR_WORDS: u32 = 3;

/// A `case` of a `switch`: its value, the end of its range, where it is and the statement it
/// jumps to (`pr_casesref`, `pr_casesref2`, `pr_cases`).
struct Case {
    value: Option<Value>,
    range_end: Option<Value>,
    span: Span,
    at: usize,
}

impl Checker<'_> {
    /// Checks `stmt`; a fatal error in one of its expressions ends its checks (FTEF-23.6).
    pub(super) fn statement(&mut self, stmt: &ast::Stmt) {
        let ended = self.statement_parts(stmt);
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
        match stmt {
            ast::Stmt::Block(block) => self.block(block),
            ast::Stmt::Expr(s) => {
                let expr = s.expr();
                if expr.as_ref().is_some_and(generates) {
                    self.layout.push(Step::Other, None, line);
                }
                self.value(expr)?;
            }
            ast::Stmt::Return(s) => self.return_stmt(s, line)?,
            ast::Stmt::If(s) => self.if_stmt(s, line)?,
            ast::Stmt::While(s) => self.while_stmt(s, line)?,
            ast::Stmt::Do(s) => self.do_stmt(s, line)?,
            ast::Stmt::Loop(s) => {
                let (breaks, continues) = self.layout.marks();
                let top = self.layout.here();
                self.statement_of(s.body());
                self.layout.push(Step::Goto, Some(top), line);
                let end = self.layout.here();
                self.layout.patch_breaks(breaks, end);
                self.layout.patch_continues(continues, top);
            }
            ast::Stmt::For(s) => self.for_stmt(s, line)?,
            ast::Stmt::Switch(s) => self.switch(s, line)?,
            ast::Stmt::Local(local) => {
                let initialised = local.declarators().any(|d| d.initializer().is_some());
                if initialised {
                    self.layout.push(Step::Other, None, line);
                }
                self.local(local)?;
            }
            ast::Stmt::Break(_) => self.layout.break_(line),
            ast::Stmt::Continue(_) => self.layout.continue_(line),
            ast::Stmt::Label(s) => {
                if let Some(name) = s.name() {
                    self.layout.label(name.text_lossless());
                }
            }
            ast::Stmt::Goto(s) => {
                let name = s.label().map(|t| t.text_lossless()).unwrap_or_default();
                self.layout.goto(name, line);
            }
            ast::Stmt::Thinktime(s) => self.thinktime(s, line)?,
            ast::Stmt::Exit(_) => {
                self.layout.push(Step::Done, None, line);
            }
            ast::Stmt::State(_) | ast::Stmt::Asm(_) => {
                self.layout.push(Step::Other, None, line);
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
        let ret_name = own_name(&self.env.type_name(&ret));
        // A type larger than a vector is returned through a pointer, which `return;` and
        // `return =` do not handle.
        let large = self.returns_large();
        if s.sets() {
            if large {
                let message = format!("'{name}' not supported when returning {ret_name}");
                return self.fatal(Code::E0341, span, message).map(drop);
            }
            self.returned = true;
            self.layout.push(Step::Other, None, line);
            self.value(s.value())?;
            return Ok(());
        }
        let Some(expr) = s.value() else {
            if !void && ret != Type::Unknown && self.env.fteqcc() {
                let message = format!("'{name}' returned nothing, expected {ret_name}");
                if large {
                    return self.fatal(Code::E0341, span, message).map(drop);
                }
                if !self.function.accumulate && !self.returned {
                    self.env.report(Code::W0256, span, message);
                }
                self.returned = true;
            }
            self.layout.push(Step::Return, None, line);
            return Ok(());
        };
        let value = self.value(Some(expr.clone()))?;
        self.generated(&expr, &value, line);
        self.layout.push(Step::Return, None, line);
        if value.ty == Type::Unknown || ret == Type::Unknown || !self.env.fteqcc() || value.null() {
            return Ok(());
        }
        if self.etype_of(&value.ty) == self.etype_of(&ret) {
            return Ok(());
        }
        let value_span = expr.syntax().span();
        if void {
            let message = format!(
                "'{name}' returned {}, expected {ret_name}",
                own_name(&self.env.type_name(&value.ty))
            );
            self.env.report(Code::W0257, value_span, message);
            return Ok(());
        }
        self.cast_to(&value, &ret, value_span)?;
        Ok(())
    }

    /// A condition of `if`, `while`, `do` or `switch`: an assignment at its top, outside
    /// parentheses, is "suggest parenthesis" (FTEF-29.6).
    fn condition(&mut self, expr: Option<ast::Expr>) -> Result<Value, Fatal> {
        self.in_condition = true;
        let checked = self.value(expr.clone());
        self.in_condition = false;
        let value = checked?;
        if let Some(expr) = &expr {
            let line = self.env.line(expr.syntax().span());
            self.generated(expr, &value, line);
        }
        Ok(value)
    }

    /// The statements an expression generates, if it generates any: one stands for them all.
    fn generated(&mut self, expr: &ast::Expr, value: &Value, line: u32) {
        if generates(expr) && value.known().is_none() {
            self.layout.push(Step::Other, None, line);
        }
    }

    /// The constant value of a condition, `QCC_SRef_EvalConst`, read as zero or not: a float as
    /// `_float`, anything else as `_int`.
    fn constant_zero(&self, value: &Value) -> Option<bool> {
        let constant = value.known()?;
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
        let start = self.layout.here();
        let (patch1, strip_truth, strip_false) = match self.constant_zero(&value) {
            Some(zero) => {
                let strip_truth = zero != negate;
                let patch = strip_truth.then(|| self.layout.push(Step::Goto, None, line));
                (patch, strip_truth, !strip_truth)
            }
            None => {
                let jump = if negate { Jump::If } else { Jump::IfNot };
                self.branch(&value, jump, condition_span)?;
                (
                    Some(self.layout.push(Step::Branch, None, line)),
                    false,
                    false,
                )
            }
        };
        let labels = self.layout.labels();
        self.statement_of(s.then_branch());
        let (patch1, strip_truth) = if strip_truth && labels == self.layout.labels() {
            self.layout.truncate(start);
            (None, true)
        } else {
            (patch1, false)
        };
        let Some(else_branch) = s.else_branch() else {
            if let Some(patch1) = patch1 {
                let end = self.layout.here();
                self.layout.patch(patch1, end);
            }
            return Ok(());
        };
        let here = self.layout.here();
        let patch2 = (!strip_truth).then(|| self.layout.push(Step::Goto, None, line));
        let after_goto = self.layout.here();
        if let Some(patch1) = patch1 {
            self.layout.patch(patch1, after_goto);
        }
        let labels = self.layout.labels();
        self.statement(&else_branch);
        let patch2 = if strip_false && labels == self.layout.labels() {
            self.layout.truncate(here);
            if let Some(patch1) = patch1 {
                self.layout.patch(patch1, here);
            }
            None
        } else {
            patch2
        };
        if let Some(patch2) = patch2 {
            let end = self.layout.here();
            self.layout.patch(patch2, end);
        }
        Ok(())
    }

    /// FTEF-34.3: `while (e)` and `until (e)`: a constant float condition generates no test, or a
    /// `goto` past the loop when it never holds.
    fn while_stmt(&mut self, s: &ast::WhileStmt, line: u32) -> Result<(), Fatal> {
        let (breaks, continues) = self.layout.marks();
        let top = self.layout.here();
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
            Some(zero) => (zero != until).then(|| self.layout.push(Step::Goto, None, line)),
            None => {
                let jump = if until { Jump::If } else { Jump::IfNot };
                self.branch(&value, jump, condition_span)?;
                Some(self.layout.push(Step::Branch, None, line))
            }
        };
        self.statement_of(s.body());
        self.layout.push(Step::Goto, Some(top), line);
        let end = self.layout.here();
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
        let top = self.layout.here();
        self.statement_of(s.body());
        let until = s.until();
        let test = self.layout.here();
        let condition_span = s
            .condition()
            .map_or(s.syntax().span(), |c| c.syntax().span());
        let value = self.condition(s.condition())?;
        match value.known() {
            Some(constant) => {
                if (constant.i() == 0) == until {
                    self.layout.push(Step::Goto, Some(top), line);
                }
            }
            None => {
                let jump = if until { Jump::IfNot } else { Jump::If };
                self.branch(&value, jump, condition_span)?;
                self.layout.push(Step::Branch, Some(top), line);
            }
        }
        let end = self.layout.here();
        self.layout.patch_breaks(breaks, end);
        self.layout.patch_continues(continues, test);
        Ok(())
    }

    /// FTEF-34.4: `for (init; e; step)`: the condition is tested by `OP_IFNOT_I`, with no test of
    /// its type, and the step follows the body.
    fn for_stmt(&mut self, s: &ast::ForStmt, line: u32) -> Result<(), Fatal> {
        let (breaks, continues) = self.layout.marks();
        if let Some(init) = s.init() {
            self.layout.push(Step::Other, None, line);
            for expr in init.children().filter_map(ast::Expr::cast) {
                self.value(Some(expr))?;
            }
        }
        let top = self.layout.here();
        let patch1 = match s.condition() {
            Some(condition) => {
                for expr in condition.children().filter_map(ast::Expr::cast) {
                    self.condition(Some(expr))?;
                }
                Some(self.layout.push(Step::Branch, None, line))
            }
            None => None,
        };
        let step = s.step();
        if let Some(step) = &step {
            for expr in step.children().filter_map(ast::Expr::cast) {
                self.value(Some(expr))?;
            }
        }
        self.statement_of(s.body());
        let next = self.layout.here();
        if step.is_some() {
            self.layout.push(Step::Other, None, line);
        }
        self.layout.push(Step::Goto, Some(top), line);
        let end = self.layout.here();
        if let Some(patch1) = patch1 {
            self.layout.patch(patch1, end);
        }
        self.layout.patch_breaks(breaks, end);
        self.layout.patch_continues(continues, next);
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
        let patch1 = self.layout.push(step, None, line);
        let body = self.layout.here();
        let mut cases = Vec::new();
        if let Some(block) = s.block() {
            for node in block.syntax().children() {
                if let Some(label) = ast::CaseLabel::cast(node.clone()) {
                    let _ended: Result<(), Fatal> = self.case(&label, &mut cases);
                } else if let Some(stmt) = ast::Stmt::cast(node) {
                    self.statement(&stmt);
                }
            }
        } else {
            self.statement_of(s.statement());
        }
        let here = self.layout.here();
        let jumps_out = match self.layout.last() {
            Some(Step::Return | Step::Done | Step::Goto) => here != body,
            Some(Step::Branch | Step::Other) | None => false,
        };
        let patch2 = (!jumps_out || self.layout.jumps_to(0, here))
            .then(|| self.layout.push(Step::Goto, None, line));
        let table = self.layout.here();
        self.layout.patch(patch1, table);
        self.uncovered_items(&value, &cases, span);
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
                self.layout.push(Step::Branch, Some(case.at), line);
                continue;
            };
            let min = self.read(min, case.span)?;
            let min = self.case_value(min, &value, case.span)?;
            let max = match case.range_end {
                Some(max) => {
                    let max = self.read(max, case.span)?;
                    Some(self.case_value(max, &value, case.span)?)
                }
                None => None,
            };
            if hcstyle {
                self.layout.push(Step::Branch, Some(case.at), line);
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
            self.layout.push(Step::Goto, Some(at), line);
        }
        let end = self.layout.here();
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
                let message = format!(
                    "{}::{item} not part of switch",
                    own_name(&self.env.type_name(&value.ty))
                );
                self.env.report(Code::W0263, span, message);
            }
        }
    }

    /// A `case` label where it stands: its value, which may generate no code, "Case statements may
    /// not use formulas".
    fn case(&mut self, label: &ast::CaseLabel, cases: &mut Vec<Case>) -> Result<(), Fatal> {
        let span = label.syntax().span();
        let at = self.layout.here();
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
        if case.ty == Type::Unknown || self.etype_of(&case.ty) == self.etype_of(&switch.ty) {
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
        at: usize,
    ) -> Result<(), Fatal> {
        let line = self.env.line(span);
        if case.known().is_some_and(|c| c.i() == 0) {
            self.branch(value, Jump::IfNot, span)?;
            self.layout.push(Step::Branch, Some(at), line);
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
        self.emit(op, value, case, span)?;
        self.layout.push(Step::Branch, Some(at), line);
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
        at: usize,
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
        self.layout.push(Step::Branch, Some(at), line);
        Ok(())
    }

    /// FTEF-34.7: `thinktime e : t;`: the time made a float, then an entity and a float or the
    /// fatal error "thinktime type mismatch".
    fn thinktime(&mut self, s: &ast::ThinktimeStmt, line: u32) -> Result<(), Fatal> {
        self.layout.push(Step::Other, None, line);
        let mut operands = s.operands();
        let entity = self.value(operands.next())?;
        let time_expr = operands.next();
        let time_span = time_expr
            .as_ref()
            .map_or(s.syntax().span(), |e| e.syntax().span());
        let time = self.value(time_expr)?;
        if entity.ty == Type::Unknown || time.ty == Type::Unknown || !self.env.fteqcc() {
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
    pub(super) fn initialiser(&mut self, expr: &ast::Expr, ty: &Type, name: &str) {
        let checked = self.initialiser_value(expr, ty, name);
        if let Some((message, span)) = self.pending.take()
            && checked.is_ok()
        {
            self.env.report(Code::E0204, span, message);
        }
    }

    fn initialiser_value(&mut self, expr: &ast::Expr, ty: &Type, name: &str) -> Result<(), Fatal> {
        let value = self.value(Some(expr.clone()))?;
        if value.ty == Type::Unknown {
            return Ok(());
        }
        let span = expr.syntax().span();
        let value = self.cast_to(&value, ty, span)?;
        if value.constness == Constness::Variable {
            let message = format!("initializer for '{name}' is not constant");
            self.env.report(Code::E0214, span, message);
        }
        Ok(())
    }

    /// Whether the function returns a type larger than a vector, through a pointer its caller
    /// passes.
    fn returns_large(&self) -> bool {
        self.env
            .layout(&self.function.ret)
            .is_some_and(|l| l.words > VECTOR_WORDS)
    }

    /// FTEF-34.8: the start of a function returning a type larger than a vector stores the
    /// pointer its caller passed with `OP_STORE_P`, which a target without it refuses, a fatal
    /// error that ends the function's checks; whether they go on.
    pub(super) fn start_function(&mut self, span: Span) -> bool {
        if !self.env.fteqcc() || !self.returns_large() {
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
        if !self.env.fteqcc()
            || self.function.accumulate
            || self.function.ret == Type::Unknown
            || self.env.failed()
        {
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
                    self.env.report(Code::W0259, span, message);
                }
            }
        }
    }
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

/// A type's own name, `type->name`, from its `TypeName`: a struct's, union's, class's or enum's
/// without its keyword or base.
fn own_name(type_name: &str) -> String {
    for keyword in ["struct ", "union ", "class "] {
        if let Some(name) = type_name.strip_prefix(keyword) {
            return name.to_owned();
        }
    }
    match type_name.strip_prefix("enum ") {
        Some(name) => name.split(':').next().unwrap_or(name).to_owned(),
        None => type_name.to_owned(),
    }
}
