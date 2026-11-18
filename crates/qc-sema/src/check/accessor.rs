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
use crate::ir;
use crate::ty::{SimpleType, Type};

impl Checker<'_> {
    /// The member `name` of the accessor value `base` (`QCC_PR_BuildAccessorRef`), of the type
    /// of the member the accessor declared first, read-only as `base` is; `None` when the
    /// accessor has no such member. The accessor is read first, `QCC_RefToDef(lhs, true)`: one
    /// that is itself an accessor's member is the value its `get` function returned.
    pub(super) fn accessor_member(
        &mut self,
        base: &Value,
        name: &str,
        span: Span,
    ) -> Result<Option<Value>, Fatal> {
        let Some(facts) = self.env.accessor(&base.ty, name) else {
            return Ok(None);
        };
        let ty = facts.ty.clone();
        let read = self.accessor_base(base, span)?;
        Ok(Some(Value {
            readonly: base.readonly,
            name: base.name.clone(),
            reference: Reference::accessor(facts, name.to_owned(), None, read),
            ..Value::of(ty)
        }))
    }

    /// The accessor `base` as its functions take it: read, when it is an accessor's member.
    fn accessor_base(&mut self, base: &Value, span: Span) -> Checked {
        match base.reference {
            Reference::Accessor(_) => self.read(base.clone(), span),
            _ => Ok(base.clone()),
        }
    }

    /// `base[index]` of an accessor member that takes an index, or of an accessor's indexer;
    /// `None` when `base` is no such reference.
    pub(super) fn accessor_index(
        &mut self,
        base: &Value,
        index: &Value,
        span: Span,
    ) -> Result<Option<Value>, Fatal> {
        let reference = match &base.reference {
            Reference::Accessor(accessor)
                if accessor.facts.indexer.is_some() && accessor.index.is_none() =>
            {
                Reference::accessor(
                    accessor.facts.clone(),
                    accessor.name.clone(),
                    Some(index.clone()),
                    accessor.base.clone(),
                )
            }
            _ => {
                let Some(indexer) = self.env.accessor(&base.ty, "") else {
                    return Ok(None);
                };
                let read = self.accessor_base(base, span)?;
                Reference::accessor(indexer, String::new(), Some(index.clone()), read)
            }
        };
        let ty = match &reference {
            Reference::Accessor(accessor) => accessor.facts.ty.clone(),
            _ => Type::Unknown,
        };
        Ok(Some(Value {
            readonly: base.readonly,
            name: base.name.clone(),
            reference,
            ..Value::of(ty)
        }))
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
        let call = self.accessor_call(accessor.facts.getter.as_deref(), &args, None, span);
        Ok(Value {
            sym: Sym::Temp,
            returned: call.is_some(),
            call,
            ..Value::of(accessor.facts.ty.clone())
        })
    }

    /// FTEF-10.14: in a function, the statements of the call a read or a store of an accessor
    /// member makes, `QCC_PR_GenerateFunctionCallSref` of the member's function `function` with
    /// `args`, as a call the program writes is (FTE-18.12); the call's index. A member with no
    /// function the model knows leaves the body unfollowed.
    fn accessor_call(
        &mut self,
        function: Option<&str>,
        args: &[Value],
        kept_value: Option<usize>,
        span: Span,
    ) -> Option<u32> {
        if self.global {
            return None;
        }
        let callee = function.and_then(|name| self.env.helper(name));
        let ty = callee
            .and_then(|def| self.env.definition(def))
            .map(|definition| definition.ty.clone());
        let (Some(callee), Some(Type::Function(function))) = (callee, ty) else {
            self.followed = false;
            return None;
        };
        let defaulted = vec![false; args.len()];
        self.call_statements(
            &function,
            args.len(),
            (Some(ir::Name::Def(callee, 0)), args, &defaulted),
            (None, &[], kept_value),
            span,
        );
        let call = self.last_made();
        let words = self.env.layout(&function.ret).map_or(1, |l| l.words);
        if words > 0 && function.ret != Type::Simple(SimpleType::Void) {
            self.live_return = Some(words);
            self.live_call = call;
        }
        call
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
        // `if (readable) QCC_UnFreeTemp(source);`: the value stored is the assignment's.
        let kept = args.len().checked_sub(1);
        self.accessor_call(accessor.facts.setter.as_deref(), &args, kept, span);
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
            name: base.name.clone(),
            readonly: base.readonly,
            ..accessor.base.clone()
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
