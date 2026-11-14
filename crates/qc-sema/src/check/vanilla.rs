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
use qc_preproc::Op;
use qc_syntax::ast::{self, AstNode as _, BinaryOp, LiteralKind};

use crate::check::assign::lexed_string;
use crate::check::call::call_opcode;
use crate::check::expr::Fatal;
use crate::check::{Env, TypedBody};
use crate::ty::{SimpleType, Type};
use crate::{DefId, ir};

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

/// The rows of `pr_opcodes` an operator of `spelling` has, each its operand types, its
/// result's and its opcode, in the table's order.
fn rows(spelling: &str) -> &'static [(Basic, Basic, Basic, Op)] {
    use Basic::{Entity, Float, Function, String, Vector};
    match spelling {
        "*" => &[
            (Float, Float, Float, Op::MulF),
            (Vector, Vector, Float, Op::MulV),
            (Float, Vector, Vector, Op::MulFv),
            (Vector, Float, Vector, Op::MulVf),
        ],
        "/" => &[(Float, Float, Float, Op::DivF)],
        "<=" => &[(Float, Float, Float, Op::LeF)],
        ">=" => &[(Float, Float, Float, Op::GeF)],
        "<" => &[(Float, Float, Float, Op::LtF)],
        ">" => &[(Float, Float, Float, Op::GtF)],
        "&&" => &[(Float, Float, Float, Op::AndF)],
        "||" => &[(Float, Float, Float, Op::OrF)],
        "&" => &[(Float, Float, Float, Op::BitandF)],
        "|" => &[(Float, Float, Float, Op::BitorF)],
        "+" => &[
            (Float, Float, Float, Op::AddF),
            (Vector, Vector, Vector, Op::AddV),
        ],
        "-" => &[
            (Float, Float, Float, Op::SubF),
            (Vector, Vector, Vector, Op::SubV),
        ],
        "==" => &[
            (Float, Float, Float, Op::EqF),
            (Vector, Vector, Float, Op::EqV),
            (String, String, Float, Op::EqS),
            (Entity, Entity, Float, Op::EqE),
            (Function, Function, Float, Op::EqFnc),
        ],
        "!=" => &[
            (Float, Float, Float, Op::NeF),
            (Vector, Vector, Float, Op::NeV),
            (String, String, Float, Op::NeS),
            (Entity, Entity, Float, Op::NeE),
            (Function, Function, Float, Op::NeFnc),
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
    /// The statements the body generates as `qcc` emits them (`TypedBody::generated`).
    generated: Vec<ir::BodyStatement>,
    /// What each expression's value names, by its span.
    names: HashMap<(usize, usize), ir::Name>,
    /// Where each source statement starts, by the next generated statement then.
    origins: Vec<(u32, usize)>,
    /// Each local declared, by the next generated statement then.
    locals: Vec<(u32, DefId)>,
    /// What the body makes without a statement reading it: locals and literals' immediates.
    effects: Vec<(u32, ir::Effect)>,
    /// The bytes of each string literal named.
    strings: Vec<Vec<u8>>,
    /// Whether every statement is in `generated`.
    followed: bool,
    /// The opcode of each binary operator, by its token's span (QC-16.1).
    opcodes: HashMap<(usize, usize), Op>,
    /// Each binary operator's fragment, its one statement (`docs/design/back-end.md`).

    /// The return value's type, `def_ret.type`, which each call sets (QC-18.9).
    returned: Option<Type>,
}

/// QC-16, QC-18: checks `body` as id's `qcc` does, reporting through `env`, and returns its
/// types.
pub(super) fn check_body(env: &mut dyn Env, body: &ast::FunctionBody) -> TypedBody {
    let mut checker = Vanilla {
        env,
        types: HashMap::new(),
        generated: Vec::new(),
        names: HashMap::new(),
        origins: Vec::new(),
        locals: Vec::new(),
        effects: Vec::new(),
        strings: Vec::new(),
        followed: true,
        opcodes: HashMap::new(),
        returned: None,
    };
    // QC-13.6: `STATE` of the frame number and the think function, the body's first.
    if let Some(state) = body.state() {
        let frame = state.frame().and_then(|frame| checker.literal_name(&frame));
        let think = state
            .next()
            .and_then(|next| checker.env.reference(next.span().start))
            .map(|def| ir::Name::Def(def, 0));
        checker.push(Op::State, [frame, think, None]);
    }
    if let Some(block) = body.block() {
        checker.block(&block);
    }
    // QC-13.7: the body ends with `DONE`.
    checker.push(Op::Done, [None, None, None]);
    TypedBody {
        types: checker.types,
        opcodes: checker.opcodes,
        generated: checker.generated,
        strings: checker.strings,
        origins: checker.origins,
        locals: checker.locals,
        start: body.syntax().span().start,
        followed: checker.followed,
        effects: checker.effects,
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
    /// Its statements are `qcc`'s: an `if`'s `IFNOT` past its branch, and a `GOTO` past an
    /// `else`; a `while`'s test, `IFNOT` past the loop and `GOTO` back; a `do`'s `IF` back; a
    /// `return`'s `RETURN` (QC-14.3 to QC-14.6).
    fn statement(&mut self, stmt: &ast::Stmt) -> Result<(), Fatal> {
        let at = self.next();
        self.origins.push((at, stmt.syntax().span().start));
        match stmt {
            ast::Stmt::Block(block) => self.block(block),
            ast::Stmt::Expr(s) => {
                self.expr_of(s.expr())?;
            }
            ast::Stmt::Return(s) => {
                let value = s.value();
                self.expr_of(value.clone())?;
                let named = value.and_then(|value| self.name(&value));
                self.push(Op::Return, [named, None, None]);
            }
            ast::Stmt::If(s) => {
                let condition = s.condition();
                self.expr_of(condition.clone())?;
                let tested = condition.and_then(|c| self.name(&c));
                let test = self.push(Op::IfnotI, [tested, None, None]);
                let _then: Result<(), Fatal> = self.statement_of(s.then_branch());
                if let Some(else_branch) = s.else_branch() {
                    let skip = self.push(Op::Goto, [None, None, None]);
                    self.target(test, 1);
                    let _else: Result<(), Fatal> = self.statement(&else_branch);
                    self.target(skip, 0);
                } else {
                    self.target(test, 1);
                }
            }
            ast::Stmt::While(s) => {
                let top = self.next();
                let condition = s.condition();
                self.expr_of(condition.clone())?;
                let tested = condition.and_then(|c| self.name(&c));
                let test = self.push(Op::IfnotI, [tested, None, None]);
                let _body: Result<(), Fatal> = self.statement_of(s.body());
                self.push(Op::Goto, [Some(ir::Name::Jump(top)), None, None]);
                self.target(test, 1);
            }
            ast::Stmt::Do(s) => {
                let top = self.next();
                let _body: Result<(), Fatal> = self.statement_of(s.body());
                let condition = s.condition();
                self.expr_of(condition.clone())?;
                let tested = condition.and_then(|c| self.name(&c));
                self.push(Op::IfI, [tested, Some(ir::Name::Jump(top)), None]);
            }
            ast::Stmt::Local(local) => self.local(local),
            ast::Stmt::Empty(_) => {}
            _ => self.followed = false,
        }
        Ok(())
    }

    /// A `local` declaration: each local declared where it is (QC-11.10); one with an
    /// initialiser is initialised once, as a global is, which the code generator writes.
    fn local(&mut self, local: &ast::LocalDecl) {
        for declarator in local.declarators() {
            let def = declarator
                .name()
                .and_then(|name| self.env.reference(name.span().start));
            if let Some(def) = def {
                let at = self.next();
                self.locals.push((at, def));
                self.effects
                    .push((at, ir::Effect::Make(ir::Name::Def(def, 0))));
            }
            match (declarator.initializer(), def) {
                (None, _) => {}
                (Some(ast::Initializer::Value(_)), Some(def)) => {
                    let at = self.next();
                    self.effects
                        .push((at, ir::Effect::Constant(ir::Name::Def(def, 0))));
                }
                (Some(_), _) => self.followed = false,
            }
        }
    }

    /// The index the next generated statement takes.
    fn next(&self) -> u32 {
        u32::try_from(self.generated.len()).unwrap_or(u32::MAX)
    }

    /// Generates `op` of the operands; its index.
    fn push(&mut self, op: Op, [a, b, c]: [Option<ir::Name>; 3]) -> u32 {
        let at = self.next();
        self.generated.push(ir::BodyStatement { op, a, b, c });
        at
    }

    /// Generates `op` of `a` and `b` into a new temporary (QC-21.5), which it names.
    fn made(&mut self, op: Op, a: Option<ir::Name>, b: Option<ir::Name>) -> ir::Name {
        let at = self.next();
        let made = ir::Name::Made(at);
        self.push(op, [a, b, Some(made)]);
        made
    }

    /// Points operand `slot` (0 `a`, 1 `b`) of the jump at `jump` at the next statement.
    fn target(&mut self, jump: u32, slot: usize) {
        let next = Some(ir::Name::Jump(self.next()));
        if let Some(statement) = usize::try_from(jump)
            .ok()
            .and_then(|at| self.generated.get_mut(at))
        {
            if slot == 0 {
                statement.a = next;
            } else {
                statement.b = next;
            }
        }
    }

    /// What `expr`'s value names, once it is checked.
    fn name(&self, expr: &ast::Expr) -> Option<ir::Name> {
        let span = expr.syntax().span();
        self.names.get(&(span.start, span.end)).copied()
    }

    /// Records that the value of the expression at `span` is `name`.
    fn name_span(&mut self, span: Span, name: Option<ir::Name>) {
        match name {
            Some(name) => {
                self.names.insert((span.start, span.end), name);
            }
            None => self.followed = false,
        }
    }

    /// QC-17.3: turns the last statement, a `LOAD_*` of the assignment's `target`, into
    /// `ADDRESS`, whether it did; a vector's keeps the three words its value took (QC-17.7).
    fn address_of_last(&mut self, target: Option<&ast::Expr>) -> bool {
        // QC-27.11: only a field on the left is made an address; an earlier statement's load is
        // never rewritten, where id's `qcc` retypes the target (QC-17.4).
        if !target.is_some_and(is_field) {
            return false;
        }
        let Some(last) = self.generated.last_mut() else {
            return false;
        };
        let words = match last.op {
            Op::LoadV => 2,
            Op::LoadF | Op::LoadS | Op::LoadEnt | Op::LoadFld | Op::LoadFnc => 0,
            _ => return false,
        };
        last.op = Op::Address;
        if words > 0 {
            let at = self.next();
            self.effects.push((at, ir::Effect::Temp(words)));
        }
        true
    }

    /// Records that `expr`'s value is `name`.
    fn set_name(&mut self, expr: &ast::Expr, name: Option<ir::Name>) {
        self.name_span(expr.syntax().span(), name);
    }

    /// QC-4, QC-5, QC-6: a literal's immediate as the lexer reads its text, made where it is
    /// read (`PR_ParseImmediate`).
    fn literal_name(&mut self, literal: &ast::Literal) -> Option<ir::Name> {
        let token = literal.token()?;
        let lexed = qc_lexer::lex_with(token.text(), qc_syntax::Dialect::Vanilla);
        let name = match lexed.tokens.as_slice() {
            [lexed] => match &lexed.kind {
                qc_lexer::TokenKind::Float(value) => ir::Name::Immediate(ir::Immediate {
                    kind: ir::ImmediateKind::Float,
                    words: [value.to_bits(), 0, 0],
                }),
                qc_lexer::TokenKind::Vector(value) => ir::Name::Immediate(ir::Immediate {
                    kind: ir::ImmediateKind::Vector,
                    words: value.map(f32::to_bits),
                }),
                qc_lexer::TokenKind::String(_) => {
                    let bytes = lexed_string(token.text())?;
                    let index = u32::try_from(self.strings.len()).ok()?;
                    self.strings.push(bytes);
                    ir::Name::String(ir::StringIndex::new(index))
                }
                _ => return None,
            },
            _ => return None,
        };
        let at = self.next();
        self.effects.push((at, ir::Effect::Make(name)));
        Some(name)
    }

    fn expr_of(&mut self, expr: Option<ast::Expr>) -> Typed {
        match expr {
            Some(expr) => self.expr(&expr),
            None => Ok(None),
        }
    }

    /// The type of `expr`, recorded, and what its value names.
    fn expr(&mut self, expr: &ast::Expr) -> Typed {
        let ty = self.term(expr)?;
        if let Some(ty) = &ty {
            let span = expr.syntax().span();
            self.types.insert((span.start, span.end), ty.clone());
        }
        let named = match expr {
            ast::Expr::Literal(literal) => self.literal_name(literal),
            ast::Expr::Name(name) => name
                .name()
                .and_then(|token| self.env.reference(token.span().start))
                .map(|def| ir::Name::Def(def, 0)),
            ast::Expr::Paren(paren) => paren.expr().and_then(|inner| self.name(&inner)),
            _ => self.name(expr),
        };
        self.set_name(expr, named);
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
        let opcode = match Basic::of(&ty) {
            Some(Basic::Float) => Op::NotF,
            Some(Basic::Vector) => Op::NotV,
            Some(Basic::String) => Op::NotS,
            Some(Basic::Entity) => Op::NotEnt,
            Some(Basic::Function) => Op::NotFnc,
            _ => return self.mismatch("!", not.syntax().span()),
        };
        // QC-16.4: `NOT_*` of the operand into a new temporary.
        let operand = not.operand().and_then(|operand| self.name(&operand));
        let named = self.made(opcode, operand, None);
        self.name_span(not.syntax().span(), Some(named));
        Ok(Some(Type::Simple(SimpleType::Float)))
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
        // QC-17.3: before an assignment's value is read, a `LOAD_*` its target ended with
        // becomes `ADDRESS` of the field, which `STOREP_*` stores through (`PR_Expression`).
        let through = op == BinaryOp::Assign && self.address_of_last(binary.lhs().as_ref());
        let rhs = self.expr_of(binary.rhs())?;
        let named = [binary.lhs(), binary.rhs()].map(|side| side.and_then(|e| self.name(&e)));
        // QC-18.9: a call's value is the one return global, retyped by each call, so an operand
        // that is a call has the type of the last call read, the other operand's included.
        let lhs = if binary.lhs().as_ref().is_some_and(is_call) {
            self.returned.clone()
        } else {
            lhs
        };
        let rhs = if binary.rhs().as_ref().is_some_and(is_call) {
            self.returned.clone()
        } else {
            rhs
        };
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
            // The `STORE_*` of the target's type, which the code generator reads.
            let store = match a {
                Basic::Vector => Op::StoreV,
                Basic::String => Op::StoreS,
                Basic::Entity => Op::StoreEnt,
                Basic::Field => Op::StoreFld,
                Basic::Function => Op::StoreFnc,
                Basic::Float | Basic::Void => Op::StoreF,
            };
            self.opcodes.insert((span.start, span.end), store);
            let [target, value] = named;
            if through {
                self.push(store_through(store), [value, target, None]);
            } else {
                self.push(store, [value, target, None]);
            }
            // QC-17.6: an assignment's value is what it stored (`PR_Statement`).
            self.name_span(binary.syntax().span(), value);
            return Ok(Some(rhs));
        }
        let Some(spelling) = spelling(op) else {
            return Ok(None);
        };
        match rows(spelling)
            .iter()
            .find(|&&(x, y, _, _)| x == a && y == b)
        {
            Some(&(_, _, result, opcode)) => {
                self.opcodes.insert((span.start, span.end), opcode);
                // QC-16.1: the operator is its opcode's one statement of its two operands,
                // into a new temporary.
                let [left, right] = named;
                let value = self.made(opcode, left, right);
                self.name_span(binary.syntax().span(), Some(value));
                Ok(Some(match result {
                    Basic::Vector => Type::Simple(SimpleType::Vector),
                    _ => Type::Simple(SimpleType::Float),
                }))
            }
            None => self.mismatch(spelling, span),
        }
    }

    /// QC-16.1: `e.f`, an entity and a field of a float, vector, string, entity, field or
    /// function, is what the field holds.
    fn field(&mut self, field: &ast::FieldExpr) -> Typed {
        // QCD-4.2: the error is at the `.`, where `PR_Expression` reads the operator.
        let span = field
            .syntax()
            .token_of(qc_syntax::SyntaxKind::Dot)
            .map_or_else(|| field.syntax().span(), |dot| dot.span());
        let base = self.expr_of(field.base())?;
        let selector = self.expr_of(field.field())?;
        let named = [field.base(), field.field()].map(|side| side.and_then(|e| self.name(&e)));
        let (Some(base), Some(selector)) = (base, selector) else {
            return Ok(None);
        };
        let loaded = match (Basic::of(&base), &selector) {
            (Some(Basic::Entity), Type::Field(held))
                if Basic::of(held).is_some_and(|b| b != Basic::Void) =>
            {
                held.as_ref().clone()
            }
            // QC-16.8: a `.void` field's `type_c` is `ev_void`, which the row loop does not
            // compare, so the first `.` row is taken, a float's (`PR_Expression`).
            (Some(Basic::Entity), Type::Field(held)) if Basic::of(held) == Some(Basic::Void) => {
                Type::Simple(SimpleType::Float)
            }
            (None, _) => return Ok(None),
            _ => return self.mismatch(".", span),
        };
        // QC-16.1: the `LOAD_*` of the field's type, of the entity and the field.
        let opcode = match Basic::of(&loaded) {
            Some(Basic::Vector) => Op::LoadV,
            Some(Basic::String) => Op::LoadS,
            Some(Basic::Entity) => Op::LoadEnt,
            Some(Basic::Field) => Op::LoadFld,
            Some(Basic::Function) => Op::LoadFnc,
            Some(Basic::Float | Basic::Void) | None => Op::LoadF,
        };
        let [entity, selected] = named;
        let value = self.made(opcode, entity, selected);
        self.name_span(field.syntax().span(), Some(value));
        Ok(Some(loaded))
    }

    /// QC-18: `f(args)`: `f` a function, else "not a function"; with any arguments, one past a
    /// fixed list is "too many parameters" before it is read, each of exactly its parameter's
    /// type, else "type mismatch on parm N", and fewer than the list "too few parameters";
    /// nine or more "More than eight parameters". With none, no count is checked.
    fn call(&mut self, call: &ast::CallExpr) -> Typed {
        let span = call.syntax().span();
        let callee_expr = call.callee();
        let Some(callee) = self.expr_of(callee_expr.clone())? else {
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
            // QC-18.3: each argument stored to its parameter slot as it is read, `STORE_V`
            // whatever its type: "a vector copy will copy everything".
            let value = self.name(arg);
            let slot = u8::try_from(at).ok().map(ir::Name::Parameter);
            self.push(Op::StoreV, [value, slot, None]);
            // QC-11.12: a field component's type is none a parameter is declared with.
            let component =
                matches!(value, Some(ir::Name::Def(def, _)) if self.env.field_component(def));
            if fixed
                && let (Some(ty), Some(param)) = (ty, function.params.get(at))
                && (ty != *param || param.holds_variadic() || component)
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
        // QC-18.3, QC-18.4: `CALLn` of the callee; the call's value is the return global.
        let callee_name = callee_expr.and_then(|e| self.name(&e));
        match call_opcode(args.len()) {
            Some(op) => {
                self.push(op, [callee_name, None, None]);
            }
            None => self.followed = false,
        }
        self.name_span(span, Some(ir::Name::Return));
        self.returned = Some(function.ret.clone());
        Ok(Some(function.ret.clone()))
    }
}

/// Whether `expr` is a field read, in parentheses or not.
fn is_field(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Field(_) => true,
        ast::Expr::Paren(paren) => paren.expr().as_ref().is_some_and(is_field),
        _ => false,
    }
}

/// `STOREP_*` of the type `store` stores, which stores through a pointer (QC-17.3).
const fn store_through(store: Op) -> Op {
    match store {
        Op::StoreV => Op::StorepV,
        Op::StoreS => Op::StorepS,
        Op::StoreEnt => Op::StorepEnt,
        Op::StoreFld => Op::StorepFld,
        Op::StoreFnc => Op::StorepFnc,
        _ => Op::StorepF,
    }
}

/// Whether `expr` is a call, in parentheses or not, whose value is the return global.
fn is_call(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Call(_) => true,
        ast::Expr::Paren(paren) => paren.expr().as_ref().is_some_and(is_call),
        _ => false,
    }
}
