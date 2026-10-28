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

use crate::check::assign::{lexed_string, string_bytes};
use crate::check::constant::Constant;
use crate::check::etype::Etype;
use crate::check::expr::{Checked, Checker, Fatal};
use crate::check::reference::expected_after;
use crate::check::term::string_immediate;
use crate::check::{Constness, CvarValue, ParamFacts, PathList, Reference, Value};
use crate::ty::{FunctionType, SimpleType, Type};

/// `MAX_PARMS`: the parameters a call passes in the parameter globals.
const MAX_PARMS: usize = 8;
/// `MAX_PARMS + MAX_EXTRA_PARMS`: the most arguments a call may pass.
const MAX_ARGUMENTS: usize = 136;

impl Checker<'_> {
    /// FTEF-32: the call `call`.
    pub(super) fn call(&mut self, call: &ast::CallExpr) -> Checked {
        let span = call.syntax().span();
        let args: Vec<ast::Expr> = call.args().map(|a| a.args().collect()).unwrap_or_default();
        // `va_arg`'s type, written in its argument list (FTEF-32.7).
        let written = call
            .args()
            .and_then(|list| list.syntax().children().find_map(ast::Type::cast))
            .and_then(|ty| self.env.written_type(ty.syntax().span().start));
        // An intrinsic no definition names, which FTEQCC declares as it is called.
        if let Some(ast::Expr::Name(name)) = call.callee()
            && let Some(token) = name.name()
            && self.env.reference(token.span().start).is_none()
            && let Some(value) =
                self.intrinsic(&token.text_lossless(), &args, written.clone(), span)?
        {
            return Ok(value);
        }
        // FTEF-32.8: a call's value is no callee; the expression ends before the `(`.
        if let Some(ast::Expr::Call(inner)) = call.callee() {
            let value = self.call(&inner)?;
            let expected = expected_after(call.syntax());
            let message = format!("expected {expected}, found (");
            if self.pending.is_none() {
                self.pending = Some((message, span));
            }
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
        if func.ty == Type::Unknown {
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
        // The function's own definition's parameters, else its type's (`QCC_typeparam_s`), a
        // struct's function member's included.
        let params = func
            .sym
            .def()
            .filter(|&def| self.env.definition(def).is_some_and(|d| d.ty.is_function()))
            .and_then(|def| self.env.params(def))
            .unwrap_or_else(|| function.as_ref().map_or_else(Vec::new, type_params));
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
            && let Some(value) = self.intrinsic(&name, &args, written, span)?
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
                    reference.ref_name()
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
            self.record_call_path(&name, at, arg);
            values.push(value);
            self.end_of_construct()?;
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
    /// `OP_CALLn`; on a target with `OP_CALL1H` the first two arguments of at most three words
    /// are passed as they are, with no store, and the call is `OP_CALLnH`. An argument of more
    /// than three words is copied three at a time, each three a parameter, by the widest store
    /// the target has for the words left (`QCC_PR_GenerateFunctionCallRef`). Only at global
    /// scope do they matter.
    pub(super) fn generate_call(&mut self, values: &[Value], span: Span) {
        if !self.global {
            return;
        }
        let target = self.env.target_at(span.start);
        let registers = target.valid(Op::Call1h);
        let mut parms: usize = 0;
        // The arguments of at most three words, by parameter, stored after every argument is
        // read (`QCC_RemapTemp`'s loop), from the third with `OP_CALL1H`.
        let mut simple: Vec<(usize, u32)> = Vec::new();
        for value in values {
            let words = self.env.layout(&value.ty).map_or(1, |l| l.words);
            if words <= 3 {
                simple.push((parms, words));
                parms = parms.saturating_add(1);
                continue;
            }
            // Copied as it is read, three words at a time, by the widest store the target has
            // for what is left of them, each three a parameter of its own.
            let mut at: u32 = 0;
            while at < words {
                let left_in_parm = 3_u32.saturating_sub(at.checked_rem(3).unwrap_or(0));
                let mut size = left_in_parm.min(words.saturating_sub(at));
                if size == 2 && !target.valid(Op::StoreI64) {
                    size = 1;
                }
                let store = match size {
                    3 => Op::StoreV,
                    2 => Op::StoreI64,
                    _ => Op::StoreF,
                };
                if at.is_multiple_of(3) {
                    parms = parms.saturating_add(1);
                }
                self.generate(store, span);
                at = at.saturating_add(size);
            }
        }
        let first = if registers { 2 } else { 0 };
        for (_, words) in simple.into_iter().filter(|&(parm, _)| parm >= first) {
            let store = match words {
                3 => Op::StoreV,
                2 if target.valid(Op::StoreI64) => Op::StoreI64,
                2 => Op::StoreV,
                _ => Op::StoreF,
            };
            self.generate(store, span);
        }
        let calls: [Op; 8] = if registers {
            [
                Op::Call1h,
                Op::Call2h,
                Op::Call3h,
                Op::Call4h,
                Op::Call5h,
                Op::Call6h,
                Op::Call7h,
                Op::Call8h,
            ]
        } else {
            [
                Op::Call1,
                Op::Call2,
                Op::Call3,
                Op::Call4,
                Op::Call5,
                Op::Call6,
                Op::Call7,
                Op::Call8,
            ]
        };
        let call = match parms.checked_sub(1) {
            None => Op::Call0,
            Some(last) => calls.get(last.min(7)).copied().unwrap_or(Op::Call8),
        };
        self.generate(call, span);
    }

    /// FTEF-32.2: the intrinsics a call of a function with no parameters stands for, by the
    /// function's name: `_`, which takes only a string immediate; `random` and `randomv`, whose
    /// arguments are made floats; `alloca`, whose size is made an integer, rounded up to words and
    /// pushed by `OP_PUSH`, a plain statement; `va_arg`; and
    /// `used_sound`, `used_model`, which take a string immediate. `None` for any other name.
    fn intrinsic(
        &mut self,
        name: &str,
        args: &[ast::Expr],
        written: Option<Type>,
        span: Span,
    ) -> Result<Option<Value>, Fatal> {
        let string = || Value::of(Type::Simple(SimpleType::String));
        // `QCC_PR_Expression` reads their first argument whatever follows the `(`.
        if args.is_empty() && matches!(name, "alloca" | "used_sound" | "used_model") {
            let message = "\")\" - not a name".to_owned();
            return self.fatal(Code::E0201, span, message).map(Some);
        }
        match name {
            "_" => {
                let immediate = args.first().is_some_and(is_string_literal);
                if !immediate {
                    let message = "_() intrinsic accepts only a string immediate".to_owned();
                    return self.fatal(Code::E0340, span, message).map(Some);
                }
                Ok(Some(string()))
            }
            "randomv" => {
                // Each argument is a vector as it is, with no conversion.
                for (at, arg) in args.iter().take(2).enumerate() {
                    let value = self.value(Some(arg.clone()))?;
                    if self.etype_of(&value.ty).is_some_and(|e| e != Etype::Vector) {
                        let message = format!("type mismatch on parm {}", at.saturating_add(1));
                        return self.fatal(Code::E0340, span, message).map(Some);
                    }
                }
                Ok(Some(Value::of(Type::Simple(SimpleType::Vector))))
            }
            "entnum" if !args.is_empty() => {
                // `(e / 1i) / (nextent(world) / 1i)`, by the `nextent` builtin.
                let entity = self.value(args.first().cloned())?;
                let one = Value::constant(Constant::int(1));
                let index = self.emit(qc_preproc::Op::DivI, &entity, &one, span)?;
                if !self.env.defines("nextent") {
                    let message = "the nextent builtin is not defined".to_owned();
                    return self.fatal(Code::E0347, span, message).map(Some);
                }
                self.generate_call(std::slice::from_ref(&entity), span);
                let first = Value::of(Type::Simple(SimpleType::Entity));
                let first = self.emit(qc_preproc::Op::DivI, &first, &one, span)?;
                let number = self.emit(qc_preproc::Op::DivI, &index, &first, span)?;
                Ok(Some(number))
            }
            "random" => {
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
                Ok(Some(Value::of(Type::Simple(SimpleType::Float))))
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
                // `push_words((sz+3)/4)`, a plain statement.
                self.simple_statement(qc_preproc::Op::Push, span)?;
                Ok(Some(Value::of(Type::Pointer(
                    Type::Simple(SimpleType::Void).into(),
                ))))
            }
            "va_arg" => {
                let index = self.value(args.first().cloned())?;
                let arg_span = args.first().map_or(span, |a| a.syntax().span());
                if index.ty != Type::Unknown {
                    if self.etype_of(&index.ty) == Some(Etype::Float) {
                        let three = Value::constant(Constant::float(3.0));
                        self.emit(qc_preproc::Op::MulF, &index, &three, arg_span)?;
                    } else {
                        let int = Type::Named("__int32".into());
                        let index = self.supplied(&index, &int, arg_span)?;
                        let three = Value::constant(Constant::int(3));
                        self.emit(qc_preproc::Op::MulI, &index, &three, arg_span)?;
                    }
                }
                let ty = written.unwrap_or(Type::Unknown);
                if !self.function.variadic {
                    let message = "va_arg() intrinsic only works inside varadic functions";
                    return self.fatal(Code::E0346, span, message.to_owned()).map(Some);
                }
                Ok(Some(Value::of(ty)))
            }
            // FTEF-17.17: `autocvar(name, default[, "description"])` is the global
            // `autocvar_name`, of the default's type and given its value.
            "autocvar" if args.len() >= 2 => {
                let Some(ast::Expr::Name(name)) = args.first() else {
                    return Ok(None);
                };
                let Some(name) = name.name().map(|t| t.text_lossless()) else {
                    return Ok(None);
                };
                let default = args.get(1).cloned();
                let value = self.value(default.clone())?;
                let cvar = match (value.known(), default.as_ref().and_then(string_immediate)) {
                    (_, Some(text)) => CvarValue::Text(text),
                    (Some(constant), None) => CvarValue::Number(constant),
                    (None, None) => CvarValue::Unknown,
                };
                if value.constness == Constness::Variable {
                    let message = "autocvar default value is not constant".to_owned();
                    self.env.report(Code::E0367, span, message);
                }
                if self.env.autocvar(&name, cvar, span) {
                    return Err(Fatal);
                }
                Ok(Some(Value::of(value.ty)))
            }
            "used_sound" | "used_model" => {
                let list = if name == "used_sound" {
                    PathList::Sound
                } else {
                    PathList::Model
                };
                if let Some(bytes) = args.first().and_then(string_bytes) {
                    self.env
                        .record_path(list, &String::from_utf8_lossy(&bytes), span);
                }
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

    /// FTEF-17.19: the path a call names by a constant string: a `precache_` function's first
    /// argument, `setmodel`'s and `localsound`'s second and `sound`'s third.
    fn record_call_path(&mut self, name: &str, at: usize, arg: &ast::Expr) {
        let list = match (at, name) {
            (0, _) if name.starts_with("precache_sound") => PathList::Sound,
            (0, _) if name.starts_with("precache_model") => PathList::Model,
            (0, _) if name.starts_with("precache_texture") => PathList::Texture,
            (0, _) if name.starts_with("precache_file") => PathList::File,
            (1, "setmodel") => PathList::Model,
            (1, "localsound") | (2, "sound") => PathList::Sound,
            _ => return,
        };
        let bytes = match arg {
            ast::Expr::Literal(_) => string_bytes(arg),
            ast::Expr::Name(name) => name
                .name()
                .and_then(|t| self.env.reference(t.span().start))
                .and_then(|def| self.env.string_text(def))
                .and_then(|text| lexed_string(&format!("\"{text}\""))),
            _ => None,
        };
        if let Some(bytes) = bytes {
            self.env
                .record_path(list, &String::from_utf8_lossy(&bytes), arg.syntax().span());
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

/// The parameters of the function type `function` as a call reads them, from how each is
/// written (`QCC_typeparam_s`, FTEF-6.25).
fn type_params(function: &FunctionType) -> Vec<ParamFacts> {
    (0..function.params.len())
        .map(|at| {
            function
                .labels
                .0
                .get(at)
                .map_or_else(ParamFacts::default, |label| ParamFacts {
                    name: label.name.clone(),
                    has_default: label.default.is_some(),
                    optional: label.optional,
                    out: label.out,
                })
        })
        .collect()
}
