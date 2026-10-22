// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/pr_comp.c, PR_CompileFile, PR_ParseDefs, PR_ParseImmediateStatements, PR_ParseState, PR_ParseStatement, PR_Expression, PR_Term, PR_ParseValue, PR_ParseFunctionCall

//! Lowering a resolved module's syntax trees to statements (QC-13 to QC-22).
//!
//! `qcc` emits code while it parses: each construct's statements in the order the parser meets
//! its parts. This module walks each file's tree in that order, with the definition every name
//! is taken from the semantic model, and builds the [`Program`] as id's `PR_CompileFile`,
//! `PR_ParseDefs`, `PR_ParseImmediateStatements`, `PR_ParseStatement` and `PR_Expression` do. The
//! type rules of expressions, which the semantic model does not check, are this module's
//! (QC-16 to QC-19): an operator's operands select its opcode by their basic types, a call's
//! arguments must have its parameters' types.

use qc_diag::{Code, Diagnostic, Span};
use qc_progs::Function;
use qc_progs::opcode::Opcode;
use qc_sema::{DefId, FileDiagnostic, FileId, ModuleIndex};
use qc_syntax::ast::{self, AstNode};
use qc_syntax::{Dialect, Parse, SyntaxToken};
use qc_types::Etype;

use crate::Internal;
use crate::names::Names;
use crate::ops::{self, Operator};
use crate::precache::Precaches;
use crate::program::{Literal, OFS_PARM0, OFS_RETURN, Program, ValueType, operand};

/// Why a declaration's code stopped: an error in the program, which abandons the declaration
/// as `PR_ParseError` does (QC-24.2), or a construct the compiler cannot lower.
#[derive(Debug)]
pub(crate) enum Abort {
    /// A diagnostic; the next declaration is compiled.
    Error(Diagnostic),
    /// The compiler met something the semantic model should have refused.
    Internal(Internal, Span),
}

type Lowered<T> = Result<T, Abort>;

/// Attaches `span` to an [`Internal`] error.
trait At<T> {
    fn at(self, span: Span) -> Lowered<T>;
}

impl<T> At<T> for Result<T, Internal> {
    fn at(self, span: Span) -> Lowered<T> {
        self.map_err(|internal| Abort::Internal(internal, span))
    }
}

impl<T> At<T> for Option<T> {
    fn at(self, span: Span) -> Lowered<T> {
        self.ok_or(Abort::Internal(Internal::Unlowered, span))
    }
}

/// A value an expression leaves: the global it is in and what that global is.
#[derive(Clone, Debug)]
struct Value {
    ofs: u32,
    kind: ValueKind,
}

/// What holds a value, which decides its type.
#[derive(Clone, Debug)]
enum ValueKind {
    /// A definition of the list, at its place: its type is the definition's.
    Def(usize),
    /// An operator's temporary, of the type the operator gives (QC-21.5).
    Temp(ValueType),
    /// The return value's global, `def_ret`: its type is that of the last call made, read when
    /// an operator checks it, since `qcc` keeps one `def_ret` and retypes it at each call
    /// (QC-18.4, QC-18.6).
    Return,
}

/// Where an assignment stores: a definition or temporary, or a field through the pointer
/// `ADDRESS` made (QC-17.6, QC-17.3).
enum Target {
    /// `STORE_*` into the global.
    Value(Value),
    /// `STOREP_*` through the pointer at `ofs` to a field of type `inner`.
    Field { ofs: u32, inner: ValueType },
}

/// The compiler's state while it lowers a module.
pub(crate) struct Compiler<'a> {
    index: &'a ModuleIndex,
    names: &'a Names,
    pub(crate) program: Program,
    pub(crate) precaches: Precaches,
    pub(crate) errors: Vec<FileDiagnostic>,
    /// The file being compiled.
    file: FileId,
    /// The string table offset of its name, `s_file`.
    s_file: i32,
    /// The function whose body is being compiled, `pr_scope`.
    scope: Option<DefId>,
    /// The end of the last `local` declaration's globals, `locals_end` (QC-21.4).
    locals_end: u32,
    /// The type of the last call's value, `def_ret.type` (QC-18.4).
    return_type: ValueType,
}

impl<'a> Compiler<'a> {
    pub(crate) fn new(index: &'a ModuleIndex, names: &'a Names) -> Self {
        Self {
            index,
            names,
            program: Program::new(),
            precaches: Precaches::default(),
            errors: Vec::new(),
            file: FileId(0),
            s_file: 0,
            scope: None,
            locals_end: 0,
            return_type: ValueType::simple(Etype::Void),
        }
    }

    /// `PR_CompileFile`: the file's name is copied into the string table, then each of its
    /// declarations is compiled; an error abandons the declaration it is in (QC-24.2,
    /// QC-25.4).
    pub(crate) fn compile_file(
        &mut self,
        file: FileId,
        name: &str,
        parse: &Parse,
    ) -> Result<(), (Internal, FileId, Span)> {
        self.file = file;
        self.s_file = self
            .program
            .copy_string(name.as_bytes())
            .map_err(|i| (i, file, Span::new(0, 0)))?;
        let Some(tree) = parse.tree() else {
            return Ok(());
        };
        for declaration in tree.declarations() {
            self.scope = None;
            match self.declaration(&declaration) {
                Ok(()) => {}
                Err(Abort::Error(diagnostic)) => {
                    self.errors.push(FileDiagnostic { file, diagnostic });
                }
                Err(Abort::Internal(internal, span)) => return Err((internal, file, span)),
            }
        }
        Ok(())
    }

    /// An error at `span`.
    fn error<T>(code: Code, message: String, span: Span) -> Lowered<T> {
        Err(Abort::Error(Diagnostic {
            code,
            message,
            span,
        }))
    }

    /// The definition the name token `token` is (QC-12).
    fn def_of(&self, token: &SyntaxToken) -> Lowered<DefId> {
        self.names
            .def(self.file, token.span(), self.scope)
            .ok_or(Abort::Internal(Internal::Unresolved, token.span()))
    }

    /// The place in the list of the definition `token` names, made now if it is new: the
    /// allocating `PR_GetDef` a declaration calls (QC-11, QC-21.2).
    fn declare(&mut self, token: &SyntaxToken) -> Lowered<usize> {
        let id = self.def_of(token)?;
        self.program.get_def(self.index, id).at(token.span())
    }

    /// The type of `value` as an operator reads it.
    fn type_of(&self, value: &Value) -> ValueType {
        match &value.kind {
            ValueKind::Def(place) => self
                .program
                .def(*place)
                .map_or(ValueType::simple(Etype::Void), |d| d.ty.clone()),
            ValueKind::Temp(ty) => ty.clone(),
            ValueKind::Return => self.return_type.clone(),
        }
    }

    // ---------------------------------------------------------------------------------------
    // Declarations
    // ---------------------------------------------------------------------------------------

    /// `PR_ParseDefs` at file level (QC-11): each name is made or found, and an initialiser
    /// defines a function (QC-13) or stores a constant (QC-11.6).
    fn declaration(&mut self, declaration: &ast::Declaration) -> Lowered<()> {
        for declarator in declaration.declarators() {
            let name = declarator.name().at(declaration.syntax().span())?;
            let id = match self.names.def(self.file, name.span(), None) {
                Some(id) => id,
                None => self
                    .names
                    .global(&name.text_lossless())
                    .ok_or(Abort::Internal(Internal::Unresolved, name.span()))?,
            };
            let place = self.program.get_def(self.index, id).at(name.span())?;
            match declarator.initializer() {
                None => {}
                Some(ast::Initializer::Value(expr)) => {
                    let literal = literal_of(&expr).at(expr.syntax().span())?;
                    self.program
                        .initialise(place, &literal)
                        .at(expr.syntax().span())?;
                }
                Some(ast::Initializer::Body(body)) => {
                    self.function(declaration, &name, (id, place), FunctionInit::Body(&body))?;
                }
                Some(ast::Initializer::Builtin(builtin)) => {
                    self.function(
                        declaration,
                        &name,
                        (id, place),
                        FunctionInit::Builtin(&builtin),
                    )?;
                }
                Some(ast::Initializer::Aggregate(aggregate)) => {
                    return Err(Abort::Internal(
                        Internal::Unsupported,
                        aggregate.syntax().span(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// A function definition, the function branch of `PR_ParseDefs` and
    /// `PR_ParseImmediateStatements` (QC-13): a body's parameters, state header and
    /// statements, or a builtin number; then the function's record (QC-13.9, QC-21.4,
    /// QC-25.2).
    fn function(
        &mut self,
        declaration: &ast::Declaration,
        name: &SyntaxToken,
        (id, place): (DefId, usize),
        init: FunctionInit<'_>,
    ) -> Lowered<()> {
        let span = name.span();
        let def = self.program.def(place).at(span)?.clone();
        let function_type = def.ty.function().at(span)?.clone();
        let locals_start = self.program.next_global().at(span)?;
        self.locals_end = locals_start;
        self.scope = Some(id);
        let first_statement = match init {
            FunctionInit::Builtin(builtin) => {
                // QC-13.3: `#n` is builtin `n`; no parameter or local storage is made.
                let token = builtin.number().at(builtin.syntax().span())?;
                let number = qc_syntax::builtin_binum(&token.text_lossless(), Dialect::Vanilla)
                    .at(token.span())?;
                number
                    .checked_neg()
                    .ok_or(Abort::Internal(Internal::TooLarge, token.span()))?
            }
            FunctionInit::Body(body) => {
                self.parameters(declaration, span)?;
                let code = operand_index(self.program.statements.len()).at(span)?;
                if let Some(state) = body.state() {
                    self.state(&state)?;
                }
                if let Some(block) = body.block() {
                    for statement in block.statements() {
                        self.statement(&statement)?;
                    }
                }
                // QC-13.7: the body ends with `DONE`.
                self.program.emit(Opcode::Done, 0, 0);
                code
            }
        };
        self.scope = None;
        // `PR_ParseDefs` marks the function initialised here, which nothing reads for a
        // function: no literal is a function (QC-20.2), and no function is saved (QC-21.6).
        let number = operand_index(self.program.functions.len()).at(span)?;
        self.program
            .set_words(def.ofs, &[u32::from_ne_bytes(number.to_ne_bytes())])
            .at(span)?;
        let s_name = self.program.copy_string(&latin1(&def.name)).at(span)?;
        let numparms = if function_type.variadic {
            -1
        } else {
            i32::try_from(function_type.params.len())
                .map_err(|_| Abort::Internal(Internal::TooLarge, span))?
        };
        let mut parm_size = [0_u8; 8];
        for (size, param) in parm_size.iter_mut().zip(&function_type.params) {
            *size = if ValueType::Of(param.clone()).etype() == Etype::Vector {
                3
            } else {
                1
            };
        }
        let locals = self
            .locals_end
            .checked_sub(locals_start)
            .and_then(|l| i32::try_from(l).ok())
            .at(span)?;
        self.program.functions.push(Function {
            first_statement,
            parm_start: operand(locals_start).at(span)?,
            locals,
            profile: 0,
            s_name,
            s_file: self.s_file,
            numparms,
            parm_size,
        });
        Ok(())
    }

    /// The parameters of a body, made at the next globals in order (QC-13.2, QC-13.5): the
    /// names written in the defining declaration's type.
    fn parameters(&mut self, declaration: &ast::Declaration, span: Span) -> Lowered<()> {
        let Some(ast::Type::Function(function)) = declaration.ty() else {
            return Err(Abort::Internal(Internal::Unlowered, span));
        };
        let Some(list) = function.params() else {
            return Ok(());
        };
        for param in list.params() {
            if let Some(name) = param.name() {
                self.declare(&name)?;
            }
        }
        Ok(())
    }

    /// `PR_ParseState`: `STATE frame, next`, the frame a shared literal, the next function
    /// made as a global `void()` if it is new (QC-13.6, QC-12.6).
    fn state(&mut self, state: &ast::State) -> Lowered<()> {
        let span = state.syntax().span();
        let frame = state.frame().at(span)?;
        let literal = literal_of_token(&frame).at(frame.syntax().span())?;
        let frame_place = self.program.immediate(&literal).at(span)?;
        let next = state.next().at(span)?;
        let next_place = self.declare(&next)?;
        let a = self.program.def(frame_place).at(span)?.ofs;
        let b = self.program.def(next_place).at(span)?.ofs;
        self.program
            .emit(Opcode::State, operand(a).at(span)?, operand(b).at(span)?);
        Ok(())
    }

    // ---------------------------------------------------------------------------------------
    // Statements
    // ---------------------------------------------------------------------------------------

    /// `PR_ParseStatement` (QC-14, QC-22.1).
    fn statement(&mut self, statement: &ast::Stmt) -> Lowered<()> {
        let span = statement.syntax().span();
        match statement {
            ast::Stmt::Block(block) => {
                for inner in block.statements() {
                    self.statement(&inner)?;
                }
            }
            ast::Stmt::Return(ret) => {
                // QC-14.3, QC-19.5: the value is not checked against the return type.
                let a = match ret.value() {
                    Some(value) => operand(self.expr(&value)?.ofs).at(span)?,
                    None => 0,
                };
                self.program.emit(Opcode::Return, a, 0);
            }
            ast::Stmt::While(w) => {
                // QC-14.5: the condition, `IFNOT` past the loop, the body, `GOTO` back.
                let top = self.program.statements.len();
                let condition = self.expr(&w.condition().at(span)?)?;
                let test = self
                    .program
                    .emit(Opcode::IfNot, operand(condition.ofs).at(span)?, 0);
                self.statement(&w.body().at(span)?)?;
                let back = branch(self.program.statements.len(), top).at(span)?;
                self.program.emit(Opcode::Goto, back, 0);
                self.patch_b(test, span)?;
            }
            ast::Stmt::Do(d) => {
                // QC-14.6: the body, the condition, `IF` back to the body.
                let top = self.program.statements.len();
                self.statement(&d.body().at(span)?)?;
                let condition = self.expr(&d.condition().at(span)?)?;
                let back = branch(self.program.statements.len(), top).at(span)?;
                self.program
                    .emit(Opcode::If, operand(condition.ofs).at(span)?, back);
            }
            ast::Stmt::Local(local) => self.local(local)?,
            ast::Stmt::If(i) => {
                // QC-14.4: `IFNOT` past the statement; with `else`, a `GOTO` past the other.
                let condition = self.expr(&i.condition().at(span)?)?;
                let test = self
                    .program
                    .emit(Opcode::IfNot, operand(condition.ofs).at(span)?, 0);
                self.statement(&i.then_branch().at(span)?)?;
                if let Some(otherwise) = i.else_branch() {
                    let skip = self.program.emit(Opcode::Goto, 0, 0);
                    self.patch_b(test, span)?;
                    self.statement(&otherwise)?;
                    let past = branch(skip, self.program.statements.len()).at(span)?;
                    if let Some(s) = self.program.statements.get_mut(skip) {
                        s.a = past;
                    }
                } else {
                    self.patch_b(test, span)?;
                }
            }
            ast::Stmt::Expr(e) => {
                // QC-14.9: any expression, its statements emitted.
                self.expr(&e.expr().at(span)?)?;
            }
            ast::Stmt::For(_)
            | ast::Stmt::Switch(_)
            | ast::Stmt::Break(_)
            | ast::Stmt::Continue(_)
            | ast::Stmt::Empty(_)
            | ast::Stmt::Label(_)
            | ast::Stmt::Goto(_)
            | ast::Stmt::Loop(_)
            | ast::Stmt::Thinktime(_)
            | ast::Stmt::State(_)
            | ast::Stmt::Using(_)
            | ast::Stmt::Exit(_)
            | ast::Stmt::Asm(_) => return Err(Abort::Internal(Internal::Unsupported, span)),
        }
        Ok(())
    }

    /// Points the branch of statement `at`, an `IF` or `IFNOT`, at the next statement
    /// (QC-22.2).
    fn patch_b(&mut self, at: usize, span: Span) -> Lowered<()> {
        let to = branch(at, self.program.statements.len()).at(span)?;
        if let Some(s) = self.program.statements.get_mut(at) {
            s.b = to;
        }
        Ok(())
    }

    /// `local`: `PR_ParseDefs` in the function (QC-11.10); its globals end the function's
    /// locals so far (QC-21.4).
    fn local(&mut self, local: &ast::LocalDecl) -> Lowered<()> {
        let span = local.syntax().span();
        for declarator in local.declarators() {
            let name = declarator.name().at(span)?;
            let place = self.declare(&name)?;
            match declarator.initializer() {
                None => {}
                Some(ast::Initializer::Value(expr)) => {
                    let literal = literal_of(&expr).at(expr.syntax().span())?;
                    self.program.initialise(place, &literal).at(span)?;
                }
                Some(
                    ast::Initializer::Body(_)
                    | ast::Initializer::Builtin(_)
                    | ast::Initializer::Aggregate(_),
                ) => return Err(Abort::Internal(Internal::Unsupported, span)),
            }
        }
        self.locals_end = self.program.next_global().at(span)?;
        Ok(())
    }

    // ---------------------------------------------------------------------------------------
    // Expressions
    // ---------------------------------------------------------------------------------------

    /// `PR_Expression` and `PR_Term` (QC-15 to QC-19): the statements of `expr`, operands left
    /// to right, each operator's after its operands' (QC-15.9).
    fn expr(&mut self, expr: &ast::Expr) -> Lowered<Value> {
        let span = expr.syntax().span();
        match expr {
            ast::Expr::Literal(literal) => {
                let value = literal_of_token(literal).at(span)?;
                let place = self.program.immediate(&value).at(span)?;
                self.def_value(place, span)
            }
            ast::Expr::Name(name) => {
                let token = name.name().at(span)?;
                let id = self.def_of(&token)?;
                let place = self
                    .program
                    .place(id)
                    .ok_or(Abort::Internal(Internal::Unallocated, span))?;
                self.def_value(place, span)
            }
            ast::Expr::Paren(paren) => self.expr(&paren.expr().at(span)?),
            ast::Expr::Not(not) => {
                // QC-16.4: `NOT_*` by the operand's type.
                let value = self.expr(&not.operand().at(span)?)?;
                let ty = self.type_of(&value).etype();
                let Some(op) = ops::not_opcode(ty) else {
                    return Self::error(Code::E0302, "type mismatch for !".to_owned(), span);
                };
                let ofs = self
                    .program
                    .emit_result(op, value.ofs, 0, Etype::Float)
                    .at(span)?;
                Ok(Value {
                    ofs,
                    kind: ValueKind::Temp(ValueType::simple(Etype::Float)),
                })
            }
            ast::Expr::Binary(binary) => self.binary(binary, span),
            ast::Expr::Field(field) => self.field(field, span),
            ast::Expr::Call(call) => self.call(call, span),
            ast::Expr::Postfix(_)
            | ast::Expr::Ternary(_)
            | ast::Expr::Index(_)
            | ast::Expr::Unary(_)
            | ast::Expr::Cast(_)
            | ast::Expr::Vector(_)
            | ast::Expr::Sizeof(_) => Err(Abort::Internal(Internal::Unsupported, span)),
        }
    }

    fn def_value(&self, place: usize, span: Span) -> Lowered<Value> {
        let ofs = self.program.def(place).at(span)?.ofs;
        Ok(Value {
            ofs,
            kind: ValueKind::Def(place),
        })
    }

    /// A binary operator (QC-16.1): both operands, then the first row of the operator whose
    /// operand types are theirs; `=` is [`Compiler::assign`].
    fn binary(&mut self, binary: &ast::BinaryExpr, span: Span) -> Lowered<Value> {
        let op_span = binary.op_token().map_or(span, |t| t.span());
        let operator = match binary.op().at(span)? {
            ast::BinaryOp::Assign => {
                let lhs = binary.lhs().at(span)?;
                let rhs = binary.rhs().at(span)?;
                return self.assign(&lhs, &rhs, op_span);
            }
            ast::BinaryOp::Mul => Operator::Mul,
            ast::BinaryOp::Div => Operator::Div,
            ast::BinaryOp::BitAnd => Operator::BitAnd,
            ast::BinaryOp::BitOr => Operator::BitOr,
            ast::BinaryOp::Add => Operator::Add,
            ast::BinaryOp::Sub => Operator::Sub,
            ast::BinaryOp::Eq => Operator::Eq,
            ast::BinaryOp::Ne => Operator::Ne,
            ast::BinaryOp::Le => Operator::Le,
            ast::BinaryOp::Ge => Operator::Ge,
            ast::BinaryOp::Lt => Operator::Lt,
            ast::BinaryOp::Gt => Operator::Gt,
            ast::BinaryOp::And => Operator::And,
            ast::BinaryOp::Or => Operator::Or,
            ast::BinaryOp::Rem
            | ast::BinaryOp::BitXor
            | ast::BinaryOp::Shl
            | ast::BinaryOp::Shr
            | ast::BinaryOp::AddAssign
            | ast::BinaryOp::SubAssign
            | ast::BinaryOp::MulAssign
            | ast::BinaryOp::DivAssign
            | ast::BinaryOp::BitAndAssign
            | ast::BinaryOp::BitOrAssign
            | ast::BinaryOp::BitClearAssign
            | ast::BinaryOp::BitXorAssign
            | ast::BinaryOp::ShlAssign
            | ast::BinaryOp::ShrAssign
            | ast::BinaryOp::Comma
            | ast::BinaryOp::Pow
            | ast::BinaryOp::Cross
            | ast::BinaryOp::Spaceship => {
                return Err(Abort::Internal(Internal::Unsupported, op_span));
            }
        };
        let left = self.expr(&binary.lhs().at(span)?)?;
        let right = self.expr(&binary.rhs().at(span)?)?;
        // The types are read after both operands, as `PR_Expression` reads them: a call in the
        // right operand retypes the return value the left one may be (QC-18.6).
        let a = self.type_of(&left).etype();
        let b = self.type_of(&right).etype();
        let Some(row) = ops::select(operator, a, b, None) else {
            return Self::mismatch(operator, op_span);
        };
        let ofs = self
            .program
            .emit_result(row.opcode, left.ofs, right.ofs, row.c)
            .at(span)?;
        Ok(Value {
            ofs,
            kind: ValueKind::Temp(ValueType::simple(row.c)),
        })
    }

    fn mismatch<T>(operator: Operator, span: Span) -> Lowered<T> {
        Self::error(
            Code::E0302,
            format!("type mismatch for {}", operator.spelling()),
            span,
        )
    }

    /// A field access `e.f` (QC-16.1, QC-16.6): the entity, the field, then `LOAD_*` by the
    /// field's type into a temporary of that type.
    fn field(&mut self, field: &ast::FieldExpr, span: Span) -> Lowered<Value> {
        let op_span = field
            .syntax()
            .child_tokens()
            .find(|t| t.kind() == qc_syntax::SyntaxKind::Dot)
            .map_or(span, |t| t.span());
        let base = self.expr(&field.base().at(span)?)?;
        let selector = self.expr(&field.field().at(span)?)?;
        let a = self.type_of(&base).etype();
        let field_type = self.type_of(&selector);
        let b = field_type.etype();
        let Some(inner) = field_type.field_inner() else {
            return Self::mismatch(Operator::Dot, op_span);
        };
        // QC-16.7: `type_c == ev_void` turns the result's check off, so a field of `void`
        // takes the first row, `LOAD_F`.
        let c = Some(inner.etype()).filter(|&c| c != Etype::Void);
        let Some(row) = ops::select(Operator::Dot, a, b, c) else {
            return Self::mismatch(Operator::Dot, op_span);
        };
        let ofs = self
            .program
            .emit_result(row.opcode, base.ofs, selector.ofs, row.c)
            .at(span)?;
        // `if (type_c != ev_void) e->type = e2->type->aux_type;`: a `void` field's value keeps
        // the row's type.
        let ty = if c.is_some() {
            inner
        } else {
            ValueType::simple(row.c)
        };
        Ok(Value {
            ofs,
            kind: ValueKind::Temp(ty),
        })
    }

    /// `x = e` (QC-17): the left side first, a field access made an address (QC-17.3,
    /// QC-27.11), then the value, then `STORE_*` or `STOREP_*` by the two basic types. The
    /// value of the assignment is the value stored (QC-17.6, QC-17.7).
    fn assign(&mut self, lhs: &ast::Expr, rhs: &ast::Expr, op_span: Span) -> Lowered<Value> {
        let span = lhs.syntax().span();
        let target = match unparenthesized(lhs) {
            Some(ast::Expr::Field(field)) => {
                let loaded = self.field(&field, span)?;
                let inner = self.type_of(&loaded);
                // QC-17.3: the field's load becomes `ADDRESS`, a pointer to the field.
                if let Some(last) = self.program.statements.last_mut() {
                    last.op = u32::from(Opcode::Address.number());
                }
                Target::Field {
                    ofs: loaded.ofs,
                    inner,
                }
            }
            _ => Target::Value(self.expr(lhs)?),
        };
        let value = self.expr(rhs)?;
        let b = self.type_of(&value).etype();
        let (a, to, inner) = match &target {
            Target::Value(v) => (self.type_of(v).etype(), v.ofs, None),
            Target::Field { ofs, inner } => (Etype::Pointer, *ofs, Some(inner.etype())),
        };
        let Some(row) = ops::select(Operator::Assign, a, b, None) else {
            return Self::mismatch(Operator::Assign, op_span);
        };
        if inner.is_some_and(|inner| inner != b) {
            return Self::mismatch(Operator::Assign, op_span);
        }
        self.program.emit(
            row.opcode,
            operand(value.ofs).at(op_span)?,
            operand(to).at(op_span)?,
        );
        Ok(value)
    }

    /// `PR_ParseFunctionCall` (QC-18): each argument evaluated and copied with `STORE_V` to
    /// its parameter slot, then `CALLn`; the value is the return global, typed as the
    /// function's return type.
    fn call(&mut self, call: &ast::CallExpr, span: Span) -> Lowered<Value> {
        let callee = self.expr(&call.callee().at(span)?)?;
        let callee_type = self.type_of(&callee);
        let Some(function) = callee_type.function().cloned() else {
            return Self::error(Code::E0303, "not a function".to_owned(), span);
        };
        let name = match &callee.kind {
            ValueKind::Def(place) => self.program.def(*place).map(|d| d.name.clone()),
            ValueKind::Temp(_) | ValueKind::Return => None,
        };
        let fixed = (!function.variadic).then_some(function.params.len());
        let mut count = 0_usize;
        let args: Vec<ast::Expr> = call.args().map(|a| a.args().collect()).unwrap_or_default();
        for arg in &args {
            let arg_span = arg.syntax().span();
            if fixed.is_some_and(|n| count >= n) {
                return Self::error(Code::E0304, "too many parameters".to_owned(), arg_span);
            }
            let value = self.expr(arg)?;
            if count == 0
                && let Some(name) = &name
            {
                self.precaches.record(name, &self.program, value.ofs);
            }
            if let Some(param) = fixed.and_then(|_| function.params.get(count))
                && self.type_of(&value) != ValueType::Of(param.clone())
            {
                return Self::error(
                    Code::E0306,
                    format!("type mismatch on parm {count}"),
                    arg_span,
                );
            }
            if let Some(slot) = parm_slot(count) {
                self.program.emit(
                    Opcode::StoreV,
                    operand(value.ofs).at(arg_span)?,
                    operand(slot).at(arg_span)?,
                );
            }
            count = count.saturating_add(1);
        }
        // QC-18.8: `if (!PR_Check(")"))` skips the count check of a call with no arguments.
        if !args.is_empty() && fixed.is_some_and(|n| count != n) {
            return Self::error(Code::E0305, "too few parameters".to_owned(), span);
        }
        let Some(op) = ops::call_opcode(count) else {
            return Self::error(Code::E0307, "More than eight parameters".to_owned(), span);
        };
        self.program.emit(op, operand(callee.ofs).at(span)?, 0);
        self.return_type = ValueType::Of(function.ret.clone());
        Ok(Value {
            ofs: OFS_RETURN,
            kind: ValueKind::Return,
        })
    }
}

/// What initialises a function.
#[derive(Clone, Copy)]
enum FunctionInit<'t> {
    /// `= { ... }` or `= [frame, next] { ... }`.
    Body(&'t ast::FunctionBody),
    /// `= #n`.
    Builtin(&'t ast::Builtin),
}

/// `expr` with any parentheses around it taken off, when it is a field access: the statement
/// `qcc` makes an address is the last one emitted, which for `(e.f)` is still the field's load
/// (QC-17.3).
fn unparenthesized(expr: &ast::Expr) -> Option<ast::Expr> {
    match expr {
        ast::Expr::Paren(paren) => paren.expr().and_then(|inner| unparenthesized(&inner)),
        ast::Expr::Field(_) => Some(expr.clone()),
        ast::Expr::Literal(_)
        | ast::Expr::Name(_)
        | ast::Expr::Not(_)
        | ast::Expr::Binary(_)
        | ast::Expr::Call(_)
        | ast::Expr::Postfix(_)
        | ast::Expr::Ternary(_)
        | ast::Expr::Index(_)
        | ast::Expr::Unary(_)
        | ast::Expr::Cast(_)
        | ast::Expr::Vector(_)
        | ast::Expr::Sizeof(_) => None,
    }
}

/// Parameter slot `i`'s global offset, `OFS_PARM0 + 3 * i` (QC-18.3). A ninth argument's slot
/// is past the eight, as id's `qcc` writes `def_parms[8]`; its call is refused (QC-18.2).
fn parm_slot(i: usize) -> Option<u32> {
    u32::try_from(i)
        .ok()?
        .checked_mul(3)?
        .checked_add(OFS_PARM0)
}

/// The relative branch from statement `from` to statement `to` (QC-22.2).
fn branch(from: usize, to: usize) -> Result<i32, Internal> {
    let from = i64::try_from(from).map_err(|_| Internal::TooLarge)?;
    let to = i64::try_from(to).map_err(|_| Internal::TooLarge)?;
    i32::try_from(to.checked_sub(from).ok_or(Internal::TooLarge)?).map_err(|_| Internal::TooLarge)
}

/// The bytes of a name the syntax tree gave as one `char` per byte (Latin-1).
pub(crate) fn latin1(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect()
}

/// A statement or function index as the `i32` a record stores.
fn operand_index(index: usize) -> Result<i32, Internal> {
    i32::try_from(index).map_err(|_| Internal::TooLarge)
}

/// The literal an initialiser is (QC-11.3; QC-27.8 refuses any other).
fn literal_of(expr: &ast::Expr) -> Option<Literal> {
    match expr {
        ast::Expr::Literal(literal) => literal_of_token(literal),
        ast::Expr::Paren(_)
        | ast::Expr::Name(_)
        | ast::Expr::Not(_)
        | ast::Expr::Binary(_)
        | ast::Expr::Field(_)
        | ast::Expr::Call(_)
        | ast::Expr::Postfix(_)
        | ast::Expr::Ternary(_)
        | ast::Expr::Index(_)
        | ast::Expr::Unary(_)
        | ast::Expr::Cast(_)
        | ast::Expr::Vector(_)
        | ast::Expr::Sizeof(_) => None,
    }
}

/// A literal's value as the vanilla lexer reads its text (QC-4 to QC-6); a frame macro
/// reference is the number the preprocessor wrote in its place (QC-8.5).
fn literal_of_token(literal: &ast::Literal) -> Option<Literal> {
    let token = literal.token()?;
    let lexed = qc_lexer::lex_with(token.text(), Dialect::Vanilla);
    let [token] = lexed.tokens.as_slice() else {
        return None;
    };
    match &token.kind {
        qc_lexer::TokenKind::Float(value) => Some(Literal::Float(*value)),
        qc_lexer::TokenKind::Vector(value) => Some(Literal::Vector(*value)),
        qc_lexer::TokenKind::String(bytes) => Some(Literal::String(bytes.clone())),
        qc_lexer::TokenKind::Name(_)
        | qc_lexer::TokenKind::Punct(_)
        | qc_lexer::TokenKind::FrameRef(_)
        | qc_lexer::TokenKind::Frames(_)
        | qc_lexer::TokenKind::FrameCommand(_) => None,
    }
}
