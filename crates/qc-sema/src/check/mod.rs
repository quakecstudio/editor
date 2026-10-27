// SPDX-License-Identifier: GPL-3.0-or-later

//! Checking function bodies: the type of every expression and the diagnostics of expressions
//! and statements, as FTEQCC's `QCC_PR_Expression` and `QCC_PR_ParseStatement` and `qcc`'s
//! `PR_Expression` and `PR_ParseStatement` check them (`docs/design/semantic-model.md`).
//!
//! The resolver runs the check of each body where it resolves the body, so the definitions and
//! types the check sees are those declared before the body in compilation order, as in the
//! one-pass compilers. It reads them through [`Env`], which the resolver implements; the check
//! knows nothing else of resolution.

pub(crate) mod convert;
pub(crate) mod etype;
mod expr;
pub(crate) mod opcodes;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use qc_diag::{Code, Span};
use qc_lexer::LexFlags;
use qc_syntax::CompilerFlags;
use qc_syntax::ast;

use crate::cast::{Literal, Source};
use crate::symbol::{DefId, Definition};
use crate::ty::Type;

use etype::Etype;

/// What the check stage reads of the module around the body it checks.
pub(crate) trait Env {
    /// The definition the name written at offset `at` of the body's text resolved to.
    fn reference(&self, at: usize) -> Option<DefId>;
    /// A definition of the module.
    fn definition(&self, def: DefId) -> Option<&Definition>;
    /// What `ty` is at bottom, through typedefs, enums, accessors and truth values; `None` for
    /// a type the model does not follow, on which nothing is checked.
    fn shape(&self, ty: &Type) -> Option<Shape>;
    /// The compiler flags in force at offset `at` (FTEF-21.1).
    fn flags_at(&self, at: usize) -> CompilerFlags;
    /// The lexer's flags in force at offset `at` (FTEF-12.12).
    fn lex_flags_at(&self, at: usize) -> LexFlags;
    /// Whether the body is a member function's (`pr_classtype`).
    fn in_class(&self) -> bool;
    /// Whether the module follows FTEQCC (PRJ-2.8).
    fn fteqcc(&self) -> bool;
    /// Reports a diagnostic at `span` of the body's text, under the warning settings in force
    /// there.
    fn report(&mut self, code: Code, span: Span, message: String);
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
    /// The literal it is, which a cast reads (`QCC_SRef_IsNull`, FTEF-19.31).
    pub(crate) literal: Option<Literal>,
}

impl Value {
    /// A value of `ty` that is no literal.
    pub(crate) const fn of(ty: Type) -> Self {
        Self { ty, literal: None }
    }

    /// Whether it is a constant 0 of an integer, float or pointer type (`QCC_SRef_IsNull`).
    pub(crate) fn null(&self) -> bool {
        self.literal.is_some_and(|l| l.null)
    }

    /// What a cast reads of it.
    pub(crate) fn source(&self) -> Source<'_> {
        self.literal
            .map_or(Source::Typed(&self.ty), Source::Literal)
    }

    /// The value of an expression the checker does not follow, on which nothing is reported.
    pub(crate) const fn unknown() -> Self {
        Self::of(Type::Unknown)
    }
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
