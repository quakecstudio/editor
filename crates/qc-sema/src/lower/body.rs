// SPDX-License-Identifier: GPL-3.0-or-later

//! Function bodies: parameters, locals, blocks and statements, and the state header
//! (QC-11.10, QC-13, FTEF-18, FTEF-19.19).

use qc_diag::{Code, Diagnostic, Span};
use qc_lexer::LexFlags;
use qc_preproc::{FteFlag, PragmaDefault};
use qc_syntax::ast::{self, AstNode as _};
use qc_syntax::c_scalar::CScalar;
use qc_syntax::{CompilerFlags, SyntaxKind, SyntaxNode};

use crate::ty::{SimpleType, Type};

use super::Cx;
use super::decl::{
    ArrayDim, LiteralValue, Qualifier, RawAggregate, RawValue, ValueKind, array_dim,
    declarator_type, function_params, qualifiers_of, raw_aggregate, raw_value,
};
use super::expr::{RawUse, UseKind, lower_expr};
use super::types::{TypedefScope, typedef_diagnostics};

/// A function body's parameters, locals and the names it uses (QC-13).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RawBody {
    /// The parameters, in order, with each one's name span (QC-13.2).
    pub params: Vec<RawParam>,
    /// The `local` declarations, in source order (QC-11.10).
    pub locals: Vec<RawLocal>,
    /// Every name used as a value in the body, in source order (QC-12.5).
    pub uses: Vec<RawUse>,
    /// The names a state header's frame uses, resolved before its next-think name is
    /// declared, as `QCC_PR_ParseState` reads them (FTEF-19.14).
    pub state_uses: Vec<RawUse>,
    /// A state header's frame and next function when they are literals, which FTEQCC types
    /// (FTEF-19.14).
    pub state_literals: Vec<(StatePart, RawValue)>,
    /// The parameters with no name, each by its number from 1 and its span (FTEF-19.1).
    pub unnamed_params: Vec<(usize, Span)>,
    /// What lowering found wrong in the body that resolution does not see: a `typedef` that
    /// defines no type or has storage qualifiers (FTEF-19.9).
    pub diagnostics: Vec<Diagnostic>,
    /// The names the body's `typedef`s define, each from where it stands to the end of its block;
    /// a local's type may be one defined before it there (FTEF-19.20, FTEF-20.15).
    pub typedefs: Vec<(String, Span)>,
    /// The next think functions the body's `state` statements name, each with its span, which
    /// `QCC_PR_ParseState` declares as a header's (FTEF-20.5).
    pub next_thinks: Vec<(String, Span)>,
    /// The aliases the body's `using` statements make (FTEF-20.5).
    pub aliases: Vec<RawAlias>,
    /// The types the body's declarations write with no declarator, each with where it starts,
    /// which only have their type names checked (FTEF-20.12).
    pub type_only: Vec<(Type, Span)>,
    /// The body as written, which the check stage types (`crate::check`).
    pub syntax: Option<ast::FunctionBody>,
    /// The compiler flags in force in the body, from each offset on (FTEF-21.1).
    pub flags: Vec<(usize, CompilerFlags)>,
    /// The lexer's flags in force in the body, from each offset on (FTEF-12.12).
    pub lex_flags: Vec<(usize, LexFlags)>,
    /// The types the body's casts and `sizeof`s write, each by the offset where it starts,
    /// which the check stage reads (FTEF-30).
    pub written_types: Vec<(usize, Type)>,
}

/// One alias a `using` statement makes (FTEF-20.5): `QCC_PR_GetDef (NULL, name, pr_scope,
/// false, 0, 0)` finds the definition, and `QCC_PR_DummyDef` makes the alias of its type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawAlias {
    /// The name of the definition aliased, and its span.
    pub target: (String, Span),
    /// The alias's name, and its span.
    pub alias: (String, Span),
    /// The byte offset where the statement the alias is made for ends, after which its name is
    /// no longer found; `None` for a `using` without parentheses, which holds to the end of the
    /// function.
    pub scope_end: Option<usize>,
}

/// A part of a state header `[frame, next]` (QC-13.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatePart {
    /// The frame.
    Frame,
    /// The first or last frame of a cycle `[++ first .. last]` (FTEF-18.28), which must be a
    /// float once converted (FTEF-22.3).
    CycleFrame,
    /// The next think function.
    Next,
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
    /// The qualifiers written before its type (FTEF-15.1).
    pub qualifiers: Vec<Qualifier>,
    /// A value initialiser, as the checks of QC-11.3 and FTEF-19.31 read it.
    pub value: Option<RawValue>,
    /// The initialiser's value when it is a literal (FTE-22.5).
    pub literal: Option<LiteralValue>,
    /// The first array dimension, when the declarator has one (FTEF-19.8).
    pub array: Option<ArrayDim>,
    /// A braced initialiser, read against the type (FTEF-19.47).
    pub aggregate: Option<RawAggregate>,
    /// The byte offset where the innermost `for` statement it is defined in ends, after which
    /// its name is no longer found (FTEF-19.19), or under `subscope` the block (FTEF-21.12);
    /// `None` outside any.
    pub scope_end: Option<usize>,
    /// The compiler flags and pragma defaults where its declaration starts (FTEF-21.1).
    pub flags: CompilerFlags,
    /// Whether it is a C function declaration with `qcfuncs` off, which declares a global
    /// constant function rather than a local (FTEF-21.10).
    pub global_function: bool,
}

impl RawLocal {
    /// Whether the qualifier `q` marks it.
    #[must_use]
    pub fn has(&self, q: &Qualifier) -> bool {
        self.qualifiers.contains(q)
    }

    /// Whether it is `static`: written so, or under `#pragma defaultstatic 1`, which
    /// `nonstatic` does not undo in a body (FTEF-21.14).
    #[must_use]
    pub fn is_static(&self) -> bool {
        self.has(&Qualifier::Static) || self.flags.default_on(PragmaDefault::Static)
    }
}

/// The next-think name of a state header `[frame, next]`, if present (QC-13.6).
pub(super) fn next_think_of(fn_body: &ast::FunctionBody) -> Option<(String, Span)> {
    let state = fn_body.state()?;
    let token = state.next()?;
    Some((token.text_lossless(), token.span()))
}

pub(super) fn lower_body(fn_body: &ast::FunctionBody, fn_ty: &Type, cx: Cx<'_>) -> RawBody {
    let span = fn_body.syntax().span();
    let mut body = RawBody {
        syntax: Some(fn_body.clone()),
        flags: Cx::marks_in(cx.flags, span),
        lex_flags: Cx::marks_in(cx.lex, span),
        ..RawBody::default()
    };
    collect_params(
        fn_ty,
        fn_body,
        cx,
        &mut body.params,
        &mut body.unnamed_params,
    );
    // FTEF-18.3, FTEF-18.28: a state header's expressions are values of the function, read
    // before its next-think name is declared (FTEF-19.14).
    if let Some(state) = fn_body.state() {
        let cycle = state.is_cycle();
        for (index, expr) in state.exprs().enumerate() {
            lower_expr(&expr, cx, &mut body.state_uses);
            let part = if cycle {
                StatePart::CycleFrame
            } else if index == 0 {
                StatePart::Frame
            } else {
                StatePart::Next
            };
            let value = raw_value(&expr, cx);
            if matches!(value.kind, ValueKind::Literal(_)) {
                body.state_literals.push((part, value));
            }
        }
    }
    if let Some(block) = fn_body.block() {
        lower_block(&block, cx, &mut body, Flow::default());
    }
    body.written_types = written_types(fn_body.syntax(), cx);
    // FTEF-32.7: the type `va_arg(index, TYPE)` reads.
    for call in fn_body
        .syntax()
        .descendants()
        .filter_map(ast::CallExpr::cast)
    {
        let Some(ast::Expr::Name(callee)) = call.callee() else {
            continue;
        };
        if !cx.fte || callee.name().is_none_or(|t| t.text_lossless() != "va_arg") {
            continue;
        }
        if let Some(ast::Expr::Name(ty)) = call.args().and_then(|a| a.args().nth(1))
            && let Some(token) = ty.name()
        {
            let start = token.span().start;
            body.written_types.push((
                start,
                Type::named(&token.text_lossless(), cx.c_context(start)),
            ));
        }
    }
    body
}

/// The types written in casts, `sizeof` and `va_arg` under `node`, each by where it starts,
/// which the check stage reads (FTEF-34.9).
pub(super) fn written_types(node: &SyntaxNode, cx: Cx<'_>) -> Vec<(usize, Type)> {
    let mut out = Vec::new();
    for node in node.descendants() {
        // A `va_arg`'s type is a child of its argument list (FTEF-32.7).
        let argument = node
            .parent()
            .filter(|parent| parent.kind() == SyntaxKind::ArgList)
            .and_then(|_| ast::Type::cast(node.clone()));
        let written = ast::CastExpr::cast(node.clone())
            .and_then(|cast| cast.ty())
            .or_else(|| ast::SizeofExpr::cast(node).and_then(|sizeof| sizeof.ty()))
            .or(argument);
        if let Some(ty) = written {
            out.push((ty.syntax().span().start, cx.ty(&ty)));
        }
    }
    out
}

/// The parameters named in the function type of the defining declaration (QC-13.2).
///
/// The parameter names come from the `FunctionType`'s `ParamList`; the semantic types come
/// from the same list, matched by position to [`Type::from_ast`]'s view of the function
/// type (QC-10.3). An FTEQCC parameter may have no name (FTEF-15.3); it declares nothing.
fn collect_params(
    fn_ty: &Type,
    fn_body: &ast::FunctionBody,
    cx: Cx<'_>,
    out: &mut Vec<RawParam>,
    unnamed: &mut Vec<(usize, Span)>,
) {
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
    let count = param_list.params().count();
    for (index, param) in param_list.params().enumerate() {
        let Some(name_token) = param.name() else {
            // FTEF-19.1: a parameter of a function with a body has a name, or FTEQCC's warning
            // "Parameter N of NAME is not named"; a lone `void` is no parameter (FTEF-15.3).
            let lone_void = count == 1
                && param
                    .ty()
                    .is_some_and(|t| cx.ty(&t) == Type::Simple(SimpleType::Void));
            if !lone_void {
                unnamed.push((index.saturating_add(1), param.syntax().span()));
            }
            continue;
        };
        let ty = fty
            .and_then(|f| f.params.get(index).cloned())
            .or_else(|| param.ty().map(|t| cx.ty(&t)))
            .unwrap_or(Type::Unknown);
        out.push(RawParam {
            name: name_token.text_lossless(),
            span: name_token.span(),
            ty,
        });
    }
}

/// The `ParamList` of the function that defines `fn_body` (QC-13.2): the vanilla function
/// type's list, an FTEQCC C-style declarator's own `(params)` list (FTE-17.1), or with
/// `qcfuncs` off the list before the declarator's name.
fn param_list_of(fn_body: &ast::FunctionBody) -> Option<ast::ParamList> {
    let declarator = fn_body.syntax().parent()?;
    if let Some(declarator) = ast::Declarator::cast(declarator.clone()) {
        if let Some(list) = declarator.params() {
            return Some(list);
        }
        // With `qcfuncs` off the type takes no list, and the declarator's own lists before
        // its name make the function, the last one its parameters (FTEF-18.15).
        if let Some(list) = declarator.prefix_params().pop() {
            return Some(list);
        }
    }
    let declaration = declarator.parent()?;
    let ty = declaration
        .children()
        .find_map(ast::Type::cast)
        .or_else(|| declarator.children().find_map(ast::Type::cast))?;
    function_params(&ty)
}

/// Which loops and `switch`es a statement is in, which `break` and `continue` find their target
/// in (FTEF-21.20).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Flow {
    loops: usize,
    switches: usize,
}

impl Flow {
    /// The flow in a loop's body.
    fn in_loop(self) -> Self {
        Self {
            loops: self.loops.saturating_add(1),
            ..self
        }
    }

    /// The flow in a `switch`'s body.
    fn in_switch(self) -> Self {
        Self {
            switches: self.switches.saturating_add(1),
            ..self
        }
    }
}

pub(super) fn lower_block(block: &ast::Block, cx: Cx<'_>, body: &mut RawBody, flow: Flow) {
    let first = body.locals.len();
    for stmt in block.statements() {
        lower_stmt(&stmt, cx, body, flow);
    }
    // FTEF-21.19: a `case` or `default` that no `switch` holds is "illegal cases".
    if flow.switches == 0
        && !body.diagnostics.iter().any(|d| d.code == Code::E0259)
        && let Some(label) = block
            .syntax()
            .children()
            .find(|n| n.kind() == SyntaxKind::CaseLabel)
    {
        body.diagnostics.push(Diagnostic {
            code: Code::E0259,
            message: "function contains illegal cases".to_owned(),
            span: label.span(),
        });
    }
    // FTEF-21.12: `if (pr_subscopedlocals) { //remove any new locals from the hashtable.`, read
    // once the `}` is, with the token after it lexed.
    let end = block.syntax().span().end;
    let subscope_somewhere = cx
        .flags
        .iter()
        .any(|(_, flags)| flags.has(FteFlag::PrSubscopedlocals));
    if cx.fte && subscope_somewhere && cx.next_flags(end).has(FteFlag::PrSubscopedlocals) {
        for local in body.locals.iter_mut().skip(first) {
            local.scope_end.get_or_insert(end);
        }
    }
    // FTEF-20.15: a struct a block's statements define is a type from its tag to the block's
    // end.
    for tag in block_struct_tags(block.syntax()) {
        let seen = Span::new(tag.span().start, block.syntax().span().end);
        body.typedefs.push((tag.text_lossless(), seen));
    }
    // FTEF-19.9: a `typedef` in a body is checked as one at file level.
    for td in block_typedefs(block.syntax()) {
        body.diagnostics
            .extend(typedef_diagnostics(&td, cx, TypedefScope::Body));
        for name in td.declarators().filter_map(|d| d.name()) {
            let seen = Span::new(name.span().start, block.syntax().span().end);
            body.typedefs.push((name.text_lossless(), seen));
        }
    }
}

/// The tags of the structs and unions a block's statements define, not those of blocks inside.
fn block_struct_tags(node: &SyntaxNode) -> Vec<qc_syntax::SyntaxToken> {
    let mut found = Vec::new();
    for child in node.children() {
        if child.kind() == SyntaxKind::Block {
            continue;
        }
        if child.kind() == SyntaxKind::StructType
            && let Some(tag) = child.child_tokens().find(|t| t.kind() == SyntaxKind::Name)
        {
            found.push(tag);
        }
        found.extend(block_struct_tags(&child));
    }
    found
}

/// The `typedef`s of a block's statements, those of the `extern` blocks among them
/// (FTEF-21.17): `QCC_PR_ParseDefs` reads a `typedef` in a block as it reads one anywhere.
fn block_typedefs(node: &qc_syntax::SyntaxNode) -> Vec<ast::TypedefDecl> {
    let mut found = Vec::new();
    for child in node.children() {
        if child.kind() == SyntaxKind::ExternBlock {
            found.extend(block_typedefs(&child));
        } else if let Some(td) = ast::TypedefDecl::cast(child) {
            found.push(td);
        }
    }
    found
}

/// A `break` (`is_break`) or `continue` that no loop, and for a `break` no `switch`, holds
/// (FTEF-21.19): FTEQCC counts it and reports the function once.
fn illegal_jump(body: &mut RawBody, flow: Flow, node: &SyntaxNode, is_break: bool) {
    let (held, code, what) = if is_break {
        (flow.loops > 0 || flow.switches > 0, Code::E0244, "breaks")
    } else {
        (flow.loops > 0, Code::E0245, "continues")
    };
    if !held && !body.diagnostics.iter().any(|d| d.code == code) {
        body.diagnostics.push(Diagnostic {
            code,
            message: format!("function contains illegal {what}"),
            span: node.span(),
        });
    }
}

fn lower_stmt(stmt: &ast::Stmt, cx: Cx<'_>, body: &mut RawBody, flow: Flow) {
    match stmt {
        ast::Stmt::Local(local) => lower_local(local, cx, body),
        // FTEF-21.17: a body's `extern` block holds local definitions.
        ast::Stmt::Extern(block) => {
            for local in block.items().into_iter().filter_map(ast::LocalDecl::cast) {
                lower_local(&local, cx, body);
            }
        }
        ast::Stmt::Block(block) => lower_block(block, cx, body, flow),
        ast::Stmt::Return(ret) => {
            if let Some(expr) = ret.value() {
                lower_expr(&expr, cx, &mut body.uses);
            }
        }
        ast::Stmt::If(if_stmt) => {
            if let Some(cond) = if_stmt.condition() {
                lower_expr(&cond, cx, &mut body.uses);
            }
            if let Some(then) = if_stmt.then_branch() {
                lower_stmt(&then, cx, body, flow);
            }
            if let Some(els) = if_stmt.else_branch() {
                lower_stmt(&els, cx, body, flow);
            }
        }
        ast::Stmt::While(while_stmt) => {
            if let Some(cond) = while_stmt.condition() {
                lower_expr(&cond, cx, &mut body.uses);
            }
            if let Some(inner) = while_stmt.body() {
                lower_stmt(&inner, cx, body, flow.in_loop());
            }
        }
        ast::Stmt::Do(do_stmt) => {
            if let Some(inner) = do_stmt.body() {
                lower_stmt(&inner, cx, body, flow.in_loop());
            }
            if let Some(cond) = do_stmt.condition() {
                lower_expr(&cond, cx, &mut body.uses);
            }
        }
        // FTEQCC `for (init; cond; step) body` (FTE-2). The init clause may declare a loop
        // local; the clauses hold expressions whose names are used (QC-12.5). The locals the
        // statement defines, in its clauses or its body, leave scope at its end (FTEF-19.19);
        // a `for` inside it has already ended its own.
        ast::Stmt::For(for_stmt) => {
            let first = body.locals.len();
            for clause in for_stmt.clauses() {
                if let Some(local) = clause.children().find_map(ast::LocalDecl::cast) {
                    lower_local(&local, cx, body);
                } else {
                    for expr in clause.children().filter_map(ast::Expr::cast) {
                        lower_expr(&expr, cx, &mut body.uses);
                    }
                }
            }
            if let Some(inner) = for_stmt.body() {
                lower_stmt(&inner, cx, body, flow.in_loop());
            }
            let end = for_stmt.syntax().span().end;
            for local in body.locals.iter_mut().skip(first) {
                local.scope_end.get_or_insert(end);
            }
        }
        // FTEQCC `switch (e) { ... }` (FTE-3): the scrutinee, each `case` value and the
        // statements of the body are lowered.
        ast::Stmt::Switch(switch_stmt) => {
            if let Some(scrutinee) = switch_stmt.scrutinee() {
                lower_expr(&scrutinee, cx, &mut body.uses);
            }
            if let Some(block) = switch_stmt.block() {
                for node in block.syntax().children() {
                    if let Some(label) = ast::CaseLabel::cast(node.clone()) {
                        // FTEF-18.10: a range's end is a value too.
                        for value in label.value().into_iter().chain(label.range_end()) {
                            lower_expr(&value, cx, &mut body.uses);
                        }
                    } else if let Some(stmt) = ast::Stmt::cast(node) {
                        lower_stmt(&stmt, cx, body, flow.in_switch());
                    }
                }
            } else if let Some(stmt) = switch_stmt.statement() {
                // FTEF-18.33: the body of a `switch` is any statement.
                lower_stmt(&stmt, cx, body, flow.in_switch());
            }
        }
        // `break;`, `continue;` (FTE-4) and `;` (FTE-21.5) bind no names and use none, and
        // neither do a label and `goto` (FTEF-18.23), whose names are labels, not values.
        ast::Stmt::Break(node) => illegal_jump(body, flow, node.syntax(), true),
        ast::Stmt::Continue(node) => illegal_jump(body, flow, node.syntax(), false),
        ast::Stmt::Empty(_) | ast::Stmt::Label(_) | ast::Stmt::Goto(_) | ast::Stmt::Exit(_) => {}
        // FTEF-20.5: an `asm` statement's `local` declarations define locals; its operands are
        // code generation's (FTEF-19.46).
        ast::Stmt::Asm(asm) => {
            for local in asm.locals() {
                lower_local(&local, cx, body);
            }
        }
        // FTEF-20.5: `loop s` runs its statement.
        ast::Stmt::Loop(loop_stmt) => {
            if let Some(inner) = loop_stmt.body() {
                lower_stmt(&inner, cx, body, flow.in_loop());
            }
        }
        // FTEF-20.5: `thinktime e : t;` uses its operands and, unless the target is Hexen II's,
        // which has `OP_THINKTIME`, `nextthink` and `time`, which `QCC_PR_GetSRef` looks up by
        // name.
        ast::Stmt::Thinktime(thinktime) => {
            for operand in thinktime.operands() {
                lower_expr(&operand, cx, &mut body.uses);
            }
            let span = thinktime
                .syntax()
                .token_of(SyntaxKind::ThinktimeKw)
                .map_or_else(|| thinktime.syntax().span(), |t| t.span());
            if !cx.keywords_at(span.start).hexen2_target() {
                for name in ["nextthink", "time"] {
                    body.uses
                        .push(RawUse::new(name.to_owned(), span, UseKind::Value));
                }
            }
        }
        // FTEF-20.5: `state [frame, next];` reads a state header as a function's: its values,
        // and a next think function named alone, declared when it is new.
        ast::Stmt::State(state_stmt) => {
            if let Some(state) = state_stmt.state() {
                let named = state.next();
                for expr in state.exprs() {
                    lower_expr(&expr, cx, &mut body.uses);
                }
                if let Some(token) = named {
                    body.next_thinks.push((token.text_lossless(), token.span()));
                }
            }
        }
        // FTEF-20.5: `using` makes an alias of each definition it names, in parentheses for its
        // statement, else for the rest of the function.
        ast::Stmt::Using(using) => {
            let inner = using.body();
            let scope_end = inner.as_ref().map(|_| using.syntax().span().end);
            for alias in using.aliases() {
                let (Some(target), Some(name)) = (alias.target(), alias.alias()) else {
                    continue;
                };
                body.aliases.push(RawAlias {
                    target: (target.text_lossless(), target.span()),
                    alias: (name.text_lossless(), name.span()),
                    scope_end,
                });
            }
            if let Some(inner) = inner {
                lower_stmt(&inner, cx, body, flow);
            }
        }
        ast::Stmt::Expr(expr_stmt) => {
            if let Some(expr) = expr_stmt.expr() {
                lower_expr(&expr, cx, &mut body.uses);
            }
        }
    }
}

/// The type of a declaration that writes none: under FTEQCC after a qualifier `int`,
/// `type_integer`, as `QCC_PR_ParseDefs` assumes when `hadmodifier` is set (FTEF-19.21); else
/// unknown.
pub(super) fn omitted_type(node: &SyntaxNode, fte: bool) -> Type {
    if fte && node.children().any(|n| n.kind() == SyntaxKind::Qualifier) {
        Type::Named(std::rc::Rc::from(CScalar::Integer.name()))
    } else {
        Type::Unknown
    }
}

fn lower_local(local: &ast::LocalDecl, cx: Cx<'_>, body: &mut RawBody) {
    let fte = cx.fte;
    let base = local
        .ty()
        .map_or_else(|| omitted_type(local.syntax(), fte), |t| cx.ty(&t));
    // QC-11.9 holds only in vanilla: FTEQCC allows a field or function local (FTE-21.2).
    let bad = !fte && (base.is_field() || base.is_function());
    if fte
        && local.declarators().next().is_none()
        && let Some(node) = local.ty()
    {
        body.type_only.push((base.clone(), node.syntax().span()));
    }
    for declarator in local.declarators() {
        let Some(name_token) = declarator.name() else {
            continue;
        };
        let ty = declarator_type(&base, &declarator, cx);
        let init = declarator.initializer();
        // A local initialiser is a constant value; if it is a non-constant expression we
        // still record any names it uses, so references in it resolve (QC-12.5).
        match &init {
            Some(ast::Initializer::Value(expr)) => lower_expr(expr, cx, &mut body.uses),
            Some(ast::Initializer::Aggregate(agg)) => {
                for element in agg.elements() {
                    lower_expr(&element, cx, &mut body.uses);
                }
            }
            _ => {}
        }
        for suffix in declarator.array_suffixes() {
            for expr in suffix.children().filter_map(ast::Expr::cast) {
                lower_expr(&expr, cx, &mut body.uses);
            }
        }
        let value = match &init {
            Some(ast::Initializer::Value(expr)) => Some(raw_value(expr, cx)),
            _ => None,
        };
        let literal = if fte {
            LiteralValue::of_fteqcc(init.as_ref(), cx)
        } else {
            None
        };
        // FTEF-21.10: `if (!flag_qcfuncs && pr_scope) isconstant = externfnc = true; //in C, a
        // locally defined function refers to an external one.`, at a C-style declarator's `(`.
        // C's `(*name)(params)` is a variable (`isvar |= isptr>0`), no function.
        let global_function = fte
            && !declarator.pointer_form().is_some_and(|stars| stars > 0)
            && declarator.params().is_some_and(|list| {
                !cx.flags_at(list.syntax().span().start)
                    .has(FteFlag::Qcfuncs)
            });
        body.locals.push(RawLocal {
            name: name_token.text_lossless(),
            span: name_token.span(),
            ty,
            initialized: init.is_some(),
            bad_field_or_function: bad,
            qualifiers: qualifiers_of(local.syntax()),
            value,
            literal,
            array: array_dim(&declarator, init.as_ref()),
            aggregate: raw_aggregate(&declarator, init.as_ref(), cx),
            scope_end: None,
            flags: cx.flags_at(local.syntax().span().start),
            global_function,
        });
    }
}
