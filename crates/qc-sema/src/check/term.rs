// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_RefTerm, QCC_PR_GenerateVector, QCC_EvaluateCast

//! The terms FTEQCC reads before an operator (FTEF-30): the prefix operators, casts, the vector
//! constructor, `sizeof` and `_length`.

use qc_diag::{Code, Span};
use qc_preproc::{FteFlag, Op};
use qc_syntax::ast::{self, AstNode as _, UnaryOp};

use crate::cast::ConstType;
use crate::check::constant::Constant;
use crate::check::emulate::conversion_opcodes;
use crate::check::etype::Etype;
use crate::check::expr::{Checked, Checker, Fatal};
use crate::check::member;
use crate::check::pointer::{Pointee, int, opaque_int};
use crate::check::truth::Test;
use crate::check::{Constness, Reference, Value};
use crate::ty::{SimpleType, Type};

/// `VMWORDSIZE`: the bytes of a word, by which `sizeof` counts a type of whole words.
const WORD_BYTES: u32 = 4;

impl Checker<'_> {
    /// A prefix operator and its operand (FTEF-30.4, FTEF-30.5).
    pub(super) fn unary(&mut self, unary: &ast::UnaryExpr) -> Checked {
        let span = unary.syntax().span();
        let Some(op) = unary.op() else {
            self.expr_of(unary.operand())?;
            return Ok(Value::unknown());
        };
        let operand = self.expr_of(unary.operand())?;
        if operand.ty == Type::Unknown || !self.env.fteqcc() {
            return Ok(Value::unknown());
        }
        // `++`, `--`, `&` and `@` take the reference; the rest read it (`QCC_PR_Expression`).
        let operand = match op {
            UnaryOp::PreIncrement
            | UnaryOp::PreDecrement
            | UnaryOp::AddressOf
            | UnaryOp::AsString => operand,
            UnaryOp::BitNot | UnaryOp::Neg | UnaryOp::Plus | UnaryOp::Deref => {
                let operand_span = unary.operand().map_or(span, |e| e.syntax().span());
                self.read(operand, operand_span)?
            }
        };
        match op {
            UnaryOp::BitNot => self.test(Test::BitNot, &operand, span, "Type mismatch: ~%s"),
            UnaryOp::Neg => self.negate(&operand, span),
            UnaryOp::Plus => {
                if self.etype_of(&operand.ty).is_some_and(signed_arithmetic) {
                    return Ok(operand);
                }
                self.fatal(Code::E0302, span, "type mismatch for +".to_owned())
            }
            UnaryOp::PreIncrement | UnaryOp::PreDecrement => {
                self.prefix_step(&operand, op == UnaryOp::PreIncrement, span)
            }
            UnaryOp::Deref => self.dereference(operand, span),
            UnaryOp::AsString => {
                if matches!(
                    self.etype_of(&operand.ty),
                    Some(Etype::Float | Etype::Integer)
                ) {
                    return Ok(Value {
                        ty: Type::Simple(SimpleType::String),
                        ..operand
                    });
                }
                let message = "'@' operator only functions on floats and ints. go figure.";
                self.fatal(Code::E0330, span, message.to_owned())
            }
            UnaryOp::AddressOf => self.address_term(operand, span),
        }
    }

    /// `!e`, which `ast::NotExpr` holds: `QCC_PR_GenerateLogicalNot(e, "Type mismatch: !%s")`.
    pub(super) fn not(&mut self, not: &ast::NotExpr) -> Checked {
        let operand = self.value(not.operand())?;
        if operand.ty == Type::Unknown || !self.env.fteqcc() {
            return Ok(Value::unknown());
        }
        let span = not.syntax().span();
        self.test(Test::Not, &operand, span, "Type mismatch: !%s")
    }

    /// FTEF-30.4: `-e`, `0 - e` of the operand's type, a truth value taken for its parent's;
    /// any other type is the fatal error "type mismatch for -".
    fn negate(&mut self, operand: &Value, span: Span) -> Checked {
        let Some(etype) = self.etype_of(&operand.ty) else {
            return Ok(Value::unknown());
        };
        // `switch(type->type)` strips only a truth value: an accessor is no number to it.
        let etype = if self.env.is_accessor(&operand.ty) {
            Etype::Void
        } else {
            etype
        };
        let (op, zero) = match etype {
            Etype::Float => (Op::SubF, Constant::float(0.0)),
            Etype::Double => (Op::SubD, Constant::double(0.0)),
            Etype::Vector => (Op::SubV, Constant::vector([0.0; 3])),
            Etype::Integer => (Op::SubI, Constant::int(0)),
            Etype::Uint => (Op::SubU, Constant::int(0)),
            Etype::Int64 => (Op::SubI64, Constant::int64(0)),
            Etype::Uint64 => (Op::SubU64, Constant::int64(0)),
            _ => return self.fatal(Code::E0302, span, "type mismatch for -".to_owned()),
        };
        self.emit(op, &Value::constant(zero), operand, span)
    }

    /// FTEF-30.4: `++e` and `--e`: a read-only operand is the fatal error "++ operator on
    /// read-only value" (or `--`); the operand plus or minus 1 of its type, a pointer by its
    /// pointee's size with `OP_ADD_PIW`, is emitted and stored; any other type is "++ operator
    /// on unsupported type". The value is the operand stored to, read-only.
    fn prefix_step(&mut self, operand: &Value, increment: bool, span: Span) -> Checked {
        let spelling = if increment { "++" } else { "--" };
        if operand.readonly {
            let message = format!("{spelling} operator on read-only value");
            return self.fatal(Code::E0329, span, message);
        }
        let Some(etype) = self.etype_of(&operand.ty) else {
            return Ok(Value::unknown());
        };
        let step = if increment { 1 } else { -1 };
        let constant = |c: Constant| Value::constant(c);
        let (op, by) = match etype {
            Etype::Integer => (Op::AddI, constant(Constant::int(step))),
            Etype::Uint => (Op::AddU, constant(Constant::int(step))),
            // `QCC_MakeIntConst(preinc * aux_type->size)`, the size unsigned, so `--p` is "will
            // be truncated" as a subtraction's (FTEF-27.10).
            Etype::Pointer => {
                let words = match self.pointee(&operand.ty) {
                    Pointee::Words(words) => words,
                    Pointee::Void | Pointee::Bits(_) => Some(1),
                };
                let by = if increment {
                    words.map_or_else(opaque_int, int)
                } else {
                    self.negated_size(words, span)
                };
                (Op::AddPiw, by)
            }
            Etype::Float => (Op::AddF, constant(Constant::float(step_f32(increment)))),
            Etype::Double => (
                Op::AddD,
                constant(Constant::double(step_f32(increment).into())),
            ),
            Etype::Int64 => (Op::AddI64, constant(Constant::int64(step.into()))),
            Etype::Uint64 => (Op::AddU64, constant(Constant::int64(step.into()))),
            _ => {
                let message = "++ operator on unsupported type".to_owned();
                return self.fatal(Code::E0329, span, message);
            }
        };
        let variable = Value {
            constness: Constness::Variable,
            ..self.read(operand.clone(), span)?
        };
        let updated = self.emit(op, &variable, &by, span)?;
        self.store_reference(operand, &updated, span)?;
        // `QCC_StoreSRefToRef` folds the store into the operand, which is the value, read-only.
        Ok(Value {
            readonly: true,
            reference: Reference::Global,
            ..variable
        })
    }

    /// FTEF-30.5: `*e`: under `qccx` a float or integer is an entity; a pointer is what it
    /// points at, which may be assigned to; a string its first character, an integer; a function
    /// itself, the fatal error "attempt to dereference function type." under `qcfuncs`; anything
    /// else the fatal error "Unable to dereference non-pointer type.".
    fn dereference(&mut self, operand: Value, span: Span) -> Checked {
        let flags = self.env.flags_at(span.start);
        let etype = self.etype_of(&operand.ty);
        if flags.has(FteFlag::Qccx) && matches!(etype, Some(Etype::Float | Etype::Integer)) {
            return Ok(Value {
                readonly: false,
                ..Value::of(Type::Simple(SimpleType::Entity))
            });
        }
        match (etype, &operand.ty) {
            (Some(Etype::Pointer), Type::Pointer(pointee)) => Ok(Value {
                readonly: false,
                reference: Reference::pointer(operand.label(), None),
                name: operand.name,
                ..Value::of(pointee.as_ref().clone())
            }),
            (Some(Etype::String), _) => Ok(Value {
                readonly: false,
                ..Value::of(Type::Named("__int32".into()))
            }),
            (Some(Etype::Function), _) if flags.has(FteFlag::Qcfuncs) => {
                let message = "attempt to dereference function type.".to_owned();
                self.fatal(Code::E0330, span, message)
            }
            (Some(Etype::Function), _) => Ok(operand),
            (None, _) => Ok(Value::unknown()),
            (Some(_), _) => {
                let message = "Unable to dereference non-pointer type.".to_owned();
                self.fatal(Code::E0330, span, message)
            }
        }
    }

    /// FTEF-30.6: `(TYPE)e`, `QCC_EvaluateCast(e, TYPE, false)`: refused only as "Cannot cast",
    /// a fatal error, and converted by the cast's opcodes, an entity and a float by
    /// `OP_CONV_ITOF` and `OP_CONV_FTOI`. `(vector){x, y, z}` is the vector constructor.
    pub(super) fn cast(&mut self, cast: &ast::CastExpr) -> Checked {
        let span = cast.syntax().span();
        let to = cast
            .ty()
            .and_then(|ty| self.env.written_type(ty.syntax().span().start));
        let elements: Vec<ast::Expr> = cast
            .syntax()
            .children()
            .filter_map(ast::AggregateInit::cast)
            .flat_map(|init| init.elements().collect::<Vec<_>>())
            .collect();
        let operand = self.value(cast.operand())?;
        if !elements.is_empty() {
            if to.as_ref().and_then(|ty| self.etype_of(ty)) == Some(Etype::Vector) {
                return self.vector(&elements, span);
            }
            for element in &elements {
                self.expr(element)?;
            }
            return Ok(to.map_or_else(Value::unknown, Value::of));
        }
        let Some(to) = to else {
            return Ok(Value::unknown());
        };
        if operand.ty == Type::Unknown || !self.env.fteqcc() {
            return Ok(Value::of(to));
        }
        if self.env.explicit_cast(operand.source(), &to, span) {
            return Err(Fatal);
        }
        let conversions: &[Op] = match (self.etype_of(&operand.ty), self.etype_of(&to)) {
            (Some(Etype::Entity), Some(Etype::Float)) => &[Op::ConvItof],
            (Some(Etype::Float), Some(Etype::Entity)) => &[Op::ConvFtoi],
            (Some(from), Some(target)) => conversion_opcodes(from, target),
            _ => &[],
        };
        let mut value = operand;
        for &conv in conversions {
            value = self.emit(conv, &value, &Value::unknown(), span)?;
        }
        Ok(Value { ty: to, ..value })
    }

    /// `[x, y, z]`, the vector constructor.
    pub(super) fn vector_expr(&mut self, vector: &ast::VectorExpr) -> Checked {
        let elements: Vec<ast::Expr> = vector.elements().collect();
        self.vector(&elements, vector.syntax().span())
    }

    /// FTEF-30.7: `QCC_PR_GenerateVector`: a component that is no float or integer is cast to a
    /// float (`QCC_EvaluateCast(x, type_float, true)`), and one still not one is the fatal error
    /// "Argument not a single numeric value in vector constructor"; a component left out is a
    /// float 0; three constants are a vector constant; else each component is stored, an integer
    /// with `OP_STORE_IF`.
    fn vector(&mut self, elements: &[ast::Expr], span: Span) -> Checked {
        let float = Type::Simple(SimpleType::Float);
        let mut components = Vec::new();
        for element in elements {
            let value = self.value(Some(element.clone()))?;
            if value.ty == Type::Unknown || !self.env.fteqcc() {
                components.push(None);
                continue;
            }
            let numeric = matches!(
                self.etype_of(&value.ty),
                Some(Etype::Float | Etype::Integer)
            );
            if !numeric && self.env.cast(value.source(), &float, span) {
                return Err(Fatal);
            }
            components.push(Some(if numeric {
                value
            } else {
                Value {
                    ty: float.clone(),
                    ..value
                }
            }));
        }
        let Some(mut values) = components.into_iter().collect::<Option<Vec<Value>>>() else {
            return Ok(Value::of(Type::Simple(SimpleType::Vector)));
        };
        // `[x]` and `[x, y]`: a component left out is `QCC_MakeFloatConst(0)`.
        while values.len() < 3 {
            values.push(Value::constant(Constant::float(0.0)));
        }
        let known: Option<Vec<Constant>> = values.iter().map(Value::known).collect();
        if let Some([x, y, z]) = known.as_deref() {
            let float_of = |c: &Constant| {
                if c.ty == ConstType::Integer {
                    int_f32(c.i())
                } else {
                    c.f()
                }
            };
            return Ok(Value::constant(Constant::vector([
                float_of(x),
                float_of(y),
                float_of(z),
            ])));
        }
        // Two null components make the vector the third times an axis, `OP_MUL_VF`.
        let null = |at: usize| values.get(at).is_some_and(Value::null);
        for (at, axis) in [
            (0, [1.0, 0.0, 0.0]),
            (1, [0.0, 1.0, 0.0]),
            (2, [0.0, 0.0, 1.0]),
        ] {
            let others = [0, 1, 2].iter().filter(|&&i| i != at).all(|&i| null(i));
            if let Some(value) = values.get(at)
                && others
            {
                let value = value.clone();
                let float = self.supplied(&value, &float, span)?;
                let axis = Value::constant(Constant::vector(axis));
                self.emit(Op::MulVf, &axis, &float, span)?;
                return Ok(Value::of(Type::Simple(SimpleType::Vector)));
            }
        }
        for value in &values {
            if self.etype_of(&value.ty) == Some(Etype::Integer) {
                self.emit(Op::StoreIf, value, &Value::unknown(), span)?;
            } else {
                self.generate(Op::StoreF, span);
            }
        }
        Ok(Value::of(Type::Simple(SimpleType::Vector)))
    }

    /// FTEF-30.8: `sizeof(TYPE)` and `sizeof e`: an unsigned constant, a string immediate's
    /// length with its NUL, else the bytes of the type, an array's times its length; under
    /// `undefwordsize` the error "sizeof was disabled, use array.length instead." for a type and
    /// "sizeof was disabled." for an expression, whose statements are discarded.
    pub(super) fn sizeof(&mut self, sizeof: &ast::SizeofExpr) -> Checked {
        let span = sizeof.syntax().span();
        let undefined = self.env.flags_at(span.start).has(FteFlag::Undefwordsize);
        if let Some(ty) = sizeof.ty() {
            if undefined {
                let message = "sizeof was disabled, use array.length instead.".to_owned();
                self.env.report(Code::E0326, span, message);
            }
            let ty = self.env.written_type(ty.syntax().span().start);
            return Ok(ty.map_or_else(opaque_uint, |ty| self.size_of(&ty)));
        }
        let value = self.expr_of(sizeof.operand())?;
        if let Some(text) = sizeof.operand().and_then(|e| string_immediate(&e)) {
            let bytes = u32::try_from(text.len()).unwrap_or(u32::MAX);
            return Ok(Value::constant(Constant::uint(bytes.saturating_add(1))));
        }
        if undefined {
            self.env
                .report(Code::E0326, span, "sizeof was disabled.".to_owned());
        }
        // An array's length the model does not keep, so its size is no known constant.
        if value.ty == Type::Unknown || value.is_array_head() {
            return Ok(opaque_uint());
        }
        Ok(self.size_of(&value.ty))
    }

    /// The bytes a value of `ty` takes as `sizeof` counts them: `bits / 8` for a type whose bits
    /// are no whole words, else `VMWORDSIZE` per word (FTEF-6.13); a type whose layout the model
    /// does not follow is no known constant.
    fn size_of(&self, ty: &Type) -> Value {
        let bytes = self.env.layout(ty).and_then(|layout| {
            if layout.bits & 31 == 0 {
                layout.words.checked_mul(WORD_BYTES)
            } else {
                Some(layout.bits >> 3)
            }
        });
        bytes.map_or_else(opaque_uint, |bytes| Value::constant(Constant::uint(bytes)))
    }

    /// FTEF-30.8: `_length(e)`: an array's length, a constant string's length without its NUL, a
    /// vector's 3; a string that is no constant is the fatal error "_length(string) requires an
    /// initialised constant", any other type "_length() unsupported argument type for
    /// intrinsic".
    pub(super) fn length(&mut self, operand: Option<ast::Expr>, span: Span) -> Checked {
        let text = operand.as_ref().and_then(string_immediate);
        let value = self.expr_of(operand)?;
        if value.ty == Type::Unknown || !self.env.fteqcc() {
            return Ok(Value::unknown());
        }
        let length = |n: usize| Value::constant(Constant::int(i32::try_from(n).unwrap_or(0)));
        if value.is_array_head() {
            return Ok(member::length());
        }
        match self.etype_of(&value.ty) {
            Some(Etype::String) => match (text, value.constness) {
                (Some(text), _) => Ok(length(text.len())),
                (None, Constness::Variable) => {
                    let message = "_length(string) requires an initialised constant".to_owned();
                    self.fatal(Code::E0334, span, message)
                }
                (None, Constness::Known(_) | Constness::Opaque) => Ok(member::length()),
            },
            Some(Etype::Vector) => Ok(length(3)),
            _ => {
                let message = "_length() unsupported argument type for intrinsic".to_owned();
                self.fatal(Code::E0334, span, message)
            }
        }
    }
}

/// Whether `-` and `+` take a value of basic type `etype`.
const fn signed_arithmetic(etype: Etype) -> bool {
    matches!(
        etype,
        Etype::Float
            | Etype::Double
            | Etype::Vector
            | Etype::Integer
            | Etype::Uint
            | Etype::Int64
            | Etype::Uint64
    )
}

/// 1 or -1 as a float.
const fn step_f32(increment: bool) -> f32 {
    if increment { 1.0 } else { -1.0 }
}

/// C's `(float)` of an `int`.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec FTEF-30.7: QCC_Eval_Float of an integer constant is C's int to float"
)]
const fn int_f32(value: i32) -> f32 {
    value as f32
}

/// The text of a string literal `expr` is, without its quotes, with adjacent parts joined.
fn string_immediate(expr: &ast::Expr) -> Option<String> {
    let ast::Expr::Literal(literal) = expr else {
        return None;
    };
    if literal.kind() != Some(ast::LiteralKind::String) {
        return None;
    }
    let mut text = String::new();
    for token in literal.syntax().child_tokens() {
        let part = token.text_lossless();
        text.push_str(
            part.get(1..part.len().saturating_sub(1))
                .unwrap_or_default(),
        );
    }
    Some(text)
}

/// An unsigned constant whose value the model does not know.
fn opaque_uint() -> Value {
    Value {
        constness: Constness::Opaque,
        ..Value::constant(Constant::uint(0))
    }
}
