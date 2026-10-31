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

use crate::DefKind;
use crate::check::assign::{lexed_string, string_bytes};
use crate::check::constant::Constant;
use crate::check::emulate::conversion_opcodes;
use crate::check::etype::Etype;
use crate::check::expr::{Checked, Checker, Fatal};
use crate::check::fold;
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
    #[expect(
        clippy::too_many_lines,
        reason = "one pass over QCC_PR_ParseFunctionCall's steps, read in its order"
    )]
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
                self.intrinsic(&token.text_lossless(), &args, written.clone(), call)?
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
        // A method's call is made with its object as `self` (`newself`), a member function named
        // bare in another with `this`. A non-virtual one names its definition, `CLASS::name`
        // (`QCC_RefToDef`), where a field's read, a virtual method's included, is a temporary
        // named by its type (FTEF-32.11).
        let member_def = match &callee.reference {
            Reference::Field(field) => field.def,
            _ => callee.sym.def(),
        };
        let member = member_def
            .and_then(|def| self.env.definition(def))
            .and_then(|d| Some((d.member_of?, d)));
        let method = member.is_some();
        let method_name = match &callee.reference {
            Reference::Field(field) if field.field.contains("::") => Some(field.field.clone()),
            _ => member
                .filter(|(_, d)| d.kind != DefKind::Field)
                .and_then(|(class, d)| {
                    Some(format!("{}::{}", self.env.definition(class)?.name, d.name))
                }),
        };
        let mut func = self.read(callee, callee_span)?;
        if method_name.is_some() {
            func.name = method_name;
        }
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
            // A `variant` takes any arguments, and no argument left empty (FTEF-32.9).
            let slots = call.args().map(|list| list.slots()).unwrap_or_default();
            let mut values = Vec::new();
            for (at, slot) in slots.iter().enumerate() {
                let Some(arg) = slot else {
                    return self.missing_argument(at, &slots, span);
                };
                values.push(self.value(Some(arg.clone()))?);
            }
            // Each argument is stored by its own size, then the call made (FTEF-34.9).
            self.generate_call(&values, span);
            return Ok(Value::of(Type::Named("__variant".into())));
        };
        // `!t->num_parms`, which counts no `...` (FTEF-32.10).
        if !method && function.params.is_empty() {
            if name == "spawn"
                && let Some(list) = call.args()
                && let Some(value) = self.spawn(&list, &args)?
            {
                return Ok(value);
            }
            if let Some(value) = self.intrinsic(&name, &args, written, call)? {
                return Ok(value);
            }
        }
        let slots = call.args().map(|list| list.slots()).unwrap_or_default();
        let mut passed = Vec::new();
        let mut values = Vec::new();
        for (at, slot) in slots.iter().enumerate() {
            let Some(arg) = slot else {
                if at >= MAX_ARGUMENTS {
                    let message = format!("More than {MAX_ARGUMENTS} parameters");
                    return self.fatal(Code::E0338, span, message);
                }
                let value = self.implicit_argument(&function, &params, at, &slots, span)?;
                passed.push(None);
                values.push(value);
                continue;
            };
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
            passed.push(Some(reference));
            self.record_call_path(&name, at, arg);
            values.push(value);
            self.end_of_construct()?;
        }
        let mut count = slots.len();
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
        // FTEF-32.14: a call of a constant's value is folded before the arguments are checked.
        let floats = passed.iter().all(|reference| {
            reference
                .as_ref()
                .is_some_and(|r| r.ty == Type::Simple(SimpleType::Float))
        });
        let constants: Vec<_> = values.iter().map(Value::known).collect();
        if let Some(folded) = fold::builtin_call(&name, &constants, floats) {
            return Ok(Value::constant(folded));
        }
        self.verify_arguments(&name, &values, span);
        self.generate_call(&values, span);
        // `out` arguments are stored back after the call (`QCC_PR_GenerateFunctionCallRef`).
        for (reference, param) in passed.iter().zip(&params) {
            let Some(reference) = reference.as_ref().filter(|_| param.out) else {
                continue;
            };
            if reference.readonly {
                let message = "Unable to write to read-only out argument".to_owned();
                self.env.report(Code::E0339, span, message);
                continue;
            }
            let returned = Value::of(reference.ty.clone());
            self.store_reference(reference, &returned, span)?;
        }
        // FTEF-29.15: the value is an alias of `def_ret`, which may be assigned to, unless a
        // return larger than a vector is read back into a temporary.
        Ok(Value {
            returned: true,
            readonly: self.is_large(&function.ret),
            ..Value::of(function.ret.clone())
        })
    }

    /// FTEF-32.9: an argument left empty where no parameter takes it: the expression read
    /// there is missing, before the `,` or the `)` after the place `at` of `slots`.
    fn missing_argument(&mut self, at: usize, slots: &[Option<ast::Expr>], span: Span) -> Checked {
        let found = if at.saturating_add(1) < slots.len() {
            ","
        } else {
            ")"
        };
        self.fatal(Code::E0201, span, format!("\"{found}\" - not a name"))
    }

    /// FTEF-32.9: the argument `at` left empty, which takes its parameter's default: the fatal
    /// error "Default value not specified for implicit argument N" for one with none, and, past
    /// the parameters, an expression that is not there (`QCC_PR_RefExpression`). The default's
    /// reference is freed twice, so FTEQCC warns "INTERNAL: over-freed refcount to NAME", the
    /// default as `QCC_VarAtOffset` prints it.
    fn implicit_argument(
        &mut self,
        function: &FunctionType,
        params: &[ParamFacts],
        at: usize,
        slots: &[Option<ast::Expr>],
        span: Span,
    ) -> Checked {
        let Some(param) = function.params.get(at) else {
            return self.missing_argument(at, slots, span);
        };
        if !params.get(at).is_some_and(|p| p.has_default) {
            let message = format!(
                "Default value not specified for implicit argument {}",
                at.saturating_add(1)
            );
            return self.fatal(Code::E0375, span, message);
        }
        if let Some(default) = function.labels.0.get(at).and_then(|l| l.default.as_ref()) {
            let message = format!("INTERNAL: over-freed refcount to {default}");
            self.env.report(Code::W0319, span, message);
        }
        Ok(Value::of(param.clone()))
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

    /// FTEF-32.10: the intrinsics a call of a function with no parameters stands for, by the
    /// function's name: `_`, which takes only a string immediate; `random` and `randomv`, whose
    /// arguments are made floats; `alloca`, whose size is made an integer, rounded up to words and
    /// pushed by `OP_PUSH`, a plain statement; `va_arg`; and
    /// `used_sound`, `used_model`, which take a string immediate. `None` for any other name.
    fn intrinsic(
        &mut self,
        name: &str,
        args: &[ast::Expr],
        written: Option<Type>,
        call: &ast::CallExpr,
    ) -> Result<Option<Value>, Fatal> {
        let span = call.syntax().span();
        let string = || Value::of(Type::Simple(SimpleType::String));
        // `QCC_PR_Expression` reads their first argument whatever follows the `(`.
        if args.is_empty() && matches!(name, "alloca" | "used_sound" | "used_model") {
            let message = "\")\" - not a name".to_owned();
            return self.fatal(Code::E0201, span, message).map(Some);
        }
        // `!QCC_PR_CheckToken(")")` takes the `)` of `entnum()` and `autocvar()`, so the call's
        // argument is read from what follows it (FTEF-32.12).
        if args.is_empty() && matches!(name, "entnum" | "autocvar") {
            let message = format!("\"{}\" - not a name", expected_after(call.syntax()));
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
            "entnum" => {
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
            "autocvar" => {
                // FTEF-32.12: a name, a `,`, the default, and at most a description.
                if args.len() < 2 || args.len() > 3 {
                    let message = if args.len() < 2 {
                        "expected ,, found )"
                    } else {
                        "expected ), found ,"
                    };
                    return self.fatal(Code::E0204, span, message.to_owned()).map(Some);
                }
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
                if self.env.autocvar(&name, &value.ty, cvar, span) {
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

    /// FTEF-17.20: `spawn(CLASS, field: value, ...)` and `spawn(e, CLASS, ...)`, the class's
    /// value. `QCC_PR_ParseType` reads the class, else an expression, a `,` and the class; a type
    /// that is no entity's ends at the token after it (`QCC_PR_ParseName`), as does a class that
    /// is no type ("Spawn operator with undefined class: NAME"). Each argument after the class is
    /// a field of it, its members first (`QCC_PR_ParseValue(rettype, ...)`), named before `:` or
    /// `=`. `None` for `spawn()`, which is a call as any other.
    fn spawn(&mut self, list: &ast::ArgList, args: &[ast::Expr]) -> Result<Option<Value>, Fatal> {
        let named: Vec<ast::NamedArg> = list.named().collect();
        let Some(first) = args.first() else {
            return Ok(None);
        };
        let type_of = |checker: &Self, arg: &ast::Expr| match arg {
            ast::Expr::Name(name) => name
                .name()
                .and_then(|t| checker.env.reference(t.span().start))
                .and_then(|def| checker.env.type_named(def)),
            _ => None,
        };
        let (class, at): (Type, usize) = match type_of(self, first) {
            Some(class) => (class, 1),
            None => {
                self.value(Some(first.clone()))?;
                match args.get(1).map(|arg| (type_of(self, arg), arg)) {
                    Some((Some(class), _)) => (class, 2),
                    Some((None, arg)) => {
                        // `QCC_PR_ParseName` reads the argument's first token.
                        let token = arg
                            .syntax()
                            .descendants_with_tokens()
                            .find_map(|e| match e {
                                qc_syntax::SyntaxElement::Token(t) if !t.kind().is_trivia() => {
                                    Some(t)
                                }
                                qc_syntax::SyntaxElement::Token(_)
                                | qc_syntax::SyntaxElement::Node(_) => None,
                            });
                        let span = token.as_ref().map_or(arg.syntax().span(), |t| t.span());
                        let shown = token.map_or_else(String::new, |t| t.text_lossless());
                        let message = format!("Spawn operator with undefined class: {shown}");
                        return self.fatal(Code::E0373, span, message).map(Some);
                    }
                    None => match named.first().and_then(ast::NamedArg::name) {
                        Some(token) => {
                            let message = format!(
                                "Spawn operator with undefined class: {}",
                                token.text_lossless()
                            );
                            return self.fatal(Code::E0373, token.span(), message).map(Some);
                        }
                        None => {
                            let message = "expected ,, found )".to_owned();
                            return self
                                .fatal(Code::E0204, list.syntax().span(), message)
                                .map(Some);
                        }
                    },
                }
            }
        };
        let class_arg = args
            .get(at.saturating_sub(1))
            .map_or(list.syntax().span(), |a| a.syntax().span());
        if self.env.shape(&class).map(|s| s.etype) != Some(Etype::Entity) {
            let found = if args.len() > at || !named.is_empty() {
                ","
            } else {
                ")"
            };
            let message = format!("\"{found}\" - not a name");
            return self.fatal(Code::E0201, class_arg, message).map(Some);
        }
        let base = Value::of(class.clone());
        if let Some(extra) = args.get(at) {
            self.value(Some(extra.clone()))?;
            let message = "Named field is not a field.".to_owned();
            return self
                .fatal(Code::E0374, extra.syntax().span(), message)
                .map(Some);
        }
        for arg in &named {
            let Some(token) = arg.name() else {
                continue;
            };
            let name = token.text_lossless();
            let span = token.span();
            // A class's member is a field of its type (`QCC_PR_ParseEntClass`).
            let field = self.env.class_member(&class, &name, span.start).is_some()
                || match self.env.reference(span.start) {
                    Some(def) => self.env.definition(def).is_some_and(|d| {
                        let ty = d.ty.clone();
                        self.env.shape(&ty).is_some_and(|s| s.etype == Etype::Field)
                    }),
                    None => {
                        self.unknown_field(&base, &name, span);
                        false
                    }
                };
            if !field {
                let message = "Named field is not a field.".to_owned();
                return self.fatal(Code::E0374, span, message).map(Some);
            }
            if arg.equals() {
                self.env
                    .report(Code::W0318, span, "That = should be a :".to_owned());
            }
            self.value(arg.value())?;
        }
        Ok(Some(base))
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
        // `QCC_ShouldConvert` of 0 returns the value with its own type, as an `int` stays for a
        // function or a pointer (FTEF-28.4).
        let unconverted = match (self.etype_of(&value.ty), self.etype_of(to)) {
            (Some(from), Some(to)) => from != to && conversion_opcodes(from, to).is_empty(),
            _ => false,
        };
        Ok(Value {
            ty: if unconverted {
                value.ty.clone()
            } else {
                to.clone()
            },
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
