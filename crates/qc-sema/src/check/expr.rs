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

use crate::cast::{ConstType, Literal};
use crate::check::convert::{Choice, Operand, choose_opcode, conversion_cost};
use crate::check::etype::Etype;
use crate::check::opcodes::{OPCODES, OpType, candidates};
use crate::check::{Env, TypedBody, Value};
use crate::ty::{SimpleType, Type};

/// The checker of one body.
pub(super) struct Checker<'e> {
    env: &'e mut dyn Env,
    types: HashMap<(usize, usize), Type>,
    /// Whether a fatal error ended the statement being checked: FTEQCC's `QCC_PR_ParseError`,
    /// after which nothing more of the statement is checked (FTEF-23.6).
    aborted: bool,
}

impl<'e> Checker<'e> {
    pub(super) fn new(env: &'e mut dyn Env) -> Self {
        Self {
            env,
            types: HashMap::new(),
            aborted: false,
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

    fn statement(&mut self, stmt: &ast::Stmt) {
        self.aborted = false;
        match stmt {
            ast::Stmt::Block(block) => self.block(block),
            ast::Stmt::Expr(s) => self.value_of(s.expr()),
            ast::Stmt::Return(s) => self.value_of(s.value()),
            ast::Stmt::If(s) => {
                self.value_of(s.condition());
                self.statement_of(s.then_branch());
                self.statement_of(s.else_branch());
            }
            ast::Stmt::While(s) => {
                self.value_of(s.condition());
                self.statement_of(s.body());
            }
            ast::Stmt::Do(s) => {
                self.statement_of(s.body());
                self.value_of(s.condition());
            }
            ast::Stmt::Loop(s) => self.statement_of(s.body()),
            ast::Stmt::For(s) => {
                for clause in s.clauses() {
                    for expr in clause.children().filter_map(ast::Expr::cast) {
                        self.expr(&expr);
                    }
                }
                self.statement_of(s.body());
            }
            ast::Stmt::Switch(s) => {
                self.value_of(s.scrutinee());
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
    }

    fn statement_of(&mut self, stmt: Option<ast::Stmt>) {
        if let Some(stmt) = stmt {
            self.statement(&stmt);
        }
    }

    fn value_of(&mut self, expr: Option<ast::Expr>) {
        if let Some(expr) = expr {
            self.expr(&expr);
        }
    }

    /// The value of `expr`, its type recorded.
    fn expr(&mut self, expr: &ast::Expr) -> Value {
        if self.aborted {
            return Value::unknown();
        }
        let value = match expr {
            ast::Expr::Literal(literal) => self.literal(literal),
            ast::Expr::Name(name) => self.name(name),
            ast::Expr::Paren(paren) => paren
                .expr()
                .map_or_else(Value::unknown, |inner| self.expr(&inner)),
            ast::Expr::Binary(binary) => self.binary(binary),
            ast::Expr::Not(_)
            | ast::Expr::Field(_)
            | ast::Expr::Call(_)
            | ast::Expr::Postfix(_)
            | ast::Expr::Ternary(_)
            | ast::Expr::Index(_)
            | ast::Expr::Unary(_)
            | ast::Expr::Cast(_)
            | ast::Expr::Vector(_)
            | ast::Expr::Sizeof(_) => {
                self.children(expr);
                Value::unknown()
            }
        };
        let span = expr.syntax().span();
        self.types.insert((span.start, span.end), value.ty.clone());
        value
    }

    /// Checks the expressions inside `expr`, a term the checker does not type yet.
    fn children(&mut self, expr: &ast::Expr) {
        for child in expr.syntax().children().filter_map(ast::Expr::cast) {
            self.expr(&child);
        }
    }

    fn literal(&mut self, literal: &ast::Literal) -> Value {
        let Some(token) = literal.token() else {
            return Value::unknown();
        };
        match literal.kind() {
            Some(ast::LiteralKind::String) => Value {
                ty: Type::Simple(SimpleType::String),
                literal: Some(Literal::of(ConstType::String)),
            },
            Some(ast::LiteralKind::Vector) => Value {
                ty: Type::Simple(SimpleType::Vector),
                literal: Some(Literal::of(ConstType::Vector)),
            },
            Some(ast::LiteralKind::FrameRef) => Value::of(Type::Simple(SimpleType::Float)),
            Some(ast::LiteralKind::Float) if self.env.fteqcc() => {
                let flags = self.env.lex_flags_at(token.span().start);
                let number = Literal::of_fteqcc_number(&token.text_lossless(), flags);
                Value {
                    ty: const_type(number.ty),
                    literal: Some(number),
                }
            }
            Some(ast::LiteralKind::Float) => Value::of(Type::Simple(SimpleType::Float)),
            None => Value::unknown(),
        }
    }

    fn name(&mut self, name: &ast::NameRef) -> Value {
        let Some(token) = name.name() else {
            return Value::unknown();
        };
        let Some(ty) = self
            .env
            .reference(token.span().start)
            .and_then(|def| self.env.definition(def))
            .map(|def| def.ty.clone())
        else {
            return Value::unknown();
        };
        // FTEF-17.14: in a member function's body a field named bare is `this.field`, the
        // value it holds (`QCC_PR_ParseRefValue`, `pr_classtype && expandmemberfields`).
        match ty {
            Type::Field(held) if self.env.in_class() => Value::of(held.as_ref().clone()),
            ty => Value::of(ty),
        }
    }

    /// A binary operator: the opcode `QCC_PR_ChooseOpcode` picks for its operands gives its type
    /// (FTEF-23.4).
    fn binary(&mut self, binary: &ast::BinaryExpr) -> Value {
        let lhs = binary.lhs().map_or_else(Value::unknown, |e| self.expr(&e));
        let rhs = binary.rhs().map_or_else(Value::unknown, |e| self.expr(&e));
        let Some(op) = binary.op() else {
            return Value::unknown();
        };
        let Some(spelling) = binary_spelling(op) else {
            return Value::unknown();
        };
        if !self.env.fteqcc() || lhs.ty == Type::Unknown || rhs.ty == Type::Unknown {
            return Value::unknown();
        }
        let (Some(left), Some(right)) = (self.operand(&lhs, &rhs), self.operand(&rhs, &lhs)) else {
            return Value::unknown();
        };
        let in_class = self.env.in_class();
        let Some(choice) = choose_opcode(&candidates(spelling), left, right, in_class) else {
            return Value::unknown();
        };
        let Some(opcode) = OPCODES.get(choice.index()) else {
            return Value::unknown();
        };
        let span = binary
            .op_token()
            .map_or_else(|| binary.syntax().span(), |t| t.span());
        if let Choice::Mismatch(_) = choice {
            self.mismatch(spelling, &lhs, &rhs, span);
        }
        let [a, b, c] = opcode.types;
        for (value, operand, row) in [(&lhs, left, a), (&rhs, right, b)] {
            if !self.convert(value, operand, row, span) {
                self.aborted = true;
                return Value::unknown();
            }
        }
        Value::of(result_type(c))
    }

    /// Converts the operand `value` to the opcode's type `row` for it, if it is another, with
    /// the warning "Implicit conversion" when the conversion is costly (FTEF-23.5) and the
    /// implicit cast's own diagnostics; `false` when the cast is refused, a fatal error.
    fn convert(&mut self, value: &Value, operand: Operand, row: OpType, span: Span) -> bool {
        if row.etype() == operand.etype || row == OpType::Variant {
            return true;
        }
        let costly = conversion_cost(operand, row.etype(), self.env.in_class())
            .is_some_and(|cost| cost >= 100);
        if costly {
            let message = format!(
                "Implicit conversion from {} to {}",
                value.ty.fteqcc_name(),
                row.name()
            );
            self.env.report(Code::W0232, span, message);
        }
        !self.env.cast(value.source(), &result_type(row), span)
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
            lhs.ty.fteqcc_name(),
            rhs.ty.fteqcc_name()
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

/// The type a literal of `ty` has.
fn const_type(ty: ConstType) -> Type {
    match ty {
        ConstType::Float => Type::Simple(SimpleType::Float),
        ConstType::Vector => Type::Simple(SimpleType::Vector),
        ConstType::String => Type::Simple(SimpleType::String),
        ConstType::Integer => Type::Named("__int32".into()),
        ConstType::Uint => Type::Named("__uint32".into()),
        ConstType::Int64 => Type::Named("__int64".into()),
        ConstType::Uint64 => Type::Named("__uint64".into()),
        ConstType::Double => Type::Named("__double".into()),
    }
}

/// The type an opcode's `type_c` gives its result.
fn result_type(ty: OpType) -> Type {
    match ty {
        OpType::None | OpType::Void => Type::Simple(SimpleType::Void),
        OpType::Float | OpType::BoolFloat => Type::Simple(SimpleType::Float),
        OpType::Vector => Type::Simple(SimpleType::Vector),
        OpType::String => Type::Simple(SimpleType::String),
        OpType::Entity => Type::Simple(SimpleType::Entity),
        OpType::Integer | OpType::BoolInt => Type::Named("__int32".into()),
        OpType::Uint => Type::Named("__uint32".into()),
        OpType::Int64 => Type::Named("__int64".into()),
        OpType::Uint64 => Type::Named("__uint64".into()),
        OpType::Double => Type::Named("__double".into()),
        OpType::Variant => Type::Named("__variant".into()),
        OpType::Field | OpType::Function => Type::Unknown,
        OpType::Pointer => Type::Pointer(Type::Simple(SimpleType::Float).into()),
        OpType::FloatPointer => Type::Pointer(Type::Simple(SimpleType::Float).into()),
        OpType::IntPointer => Type::Pointer(Type::Named("__int32".into()).into()),
    }
}
