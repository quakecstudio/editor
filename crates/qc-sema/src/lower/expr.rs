// SPDX-License-Identifier: GPL-3.0-or-later

//! The names an expression uses, with how each is used (QC-12.5, FTEF-17.3, FTEF-19.43).

use qc_diag::Span;
use qc_syntax::ast::{self, AstNode as _};

use crate::ty::Type;

use super::Cx;

/// One use of a name (QC-12.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawUse {
    /// The name used.
    pub name: String,
    /// The byte span of the name token.
    pub span: Span,
    /// How the name is used.
    pub kind: UseKind,
    /// The scope of an FTEQCC `Scope::name`: a class, `super`, or an enum (FTEF-16.4).
    pub scope: Option<String>,
    /// For a member use, what the access is made on.
    pub base: MemberBase,
    /// Whether the name is the callee of a call, `name(...)` (FTEF-19.40).
    pub called: bool,
}

impl RawUse {
    /// A use of `name` at `span`.
    pub(super) fn new(name: String, span: Span, kind: UseKind) -> Self {
        Self {
            name,
            span,
            kind,
            scope: None,
            base: MemberBase::Unknown,
            called: false,
        }
    }
}

/// How a name is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseKind {
    /// As a value: a global, a parameter or a local (QC-12.5).
    Value,
    /// In an FTEQCC dialect, as the selector of a member access `e.name`: an entity field, a
    /// class member, or a vector member, `.length` or a struct field (FTE-9, FTE-13, FTE-14,
    /// FTEF-9.3).
    Member,
    /// The name of a type a cast or a `sizeof` writes, which must be one (FTEF-19.43).
    TypeName,
    /// A cast of a literal to a type, whose name is the use's, from the literal's type: a
    /// struct takes no such value (FTEF-6.12).
    CastTo {
        /// The literal's type, as FTEQCC names it.
        from: &'static str,
    },
    /// A name that may be a type's, in a `sizeof` (FTEF-18.12): the operand FTEQCC tries as a
    /// type first (FTEF-19.38).
    TypeOrValue,
}

/// What a member access is made on, as far as the syntax tells (FTEF-17.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemberBase {
    /// Something whose type is not known from its shape.
    Unknown,
    /// A name, whose definition's type is the base's.
    Name(String),
    /// A cast to a type.
    Type(Type),
    /// A call of a named function, whose return type is the base's.
    Call(String),
}

/// What a member access is made on, read from the base expression's shape (FTEF-17.3).
fn member_base(base: &ast::Expr, cx: Cx<'_>) -> MemberBase {
    match base {
        ast::Expr::Name(name_ref) if name_ref.scope().is_none() => name_ref
            .name()
            .map_or(MemberBase::Unknown, |t| MemberBase::Name(t.text_lossless())),
        ast::Expr::Cast(cast) => cast
            .ty()
            .map_or(MemberBase::Unknown, |t| MemberBase::Type(cx.ty(&t))),
        ast::Expr::Call(call) => match call.callee() {
            Some(ast::Expr::Name(name_ref)) if name_ref.scope().is_none() => name_ref
                .name()
                .map_or(MemberBase::Unknown, |t| MemberBase::Call(t.text_lossless())),
            _ => MemberBase::Unknown,
        },
        ast::Expr::Paren(paren) => paren
            .expr()
            .map_or(MemberBase::Unknown, |e| member_base(&e, cx)),
        _ => MemberBase::Unknown,
    }
}

pub(super) fn lower_expr(expr: &ast::Expr, cx: Cx<'_>, uses: &mut Vec<RawUse>) {
    let fte = cx.fte;
    match expr {
        ast::Expr::Name(name_ref) => {
            if let Some(token) = name_ref.name() {
                let mut use_ = RawUse::new(token.text_lossless(), token.span(), UseKind::Value);
                use_.scope = name_ref.scope().map(|t| t.text_lossless());
                uses.push(use_);
            }
        }
        ast::Expr::Literal(_) => {}
        ast::Expr::Paren(paren) => {
            if let Some(inner) = paren.expr() {
                lower_expr(&inner, cx, uses);
            }
        }
        ast::Expr::Not(not) => {
            if let Some(operand) = not.operand() {
                lower_expr(&operand, cx, uses);
            }
        }
        ast::Expr::Binary(bin) => {
            if let Some(lhs) = bin.lhs() {
                lower_expr(&lhs, cx, uses);
            }
            if let Some(rhs) = bin.rhs() {
                lower_expr(&rhs, cx, uses);
            }
        }
        ast::Expr::Field(field) => {
            // The base is a value. In vanilla the field selector is an entity-field name
            // used as a value (QC-15.7). In an FTEQCC dialect a selector that is a name may
            // also be a vector member (`.x`), `.length`, a struct field or a class member
            // (FTE-9, FTE-13, FTE-14, FTEF-9.3), so it is a member use, which resolution tells
            // apart by what the access is made on.
            let base = field.base();
            if let Some(base) = &base {
                lower_expr(base, cx, uses);
            }
            match field.field() {
                Some(ast::Expr::Name(name_ref)) if fte => {
                    if let Some(token) = name_ref.name() {
                        let mut use_ =
                            RawUse::new(token.text_lossless(), token.span(), UseKind::Member);
                        use_.scope = name_ref.scope().map(|t| t.text_lossless());
                        use_.base = base
                            .as_ref()
                            .map_or(MemberBase::Unknown, |b| member_base(b, cx));
                        uses.push(use_);
                    }
                }
                Some(field_expr) => lower_expr(&field_expr, cx, uses),
                None => {}
            }
        }
        ast::Expr::Call(call) => lower_call(call, cx, uses),
        // FTEQCC `e++`/`e--` (FTE-5): the operand is a value use.
        ast::Expr::Postfix(post) => {
            if let Some(operand) = post.operand() {
                lower_expr(&operand, cx, uses);
            }
        }
        // FTEQCC `cond ? a : b` (FTE-8): the condition and both arms are values.
        ast::Expr::Ternary(tern) => {
            if let Some(cond) = tern.condition() {
                lower_expr(&cond, cx, uses);
            }
            let (a, b) = tern.arms();
            if let Some(a) = a {
                lower_expr(&a, cx, uses);
            }
            if let Some(b) = b {
                lower_expr(&b, cx, uses);
            }
        }
        // FTEQCC `-e`, `+e`, `~e` (FTE-19.4), `++e`, `&e`, `*e` (FTEF-16.7) and `(type) e`
        // (FTE-19.6): the operand is a value.
        ast::Expr::Unary(unary) => {
            if let Some(operand) = unary.operand() {
                lower_expr(&operand, cx, uses);
            }
        }
        ast::Expr::Cast(cast) => lower_cast(cast, cx, uses),
        // FTEQCC `e[i]` (FTE-13): the base and index are values.
        ast::Expr::Index(index) => {
            if let Some(base) = index.base() {
                lower_expr(&base, cx, uses);
            }
            if let Some(i) = index.index() {
                lower_expr(&i, cx, uses);
            }
        }
        // FTEQCC `[x, y, z]` (FTEF-16.3): each component is a value.
        ast::Expr::Vector(vector) => {
            for element in vector.elements() {
                lower_expr(&element, cx, uses);
            }
        }
        // FTEQCC `sizeof` (FTEF-18.12): an expression it measures names values; a type does
        // not.
        ast::Expr::Sizeof(sizeof) => {
            if let Some(ty) = sizeof.ty() {
                type_uses(&ty, cx, uses);
            }
            if let Some(operand) = sizeof.operand() {
                let before = uses.len();
                lower_expr(&operand, cx, uses);
                if let ast::Expr::Name(_) = &operand
                    && uses.len() == before.saturating_add(1)
                    && let Some(use_) = uses.last_mut()
                {
                    use_.kind = UseKind::TypeOrValue;
                }
            }
        }
    }
}

/// A call: the callee, marked as called when it is a plain name under an FTEQCC profile, and
/// each argument as a value.
fn lower_call(call: &ast::CallExpr, cx: Cx<'_>, uses: &mut Vec<RawUse>) {
    let fte = cx.fte;
    let callee = call.callee();
    if let Some(callee) = &callee {
        let before = uses.len();
        lower_expr(callee, cx, uses);
        if fte
            && let ast::Expr::Name(name) = callee
            && name.scope().is_none()
            && uses.len() == before.saturating_add(1)
            && let Some(use_) = uses.last_mut()
        {
            use_.called = true;
        }
    }
    // FTEF-17.4: `autocvar(name, default)` names the cvar, which is no value. (`sizeof`
    // and the type it may take are a `SizeofExpr`, FTEF-18.12.)
    let callee_name = match &callee {
        Some(ast::Expr::Name(n)) if fte => n.name().map(|t| t.text_lossless()),
        _ => None,
    };
    let skip = usize::from(callee_name.as_deref() == Some("autocvar"));
    if let Some(args) = call.args() {
        for arg in args.args().skip(skip) {
            let before = uses.len();
            lower_expr(&arg, cx, uses);
            // FTEF-17.1: `spawn(Thing)` takes a class by its name.
            if callee_name.as_deref() == Some("spawn")
                && let ast::Expr::Name(_) = &arg
                && uses.len() == before.saturating_add(1)
                && let Some(use_) = uses.last_mut()
            {
                use_.kind = UseKind::TypeOrValue;
            }
        }
    }
}

/// A cast: the type's name as a type use, a literal operand's source type under an FTEQCC
/// profile (FTEF-22), then the operand and any aggregate initialiser's elements as values.
fn lower_cast(cast: &ast::CastExpr, cx: Cx<'_>, uses: &mut Vec<RawUse>) {
    if let Some(ty) = cast.ty() {
        type_uses(&ty, cx, uses);
        if cx.fte
            && let Some(ast::Expr::Literal(literal)) = cast.operand()
            && let Type::Named(name) = cx.ty(&ty)
        {
            let from = match literal.kind() {
                Some(ast::LiteralKind::Vector) => "vector",
                Some(ast::LiteralKind::String) => "string",
                Some(ast::LiteralKind::Float | ast::LiteralKind::FrameRef) | None => "float",
            };
            uses.push(RawUse::new(
                name.to_string(),
                ty.syntax().span(),
                UseKind::CastTo { from },
            ));
        }
    }
    if let Some(operand) = cast.operand() {
        lower_expr(&operand, cx, uses);
    }
    for element in cast
        .syntax()
        .children()
        .filter_map(ast::AggregateInit::cast)
        .flat_map(|agg| agg.elements().collect::<Vec<_>>())
    {
        lower_expr(&element, cx, uses);
    }
}

/// The name a type written in a cast or `sizeof` ends in, as a use that must name a type
/// (FTEF-19.43): under the `*`s and field marks, a name that is no C type word.
fn type_uses(ty: &ast::Type, cx: Cx<'_>, uses: &mut Vec<RawUse>) {
    if !cx.fte {
        return;
    }
    let mut current = cx.ty(ty);
    loop {
        match current {
            Type::Pointer(inner) | Type::Field(inner) => current = (*inner).clone(),
            Type::Named(name) if !name.contains(char::is_whitespace) => {
                uses.push(RawUse::new(
                    name.to_string(),
                    ty.syntax().span(),
                    UseKind::TypeName,
                ));
                return;
            }
            Type::Named(_) | Type::Simple(_) | Type::Function(_) | Type::Unknown => return,
        }
    }
}
