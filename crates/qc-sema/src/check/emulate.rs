// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_StatementFlags

//! What an opcode becomes (FTEF-27): `QCC_PR_StatementFlags` folds its constant operands
//! (FTEF-26), emits it when the target has it (FTEF-25), and otherwise emulates it: converts
//! an operand and emits another opcode, calls a QuakeC helper the program defines, expands it
//! into other opcodes, or refuses it. Each step is an opcode of its own, folded, emitted or
//! emulated in turn.

use qc_diag::{Code, Span};
use qc_preproc::{FteFlag, Op, OpcodeTarget, TargetFormat};

use crate::DefId;
use crate::check::constant::Constant;
use crate::check::etype::Etype;
use crate::check::expr::{Checker, Fatal};
use crate::check::fold::{Folded, fold};
use crate::check::opcodes::{Opcode, row};
use crate::check::{Constness, Sym, Value};
use crate::ir;
use crate::ty::Type;

/// `QCTARGVER_FTE_DEF` and `QCTARGVER_FTE_MAX`: the FTE revisions `badopcode` suggests.
const FTE_DEFAULT: u32 = 5768;
/// See [`FTE_DEFAULT`].
const FTE_MAX: u32 = 6614;

/// How an opcode the target lacks is emulated (FTEF-27.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Emulation {
    /// Convert the right operand with the first opcode, then emit the second.
    ConvertRight(Op, Op),
    /// Convert the left operand with the first opcode, then emit the second.
    ConvertLeft(Op, Op),
    /// Call the helper the program defines with one of these names, else warn "NAME function
    /// not defined: cannot emulate WHAT", NAME the first, and refuse the opcode.
    Helper(&'static [&'static str], &'static str),
    /// Emit these opcodes on the operands in turn, the last giving the value; each is emitted
    /// or emulated in turn. For `%` when no `mod` function is defined (FTEF-27.4).
    Expand(&'static [Op]),
    /// A 64-bit `==` or `!=` word by word: the first opcode on the operands' low words, then
    /// on their high words, each cast to `int`, joined by the second (FTEF-27.13).
    Words(Op, Op),
    /// `a % b` as `a - b * (a / b)`: the division, the floor (`d | d`) for a float one, the
    /// product of `b` and the quotient and the difference from `a`, each emitted in turn, when
    /// the program defines no function of the name given (`QCC_PR_EmulationFunc`), which is
    /// otherwise called (FTEF-27.4).
    Remainder(&'static str, &'static [Op]),
    /// `~a` as `-1 - a` (`op = SUB; var_b = var_a; var_a = QCC_MakeFloatConst(-1)`,
    /// FTEF-27.9): this subtraction of the constant -1 (a vector of them when it is a vector's)
    /// and the operand, emitted with no further check.
    Complement(Op),
    /// `a ^ b` as `(a & ~b) | (b & ~a)` (FTE-19.7): the complement of `b`, the `&` of `a` and
    /// it, the complement of `a`, the `&` of `b` and it, and the `|` of the two, each emitted in
    /// turn; for an `int`, a call of the program's function of the name given when it has one
    /// (`QCC_PR_EmulationFunc`).
    Xor {
        /// The function called instead, if the program defines it.
        helper: Option<&'static str>,
        /// The complement.
        not: Op,
        /// The `&`.
        and: Op,
        /// The `|`.
        or: Op,
    },
    /// A vector operator as this float one of each word (FTE-19.12): a vector temporary taken
    /// (`QCC_GetTemp(type_vector)`), then three plain statements, `a+i`, `b+i` into `c+i`
    /// (`QCC_PR_SimpleStatement`), which free neither operand.
    ByWord(Op),
    /// `v / f` as `v * (1 / f)` (FTE-19.9): the first opcode of the float constant 1
    /// (`QCC_MakeFloatConst(1)`) and the right operand, then the second of the left operand and
    /// that quotient.
    Reciprocal(Op, Op),
    /// `return QCC_PR_StatementFlags(OTHER, a, b, ...)`: this opcode instead, folded, emitted or
    /// emulated in turn.
    Instead(Op),
    /// `return QCC_PR_StatementFlags(OTHER, b, a, ...)`: this opcode on the operands swapped.
    Swapped(Op),
    /// `op = OTHER; break;`: emitted as this opcode, with no further check (FTEF-27.9).
    Replaced(Op),
    /// A bit clear, `b - (b & a)`: the store opcode when there is one and the target has it,
    /// else the `&` emitted in turn and the `-` replacing the opcode (FTEF-27.9).
    Clear {
        /// The `&` opcode.
        and: Op,
        /// The `-` opcode.
        sub: Op,
        /// The opcode that clears in place, used when the target has it.
        store: Option<Op>,
    },
    /// `QCC_SupplyConversion`'s conversions: fold a constant, else call the helper, else warn
    /// that denormals are unsafe and emit the opcode that stands for it.
    Conversion,
    /// `OP_ADD_SF`, `OP_ADD_SI` and the entity offsets: their own rules (FTEF-27.3).
    Special,
    /// Emitted as statements of another opcode with no check of the target.
    Emitted,
    /// Emitted as these plain statements (`QCC_PR_SimpleStatement`, `force` off), each refused
    /// with the fatal error when the target lacks it: a 64-bit `&`, `|` and `^` word by word,
    /// and a 64-bit field load by two `OP_LOAD_FLD`s.
    Simple(&'static [Op]),
    /// No emulation: "Opcode ... not valid for target" (FTEF-27.5).
    Refuse,
}

impl Checker<'_> {
    /// `QCC_PR_StatementFlags` of `op` on `lhs` and `rhs` at `span`: folded (FTEF-26), else
    /// emitted when the target has it, else emulated (FTEF-27). An opaque constant operand
    /// decides nothing.
    pub(super) fn emit(
        &mut self,
        op: Op,
        lhs: &Value,
        rhs: &Value,
        span: Span,
    ) -> Result<Value, Fatal> {
        // A statement reading a call's value reads it from the return global; a fold that
        // gives an operand back reads nothing.
        self.not_of_return = None;
        let opaque =
            |value: &Value| value.constness == Constness::Opaque && !value.ty.is_function();
        let folds_to_operand = !opaque(lhs)
            && !opaque(rhs)
            && matches!(
                fold(op, lhs.known(), rhs.known(), &mut Vec::new()),
                Some(Folded::Left | Folded::Right)
            );
        if (lhs.returned || rhs.returned) && !folds_to_operand {
            if matches!(op, Op::NotF | Op::NotFnc | Op::NotEnt) {
                self.not_of_return = self.live_return.map(|words| (words, self.live_call));
            }
            self.live_return = None;
        }
        let result = replaced(op);
        // A function's definition is a constant FTEQCC folds no opcode of (FTEF-26.12).
        if opaque(lhs) || opaque(rhs) {
            self.emitted = self.emitted.saturating_add(1);
            // An opcode the target lacks is emulated by the code generator, which follows the
            // value; the intermediate layer holds no fragment of it.
            if !self.env.target_at(span.start).valid(op) {
                self.followed = false;
                if let Some(builder) = &mut self.fragment {
                    builder.give_up();
                }
            }
            // Its value decides no folding or emulation; the comparison warnings read no value.
            if self.env.target_at(span.start).valid(op) {
                self.compare(op, lhs, rhs, span);
                self.generate(op, span);
            }
            let result = self.statement_temps(op, lhs, rhs, result);
            return Ok(Value {
                constness: Constness::Opaque,
                ..result
            });
        }
        let mut notices = Vec::new();
        let folded = fold(op, lhs.known(), rhs.known(), &mut notices);
        for notice in notices {
            self.env.report(notice.code, span, notice.message);
        }
        match folded {
            Some(Folded::Constant(constant)) => {
                // FTEF-26.2: a folded value is an immediate FTEQCC makes (`QCC_MakeFloatConst`).
                self.made(constant);
                return Ok(Value::constant(constant));
            }
            Some(Folded::Left) => return Ok(lhs.clone()),
            Some(Folded::Right) => return Ok(rhs.clone()),
            None => {}
        }
        self.emitted = self.emitted.saturating_add(1);
        let target = self.env.target_at(span.start);
        if target.valid(op) {
            self.compare(op, lhs, rhs, span);
            self.generate(op, span);
            return Ok(self.statement_temps(op, lhs, rhs, result));
        }
        let before = self.generated.len();
        let emulated = self.emulate(op, lhs, rhs, span, result)?;
        // An emulation that generated no statement and left no constant or other value it names,
        // as `1i * f` folds to `f`, is one the code generator lowers itself, which the body does
        // not follow.
        if self.generated.len() == before
            && emulated.known().is_none()
            && self.name_of(&emulated).is_none()
            && !self.global
        {
            self.followed = false;
        }
        // A refused opcode is still the statement it was (`badopcode`), its result a temporary;
        // a helper's call leaves its value in the return global instead.
        if emulated.temp.is_none()
            && emulated.known().is_none()
            && emulated.sym == Sym::Temp
            && !emulated.returned
        {
            return Ok(self.statement_temps(op, lhs, rhs, emulated));
        }
        Ok(emulated)
    }

    /// FTEF-27.16: a statement frees its operands' temporaries, then takes one for its result,
    /// which a store, a jump or a `void` opcode has none of.
    fn statement_temps(&mut self, op: Op, lhs: &Value, rhs: &Value, result: Value) -> Value {
        for offset in [lhs.temp, rhs.temp].into_iter().flatten() {
            self.env.temps().free(offset);
        }
        let Some(row) = row(op) else {
            if let Some(builder) = &mut self.fragment {
                builder.give_up();
            }
            return result;
        };
        let result = self.lowered(op, row, lhs, rhs, result);
        // The generated statement's operands: what each value it reads names, and its own value.
        let made = self
            .generated
            .len()
            .checked_sub(1)
            .and_then(|index| u32::try_from(index).ok())
            .filter(|&index| {
                usize::try_from(index)
                    .ok()
                    .and_then(|index| self.generated.get(index))
                    .is_some_and(|statement| statement.op == op)
            });
        let reads = |slot: usize| {
            row.types
                .get(slot)
                .is_some_and(|t| t.etype() != Etype::Void)
        };
        if let Some(index) = made {
            let (a, b) = (
                reads(0).then(|| self.name_of(lhs)).flatten(),
                reads(1).then(|| self.name_of(rhs)).flatten(),
            );
            let c = row.makes_value().then_some(ir::Name::Made(index));
            if let Some(statement) = usize::try_from(index)
                .ok()
                .and_then(|index| self.generated.get_mut(index))
            {
                *statement = ir::BodyStatement { op, a, b, c };
            }
        }
        if !row.makes_value() {
            return result;
        }
        let words = match row.types[2].etype() {
            Etype::Vector => 3,
            Etype::Int64 | Etype::Uint64 | Etype::Double => 2,
            _ => 1,
        };
        let temp = Some(self.env.temps().take(words));
        self.last_temp = temp;
        Value {
            temp,
            made,
            ..result
        }
    }

    /// Records `constant` as an immediate the fragment being lowered made, when there is one; a
    /// kind of constant the layer does not hold leaves the fragment unheld.
    pub(super) fn made(&mut self, constant: Constant) {
        if !self.global
            && let Some(immediate) = constant.immediate()
        {
            let at = self.next_made();
            self.effects
                .push((at, ir::Effect::Make(ir::Name::Immediate(immediate))));
        }
        let Some(builder) = &mut self.fragment else {
            return;
        };
        match constant.immediate() {
            Some(immediate) => builder.made(immediate),
            None => builder.give_up(),
        }
    }

    /// The statement `op` of `lhs` and `rhs` added to the fragment being lowered, when there is
    /// one: an operand its row reads that the fragment cannot name means it holds the
    /// operator no longer. `result` is given the statement's value as its operand.
    fn lowered(&mut self, op: Op, row: &Opcode, lhs: &Value, rhs: &Value, result: Value) -> Value {
        let Some(builder) = &mut self.fragment else {
            return result;
        };
        let reads = |value: &Value, slot: usize| {
            row.types
                .get(slot)
                .is_some_and(|t| t.etype() != Etype::Void)
                .then(|| value.operand())
        };
        let c = if row.makes_value() {
            ir::Destination::Value
        } else {
            ir::Destination::Nothing
        };
        let ir = builder.push(op, [reads(lhs, 0), reads(rhs, 1)], c);
        Value { ir, ..result }
    }

    /// FTEF-34.9: the store a value of basic type `etype` is copied by, `OP_STORE_*`.
    pub(super) const fn store_op(etype: Option<Etype>) -> Op {
        match etype {
            Some(Etype::Vector) => Op::StoreV,
            Some(Etype::String) => Op::StoreS,
            Some(Etype::Entity) => Op::StoreEnt,
            Some(Etype::Field) => Op::StoreFld,
            Some(Etype::Function) => Op::StoreFnc,
            Some(Etype::Integer | Etype::Uint | Etype::Boolean) => Op::StoreI,
            Some(Etype::Pointer) => Op::StoreP,
            Some(Etype::Int64 | Etype::Uint64 | Etype::Double) => Op::StoreI64,
            _ => Op::StoreF,
        }
    }

    /// FTEF-34.9: the statement `op` generated, which at global scope (`!pr_scope`) is the error
    /// "Unable to generate statements at global scope (OPNAME).".
    /// Names the operands `a` and `b` of the last generated statement when it is `op`.
    pub(super) fn name_last(&mut self, op: Op, a: Option<ir::Name>, b: Option<ir::Name>) {
        if let Some(statement) = self.generated.last_mut()
            && statement.op == op
        {
            statement.a = a.or(statement.a);
            statement.b = b.or(statement.b);
        }
    }

    /// FTEF-29.13: the store of `value` folded into the statement that made it, which then
    /// writes `to` instead.
    pub(super) fn fold_into(&mut self, value: &Value, to: Option<ir::Name>) {
        if let Some(statement) = value
            .made
            .and_then(|made| usize::try_from(made).ok())
            .and_then(|made| self.generated.get_mut(made))
        {
            statement.c = to;
        }
    }

    /// FTE-18.13: `QCC_ClobberDef(&def_ret)`: a call's value still to be read is copied out of
    /// the return global, by its size, before a call or a jump overwrites or leaves it.
    pub(super) fn clobber_return(&mut self) {
        if let Some(words) = self.live_return.take() {
            let copy = u32::try_from(self.generated.len()).ok();
            self.generated.push(ir::BodyStatement {
                a: Some(ir::Name::Return),
                b: copy.map(ir::Name::Made),
                ..ir::BodyStatement::bare(if words == 3 { Op::StoreV } else { Op::StoreF })
            });
            if let (Some(call), Some(copy)) = (self.live_call.take(), copy) {
                self.copies.push((call, copy));
            }
            self.last_temp = None;
        }
    }

    /// The index of the last statement generated, if there is one.
    pub(super) fn last_made(&self) -> Option<u32> {
        u32::try_from(self.generated.len().checked_sub(1)?).ok()
    }

    /// What `value` names as an operand of the next generated statement: a call's value read
    /// after its copy out of the return global is the copy (FTE-18.13).
    pub(super) fn name_of(&self, value: &Value) -> Option<ir::Name> {
        self.copied(value.name(), value.call)
    }

    /// `name`, or the copy of the value of the generated `CALL` `call` when `name` is the
    /// return global and a statement copied the value out of it (FTE-18.13).
    pub(super) fn copied(&self, name: Option<ir::Name>, call: Option<u32>) -> Option<ir::Name> {
        let word = match name {
            Some(ir::Name::Return) => 0,
            Some(ir::Name::ReturnWord(word)) => word,
            _ => return name,
        };
        if let Some(call) = call
            && let Some(&(_, copy)) = self.copies.iter().rev().find(|(of, _)| *of == call)
        {
            return Some(match word {
                0 => ir::Name::Made(copy),
                word => ir::Name::MadeWord(copy, word),
            });
        }
        name
    }

    pub(super) fn generate(&mut self, op: Op, span: Span) {
        self.generated.push(ir::BodyStatement::bare(op));
        if self.global {
            let message = format!(
                "Unable to generate statements at global scope ({}).",
                op.name()
            );
            self.env.report(Code::E0345, span, message);
        }
    }

    /// FTE-19.10: the operands `OP_AND_ANY` and `OP_OR_ANY` read: a vector, under
    /// `vectorlogic`, its dot product with itself (`QCC_PR_StatementFlags(&pr_opcodes[OP_MUL_V],
    /// var_a, var_a, ...)`), the left operand's first; any other operand as it is. A string
    /// under `ifstring`, which FTEQCC compares with `""`, is read as it is too: neither this
    /// model nor the code generator follows `ifstring` yet.
    fn truths(
        &mut self,
        op: Op,
        lhs: &Value,
        rhs: &Value,
        span: Span,
    ) -> Result<(Value, Value), Fatal> {
        if !matches!(op, Op::AndAny | Op::OrAny) {
            return Ok((lhs.clone(), rhs.clone()));
        }
        let flags = self.env.flags_at(span.start);
        let truth = |checker: &mut Self, value: &Value| -> Result<Value, Fatal> {
            let etype = checker.env.shape(&value.ty).map(|shape| shape.etype);
            if etype == Some(Etype::Vector) && flags.has(FteFlag::Vectorlogic) {
                return checker.emit(Op::MulV, value, value, span);
            }
            Ok(value.clone())
        };
        let lhs = truth(self, lhs)?;
        let rhs = truth(self, rhs)?;
        Ok((lhs, rhs))
    }

    /// `QCC_PR_EmulationFunc` (FTE-19.8): the program's `function` called with the two
    /// operands, each stored to its parameter, its value the return; the value takes a
    /// temporary as the opcode's statement would (FTEF-27.16), which the warnings that name one
    /// count.
    fn helper_call(
        &mut self,
        function: DefId,
        (lhs, rhs): (&Value, &Value),
        result: Value,
    ) -> Value {
        if let Some(builder) = &mut self.fragment {
            for (operand, slot) in [(lhs, 0), (rhs, 1)] {
                let reads = [
                    Some(operand.operand()),
                    Some(Some(ir::Operand::Parameter(slot))),
                ];
                builder.push(Op::StoreF, reads, ir::Destination::Nothing);
            }
            let callee = [Some(Some(ir::Operand::Def(function))), None];
            builder.push(Op::Call2, callee, ir::Destination::Nothing);
        }
        let call = self.helper_statements(function, &[lhs, rhs]);
        for offset in [lhs.temp, rhs.temp].into_iter().flatten() {
            self.env.temps().free(offset);
        }
        let temp = Some(self.env.temps().take(1));
        // In a function the value is the call's, in the return global, which no store folds
        // into.
        self.last_temp = if call.is_some() { None } else { temp };
        Value {
            ir: Some(ir::Operand::Return),
            temp,
            returned: call.is_some(),
            call,
            made: None,
            ..result
        }
    }

    /// The value of the call of the helper `function` of `operands`, `result` in the return
    /// global (FTE-19.11).
    fn helper_value(&mut self, function: DefId, operands: &[&Value], result: &Value) -> Value {
        let call = self.helper_statements(function, operands);
        Value {
            returned: call.is_some(),
            call,
            made: None,
            temp: None,
            ..result.clone()
        }
    }

    /// The generated statements of a call of the helper `function` with `operands`: each
    /// stored to its parameter, only the first folding into the statement that made it
    /// (FTE-18.12), then `CALL1` or `CALL2`; the call's index, its value then in the return
    /// global.
    fn helper_statements(&mut self, function: DefId, operands: &[&Value]) -> Option<u32> {
        if self.global {
            return None;
        }
        for (slot, &operand) in (0_u8..).zip(operands) {
            let parameter = Some(ir::Name::Parameter(slot));
            if slot == 0 && operand.temp.is_some() && operand.temp == self.last_temp {
                self.fold_into(operand, parameter);
            } else {
                self.generated.push(ir::BodyStatement {
                    a: self.name_of(operand),
                    b: parameter,
                    ..ir::BodyStatement::bare(Op::StoreF)
                });
            }
            if operand.returned {
                self.live_return = None;
                self.live_call = None;
            }
        }
        self.clobber_return();
        let op = if operands.len() == 1 {
            Op::Call1
        } else {
            Op::Call2
        };
        self.generated.push(ir::BodyStatement {
            a: Some(ir::Name::Def(function, 0)),
            ..ir::BodyStatement::bare(op)
        });
        let call = self.last_made();
        self.live_return = Some(1);
        self.live_call = call;
        call
    }

    /// FTEF-27.2: the emulation of `op`, which the target lacks.
    fn emulate(
        &mut self,
        op: Op,
        lhs: &Value,
        rhs: &Value,
        span: Span,
        result: Value,
    ) -> Result<Value, Fatal> {
        let emulation = emulation(op);
        // The intermediate layer holds the emulations that are statements emitted in turn;
        // the others the code generator still lowers itself.
        if !matches!(
            emulation,
            Emulation::ConvertRight(..)
                | Emulation::ConvertLeft(..)
                | Emulation::Remainder(..)
                | Emulation::Reciprocal(..)
                | Emulation::Complement(_)
                | Emulation::ByWord(_)
                | Emulation::Clear { .. }
                | Emulation::Xor { .. }
                | Emulation::Instead(_)
                | Emulation::Swapped(_)
        ) && let Some(builder) = &mut self.fragment
        {
            builder.give_up();
        }
        match emulation {
            Emulation::ConvertRight(conv, then) => {
                let rhs = self.emit(conv, rhs, &Value::unknown(), span)?;
                self.emit(then, lhs, &rhs, span)
            }
            Emulation::ConvertLeft(conv, then) => {
                let lhs = self.emit(conv, lhs, &Value::unknown(), span)?;
                self.emit(then, &lhs, rhs, span)
            }
            Emulation::Helper(names, what) => {
                if names.iter().any(|name| self.env.defines(name)) {
                    return Ok(result);
                }
                let name = names.first().copied().unwrap_or_default();
                let message = format!("{name} function not defined: cannot emulate {what}");
                self.env.report(Code::W0235, span, message);
                self.refuse_emitted(op, span);
                Ok(result)
            }
            Emulation::Expand(steps) => {
                let mut value = lhs.clone();
                for &step in steps {
                    value = self.emit(step, &value, rhs, span)?;
                }
                Ok(Value {
                    constness: Constness::Variable,
                    ..result
                })
            }
            Emulation::Words(compare, join) => {
                let int = Type::Named("__int32".into());
                let lhs = Value {
                    ty: int.clone(),
                    ..lhs.clone()
                };
                let rhs = Value {
                    ty: int,
                    ..rhs.clone()
                };
                let low = self.emit(compare, &lhs, &rhs, span)?;
                let high = self.emit(compare, &lhs, &rhs, span)?;
                self.emit(join, &low, &high, span)
            }
            Emulation::Remainder(helper, steps) => {
                if let Some(function) = self.env.helper(helper) {
                    // The layer holds the float remainder's call, whose value is a float; the
                    // integer ones' calls the code generator does not lower from it yet.
                    if op == Op::ModF {
                        return Ok(self.helper_call(function, (lhs, rhs), result));
                    }
                    self.followed = false;
                    if let Some(builder) = &mut self.fragment {
                        builder.give_up();
                    }
                    return Ok(result);
                }
                let &[div, ref rest @ .., mul, sub] = steps else {
                    return Ok(result);
                };
                let mut quotient = self.emit(div, lhs, rhs, span)?;
                if let [floor] = rest {
                    quotient = self.emit(*floor, &quotient, &quotient, span)?;
                }
                let product = self.emit(mul, rhs, &quotient, span)?;
                self.emit(sub, lhs, &product, span)
            }
            Emulation::Complement(sub) => Ok(self.complement(sub, lhs, span)),
            Emulation::ByWord(word_op) => Ok(self.by_word(word_op, (lhs, rhs), result)),
            Emulation::Xor {
                helper,
                not,
                and,
                or,
            } => self.xor(helper, [not, and, or], (lhs, rhs), span, result),
            Emulation::Reciprocal(div, then) => {
                let one = Constant::float(1.0);
                self.made(one);
                let reciprocal = self.emit(div, &Value::constant(one), rhs, span)?;
                self.emit(then, lhs, &reciprocal, span)
            }
            Emulation::Instead(other) => {
                let (lhs, rhs) = &self.truths(op, lhs, rhs, span)?;
                // `var_c.cast = type_uint;` (or `type_uint64`) after an unsigned operator's
                // signed one, a comparison's truth value included (FTEF-27.12).
                let value = self.emit(other, lhs, rhs, span)?;
                let ty = match op {
                    Op::EqU | Op::NeU => Type::Named("__uint32".into()),
                    Op::EqU64 | Op::NeU64 => Type::Named("__uint64".into()),
                    // FTEF-31.18: `OP_CONV_FU` as `OP_CONV_FTOI` is an `int`.
                    Op::ConvFu
                    | Op::EqFld
                    | Op::EqI
                    | Op::NeFld
                    | Op::NeI
                    | Op::AndI
                    | Op::AndFi
                    | Op::AndIf
                    | Op::AndAny
                    | Op::OrI
                    | Op::OrFi
                    | Op::OrIf
                    | Op::OrAny => value.ty.clone(),
                    _ => result.ty,
                };
                Ok(Value { ty, ..value })
            }
            Emulation::Swapped(other) => Ok(Value {
                ty: result.ty,
                ..self.emit(other, rhs, lhs, span)?
            }),
            Emulation::Replaced(other) => {
                self.generate(other, span);
                Ok(replaced(other))
            }
            Emulation::Clear { and, sub, store } => self.clear([and, sub], store, (lhs, rhs), span),
            Emulation::Conversion => self.conversion(op, lhs, span, result),
            Emulation::Special => self.special(op, lhs, rhs, span, result),
            Emulation::Emitted => {
                self.generate(op, span);
                Ok(result)
            }
            // A plain statement is no `QCC_PR_StatementFlags`, which alone reports one at global
            // scope.
            Emulation::Simple(ops) => {
                for &plain in ops {
                    self.simple_statement(plain, span)?;
                }
                Ok(result)
            }
            Emulation::Refuse => {
                self.refuse_emitted(op, span);
                Ok(result)
            }
        }
    }

    /// A bit clear (`Emulation::Clear`): the `store` opcode when the target has it, else
    /// `a - (a & b)` of the `and` and `sub` opcodes: `var_b = (a & b); op = SUB`, the subtraction
    /// emitted as any statement (`QCC_PR_StatementFlags` after the `break`).
    fn clear(
        &mut self,
        [and, sub]: [Op; 2],
        store: Option<Op>,
        (lhs, rhs): (&Value, &Value),
        span: Span,
    ) -> Result<Value, Fatal> {
        let target = self.env.target_at(span.start);
        if let Some(store) = store.filter(|&store| target.valid(store)) {
            // The store opcode is not held by the intermediate layer yet.
            if let Some(builder) = &mut self.fragment {
                builder.give_up();
            }
            self.generate(store, span);
            return Ok(replaced(store));
        }
        let masked = self.emit(and, lhs, rhs, span)?;
        self.generate(sub, span);
        Ok(self.statement_temps(sub, lhs, &masked, replaced(sub)))
    }

    /// `~a` as the subtraction `sub` of -1 and `a` (`Emulation::Complement`), its constant an
    /// immediate the lowering made.
    fn complement(&mut self, sub: Op, lhs: &Value, span: Span) -> Value {
        let minus_one = if sub == Op::SubV {
            Constant::vector([-1.0; 3])
        } else {
            Constant::float(-1.0)
        };
        self.made(minus_one);
        self.generate(sub, span);
        self.statement_temps(sub, &Value::constant(minus_one), lhs, replaced(sub))
    }

    /// The vector operator whose float one of each word is `word_op` (`Emulation::ByWord`):
    /// `result`, a vector temporary, its three words written in turn.
    fn by_word(&mut self, word_op: Op, (lhs, rhs): (&Value, &Value), result: Value) -> Value {
        let temp = self.env.temps().take(3);
        // A call's vector read word by word stays the return global's to copy later, which the
        // body does not follow.
        if lhs.returned || rhs.returned {
            self.followed = false;
        }
        // In a function, the vector is taken, then each word's statement writes its word.
        let first = self.next_made();
        let made = (!self.global).then_some(first);
        if made.is_some() {
            let operands = [self.name_of(lhs), self.name_of(rhs)];
            self.effects.push((first, ir::Effect::Block(3)));
            for word in 0..3 {
                let [a, b] = operands.map(|name| name.and_then(|name| name.word(word)));
                self.generated.push(ir::BodyStatement {
                    op: word_op,
                    a,
                    b,
                    c: Some(ir::Name::MadeWord(first, word)),
                });
            }
        }
        // A store of the vector folds into no word's statement (`opt_assignments` reads c).
        self.last_temp = None;
        let mut ir = None;
        if let Some(builder) = &mut self.fragment {
            match (lhs.operand(), rhs.operand()) {
                (Some(a), Some(b)) => {
                    if let Some(vector) = builder.vector() {
                        for word in 0..3 {
                            builder.push(
                                word_op,
                                [Some(a.word(word)), Some(b.word(word))],
                                ir::Destination::Word(vector, word),
                            );
                        }
                        ir = Some(ir::Operand::Temp(vector));
                    }
                }
                _ => builder.give_up(),
            }
        }
        // Plain statements, which report nothing at global scope (FTEF-34.9).
        Value {
            temp: Some(temp),
            ir,
            made,
            word: 0,
            ..result
        }
    }

    /// `a ^ b` as `(a & ~b) | (b & ~a)` of the complement, `&` and `|` opcodes `ops`
    /// (`Emulation::Xor`), or `result`, a call of `helper` when the program defines it, which
    /// the intermediate layer does not hold yet.
    fn xor(
        &mut self,
        helper: Option<&str>,
        [not, and, or]: [Op; 3],
        (lhs, rhs): (&Value, &Value),
        span: Span,
        result: Value,
    ) -> Result<Value, Fatal> {
        if helper.is_some_and(|helper| self.env.defines(helper)) {
            self.followed = false;
            if let Some(builder) = &mut self.fragment {
                builder.give_up();
            }
            return Ok(result);
        }
        let not_b = self.emit(not, rhs, &Value::unknown(), span)?;
        let left = self.emit(and, lhs, &not_b, span)?;
        let not_a = self.emit(not, lhs, &Value::unknown(), span)?;
        let right = self.emit(and, rhs, &not_a, span)?;
        self.emit(or, &left, &right, span)
    }

    /// FTEF-27.4: a conversion the target lacks: by the helper the program defines, else with
    /// the warning that denormals or big values are unsafe. With no unsigned helper,
    /// `OP_CONV_UF` is `OP_CONV_ITOF` first, then the warning.
    fn conversion(
        &mut self,
        op: Op,
        value: &Value,
        span: Span,
        result: Value,
    ) -> Result<Value, Fatal> {
        let (helpers, code, message): (&[&str], Code, String) = match op {
            Op::ConvItof | Op::StoreIf => (
                &["itof"],
                Code::W0237,
                format!("itof emulation: denormals are unsafe {}", value.at_offset()),
            ),
            Op::ConvFtoi | Op::StoreFi => (
                &["ftoi"],
                Code::W0237,
                "ftoi emulation: denormals are unsafe".to_owned(),
            ),
            Op::ConvUf => (
                &["utof", "quake_utof", "itof", "quake_itof"],
                Code::W0238,
                format!("utof emulation: will break if {} is big", value.at_offset()),
            ),
            _ => return Ok(result),
        };
        if helpers.iter().any(|name| self.env.defines(name)) {
            // FTE-19.8: a conversion opcode is the helper's call of the value; a store's
            // conversion is not followed yet.
            let helper = helpers.iter().find_map(|name| self.env.helper(name));
            if let (Some(function), Op::ConvItof | Op::ConvFtoi | Op::ConvUf) = (helper, op) {
                let call = self.helper_statements(function, &[value]);
                return Ok(Value {
                    returned: call.is_some(),
                    call,
                    made: None,
                    temp: None,
                    ..result
                });
            }
            // A store's conversion is the call, then the store of its value, which the
            // assignment names the target of.
            if let (Some(function), Op::StoreIf | Op::StoreFi) = (helper, op)
                && self.helper_statements(function, &[value]).is_some()
            {
                self.live_return = None;
                self.live_call = None;
                self.generated.push(ir::BodyStatement {
                    a: Some(ir::Name::Return),
                    ..ir::BodyStatement::bare(if op == Op::StoreIf {
                        Op::StoreF
                    } else {
                        Op::StoreI
                    })
                });
            }
            return Ok(result);
        }
        if op == Op::ConvUf {
            self.emit(Op::ConvItof, value, &Value::unknown(), span)?;
        }
        self.env.report(code, span, message);
        // `5i / 1i` and `5.0 * 1i` of the integer constant 1 (`QCC_MakeIntConst(1)`), made
        // before the statement, into a temporary; then the store a `STORE_*` conversion is.
        let (arith, store) = match op {
            Op::ConvItof => (Op::DivF, None),
            Op::StoreIf => (Op::DivF, Some(Op::StoreF)),
            Op::ConvFtoi => (Op::MulF, None),
            Op::StoreFi => (Op::MulF, Some(Op::StoreI)),
            _ => return Ok(result),
        };
        let one = ir::Name::Immediate(ir::Immediate {
            kind: ir::ImmediateKind::Integer,
            words: [1, 0, 0],
        });
        // A store's conversion folds into the statement (FTEF-29.13), which the model does not
        // name yet.
        if let Some(store) = store {
            self.generate(arith, span);
            self.generate(store, span);
            return Ok(result);
        }
        let at = self.next_made();
        if !self.global {
            self.effects.push((at, ir::Effect::Make(one)));
        }
        self.generate(arith, span);
        self.name_last(arith, self.name_of(value), Some(one));
        if let Some(statement) = self.generated.last_mut() {
            statement.c = Some(ir::Name::Made(at));
        }
        // FTEF-27.16: the statement frees its operand's temporary and takes one for its value,
        // which a store folds into (FTEF-29.13).
        if let Some(offset) = value.temp {
            self.env.temps().free(offset);
        }
        let temp = Some(self.env.temps().take(1));
        self.last_temp = temp;
        Ok(Value {
            made: Some(at),
            word: 0,
            temp,
            ..result
        })
    }

    /// FTEF-27.15: `OP_LSHIFT_I`, `OP_RSHIFT_I` and `OP_RSHIFT_U` on a target without them, by
    /// their helper functions when the module defines them, else as a product or quotient by a
    /// power of 2; a signed right shift tests the sign and shifts a negative value flipped.
    fn shift(
        &mut self,
        op: Op,
        lhs: &Value,
        rhs: &Value,
        span: Span,
        result: Value,
    ) -> Result<Value, Fatal> {
        let (helper, divide) = match op {
            Op::LshiftI => ("LShiftInt", Op::MulI),
            Op::RshiftI => {
                let target = self.env.target_at(span.start);
                (
                    "RShiftInt",
                    if target.valid(Op::DivU) {
                        Op::DivU
                    } else {
                        Op::DivI
                    },
                )
            }
            _ => ("RShiftUInt", Op::DivU),
        };
        if self.env.defines(helper) {
            self.followed = false;
            return Ok(result);
        }
        // A shift by a constant is a product or quotient by `QCC_MakeIntConst(1<<n)`; by
        // a variable, by `2 *^ n`.
        let factor = match rhs.known() {
            Some(shift) => {
                let power = 1_i32.checked_shl(shift.u()).unwrap_or(0);
                Value::constant(Constant::int(power))
            }
            None => self.emit(Op::PowI, &Value::unknown(), rhs, span)?,
        };
        if op == Op::RshiftI {
            // `OP_LT_I` against `QCC_MakeFloatConst(0)`, the negative form by `~`, and
            // the positive form a plain statement of the division.
            // `QCC_Generate_OP_IFNOT` jumps past the negative form, which a
            // `QCC_Generate_OP_GOTO` ends (FTEF-27.15).
            let zero = Value::constant(Constant::float(0.0));
            self.emit(Op::LtI, lhs, &zero, span)?;
            self.generate(Op::IfnotI, span);
            let flipped = self.emit(Op::BitnotI, lhs, &Value::unknown(), span)?;
            let shifted = self.emit(divide, &flipped, &factor, span)?;
            // The positive form shifts into the temporary the negative form made.
            let negative = self.emit(Op::BitnotI, &shifted, &Value::unknown(), span)?;
            self.generate(Op::Goto, span);
            self.simple_statement(divide, span)?;
            return Ok(Value {
                ty: result.ty,
                ..negative
            });
        }
        let shifted = self.emit(divide, lhs, &factor, span)?;
        Ok(Value {
            ty: result.ty,
            ..shifted
        })
    }

    /// FTEF-27.3: a string or entity offset.
    fn special(
        &mut self,
        op: Op,
        lhs: &Value,
        rhs: &Value,
        span: Span,
        result: Value,
    ) -> Result<Value, Fatal> {
        let qccx = self.env.flags_at(span.start).has(FteFlag::Qccx);
        match op {
            Op::AddSf => {
                if self.env.defines("AddStringFloat") {
                    return Ok(result);
                }
                if lhs.constness == Constness::Variable {
                    let message = "OP_ADD_SF: string+float may be unsafe".to_owned();
                    self.env.report(Code::W0236, span, message);
                }
                let rhs = self.emit(Op::ConvFtoi, rhs, &Value::unknown(), span)?;
                self.emit(Op::AddI, lhs, &rhs, span)?;
                Ok(result)
            }
            Op::AddSi | Op::AddIs => {
                let message = "OP_ADD_SI: string+int may be unsafe".to_owned();
                self.env.report(Code::W0236, span, message);
                self.emit(Op::AddI, lhs, rhs, span)?;
                Ok(result)
            }
            Op::AddEf | Op::SubEf => {
                if qccx {
                    let message =
                        "OP_ADD_EF: qccx entity offsets are unsafe, and denormals are unsafe";
                    self.env.report(Code::W0237, span, message.to_owned());
                    let then = if op == Op::AddEf { Op::AddIf } else { Op::SubF };
                    self.emit(then, lhs, rhs, span)?;
                    return Ok(result);
                }
                let rhs = self.emit(Op::ConvFtoi, rhs, &Value::unknown(), span)?;
                let then = if op == Op::AddEf {
                    Op::AddEi
                } else {
                    Op::SubEi
                };
                self.emit(then, lhs, &rhs, span)?;
                Ok(result)
            }
            Op::AddEi | Op::SubEi => {
                if qccx {
                    let message = "qccx entity offsets are unsafe".to_owned();
                    self.env.report(Code::W0235, span, message);
                } else if !self.env.defines("nextent") {
                    let message = "the nextent builtin is not defined".to_owned();
                    self.env.report(Code::W0235, span, message);
                    self.refuse_emitted(op, span);
                    return Ok(result);
                } else {
                    self.emit(Op::MulI, &Value::unknown(), rhs, span)?;
                }
                // `(op==&pr_opcodes[OP_ADD_EF])?OP_ADD_I:OP_SUB_I`: never `OP_ADD_EF` here.
                self.emit(Op::SubI, lhs, rhs, span)?;
                Ok(result)
            }
            Op::LshiftI | Op::RshiftI | Op::RshiftU => self.shift(op, lhs, rhs, span, result),
            Op::LshiftF | Op::RshiftF => {
                let (int_op, what) = if op == Op::LshiftF {
                    (Op::LshiftI, "OP_LSHIFT_F*")
                } else {
                    (Op::RshiftI, "OP_RSHIFT_F*")
                };
                if self.env.target_at(span.start).valid(int_op) {
                    let lhs = self.emit(Op::ConvFtoi, lhs, &Value::unknown(), span)?;
                    let rhs = self.emit(Op::ConvFtoi, rhs, &Value::unknown(), span)?;
                    self.emit(int_op, &lhs, &rhs, span)?;
                    return Ok(result);
                }
                // FTE-19.11: `floor(a * pow(2, b))` or `floor(a / pow(2, b))` with `powf` or
                // `pow` and `floor`; else `bitshift(a, b)` or `bitshift(a, 0 - b)`.
                let pow = self.env.helper("powf").or_else(|| self.env.helper("pow"));
                if let (Some(pow), Some(floor)) = (pow, self.env.helper("floor")) {
                    let two = Value::constant(Constant::float(2.0));
                    let power = self.helper_value(pow, &[&two, rhs], &result);
                    let scale = if op == Op::LshiftF {
                        Op::MulF
                    } else {
                        Op::DivF
                    };
                    let scaled = self.emit(scale, lhs, &power, span)?;
                    return Ok(self.helper_value(floor, &[&scaled], &result));
                }
                if let Some(bitshift) = self.env.helper("bitshift") {
                    if op == Op::LshiftF {
                        return Ok(self.helper_value(bitshift, &[lhs, rhs], &result));
                    }
                    let zero = Value::constant(Constant::float(0.0));
                    let negated = self.emit(Op::SubF, &zero, rhs, span)?;
                    return Ok(self.helper_value(bitshift, &[lhs, &negated], &result));
                }
                let message = format!("bitshift function not defined: cannot emulate {what}");
                self.env.report(Code::W0235, span, message);
                self.refuse_emitted(op, span);
                Ok(result)
            }
            Op::SpaceshipS => {
                if self.env.defines("strcmp") {
                    return Ok(result);
                }
                let message = "strcmp function not defined: cannot emulate string<=>string";
                self.env.report(Code::E0321, span, message.to_owned());
                Err(Fatal)
            }
            Op::AddPf
            | Op::AddFp
            | Op::AddPi
            | Op::AddIp
            | Op::AddPu
            | Op::AddUp
            | Op::SubPf
            | Op::SubPi
            | Op::SubPu => self.pointer_offset(op, lhs, rhs, span),
            Op::SubPp => self.pointer_difference(lhs, rhs, span),
            Op::AddPiw => self.word_offset(lhs, rhs, span),
            Op::LoadpU8 | Op::LoadpI8 | Op::LoadpU16 | Op::LoadpI16 => {
                self.narrow_load(op, lhs, rhs, span)
            }
            Op::LoadpF | Op::LoadpS | Op::LoadpEnt | Op::LoadpFld | Op::LoadpFnc | Op::LoadpI => {
                self.word_load(op, rhs, span, result)
            }
            Op::LtU | Op::LeU | Op::DivU | Op::BitextendI | Op::BitextendU => {
                self.unsigned(op, lhs, rhs, span, result)
            }
            Op::BitandD | Op::BitorD | Op::BitxorD | Op::BitnotI => {
                self.bitwise(op, lhs, rhs, span, result)
            }
            Op::IfF | Op::IfnotF | Op::IfS | Op::IfnotS | Op::NotI => self.truth(op, lhs, span),
            _ => {
                self.refuse_emitted(op, span);
                Ok(result)
            }
        }
    }

    /// FTEF-27.7: the unsigned comparisons, division and sign extension.
    fn unsigned(
        &mut self,
        op: Op,
        lhs: &Value,
        rhs: &Value,
        span: Span,
        result: Value,
    ) -> Result<Value, Fatal> {
        match op {
            Op::LtU | Op::LeU => {
                // FTEF-27.7: as floats when the target has no integer comparison, else biased
                // by 2^31 to compare as signed integers.
                let target = self.env.target_at(span.start);
                if !target.valid(Op::LtI) {
                    let lhs = self.emit(Op::ConvUf, lhs, &Value::unknown(), span)?;
                    let rhs = self.emit(Op::ConvUf, rhs, &Value::unknown(), span)?;
                    let then = if op == Op::LtU { Op::LtF } else { Op::LeF };
                    return self.emit(then, &lhs, &rhs, span);
                }
                let bias = Value::constant(Constant::uint(0x8000_0000));
                let lhs = self.emit(Op::SubU, lhs, &bias, span)?;
                let rhs = self.emit(Op::SubU, rhs, &bias, span)?;
                let then = if op == Op::LtU { Op::LtI } else { Op::LeI };
                self.emit(then, &lhs, &rhs, span)
            }
            Op::DivU => {
                let message = "OP_DIV_U emulation: assuming the high bit is not set...";
                self.env.report(Code::W0235, span, message.to_owned());
                Ok(Value {
                    ty: result.ty,
                    ..self.emit(Op::DivI, lhs, rhs, span)?
                })
            }
            Op::BitextendI | Op::BitextendU => {
                // FTEF-27.7: by shifts, when the width and offset are a constant.
                let Some(field) = rhs.known() else {
                    self.refuse_emitted(op, span);
                    return Ok(result);
                };
                let bits = field.u() & 0xff;
                let offset = field.u() >> 8;
                let (up, down) = if op == Op::BitextendI {
                    (Op::LshiftI, Op::RshiftI)
                } else {
                    (Op::LshiftU, Op::RshiftU)
                };
                let int = |value: u32| {
                    let value = 32_u32.wrapping_sub(value);
                    Value::constant(Constant::int(i32::from_le_bytes(value.to_le_bytes())))
                };
                let high = self.emit(up, lhs, &int(bits.wrapping_add(offset)), span)?;
                self.emit(down, &high, &int(bits), span)
            }
            _ => Ok(result),
        }
    }

    /// FTEF-27.7: the bitwise opcodes of doubles, by 64-bit integers; `^` of vectors and
    /// doubles, as `(a & ~b) | (b & ~a)`; and `~` of an integer, `-1 - a` (FTEF-27.2).
    fn bitwise(
        &mut self,
        op: Op,
        lhs: &Value,
        rhs: &Value,
        span: Span,
        result: Value,
    ) -> Result<Value, Fatal> {
        match op {
            Op::BitnotI => {
                // `-1 - a`, `op` already `OP_SUB_I` when it is refused.
                if self.env.target_at(span.start).valid(Op::SubI) {
                    return Ok(replaced(Op::SubI));
                }
                if !self.env.defines("SubInt") {
                    let message = "SubInt function not defined: cannot emulate ~int";
                    self.env.report(Code::W0235, span, message.to_owned());
                    self.refuse_emitted(Op::SubI, span);
                }
                Ok(result)
            }
            Op::BitandD | Op::BitorD => {
                let lhs = self.emit(Op::ConvDi64, lhs, &Value::unknown(), span)?;
                let rhs = self.emit(Op::ConvDi64, rhs, &Value::unknown(), span)?;
                let then = if op == Op::BitandD {
                    Op::BitandI64
                } else {
                    Op::BitorI64
                };
                let value = self.emit(then, &lhs, &rhs, span)?;
                self.emit(Op::ConvI64d, &value, &Value::unknown(), span)
            }
            Op::BitxorD => {
                // `(a & ~b) | (b & ~a)`.
                let (not, and, or) = (Op::BitnotD, Op::BitandD, Op::BitorD);
                let not_b = self.emit(not, rhs, &Value::unknown(), span)?;
                let left = self.emit(and, lhs, &not_b, span)?;
                let not_a = self.emit(not, lhs, &Value::unknown(), span)?;
                let right = self.emit(and, rhs, &not_a, span)?;
                self.emit(or, &left, &right, span)
            }
            _ => Ok(Value::unknown()),
        }
    }

    /// FTEF-27.9: a float or string tested by comparing it with 0, then the integer test; `!` of
    /// an integer as `== 0`.
    fn truth(&mut self, op: Op, lhs: &Value, span: Span) -> Result<Value, Fatal> {
        match op {
            Op::IfF | Op::IfnotF | Op::IfS | Op::IfnotS => {
                // A float or string tested by comparing it with 0, then the integer test.
                let (compare, zero) = if matches!(op, Op::IfF | Op::IfnotF) {
                    (Op::NeF, Constant::float(0.0))
                } else {
                    (Op::NeS, Constant::string(true))
                };
                self.emit(compare, lhs, &Value::constant(zero), span)?;
                Ok(replaced(if matches!(op, Op::IfF | Op::IfS) {
                    Op::IfI
                } else {
                    Op::IfnotI
                }))
            }
            Op::NotI => {
                let zero = Value::constant(Constant::int(0));
                self.emit(Op::EqFnc, lhs, &zero, span)
            }
            _ => Ok(Value::unknown()),
        }
    }

    /// FTEF-27.5: `badopcode`, "Opcode \"NAME|OPNAME\" not valid for target", suggesting the FTE
    /// target or its newest revision when either has the opcode, else "is not supported".
    /// `QCC_PR_SimpleStatement`: `op` emitted as it is, the fatal error `refuse` reports when
    /// the target does not have it.
    pub(super) fn simple_statement(&mut self, op: Op, span: Span) -> Result<(), Fatal> {
        if self.env.target_at(span.start).valid(op) {
            return Ok(());
        }
        self.refuse(op, span);
        Err(Fatal)
    }

    /// `badopcode` in `QCC_PR_StatementFlags`: [`Self::refuse`], an error that is no fatal
    /// one, after which the statement is emitted as any is, at global scope "Unable to generate
    /// statements at global scope" (FTEF-34.9).
    pub(super) fn refuse_emitted(&mut self, op: Op, span: Span) {
        self.refuse(op, span);
        self.generate(op, span);
    }

    pub(super) fn refuse(&mut self, op: Op, span: Span) {
        let name = row(op).map_or("", |r| r.name);
        let opname = op.name();
        let message = if OpcodeTarget::new(TargetFormat::Fte, FTE_DEFAULT).valid(op) {
            format!(
                "Opcode \"{name}|{opname}\" not valid for target. Consider the use of: #pragma \
                 target fte"
            )
        } else if OpcodeTarget::new(TargetFormat::Fte, FTE_MAX).valid(op) {
            format!(
                "Opcode \"{name}|{opname}\" not valid for target. Consider the use of: #pragma \
                 target fte_{FTE_MAX}"
            )
        } else {
            format!("Opcode \"{name}|{opname}\" is not supported.")
        };
        self.env.report(Code::E0320, span, message);
    }
}

/// FTEF-27.2: how `QCC_PR_StatementFlags`'s switch emulates `op`.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per case of QCC_PR_StatementFlags's switch, read against it in order"
)]
const fn emulation(op: Op) -> Emulation {
    use Emulation::{
        ByWord, Conversion, ConvertLeft, ConvertRight, Emitted, Expand, Helper, Instead,
        Reciprocal, Refuse, Remainder, Simple, Special, Words, Xor,
    };
    if let Some(substitution) = substitution(op) {
        return substitution;
    }
    match op {
        Op::AddSf
        | Op::AddSi
        | Op::AddIs
        | Op::AddEf
        | Op::SubEf
        | Op::AddEi
        | Op::SubEi
        | Op::SpaceshipS => Special,
        Op::BitandI => Helper(&["BitandInt"], "int&int"),
        Op::BitorI => Helper(&["BitorInt"], "int|int"),
        Op::AddI => Helper(&["AddInt"], "int+int"),
        Op::SubI => Helper(&["SubInt"], "int-int"),
        Op::MulI => Helper(&["MulInt"], "int*int"),
        Op::DivI => Helper(&["DivInt"], "int/int"),
        Op::ModV => Helper(&["ModVec"], "vector%vector"),
        Op::PowF => Helper(&["pow", "powf"], "float*^float"),
        Op::PowI => Helper(&["pow", "powf"], "int*^int"),
        Op::PowFi => Helper(&["pow", "powf"], "float*^int"),
        Op::PowIf => Helper(&["pow", "powf"], "int*^float"),
        Op::ModF => Remainder("mod", &[Op::DivF, Op::BitorF, Op::MulF, Op::SubF]),
        Op::ModI => Remainder("ModInt", &[Op::DivI, Op::MulI, Op::SubI]),
        Op::ModU => Remainder("ModInt", &[Op::DivU, Op::MulU, Op::SubU]),
        Op::ModFi => Remainder("mod", &[Op::DivFi, Op::BitorF, Op::MulIf, Op::SubF]),
        Op::ModIf => Remainder("mod", &[Op::DivIf, Op::BitorF, Op::MulF, Op::SubIf]),
        Op::BitxorF => Xor {
            helper: None,
            not: Op::BitnotF,
            and: Op::BitandF,
            or: Op::BitorF,
        },
        Op::BitxorI => Xor {
            helper: Some("BitxorInt"),
            not: Op::BitnotI,
            and: Op::BitandI,
            or: Op::BitorI,
        },
        Op::BitxorV => Xor {
            helper: None,
            not: Op::BitnotV,
            and: Op::BitandV,
            or: Op::BitorV,
        },
        Op::DivVf => Reciprocal(Op::DivF, Op::MulVf),
        Op::CrossV | Op::SpaceshipF => Expand(&[Op::MulF, Op::SubF]),
        Op::ConvItof | Op::StoreIf | Op::ConvFtoi | Op::StoreFi | Op::ConvUf => Conversion,
        Op::ConvFu => Instead(Op::ConvFtoi),
        Op::LshiftDi => ConvertLeft(Op::ConvDi64, Op::LshiftI64i),
        Op::RshiftDi => ConvertLeft(Op::ConvDi64, Op::RshiftI64i),
        Op::AddPf
        | Op::AddFp
        | Op::AddPi
        | Op::AddIp
        | Op::AddPu
        | Op::AddUp
        | Op::SubPf
        | Op::SubPi
        | Op::SubPu
        | Op::SubPp
        | Op::AddPiw
        | Op::LoadpF
        | Op::LoadpS
        | Op::LoadpEnt
        | Op::LoadpFld
        | Op::LoadpFnc
        | Op::LoadpI
        | Op::LoadpU8
        | Op::LoadpI8
        | Op::LoadpU16
        | Op::LoadpI16
        | Op::LtU
        | Op::LeU
        | Op::DivU
        | Op::BitextendI
        | Op::BitextendU
        | Op::BitandD
        | Op::BitorD
        | Op::BitxorD
        | Op::IfF
        | Op::IfnotF
        | Op::IfS
        | Op::IfnotS
        | Op::NotI
        | Op::BitnotI => Special,
        Op::LshiftIf => ConvertRight(Op::ConvFtoi, Op::LshiftI),
        Op::RshiftIf => ConvertRight(Op::ConvFtoi, Op::RshiftI),
        Op::LshiftFi => ConvertLeft(Op::ConvFtoi, Op::LshiftI),
        Op::RshiftFi => ConvertLeft(Op::ConvFtoi, Op::RshiftI),
        Op::LshiftF | Op::RshiftF | Op::LshiftI | Op::RshiftI | Op::RshiftU => Special,
        Op::AddFi => ConvertRight(Op::ConvItof, Op::AddF),
        Op::SubFi => ConvertRight(Op::ConvItof, Op::SubF),
        Op::BitandFi => ConvertRight(Op::ConvItof, Op::BitandF),
        Op::BitorFi => ConvertRight(Op::ConvItof, Op::BitorF),
        Op::LtFi => ConvertRight(Op::ConvItof, Op::LtF),
        Op::LeFi => ConvertRight(Op::ConvItof, Op::LeF),
        Op::GtFi => ConvertRight(Op::ConvItof, Op::GtF),
        Op::GeFi => ConvertRight(Op::ConvItof, Op::GeF),
        Op::EqFi => ConvertRight(Op::ConvItof, Op::EqF),
        Op::NeFi => ConvertRight(Op::ConvItof, Op::NeF),
        Op::DivFi => ConvertRight(Op::ConvItof, Op::DivF),
        Op::MulFi => ConvertRight(Op::ConvItof, Op::MulF),
        Op::MulVi => ConvertRight(Op::ConvItof, Op::MulVf),
        Op::AddIf => ConvertLeft(Op::ConvItof, Op::AddF),
        Op::SubIf => ConvertLeft(Op::ConvItof, Op::SubF),
        Op::BitandIf => ConvertLeft(Op::ConvItof, Op::BitandF),
        Op::BitorIf => ConvertLeft(Op::ConvItof, Op::BitorF),
        Op::LtIf => ConvertLeft(Op::ConvItof, Op::LtF),
        Op::LeIf => ConvertLeft(Op::ConvItof, Op::LeF),
        Op::GtIf => ConvertLeft(Op::ConvItof, Op::GtF),
        Op::GeIf => ConvertLeft(Op::ConvItof, Op::GeF),
        Op::EqIf => ConvertLeft(Op::ConvItof, Op::EqF),
        Op::NeIf => ConvertLeft(Op::ConvItof, Op::NeF),
        Op::DivIf => ConvertLeft(Op::ConvItof, Op::DivF),
        Op::MulIf => ConvertLeft(Op::ConvItof, Op::MulF),
        Op::MulIv => ConvertLeft(Op::ConvItof, Op::MulFv),
        Op::LeI => ConvertLeft(Op::ConvItof, Op::LeFi),
        Op::GeI => ConvertLeft(Op::ConvItof, Op::GeFi),
        Op::LtI => ConvertLeft(Op::ConvItof, Op::LtFi),
        Op::GtI => ConvertLeft(Op::ConvItof, Op::GtFi),
        Op::EqFld | Op::EqI => Instead(Op::EqFnc),
        Op::NeFld | Op::NeI => Instead(Op::NeFnc),
        // FTEF-27.6: the logic opcodes are `OP_AND_F` and `OP_OR_F` when the target has no
        // other, their operands' truth taken as FTEQCC takes it.
        Op::AndI | Op::AndFi | Op::AndIf | Op::AndAny => Instead(Op::AndF),
        Op::OrI | Op::OrFi | Op::OrIf | Op::OrAny => Instead(Op::OrF),
        // FTEF-27.7: the unsigned opcodes are the signed ones, the 64-bit pairs of words.
        Op::AddU => Instead(Op::AddI),
        Op::SubU => Instead(Op::SubI),
        Op::MulU => Instead(Op::MulI),
        Op::BitandU => Instead(Op::BitandI),
        Op::BitorU => Instead(Op::BitorI),
        Op::BitxorU => Instead(Op::BitxorI),
        Op::BitnotU => Instead(Op::BitnotI),
        Op::BitclrU => Instead(Op::BitclrI),
        Op::LshiftU => Instead(Op::LshiftI),
        Op::EqU => Instead(Op::EqI),
        Op::NeU => Instead(Op::NeI),
        Op::AddU64 => Instead(Op::AddI64),
        Op::SubU64 => Instead(Op::SubI64),
        Op::MulU64 => Instead(Op::MulI64),
        Op::BitandU64 => Instead(Op::BitandI64),
        Op::BitorU64 => Instead(Op::BitorI64),
        Op::BitxorU64 => Instead(Op::BitxorI64),
        Op::BitnotU64 => Instead(Op::BitnotI64),
        Op::BitclrU64 => Instead(Op::BitclrI64),
        Op::LshiftU64i => Instead(Op::LshiftI64i),
        Op::EqU64 => Instead(Op::EqI64),
        Op::NeU64 => Instead(Op::NeI64),
        Op::EqI64 => Words(Op::EqI, Op::AndI),
        Op::NeI64 => Words(Op::NeI, Op::OrI),
        // `case OP_STORE_I64:` its two words by `OP_STORE_FLD`.
        Op::StoreI64 => Expand(&[Op::StoreFld, Op::StoreFld]),
        // `QCC_PR_SimpleStatement` of the word opcodes, which the target must have.
        Op::LoadI64 => Simple(&[Op::LoadFld, Op::LoadFld]),
        Op::BitandI64 => Simple(&[Op::BitandI, Op::BitandI]),
        Op::BitorI64 => Simple(&[Op::BitorI, Op::BitorI]),
        Op::BitxorI64 => Simple(&[Op::BitxorI, Op::BitxorI]),
        // `QCC_PR_SimpleStatement(op, ..., true)`, forced, per component.
        Op::BitandV => ByWord(Op::BitandF),
        Op::BitorV => ByWord(Op::BitorF),
        // "emit this anyway. if it reaches runtime then you messed up."
        Op::LoadaStruct => Emitted,
        _ => Refuse,
    }
}

/// FTEF-27.9: the opcodes `QCC_PR_StatementFlags` replaces by others: `op = OTHER; break;`,
/// emitted with no further check, a bit clear, and a comparison of the operands swapped.
const fn substitution(op: Op) -> Option<Emulation> {
    use Emulation::{Clear, Complement, Replaced, Swapped};
    Some(match op {
        Op::BitnotF => Complement(Op::SubF),
        Op::BitnotV => Complement(Op::SubV),
        Op::BitnotI64 => Replaced(Op::SubI64),
        Op::StoreP | Op::StoreI => Replaced(Op::StoreF),
        Op::LoadP | Op::LoadI => Replaced(Op::LoadF),
        Op::StorepP => Replaced(Op::StorepI),
        Op::AddstoreF => Replaced(Op::AddF),
        Op::AddstoreI => Replaced(Op::AddI),
        Op::AddstoreFi => Replaced(Op::AddFi),
        Op::SubstoreF => Replaced(Op::SubF),
        Op::SubstoreFi => Replaced(Op::SubFi),
        Op::SubstoreI => Replaced(Op::SubI),
        Op::MulstoreF => Replaced(Op::MulF),
        Op::MulstoreFi => Replaced(Op::MulFi),
        Op::DivstoreF => Replaced(Op::DivF),
        Op::DivstoreFi => Replaced(Op::DivFi),
        Op::DivstoreI => Replaced(Op::DivI),
        Op::AddstoreV => Replaced(Op::AddV),
        Op::SubstoreV => Replaced(Op::SubV),
        Op::MulstoreVf => Replaced(Op::MulVf),
        Op::MulstoreVi => Replaced(Op::MulVi),
        Op::BitsetstoreF => Replaced(Op::BitorF),
        Op::BitsetstoreI => Replaced(Op::BitorI),
        Op::EqP => Replaced(Op::EqE),
        Op::NeP => Replaced(Op::NeE),
        Op::GtP => Replaced(Op::GtI),
        Op::GeP => Replaced(Op::GeI),
        Op::LeP => Replaced(Op::LeI),
        Op::LtP => Replaced(Op::LtI),
        Op::BitclrF => Clear {
            and: Op::BitandF,
            sub: Op::SubF,
            store: Some(Op::BitclrstoreF),
        },
        Op::BitclrstoreF => Clear {
            and: Op::BitandF,
            sub: Op::SubF,
            store: None,
        },
        Op::BitclrI => Clear {
            and: Op::BitandI,
            sub: Op::SubI,
            store: Some(Op::BitclrstoreI),
        },
        Op::BitclrstoreI => Clear {
            and: Op::BitandI,
            sub: Op::SubI,
            store: None,
        },
        Op::BitclrV => Clear {
            and: Op::BitandV,
            sub: Op::SubV,
            store: None,
        },
        Op::BitclrD => Clear {
            and: Op::BitandD,
            sub: Op::SubD,
            store: None,
        },
        Op::BitclrI64 => Clear {
            and: Op::BitandI64,
            sub: Op::SubI64,
            store: None,
        },
        Op::GeU => Swapped(Op::LeU),
        Op::GtU => Swapped(Op::LtU),
        Op::GeU64 => Swapped(Op::LeU64),
        Op::GtU64 => Swapped(Op::LtU64),
        Op::GeI64 => Swapped(Op::LeI64),
        Op::GtI64 => Swapped(Op::LtI64),
        Op::GeD => Swapped(Op::LeD),
        Op::GtD => Swapped(Op::LtD),
        _ => return None,
    })
}

/// The conversion opcodes `QCC_TryEvaluateCast` emits to make a `from` into a `to`, in order
/// (FTEF-27.8); none when it reinterprets the value or the cast is no numeric conversion.
pub(super) fn conversion_opcodes(from: Etype, to: Etype) -> &'static [Op] {
    use Etype::{Double, Float, Int64, Integer, Uint, Uint64};
    match (from, to) {
        (Uint, Float) => &[Op::ConvUf],
        (Float, Uint) => &[Op::ConvFu],
        (Integer, Float) => &[Op::ConvItof],
        (Float, Integer) => &[Op::ConvFtoi],
        (Double, Float) => &[Op::ConvDf],
        (Float, Double) => &[Op::ConvFd],
        (Int64, Float) => &[Op::ConvI64f],
        (Float, Int64) => &[Op::ConvFi64],
        (Uint64, Float) => &[Op::ConvU64f],
        (Float, Uint64) => &[Op::ConvFu64],
        (Double, Uint64) => &[Op::ConvDu64],
        (Uint64, Double) => &[Op::ConvU64d],
        (Double, Int64) => &[Op::ConvDi64],
        (Int64, Double) => &[Op::ConvI64d],
        (Double, Uint) => &[Op::ConvDu64, Op::ConvI64i],
        (Double, Integer) => &[Op::ConvDi64, Op::ConvI64i],
        (Uint, Double) => &[Op::ConvUi64, Op::ConvI64d],
        (Integer, Double) => &[Op::ConvIi64, Op::ConvI64d],
        (Uint, Int64 | Uint64) => &[Op::ConvUi64],
        (Integer, Int64 | Uint64) => &[Op::ConvIi64],
        (Int64 | Uint64, Integer | Uint) => &[Op::ConvI64i],
        _ => &[],
    }
}

/// The value of `op` emitted: a temporary of the opcode's result type.
fn replaced(op: Op) -> Value {
    row(op).map_or_else(Value::unknown, |r| Value::of(r.types[2].value_type()))
}

#[cfg(test)]
mod tests;
