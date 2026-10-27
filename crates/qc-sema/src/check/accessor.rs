// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseField, QCC_PR_BuildAccessorRef, QCC_RefToDef, QCC_StoreSRefToRef

//! An accessor's members as FTEQCC reads and stores them (FTEF-10.7): a member is a reference
//! whose read calls its `get` function and whose store calls its `set` function, its index made
//! the indexer's type.

use qc_diag::Code;
use qc_diag::Span;

use crate::check::constant::Constant;
use crate::check::expr::{Checked, Checker, Fatal};
use crate::check::{AccessorRef, Reference, Sym, Value};
use crate::ty::Type;

impl Checker<'_> {
    /// The member `name` of the accessor value `base` (`QCC_PR_BuildAccessorRef`), of the type
    /// of the member the accessor declared first, read-only as `base` is; `None` when the
    /// accessor has no such member.
    pub(super) fn accessor_member(&self, base: &Value, name: &str) -> Option<Value> {
        let facts = self.env.accessor(&base.ty, name)?;
        let ty = facts.ty.clone();
        Some(Value {
            readonly: base.readonly,
            name: base.name.clone(),
            reference: Reference::accessor(facts, name.to_owned(), None),
            ..Value::of(ty)
        })
    }

    /// `base[index]` of an accessor member that takes an index, or of an accessor's indexer;
    /// `None` when `base` is no such reference.
    pub(super) fn accessor_index(&self, base: &Value, index: &Value) -> Option<Value> {
        let reference = match &base.reference {
            Reference::Accessor(accessor)
                if accessor.facts.indexer.is_some() && accessor.index.is_none() =>
            {
                Reference::accessor(
                    accessor.facts.clone(),
                    accessor.name.clone(),
                    Some(index.clone()),
                )
            }
            _ => {
                let indexer = self.env.accessor(&base.ty, "")?;
                Reference::accessor(indexer, String::new(), Some(index.clone()))
            }
        };
        let ty = match &reference {
            Reference::Accessor(accessor) => accessor.facts.ty.clone(),
            _ => Type::Unknown,
        };
        Some(Value {
            readonly: base.readonly,
            name: base.name.clone(),
            reference,
            ..Value::of(ty)
        })
    }

    /// FTEF-10.7: a read of an accessor member calls its `get` function with the accessor and
    /// the index, made the indexer's type (`QCC_SupplyConversion`, an integer 0 with none
    /// given); with no `get` it is the fatal error "Accessor NAME has no get function".
    pub(super) fn accessor_get(
        &mut self,
        value: &Value,
        accessor: &AccessorRef,
        span: Span,
    ) -> Checked {
        if !accessor.facts.get {
            let message = format!("Accessor {} has no get function", accessor.name);
            return self.fatal(Code::E0348, span, message);
        }
        let args = self.accessor_args(value, accessor, span)?;
        self.generate_call(&args, span);
        Ok(Value {
            sym: Sym::Temp,
            ..Value::of(accessor.facts.ty.clone())
        })
    }

    /// FTEF-10.7: a store to an accessor member calls its `set` function with the accessor,
    /// the index and the value, cast to the type the `set` member declares as an argument is;
    /// with no `set` it is the fatal error "Accessor has no set function".
    pub(super) fn accessor_set(
        &mut self,
        lhs: &Value,
        accessor: &AccessorRef,
        stored: &Value,
        span: Span,
    ) -> Result<(), Fatal> {
        if !accessor.facts.set {
            let message = "Accessor has no set function".to_owned();
            return self.fatal(Code::E0349, span, message).map(drop);
        }
        let mut args = self.accessor_args(lhs, accessor, span)?;
        let set_ty = &accessor.facts.set_ty;
        let value = if stored.ty == Type::Unknown || self.env.typecmp(&stored.ty, set_ty) {
            stored.clone()
        } else {
            self.cast_to(stored, set_ty, span)?
        };
        args.push(value);
        self.generate_call(&args, span);
        Ok(())
    }

    /// The accessor and its index, the arguments a `get` or `set` function takes first.
    fn accessor_args(
        &mut self,
        base: &Value,
        accessor: &AccessorRef,
        span: Span,
    ) -> Result<Vec<Value>, Fatal> {
        let mut args = vec![Value {
            reference: Reference::Global,
            ..base.clone()
        }];
        if let Some(indexer) = &accessor.facts.indexer {
            let index = match &accessor.index {
                Some(index) => self.supplied(index, indexer, span)?,
                None => Value::constant(Constant::int(0)),
            };
            args.push(index);
        }
        Ok(args)
    }
}
