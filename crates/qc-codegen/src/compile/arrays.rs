// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_LoadFromArray, QCC_PR_ParseRefArrayPointer, QCC_PR_EmitArrayGetFunction, QCC_PR_ArrayRecurseDivideRegular

//! FTEQCC's arrays and structs on the vanilla opcodes (FTE-13, FTE-14).
//!
//! An array is its elements' words in consecutive globals and a struct its layout's. A constant
//! index or a member is a fixed offset; the vanilla opcodes cannot address a global by a value,
//! so a subscript by a value calls a function the compiler generates for the array, which
//! FTEQCC names `ArrayGet*NAME` and emits after every file: a ladder of comparisons that
//! returns the word the index names.

use qc_diag::Span;
use qc_progs::Function;
use qc_progs::opcode::Opcode;
use qc_sema::{FileId, FunctionType, Location, SimpleType, Type};
use qc_syntax::ast::{self, AstNode};
use qc_types::Etype;

use super::{Abort, At, Compiler, Lowered, Value, ValueKind, latin1, literal_of, operand_index};
use crate::Internal;
use crate::ops::{self, Operator};
use crate::program::{Literal, OFS_PARM0, OFS_RETURN, ValueType, operand, type_words};

/// An array definition as a subscript reads it.
#[derive(Clone, Debug)]
pub(super) struct Array {
    /// Its place in the definition list.
    place: usize,
    /// Its first global.
    ofs: u32,
    /// Its elements' type.
    pub(super) elem: ValueType,
    /// Its elements, `arraysize`.
    count: u32,
    /// The words of one element.
    words: u32,
    /// Where it is declared.
    location: Location,
}

/// An array's getter, made where a subscript by a value first needs it and emitted after every
/// file (FTE-13.8).
#[derive(Debug)]
pub(super) struct Getter {
    /// The getter's definition.
    place: usize,
    /// The array it reads; the getter's statements come from where the array is declared.
    array: Array,
}

impl Compiler<'_> {
    /// The array `expr` names, when it is the bare name of an array definition.
    pub(super) fn array_named(&mut self, expr: &ast::Expr) -> Lowered<Option<Array>> {
        let ast::Expr::Name(name) = expr else {
            return Ok(None);
        };
        let span = expr.syntax().span();
        let id = self.def_of(&name.name().at(span)?)?;
        let ValueKind::Def(place) = self.expr(expr)?.kind else {
            return Ok(None);
        };
        let def = self.program.def(place).at(span)?;
        let Some(count) = def.count else {
            return Ok(None);
        };
        Ok(Some(Array {
            place,
            ofs: def.ofs,
            elem: def.ty.clone(),
            count,
            words: type_words(self.index, &def.ty),
            location: self.index.def(id).at(span)?.location,
        }))
    }

    /// FTE-13.9: `name.length` is the array's element count, a float immediate.
    pub(super) fn length(&mut self, array: &Array, span: Span) -> Lowered<Value> {
        let count = f32_of(array.count).at(span)?;
        let place = self.program.immediate(&Literal::Float(count)).at(span)?;
        self.def_value(place, span)
    }

    /// The member `name` of a value of struct type `ty`: its word and its type (FTE-14.5).
    pub(super) fn member_of(&self, ty: &ValueType, name: &str) -> Option<(u32, ValueType)> {
        let ValueType::Of(ty) = ty else {
            return None;
        };
        let member = self
            .index
            .structs
            .get(ty)?
            .members
            .iter()
            .find(|m| m.name == name)?;
        Some((member.word, ValueType::Of(member.ty.clone())))
    }

    /// `name[i]`, or with `member` its element's member `name[i].m` (FTE-13.6, FTE-13.7): a
    /// constant index the element's words, any other a call of the array's getter for each
    /// word of the value, the index in words. A store through an index by a value needs
    /// FTEQCC's setter, which is not lowered.
    pub(super) fn element(
        &mut self,
        array: &Array,
        index: &ast::Expr,
        member: Option<(u32, ValueType)>,
        writing: bool,
    ) -> Lowered<Value> {
        let span = index.syntax().span();
        let (word, ty) = member.unwrap_or((0, array.elem.clone()));
        let at = self.expr(index)?;
        let constant = match at.kind {
            ValueKind::Def(place)
                if self.constant(place) && self.type_of(&at).etype() == Etype::Float =>
            {
                Some(f32::from_bits(self.program.word(at.ofs)))
            }
            ValueKind::Def(_) | ValueKind::Temp(_) | ValueKind::Return => None,
        };
        if let Some(i) = constant {
            let element = element_number(i, array.count).at(span)?;
            let ofs = element
                .checked_mul(array.words)
                .and_then(|w| w.checked_add(word))
                .and_then(|w| w.checked_add(array.ofs))
                .at(span)?;
            return Ok(Value {
                ofs,
                kind: ValueKind::Temp(ty),
            });
        }
        if writing || self.type_of(&at).etype() != Etype::Float {
            return Err(Abort::Internal(Internal::Unsupported, span));
        }
        let mut words = at.ofs;
        if array.words != 1 {
            words = self.float_op(Opcode::MulF, words, array.words, span)?;
        }
        if word != 0 {
            words = self.float_op(Opcode::AddF, words, word, span)?;
        }
        let getter = self.getter(array, span)?;
        if array.elem.etype() == Etype::Vector {
            let elements = self.float_op(Opcode::DivF, words, 3, span)?;
            return self.call1(getter, elements, ty, span);
        }
        let size = type_words(self.index, &ty);
        if size == 1 {
            return self.call1(getter, words, ty, span);
        }
        // A value of more than one word is read a word at a time into a temporary.
        let temp = self.program.allocate(size).at(span)?;
        for w in 0..size {
            let arg = if w == 0 {
                words
            } else {
                self.float_op(Opcode::AddF, words, w, span)?
            };
            self.call1(getter, arg, ValueType::simple(Etype::Float), span)?;
            let to = temp.checked_add(w).at(span)?;
            self.program.emit(
                Opcode::StoreF,
                operand(OFS_RETURN).at(span)?,
                operand(to).at(span)?,
            );
        }
        Ok(Value {
            ofs: temp,
            kind: ValueKind::Temp(ty),
        })
    }

    /// `OP a, n` of the float at `a` and the immediate `n`, into a new temporary.
    fn float_op(&mut self, op: Opcode, a: u32, n: u32, span: Span) -> Lowered<u32> {
        let b = self.float_immediate(n, span)?;
        self.program.emit_result(op, a, b, Etype::Float).at(span)
    }

    /// The offset of the float immediate `n`.
    fn float_immediate(&mut self, n: u32, span: Span) -> Lowered<u32> {
        let place = self
            .program
            .immediate(&Literal::Float(f32_of(n).at(span)?))
            .at(span)?;
        Ok(self.program.def(place).at(span)?.ofs)
    }

    /// `QCC_PR_GenerateFunctionCall1`: the argument at `arg` stored to the first parameter by
    /// its type, `CALL1` of the function at `function`, the value the return global as `ret`.
    fn call1(&mut self, function: usize, arg: u32, ret: ValueType, span: Span) -> Lowered<Value> {
        self.program.emit(
            Opcode::StoreF,
            operand(arg).at(span)?,
            operand(OFS_PARM0).at(span)?,
        );
        let ofs = self.program.def(function).at(span)?.ofs;
        self.program.emit(Opcode::Call1, operand(ofs).at(span)?, 0);
        self.return_type = ret;
        Ok(Value {
            ofs: OFS_RETURN,
            kind: ValueKind::Return,
        })
    }

    /// The getter of `array`, `ArrayGet*NAME` of type `T(float)`, made the first time a
    /// subscript by a value needs it (FTE-13.8).
    fn getter(&mut self, array: &Array, span: Span) -> Lowered<usize> {
        if let Some(getter) = self.getters.iter().find(|g| g.array.place == array.place) {
            return Ok(getter.place);
        }
        let def = self.program.def(array.place).at(span)?;
        let name = format!("ArrayGet*{}", def.name);
        let ValueType::Of(elem) = &array.elem else {
            return Err(Abort::Internal(Internal::Unsupported, span));
        };
        let ty = ValueType::Of(Type::Function(
            FunctionType {
                ret: elem.clone(),
                params: vec![Type::Simple(SimpleType::Float)],
                variadic: false,
                labels: Default::default(),
            }
            .into(),
        ));
        let place = self.program.generated(&name, ty, 1).at(span)?;
        self.getters.push(Getter {
            place,
            array: array.clone(),
        });
        Ok(place)
    }

    /// `QCC_PR_EmitArrayGetFunction` for each getter made, after every file (FTE-13.8).
    pub(crate) fn emit_getters(&mut self) -> Result<(), (Internal, FileId, Span)> {
        for getter in std::mem::take(&mut self.getters) {
            let file = getter.array.location.file;
            match self.emit_getter(&getter) {
                Ok(()) => {}
                Err(Abort::Internal(internal, at)) => return Err((internal, file, at)),
                Err(Abort::Error(diagnostic)) => {
                    self.errors
                        .push(qc_sema::FileDiagnostic { file, diagnostic });
                }
            }
        }
        Ok(())
    }

    /// One getter's function: its parameter `__indexg`; `IF` of `index < 0` to the bounds
    /// error; the ladder that returns the word the index names; then the bounds error,
    /// `error(sprintf(...))`, or `error("bounds check failed\n")` without a `sprintf`; then
    /// the last word returned, and `DONE`.
    fn emit_getter(&mut self, getter: &Getter) -> Lowered<()> {
        let Location { file, span } = getter.array.location;
        self.file = file;
        self.program.origin = Some((file, span.start));
        let array = &getter.array;
        let vector = array.elem.etype() == Etype::Vector;
        let (slots, stride) = if vector {
            (array.count.max(1), 3)
        } else {
            (array.count.max(1).checked_mul(array.words).at(span)?, 1)
        };
        let parm_start = self.program.next_global().at(span)?;
        let index_place = self
            .program
            .generated("__indexg", ValueType::simple(Etype::Float), 1)
            .at(span)?;
        let index = self.program.def(index_place).at(span)?.ofs;
        let first_statement = operand_index(self.program.statements.len()).at(span)?;
        let zero = self.float_immediate(0, span)?;
        let below = self
            .program
            .emit_result(Opcode::Lt, index, zero, Etype::Float)
            .at(span)?;
        let bounds = self.program.emit(Opcode::If, operand(below).at(span)?, 0);
        self.ladder(array.ofs, index, (0, slots), stride, span)?;
        self.patch_b(bounds, span)?;
        self.bounds_error(index, slots, span)?;
        let last = slots
            .checked_sub(1)
            .and_then(|s| s.checked_mul(stride))
            .and_then(|s| s.checked_add(array.ofs))
            .at(span)?;
        self.program
            .emit(Opcode::Return, operand(last).at(span)?, 0);
        self.program.emit(Opcode::Done, 0, 0);
        let number = operand_index(self.program.functions.len()).at(span)?;
        let ofs = self.program.def(getter.place).at(span)?.ofs;
        self.program
            .set_words(ofs, &[u32::from_ne_bytes(number.to_ne_bytes())])
            .at(span)?;
        let name = self.program.def(getter.place).at(span)?.name.clone();
        let s_name = self.program.copy_string(&latin1(&name)).at(span)?;
        let s_file = self.s_files.get(&file).copied().at(span)?;
        self.program.functions.push(Function {
            first_statement,
            parm_start: operand(parm_start).at(span)?,
            locals: 1,
            profile: 0,
            s_name,
            s_file,
            numparms: 1,
            parm_size: [1, 0, 0, 0, 0, 0, 0, 0],
        });
        Ok(())
    }

    /// `QCC_PR_ArrayRecurseDivideRegular` over the slots `min..max`: one slot is `index <
    /// min+1` and `IFNOT` past the `RETURN` of its word; more are halved at `mid`, with a test
    /// of `index < mid+1` that skips the lower half when more than four slots remain.
    fn ladder(
        &mut self,
        array: u32,
        index: u32,
        (min, max): (u32, u32),
        stride: u32,
        span: Span,
    ) -> Lowered<()> {
        let width = max.checked_sub(min).at(span)?;
        if width <= 1 {
            let bound = self.float_immediate(min.checked_add(1).at(span)?, span)?;
            let below = self
                .program
                .emit_result(Opcode::Lt, index, bound, Etype::Float)
                .at(span)?;
            self.program
                .emit(Opcode::IfNot, operand(below).at(span)?, 2);
            let word = min
                .checked_mul(stride)
                .and_then(|w| w.checked_add(array))
                .at(span)?;
            self.program
                .emit(Opcode::Return, operand(word).at(span)?, 0);
            return Ok(());
        }
        let mid = min.checked_add(width.checked_div(2).at(span)?).at(span)?;
        let skip = if width > 4 {
            let bound = self.float_immediate(mid.checked_add(1).at(span)?, span)?;
            let below = self
                .program
                .emit_result(Opcode::Lt, index, bound, Etype::Float)
                .at(span)?;
            Some(
                self.program
                    .emit(Opcode::IfNot, operand(below).at(span)?, 0),
            )
        } else {
            None
        };
        self.ladder(array, index, (min, mid), stride, span)?;
        if let Some(skip) = skip {
            self.patch_b(skip, span)?;
        }
        self.ladder(array, index, (mid, max), stride, span)
    }

    /// The bounds error of a getter: `error` called with `sprintf("bounds check failed (0 <=
    /// %g < %g is false)\n", index, slots)` when the program defines `sprintf`, else with
    /// `"bounds check failed\n"`.
    fn bounds_error(&mut self, index: u32, slots: u32, span: Span) -> Lowered<()> {
        let message = match self.defined("sprintf") {
            Some(sprintf) => {
                let format = b"bounds check failed (0 <= %g < %g is false)\n".to_vec();
                let format = self.string_immediate(format, span)?;
                let count = self.float_immediate(slots, span)?;
                for (slot, (arg, etype)) in [
                    (format, Etype::String),
                    (index, Etype::Float),
                    (count, Etype::Float),
                ]
                .into_iter()
                .enumerate()
                {
                    let to = u32::try_from(slot)
                        .ok()
                        .and_then(|s| s.checked_mul(3))
                        .and_then(|s| s.checked_add(OFS_PARM0))
                        .at(span)?;
                    self.store_word(arg, to, etype, span)?;
                }
                self.program
                    .emit(Opcode::Call3, operand(sprintf).at(span)?, 0);
                OFS_RETURN
            }
            None => self.string_immediate(b"bounds check failed\n".to_vec(), span)?,
        };
        let error = self
            .defined("error")
            .ok_or(Abort::Internal(Internal::Unsupported, span))?;
        self.store_word(message, OFS_PARM0, Etype::String, span)?;
        self.program
            .emit(Opcode::Call1, operand(error).at(span)?, 0);
        Ok(())
    }

    /// FTEQCC's `a % b` of floats on the vanilla opcodes (FTE-19.8), `OP_MOD_F`'s emulation:
    /// a call of the program's `mod(a, b)` when it has one, else `a - b * (a/b | a/b)`.
    pub(super) fn modulo(
        &mut self,
        left: &Value,
        right: &Value,
        (op_span, span): (Span, Span),
    ) -> Lowered<Value> {
        if self.type_of(left).etype() != Etype::Float || self.type_of(right).etype() != Etype::Float
        {
            return Err(Abort::Internal(Internal::Unsupported, op_span));
        }
        if let Some(function) = self.defined("mod") {
            for (slot, arg) in [(OFS_PARM0, left.ofs), (OFS_PARM0 + 3, right.ofs)] {
                self.store_word(arg, slot, Etype::Float, span)?;
            }
            self.program
                .emit(Opcode::Call2, operand(function).at(span)?, 0);
            self.return_type = ValueType::simple(Etype::Float);
            return Ok(Value {
                ofs: OFS_RETURN,
                kind: ValueKind::Return,
            });
        }
        let emit = |compiler: &mut Self, op, a, b| {
            compiler
                .program
                .emit_result(op, a, b, Etype::Float)
                .at(span)
        };
        let quotient = emit(self, Opcode::DivF, left.ofs, right.ofs)?;
        let whole = emit(self, Opcode::BitOr, quotient, quotient)?;
        let product = emit(self, Opcode::MulF, right.ofs, whole)?;
        let ofs = emit(self, Opcode::SubF, left.ofs, product)?;
        Ok(Value {
            ofs,
            kind: ValueKind::Temp(ValueType::simple(Etype::Float)),
        })
    }

    /// The global offset of the global `name` the program has made, as
    /// `QCC_PR_GetSRef(NULL, name, NULL, false, ...)` finds it.
    fn defined(&self, name: &str) -> Option<u32> {
        let id = self.names.global(name)?;
        let place = self.program.place(id)?;
        Some(self.program.def(place)?.ofs)
    }

    /// The offset of the string immediate `text`.
    fn string_immediate(&mut self, text: Vec<u8>, span: Span) -> Lowered<u32> {
        let place = self.program.immediate(&Literal::String(text)).at(span)?;
        Ok(self.program.def(place).at(span)?.ofs)
    }

    /// `STORE_*` of the value of basic type `etype` at `from` to `to`.
    fn store_word(&mut self, from: u32, to: u32, etype: Etype, span: Span) -> Lowered<()> {
        let row = ops::select(Operator::Assign, etype, etype, None).at(span)?;
        self.program
            .emit(row.opcode, operand(from).at(span)?, operand(to).at(span)?);
        Ok(())
    }

    /// FTE-14.6: a struct value copied to `to`, `STORE_V` for each three words and `STORE_F`
    /// for the rest, when `ty` is a struct.
    pub(super) fn copy_struct(
        &mut self,
        ty: &ValueType,
        from: u32,
        to: u32,
        span: Span,
    ) -> Lowered<bool> {
        let ValueType::Of(t) = ty else {
            return Ok(false);
        };
        let Some(shape) = self.index.structs.get(t) else {
            return Ok(false);
        };
        let mut w = 0;
        while w < shape.words {
            let (op, n) = if shape.words.saturating_sub(w) >= 3 {
                (Opcode::StoreV, 3)
            } else {
                (Opcode::StoreF, 1)
            };
            let a = from.checked_add(w).at(span)?;
            let b = to.checked_add(w).at(span)?;
            self.program
                .emit(op, operand(a).at(span)?, operand(b).at(span)?);
            w = w.saturating_add(n);
        }
        Ok(true)
    }

    /// FTE-13.10, FTE-14.7: the words of an aggregate initialiser of a value of `ty`, `count`
    /// elements of it for an array: each element at its index times the element's words, each
    /// member of a struct at its word, each value a literal.
    pub(super) fn aggregate_words(
        &mut self,
        ty: &ValueType,
        count: Option<u32>,
        aggregate: &ast::AggregateInit,
    ) -> Lowered<Vec<u32>> {
        let span = aggregate.syntax().span();
        let size = type_words(self.index, ty);
        let total = size.checked_mul(count.unwrap_or(1)).at(span)?;
        let mut words =
            vec![0; usize::try_from(total).map_err(|_| Abort::Internal(Internal::TooLarge, span))?];
        match count {
            Some(count) => {
                let items: Vec<_> = aggregate.syntax().children().collect();
                if u32::try_from(items.len()).ok().is_none_or(|n| n > count) {
                    return Err(Abort::Internal(Internal::Unsupported, span));
                }
                for (k, item) in (0_u32..).zip(&items) {
                    let at = k.checked_mul(size).at(span)?;
                    self.item_words(ty, item, &mut words, at)?;
                }
            }
            None => self.item_words(ty, aggregate.syntax(), &mut words, 0)?,
        }
        Ok(words)
    }

    /// Writes the value `item` gives a `ty` into `words` from word `at`.
    fn item_words(
        &mut self,
        ty: &ValueType,
        item: &qc_syntax::SyntaxNode,
        words: &mut [u32],
        at: u32,
    ) -> Lowered<()> {
        let span = item.span();
        if let ValueType::Of(t) = ty
            && let Some(shape) = self.index.structs.get(t).cloned()
        {
            let aggregate = ast::AggregateInit::cast(item.clone()).at(span)?;
            if aggregate.syntax().children().count() > shape.members.len() {
                return Err(Abort::Internal(Internal::Unsupported, span));
            }
            for (member, value) in shape.members.iter().zip(aggregate.syntax().children()) {
                if member.count != 1 {
                    return Err(Abort::Internal(Internal::Unsupported, span));
                }
                let to = at.checked_add(member.word).at(span)?;
                self.item_words(&ValueType::Of(member.ty.clone()), &value, words, to)?;
            }
            return Ok(());
        }
        let expr = ast::Expr::cast(item.clone()).at(span)?;
        let value = match literal_of(&expr, self.dialect) {
            Some(literal) if literal.ty().etype() == ty.etype() => {
                self.program.literal_words(&literal).at(span)?
            }
            // FTEF-19.31: a null, a constant whose word is 0 such as `__NULL__`, casts to any
            // type, its words all 0.
            Some(Literal::Float(zero)) if zero.to_bits() == 0 => {
                vec![
                    0;
                    usize::try_from(ty.words())
                        .map_err(|_| Abort::Internal(Internal::TooLarge, span))?
                ]
            }
            Some(_) => return Err(Abort::Internal(Internal::Unsupported, span)),
            None => self.constant_words(ty, &expr)?,
        };
        let start = usize::try_from(at).map_err(|_| Abort::Internal(Internal::TooLarge, span))?;
        let end = start.checked_add(value.len()).at(span)?;
        words.get_mut(start..end).at(span)?.copy_from_slice(&value);
        Ok(())
    }

    /// FTE-13.10: the words of an initialiser's value that is no literal but a constant of type
    /// `ty`, a constant's name or an expression that folds with no statement, as
    /// `QCC_PR_GenerateInitializerType` copies `QCC_SRef_EvalConst`.
    fn constant_words(&mut self, ty: &ValueType, expr: &ast::Expr) -> Lowered<Vec<u32>> {
        let span = expr.syntax().span();
        let before = self.program.statements.len();
        let value = self.expr(expr)?;
        let ValueKind::Def(place) = value.kind else {
            return Err(Abort::Internal(Internal::Unsupported, span));
        };
        if self.program.statements.len() != before
            || !self.constant(place)
            || self.type_of(&value).etype() != ty.etype()
        {
            return Err(Abort::Internal(Internal::Unsupported, span));
        }
        let ofs = self.program.def(place).at(span)?.ofs;
        Ok((0..ty.words())
            .map(|i| self.program.word(ofs.saturating_add(i)))
            .collect())
    }
}

/// The element a constant index names, when it is a whole number within the array.
fn element_number(i: f32, count: u32) -> Option<u32> {
    let n = (0..count).find(|&n| f32_of(n).is_some_and(|f| f.to_bits() == i.to_bits()))?;
    Some(n)
}

/// `n` as the float an immediate holds, when it is exact.
fn f32_of(n: u32) -> Option<f32> {
    let small = u16::try_from(n).ok()?;
    Some(f32::from(small))
}
