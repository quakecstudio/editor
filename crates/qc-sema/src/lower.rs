// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-file lowering: the raw declarations a single file contributes, in source order.
//!
//! This is the semantic model's leaf query. It walks one file's syntax tree
//! (`qc_syntax::Parse`) and lists every name the file declares, with its type, kind and the
//! span of its name, plus the references each function body makes. It does no cross-file
//! resolution: that is [`crate::resolve`], which joins the files of a module in compilation
//! order (QC-23.3). Keeping this query per file is what lets an edit to one file re-lower
//! only that file (GOAL-1, the incremental model).

use qc_lexer::Span;
use qc_syntax::Parse;
use qc_syntax::ast::{self, AstNode as _};

use crate::symbol::DefKind;
use crate::ty::Type;

/// A name a file declares, before cross-file resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawDecl {
    /// The declared name.
    pub name: String,
    /// The byte span of the name token.
    pub span: Span,
    /// Its declared type.
    pub ty: Type,
    /// Whether it is a `local` (QC-11.10); at file level it is a global, function or field.
    pub local: bool,
    /// Whether the declaration has an initialiser (QC-11.5).
    pub initialized: bool,
    /// For a function definition, the lowered body; otherwise `None`.
    pub body: Option<RawBody>,
    /// For a function definition with a `[frame, next]` header, the next-think name and its
    /// span (QC-13.6): resolution declares it as a global `void()` if new.
    pub next_think: Option<(String, Span)>,
}

/// A function body's parameters, locals and the names it uses (QC-13).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RawBody {
    /// The parameters, in order, with each one's name span (QC-13.2).
    pub params: Vec<RawParam>,
    /// The `local` declarations, in source order (QC-11.10).
    pub locals: Vec<RawLocal>,
    /// Every name used as a value in the body, in source order (QC-12.5).
    pub uses: Vec<RawUse>,
}

/// One parameter of a function definition (QC-13.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawParam {
    /// The parameter name.
    pub name: String,
    /// The byte span of the name token.
    pub span: Span,
    /// The parameter type.
    pub ty: Type,
}

/// One `local` declaration inside a body (QC-11.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawLocal {
    /// The local's name.
    pub name: String,
    /// The byte span of the name token.
    pub span: Span,
    /// The local's type.
    pub ty: Type,
    /// Whether it has an initialiser (QC-11.10).
    pub initialized: bool,
    /// Whether it is a `local` field or function, which is an error (QCD-3.8, QC-11.9).
    pub bad_field_or_function: bool,
}

/// One use of a name as a value inside a body (QC-12.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawUse {
    /// The name used.
    pub name: String,
    /// The byte span of the name token.
    pub span: Span,
}

/// Every declaration a file contributes, in source order.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FileDecls {
    /// The file-level and body declarations, in source order.
    pub decls: Vec<RawDecl>,
}

/// Lowers one parsed file to its raw declarations (QC-11, QC-13).
///
/// Implements the declaration grammar of QC-11.1 and the function-body shape of QC-13.1.
/// Never fails: a file with syntax errors yields whatever declarations parsed (QCD-1.4).
#[must_use]
pub fn lower_file(parse: &Parse) -> FileDecls {
    let mut decls = Vec::new();
    let Some(tree) = parse.tree() else {
        return FileDecls { decls };
    };
    for decl in tree.declarations() {
        lower_declaration(&decl, &mut decls);
    }
    FileDecls { decls }
}

fn lower_declaration(decl: &ast::Declaration, out: &mut Vec<RawDecl>) {
    let base = decl.ty().map_or(Type::Unknown, |t| Type::from_ast(&t));
    for declarator in decl.declarators() {
        let Some(name_token) = declarator.name() else {
            continue;
        };
        let name = name_token.text_lossless();
        let span = name_token.span();
        let init = declarator.initializer();
        let initialized = init.is_some();
        let (body, next_think) = match &init {
            Some(ast::Initializer::Body(fn_body)) => {
                let next_think = next_think_of(fn_body);
                (Some(lower_body(fn_body, &base)), next_think)
            }
            _ => (None, None),
        };
        out.push(RawDecl {
            name,
            span,
            ty: base.clone(),
            local: false,
            initialized,
            body,
            next_think,
        });
    }
}

/// The next-think name of a state header `[frame, next]`, if present (QC-13.6).
fn next_think_of(fn_body: &ast::FunctionBody) -> Option<(String, Span)> {
    let state = fn_body.state()?;
    let token = state.next()?;
    Some((token.text_lossless(), token.span()))
}

fn lower_body(fn_body: &ast::FunctionBody, fn_ty: &Type) -> RawBody {
    let mut body = RawBody::default();
    collect_params(fn_ty, fn_body, &mut body.params);
    if let Some(block) = fn_body.block() {
        lower_block(&block, &mut body);
    }
    body
}

/// The parameters named in the function type of the defining declaration (QC-13.2).
///
/// The parameter names come from the `FunctionType`'s `ParamList`; the semantic types come
/// from the same list, matched by position to [`Type::from_ast`]'s view of the function
/// type (QC-10.3).
fn collect_params(fn_ty: &Type, fn_body: &ast::FunctionBody, out: &mut Vec<RawParam>) {
    // The AST parameter list lives on the declaration's FunctionType, not the body. Reach it
    // through the body's parent declaration's type.
    let Some(param_list) = param_list_of(fn_body) else {
        return;
    };
    let Type::Function(fty) = fn_ty else {
        return;
    };
    for (index, param) in param_list.params().enumerate() {
        let Some(name_token) = param.name() else {
            continue;
        };
        let ty = fty.params.get(index).cloned().unwrap_or(Type::Unknown);
        out.push(RawParam {
            name: name_token.text_lossless(),
            span: name_token.span(),
            ty,
        });
    }
}

/// The `ParamList` of the function type that defines `fn_body` (QC-13.2).
fn param_list_of(fn_body: &ast::FunctionBody) -> Option<ast::ParamList> {
    let declarator = fn_body.syntax().parent()?;
    let declaration = declarator.parent()?;
    let ty = declaration
        .children()
        .find_map(ast::Type::cast)
        .or_else(|| declarator.children().find_map(ast::Type::cast))?;
    match ty {
        ast::Type::Function(func) => func.params(),
        ast::Type::Simple(_) | ast::Type::Field(_) => None,
    }
}

fn lower_block(block: &ast::Block, body: &mut RawBody) {
    for stmt in block.statements() {
        lower_stmt(&stmt, body);
    }
}

fn lower_stmt(stmt: &ast::Stmt, body: &mut RawBody) {
    match stmt {
        ast::Stmt::Local(local) => lower_local(local, body),
        ast::Stmt::Block(block) => lower_block(block, body),
        ast::Stmt::Return(ret) => {
            if let Some(expr) = ret.value() {
                lower_expr(&expr, body);
            }
        }
        ast::Stmt::If(if_stmt) => {
            if let Some(cond) = if_stmt.condition() {
                lower_expr(&cond, body);
            }
            if let Some(then) = if_stmt.then_branch() {
                lower_stmt(&then, body);
            }
            if let Some(els) = if_stmt.else_branch() {
                lower_stmt(&els, body);
            }
        }
        ast::Stmt::While(while_stmt) => {
            if let Some(cond) = while_stmt.condition() {
                lower_expr(&cond, body);
            }
            if let Some(inner) = while_stmt.body() {
                lower_stmt(&inner, body);
            }
        }
        ast::Stmt::Do(do_stmt) => {
            if let Some(inner) = do_stmt.body() {
                lower_stmt(&inner, body);
            }
            if let Some(cond) = do_stmt.condition() {
                lower_expr(&cond, body);
            }
        }
        ast::Stmt::Expr(expr_stmt) => {
            if let Some(expr) = expr_stmt.expr() {
                lower_expr(&expr, body);
            }
        }
    }
}

fn lower_local(local: &ast::LocalDecl, body: &mut RawBody) {
    let base = local.ty().map_or(Type::Unknown, |t| Type::from_ast(&t));
    let bad = base.is_field() || base.is_function();
    for declarator in local.declarators() {
        let Some(name_token) = declarator.name() else {
            continue;
        };
        let init = declarator.initializer();
        // A local initialiser is a constant value; if it is a non-constant expression we
        // still record any names it uses, so references in it resolve (QC-12.5).
        if let Some(ast::Initializer::Value(expr)) = &init {
            lower_expr(expr, body);
        }
        body.locals.push(RawLocal {
            name: name_token.text_lossless(),
            span: name_token.span(),
            ty: base.clone(),
            initialized: init.is_some(),
            bad_field_or_function: bad,
        });
    }
}

fn lower_expr(expr: &ast::Expr, body: &mut RawBody) {
    match expr {
        ast::Expr::Name(name_ref) => {
            if let Some(token) = name_ref.name() {
                body.uses.push(RawUse {
                    name: token.text_lossless(),
                    span: token.span(),
                });
            }
        }
        ast::Expr::Literal(_) => {}
        ast::Expr::Paren(paren) => {
            if let Some(inner) = paren.expr() {
                lower_expr(&inner, body);
            }
        }
        ast::Expr::Not(not) => {
            if let Some(operand) = not.operand() {
                lower_expr(&operand, body);
            }
        }
        ast::Expr::Binary(bin) => {
            if let Some(lhs) = bin.lhs() {
                lower_expr(&lhs, body);
            }
            if let Some(rhs) = bin.rhs() {
                lower_expr(&rhs, body);
            }
        }
        ast::Expr::Field(field) => {
            // The base is a value; the field selector is a name used as a value too
            // (fields are ordinary names, QC-15.7).
            if let Some(base) = field.base() {
                lower_expr(&base, body);
            }
            if let Some(field_expr) = field.field() {
                lower_expr(&field_expr, body);
            }
        }
        ast::Expr::Call(call) => {
            if let Some(callee) = call.callee() {
                lower_expr(&callee, body);
            }
            if let Some(args) = call.args() {
                for arg in args.args() {
                    lower_expr(&arg, body);
                }
            }
        }
    }
}

/// The kind a top-level raw declaration resolves to, from its type (QC-11.8, QC-11.9).
#[must_use]
pub fn top_level_kind(ty: &Type) -> DefKind {
    if ty.is_function() {
        DefKind::Function
    } else if ty.is_field() {
        DefKind::Field
    } else {
        DefKind::Global
    }
}
