// SPDX-License-Identifier: GPL-3.0-or-later

//! Type declarations: structs and unions, enums, typedefs, classes and their members, and
//! accessors (FTE-14, FTE-15, FTEF-6 to FTEF-10).

use qc_diag::{Code, Diagnostic, Span};
use qc_preproc::{FteFlag, PragmaDefault};
use qc_syntax::ast::{self, AstNode as _};
use qc_syntax::c_scalar::{CScalar, CWords};
use qc_syntax::{Dialect, Keyword, SyntaxKind, SyntaxNode};

use std::rc::Rc;

use crate::ty::{FunctionType, ParamLabels, SimpleType, Type};

use super::body::{RawBody, RawParam, lower_body, next_think_of};
use super::decl::{
    LiteralValue, Qualifier, RawDecl, RawValue, decl_base, declarator_type, lower_declarators,
    no_function, qualifiers, qualifiers_of, raw_value,
};
use super::expr::{RawUse, lower_expr};
use super::{Cx, FileDecls, RawAccessor, first_word, token_text};

/// How an enum's name gets its value (FTEF-19.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumItem {
    /// Whether the enum is `enumflags`, whose values double.
    pub flags: bool,
    /// The enum's base type: `float` unless `: type` gives another (FTEF-8).
    pub base: Type,
    /// The value given after `=`, if any.
    pub value: Option<RawValue>,
    /// That value's number, as the bits of a `float`, when it is a number literal.
    pub number: Option<u32>,
    /// Whether this is the first name of an enum whose base type is written in FTEQCC's
    /// legacy form, before the `{` with no `:` (FTEF-19.10).
    pub legacy: bool,
    /// Whether this is the enum's first name, where its count starts (FTEF-19.10).
    pub first: bool,
    /// The enum's own type name, which `QCC_PR_ParseEnum` keeps its names under; `None` for an
    /// enum with no name or a legacy base type (FTEF-19.36).
    pub enum_name: Option<String>,
}

/// The kind of type a type's name names (FTEF-7.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeKind {
    /// A class (FTEF-9).
    Class,
    /// A struct or union (FTEF-6).
    Struct,
    /// A typedef (FTEF-7).
    Typedef,
    /// A named enum (FTE-15.4).
    Enum,
    /// An accessor (FTEF-10).
    Accessor,
}

/// How a class member is qualified by `virtual`, `nonvirtual` and `static` (or `const`), which
/// only a member function's checking reads (FTEF-9.16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Qualification {
    /// None of the words, and no `virtual:` or `nonvirtual:` label before it.
    Unqualified,
    /// One of the words, or none under a label that set the default.
    Qualified,
    /// More than one of the words.
    Conflicting,
}

/// Where a class member is kept (FTEF-9.18).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberStorage {
    /// In each instance: a field, or a function.
    Instance,
    /// `static`: the global `CLASS::NAME`.
    Static,
    /// `const`: the constant `CLASS::NAME`, static too.
    Const,
}

impl MemberStorage {
    /// Whether `static` or `const` makes it a global of the class's (`isstatic`).
    #[must_use]
    pub fn is_static(self) -> bool {
        self != Self::Instance
    }
}

/// An FTEQCC class's parent and members (FTEF-9).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RawClass {
    /// The parent class's name, if any.
    pub parent: Option<String>,
    /// Whether a body was given, `{ ... }`, which may hold no member (FTEF-9.12).
    pub body: bool,
    /// The members, in declaration order.
    pub members: Vec<RawMember>,
}

/// One member of an FTEQCC class (FTEF-9.2, FTEF-9.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawMember {
    /// The member's name.
    pub name: String,
    /// The byte span of its name.
    pub span: Span,
    /// Its type.
    pub ty: Type,
    /// What it is.
    pub kind: MemberKind,
    /// Whether it has a body or value.
    pub initialized: bool,
    /// A member function's body written in the class.
    pub body: Option<RawBody>,
    /// Whether it is given a value though it is no function, `static` or `const`, and the
    /// value is no 0: "Can only initialise member functions" (FTEF-9.10).
    pub bad_init: bool,
    /// Whether `var` qualifies it, so a function member of it needs no body (FTEF-19.41).
    pub is_var: bool,
    /// Whether `static` or `ignore` qualifies it: a global of the class's, or nothing, and in
    /// neither case a member compared on redeclaration (FTEF-9.13).
    pub is_global: bool,
    /// Whether it is virtual, by `virtual` or the class's default (FTEF-9.3), which makes a
    /// function member a field of the instance.
    pub is_virtual: bool,
    /// The text of its first array suffix, compared on redeclaration (FTEF-9.13).
    pub array: Option<String>,
    /// Its `virtual`, `nonvirtual` and `static` words (FTEF-9.16).
    pub qualification: Qualification,
    /// Whether it belongs to each instance or is a global of the class's (FTEF-9.16, FTEF-9.18).
    pub storage: MemberStorage,
    /// The token after its name and array suffixes, which a constructor's warnings name as
    /// the member, `pr_token` (FTEF-9.16).
    pub follow: String,
}

/// What a class member is (FTEF-9.2, FTEF-9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    /// A data member: a field of the entity.
    Field,
    /// A `virtual` member function: a field of function type each instance may override.
    Virtual,
    /// A `nonvirtual` or `static` member function: a global named `Class::member`.
    Method,
}

/// A `struct` or `union` as `QCC_PR_ParseStruct` reads it (FTEF-6, FTEF-6.7): its tag, its
/// parent and its members, wherever the type is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawStruct {
    /// Where the keyword stands, which orders the struct among the declarations.
    pub at: usize,
    /// The byte offset just after the `}` or the tag, where the type is made.
    pub end: usize,
    /// The tag's span, else the keyword's.
    pub span: Span,
    /// The tag; `None` for an anonymous struct or union.
    pub name: Option<String>,
    /// Whether it is a `union`.
    pub union: bool,
    /// The parent a struct names after `:`, with its span.
    pub parent: Option<(String, Span)>,
    /// Whether it has a body, so it defines the type; `struct Name` alone names one.
    pub body: bool,
    /// Whether a function's body writes it, where `QCC_PR_ParseStruct`'s type lasts to the
    /// block's end.
    pub local: bool,
    /// The members, in order, of the body.
    pub members: Vec<StructMember>,
}

/// One member of a [`RawStruct`] (FTEF-6.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructMember {
    /// The name; empty for an anonymous member.
    pub name: String,
    /// The name's span, else the member's.
    pub span: Span,
    /// The member's type, with the `*`s and parameter list of its declarator.
    pub ty: Type,
    /// The text of its array suffixes, in order.
    pub array: Vec<String>,
    /// The width after `:`, when the member is a bitfield: the number when it is a literal,
    /// else `None` inside `Some`.
    pub bits: Option<Option<u32>>,
    /// Whether `static` qualifies it, so it is a global and no member.
    pub is_static: bool,
    /// Whether `virtual` qualifies it, so a function member is defined as `Struct::member`
    /// and needs a body (FTEF-19.41).
    pub is_virtual: bool,
    /// Whether the member has a body or value after `=`.
    pub initialized: bool,
    /// The names a bitfield's width uses, `QCC_PR_IntConstExpr()`'s (FTEF-6.9).
    pub width_uses: Vec<RawUse>,
    /// Whether the width is an integer constant: a literal with no fraction, or an
    /// expression; "Value is not an integer constant" otherwise (FTEF-6.9).
    pub width_integer: bool,
    /// The width's value as `QCC_PR_IntConstExpr` reads it, when it is a bitfield (FTEF-6.9).
    pub width_const: Option<IntConst>,
    /// Each array suffix's size as `QCC_PR_IntConstExpr` reads it (FTEF-6.17).
    pub sizes: Vec<IntConst>,
    /// An anonymous struct or union member's own members, which an access finds through it
    /// (FTEF-6.11).
    pub nested: Vec<StructMember>,
}

/// Whether the bitfield width `e` is an integer constant: a literal with no fraction, or an
/// expression (FTEF-6.9).
fn integer_width(e: &ast::Expr, cx: Cx<'_>) -> bool {
    match e {
        ast::Expr::Literal(literal) => match literal.kind() {
            Some(ast::LiteralKind::Float) => LiteralValue::of_literal(
                e,
                Dialect::Fteqcc,
                cx.lex_flags_at(e.syntax().span().start),
            )
            .is_some_and(
                |v| matches!(v, LiteralValue::Float(bits) if f32::from_bits(bits).fract() == 0.0),
            ),
            Some(ast::LiteralKind::FrameRef) | None => true,
            Some(ast::LiteralKind::Vector | ast::LiteralKind::String) => false,
        },
        _ => true,
    }
}

/// The members of the anonymous struct or union the member `field` is (FTEF-6.11).
fn anonymous_members(field: &SyntaxNode, cx: Cx<'_>) -> Vec<StructMember> {
    field
        .descendants()
        .find(|n| n.kind() == SyntaxKind::StructType)
        .and_then(|n| lower_struct(&n, cx))
        .map(|st| st.members)
        .unwrap_or_default()
}

/// Each array suffix's size of the struct member `field` (FTEF-6.17).
fn array_sizes(field: &SyntaxNode, cx: Cx<'_>) -> Vec<IntConst> {
    field
        .children()
        .filter(|n| n.kind() == SyntaxKind::ArraySuffix)
        .map(|n| {
            n.children()
                .find_map(ast::Expr::cast)
                .map_or(IntConst::Unfolded, |e| IntConst::of(&e, cx))
        })
        .collect()
}

/// What `QCC_PR_IntConstExpr` reads of an expression, as far as the model follows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntConst {
    /// A literal number with no fraction.
    Integer(i64),
    /// A literal that is no integer: a fraction, a vector or a string.
    NotInteger,
    /// A name, a constant if the module defines it.
    Name(String),
    /// Another expression, which the model does not fold.
    Unfolded,
}

impl IntConst {
    /// The constant `expr` is.
    pub(super) fn of(expr: &ast::Expr, cx: Cx<'_>) -> Self {
        match expr {
            ast::Expr::Literal(_) => {
                let flags = cx.lex_flags_at(expr.syntax().span().start);
                match LiteralValue::of_literal(expr, Dialect::Fteqcc, flags) {
                    Some(LiteralValue::Float(bits)) => {
                        let number = f64::from(f32::from_bits(bits));
                        if number.fract() == 0.0 && number.abs() < 9.0e15 {
                            #[expect(
                                clippy::cast_possible_truncation,
                                reason = "spec FTEF-6.17: an integral float below 2^53 is exact"
                            )]
                            let integer = number as i64;
                            Self::Integer(integer)
                        } else {
                            Self::NotInteger
                        }
                    }
                    Some(
                        LiteralValue::Vector(_)
                        | LiteralValue::String(_)
                        | LiteralValue::Builtin { .. }
                        | LiteralValue::Text(_),
                    ) => Self::NotInteger,
                    None => Self::Unfolded,
                }
            }
            ast::Expr::Name(name) if name.scope().is_none() => name
                .name()
                .map_or(Self::Unfolded, |t| Self::Name(t.text_lossless())),
            _ => Self::Unfolded,
        }
    }
}

/// The tag of a struct or union the `StructType` node `node` writes with none, a type of its
/// own (FTEF-6.12): `{}` and where its keyword stands, which no program can name.
pub(super) fn anonymous_tag(node: &SyntaxNode) -> String {
    let at = node
        .child_tokens()
        .next()
        .map_or(node.span().start, |t| t.span().start);
    format!("{{}}{at}")
}

/// The [`RawStruct`] of a `StructType` node.
pub(super) fn lower_struct(node: &SyntaxNode, cx: Cx<'_>) -> Option<RawStruct> {
    let keyword = node.child_tokens().next()?;
    let union = keyword.kind() == SyntaxKind::UnionKw;
    let mut name = None;
    let mut parent = None;
    let mut after_colon = false;
    let mut body = false;
    for token in node.child_tokens() {
        match token.kind() {
            SyntaxKind::Name if after_colon && parent.is_none() => {
                parent = Some((token.text_lossless(), token.span()));
            }
            SyntaxKind::Name if name.is_none() && !body => {
                name = Some(token.text_lossless());
            }
            SyntaxKind::Colon if !body => after_colon = true,
            SyntaxKind::LBrace => body = true,
            _ => {}
        }
    }
    let local = node
        .ancestors()
        .any(|n| matches!(n.kind(), SyntaxKind::Block | SyntaxKind::FunctionBody));
    let span = node
        .child_tokens()
        .find(|t| t.kind() == SyntaxKind::Name)
        .map_or_else(|| keyword.span(), |t| t.span());
    let mut members = Vec::new();
    let mut ty = Type::Unknown;
    let mut is_static = false;
    let mut is_virtual = false;
    for field in node
        .children()
        .filter(|n| n.kind() == SyntaxKind::StructField)
    {
        if let Some(node) = field.children().find_map(ast::Type::cast) {
            ty = cx.ty(&node);
            let qualified = |word: &str| {
                field
                    .children()
                    .filter(|n| n.kind() == SyntaxKind::Qualifier)
                    .any(|q| q.child_tokens().any(|t| t.text_lossless() == word))
            };
            is_static = qualified("static");
            is_virtual = qualified("virtual");
        }
        let width = field
            .child_tokens()
            .any(|t| t.kind() == SyntaxKind::Colon)
            .then(|| field.children().filter_map(ast::Expr::cast).last())
            .flatten();
        let mut width_uses = Vec::new();
        if let Some(expr) = &width {
            lower_expr(expr, cx, &mut width_uses);
        }
        let width_integer = width.as_ref().is_none_or(|e| integer_width(e, cx));
        let initialized = field.child_tokens().any(|t| t.kind() == SyntaxKind::Eq);
        let mut member_ty = ty.clone();
        // The `*`s before the name, not those of C's `(*name)(params)` (FTEF-21.7).
        for _ in field
            .child_tokens()
            .take_while(|t| !matches!(t.kind(), SyntaxKind::Name | SyntaxKind::LParen))
            .filter(|t| t.kind() == SyntaxKind::Star)
        {
            member_ty = Type::Pointer(std::rc::Rc::new(member_ty));
        }
        if let Some(list) = field.children().find_map(ast::ParamList::cast) {
            member_ty = cx.function(member_ty, &list);
        }
        let member_name = field.child_tokens().find(|t| t.kind() == SyntaxKind::Name);
        let bits = field
            .child_tokens()
            .any(|t| t.kind() == SyntaxKind::Colon)
            .then(|| {
                width
                    .as_ref()
                    .and_then(|e| {
                        LiteralValue::of_literal(
                            e,
                            Dialect::Fteqcc,
                            cx.lex_flags_at(e.syntax().span().start),
                        )
                    })
                    .and_then(|value| match value {
                        LiteralValue::Float(bits) => {
                            let number = f32::from_bits(bits);
                            #[expect(
                                clippy::cast_possible_truncation,
                                clippy::cast_sign_loss,
                                reason = "spec FTEF-6.9: the width is an integer constant, \
                                          and a negative or fractional one is none"
                            )]
                            let width = number as u32;
                            (number >= 0.0 && number.fract() == 0.0).then_some(width)
                        }
                        LiteralValue::Vector(_)
                        | LiteralValue::String(_)
                        | LiteralValue::Builtin { .. }
                        | LiteralValue::Text(_) => None,
                    })
            });
        members.push(StructMember {
            is_virtual,
            initialized,
            width_uses: std::mem::take(&mut width_uses),
            width_integer,
            name: member_name
                .as_ref()
                .map(|t| t.text_lossless())
                .unwrap_or_default(),
            span: member_name
                .as_ref()
                .map_or_else(|| field.span(), |t| t.span()),
            ty: member_ty,
            array: field
                .children()
                .filter(|n| n.kind() == SyntaxKind::ArraySuffix)
                .map(|n| token_text(&n))
                .collect(),
            sizes: array_sizes(&field, cx),
            nested: if member_name.is_none() {
                anonymous_members(&field, cx)
            } else {
                Vec::new()
            },
            width_const: width.as_ref().map(|e| IntConst::of(e, cx)),
            bits,
            is_static,
        });
    }
    Some(RawStruct {
        at: keyword.span().start,
        end: node.span().end,
        span,
        name,
        union,
        parent,
        body,
        local,
        members,
    })
}

/// An `enum`'s names become global float constants (FTE-15.1, FTE-15.3), and a named enum's
/// type name is recorded as a typedef's is (FTE-15.4).
/// Returns the type of a declarator after the body (FTEF-8.5).
pub(super) fn lower_enum(en: &ast::EnumDecl, cx: Cx<'_>, out: &mut Vec<RawDecl>) -> Type {
    // FTEF-19.10: in the legacy form `enum float { ... }` the word before the `{` is the base
    // type, which `QCC_PR_ParseEnum` tries first (`QCC_PR_ParseType(false, true, false)`).
    let legacy_word = en.name().filter(|t| legacy_base(t, cx).is_some());
    if legacy_word.is_none()
        && let Some(token) = en.name()
    {
        out.push(RawDecl::type_name(&token, TypeKind::Enum));
    }
    // FTEF-19.10: `enumflags` doubles its values, and a base type given after `:`, or before
    // the `{` in the legacy form, types them.
    let flags = en
        .syntax()
        .child_tokens()
        .any(|t| t.kind() == SyntaxKind::EnumKw && cx.spells(&t, Keyword::Enumflags, "enumflags"));
    // The base is the type after `:`; a parameter list after the body is the declaration's.
    let base_node = en
        .syntax()
        .child_tokens()
        .any(|t| t.kind() == SyntaxKind::Colon)
        .then(|| en.syntax().children().find_map(ast::Type::cast))
        .flatten();
    let base = match (&base_node, &legacy_word) {
        (Some(node), _) => cx.ty(node),
        (None, Some(word)) => legacy_base(word, cx).unwrap_or(Type::Unknown),
        // FTEF-21.13: `basetype = (flag_assume_integer?type_integer:type_float);`, as
        // `QCC_PR_ParseEnum` starts.
        (None, None)
            if cx
                .flags_at(en.syntax().span().start)
                .has(FteFlag::AssumeInteger) =>
        {
            Type::Named(std::rc::Rc::from(CScalar::Integer.name()))
        }
        (None, None) => Type::Simple(SimpleType::Float),
    };
    // FTEF-19.10: a base type with no `:` before it is FTEQCC's legacy form, and `enum class`
    // names its values only as `Name::value`.
    let legacy = legacy_word.is_some()
        || (base_node.is_some()
            && !en
                .syntax()
                .child_tokens()
                .any(|t| t.kind() == SyntaxKind::Colon));
    // `strictenum = QCC_PR_CheckName("class");`
    let strict = en
        .syntax()
        .child_tokens()
        .any(|t| {
            t.kind() == SyntaxKind::Name
                && cx
                    .keywords_at(t.span().start)
                    .names("class", &t.text_lossless())
        })
        .then(|| en.name().map(|t| t.text_lossless()))
        .flatten();
    let enum_name = en
        .name()
        .map(|t| t.text_lossless())
        .filter(|_| legacy_word.is_none());
    for (index, node) in en.enumerators().enumerate() {
        let Some(token) = node.child_tokens().find(|t| t.kind() == SyntaxKind::Name) else {
            continue;
        };
        // FTEF-17.10: FTEQCC makes each enum name a new constant with `QCC_PR_DummyDef`, which
        // looks for no earlier definition, so an enum declared again is no redeclaration.
        let name = match &strict {
            Some(scope) => format!("{scope}::{}", token.text_lossless()),
            None => token.text_lossless(),
        };
        // FTEF-19.10: a strict enum's values are of the enum's own type.
        let ty = strict.as_ref().map_or_else(
            || base.clone(),
            |scope| Type::Named(std::rc::Rc::from(scope.as_str())),
        );
        let mut decl = RawDecl::new(name, token.span(), ty);
        let value = node.children().find_map(ast::Expr::cast);
        if let Some(expr) = &value {
            lower_expr(expr, cx, &mut decl.uses);
        }
        let number = match value.as_ref().and_then(|e| {
            LiteralValue::of_literal(e, Dialect::Fteqcc, cx.lex_flags_at(e.syntax().span().start))
        }) {
            Some(LiteralValue::Float(bits)) => Some(bits),
            _ => None,
        };
        decl.enum_item = Some(EnumItem {
            flags,
            base: base.clone(),
            value: value.as_ref().map(|e| raw_value(e, cx)),
            number,
            legacy: legacy && index == 0,
            first: index == 0,
            enum_name: enum_name.clone(),
        });
        out.push(decl);
    }
    // The type a declarator after the body is of (FTEF-8.5): the enum's own when it has a name,
    // else its base.
    enum_name.map_or(base, |name| Type::Named(std::rc::Rc::from(name.as_str())))
}

/// The base type the word `token` before an enum's `{` names in FTEQCC's legacy form
/// (FTEF-19.10): a simple type, or a C type word or registered type name read with the keywords
/// in force (FTEF-2.8, FTEF-20.9); `None` for a name, the enum's own.
fn legacy_base(token: &qc_syntax::SyntaxToken, cx: Cx<'_>) -> Option<Type> {
    let word = token.text_lossless();
    let simple = match word.as_str() {
        "float" => SimpleType::Float,
        "vector" => SimpleType::Vector,
        "string" => SimpleType::String,
        "entity" => SimpleType::Entity,
        "void" => SimpleType::Void,
        word => {
            let context = cx.c_context(token.span().start);
            let (_, c_type) = CWords::read([word], context);
            return c_type.map(|_| Type::named(word, context));
        }
    };
    Some(Type::Simple(simple))
}

/// The names of the fields of a struct node, recorded as member names (FTE-14.1, FTEF-6).
pub(super) fn record_fields(field: &SyntaxNode, members: &mut Vec<String>) {
    members.extend(
        field
            .child_tokens()
            .filter(|t| t.kind() == SyntaxKind::Name)
            .map(|t| t.text_lossless()),
    );
}

/// A `typedef` declares the type name (FTE-14.1, FTEF-7.1), recorded as a global so a use of
/// it resolves. A struct's fields are member names, not globals, and an enum's names are
/// constants.
pub(super) fn lower_typedef(td: &ast::TypedefDecl, cx: Cx<'_>, out: &mut FileDecls) {
    let enum_ty = td
        .syntax()
        .children()
        .find_map(ast::EnumDecl::cast)
        .map(|en| lower_enum(&en, cx, &mut out.decls));
    // FTEF-7.1: `typedef struct S { ... } T;` names the struct by its keyword and tag.
    let struct_ty = td
        .syntax()
        .children()
        .find(|n| n.kind() == SyntaxKind::StructType)
        .map(|st| {
            let keyword = if st.child_tokens().any(|t| t.kind() == SyntaxKind::UnionKw) {
                "union"
            } else {
                "struct"
            };
            let tag = st
                .child_tokens()
                .find(|t| t.kind() == SyntaxKind::Name)
                .map_or_else(|| anonymous_tag(&st), |t| t.text_lossless());
            Type::Named(std::rc::Rc::from(format!("{keyword} {tag}").as_str()))
        });
    let enum_ty = enum_ty.or(struct_ty);
    out.diagnostics
        .extend(typedef_diagnostics(td, cx, TypedefScope::File));
    let node = td.syntax().children().find_map(ast::Type::cast);
    // FTEF-19.21: with its type left out, a typedef names an `int`.
    let omitted = node.is_none()
        && td
            .syntax()
            .children()
            .all(|n| !matches!(n.kind(), SyntaxKind::StructType | SyntaxKind::EnumDecl));
    // FTEF-19.35: `typedef float (t);` is `float(t)`, whose parameter `t` names no type.
    if td.declarators().next().is_none()
        && let Some(t) = &node
    {
        let mut raw = RawDecl::new(String::new(), t.syntax().span(), cx.ty(t));
        raw.type_only = true;
        out.decls.push(raw);
    }
    for declarator in td.declarators() {
        let Some(token) = declarator.name() else {
            continue;
        };
        let mut decl = RawDecl::type_name(&token, TypeKind::Typedef);
        // FTEF-7.5: the parameters of a function type, from the type's list or the
        // declarator's, for the bodies of the type's definitions.
        let list = match &node {
            Some(ast::Type::Function(func)) => func.params(),
            _ => None,
        }
        .or_else(|| declarator.params())
        .or_else(|| declarator.prefix_params().into_iter().next());
        if let Some(list) = list {
            for param in list.params() {
                if let Some(name_token) = param.name() {
                    decl.typedef_params.push(RawParam {
                        name: name_token.text_lossless(),
                        span: name_token.span(),
                        ty: param.ty().map_or(Type::Unknown, |t| cx.ty(&t)),
                    });
                }
            }
        }
        // FTEF-19.9: the type it names, which a typedef of the same name must name again. An
        // array typedef names a type the model does not follow.
        let array = declarator.array_suffixes().next().is_some();
        // FTEF-21.7: the name is a declarator's, so its parameter lists and C's `(*name)(params)`
        // make the function type it names.
        let pointer = declarator.pointer_depth() > 0
            || declarator.pointer_form().is_some_and(|stars| stars > 1);
        decl.typedef = Some(if omitted {
            Type::Named(std::rc::Rc::from(CScalar::Integer.name()))
        } else if let (Some(en), false, false) = (&enum_ty, array, pointer) {
            // FTEF-8.5: `typedef enum E { A } T;` names the enum's type, an anonymous enum's
            // base.
            en.clone()
        } else if pointer && !array && declarator.pointer_form().is_none() {
            // `typedef float *u;` names a pointer to the type, which a struct's, class's or
            // accessor's name refuses as any typedef does (FTEF-19.9).
            node.clone().map_or(Type::Unknown, |t| {
                (0..declarator.pointer_depth())
                    .fold(cx.ty(&t), |inner, _| Type::Pointer(std::rc::Rc::new(inner)))
            })
        } else {
            node.clone()
                .filter(|_| !array && !pointer)
                .map_or(Type::Unknown, |t| {
                    let mut ty = cx.ty(&t);
                    for list in declarator.prefix_params() {
                        ty = cx.function(ty, &list);
                    }
                    if let Some(list) = declarator.params() {
                        ty = cx.function(ty, &list);
                    }
                    ty
                })
        });
        out.decls.push(decl);
    }
}

/// Where a `typedef` is: `QCC_PR_ParseDefs` drops `extern` from a definition whose type is no
/// function's only `if (externfnc && !pr_scope ...)`, at file level (FTEF-21.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TypedefScope {
    /// Outside every function.
    File,
    /// In a function's body, where the `extern` stays.
    Body,
}

/// FTEF-19.9: what is wrong with a `typedef` alone: `typedef;` and `typedef float;` define no
/// type, a warning; a qualifier is "bad combination of modifiers with typedef", but `auto`,
/// `register` and `nonstatic`, and `static`, `noref` and `nosave` when a `#pragma` makes them the
/// default (FTEF-21.14). Before it, as for any declaration at file level, `extern` on a type that
/// is no function's is dropped, with its warning when `qcfuncs` is on (FTEF-21.10), and more than one
/// storage class, `typedef` among them and `static` by its default, is "Conflicting storage
/// classes." (FTEF-19.16).
pub(super) fn typedef_diagnostics(
    td: &ast::TypedefDecl,
    cx: Cx<'_>,
    scope: TypedefScope,
) -> Vec<Diagnostic> {
    let Some(token) = td.declarators().find_map(|d| d.name()) else {
        return vec![Diagnostic {
            code: Code::W0215,
            message: "typedef defines no types".to_owned(),
            span: td.syntax().span(),
        }];
    };
    let mut qualifiers = qualifiers_of(td.syntax());
    let flags = cx.flags_at(td.syntax().span().start);
    let mut out = Vec::new();
    let warn = |message: &str| Diagnostic {
        code: Code::W0214,
        message: message.to_owned(),
        span: token.span(),
    };
    let ty_node = td.syntax().children().find_map(ast::Type::cast);
    if scope == TypedefScope::File
        && qualifiers.contains(&Qualifier::Extern)
        && ty_node.as_ref().is_some_and(|t| no_function(t, &cx.ty(t)))
    {
        qualifiers.retain(|q| *q != Qualifier::Extern);
        let after = ty_node.as_ref().map_or(0, |t| t.syntax().span().end);
        if cx.next_flags(after).has(FteFlag::Qcfuncs) {
            out.push(warn("extern keyword may only apply to qc functions."));
        }
    }
    let has = |q: &Qualifier| qualifiers.contains(q);
    let statics = has(&Qualifier::Static)
        || (flags.default_on(PragmaDefault::Static) && !has(&Qualifier::Nonstatic));
    let storage = 1_usize
        .saturating_add(usize::from(has(&Qualifier::Extern)))
        .saturating_add(usize::from(has(&Qualifier::Auto)))
        .saturating_add(usize::from(statics))
        .saturating_add(usize::from(has(&Qualifier::Shared)))
        .saturating_add(usize::from(
            qualifiers.iter().any(|q| matches!(q, Qualifier::Alias(_))),
        ));
    if storage > 1 {
        out.push(warn("Conflicting storage classes."));
    }
    if has(&Qualifier::Const) && has(&Qualifier::Var) {
        out.push(warn("Conflicting constness."));
    }
    let bad = qualifiers.iter().any(|q| match q {
        Qualifier::Static => !flags.default_on(PragmaDefault::Static),
        Qualifier::Noref => !flags.default_on(PragmaDefault::Noref),
        Qualifier::Nosave => !flags.default_on(PragmaDefault::Nosave),
        Qualifier::Auto | Qualifier::Nonstatic => false,
        Qualifier::Extern
        | Qualifier::Shared
        | Qualifier::Const
        | Qualifier::Var
        | Qualifier::Used
        | Qualifier::Strip
        | Qualifier::Inline
        | Qualifier::Wrap
        | Qualifier::Weak
        | Qualifier::Accumulate
        | Qualifier::Deprecated(_)
        | Qualifier::Alias(_) => true,
    });
    if bad {
        out.push(Diagnostic {
            code: Code::E0224,
            message: format!(
                "bad combination of modifiers with typedef (defining {})",
                token.text_lossless()
            ),
            span: token.span(),
        });
    }
    out
}

/// An FTEQCC class: its name as a type, and its parent and members (FTEF-9). A member
/// function's body written in the class is lowered with the class as its scope.
pub(super) fn lower_class(class: &ast::ClassDecl, cx: Cx<'_>, out: &mut FileDecls) {
    let Some(name) = class.name() else {
        return;
    };
    let class_name = name.text_lossless();
    let mut raw = RawClass {
        parent: class.parent().map(|t| t.text_lossless()),
        body: class
            .syntax()
            .child_tokens()
            .any(|t| t.kind() == SyntaxKind::LBrace),
        members: Vec::new(),
    };
    // FTEF-9.2: `virtual:` and `nonvirtual:` set the kind of the member functions after them.
    let mut assume_virtual = None;
    for node in class.syntax().children() {
        match node.kind() {
            SyntaxKind::AccessLabel => {
                // FTEF-20.11: the label's word, `__` and any case as the parser took it.
                let word = first_word(&node).to_ascii_lowercase();
                if word.ends_with("nonvirtual") {
                    assume_virtual = Some(false);
                } else if word.ends_with("virtual") {
                    assume_virtual = Some(true);
                }
            }
            SyntaxKind::Declaration => {
                if let Some(decl) = ast::Declaration::cast(node) {
                    lower_member(&decl, &class_name, assume_virtual, cx, &mut raw.members);
                }
            }
            SyntaxKind::AccessorMember => {
                if let Some(member) = ast::AccessorMember::cast(node) {
                    out.members.extend(member.name().map(|t| t.text_lossless()));
                    out.accessors.push(raw_accessor(&member, &class_name, cx));
                    lower_accessor_body(&member, &class_name, cx, &mut out.decls);
                }
            }
            _ => {}
        }
    }
    let mut decl = RawDecl::type_name(&name, TypeKind::Class);
    decl.class = Some(raw);
    out.decls.push(decl);
    // FTEF-9.8: the variables declared with the class as their type.
    let base = decl_base(
        class.syntax(),
        Type::Named(std::rc::Rc::from(class_name.as_str())),
        cx,
        &mut out.decls,
    );
    let first = out.decls.len();
    lower_declarators(class.declarators(), &base, None, cx, &mut out.decls);
    let flags = cx.flags_at(class.syntax().span().start);
    for raw in out.decls.iter_mut().skip(first) {
        raw.flags = flags;
    }
}

/// One member declaration of a class (FTEF-9.2 to FTEF-9.4).
fn lower_member(
    decl: &ast::Declaration,
    class: &str,
    assume_virtual: Option<bool>,
    cx: Cx<'_>,
    out: &mut Vec<RawMember>,
) {
    let words = qualifiers(decl);
    // FTEF-20.11: a member's words are `__` and any case as the parser took them.
    let has = |word: &str| {
        words
            .iter()
            .any(|w| w.trim_start_matches('_').eq_ignore_ascii_case(word))
    };
    let virtual_ =
        has("virtual") || (assume_virtual == Some(true) && !has("nonvirtual") && !has("static"));
    let words_given = [
        has("virtual"),
        has("nonvirtual"),
        has("static") || has("const"),
    ];
    let qualification = match words_given.iter().filter(|&&given| given).count() {
        0 if assume_virtual.is_none() => Qualification::Unqualified,
        0 | 1 => Qualification::Qualified,
        _ => Qualification::Conflicting,
    };
    let base = decl.ty().map_or(Type::Unknown, |t| cx.ty(&t));
    let mut declared = false;
    for declarator in decl.declarators() {
        let Some(name_token) = declarator.name() else {
            continue;
        };
        declared = true;
        let ty = declarator_type(&base, &declarator, cx);
        let kind = if !ty.is_function() {
            MemberKind::Field
        } else if virtual_ {
            MemberKind::Virtual
        } else {
            MemberKind::Method
        };
        let name_end = declarator
            .array_suffixes()
            .last()
            .map_or(name_token.span().end, |n| n.span().end);
        let scope = decl
            .syntax()
            .parent()
            .unwrap_or_else(|| decl.syntax().clone());
        let follow = scope
            .tokens()
            .find(|t| !t.kind().is_trivia() && t.span().start >= name_end)
            .map(|t| t.text_lossless())
            .unwrap_or_default();
        let init = declarator.initializer();
        let body = match &init {
            Some(ast::Initializer::Body(fn_body)) => {
                let mut body = lower_body(fn_body, &ty, cx);
                // FTEF-19.42: a member's state header names its next function in the class.
                if let Some(next) = next_think_of(fn_body) {
                    body.next_thinks.insert(0, next);
                }
                Some(body)
            }
            _ => None,
        };
        // FTEF-9.10: `QCC_PR_CheckImmediate("0") || QCC_PR_CheckImmediate("0i")` reads the
        // token's text, which a vector immediate leaves as its last component's.
        let null = match &init {
            Some(ast::Initializer::Value(ast::Expr::Literal(literal))) => {
                let text = token_text(literal.syntax());
                let last = match text.strip_prefix('\'') {
                    Some(inner) => inner
                        .trim_end_matches('\'')
                        .split_whitespace()
                        .last()
                        .unwrap_or_default()
                        .to_owned(),
                    None => text,
                };
                matches!(last.as_str(), "0" | "0i")
            }
            _ => false,
        };
        let bad_init =
            init.is_some() && kind == MemberKind::Field && !null && !has("static") && !has("const");
        out.push(RawMember {
            name: name_token.text_lossless(),
            span: name_token.span(),
            ty,
            kind,
            initialized: init.is_some(),
            body,
            bad_init,
            is_var: has("var"),
            is_global: has("static") || has("ignore"),
            is_virtual: virtual_,
            qualification,
            storage: if has("const") {
                MemberStorage::Const
            } else if has("static") {
                MemberStorage::Static
            } else {
                MemberStorage::Instance
            },
            follow,
            array: declarator
                .array_suffixes()
                .next()
                .map(|n| String::from_utf8_lossy(&n.text()).into_owned()),
        });
    }
    // FTEF-9.4: `Name(void);`, a constructor written without a return type.
    if !declared
        && let Some(ast::Type::Function(func)) = decl.ty()
        && let Some(ast::Type::Simple(ret)) = func.return_type()
        && ret.text() == class
        && let Some(token) = ret
            .syntax()
            .child_tokens()
            .find(|t| t.kind() == SyntaxKind::Name)
    {
        // `QCC_PR_GenFunctionType(type_void, ...)`: the constructor returns nothing.
        let ty = match cx.ty(&ast::Type::Function(func)) {
            Type::Function(f) => Type::Function(Rc::new(FunctionType {
                ret: Type::Simple(SimpleType::Void),
                ..(*f).clone()
            })),
            other => other,
        };
        out.push(RawMember {
            name: class.to_owned(),
            span: token.span(),
            ty,
            kind: MemberKind::Method,
            initialized: false,
            body: None,
            bad_init: false,
            is_var: false,
            is_global: false,
            is_virtual: false,
            array: None,
            qualification: Qualification::Qualified,
            storage: MemberStorage::Instance,
            follow: String::new(),
        });
    }
}

/// An accessor: its name as a type, its members' names, and its member bodies as functions
/// named `Name::get_member` and `Name::set_member` (FTEF-10.1, FTEF-10.2).
pub(super) fn lower_accessor(acc: &ast::AccessorDecl, cx: Cx<'_>, out: &mut FileDecls) {
    let Some(name) = acc.name() else {
        return;
    };
    let accessor = name.text_lossless();
    let mut type_decl = RawDecl::type_name(&name, TypeKind::Accessor);
    // The base is the type after `:`; a parameter list after the name is the declaration's.
    type_decl.accessor = Some(
        acc.syntax()
            .child_tokens()
            .any(|t| t.kind() == SyntaxKind::Colon)
            .then(|| acc.syntax().children().find_map(ast::Type::cast))
            .flatten()
            .map(|t| cx.ty(&t)),
    );
    out.decls.push(type_decl);
    for member in acc.members() {
        out.members.extend(member.name().map(|t| t.text_lossless()));
        out.accessors.push(raw_accessor(&member, &accessor, cx));
        lower_accessor_body(&member, &accessor, cx, &mut out.decls);
    }
    let base = decl_base(
        acc.syntax(),
        Type::Named(std::rc::Rc::from(accessor.as_str())),
        cx,
        &mut out.decls,
    );
    let first = out.decls.len();
    lower_declarators(acc.declarators(), &base, None, cx, &mut out.decls);
    let flags = cx.flags_at(acc.syntax().span().start);
    for raw in out.decls.iter_mut().skip(first) {
        raw.flags = flags;
    }
}

/// The type a struct, union, class, enum or accessor written where a type stands names
/// (FTEF-6.8, FTEF-8.5, FTEF-9.8, FTEF-10.5), which [`Type::from_ast`] reads as no words: a
/// struct or union by its keyword and tag (`struct S`, `struct {}` for one with no tag), a class
/// or accessor by its name, and an enum as [`enum_type`] says.
pub(super) fn nested_type(simple: &ast::SimpleType, cx: Cx<'_>) -> Option<Type> {
    let child = simple.syntax().children().next()?;
    let name = |node: &SyntaxNode| {
        node.child_tokens()
            .find(|t| t.kind() == SyntaxKind::Name)
            .map(|t| t.text_lossless())
    };
    Some(match child.kind() {
        SyntaxKind::StructType => {
            let keyword = if child
                .child_tokens()
                .any(|t| t.kind() == SyntaxKind::UnionKw)
            {
                "union"
            } else {
                "struct"
            };
            let tag = name(&child).unwrap_or_else(|| anonymous_tag(&child));
            Type::Named(std::rc::Rc::from(format!("{keyword} {tag}").as_str()))
        }
        SyntaxKind::ClassDecl | SyntaxKind::AccessorDecl => {
            Type::Named(std::rc::Rc::from(name(&child)?.as_str()))
        }
        SyntaxKind::EnumDecl => enum_type(&ast::EnumDecl::cast(child)?, cx),
        _ => return None,
    })
}

/// The type an enum names (FTEF-8.5, FTEF-19.10): its legacy base when the word before the `{`
/// is one, else its own name when it has one, else its base after `:`, else `float` (an `int`
/// under `assumeint`, FTEF-21.13).
fn enum_type(en: &ast::EnumDecl, cx: Cx<'_>) -> Type {
    if let Some(base) = en.name().and_then(|t| legacy_base(&t, cx)) {
        return base;
    }
    if let Some(token) = en.name() {
        return Type::Named(std::rc::Rc::from(token.text_lossless().as_str()));
    }
    if en
        .syntax()
        .child_tokens()
        .any(|t| t.kind() == SyntaxKind::Colon)
        && let Some(node) = en.syntax().children().find_map(ast::Type::cast)
    {
        return cx.ty(&node);
    }
    if cx
        .flags_at(en.syntax().span().start)
        .has(FteFlag::AssumeInteger)
    {
        return Type::Named(std::rc::Rc::from(CScalar::Integer.name()));
    }
    Type::Simple(SimpleType::Float)
}

/// An accessor member of `owner`, as a read or a store through it sees it (FTEF-10.3).
fn raw_accessor(member: &ast::AccessorMember, owner: &str, cx: Cx<'_>) -> RawAccessor {
    RawAccessor {
        owner: owner.to_owned(),
        name: member
            .name()
            .map_or_else(String::new, |t| t.text_lossless()),
        ty: member.ty().map_or(Type::Unknown, |t| cx.ty(&t)),
        set: member.is_set(),
        indexer: member.index_ty().map(|t| cx.ty(&t)),
    }
}

/// The body of an accessor member, as a function whose parameters are `this`, `value` and the
/// index name (FTEF-10.2); a member whose value is a function is a use of it.
fn lower_accessor_body(
    member: &ast::AccessorMember,
    owner: &str,
    cx: Cx<'_>,
    out: &mut Vec<RawDecl>,
) {
    let set = member.is_set();
    let field = member
        .name()
        .map_or_else(String::new, |t| t.text_lossless());
    let span = member
        .name()
        .map_or_else(|| member.syntax().span(), |t| t.span());
    let name = format!("{owner}::{}_{field}", if set { "set" } else { "get" });
    let mut decl = RawDecl::new(name, span, Type::Unknown);
    if let Some(fn_body) = member.body() {
        // FTEF-10.8: the function `QCC_PR_GenFunctionType` makes: `this` of the accessor's
        // type, the index of its indexer's, and for `set` the value of the member's; `get`
        // returns the member's type.
        let member_ty = member.ty().map_or(Type::Unknown, |t| cx.ty(&t));
        let index_ty = member.index_ty().map(|t| cx.ty(&t));
        let this = Type::Named(owner.into());
        // The accessor's own type is no type name yet where its members are declared.
        let mut params = vec![Type::Unknown];
        params.extend(index_ty.clone());
        if set {
            params.push(member_ty.clone());
        }
        let ret = if set {
            Type::Simple(SimpleType::Void)
        } else {
            member_ty.clone()
        };
        let fn_ty = Type::Function(Rc::new(FunctionType {
            ret,
            params,
            variadic: false,
            labels: ParamLabels::default(),
        }));
        let mut body = lower_body(&fn_body, &fn_ty, cx);
        body.params.clear();
        for (param, ty) in [("this", this), ("value", member_ty)] {
            body.params.push(RawParam {
                name: param.to_owned(),
                span: Span::new(span.start, span.start),
                ty,
            });
        }
        if let Some(index) = member.index_name() {
            body.params.push(RawParam {
                name: index.text_lossless(),
                span: index.span(),
                ty: index_ty.unwrap_or(Type::Unknown),
            });
        }
        decl.ty = fn_ty;
        decl.body = Some(body);
        decl.initialized = true;
    } else if let Some(value) = member.value() {
        lower_expr(&value, cx, &mut decl.uses);
        decl.initialized = true;
    }
    out.push(decl);
}
