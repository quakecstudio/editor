// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseEnum

//! Enums and their values (FTE-15, FTEF-8, FTEF-19.10, FTEF-19.36).

use qc_diag::Code;

use crate::cast::{Kind, Source, Supply};
use crate::lower::{EnumItem, LiteralValue, RawDecl, RawValue, ValueKind};
use crate::resolve::{EnumCursor, Resolver};
use crate::symbol::{DefKind, Definition, FileId, Location};
use crate::ty::{SimpleType, Type};

impl<'a> Resolver<'a> {
    /// An enum's name (FTEF-19.10): a constant of the enum's base type whose value is the one
    /// after `=`, a literal or a constant, or else the one before it plus one, doubled for
    /// `enumflags`. `QCC_PR_ParseEnum` makes it with `QCC_PR_DummyDef`, which looks for no
    /// earlier definition (FTEF-17.10).
    pub(in crate::resolve) fn declare_enum_item(
        &mut self,
        file: FileId,
        decl: &'a RawDecl,
        item: &EnumItem,
        location: Location,
    ) {
        self.resolve_uses(file, None, &decl.uses);
        // FTEF-19.37: the base type after `:` is a type.
        if item.first && self.check_type_names(&item.base, &[], decl.span.start, location) {
            return;
        }
        if item.legacy {
            self.report(
                Code::W0224,
                location,
                "legacy enum base type. Use \"enum [class] [name_e]:type\" instead".to_owned(),
            );
        }
        let base = self.resolved(&item.base);
        // FTEF-19.44: a base that is a class, a struct or a union, as the legacy form reads a
        // class's name, is no number: "values for enums of this type must be initialised".
        let class_base = self.enum_redeclared
            && item.enum_name.as_ref().is_some_and(|n| {
                self.has_name_key(&self.class_names, n)
                    || self.struct_defs.contains_key(&self.type_key(n))
            })
            || matches!(&base, Type::Named(n) if self.has_name_key(&self.class_names, n))
            || self.is_struct_type(&base);
        if class_base {
            self.report(
                Code::E0229,
                location,
                "values for enums of this type must be initialised".to_owned(),
            );
            return;
        }
        let numeric = base.fteqcc_kind().is_numeric();
        if item.first {
            self.enum_next = EnumCursor::Start;
        }
        let next = match self.enum_next {
            EnumCursor::Start => Some(if item.flags { 1.0 } else { 0.0 }),
            EnumCursor::Known(next) => Some(next),
            EnumCursor::Unknown => None,
        };
        let value: Option<f64> = match item.value.as_ref() {
            None => {
                if !numeric && base.fteqcc_kind() != Kind::Opaque {
                    self.report(
                        Code::E0229,
                        location,
                        "values for enums of this type must be initialised".to_owned(),
                    );
                    None
                } else {
                    next
                }
            }
            Some(value) => self.enum_value(file, value, item.number, next, &base),
        };
        if item.flags
            && let Some(v) = value
        {
            self.warn_enumflag(v, &base, location);
        }
        // FTEF-19.36: a name an enum gave again with another value is "NAME::ITEM already
        // declared".
        if let (Some(enum_name), Some(v), false) = (&item.enum_name, value, self.enum_redeclared) {
            // FTEF-19.33: the enum is found in any case under `-Fcaseinsens`; the name is
            // compared exactly (`strcmp (acc->fieldname, name)`), so `A` and `a` are two.
            let key = (self.type_key(enum_name), decl.name.clone());
            #[expect(
                clippy::float_cmp,
                reason = "spec FTEF-19.36: FTEQCC compares the constants' values exactly"
            )]
            let differs = self.enum_values.get(&key).is_some_and(|old| *old != v);
            if differs {
                let message = format!("{enum_name}::{} already declared", decl.name);
                self.report(Code::E0251, location, message);
            }
            self.enum_values.entry(key).or_insert(v);
        }
        // A value the model does not compute leaves every later implicit one unknown.
        self.enum_next = value.map_or(EnumCursor::Unknown, |v| {
            if item.flags {
                // FTEF-19.10: `next_i <<= 1;` then `if (!next_i) next_f = next_i = 1;`, where
                // `next_i` holds the value's integer part.
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "spec FTEF-19.10: `next_i = next_f` keeps the value's integer part"
                )]
                let whole = v as i64;
                EnumCursor::Known(if whole.wrapping_shl(1) == 0 {
                    1.0
                } else {
                    v * 2.0
                })
            } else {
                EnumCursor::Known(v + 1.0)
            }
        });
        let literal = value
            .filter(|_| base == Type::Simple(SimpleType::Float))
            .map(|v| {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "spec FTEF-19.10: `QCC_MakeFloatConst(next_f)` stores the value as a float"
                )]
                let single = v as f32;
                LiteralValue::Float(single.to_bits())
            });
        // The same declaration read by the other pass is one definition for navigation.
        if self
            .globals
            .get(&decl.name)
            .into_iter()
            .flatten()
            .any(|id| {
                self.index
                    .def(*id)
                    .is_some_and(|d| d.location == location && d.ty == decl.ty)
            })
        {
            return;
        }
        // FTEF-17.10: a new definition each time, never a redeclaration, which the name's
        // lookups find before the older ones (`Hash_Add`, FTEF-19.33).
        if let Type::Named(strict) = &decl.ty
            && decl.ty != base
        {
            self.strict_enums.insert(strict.to_string(), base.clone());
        }
        let id = self.push_def(Definition {
            name: decl.name.clone(),
            ty: decl.ty.clone(),
            kind: DefKind::Global,
            location,
            owner: None,
            initialized: true,
            member_of: None,
            generated: false,
        });
        if let Some(newer) = self.globals.get_mut(&decl.name)
            && let Some(last) = newer.pop()
        {
            newer.insert(0, last);
        }
        if let Some(facts) = self.fact_mut(id) {
            facts.constant = true;
        }
        if let Some(literal) = literal {
            self.literals.insert(id, literal);
        }
    }

    /// FTEF-19.10: an `enumflags` value that is no whole number, or has more than one bit set,
    /// is FTEQCC's warning.
    pub(in crate::resolve) fn warn_enumflag(
        &mut self,
        value: f64,
        base: &Type,
        location: Location,
    ) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec FTEF-19.10: `i = (longlong)next_f;` truncates the value to an integer"
        )]
        let whole = value as i64;
        #[expect(
            clippy::cast_precision_loss,
            clippy::float_cmp,
            reason = "spec FTEF-19.10: `(double)i != next_f` compares the integer back exactly"
        )]
        let fraction = base.fteqcc_kind() != Kind::Integer && whole as f64 != value;
        let message = if fraction {
            format!("enumflags - {value:.6} not an integer value")
        } else if whole.count_ones() > 1 {
            let bits = u64::from_ne_bytes(whole.to_ne_bytes());
            format!("enumflags - {bits:#x}({whole}) has multiple bits set")
        } else {
            return;
        };
        self.report(Code::W0225, location, message);
    }

    /// The value after an enum name's `=` (FTEF-19.10): converted to the base type by
    /// `QCC_SupplyConversion` (FTEF-22.3), then a number literal's value or a numeric
    /// constant's, and a name that is no constant "is not a compile-time constant". A value
    /// `lax` let through unconverted leaves the name the value it would have had with none,
    /// `next`, as no branch of `QCC_PR_ParseEnum` reads its type.
    pub(in crate::resolve) fn enum_value(
        &mut self,
        file: FileId,
        value: &RawValue,
        number: Option<u32>,
        next: Option<f64>,
        base: &Type,
    ) -> Option<f64> {
        let location = self.at(file, value.span);
        let kind = base.fteqcc_kind();
        let wanted = if kind == Kind::Opaque {
            Kind::Float
        } else {
            kind
        };
        match &value.kind {
            ValueKind::Literal(literal) => {
                match self.supply_value(Source::Literal(*literal), wanted, value, location) {
                    Supply::Converted => number.map(|bits| f64::from(f32::from_bits(bits))),
                    Supply::Lax => next,
                    Supply::Explicit | Supply::Mismatch => None,
                }
            }
            ValueKind::Name(name) => {
                let def = self.lookup_global(name)?;
                let ty = self.resolved(&self.index.def(def)?.ty);
                let supply = self.supply_value(Source::Typed(&ty), wanted, value, location);
                if matches!(supply, Supply::Explicit | Supply::Mismatch) {
                    return None;
                }
                let constant = self.facts.get(def.0).is_some_and(|f| f.constant)
                    && self.index.def(def).is_some_and(|d| d.initialized);
                if !constant {
                    self.report(
                        Code::E0228,
                        location,
                        format!("enum - {name} is not a compile-time constant"),
                    );
                    return None;
                }
                if supply == Supply::Lax {
                    return next;
                }
                match self.literals.get(&def) {
                    Some(LiteralValue::Float(bits)) => Some(f64::from(f32::from_bits(*bits))),
                    Some(_) | None => None,
                }
            }
            ValueKind::Translate(_)
            | ValueKind::Indexed(_)
            | ValueKind::Cast(_)
            | ValueKind::Other => None,
        }
    }
}
