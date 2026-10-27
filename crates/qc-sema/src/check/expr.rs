// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_RefExpression, QCC_PR_ChooseOpcode

//! The type of each expression of a body: terms and operators (FTEF-23).

use std::collections::HashMap;

use qc_diag::{Code, Span};
use qc_preproc::FteFlag;
use qc_syntax::ast::{self, AstNode as _, BinaryOp};

use crate::check::constant::Constant;
use crate::check::convert::{Choice, Operand, choose_opcode};
use crate::check::etype::Etype;
use crate::check::opcodes::{OpType, candidates, row};
use qc_preproc::Op;

use crate::check::fold::{Folded, fold};
use crate::check::{Constness, Env, TypedBody, Value};
use crate::ty::{SimpleType, Type};

/// The checker of one body.
pub(super) struct Checker<'e> {
    pub(super) env: &'e mut dyn Env,
    types: HashMap<(usize, usize), Type>,
}

/// A fatal error reported: FTEQCC's `QCC_PR_ParseError`, after which nothing more of the
/// statement is checked (FTEF-23.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Fatal;

/// The value of an expression, or the fatal error that ended its statement.
pub(super) type Checked = Result<Value, Fatal>;

impl<'e> Checker<'e> {
    pub(super) fn new(env: &'e mut dyn Env) -> Self {
        Self {
            env,
            types: HashMap::new(),
        }
    }

    pub(super) fn finish(self) -> TypedBody {
        TypedBody { types: self.types }
    }

    pub(super) fn block(&mut self, block: &ast::Block) {
        for stmt in block.statements() {
            self.statement(&stmt);
        }
    }

    /// Checks `stmt`; a fatal error in one of its expressions ends its checks (FTEF-23.6).
    fn statement(&mut self, stmt: &ast::Stmt) {
        let _ended: Result<(), Fatal> = self.statement_parts(stmt);
    }

    fn statement_parts(&mut self, stmt: &ast::Stmt) -> Result<(), Fatal> {
        match stmt {
            ast::Stmt::Block(block) => self.block(block),
            ast::Stmt::Expr(s) => self.value_of(s.expr())?,
            ast::Stmt::Return(s) => self.value_of(s.value())?,
            ast::Stmt::If(s) => {
                self.value_of(s.condition())?;
                self.statement_of(s.then_branch());
                self.statement_of(s.else_branch());
            }
            ast::Stmt::While(s) => {
                self.value_of(s.condition())?;
                self.statement_of(s.body());
            }
            ast::Stmt::Do(s) => {
                self.statement_of(s.body());
                self.value_of(s.condition())?;
            }
            ast::Stmt::Loop(s) => self.statement_of(s.body()),
            ast::Stmt::For(s) => {
                for clause in s.clauses() {
                    for expr in clause.children().filter_map(ast::Expr::cast) {
                        self.expr(&expr)?;
                    }
                }
                self.statement_of(s.body());
            }
            ast::Stmt::Switch(s) => {
                self.value_of(s.scrutinee())?;
                if let Some(block) = s.block() {
                    self.block(&block);
                }
                self.statement_of(s.statement());
            }
            ast::Stmt::Local(_)
            | ast::Stmt::Break(_)
            | ast::Stmt::Continue(_)
            | ast::Stmt::Empty(_)
            | ast::Stmt::Label(_)
            | ast::Stmt::Goto(_)
            | ast::Stmt::Thinktime(_)
            | ast::Stmt::State(_)
            | ast::Stmt::Using(_)
            | ast::Stmt::Exit(_)
            | ast::Stmt::Asm(_)
            | ast::Stmt::Extern(_) => {}
        }
        Ok(())
    }

    fn statement_of(&mut self, stmt: Option<ast::Stmt>) {
        if let Some(stmt) = stmt {
            self.statement(&stmt);
        }
    }

    fn value_of(&mut self, expr: Option<ast::Expr>) -> Result<(), Fatal> {
        if let Some(expr) = expr {
            self.expr(&expr)?;
        }
        Ok(())
    }

    /// The value of `expr`, if it is there.
    pub(super) fn expr_of(&mut self, expr: Option<ast::Expr>) -> Checked {
        expr.map_or_else(|| Ok(Value::unknown()), |e| self.expr(&e))
    }

    /// The value of `expr`, its type recorded.
    pub(super) fn expr(&mut self, expr: &ast::Expr) -> Checked {
        let value = match expr {
            ast::Expr::Literal(literal) => self.literal(literal),
            ast::Expr::Name(name) => self.name(name),
            ast::Expr::Paren(paren) => self.expr_of(paren.expr())?,
            ast::Expr::Binary(binary) => self.binary(binary)?,
            ast::Expr::Field(field) => self.member(field)?,
            ast::Expr::Not(_)
            | ast::Expr::Call(_)
            | ast::Expr::Postfix(_)
            | ast::Expr::Ternary(_)
            | ast::Expr::Index(_)
            | ast::Expr::Unary(_)
            | ast::Expr::Cast(_)
            | ast::Expr::Vector(_)
            | ast::Expr::Sizeof(_) => {
                self.children(expr)?;
                Value::unknown()
            }
        };
        let span = expr.syntax().span();
        self.types.insert((span.start, span.end), value.ty.clone());
        Ok(value)
    }

    /// Checks the expressions inside `expr`, a term the checker does not type yet.
    fn children(&mut self, expr: &ast::Expr) -> Result<(), Fatal> {
        for child in expr.syntax().children().filter_map(ast::Expr::cast) {
            self.expr(&child)?;
        }
        Ok(())
    }

    fn literal(&mut self, literal: &ast::Literal) -> Value {
        let Some(token) = literal.token() else {
            return Value::unknown();
        };
        match literal.kind() {
            Some(ast::LiteralKind::String) => {
                Value::constant(Constant::string(token.text_lossless() == "\"\""))
            }
            Some(ast::LiteralKind::Vector) => {
                let flags = self.env.lex_flags_at(token.span().start);
                let lexed = qc_lexer::lex_flagged(
                    token.text_lossless().as_bytes(),
                    qc_lexer::Dialect::Fteqcc,
                    flags,
                    &|_| None,
                );
                match lexed.tokens.first().map(|t| &t.kind) {
                    Some(qc_lexer::TokenKind::Vector(v)) => Value::constant(Constant::vector(*v)),
                    Some(_) | None => Value::of(Type::Simple(SimpleType::Vector)),
                }
            }
            Some(ast::LiteralKind::FrameRef) => Value::of(Type::Simple(SimpleType::Float)),
            Some(ast::LiteralKind::Float) if self.env.fteqcc() => {
                let flags = self.env.lex_flags_at(token.span().start);
                Value::constant(Constant::of_number(&token.text_lossless(), flags))
            }
            Some(ast::LiteralKind::Float) => Value::of(Type::Simple(SimpleType::Float)),
            None => Value::unknown(),
        }
    }

    fn name(&mut self, name: &ast::NameRef) -> Value {
        let Some(token) = name.name() else {
            return Value::unknown();
        };
        let Some(def) = self.env.reference(token.span().start) else {
            return Value::unknown();
        };
        let array = self.env.is_array(def);
        let constness = self.env.constness(def);
        let Some(ty) = self.env.definition(def).map(|def| def.ty.clone()) else {
            return Value::unknown();
        };
        // FTEF-17.14: in a member function's body a field named bare is `this.field`, the
        // value it holds (`QCC_PR_ParseRefValue`, `pr_classtype && expandmemberfields`).
        let ty = match ty {
            Type::Field(held) if self.env.in_class() => held.as_ref().clone(),
            ty => ty,
        };
        Value {
            array,
            constness,
            ..Value::of(ty)
        }
    }

    /// A binary operator: the opcode `QCC_PR_ChooseOpcode` picks for its operands gives its type
    /// (FTEF-23.4).
    fn binary(&mut self, binary: &ast::BinaryExpr) -> Checked {
        let lhs = self.expr_of(binary.lhs())?;
        let rhs = self.expr_of(binary.rhs())?;
        Ok(self
            .operator(binary, &lhs, &rhs)?
            .unwrap_or_else(Value::unknown))
    }

    /// The value of the binary operator `binary` of `lhs` and `rhs`; `None` when it is not
    /// followed: an assignment, an operand of a type the model does not follow, or an array.
    fn operator(
        &mut self,
        binary: &ast::BinaryExpr,
        lhs: &Value,
        rhs: &Value,
    ) -> Result<Option<Value>, Fatal> {
        let Some(op) = binary.op() else {
            return Ok(None);
        };
        let Some(spelling) = binary_spelling(op) else {
            return Ok(None);
        };
        // An array named bare is its head, a pointer (`REF_ARRAYHEAD`), which is not followed.
        if !self.env.fteqcc()
            || lhs.ty == Type::Unknown
            || rhs.ty == Type::Unknown
            || lhs.array
            || rhs.array
        {
            return Ok(None);
        }
        let (Some(left), Some(right)) = (self.operand(lhs, rhs), self.operand(rhs, lhs)) else {
            return Ok(None);
        };
        let in_class = self.env.in_class();
        let Some(choice) = choose_opcode(&candidates(spelling), left, right, in_class) else {
            return Ok(None);
        };
        let Some(opcode) = row(choice.op()) else {
            return Ok(None);
        };
        let span = binary
            .op_token()
            .map_or_else(|| binary.syntax().span(), |t| t.span());
        if let Choice::Mismatch(_) = choice {
            self.mismatch(spelling, lhs, rhs, span);
        }
        let [a, b, c] = opcode.types;
        for (value, operand, row) in [(lhs, left, a), (rhs, right, b)] {
            self.convert(value, operand, row, span)?;
        }
        // A constant converted to the row's type is folded by FTEQCC's conversion, which the
        // model does not follow: its value no longer decides anything (FTEF-26.5).
        let constness = |value: &Value, operand: Operand, row: OpType| {
            if row.etype() == operand.etype || value.constness == Constness::Variable {
                value.constness
            } else {
                Constness::Opaque
            }
        };
        let operands = [constness(lhs, left, a), constness(rhs, right, b)];
        Ok(Some(self.fold(choice.op(), lhs, rhs, operands, c, span)))
    }

    /// The value of the opcode `op` of `lhs` and `rhs`, folded as `QCC_PR_StatementFlags`
    /// folds its constant operands (FTEF-26), with the warnings folding gives at `span`;
    /// `result` is the opcode's result type when nothing is folded.
    fn fold(
        &mut self,
        op: Op,
        lhs: &Value,
        rhs: &Value,
        operands: [Constness; 2],
        result: OpType,
        span: Span,
    ) -> Value {
        let known = |constness: Constness| match constness {
            Constness::Known(constant) => Some(constant),
            Constness::Variable | Constness::Opaque => None,
        };
        if operands.contains(&Constness::Opaque) {
            return Value {
                constness: Constness::Opaque,
                ..Value::of(result.value_type())
            };
        }
        let mut notices = Vec::new();
        let folded = fold(op, known(operands[0]), known(operands[1]), &mut notices);
        for notice in notices {
            self.env.report(notice.code, span, notice.message);
        }
        match folded {
            Some(Folded::Constant(constant)) => Value::constant(constant),
            Some(Folded::Left) => lhs.clone(),
            Some(Folded::Right) => rhs.clone(),
            None => Value::of(result.value_type()),
        }
    }

    /// Converts the operand `value` to the opcode's type `row` for it, if it is another, with
    /// the warning "Implicit conversion" when the conversion is costly (FTEF-23.5) and the
    /// implicit cast's own diagnostics; a refused cast is a fatal error.
    fn convert(
        &mut self,
        value: &Value,
        operand: Operand,
        row: OpType,
        span: Span,
    ) -> Result<(), Fatal> {
        if row.etype() == operand.etype || row == OpType::Variant {
            return Ok(());
        }
        let costly = operand
            .cost_to(row.etype(), self.env.in_class())
            .is_some_and(|cost| cost >= 100);
        if costly {
            let message = format!(
                "Implicit conversion from {} to {}",
                value.ty.fteqcc_name(),
                row.name()
            );
            self.env.report(Code::W0232, span, message);
        }
        if self.env.cast(value.source(), &row.value_type(), span) {
            return Err(Fatal);
        }
        Ok(())
    }

    /// The operand `value` as `QCC_PR_ChooseOpcode` reads it beside `other`: a truth value as
    /// its underlying type, and an enum beside a value of the same enum type as the enum's
    /// base (FTEF-23.4).
    fn operand(&self, value: &Value, other: &Value) -> Option<Operand> {
        let shape = self.env.shape(&value.ty)?;
        let etype = if shape.etype == Etype::Enum && value.ty == other.ty {
            shape.holds.unwrap_or(Etype::Enum)
        } else {
            shape.etype
        };
        Some(Operand {
            etype,
            holds: shape.holds,
            null: value.null(),
        })
    }

    /// "type mismatch for OP (A and B)", a warning with `lax` on (FTEF-23.4).
    fn mismatch(&mut self, spelling: &str, lhs: &Value, rhs: &Value, span: Span) {
        let lax = self.env.flags_at(span.start).has(FteFlag::Laxcasts);
        let code = if lax { Code::W0231 } else { Code::E0302 };
        let message = format!(
            "type mismatch for {spelling} ({} and {})",
            self.env.type_name(&lhs.ty),
            self.env.type_name(&rhs.ty)
        );
        self.env.report(code, span, message);
    }
}

/// The spelling of a binary operator `QCC_PR_RefExpression` looks its opcodes up by; `None`
/// for those it reads otherwise: the assignments and `,`.
const fn binary_spelling(op: BinaryOp) -> Option<&'static str> {
    Some(match op {
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitOr => "|",
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Eq => "==",
        BinaryOp::Ne => "!=",
        BinaryOp::Le => "<=",
        BinaryOp::Ge => ">=",
        BinaryOp::Lt => "<",
        BinaryOp::Gt => ">",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
        BinaryOp::Rem => "%",
        BinaryOp::BitXor => "^",
        BinaryOp::Shl => "<<",
        BinaryOp::Shr => ">>",
        BinaryOp::Pow => "*^",
        BinaryOp::Cross => "><",
        BinaryOp::Spaceship => "<=>",
        BinaryOp::Assign
        | BinaryOp::AddAssign
        | BinaryOp::SubAssign
        | BinaryOp::MulAssign
        | BinaryOp::DivAssign
        | BinaryOp::BitAndAssign
        | BinaryOp::BitOrAssign
        | BinaryOp::BitClearAssign
        | BinaryOp::BitXorAssign
        | BinaryOp::ShlAssign
        | BinaryOp::ShrAssign
        | BinaryOp::Comma => return None,
    })
}
