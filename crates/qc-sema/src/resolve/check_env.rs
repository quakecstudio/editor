// SPDX-License-Identifier: GPL-3.0-or-later

//! The resolver as the check stage reads it ([`Env`]): one body's references, the module's
//! definitions and types, the flags in force and the reporting under the warning settings.

use std::collections::HashMap;

use qc_diag::{Code, Span};
use qc_lexer::LexFlags;
use qc_preproc::OpcodeTarget;
use qc_syntax::CompilerFlags;
use qc_syntax::c_scalar::CScalar;

use crate::cast::{Cast, Kind, Source, Supply};
use crate::check::constant::{Constant, unquoted};
use crate::check::etype::Etype;
use crate::check::{
    Constness, CvarValue, Env, FunctionFacts, ParamFacts, PathList, Shape, StructMemberFacts,
};
use crate::lower::{ArrayDim, LiteralValue, RawLocal, top_level_kind};
use crate::resolve::{AccessorFacts, Layout, MAX_TYPEDEF_DEPTH, Resolver};
use crate::symbol::{DefId, DefKind, Definition, FileId};
use crate::ty::Type;

/// One body being checked.
pub(in crate::resolve) struct BodyEnv<'r, 'a> {
    pub(in crate::resolve) resolver: &'r mut Resolver<'a>,
    /// The file the body is in.
    pub(in crate::resolve) file: FileId,
    /// The function whose body it is, or the definition whose initialiser or size it is; none
    /// for a size read before its definition is made (FTEF-19.62).
    pub(in crate::resolve) owner: Option<DefId>,
    /// The diagnostics reported before the body's own.
    pub(in crate::resolve) reported: usize,
    /// Whether it is a member function's.
    pub(in crate::resolve) in_class: bool,
    /// Whether the body is a member function's written in its class (FTEF-17.16).
    pub(in crate::resolve) inline_member: bool,
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

impl BodyEnv<'_, '_> {
    /// The file name and line FTEQCC's messages give for offset `span.start` of the body
    /// (`s_filen`, `pr_source_line`): the source file's, as `#pragma file` and `#pragma line`
    /// change them (FTEF-14.43).
    fn message_place(&self, span: Span) -> Option<(String, u32)> {
        let location = self.resolver.at(self.file, span);
        let place = self.resolver.places.get(&location.file)?;
        let line = i64::try_from(place.line(location.span.start)).ok()?;
        let marks = self
            .resolver
            .message_places
            .get(self.resolver.unit.0)
            .copied()
            .unwrap_or_default();
        let index = marks.partition_point(|(from, _, _)| *from <= span.start);
        let mark = index.checked_sub(1).and_then(|i| marks.get(i));
        let name = mark.map_or_else(|| place.name.clone(), |(_, name, _)| name.clone());
        let line = line.saturating_add(mark.map_or(0, |(_, _, delta)| *delta));
        Some((name, u32::try_from(line).unwrap_or(0)))
    }

    /// FTEF-6.12: a struct or union is cast to and from nothing but its own type, "Cannot cast
    /// from X to Y" reported at `span`, and a `variant` either way, which
    /// `QCC_SupplyConversionForAssignment` never refuses (FTEF-19.69); whether the cast is so
    /// refused.
    fn struct_cast_refused(&mut self, source: Source<'_>, to: &Type, span: Span) -> bool {
        if !self.resolver.profile.type_declarations() {
            return false;
        }
        let to = self.resolver.resolved_deep(to, MAX_TYPEDEF_DEPTH);
        let from = match source {
            Source::Typed(ty) => {
                let ty = self.resolver.resolved_deep(ty, MAX_TYPEDEF_DEPTH);
                let structs =
                    self.resolver.is_struct_type(&ty) || self.resolver.is_struct_type(&to);
                let variant = |t: &Type| {
                    self.resolver
                        .shape(t)
                        .is_some_and(|s| s.etype == Etype::Variant)
                };
                if !structs || self.resolver.typecmp(&ty, &to) || variant(&ty) || variant(&to) {
                    return false;
                }
                self.resolver.type_name(&ty)
            }
            Source::Literal(_) if self.resolver.is_struct_type(&to) => source.name(),
            Source::Literal(_) => return false,
        };
        self.resolver.settle(span.start);
        let location = self.resolver.at(self.file, span);
        let message = format!(
            "Cannot cast from {from} to {}",
            self.resolver.type_name(&to)
        );
        self.resolver.report(Code::E0213, location, message);
        true
    }
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

    fn autocvar(&mut self, name: &str, ty: &Type, value: CvarValue, span: Span) -> bool {
        self.resolver.settle(span.start);
        let location = self.resolver.at(self.file, span);
        // `QCC_PR_GetSRef(e.cast, autocvarname, NULL, true, 0, GDF_USED)`.
        let global = format!("autocvar_{name}");
        let def = self
            .resolver
            .declare_global(&global, ty, top_level_kind(ty), location, None);
        let Some(facts) = self.resolver.index.defs.get_mut(def.0) else {
            return false;
        };
        if !facts.initialized {
            facts.initialized = true;
            if let Some(literal) = value.literal() {
                self.resolver.literals.insert(def, literal);
            }
            return false;
        }
        let before = self
            .resolver
            .literals
            .get(&def)
            .map_or(CvarValue::Unknown, CvarValue::of_literal);
        if before == CvarValue::Unknown || value == CvarValue::Unknown || before == value {
            return false;
        }
        let message = format!("autocvar {name} was already initialised with another value");
        self.resolver.report(Code::E0368, location, message);
        true
    }

    fn record_path(&mut self, list: PathList, path: &str, span: Span) {
        if path.is_empty() || !self.resolver.paths.insert((list, path.to_owned())) {
            return;
        }
        if path.contains('\\') {
            self.resolver.settle(span.start);
            let location = self.resolver.at(self.file, span);
            let message = format!("backslashes in path names are non-portable - {path}");
            self.resolver.report(Code::W0317, location, message);
        }
    }

    fn local_static(&self, at: usize) -> bool {
        self.locals
            .iter()
            .any(|local| local.span.start == at && local.is_static())
    }

    fn size_local(&mut self, at: usize, size: u32) {
        let place = self.resolver.at(self.file, Span::new(at, at));
        let found = self.resolver.index.defs.iter().position(|d| {
            d.kind == DefKind::Local
                && d.location.file == place.file
                && d.location.span.start == place.span.start
        });
        if let Some(facts) = found.and_then(|at| self.resolver.facts.get_mut(at)) {
            facts.array = Some(ArrayDim::Sized(size));
        }
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

    fn enum_items(&self, ty: &Type) -> Vec<(String, Option<Constant>)> {
        let Type::Named(name) = ty else {
            return Vec::new();
        };
        if self
            .resolver
            .named(&self.resolver.strict_enums, name)
            .is_none()
        {
            return Vec::new();
        }
        let mut items = Vec::new();
        for (at, def) in self.resolver.index.defs.iter().enumerate() {
            if def.ty != *ty || def.kind != DefKind::Global {
                continue;
            }
            let item = def.name.rsplit("::").next().unwrap_or(&def.name).to_owned();
            if items.iter().any(|(known, _): &(String, _)| *known == item) {
                continue;
            }
            let value = match self.constness(DefId(at)) {
                Constness::Known(constant) => Some(constant),
                Constness::Opaque | Constness::Variable => None,
            };
            items.push((item, value));
        }
        items
    }

    fn layout(&self, ty: &Type) -> Option<Layout> {
        self.resolver.layout(ty)
    }

    fn failed(&self) -> bool {
        self.resolver
            .index
            .diagnostics
            .get(self.reported..)
            .is_some_and(|reported| {
                reported
                    .iter()
                    .any(|d| d.diagnostic.is_error() && d.diagnostic.code != Code::E0125)
            })
    }

    fn line(&self, span: Span) -> u32 {
        self.message_place(span).map_or(0, |(_, line)| line)
    }

    fn function(&self) -> FunctionFacts {
        let def = self.owner.and_then(|owner| self.resolver.index.def(owner));
        let (ret, variadic) = match def.map(|d| self.resolver.resolved(&d.ty)) {
            Some(Type::Function(function)) => (function.ret.clone(), function.variadic),
            Some(_) | None => (Type::Unknown, false),
        };
        FunctionFacts {
            // `pr_scope->name`, a member function's `CLASS::NAME` (FTEF-21.23).
            name: def.map_or_else(String::new, |d| {
                match d.member_of.and_then(|class| self.resolver.index.def(class)) {
                    Some(class) => format!("{}::{}", class.name, d.name),
                    None => d.name.clone(),
                }
            }),
            ret,
            accumulate: self
                .owner
                .and_then(|owner| self.resolver.facts.get(owner.0))
                .is_some_and(|f| f.accumulate),
            variadic,
        }
    }

    fn supply(&mut self, ty: &Type, wanted: &Type, span: Span) -> bool {
        self.resolver.settle(span.start);
        let location = self.resolver.at(self.file, span);
        let flags = CompilerFlags::at(self.flags, span.start);
        let ty = self.resolver.resolved_deep(ty, MAX_TYPEDEF_DEPTH);
        let enum_shape = |t: &Type| {
            self.resolver
                .shape(t)
                .is_some_and(|s| s.etype == Etype::Enum)
        };
        // A strict enum is its own basic type, `ev_enum`, to which nothing else converts.
        if enum_shape(wanted) && !enum_shape(&ty) {
            let flags_lax = flags.has(qc_preproc::FteFlag::Laxcasts);
            let (code, text) = if flags.has(qc_preproc::FteFlag::Typeexplicit) {
                (Code::E0236, "Automatic type conversions disabled.")
            } else if flags_lax {
                (Code::W0230, "Implicit type mismatch.")
            } else {
                (Code::E0227, "Implicit type mismatch.")
            };
            let from = self
                .resolver
                .shape(&ty)
                .map_or("variant", |s| s.etype.basic_name());
            let message = format!("{text} Needed enum, got {from}.");
            self.resolver.report(code, location, message);
            return code != Code::W0230;
        }
        let wanted = Kind::of(&self.resolver.resolved_deep(wanted, MAX_TYPEDEF_DEPTH));
        let source = Source::Typed(&ty);
        let (code, text) = match source.supply(wanted, flags) {
            Supply::Converted => return false,
            Supply::Explicit => (Code::E0236, "Automatic type conversions disabled."),
            Supply::Lax => (Code::W0230, "Implicit type mismatch."),
            Supply::Mismatch => (Code::E0227, "Implicit type mismatch."),
        };
        let message = format!(
            "{text} Needed {}, got {}.",
            wanted.basic_name(),
            source.kind().basic_name()
        );
        self.resolver.report(code, location, message);
        code != Code::W0230
    }

    fn file_line(&self, span: Span) -> String {
        self.message_place(span)
            .map_or_else(String::new, |(name, line)| format!("{name}:{line}"))
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
        if let Some(constant) = self.resolver.integer_items.get(&def) {
            return Constness::Known(*constant);
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
        self.resolver.target_at(at)
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

    fn is_accessor(&self, ty: &Type) -> bool {
        self.resolver.is_accessor_type(ty)
    }

    fn has_accessors(&self, ty: &Type) -> bool {
        let mut current = self.resolver.resolved(ty);
        for _ in 0..MAX_TYPEDEF_DEPTH {
            let Type::Named(name) = &current else {
                return false;
            };
            if self
                .resolver
                .accessors
                .keys()
                .any(|(owner, _)| owner == name.as_ref())
            {
                return true;
            }
            let Some(parent) = self.resolver.class_parent_type(&current) else {
                return false;
            };
            current = parent;
        }
        false
    }

    fn defines(&self, name: &str) -> bool {
        self.resolver.lookup_global(name).is_some()
    }

    fn in_class(&self) -> bool {
        self.in_class
    }

    fn this_type(&self) -> Option<Type> {
        let class = self.resolver.index.def(self.owner?)?.member_of?;
        let name = &self.resolver.index.def(class)?.name;
        Some(Type::Named(name.as_str().into()))
    }

    fn fteqcc(&self) -> bool {
        self.resolver.profile.fteqcc_diagnostics()
    }

    fn report(&mut self, code: Code, span: Span, message: String) {
        self.resolver.settle(span.start);
        let location = self.resolver.at(self.file, span);
        self.resolver.report(code, location, message);
    }

    fn class_member(&self, ty: &Type, name: &str, at: usize) -> Option<DefId> {
        let member = self.resolver.class_member(ty, name)?;
        (!self.inline_member || self.resolver.declared_by(member, self.file, at)).then_some(member)
    }

    fn class_name(&self, ty: &Type) -> Option<String> {
        let class = self.resolver.class_of(ty)?;
        self.resolver.index.def(class).map(|def| def.name.clone())
    }

    fn class_undefined(&self, ty: &Type) -> bool {
        // `parentclass` is set where the class is defined, its parent the entity by default.
        self.resolver.class_of(ty).is_some_and(|class| {
            self.resolver
                .classes
                .get(&class)
                .is_none_or(|c| c.shape.is_none() && c.parent.is_none())
        })
    }

    fn type_named(&self, def: DefId) -> Option<Type> {
        if !self.resolver.type_defs.contains(&def) {
            return None;
        }
        let def = self.resolver.index.def(def)?;
        Some(Type::Named(def.name.as_str().into()))
    }

    fn struct_of_no_size(&self, ty: &Type) -> bool {
        self.resolver.struct_of_no_size(ty)
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

    fn struct_member(&self, ty: &Type, name: &str) -> Option<StructMemberFacts> {
        self.resolver.struct_member(ty, name)
    }

    fn explicit_cast(&mut self, source: Source<'_>, to: &Type, span: Span) -> bool {
        if self.struct_cast_refused(source, to, span) {
            return true;
        }
        self.resolver.settle(span.start);
        let location = self.resolver.at(self.file, span);
        let flags = CompilerFlags::at(self.flags, span.start);
        let to = self.resolver.resolved_deep(to, MAX_TYPEDEF_DEPTH);
        // FTEF-23.8: a cast to a boolean is its logical truth, with no message.
        if matches!(to.fteqcc_scalar(), Some(CScalar::Bool | CScalar::BFloat)) {
            return false;
        }
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
        if self.struct_cast_refused(source, to, span) {
            return true;
        }
        self.resolver.settle(span.start);
        let location = self.resolver.at(self.file, span);
        let flags = CompilerFlags::at(self.flags, span.start);
        self.resolver.cast_value(source, to, location, flags)
    }
}
