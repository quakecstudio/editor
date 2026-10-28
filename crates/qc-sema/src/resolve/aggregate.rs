// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseInitializerType_Internal

//! A braced initialiser read against its type, as `QCC_PR_ParseInitializerType_Internal`
//! reads it element by element (FTEF-19.47): an array's elements up to its size, a struct's up
//! to its members, a union's one, each member's in turn, and each value as a definition's value
//! is; what is left over is the reader's caller's to read.

use qc_diag::Code;

use crate::lower::{RawAggregate, RawBraces, RawElement, RawValue};
use crate::resolve::{Fatal, Holder, Resolver};
use crate::symbol::FileId;
use crate::ty::{SimpleType, Type};

/// Where an aggregate stands, which decides what reads what it leaves over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::resolve) enum AggregateSite {
    /// A global's initialiser: `QCC_PR_ParseDefs` reads on.
    Global,
    /// A local's initialiser: the body's statements read on.
    Local,
    /// Inside another aggregate, whose loop expects `,` or `}`.
    Nested,
}

/// The definition an aggregate initialises, which its values are read for.
#[derive(Clone, Copy)]
pub(in crate::resolve) struct Initialised<'d> {
    /// Its name.
    pub(in crate::resolve) name: &'d str,
    /// Where it is defined.
    pub(in crate::resolve) holder: Holder,
}

/// A struct member as the reader takes it: its name, type and array sizes.
struct Part {
    name: String,
    ty: Type,
    sizes: Vec<Option<u32>>,
}

impl Resolver<'_> {
    /// FTEF-19.47: the aggregate `aggregate` initialising `def`, of type `ty`, at `site`.
    pub(in crate::resolve) fn check_aggregate(
        &mut self,
        file: FileId,
        aggregate: &RawAggregate,
        ty: &Type,
        def: Initialised<'_>,
        site: AggregateSite,
    ) -> Result<(), Fatal> {
        let element = RawElement::Braces(aggregate.init.clone());
        self.read_element(file, &aggregate.sizes, ty, &element, def, site)
    }

    /// One element against `sizes` (the array dimensions left) and `ty`.
    fn read_element(
        &mut self,
        file: FileId,
        sizes: &[Option<u32>],
        ty: &Type,
        element: &RawElement,
        def: Initialised<'_>,
        site: AggregateSite,
    ) -> Result<(), Fatal> {
        if let Some((size, inner)) = sizes.split_first() {
            // "arrays go recursive": `QCC_PR_Expect("{")`, a string standing for a `char` array.
            let braces = match element {
                RawElement::Braces(braces) => braces,
                RawElement::Value(value) => {
                    if site == AggregateSite::Nested {
                        let (text, span) = element.first();
                        let location = self.at(file, span);
                        self.report(Code::E0204, location, format!("expected {{, found {text}"));
                        return Err(Fatal);
                    }
                    return self.read_value(file, ty, value, def);
                }
            };
            if braces.designated {
                return Ok(());
            }
            let limit = size.and_then(|n| usize::try_from(n).ok());
            return self.read_parts(file, braces, limit, site, |r, _, e| {
                r.read_element(file, inner, ty, e, def, AggregateSite::Nested)
            });
        }
        let ty = self.resolved(ty);
        let braces = match element {
            RawElement::Value(value) => return self.read_value(file, &ty, value, def),
            RawElement::Braces(braces) => braces,
        };
        if ty == Type::Simple(SimpleType::Vector) {
            // "vectors can be treated as an array of 3 floats".
            let float = Type::Simple(SimpleType::Float);
            return self.read_element(file, &[Some(3)], &float, element, def, site);
        }
        // FTEF-19.53: a scalar's value is an expression, which `{` cannot start.
        let scalar = matches!(
            ty,
            Type::Simple(SimpleType::Float | SimpleType::String | SimpleType::Entity)
                | Type::Field(_)
        ) || ty.fteqcc_scalar().is_some();
        if scalar {
            let location = self.at(file, braces.open);
            self.report(Code::E0201, location, "\"{\" - not a name".to_owned());
            return Err(Fatal);
        }
        let Some(struct_def) = self.struct_def_of(&ty) else {
            return Ok(());
        };
        if braces.designated {
            return Ok(());
        }
        if struct_def.parent.is_some() {
            if let Some(first) = braces.elements.first() {
                let location = self.at(file, first.first().1);
                let message = "inherited structs must use designated initialisers".to_owned();
                self.report(Code::E0354, location, message);
                return Err(Fatal);
            }
            return Ok(());
        }
        let members: Vec<Part> = struct_def
            .members
            .iter()
            .filter(|m| !m.is_static && !m.is_virtual)
            .map(|m| Part {
                name: m.name.clone(),
                ty: m.ty.clone(),
                sizes: m.array.iter().map(|s| s.trim().parse().ok()).collect(),
            })
            .collect();
        if struct_def.union
            && let (Some(element), Some(second)) = (braces.elements.get(1), members.get(1))
        {
            // Each member of a union starts at 0, so the second is already initialised.
            let location = self.at(file, element.first().1);
            let message = format!(
                "member {}.{} was already ininitialised",
                struct_def.name, second.name
            );
            self.report(Code::E0355, location, message);
            return Err(Fatal);
        }
        self.read_parts(
            file,
            braces,
            Some(members.len()),
            site,
            |r, at, e| match members.get(at) {
                Some(member) => r.read_element(
                    file,
                    &member.sizes,
                    &member.ty,
                    e,
                    def,
                    AggregateSite::Nested,
                ),
                None => Ok(()),
            },
        )
    }

    /// A value element, cast to `ty` as a definition's value is (FTEF-19.31).
    fn read_value(
        &mut self,
        file: FileId,
        ty: &Type,
        value: &RawValue,
        def: Initialised<'_>,
    ) -> Result<(), Fatal> {
        self.check_value(file, def.name, ty, Some(value), def.holder)
            .map(drop)
    }

    /// The elements of `braces`, of which the reader takes `limit`, each read by `read`; the
    /// first one past the limit is left to what reads on at `site`.
    fn read_parts(
        &mut self,
        file: FileId,
        braces: &RawBraces,
        limit: Option<usize>,
        site: AggregateSite,
        mut read: impl FnMut(&mut Self, usize, &RawElement) -> Result<(), Fatal>,
    ) -> Result<(), Fatal> {
        for (at, part) in braces.elements.iter().enumerate() {
            if limit.is_some_and(|limit| at >= limit) {
                self.left_over(file, part, site);
                return Err(Fatal);
            }
            read(self, at, part)?;
        }
        Ok(())
    }

    /// The element `part` an aggregate did not take, read by what follows it at `site`.
    fn left_over(&mut self, file: FileId, part: &RawElement, site: AggregateSite) {
        let (text, span) = part.first();
        let location = self.at(file, span);
        match site {
            AggregateSite::Nested => {
                self.report(Code::E0204, location, format!("expected }}, found {text}"));
            }
            AggregateSite::Global | AggregateSite::Local => {
                let message = "Missing semicolon at end of definition".to_owned();
                self.report(Code::W0203, location, message);
                if site == AggregateSite::Global {
                    self.report(Code::E0203, location, format!("\"{text}\" is not a type"));
                } else {
                    let message = "Statement does not do anything".to_owned();
                    self.report(Code::W0204, location, message);
                    self.report(Code::E0204, location, "expected ;, found }".to_owned());
                }
            }
        }
    }
}
