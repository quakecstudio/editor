// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/pr_comp.c, PR_Expression, PR_Term, PR_ParseFunctionCall, PR_ParseStatement

//! The types of a body's expressions as id's `qcc` checks them (QC-16, QC-18): each operator
//! takes the operand types of a row of `pr_opcodes` exactly, a call its parameters' types
//! exactly; any other is an error that ends the statement (`PR_ParseError`).

use std::collections::HashMap;

use qc_diag::{Code, Span};
use qc_syntax::ast::{self, AstNode as _, BinaryOp, LiteralKind};

use crate::check::expr::Fatal;
use crate::check::{Env, TypedBody};
use crate::ty::{SimpleType, Type};

/// A basic type as `pr_opcodes` names its operands' (`etype_t`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Basic {
    Void,
    String,
    Float,
    Vector,
    Entity,
    Field,
    Function,
}

impl Basic {
    /// The basic type of `ty`, `None` for a type the model does not follow.
    fn of(ty: &Type) -> Option<Self> {
        Some(match ty {
            Type::Simple(SimpleType::Void) => Self::Void,
            Type::Simple(SimpleType::String) => Self::String,
            Type::Simple(SimpleType::Float) => Self::Float,
            Type::Simple(SimpleType::Vector) => Self::Vector,
            Type::Simple(SimpleType::Entity) => Self::Entity,
            Type::Field(_) => Self::Field,
            Type::Function(_) => Self::Function,
            Type::Pointer(_) | Type::Named(_) | Type::Unknown => return None,
        })
    }
}

/// The rows of `pr_opcodes` an operator of `spelling` has, each its operand types and its
/// result's, in the table's order.
fn rows(spelling: &str) -> &'static [(Basic, Basic, Basic)] {
    use Basic::{Entity, Float, Function, String, Vector};
    match spelling {
        "*" => &[
            (Float, Float, Float),
            (Vector, Vector, Float),
            (Float, Vector, Vector),
            (Vector, Float, Vector),
        ],
        "/" | "<=" | ">=" | "<" | ">" | "&&" | "||" | "&" | "|" => &[(Float, Float, Float)],
        "+" | "-" => &[(Float, Float, Float), (Vector, Vector, Vector)],
        "==" | "!=" => &[
            (Float, Float, Float),
            (Vector, Vector, Float),
            (String, String, Float),
            (Entity, Entity, Float),
            (Function, Function, Float),
        ],
        _ => &[],
    }
}

/// The spelling `pr_opcodes` gives a binary operator, `None` for one vanilla has no row of.
const fn spelling(op: BinaryOp) -> Option<&'static str> {
    Some(match op {
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
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
        BinaryOp::BitAnd => "&",
        BinaryOp::BitOr => "|",
        _ => return None,
    })
}

/// The type of an expression, `None` where an earlier error or a name with no definition left
/// none, on which nothing more is checked.
type Typed = Result<Option<Type>, Fatal>;

/// The checker of one body under id's rules.
struct Vanilla<'e> {
    env: &'e mut dyn Env,
    types: HashMap<(usize, usize), Type>,
}

/// QC-16, QC-18: checks `body` as id's `qcc` does, reporting through `env`, and returns its
/// types.
pub(super) fn check_body(env: &mut dyn Env, body: &ast::FunctionBody) -> TypedBody {
    let mut checker = Vanilla {
        env,
        types: HashMap::new(),
    };
    if let Some(block) = body.block() {
        checker.block(&block);
    }
    TypedBody {
        types: checker.types,
    }
}

impl Vanilla<'_> {
    fn block(&mut self, block: &ast::Block) {
        for stmt in block.statements() {
            // A `PR_ParseError` ends the statement.
            let _ended: Result<(), Fatal> = self.statement(&stmt);
        }
    }

    fn statement_of(&mut self, stmt: Option<ast::Stmt>) -> Result<(), Fatal> {
        match stmt {
            Some(stmt) => self.statement(&stmt),
            None => Ok(()),
        }
    }

    /// `PR_ParseStatement`: the expressions of a statement are typed; a condition is any type.
    fn statement(&mut self, stmt: &ast::Stmt) -> Result<(), Fatal> {
        match stmt {
            ast::Stmt::Block(block) => self.block(block),
            ast::Stmt::Expr(s) => {
                self.expr_of(s.expr())?;
            }
            ast::Stmt::Return(s) => {
                self.expr_of(s.value())?;
            }
            ast::Stmt::If(s) => {
                self.expr_of(s.condition())?;
                let _then: Result<(), Fatal> = self.statement_of(s.then_branch());
                let _else: Result<(), Fatal> = self.statement_of(s.else_branch());
            }
            ast::Stmt::While(s) => {
                self.expr_of(s.condition())?;
                let _body: Result<(), Fatal> = self.statement_of(s.body());
            }
            ast::Stmt::Do(s) => {
                let _body: Result<(), Fatal> = self.statement_of(s.body());
                self.expr_of(s.condition())?;
            }
            _ => {}
        }
        Ok(())
    }

    fn expr_of(&mut self, expr: Option<ast::Expr>) -> Typed {
        match expr {
            Some(expr) => self.expr(&expr),
            None => Ok(None),
        }
    }

    /// The type of `expr`, recorded.
    fn expr(&mut self, expr: &ast::Expr) -> Typed {
        let ty = self.term(expr)?;
        if let Some(ty) = &ty {
            let span = expr.syntax().span();
            self.types.insert((span.start, span.end), ty.clone());
        }
        Ok(ty)
    }

    fn term(&mut self, expr: &ast::Expr) -> Typed {
        match expr {
            ast::Expr::Literal(literal) => Ok(match literal.kind() {
                Some(LiteralKind::String) => Some(Type::Simple(SimpleType::String)),
                Some(LiteralKind::Vector) => Some(Type::Simple(SimpleType::Vector)),
                Some(LiteralKind::Float | LiteralKind::FrameRef) => {
                    Some(Type::Simple(SimpleType::Float))
                }
                None => None,
            }),
            ast::Expr::Name(name) => Ok(name
                .name()
                .and_then(|token| self.env.reference(token.span().start))
                .and_then(|def| self.env.definition(def))
                .map(|def| def.ty.clone())),
            ast::Expr::Paren(paren) => self.expr_of(paren.expr()),
            ast::Expr::Not(not) => self.not(not),
            ast::Expr::Binary(binary) => self.binary(binary),
            ast::Expr::Field(field) => self.field(field),
            ast::Expr::Call(call) => self.call(call),
            _ => {
                for child in expr.syntax().children().filter_map(ast::Expr::cast) {
                    self.expr(&child)?;
                }
                Ok(None)
            }
        }
    }

    /// The fatal error "type mismatch for OP" (QCD-4.2) at `span`.
    fn mismatch<T>(&mut self, op: &str, span: Span) -> Result<T, Fatal> {
        self.env
            .report(Code::E0302, span, format!("type mismatch for {op}"));
        Err(Fatal)
    }

    /// QC-16.4: `!` of a float, vector, string, entity or function is a float.
    fn not(&mut self, not: &ast::NotExpr) -> Typed {
        let Some(ty) = self.expr_of(not.operand())? else {
            return Ok(None);
        };
        match Basic::of(&ty) {
            Some(
                Basic::Float | Basic::String | Basic::Entity | Basic::Vector | Basic::Function,
            ) => Ok(Some(Type::Simple(SimpleType::Float))),
            _ => self.mismatch("!", not.syntax().span()),
        }
    }

    /// QC-16.1, QC-16.2, QC-17: a binary operator's row by its operands' basic types, an
    /// assignment's by the target's and the value's, the same.
    fn binary(&mut self, binary: &ast::BinaryExpr) -> Typed {
        let span = binary
            .op_token()
            .map_or_else(|| binary.syntax().span(), |t| t.span());
        let Some(op) = binary.op() else {
            return Ok(None);
        };
        let lhs = self.expr_of(binary.lhs())?;
        let rhs = self.expr_of(binary.rhs())?;
        let (Some(lhs), Some(rhs)) = (lhs, rhs) else {
            return Ok(None);
        };
        let (Some(a), Some(b)) = (Basic::of(&lhs), Basic::of(&rhs)) else {
            return Ok(None);
        };
        if op == BinaryOp::Assign {
            // `STORE_*` and `STOREP_*` take a value of the target's type, one of their six.
            let stored = matches!(
                a,
                Basic::Float
                    | Basic::Vector
                    | Basic::String
                    | Basic::Entity
                    | Basic::Field
                    | Basic::Function
            );
            if a != b || !stored {
                return self.mismatch("=", span);
            }
            return Ok(Some(rhs));
        }
        let Some(spelling) = spelling(op) else {
            return Ok(None);
        };
        match rows(spelling).iter().find(|&&(x, y, _)| x == a && y == b) {
            Some(&(_, _, result)) => Ok(Some(match result {
                Basic::Vector => Type::Simple(SimpleType::Vector),
                _ => Type::Simple(SimpleType::Float),
            })),
            None => self.mismatch(spelling, span),
        }
    }

    /// QC-16.1: `e.f`, an entity and a field of a float, vector, string, entity, field or
    /// function, is what the field holds.
    fn field(&mut self, field: &ast::FieldExpr) -> Typed {
        let span = field.syntax().span();
        let base = self.expr_of(field.base())?;
        let selector = self.expr_of(field.field())?;
        let (Some(base), Some(selector)) = (base, selector) else {
            return Ok(None);
        };
        match (Basic::of(&base), &selector) {
            (Some(Basic::Entity), Type::Field(held))
                if Basic::of(held).is_some_and(|b| b != Basic::Void) =>
            {
                Ok(Some(held.as_ref().clone()))
            }
            // QC-16.8: a `.void` field's `type_c` is `ev_void`, which the row loop does not
            // compare, so the first `.` row is taken, a float's (`PR_Expression`).
            (Some(Basic::Entity), Type::Field(held)) if Basic::of(held) == Some(Basic::Void) => {
                Ok(Some(Type::Simple(SimpleType::Float)))
            }
            (None, _) => Ok(None),
            _ => self.mismatch(".", span),
        }
    }

    /// QC-18: `f(args)`: `f` a function, else "not a function"; with any arguments, one past a
    /// fixed list is "too many parameters" before it is read, each of exactly its parameter's
    /// type, else "type mismatch on parm N", and fewer than the list "too few parameters";
    /// nine or more "More than eight parameters". With none, no count is checked.
    fn call(&mut self, call: &ast::CallExpr) -> Typed {
        let span = call.syntax().span();
        let Some(callee) = self.expr_of(call.callee())? else {
            for arg in call
                .args()
                .into_iter()
                .flat_map(|a| a.args().collect::<Vec<_>>())
            {
                self.expr(&arg)?;
            }
            return Ok(None);
        };
        let Type::Function(function) = &callee else {
            self.env
                .report(Code::E0303, span, "not a function".to_owned());
            return Err(Fatal);
        };
        let args: Vec<ast::Expr> = call.args().map(|a| a.args().collect()).unwrap_or_default();
        let fixed = !function.variadic;
        for (at, arg) in args.iter().enumerate() {
            if fixed && at >= function.params.len() {
                self.env
                    .report(Code::E0304, span, "too many parameters".to_owned());
                return Err(Fatal);
            }
            let ty = self.expr(arg)?;
            if fixed
                && let (Some(ty), Some(param)) = (ty, function.params.get(at))
                && (ty != *param || param.holds_variadic())
            {
                let message = format!("type mismatch on parm {at}");
                self.env.report(Code::E0306, arg.syntax().span(), message);
                return Err(Fatal);
            }
        }
        if fixed && !args.is_empty() && args.len() != function.params.len() {
            self.env
                .report(Code::E0305, span, "too few parameters".to_owned());
            return Err(Fatal);
        }
        if args.len() > 8 {
            self.env
                .report(Code::E0307, span, "More than eight parameters".to_owned());
            return Err(Fatal);
        }
        Ok(Some(function.ret.clone()))
    }
}
