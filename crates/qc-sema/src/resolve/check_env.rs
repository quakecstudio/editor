// SPDX-License-Identifier: GPL-3.0-or-later

//! The resolver as the check stage reads it ([`Env`]): one body's references, the module's
//! definitions and types, the flags in force and the reporting under the warning settings.

use std::collections::HashMap;

use qc_diag::{Code, Span};
use qc_lexer::LexFlags;
use qc_preproc::OpcodeTarget;
use qc_syntax::CompilerFlags;

use crate::cast::{Cast, Source};
use crate::check::constant::{Constant, unquoted};
use crate::check::{Constness, Env, ParamFacts, Shape};
use crate::lower::{ArrayDim, LiteralValue, RawLocal};
use crate::resolve::{AccessorFacts, MAX_TYPEDEF_DEPTH, Resolver};
use crate::symbol::{DefId, DefKind, Definition, FileId};
use crate::ty::Type;

/// One body being checked.
pub(in crate::resolve) struct BodyEnv<'r, 'a> {
    pub(in crate::resolve) resolver: &'r mut Resolver<'a>,
    /// The file the body is in.
    pub(in crate::resolve) file: FileId,
    /// Whether it is a member function's.
    pub(in crate::resolve) in_class: bool,
    /// The definition each name of the body resolved to, by where the name starts.
    pub(in crate::resolve) uses: &'r HashMap<usize, DefId>,
    /// The compiler flags in force in the body, from each offset on.
    pub(in crate::resolve) flags: &'r [(usize, CompilerFlags)],
    /// The lexer's flags in force in the body, from each offset on.
    pub(in crate::resolve) lex: &'r [(usize, LexFlags)],
    /// The types the body's casts and `sizeof`s write, by where each starts.
    pub(in crate::resolve) written: &'r [(usize, Type)],
    /// The body's locals.
    pub(in crate::resolve) locals: &'r [RawLocal],
}

impl Env for BodyEnv<'_, '_> {
    fn reference(&self, at: usize) -> Option<DefId> {
        self.uses.get(&at).copied()
    }

    fn local_type(&self, at: usize) -> Option<Type> {
        self.locals
            .iter()
            .find(|local| local.span.start == at)
            .map(|local| self.resolver.resolved_deep(&local.ty, MAX_TYPEDEF_DEPTH))
    }

    fn written_type(&self, at: usize) -> Option<Type> {
        self.written
            .iter()
            .find(|(start, _)| *start == at)
            .map(|(_, ty)| self.resolver.resolved_deep(ty, MAX_TYPEDEF_DEPTH))
    }

    fn definition(&self, def: DefId) -> Option<&Definition> {
        self.resolver.index.def(def)
    }

    fn is_array(&self, def: DefId) -> bool {
        self.resolver
            .facts
            .get(def.0)
            .is_some_and(|f| f.array.is_some())
    }

    fn file_line(&self, span: Span) -> String {
        let location = self.resolver.at(self.file, span);
        self.resolver
            .places
            .get(&location.file)
            .map_or_else(String::new, |place| place.of(location.span.start))
    }

    fn params(&self, def: DefId) -> Option<Vec<ParamFacts>> {
        let facts = self.resolver.facts.get(def.0)?;
        if facts.params.is_empty() {
            return None;
        }
        Some(
            facts
                .params
                .iter()
                .map(|p| ParamFacts {
                    name: p.name.clone(),
                    has_default: p.default.is_some(),
                    optional: p.optional,
                    out: p.out,
                })
                .collect(),
        )
    }

    fn array_length(&self, def: DefId) -> Option<u32> {
        match self.resolver.facts.get(def.0)?.array.as_ref()? {
            ArrayDim::Sized(length) => Some(*length),
            ArrayDim::Unsized | ArrayDim::Unknown => None,
        }
    }

    fn is_local(&self, def: DefId) -> bool {
        self.resolver
            .index
            .def(def)
            .is_some_and(|d| matches!(d.kind, DefKind::Local | DefKind::Param))
    }

    fn constant(&self, def: DefId) -> bool {
        self.resolver.facts.get(def.0).is_some_and(|f| f.constant)
    }

    fn constness(&self, def: DefId) -> Constness {
        if !self.constant(def) {
            return Constness::Variable;
        }
        match self.resolver.literals.get(&def) {
            Some(LiteralValue::Float(bits)) => {
                Constness::Known(Constant::float(f32::from_bits(*bits)))
            }
            Some(LiteralValue::Vector(bits)) => {
                Constness::Known(Constant::vector(bits.map(f32::from_bits)))
            }
            Some(LiteralValue::String(text)) => Constness::Known(Constant::string(text == "\"\"")),
            Some(LiteralValue::Builtin { .. } | LiteralValue::Text(_)) => Constness::Opaque,
            // An enum's item and any other initialised constant whose value is not kept.
            None if self.resolver.index.def(def).is_some_and(|d| d.initialized) => {
                Constness::Opaque
            }
            None => Constness::Variable,
        }
    }

    fn shape(&self, ty: &Type) -> Option<Shape> {
        self.resolver.shape(ty)
    }

    fn flags_at(&self, at: usize) -> CompilerFlags {
        CompilerFlags::at(self.flags, at)
    }

    fn lex_flags_at(&self, at: usize) -> LexFlags {
        let index = self.lex.partition_point(|&(from, _)| from <= at);
        index
            .checked_sub(1)
            .and_then(|i| self.lex.get(i))
            .map(|&(_, flags)| flags)
            .unwrap_or_default()
    }

    fn target_at(&self, at: usize) -> OpcodeTarget {
        let marks = self
            .resolver
            .targets
            .get(self.resolver.unit.0)
            .copied()
            .unwrap_or_default();
        let index = marks.partition_point(|&(from, _)| from <= at);
        index
            .checked_sub(1)
            .and_then(|i| marks.get(i))
            .or_else(|| marks.first())
            .map(|&(_, target)| target)
            .unwrap_or_default()
    }

    fn string_text(&self, def: DefId) -> Option<String> {
        if !self.constant(def) {
            return None;
        }
        match self.resolver.literals.get(&def) {
            Some(LiteralValue::String(text)) => Some(unquoted(text)),
            _ => None,
        }
    }

    fn accessor(&self, ty: &Type, member: &str) -> Option<AccessorFacts> {
        let mut current = self.resolver.resolved(ty);
        for _ in 0..MAX_TYPEDEF_DEPTH {
            let Type::Named(name) = &current else {
                return None;
            };
            if let Some(facts) = self
                .resolver
                .accessors
                .get(&(name.to_string(), member.to_owned()))
            {
                return Some(facts.clone());
            }
            let parent = self.resolver.class_parent_type(&current)?;
            current = parent;
        }
        None
    }

    fn defines(&self, name: &str) -> bool {
        self.resolver.lookup_global(name).is_some()
    }

    fn in_class(&self) -> bool {
        self.in_class
    }

    fn fteqcc(&self) -> bool {
        self.resolver.profile.fteqcc_diagnostics()
    }

    fn report(&mut self, code: Code, span: Span, message: String) {
        self.resolver.settle(span.start);
        let location = self.resolver.at(self.file, span);
        self.resolver.report(code, location, message);
    }

    fn class_member(&self, ty: &Type, name: &str) -> Option<DefId> {
        self.resolver.class_member(ty, name)
    }

    fn lax_same(&self, a: &Type, b: &Type) -> bool {
        self.resolver.lax_same_value(a, b)
    }

    fn typecmp_lax(&self, a: &Type, b: &Type) -> bool {
        self.resolver.typecmp_lax(a, b)
    }

    fn typecmp(&self, a: &Type, b: &Type) -> bool {
        self.resolver.typecmp(a, b)
    }

    fn type_name(&self, ty: &Type) -> String {
        self.resolver.type_name(ty)
    }

    fn struct_member(&self, ty: &Type, name: &str) -> Option<(Type, bool)> {
        self.resolver.struct_member(ty, name)
    }

    fn explicit_cast(&mut self, source: Source<'_>, to: &Type, span: Span) -> bool {
        self.resolver.settle(span.start);
        let location = self.resolver.at(self.file, span);
        let flags = CompilerFlags::at(self.flags, span.start);
        let to = self.resolver.resolved_deep(to, MAX_TYPEDEF_DEPTH);
        let typed;
        let source = match source {
            Source::Typed(ty) => {
                typed = self.resolver.resolved_deep(ty, MAX_TYPEDEF_DEPTH);
                Source::Typed(&typed)
            }
            Source::Literal(_) => source,
        };
        match source.cast_explicit(&to, flags) {
            Cast::Refused => {
                let from = match source {
                    Source::Typed(ty) => self.resolver.type_name(ty),
                    Source::Literal(_) => source.name(),
                };
                let message = format!(
                    "Cannot cast from {from} to {}",
                    self.resolver.type_name(&to)
                );
                self.resolver.report(Code::E0213, location, message);
                true
            }
            Cast::StringPointerDisabled => {
                let message = "pointer<->string casts were disabled.".to_owned();
                self.resolver.report(Code::E0237, location, message);
                false
            }
            Cast::Silent | Cast::Implicit | Cast::Lax => false,
        }
    }

    fn cast(&mut self, source: Source<'_>, to: &Type, span: Span) -> bool {
        self.resolver.settle(span.start);
        let location = self.resolver.at(self.file, span);
        let flags = CompilerFlags::at(self.flags, span.start);
        self.resolver.cast_value(source, to, location, flags)
    }
}
