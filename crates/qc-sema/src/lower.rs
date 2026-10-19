// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-file lowering: the raw declarations a single file contributes, in source order.
//!
//! This is the semantic model's leaf query. It walks one file's syntax tree
//! (`qc_syntax::Parse`) and lists every name the file declares, with its type, kind and the
//! span of its name, plus the references each function body makes. It does no cross-file
//! resolution: that is [`crate::resolve`], which joins the files of a module in compilation
//! order (QC-23.3). Keeping this query per file is what lets an edit to one file re-lower
//! only that file (GOAL-1, the incremental model).

use qc_diag::Span;
use qc_lexer::atof;
use qc_syntax::ast::{self, AstNode as _};
use qc_syntax::{Dialect, Parse, SyntaxKind};

use crate::symbol::DefKind;
use crate::ty::{FunctionType, SimpleType, Type};

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
    /// The value of an initialiser that is a single literal, which FTEQCC compares when a
    /// name is initialised twice (FTE-22.2); `None` for any other initialiser or none.
    pub literal: Option<LiteralValue>,
    /// For a function definition, the lowered body; otherwise `None`.
    pub body: Option<RawBody>,
    /// For a function definition with a `[frame, next]` header, the next-think name and its
    /// span (QC-13.6): resolution declares it as a global `void()` if new.
    pub next_think: Option<(String, Span)>,
}

/// The value of a literal initialiser (FTE-22.2). A float is kept as its bits, so two
/// initialisers are the same value exactly when FTEQCC's word-by-word comparison finds them
/// equal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiteralValue {
    /// A float literal (QC-4).
    Float(u32),
    /// A vector literal (QC-5).
    Vector([u32; 3]),
    /// A string literal as written, quotes included (QC-6).
    String(String),
}

impl LiteralValue {
    /// The value of a literal initialiser, if `init` is one.
    fn of(init: Option<&ast::Initializer>) -> Option<Self> {
        let Some(ast::Initializer::Value(ast::Expr::Literal(literal))) = init else {
            return None;
        };
        let token = literal.token()?;
        let text = token.text_lossless();
        match literal.kind()? {
            ast::LiteralKind::Float => Some(Self::Float(atof(text.as_bytes()).to_bits())),
            ast::LiteralKind::Vector => {
                let inner = text.trim_matches('\'');
                let mut parts = inner.split_ascii_whitespace().map(|p| atof(p.as_bytes()));
                let mut value = [0_u32; 3];
                for word in &mut value {
                    *word = parts.next().unwrap_or(0.0).to_bits();
                }
                Some(Self::Vector(value))
            }
            ast::LiteralKind::String => Some(Self::String(text)),
            ast::LiteralKind::FrameRef => None,
        }
    }
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

/// Lowers one parsed file to its raw declarations (QC-11, QC-13), in `dialect`.
///
/// Implements the declaration grammar of QC-11.1 and the function-body shape of QC-13.1, and
/// in an FTEQCC dialect the declaration forms of `docs/spec/dialect-fteqcc-rerelease.md`:
/// `enum` names and a `typedef` type name become global definitions (FTE-10, FTE-14,
/// FTE-15), and member access (vector members, `.length`, struct fields; FTE-9, FTE-13,
/// FTE-14) is not treated as a global field use. Never fails: a file with syntax errors
/// yields whatever declarations parsed (QCD-1.4).
#[must_use]
pub fn lower_file(parse: &Parse, dialect: Dialect) -> FileDecls {
    let mut decls = Vec::new();
    let Some(tree) = parse.tree() else {
        return FileDecls { decls };
    };
    let fte = dialect.has_fteqcc_extensions();
    // Walk the top-level nodes in source order, so a name an `enum` or `typedef` declares is
    // visible to the declarations after it (QC-12.5, QC-23.3). Only an FTEQCC parse has
    // `enum` and `typedef` nodes.
    for node in tree.syntax().children() {
        match node.kind() {
            SyntaxKind::Declaration => {
                if let Some(decl) = ast::Declaration::cast(node) {
                    lower_declaration(&decl, fte, &mut decls);
                }
            }
            SyntaxKind::EnumDecl => {
                if let Some(en) = ast::EnumDecl::cast(node) {
                    lower_enum(&en, &mut decls);
                }
            }
            SyntaxKind::TypedefDecl => {
                if let Some(td) = ast::TypedefDecl::cast(node) {
                    lower_typedef(&td, &mut decls);
                }
            }
            _ => {}
        }
    }
    FileDecls { decls }
}

/// An `enum`'s names become global float constants (FTE-15.1, FTE-15.3), and a named enum's
/// type name is recorded as a typedef's is (FTE-15.4).
fn lower_enum(en: &ast::EnumDecl, out: &mut Vec<RawDecl>) {
    if let Some(token) = en.name() {
        out.push(RawDecl {
            name: token.text_lossless(),
            span: token.span(),
            ty: Type::Unknown,
            local: false,
            initialized: true,
            literal: None,
            body: None,
            next_think: None,
        });
    }
    for node in en.enumerators() {
        let Some(token) = node.child_tokens().find(|t| t.kind() == SyntaxKind::Name) else {
            continue;
        };
        out.push(RawDecl {
            name: token.text_lossless(),
            span: token.span(),
            ty: Type::Simple(SimpleType::Float),
            local: false,
            initialized: true,
            literal: None,
            body: None,
            next_think: None,
        });
    }
}

/// A `typedef struct { ... } Name;` declares the type name `Name` (FTE-14.1), recorded as a
/// global so a use of it resolves. The struct's fields are member names, not globals.
fn lower_typedef(td: &ast::TypedefDecl, out: &mut Vec<RawDecl>) {
    let Some(token) = td.name() else {
        return;
    };
    out.push(RawDecl {
        name: token.text_lossless(),
        span: token.span(),
        ty: Type::Unknown,
        local: false,
        initialized: true,
        literal: None,
        body: None,
        next_think: None,
    });
}

fn lower_declaration(decl: &ast::Declaration, fte: bool, out: &mut Vec<RawDecl>) {
    let base = decl.ty().map_or(Type::Unknown, |t| Type::from_ast(&t));
    for declarator in decl.declarators() {
        let Some(name_token) = declarator.name() else {
            continue;
        };
        let name = name_token.text_lossless();
        let span = name_token.span();
        // FTE-17.1: an FTEQCC C-style declarator `rettype name(params)` has its own
        // `ParamList`; its type is the function type of the return type and those
        // parameters, so it is the type a vanilla `rettype(params) name` declares.
        let ty = c_style_function_type(&base, &declarator).unwrap_or_else(|| base.clone());
        let init = declarator.initializer();
        let initialized = init.is_some();
        let literal = LiteralValue::of(init.as_ref());
        let (body, next_think) = match &init {
            Some(ast::Initializer::Body(fn_body)) => {
                let next_think = next_think_of(fn_body);
                (Some(lower_body(fn_body, &ty, fte)), next_think)
            }
            _ => (None, None),
        };
        out.push(RawDecl {
            name,
            span,
            ty,
            local: false,
            initialized,
            literal,
            body,
            next_think,
        });
    }
}

/// If `declarator` is an FTEQCC C-style function declarator (its own `(params)` list beside
/// the name), the function type built from the return type `ret` and the parameter types;
/// otherwise `None` (FTE-17.1). A return type other than a simple type is `void`, as
/// [`Type::from_ast`] reads the return type of a vanilla function type.
fn c_style_function_type(ret: &Type, declarator: &ast::Declarator) -> Option<Type> {
    let list = declarator.params()?;
    let ret_simple = match ret {
        Type::Simple(s) => *s,
        Type::Unknown | Type::Field(_) | Type::Function(_) => SimpleType::Void,
    };
    let params = list
        .params()
        .map(|p| p.ty().map_or(Type::Unknown, |t| Type::from_ast(&t)))
        .collect();
    Some(Type::Function(std::rc::Rc::new(FunctionType {
        ret: ret_simple,
        params,
        variadic: list.is_variadic(),
    })))
}

/// The next-think name of a state header `[frame, next]`, if present (QC-13.6).
fn next_think_of(fn_body: &ast::FunctionBody) -> Option<(String, Span)> {
    let state = fn_body.state()?;
    let token = state.next()?;
    Some((token.text_lossless(), token.span()))
}

fn lower_body(fn_body: &ast::FunctionBody, fn_ty: &Type, fte: bool) -> RawBody {
    let mut body = RawBody::default();
    collect_params(fn_ty, fn_body, &mut body.params);
    if let Some(block) = fn_body.block() {
        lower_block(&block, fte, &mut body);
    }
    body
}

/// The parameters named in the function type of the defining declaration (QC-13.2).
///
/// The parameter names come from the `FunctionType`'s `ParamList`; the semantic types come
/// from the same list, matched by position to [`Type::from_ast`]'s view of the function
/// type (QC-10.3).
fn collect_params(fn_ty: &Type, fn_body: &ast::FunctionBody, out: &mut Vec<RawParam>) {
    let Some(param_list) = param_list_of(fn_body) else {
        return;
    };
    // The function type's positional parameter types, for the vanilla `rettype(params) name`
    // order. For the FTEQCC C-style `rettype name(params)` order the return type is not a
    // function type, so each parameter's type comes from its own `Param` node instead.
    let fty = match fn_ty {
        Type::Function(fty) => Some(fty),
        _ => None,
    };
    for (index, param) in param_list.params().enumerate() {
        let Some(name_token) = param.name() else {
            continue;
        };
        let ty = fty
            .and_then(|f| f.params.get(index).cloned())
            .or_else(|| param.ty().map(|t| Type::from_ast(&t)))
            .unwrap_or(Type::Unknown);
        out.push(RawParam {
            name: name_token.text_lossless(),
            span: name_token.span(),
            ty,
        });
    }
}

/// The `ParamList` of the function that defines `fn_body` (QC-13.2): the vanilla function
/// type's list, or an FTEQCC C-style declarator's own `(params)` list (FTE-17.1).
fn param_list_of(fn_body: &ast::FunctionBody) -> Option<ast::ParamList> {
    let declarator = fn_body.syntax().parent()?;
    if let Some(list) = ast::Declarator::cast(declarator.clone()).and_then(|d| d.params()) {
        return Some(list);
    }
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

fn lower_block(block: &ast::Block, fte: bool, body: &mut RawBody) {
    for stmt in block.statements() {
        lower_stmt(&stmt, fte, body);
    }
}

fn lower_stmt(stmt: &ast::Stmt, fte: bool, body: &mut RawBody) {
    match stmt {
        ast::Stmt::Local(local) => lower_local(local, fte, body),
        ast::Stmt::Block(block) => lower_block(block, fte, body),
        ast::Stmt::Return(ret) => {
            if let Some(expr) = ret.value() {
                lower_expr(&expr, fte, body);
            }
        }
        ast::Stmt::If(if_stmt) => {
            if let Some(cond) = if_stmt.condition() {
                lower_expr(&cond, fte, body);
            }
            if let Some(then) = if_stmt.then_branch() {
                lower_stmt(&then, fte, body);
            }
            if let Some(els) = if_stmt.else_branch() {
                lower_stmt(&els, fte, body);
            }
        }
        ast::Stmt::While(while_stmt) => {
            if let Some(cond) = while_stmt.condition() {
                lower_expr(&cond, fte, body);
            }
            if let Some(inner) = while_stmt.body() {
                lower_stmt(&inner, fte, body);
            }
        }
        ast::Stmt::Do(do_stmt) => {
            if let Some(inner) = do_stmt.body() {
                lower_stmt(&inner, fte, body);
            }
            if let Some(cond) = do_stmt.condition() {
                lower_expr(&cond, fte, body);
            }
        }
        // FTEQCC `for (init; cond; step) body` (FTE-2). The init clause may declare a loop
        // local; the clauses hold expressions whose names are used (QC-12.5).
        ast::Stmt::For(for_stmt) => {
            for clause in for_stmt.clauses() {
                if let Some(local) = clause.children().find_map(ast::LocalDecl::cast) {
                    lower_local(&local, fte, body);
                } else {
                    for expr in clause.children().filter_map(ast::Expr::cast) {
                        lower_expr(&expr, fte, body);
                    }
                }
            }
            if let Some(inner) = for_stmt.body() {
                lower_stmt(&inner, fte, body);
            }
        }
        // FTEQCC `switch (e) { ... }` (FTE-3): the scrutinee, each `case` value and the
        // statements of the body are lowered.
        ast::Stmt::Switch(switch_stmt) => {
            if let Some(scrutinee) = switch_stmt.scrutinee() {
                lower_expr(&scrutinee, fte, body);
            }
            if let Some(block) = switch_stmt.block() {
                for node in block.syntax().children() {
                    if let Some(label) = ast::CaseLabel::cast(node.clone()) {
                        if let Some(value) = label.value() {
                            lower_expr(&value, fte, body);
                        }
                    } else if let Some(stmt) = ast::Stmt::cast(node) {
                        lower_stmt(&stmt, fte, body);
                    }
                }
            }
        }
        // `break;`, `continue;` (FTE-4) and `;` (FTE-21.5) bind no names and use none.
        ast::Stmt::Break(_) | ast::Stmt::Continue(_) | ast::Stmt::Empty(_) => {}
        ast::Stmt::Expr(expr_stmt) => {
            if let Some(expr) = expr_stmt.expr() {
                lower_expr(&expr, fte, body);
            }
        }
    }
}

fn lower_local(local: &ast::LocalDecl, fte: bool, body: &mut RawBody) {
    let base = local.ty().map_or(Type::Unknown, |t| Type::from_ast(&t));
    // QC-11.9 holds only in vanilla: FTEQCC allows a field or function local (FTE-21.2).
    let bad = !fte && (base.is_field() || base.is_function());
    for declarator in local.declarators() {
        let Some(name_token) = declarator.name() else {
            continue;
        };
        let init = declarator.initializer();
        // A local initialiser is a constant value; if it is a non-constant expression we
        // still record any names it uses, so references in it resolve (QC-12.5).
        match &init {
            Some(ast::Initializer::Value(expr)) => lower_expr(expr, fte, body),
            Some(ast::Initializer::Aggregate(agg)) => {
                for element in agg.elements() {
                    lower_expr(&element, fte, body);
                }
            }
            _ => {}
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

fn lower_expr(expr: &ast::Expr, fte: bool, body: &mut RawBody) {
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
                lower_expr(&inner, fte, body);
            }
        }
        ast::Expr::Not(not) => {
            if let Some(operand) = not.operand() {
                lower_expr(&operand, fte, body);
            }
        }
        ast::Expr::Binary(bin) => {
            if let Some(lhs) = bin.lhs() {
                lower_expr(&lhs, fte, body);
            }
            if let Some(rhs) = bin.rhs() {
                lower_expr(&rhs, fte, body);
            }
        }
        ast::Expr::Field(field) => {
            // The base is a value. In vanilla the field selector is an entity-field name
            // used as a value (QC-15.7), so it is collected; in an FTEQCC dialect the
            // selector may be a vector member (`.x`), `.length` or a struct field (FTE-9,
            // FTE-13, FTE-14), none of which are globals, so it is not collected.
            if let Some(base) = field.base() {
                lower_expr(&base, fte, body);
            }
            if !fte && let Some(field_expr) = field.field() {
                lower_expr(&field_expr, fte, body);
            }
        }
        ast::Expr::Call(call) => {
            if let Some(callee) = call.callee() {
                lower_expr(&callee, fte, body);
            }
            if let Some(args) = call.args() {
                for arg in args.args() {
                    lower_expr(&arg, fte, body);
                }
            }
        }
        // FTEQCC `e++`/`e--` (FTE-5): the operand is a value use.
        ast::Expr::Postfix(post) => {
            if let Some(operand) = post.operand() {
                lower_expr(&operand, fte, body);
            }
        }
        // FTEQCC `cond ? a : b` (FTE-8): the condition and both arms are values.
        ast::Expr::Ternary(tern) => {
            if let Some(cond) = tern.condition() {
                lower_expr(&cond, fte, body);
            }
            let (a, b) = tern.arms();
            if let Some(a) = a {
                lower_expr(&a, fte, body);
            }
            if let Some(b) = b {
                lower_expr(&b, fte, body);
            }
        }
        // FTEQCC `-e`, `+e`, `~e` (FTE-19.4) and `(type) e` (FTE-19.6): the operand is a value.
        ast::Expr::Unary(unary) => {
            if let Some(operand) = unary.operand() {
                lower_expr(&operand, fte, body);
            }
        }
        ast::Expr::Cast(cast) => {
            if let Some(operand) = cast.operand() {
                lower_expr(&operand, fte, body);
            }
        }
        // FTEQCC `e[i]` (FTE-13): the base and index are values.
        ast::Expr::Index(index) => {
            if let Some(base) = index.base() {
                lower_expr(&base, fte, body);
            }
            if let Some(i) = index.index() {
                lower_expr(&i, fte, body);
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
