// SPDX-License-Identifier: GPL-3.0-or-later

//! Checking function bodies: the type of every expression and the diagnostics of expressions
//! and statements, as FTEQCC's `QCC_PR_Expression` and `QCC_PR_ParseStatement` and `qcc`'s
//! `PR_Expression` and `PR_ParseStatement` check them (`docs/design/semantic-model.md`).
//!
//! The resolver runs the check of each body where it resolves the body, so the definitions and
//! types the check sees are those declared before the body in compilation order, as in the
//! one-pass compilers. It reads them through [`Env`], which the resolver implements; the check
//! knows nothing else of resolution.

mod assign;
mod compare;
pub(crate) mod constant;
pub(crate) mod convert;
mod emulate;
pub(crate) mod etype;
mod expr;
pub(crate) mod fold;
mod member;
pub(crate) mod opcodes;
mod pointer;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use qc_diag::{Code, Span};
use qc_lexer::LexFlags;
use qc_preproc::OpcodeTarget;
use qc_syntax::CompilerFlags;
use qc_syntax::ast;

use crate::cast::{Literal, Source};
use crate::symbol::{DefId, Definition};
use crate::ty::Type;
use constant::Constant;

use etype::Etype;

/// What the check stage reads of the module around the body it checks.
pub(crate) trait Env {
    /// The definition the name written at offset `at` of the body's text resolved to.
    fn reference(&self, at: usize) -> Option<DefId>;
    /// A definition of the module.
    fn definition(&self, def: DefId) -> Option<&Definition>;
    /// Whether a definition is an array (`def->arraysize`, FTEF-19.8).
    fn is_array(&self, def: DefId) -> bool;
    /// Whether a definition is a constant, and its value (`QCC_SRef_EvalConst`, FTEF-26.5).
    fn constness(&self, def: DefId) -> Constness;
    /// Whether FTEQCC made a definition constant (`def->constant`, FTEF-19.3), which makes it
    /// read-only to an assignment (FTEF-29.1) whether or not its value is known.
    fn constant(&self, def: DefId) -> bool;
    /// What `ty` is at bottom, through typedefs, enums, accessors and truth values; `None` for
    /// a type the model does not follow, on which nothing is checked.
    fn shape(&self, ty: &Type) -> Option<Shape>;
    /// The compiler flags in force at offset `at` (FTEF-21.1).
    fn flags_at(&self, at: usize) -> CompilerFlags;
    /// The lexer's flags in force at offset `at` (FTEF-12.12).
    fn lex_flags_at(&self, at: usize) -> LexFlags;
    /// The output target and the opcodes it has at offset `at` (FTEF-25).
    fn target_at(&self, at: usize) -> OpcodeTarget;
    /// Whether a definition named `name` is visible where the body is, as
    /// `QCC_PR_GetSRef(NULL, name, pr_scope, false, ...)` finds an emulation's helper.
    fn defines(&self, name: &str) -> bool;
    /// Whether the body is a member function's (`pr_classtype`).
    fn in_class(&self) -> bool;
    /// Whether the module follows FTEQCC (PRJ-2.8).
    fn fteqcc(&self) -> bool;
    /// Reports a diagnostic at `span` of the body's text, under the warning settings in force
    /// there.
    fn report(&mut self, code: Code, span: Span, message: String);
    /// The member `name` of the class `ty` names, found in it or its parents (`CLASS::NAME`,
    /// FTEF-24.6); `None` when `ty` is no class or none has the member.
    fn class_member(&self, ty: &Type, name: &str) -> Option<DefId>;
    /// Whether `a` and `b` are the same type by FTEQCC's `typecmp_lax`, or one is a truth value of
    /// the other (FTEF-28.2).
    fn lax_same(&self, a: &Type, b: &Type) -> bool;
    /// `ty` as FTEQCC's messages print it, `TypeName` (FTEF-24.7).
    fn type_name(&self, ty: &Type) -> String;
    /// The type of the member `name` of the struct or union `ty`, through its parents, and
    /// whether the member is an array (`QCC_PR_FindStructMember`, FTEF-6.11).
    fn struct_member(&self, ty: &Type, name: &str) -> Option<(Type, bool)>;
    /// FTEQCC's implicit cast of `source` to `to` at `span` (`QCC_EvaluateCast(src, to, true)`,
    /// FTEF-19.31), reported there; whether it is refused, "Cannot cast", a fatal error.
    fn cast(&mut self, source: Source<'_>, to: &Type, span: Span) -> bool;
}

/// What a type is at bottom, as FTEQCC's checker reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shape {
    /// The basic type, an accessor or truth value taken for its `parentclass`.
    pub(crate) etype: Etype,
    /// For a field or a pointer, the basic type it holds or points at.
    pub(crate) holds: Option<Etype>,
}

/// The type of a checked expression and what the checker knows of its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Value {
    /// Its type; [`Type::Unknown`] after an error, on which no further error is reported.
    pub(crate) ty: Type,
    /// Whether it is a constant, and its value (`QCC_SRef_EvalConst`, FTEF-26).
    pub(crate) constness: Constness,
    /// Whether it names an array, whose type is its elements' (`REF_ARRAYHEAD`).
    pub(crate) array: bool,
    /// The name of the definition it is, which FTEQCC's messages print (`sym->name`).
    pub(crate) name: Option<String>,
    /// The symbol that holds it, which the comparison warnings read (FTEF-28).
    pub(crate) sym: Sym,
    /// Whether an assignment to it is to a constant (`ref->readonly`, FTEF-29.1).
    pub(crate) readonly: bool,
    /// What it refers to, which a store names and a compound assignment may update in place.
    pub(crate) reference: Reference,
    /// Whether it is the operand of a postfix `++` or `--` (`ref->postinc`).
    pub(crate) postinc: bool,
}

/// What holds a value, `QCC_sref_t`'s `sym` as the comparison warnings read it (FTEF-28.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sym {
    /// A temporary the compiler made.
    Temp,
    /// A definition, and whether it is constant (`sym->constant`).
    Def(DefId, bool),
    /// An immediate constant, which FTEQCC shares among equal values.
    Immediate(Constant),
}

impl Sym {
    /// Whether it is constant and no temporary (`sym->constant && !sym->temp`).
    pub(crate) const fn constant(self) -> bool {
        match self {
            Self::Temp => false,
            Self::Def(_, constant) => constant,
            Self::Immediate(_) => true,
        }
    }
}

impl Value {
    /// A value of `ty` that is no literal.
    pub(crate) const fn of(ty: Type) -> Self {
        Self {
            ty,
            constness: Constness::Variable,
            array: false,
            name: None,
            sym: Sym::Temp,
            readonly: true,
            reference: Reference::Global,
            postinc: false,
        }
    }

    /// The constant `constant`, of its own type.
    pub(crate) fn constant(constant: Constant) -> Self {
        Self {
            constness: Constness::Known(constant),
            sym: Sym::Immediate(constant),
            ..Self::of(constant.ty.value_type())
        }
    }

    /// Its value, when it is a constant the model knows the value of.
    pub(crate) const fn known(&self) -> Option<Constant> {
        match self.constness {
            Constness::Known(constant) => Some(constant),
            Constness::Variable | Constness::Opaque => None,
        }
    }

    /// Whether it is a constant 0 of an integer, float or pointer type (`QCC_SRef_IsNull`).
    pub(crate) fn null(&self) -> bool {
        self.known().is_some_and(Constant::null)
    }

    /// What a cast reads of it: a constant of its own type as a literal, whose nullness the cast
    /// reads (FTEF-19.31), anything else by its type.
    pub(crate) fn source(&self) -> Source<'_> {
        match self.known() {
            Some(constant) if constant.ty.value_type() == self.ty => Source::Literal(Literal {
                ty: constant.ty,
                null: constant.null(),
            }),
            Some(_) | None => Source::Typed(&self.ty),
        }
    }

    /// The name FTEQCC prints for it (`QCC_GetSRefName`, FTEF-27.4): `IMMEDIATE` for an
    /// immediate, its definition's name, or for a temporary the name of its type.
    pub(crate) fn label(&self) -> String {
        match (self.sym, self.name.as_deref()) {
            (Sym::Immediate(_), _) => "IMMEDIATE".to_owned(),
            (_, Some(name)) => name.to_owned(),
            (_, None) => self.ty.fteqcc_name(),
        }
    }

    /// The value of an expression the checker does not follow, on which nothing is reported.
    pub(crate) const fn unknown() -> Self {
        Self::of(Type::Unknown)
    }

    /// How FTEQCC names it as the target of a store (`QCC_GetRefName`): `base.field` for a
    /// field, else its label.
    pub(crate) fn place(&self) -> String {
        match &self.reference {
            Reference::Field { base, field } => format!("{base}.{field}"),
            Reference::Global => self.label(),
        }
    }

    /// The field `field` of the entity `base`, holding a `ty`, which may be assigned to.
    pub(crate) fn field_of(base: &Self, field: String, ty: Type) -> Self {
        Self {
            readonly: false,
            reference: Reference::Field {
                base: base.label(),
                field,
            },
            ..Self::of(ty)
        }
    }
}

/// What a value refers to, as FTEQCC's references are of kinds (`QCC_ref_t::type`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reference {
    /// A definition, temporary or immediate, read directly (`REF_GLOBAL`).
    Global,
    /// An entity's field (`REF_FIELD`): the labels of the entity and of the field.
    Field {
        /// The entity's label.
        base: String,
        /// The field's label.
        field: String,
    },
}

/// Whether a value is a constant (`QCC_SRef_EvalConst`, FTEF-26.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Constness {
    /// Not a constant: no folding reads it.
    Variable,
    /// A constant the model knows the value of.
    Known(Constant),
    /// A constant whose value the model does not follow; nothing that its value decides, folding
    /// or emulation, is checked.
    Opaque,
}

/// A checked function body: the type of each expression, by where it starts and ends in the
/// body's text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypedBody {
    types: HashMap<(usize, usize), Type>,
}

impl TypedBody {
    /// The type of the expression at `span`, if the check typed one there.
    #[must_use]
    pub fn type_at(&self, span: Span) -> Option<&Type> {
        self.types.get(&(span.start, span.end))
    }
}

/// Checks `body`, reporting through `env`, and returns its types.
pub(crate) fn check_body(env: &mut dyn Env, body: &ast::FunctionBody) -> TypedBody {
    let mut checker = expr::Checker::new(env);
    if let Some(block) = body.block() {
        checker.block(&block);
    }
    checker.finish()
}
