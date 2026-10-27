// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseFunctionCall, QCC_PR_GenerateFunctionCallRef

//! Calls as FTEQCC checks them (FTEF-32): the callee, the intrinsics a function with no
//! parameters stands for, each argument cast to its parameter's type, too many and too few
//! arguments, default values, and `out` arguments.

use qc_diag::{Code, Span};
use qc_preproc::Op;
use qc_syntax::ast::{self, AstNode as _};

use crate::check::constant::Constant;
use crate::check::etype::Etype;
use crate::check::expr::{Checked, Checker, Fatal};
use crate::check::{ParamFacts, Reference, Value};
use crate::ty::{SimpleType, Type};

/// `MAX_PARMS`: the parameters a call passes in the parameter globals.
const MAX_PARMS: usize = 8;
/// `MAX_PARMS + MAX_EXTRA_PARMS`: the most arguments a call may pass.
const MAX_ARGUMENTS: usize = 136;

impl Checker<'_> {
    /// FTEF-32: the call `call`.
    pub(super) fn call(&mut self, call: &ast::CallExpr) -> Checked {
        let span = call.syntax().span();
        let args: Vec<ast::Expr> = call.args().map(|a| a.args().collect()).unwrap_or_default();
        // An intrinsic no definition names, which FTEQCC declares as it is called.
        if let Some(ast::Expr::Name(name)) = call.callee()
            && let Some(token) = name.name()
            && self.env.reference(token.span().start).is_none()
            && self.env.fteqcc()
            && let Some(value) = self.intrinsic(&token.text_lossless(), &args, span)?
        {
            return Ok(value);
        }
        let callee = self.expr_of(call.callee())?;
        let callee_span = call.callee().map_or(span, |c| c.syntax().span());
        let method = match &callee.reference {
            Reference::Field(field) => {
                field.field.contains("::")
                    || field
                        .def
                        .and_then(|def| self.env.definition(def))
                        .is_some_and(|d| d.member_of.is_some())
            }
            _ => false,
        };
        let func = self.read(callee, callee_span)?;
        if func.ty == Type::Unknown || !self.env.fteqcc() {
            for arg in &args {
                self.value(Some(arg.clone()))?;
            }
            return Ok(Value::unknown());
        }
        let function = match self.env.shape(&func.ty).map(|shape| shape.etype) {
            Some(Etype::Function | Etype::Variant) => match &func.ty {
                Type::Function(function) => Some(function.as_ref().clone()),
                _ => None,
            },
            Some(_) => return self.fatal(Code::E0256, callee_span, "not a function".to_owned()),
            None => None,
        };
        let name = func.label();
        let params = func
            .sym
            .def()
            .and_then(|def| self.env.params(def))
            .unwrap_or_else(|| {
                function.as_ref().map_or_else(Vec::new, |f| {
                    f.params.iter().map(|_| ParamFacts::default()).collect()
                })
            });
        let Some(function) = function else {
            // A `variant` takes any arguments.
            for arg in &args {
                self.value(Some(arg.clone()))?;
            }
            return Ok(Value::of(Type::Named("__variant".into())));
        };
        if !method
            && function.params.is_empty()
            && !function.variadic
            && let Some(value) = self.intrinsic(&name, &args, span)?
        {
            return Ok(value);
        }
        let mut passed = Vec::new();
        let mut values = Vec::new();
        for (at, arg) in args.iter().enumerate() {
            let arg_span = arg.syntax().span();
            if at >= MAX_ARGUMENTS {
                let message = format!("More than {MAX_ARGUMENTS} parameters");
                return self.fatal(Code::E0338, arg_span, message);
            }
            let reference = self.expr(arg)?;
            let extra = function.variadic;
            if extra && at >= MAX_PARMS {
                let message = format!("More than {MAX_PARMS} parameters on varargs function");
                self.env.report(Code::W0251, arg_span, message);
            } else if !extra && at >= function.params.len() {
                let message = format!(
                    "too many parameters on call to {name}, argument {} will be ignored",
                    reference.place()
                );
                self.env.report(Code::W0252, arg_span, message);
            }
            let mut value = self.read(reference.clone(), arg_span)?;
            if let Some(param) = function.params.get(at)
                && value.ty != Type::Unknown
                && !self.env.typecmp(&value.ty, param)
            {
                value = self.cast_to(&value, param, arg_span)?;
            }
            passed.push(reference);
            values.push(value);
        }
        let mut count = args.len();
        while count < params.len()
            && params
                .get(count)
                .is_some_and(|p| p.has_default && !p.optional)
        {
            count = count.saturating_add(1);
        }
        let expected = if params.get(count).is_some_and(|p| p.optional) {
            count
        } else {
            function.params.len()
        };
        if count < expected {
            let message = match params.get(count).and_then(|p| p.name.as_deref()) {
                Some(param) => {
                    format!("too few parameters on call to {name}, {param} will be UNDEFINED")
                }
                None => format!("too few parameters on call to {name}"),
            };
            self.env.report(Code::W0253, span, message);
        }
        self.verify_arguments(&name, &values, span);
        self.generate_call(&values, span);
        // `out` arguments are stored back after the call (`QCC_PR_GenerateFunctionCallRef`).
        for (reference, param) in passed.iter().zip(&params) {
            if !param.out {
                continue;
            }
            if reference.readonly {
                let message = "Unable to write to read-only out argument".to_owned();
                self.env.report(Code::E0339, span, message);
                continue;
            }
            let returned = Value::of(reference.ty.clone());
            self.store_reference(reference, &returned, span)?;
        }
        Ok(Value::of(function.ret.clone()))
    }

    /// FTEF-34.9: the statements a call generates, each argument stored to its parameter by its
    /// words, `OP_STORE_V`, `OP_STORE_I64` where the target has it, else `OP_STORE_F`, then
    /// `OP_CALLn`; only at global scope do they matter.
    fn generate_call(&mut self, values: &[Value], span: Span) {
        if !self.global {
            return;
        }
        let target = self.env.target_at(span.start);
        for value in values {
            let store = match self.env.layout(&value.ty).map(|l| l.words) {
                Some(3) => Op::StoreV,
                Some(2) if target.valid(Op::StoreI64) => Op::StoreI64,
                Some(2) => Op::StoreV,
                _ => Op::StoreF,
            };
            self.generate(store, span);
        }
        let calls = [
            Op::Call0,
            Op::Call1,
            Op::Call2,
            Op::Call3,
            Op::Call4,
            Op::Call5,
            Op::Call6,
            Op::Call7,
            Op::Call8,
        ];
        let call = calls.get(values.len()).copied().unwrap_or(Op::Call8);
        self.generate(call, span);
    }

    /// FTEF-32.2: the intrinsics a call of a function with no parameters stands for, by the
    /// function's name: `_`, which takes only a string immediate; `random` and `randomv`, whose
    /// arguments are made floats; `alloca`, whose size is made an integer; `va_arg`; and
    /// `used_sound`, `used_model`, which take a string immediate. `None` for any other name.
    fn intrinsic(
        &mut self,
        name: &str,
        args: &[ast::Expr],
        span: Span,
    ) -> Result<Option<Value>, Fatal> {
        let string = || Value::of(Type::Simple(SimpleType::String));
        match name {
            "_" => {
                let immediate = args.first().is_some_and(is_string_literal);
                if !immediate {
                    let message = "_() intrinsic accepts only a string immediate".to_owned();
                    return self.fatal(Code::E0340, span, message).map(Some);
                }
                Ok(Some(string()))
            }
            "random" | "randomv" => {
                for (at, arg) in args.iter().take(2).enumerate() {
                    let value = self.value(Some(arg.clone()))?;
                    self.supplied(
                        &value,
                        &Type::Simple(SimpleType::Float),
                        arg.syntax().span(),
                    )?;
                    if self.etype_of(&value.ty).is_some_and(|e| e != Etype::Float)
                        && self.etype_of(&value.ty) != Some(Etype::Integer)
                    {
                        let message = format!("type mismatch on parm {}", at.saturating_add(1));
                        return self.fatal(Code::E0340, span, message).map(Some);
                    }
                }
                let ty = if name == "random" {
                    SimpleType::Float
                } else {
                    SimpleType::Vector
                };
                Ok(Some(Value::of(Type::Simple(ty))))
            }
            "alloca" => {
                if let Some(arg) = args.first() {
                    let size = self.value(Some(arg.clone()))?;
                    let size = self.supplied(&size, &Type::Named("__int32".into()), span)?;
                    let three = Value::constant(Constant::int(3));
                    let four = Value::constant(Constant::int(4));
                    let rounded = self.emit(qc_preproc::Op::AddI, &size, &three, span)?;
                    self.emit(qc_preproc::Op::DivI, &rounded, &four, span)?;
                }
                Ok(Some(Value::of(Type::Pointer(
                    Type::Simple(SimpleType::Void).into(),
                ))))
            }
            "used_sound" | "used_model" => {
                if !args.first().is_some_and(is_string_literal) {
                    let message =
                        format!("Argument to {name} intrinsic was not a string immediate.");
                    self.env.report(Code::E0340, span, message);
                }
                Ok(Some(string()))
            }
            _ => Ok(None),
        }
    }

    /// `QCC_SupplyConversion(value, to, true)` (FTEF-22.3): `value` converted to the basic type
    /// of `to`, or the fatal error "Implicit type mismatch. Needed A, got B." for a type with no
    /// conversion.
    pub(super) fn supplied(&mut self, value: &Value, to: &Type, span: Span) -> Checked {
        if value.ty == Type::Unknown {
            return Ok(value.clone());
        }
        if self.env.supply(&value.ty, to, span) {
            return Err(Fatal);
        }
        let converted = match self.etype_of(to) {
            Some(etype) => self.converted(value, etype, span)?,
            None => value.clone(),
        };
        Ok(Value {
            ty: to.clone(),
            ..converted
        })
    }
}

/// Whether `expr` is a string literal.
fn is_string_literal(expr: &ast::Expr) -> bool {
    matches!(expr, ast::Expr::Literal(l) if l.kind() == Some(ast::LiteralKind::String))
}
