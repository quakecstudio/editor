// SPDX-License-Identifier: GPL-3.0-or-later

//! The resolver as the check stage reads it ([`Env`]): one body's references, the module's
//! definitions and types, the flags in force and the reporting under the warning settings.

use std::collections::HashMap;

use qc_diag::{Code, Span};
use qc_lexer::LexFlags;
use qc_preproc::OpcodeTarget;
use qc_syntax::CompilerFlags;

use crate::cast::Source;
use crate::check::constant::Constant;
use crate::check::{Constness, Env, Shape};
use crate::lower::LiteralValue;
use crate::resolve::Resolver;
use crate::symbol::{DefId, Definition, FileId};
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
}

impl Env for BodyEnv<'_, '_> {
    fn reference(&self, at: usize) -> Option<DefId> {
        self.uses.get(&at).copied()
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

    fn constness(&self, def: DefId) -> Constness {
        let constant = self.resolver.facts.get(def.0).is_some_and(|f| f.constant);
        if !constant {
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

    fn type_name(&self, ty: &Type) -> String {
        self.resolver.type_name(ty)
    }

    fn struct_member(&self, ty: &Type, name: &str) -> Option<(Type, bool)> {
        self.resolver.struct_member(ty, name)
    }

    fn cast(&mut self, source: Source<'_>, to: &Type, span: Span) -> bool {
        self.resolver.settle(span.start);
        let location = self.resolver.at(self.file, span);
        let flags = CompilerFlags::at(self.flags, span.start);
        self.resolver.cast_value(source, to, location, flags)
    }
}
