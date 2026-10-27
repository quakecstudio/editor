// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_RefExpression, QCC_PR_ChooseOpcode

//! The type of each expression of a body: terms and operators (FTEF-23).

use std::collections::{HashMap, HashSet};

use qc_diag::{Code, Span};
use qc_preproc::FteFlag;
use qc_syntax::ast::{self, AstNode as _, BinaryOp};

use crate::check::assign::assign_spelling;
use crate::check::constant::{Constant, unquoted};
use crate::check::convert::{Choice, Operand, choose_opcode};
use crate::check::emulate::conversion_opcodes;
use crate::check::etype::Etype;
use crate::check::opcodes::{OpType, candidates, row};

use crate::check::flow::Layout;
use crate::check::{Constness, Env, FunctionFacts, Reference, Sym, TypedBody, Value};
use crate::symbol::{DefId, DefKind};
use crate::ty::{SimpleType, Type};

/// The checker of one body.
pub(super) struct Checker<'e> {
    pub(super) env: &'e mut dyn Env,
    types: HashMap<(usize, usize), Type>,
    /// Whether the checker is in a condition of `if`, `while` or `do`, outside parentheses
    /// (FTEQCC's `conditional`).
    pub(super) in_condition: bool,
    /// The locals whose address was taken, each warned of once (`addressedwarned`).
    pub(super) addressed: HashSet<DefId>,
    /// The function whose body it is.
    pub(super) function: FunctionFacts,
    /// Whether `return =` or a bare `return;` made the function its return value
    /// (`pr_scope->returndef`).
    pub(super) returned: bool,
    /// The statements generated so far, as far as the checks of the finished function read them.
    pub(super) layout: Layout,
    /// The statements emitted so far, `numstatements`, which a `case` value may not change.
    pub(super) emitted: usize,
    /// Whether the checker reads an initialiser at global scope, where no statement may be
    /// generated (`!pr_scope`, FTEF-34.9).
    pub(super) global: bool,
    /// A token the statement does not take, found where an expression ended early, reported
    /// once the statement has read the expression it has (FTEF-24.10).
    pub(super) pending: Option<(String, qc_diag::Span)>,
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
            function: env.function(),
            env,
            types: HashMap::new(),
            in_condition: false,
            addressed: HashSet::new(),
            returned: false,
            layout: Layout::default(),
            emitted: 0,
            global: false,
            pending: None,
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

    /// The value of `expr`, if it is there.
    pub(super) fn expr_of(&mut self, expr: Option<ast::Expr>) -> Checked {
        expr.map_or_else(|| Ok(Value::unknown()), |e| self.expr(&e))
    }

    /// The value of `expr`, its type recorded.
    pub(super) fn expr(&mut self, expr: &ast::Expr) -> Checked {
        let value = self.term(expr)?;
        let span = expr.syntax().span();
        self.types.insert((span.start, span.end), value.ty.clone());
        Ok(value)
    }

    /// The value of `expr` by its kind. Each kind is its own method, so that the frame a nested
    /// expression adds stays small (QC-27.16).
    fn term(&mut self, expr: &ast::Expr) -> Checked {
        match expr {
            ast::Expr::Literal(literal) => Ok(self.literal(literal)),
            ast::Expr::Name(name) => Ok(self.name(name)),
            ast::Expr::Paren(paren) => self.paren(paren),
            ast::Expr::Binary(binary) => match binary.op() {
                Some(op) if assign_spelling(op).is_some() => self.assign(binary, op),
                Some(_) | None => self.binary(binary),
            },
            ast::Expr::Postfix(postfix) => self.postfix(postfix),
            ast::Expr::Field(field) => self.member(field),
            ast::Expr::Not(not) => self.not(not),
            ast::Expr::Ternary(ternary) => self.ternary(ternary),
            ast::Expr::Unary(unary) => self.unary(unary),
            ast::Expr::Cast(cast) => self.cast(cast),
            ast::Expr::Vector(vector) => self.vector_expr(vector),
            ast::Expr::Sizeof(sizeof) => self.sizeof(sizeof),
            ast::Expr::Call(call) if self.is_length(call) => {
                let operand = call.args().and_then(|args| args.args().next());
                self.length(operand, call.syntax().span())
            }
            ast::Expr::Index(index) => self.index(index),
            ast::Expr::Call(call) => self.call(call),
        }
    }

    /// `(e)`: the value inside, which is no condition's whole.
    fn paren(&mut self, paren: &ast::ParenExpr) -> Checked {
        let in_condition = std::mem::replace(&mut self.in_condition, false);
        let value = self.expr_of(paren.expr());
        self.in_condition = in_condition;
        let value = value?;
        self.end_of_construct()?;
        Ok(value)
    }

    /// FTEF-24.10: the end of a construct that expects its own token, `)` or `]`: a token an
    /// expression in it ended before is the fatal error "expected TOKEN, found X".
    pub(super) fn end_of_construct(&mut self) -> Result<(), Fatal> {
        match self.pending.take() {
            Some((message, span)) => {
                self.env.report(Code::E0204, span, message);
                Err(Fatal)
            }
            None => Ok(()),
        }
    }

    /// Whether `call` is the intrinsic `_length(e)`: its callee the name `_length` with no
    /// definition (FTEF-30.8).
    fn is_length(&self, call: &ast::CallExpr) -> bool {
        let Some(ast::Expr::Name(name)) = call.callee() else {
            return false;
        };
        name.name().is_some_and(|token| {
            token.text_lossless() == "_length" && self.env.reference(token.span().start).is_none()
        })
    }

    fn literal(&mut self, literal: &ast::Literal) -> Value {
        let Some(token) = literal.token() else {
            return Value::unknown();
        };
        match literal.kind() {
            Some(ast::LiteralKind::String) => {
                let written = String::from_utf8_lossy(&literal.syntax().text()).into_owned();
                let text = unquoted(&written);
                Value {
                    text: Some(text.as_str().into()),
                    ..Value::constant(Constant::string(text.is_empty()))
                }
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
        // `::name` and `Scope::name` both hold a `::`.
        let scoped = name
            .syntax()
            .child_tokens()
            .any(|t| t.kind() == qc_syntax::SyntaxKind::ColonColon);
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
        let name = token.text_lossless();
        // FTEF-17.14: in a member function's body a field named bare is `this.field`, the
        // value it holds (`QCC_PR_ParseRefValue`, `pr_classtype && expandmemberfields`), and so
        // is a class's member field, which holds its own type.
        let member_field = self
            .env
            .definition(def)
            .is_some_and(|d| d.member_of.is_some() && d.kind == DefKind::Field);
        let held = match &ty {
            Type::Field(held) => Some(held.as_ref().clone()),
            _ if member_field => Some(ty.clone()),
            _ => None,
        };
        // `::name` and `this::name` keep the field itself, for the `find` builtin and the like
        // (`expandmemberfields = false;`).
        if let Some(held) = held
            && self.env.in_class()
            && !scoped
        {
            let this = Value {
                name: Some("this".to_owned()),
                ..Value::unknown()
            };
            return Value::field_of(&this, name, held, Some(def));
        }
        Value {
            reference: if array {
                Reference::ArrayHead
            } else {
                Reference::Global
            },
            constness,
            name: Some(name),
            // A function's definition is no constant symbol to the comparison warnings: `ff != vf`
            // of two functions gives none (FTEF-28.1).
            sym: Sym::Def(def, constness != Constness::Variable && !ty.is_function()),
            // `QCC_DefToRef`: `!!def.sym->constant || def.sym->temp`.
            readonly: self.env.constant(def),
            text: self.env.string_text(def).map(|t| t.as_str().into()),
            ..Value::of(ty)
        }
    }

    /// A binary operator: the opcode `QCC_PR_ChooseOpcode` picks for its operands gives its type
    /// (FTEF-23.4).
    fn binary(&mut self, binary: &ast::BinaryExpr) -> Checked {
        let lhs = self.value(binary.lhs())?;
        let rhs = self.value(binary.rhs())?;
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
            || lhs.is_array_head()
            || rhs.is_array_head()
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
        let [a, b, _] = opcode.types;
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
        // A converted operand is a temporary the conversion made.
        let converted = |value: &Value, operand: Operand, row: OpType| Value {
            constness: constness(value, operand, row),
            sym: if row.etype() == operand.etype || row == OpType::Variant {
                value.sym
            } else {
                Sym::Temp
            },
            ..value.clone()
        };
        let lhs = converted(lhs, left, a);
        let rhs = converted(rhs, right, b);
        // The value is what the opcode, its folding or its emulation gives, of that one's type.
        Ok(Some(self.emit(choice.op(), &lhs, &rhs, span)?))
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
        // FTEF-27.8: the cast emits its conversion opcodes, which are folded, emitted or
        // emulated in turn.
        let mut converted = value.clone();
        for &conv in conversion_opcodes(operand.etype, row.etype()) {
            converted = self.emit(conv, &converted, &Value::unknown(), span)?;
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
