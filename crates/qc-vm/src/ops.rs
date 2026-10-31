// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_exec.c, PR_ExecuteProgram

//! Executing one statement: the vanilla opcodes of VM-4 to VM-17.
//!
//! Each function here is one case of id's `PR_ExecuteProgram` switch. Every operand is used
//! only as its section names (VM-1.3), every global slot is bounds-checked (VM-1.4), and the
//! arithmetic that id's C does with `+`, `-`, string compares and float-to-int conversions is
//! written out with the wrap, truncation or exact comparison the spec fixes.

use qc_edict::EdictNumber;
use qc_edict::system::{SysField, SysGlobal};
use qc_progs::opcode::Opcode;
use qc_progs::{Progs, Statement};
use qc_types::{EntityRef, PROGS_OFS_RETURN, PointerRef, Slot};

use crate::convert::{float_to_int, int_to_float};
use crate::error::{Divergence, ErrorKind, FieldFault, RunError};
use crate::exec::{self, Builtins, Context, Observer, ServerState};
use crate::machine::{Machine, add_slot};

/// What a statement decided about the next statement (VM-3, VM-13, VM-16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Control {
    /// Advance to the next statement. `builtin` is set when the statement was a `CALL` that
    /// called a builtin (TRACE-4.3).
    Next {
        /// The builtin number a `CALL` called, if any.
        builtin: Option<u32>,
    },
    /// Jump: the next statement is this absolute index (VM-13).
    Jump(i32),
    /// Return from the current function (VM-16).
    Return,
}

/// `Control::Next` with no builtin.
const NEXT: Control = Control::Next { builtin: None };

/// The float value `1.0` and `0.0` as slots (VM notation).
const ONE: Slot = Slot::from_bits(0x3F80_0000);
const ZERO: Slot = Slot::ZERO;

/// Executes one statement, id's `PR_ExecuteProgram` switch (VM-4 to VM-17).
pub(crate) fn execute_statement(
    machine: &mut Machine,
    progs: &Progs<'_>,
    server: ServerState,
    builtins: &mut dyn Builtins,
    observer: &mut dyn Observer,
    opcode: Opcode,
    st: &Statement,
) -> Result<Control, RunError> {
    match opcode {
        // VM-4: float arithmetic.
        Opcode::AddF => float_binop(machine, progs, st, |a, b| a + b),
        Opcode::SubF => float_binop(machine, progs, st, |a, b| a - b),
        Opcode::MulF => float_binop(machine, progs, st, |a, b| a * b),
        Opcode::DivF => float_binop(machine, progs, st, |a, b| a / b),

        // VM-5: vector arithmetic.
        Opcode::AddV => vector_binop(machine, progs, st, |a, b| a + b),
        Opcode::SubV => vector_binop(machine, progs, st, |a, b| a - b),
        Opcode::MulV => mul_v(machine, progs, st),
        Opcode::MulFv => mul_fv(machine, progs, st),
        Opcode::MulVf => mul_vf(machine, progs, st),

        // VM-6: float and vector comparisons.
        Opcode::EqF => float_cmp(machine, progs, st, Cmp::Eq),
        Opcode::NeF => float_cmp(machine, progs, st, Cmp::Ne),
        Opcode::Le => float_cmp(machine, progs, st, Cmp::Le),
        Opcode::Ge => float_cmp(machine, progs, st, Cmp::Ge),
        Opcode::Lt => float_cmp(machine, progs, st, Cmp::Lt),
        Opcode::Gt => float_cmp(machine, progs, st, Cmp::Gt),
        Opcode::EqV => vector_cmp(machine, progs, st, Equality::Eq),
        Opcode::NeV => vector_cmp(machine, progs, st, Equality::Ne),

        // VM-7: string, entity and function comparisons.
        Opcode::EqS => string_cmp(machine, progs, &*builtins, st, Equality::Eq),
        Opcode::NeS => string_cmp(machine, progs, &*builtins, st, Equality::Ne),
        Opcode::EqE | Opcode::EqFnc => bits_cmp(machine, progs, st, Equality::Eq),
        Opcode::NeE | Opcode::NeFnc => bits_cmp(machine, progs, st, Equality::Ne),

        // VM-8: logical operators.
        Opcode::NotF => not_float(machine, progs, st),
        Opcode::NotV => not_vector(machine, progs, st),
        Opcode::NotS => not_string(machine, progs, &*builtins, st),
        Opcode::NotEnt => not_bits(machine, progs, st),
        Opcode::NotFnc => not_bits(machine, progs, st),
        Opcode::And => logical(machine, progs, st, Connective::And),
        Opcode::Or => logical(machine, progs, st, Connective::Or),

        // VM-9: bitwise operators.
        Opcode::BitAnd => bitwise(machine, progs, st, Connective::And),
        Opcode::BitOr => bitwise(machine, progs, st, Connective::Or),

        // VM-10: global stores.
        Opcode::StoreF
        | Opcode::StoreS
        | Opcode::StoreEnt
        | Opcode::StoreFld
        | Opcode::StoreFnc => store_scalar(machine, progs, st),
        Opcode::StoreV => store_vector(machine, progs, st),

        // VM-11: entity field loads.
        Opcode::LoadF | Opcode::LoadS | Opcode::LoadEnt | Opcode::LoadFld | Opcode::LoadFnc => {
            load_scalar(machine, progs, st)
        }
        Opcode::LoadV => load_vector(machine, progs, st),

        // VM-12: field addresses and pointer stores.
        Opcode::Address => address(machine, progs, server, st),
        Opcode::StorepF
        | Opcode::StorepS
        | Opcode::StorepEnt
        | Opcode::StorepFld
        | Opcode::StorepFnc => storep_scalar(machine, progs, st),
        Opcode::StorepV => storep_vector(machine, progs, st),

        // VM-13: jumps.
        Opcode::Goto => goto(machine, st),
        Opcode::If => branch(machine, progs, st, JumpWhen::Nonzero),
        Opcode::IfNot => branch(machine, progs, st, JumpWhen::Zero),

        // VM-14: calls.
        Opcode::Call0 => call(machine, progs, server, builtins, observer, st, 0),
        Opcode::Call1 => call(machine, progs, server, builtins, observer, st, 1),
        Opcode::Call2 => call(machine, progs, server, builtins, observer, st, 2),
        Opcode::Call3 => call(machine, progs, server, builtins, observer, st, 3),
        Opcode::Call4 => call(machine, progs, server, builtins, observer, st, 4),
        Opcode::Call5 => call(machine, progs, server, builtins, observer, st, 5),
        Opcode::Call6 => call(machine, progs, server, builtins, observer, st, 6),
        Opcode::Call7 => call(machine, progs, server, builtins, observer, st, 7),
        Opcode::Call8 => call(machine, progs, server, builtins, observer, st, 8),

        // VM-16: returning.
        Opcode::Done | Opcode::Return => done(machine, progs, st),

        // VM-17: STATE.
        Opcode::State => state(machine, progs, st),
    }
}

// ---------------------------------------------------------------------------
// Operand access
// ---------------------------------------------------------------------------

/// Reads operand slot `slot`, bounds-checked (VM-1.3, VM-1.4).
pub(crate) fn read_slot(machine: &Machine, progs: &Progs<'_>, slot: i32) -> Result<Slot, RunError> {
    machine
        .global(slot)
        .map_err(|e| exec::global_error(machine, progs, e))
}

/// Reads the float in operand slot `slot`, bounds-checked (VM-1.3, VM-1.4).
fn read_float(machine: &Machine, progs: &Progs<'_>, slot: i32) -> Result<f32, RunError> {
    Ok(read_slot(machine, progs, slot)?.as_f32())
}

/// Reads the three floats of a vector operand, slots `slot`, `slot + 1`, `slot + 2`.
fn read_vec(machine: &Machine, progs: &Progs<'_>, slot: i32) -> Result<[f32; 3], RunError> {
    let cells = machine
        .global3(slot)
        .map_err(|e| exec::global_error(machine, progs, e))?;
    Ok([cells[0].as_f32(), cells[1].as_f32(), cells[2].as_f32()])
}

/// Writes a slot to operand slot `slot`, bounds-checked (VM-1.4).
pub(crate) fn write_slot(
    machine: &mut Machine,
    progs: &Progs<'_>,
    slot: i32,
    value: Slot,
) -> Result<(), RunError> {
    machine
        .set_global(slot, value)
        .map_err(|e| exec::global_error(machine, progs, e))
}

/// Writes a float result to operand slot `slot`.
fn write_float(
    machine: &mut Machine,
    progs: &Progs<'_>,
    slot: i32,
    value: f32,
) -> Result<(), RunError> {
    write_slot(machine, progs, slot, Slot::from_f32(value))
}

/// A float comparison result as the `1.0`/`0.0` slot it writes (VM-6.1).
const fn bool_slot(value: bool) -> Slot {
    if value { ONE } else { ZERO }
}

// ---------------------------------------------------------------------------
// VM-4 float arithmetic
// ---------------------------------------------------------------------------

/// `c = f(a, b)` for the float binary opcodes (VM-4.1, VM-4.4): both operands read before `c`
/// is written.
fn float_binop(
    machine: &mut Machine,
    progs: &Progs<'_>,
    st: &Statement,
    f: impl Fn(f32, f32) -> f32,
) -> Result<Control, RunError> {
    let a = read_float(machine, progs, st.a)?;
    let b = read_float(machine, progs, st.b)?;
    // VM-4.1: IEEE 754 single precision; +, -, * and / on two f32 round once to f32.
    let c = f(a, b);
    write_float(machine, progs, st.c, c)?;
    Ok(NEXT)
}

// ---------------------------------------------------------------------------
// VM-5 vector arithmetic
// ---------------------------------------------------------------------------

/// `ADD_V` and `SUB_V`: `c[i] = f(a[i], b[i])` for i = 0, 1, 2 in order, each component read
/// after the previous one's result is written (VM-5.1).
fn vector_binop(
    machine: &mut Machine,
    progs: &Progs<'_>,
    st: &Statement,
    f: impl Fn(f32, f32) -> f32,
) -> Result<Control, RunError> {
    for i in 0..3_i32 {
        let a = read_float(machine, progs, add_operand(machine, progs, st.a, i)?)?;
        let b = read_float(machine, progs, add_operand(machine, progs, st.b, i)?)?;
        // VM-5.1: each component is one f32 add or subtract (VM-4.1).
        let c = f(a, b);
        write_float(machine, progs, add_operand(machine, progs, st.c, i)?, c)?;
    }
    Ok(NEXT)
}

/// `MUL_V`: the dot product, all six inputs read before the one result is written (VM-5.2).
fn mul_v(machine: &mut Machine, progs: &Progs<'_>, st: &Statement) -> Result<Control, RunError> {
    let a = read_vec(machine, progs, st.a)?;
    let b = read_vec(machine, progs, st.b)?;
    // VM-5.2: (a0*b0 + a1*b1) + a2*b2, each product and sum rounded to f32, left to right,
    // no fused multiply-add.
    let c = (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2];
    write_float(machine, progs, st.c, c)?;
    Ok(NEXT)
}

/// `MUL_FV`: `c[i] = a * b[i]`, rereading the scalar `a` for each component (VM-5.3).
fn mul_fv(machine: &mut Machine, progs: &Progs<'_>, st: &Statement) -> Result<Control, RunError> {
    for i in 0..3_i32 {
        let a = read_float(machine, progs, st.a)?;
        let b = read_float(machine, progs, add_operand(machine, progs, st.b, i)?)?;
        // VM-5.3: id's C f32 multiply, the scalar reread each component.
        let c = a * b;
        write_float(machine, progs, add_operand(machine, progs, st.c, i)?, c)?;
    }
    Ok(NEXT)
}

/// `MUL_VF`: `c[i] = b * a[i]`, rereading the scalar `b` for each component (VM-5.4).
fn mul_vf(machine: &mut Machine, progs: &Progs<'_>, st: &Statement) -> Result<Control, RunError> {
    for i in 0..3_i32 {
        let a = read_float(machine, progs, add_operand(machine, progs, st.a, i)?)?;
        let b = read_float(machine, progs, st.b)?;
        // VM-5.4: id's C f32 multiply, the scalar reread each component.
        let c = b * a;
        write_float(machine, progs, add_operand(machine, progs, st.c, i)?, c)?;
    }
    Ok(NEXT)
}

/// `operand + i` as an operand slot, failing as a VM-1.4 error on overflow.
fn add_operand(
    machine: &Machine,
    progs: &Progs<'_>,
    operand: i32,
    i: i32,
) -> Result<i32, RunError> {
    add_slot(operand, i).map_err(|e| exec::global_error(machine, progs, e))
}

// ---------------------------------------------------------------------------
// VM-6 float and vector comparisons
// ---------------------------------------------------------------------------

/// One of the six float comparisons (VM-6.1).
#[derive(Clone, Copy, Debug)]
enum Cmp {
    Eq,
    Ne,
    Le,
    Ge,
    Lt,
    Gt,
}

/// `EQ_F`, `NE_F`, `LE`, `GE`, `LT`, `GT`: write `1.0` or `0.0` by the float comparison
/// (VM-6.1, VM-6.2). All inputs read before `c` is written (VM-6.4).
fn float_cmp(
    machine: &mut Machine,
    progs: &Progs<'_>,
    st: &Statement,
    cmp: Cmp,
) -> Result<Control, RunError> {
    let a = read_float(machine, progs, st.a)?;
    let b = read_float(machine, progs, st.b)?;
    let result = match cmp {
        // VM-6.1, VM-6.2: EQ_F and NE_F are IEEE 754 == and !=; NaN is unordered, so
        // == is false and != true when either operand is a NaN, and -0 equals +0.
        Cmp::Eq => {
            #[expect(
                clippy::float_cmp,
                reason = "spec VM-6.1, VM-6.2: exact IEEE 754 equality, NaN unordered, as id's C =="
            )]
            let eq = a == b;
            eq
        }
        Cmp::Ne => {
            #[expect(
                clippy::float_cmp,
                reason = "spec VM-6.1, VM-6.2: exact IEEE 754 inequality, NaN unordered, as id's C !="
            )]
            let ne = a != b;
            ne
        }
        // VM-6.1, VM-6.2: the ordered comparisons; a NaN operand makes them false.
        Cmp::Le => a <= b,
        Cmp::Ge => a >= b,
        Cmp::Lt => a < b,
        Cmp::Gt => a > b,
    };
    write_slot(machine, progs, st.c, bool_slot(result))?;
    Ok(NEXT)
}

/// Whether a comparison opcode is the `EQ_` or the `NE_` of its pair (VM-6.3, VM-7).
#[derive(Clone, Copy, Debug)]
enum Equality {
    Eq,
    Ne,
}

impl Equality {
    /// The opcode's answer when the operands are, or are not, the same.
    const fn holds(self, same: bool) -> bool {
        match self {
            Self::Eq => same,
            Self::Ne => !same,
        }
    }
}

/// `EQ_V` and `NE_V` (VM-6.3): all three components compared as VM-6.2; `c` may overlap
/// (VM-6.4).
fn vector_cmp(
    machine: &mut Machine,
    progs: &Progs<'_>,
    st: &Statement,
    equality: Equality,
) -> Result<Control, RunError> {
    let a = read_vec(machine, progs, st.a)?;
    let b = read_vec(machine, progs, st.b)?;
    // VM-6.3: EQ_V is 1.0 when every component is equal as VM-6.2, NE_V is 1.0 when any
    // is unequal; a NaN component makes EQ_V 0.0 and NE_V 1.0.
    #[expect(
        clippy::float_cmp,
        reason = "spec VM-6.3: per-component IEEE 754 equality, NaN unordered, as id's C"
    )]
    let all_equal = a[0] == b[0] && a[1] == b[1] && a[2] == b[2];
    let result = equality.holds(all_equal);
    write_slot(machine, progs, st.c, bool_slot(result))?;
    Ok(NEXT)
}

// ---------------------------------------------------------------------------
// VM-7 string, entity and function comparisons
// ---------------------------------------------------------------------------

/// Resolves a string value to its bytes up to NUL, id's `pr_strings + value` (PROGS-11.1).
///
/// VM-7.3: a string value is a string-table offset or a string the engine allocated
/// (PROGS-11.6, EDICT-13.1). A value from 0 to `numstrings - 1` is a table offset; any other
/// value is resolved against the engine-allocated strings the [`Builtins`] dispatch owns
/// (EDICT-13.3), id's `PR_GetString` reading `pr_strings + value` for both. Only a value that
/// is neither a table offset nor an engine string names memory outside both, which is
/// undefined in C; this spec fails it with the VM-19.7 runtime error.
fn resolve_string<'a>(
    progs: &'a Progs<'_>,
    builtins: &'a dyn Builtins,
    value: i32,
) -> Result<&'a [u8], ()> {
    // A value from 0 to numstrings - 1 is a table offset (PROGS-11.6). offset 0 is the empty
    // string. The loader terminates every referenced string, but an arbitrary offset into the
    // table must still reach a NUL; progs.string returns None otherwise.
    let numstrings = progs.strings().len();
    match usize::try_from(value) {
        Ok(offset) if offset < numstrings => progs.string(value).ok_or(()),
        // Outside the table: an engine-allocated string (EDICT-13.1, EDICT-13.3), resolved
        // through the dispatch that owns it. id's PR_GetString reads it the same way.
        _ => builtins.engine_string(value).ok_or(()),
    }
}

/// `EQ_S` and `NE_S` (VM-7.1, VM-7.2): compare by content with `strcmp`.
///
/// A string value outside the table is an engine-allocated string resolved through `builtins`
/// (VM-7.3, PROGS-11.6, EDICT-13.1).
fn string_cmp(
    machine: &mut Machine,
    progs: &Progs<'_>,
    builtins: &dyn Builtins,
    st: &Statement,
    equality: Equality,
) -> Result<Control, RunError> {
    let a_value = read_slot(machine, progs, st.a)?.as_i32();
    let b_value = read_slot(machine, progs, st.b)?.as_i32();
    let a = resolve_string(progs, builtins, a_value)
        .map_err(|()| machine.divergence(progs, Divergence::BadString { value: a_value }))?;
    let b = resolve_string(progs, builtins, b_value)
        .map_err(|()| machine.divergence(progs, Divergence::BadString { value: b_value }))?;
    match equality {
        // VM-7.1: EQ_S writes 1.0 when the strings are equal.
        Equality::Eq => write_slot(machine, progs, st.c, bool_slot(a == b))?,
        // VM-7.2: NE_S writes strcmp's result as a float: 0.0 when equal, else the first
        // differing byte of a minus that of b as unsigned bytes, the NUL counting as 0.
        Equality::Ne => write_float(machine, progs, st.c, int_to_float(strcmp(a, b)))?,
    }
    Ok(NEXT)
}

/// id's `strcmp` reduced to the byte difference VM-7.2 fixes: 0 when equal, else the first
/// differing byte of `a` minus that of `b` as unsigned bytes, the shorter string's NUL as 0.
fn strcmp(a: &[u8], b: &[u8]) -> i32 {
    let mut i = 0_usize;
    loop {
        let ca = a.get(i).copied();
        let cb = b.get(i).copied();
        match (ca, cb) {
            (None, None) => return 0,
            _ => {
                let va = i32::from(ca.unwrap_or(0));
                let vb = i32::from(cb.unwrap_or(0));
                if va != vb {
                    return va.wrapping_sub(vb);
                }
                if ca.is_none() && cb.is_none() {
                    return 0;
                }
                i = i.wrapping_add(1);
            }
        }
    }
}

/// `EQ_E`, `NE_E`, `EQ_FNC`, `NE_FNC` (VM-7.4): compare the 32-bit patterns, not floats.
fn bits_cmp(
    machine: &mut Machine,
    progs: &Progs<'_>,
    st: &Statement,
    equality: Equality,
) -> Result<Control, RunError> {
    let a = read_slot(machine, progs, st.a)?;
    let b = read_slot(machine, progs, st.b)?;
    // VM-7.4: EQ_E compares bits; a NaN pattern equals itself, -0 and +0 differ.
    let same = a.bits() == b.bits();
    let result = equality.holds(same);
    write_slot(machine, progs, st.c, bool_slot(result))?;
    Ok(NEXT)
}

// ---------------------------------------------------------------------------
// VM-8 logical operators
// ---------------------------------------------------------------------------

/// `NOT_F` (VM-8.1): 1.0 when the float is 0.0 (for +0 and -0), 0.0 otherwise including NaN.
fn not_float(
    machine: &mut Machine,
    progs: &Progs<'_>,
    st: &Statement,
) -> Result<Control, RunError> {
    let a = read_float(machine, progs, st.a)?;
    // VM-8.1: NOT_F writes 1.0 when a == 0.0, so for +0 and -0; NaN gives 0.0.
    let is_zero = a == 0.0;
    write_slot(machine, progs, st.c, bool_slot(is_zero))?;
    Ok(NEXT)
}

/// `NOT_V` (VM-8.2): 1.0 when all three components equal 0.0.
fn not_vector(
    machine: &mut Machine,
    progs: &Progs<'_>,
    st: &Statement,
) -> Result<Control, RunError> {
    let a = read_vec(machine, progs, st.a)?;
    // VM-8.2: NOT_V writes 1.0 when every component equals 0.0.
    let all_zero = a[0] == 0.0 && a[1] == 0.0 && a[2] == 0.0;
    write_slot(machine, progs, st.c, bool_slot(all_zero))?;
    Ok(NEXT)
}

/// `NOT_S` (VM-8.3): 1.0 when the string value is 0 or names an empty string.
///
/// A string value outside the table is an engine-allocated string resolved through `builtins`
/// (VM-8.3, PROGS-11.6, EDICT-13.1); its text decides whether it is empty (EDICT-13.2).
fn not_string(
    machine: &mut Machine,
    progs: &Progs<'_>,
    builtins: &dyn Builtins,
    st: &Statement,
) -> Result<Control, RunError> {
    let value = read_slot(machine, progs, st.a)?.as_i32();
    let bytes = resolve_string(progs, builtins, value)
        .map_err(|()| machine.divergence(progs, Divergence::BadString { value }))?;
    // VM-8.3: NOT_S writes 1.0 when the value is 0 or the first byte is NUL.
    let empty = bytes.first().copied().unwrap_or(0) == 0;
    write_slot(machine, progs, st.c, bool_slot(empty))?;
    Ok(NEXT)
}

/// `NOT_ENT` and `NOT_FNC` (VM-8.4, VM-8.5): 1.0 when the 32-bit value is 0.
fn not_bits(machine: &mut Machine, progs: &Progs<'_>, st: &Statement) -> Result<Control, RunError> {
    let a = read_slot(machine, progs, st.a)?;
    // VM-8.4, VM-8.5: NOT_ENT and NOT_FNC write 1.0 when the value's bits are 0.
    write_slot(machine, progs, st.c, bool_slot(a.bits() == 0))?;
    Ok(NEXT)
}

/// Whether a logical or bitwise opcode is the `AND` or the `OR` of its pair (VM-8.6, VM-9).
#[derive(Clone, Copy, Debug)]
enum Connective {
    And,
    Or,
}

/// `AND` and `OR` (VM-8.6): both operands read as floats; -0 is zero, NaN is non-zero.
fn logical(
    machine: &mut Machine,
    progs: &Progs<'_>,
    st: &Statement,
    connective: Connective,
) -> Result<Control, RunError> {
    let a = read_float(machine, progs, st.a)?;
    let b = read_float(machine, progs, st.b)?;
    // VM-8.6: AND is 1.0 when both floats are non-zero, OR when either is; a NaN is
    // non-zero and -0 is zero. Both operands are always read.
    let (ta, tb) = (a != 0.0, b != 0.0);
    let result = match connective {
        Connective::And => ta && tb,
        Connective::Or => ta || tb,
    };
    write_slot(machine, progs, st.c, bool_slot(result))?;
    Ok(NEXT)
}

// ---------------------------------------------------------------------------
// VM-9 bitwise operators
// ---------------------------------------------------------------------------

/// `BITAND` and `BITOR` (VM-9): convert each float to an i32 by truncation, AND or OR, convert
/// back to float.
fn bitwise(
    machine: &mut Machine,
    progs: &Progs<'_>,
    st: &Statement,
    connective: Connective,
) -> Result<Control, RunError> {
    let a = float_to_int(read_float(machine, progs, st.a)?);
    let b = float_to_int(read_float(machine, progs, st.b)?);
    let result = match connective {
        Connective::And => a & b,
        Connective::Or => a | b,
    };
    // VM-9.1: the integer result is converted back to float, rounded to nearest even.
    write_float(machine, progs, st.c, int_to_float(result))?;
    Ok(NEXT)
}

// ---------------------------------------------------------------------------
// VM-10 global stores
// ---------------------------------------------------------------------------

/// `STORE_F`, `STORE_S`, `STORE_ENT`, `STORE_FLD`, `STORE_FNC` (VM-10.1, VM-10.4): copy the
/// 32-bit pattern of `a` to `b`; `c` is unused. The type in the name is not checked.
fn store_scalar(
    machine: &mut Machine,
    progs: &Progs<'_>,
    st: &Statement,
) -> Result<Control, RunError> {
    let value = read_slot(machine, progs, st.a)?;
    // VM-10.1, VM-10.3: a bit-exact copy, every NaN pattern and -0.0 included.
    write_slot(machine, progs, st.b, value)?;
    Ok(NEXT)
}

/// `STORE_V` (VM-10.2): copy slots a, a+1, a+2 to b, b+1, b+2 one at a time in order, so an
/// overlap sees the slots already written.
fn store_vector(
    machine: &mut Machine,
    progs: &Progs<'_>,
    st: &Statement,
) -> Result<Control, RunError> {
    // VM-1.4: all six slots must be inside the globals; checked before any is written, so a
    // partly out-of-range store writes nothing (vector_partly_out_of_range).
    for i in 0..3_i32 {
        let _ = read_slot(machine, progs, add_operand(machine, progs, st.a, i)?)?;
        let _ = read_slot(machine, progs, add_operand(machine, progs, st.b, i)?)?;
    }
    for i in 0..3_i32 {
        let value = read_slot(machine, progs, add_operand(machine, progs, st.a, i)?)?;
        write_slot(machine, progs, add_operand(machine, progs, st.b, i)?, value)?;
    }
    Ok(NEXT)
}

// ---------------------------------------------------------------------------
// VM-11 entity field loads
// ---------------------------------------------------------------------------

/// Resolves the entity value in `a` and the field index in `b` to an edict number and a field
/// slot, failing with VM-11.4 when either is out of range.
fn load_target(
    machine: &Machine,
    progs: &Progs<'_>,
    st: &Statement,
) -> Result<(EdictNumber, usize), RunError> {
    let entity = read_slot(machine, progs, st.a)?.as_i32();
    let field = read_slot(machine, progs, st.b)?.as_i32();
    // VM-11.4: a value that does not name an edict, or a field slot outside 0..entityfields,
    // fails.
    let number = machine
        .edicts
        .num_for_edict(EntityRef::from_bits(entity))
        .map_err(|_| machine.divergence(progs, Divergence::Load(FieldFault::NotAnEdict)))?;
    let field_index = usize::try_from(field)
        .ok()
        .filter(|f| *f < machine.edicts.layout().entityfields())
        .ok_or_else(|| machine.divergence(progs, Divergence::Load(FieldFault::FieldOutOfRange)))?;
    Ok((number, field_index))
}

/// `LOAD_F`, `LOAD_S`, `LOAD_ENT`, `LOAD_FLD`, `LOAD_FNC` (VM-11.1): copy one field slot to c.
fn load_scalar(
    machine: &mut Machine,
    progs: &Progs<'_>,
    st: &Statement,
) -> Result<Control, RunError> {
    let (number, field) = load_target(machine, progs, st)?;
    let value = machine
        .edicts
        .field(number, field)
        .ok_or_else(|| machine.divergence(progs, Divergence::Load(FieldFault::FieldSlotMissing)))?;
    write_slot(machine, progs, st.c, value)?;
    Ok(NEXT)
}

/// `LOAD_V` (VM-11.2): copy field slots b, b+1, b+2 to c, c+1, c+2.
fn load_vector(
    machine: &mut Machine,
    progs: &Progs<'_>,
    st: &Statement,
) -> Result<Control, RunError> {
    let (number, field) = load_target(machine, progs, st)?;
    // The three field slots must all exist (VM-11.4): b+2 below entityfields.
    let last = field
        .checked_add(2)
        .filter(|f| *f < machine.edicts.layout().entityfields());
    if last.is_none() {
        return Err(machine.divergence(progs, Divergence::Load(FieldFault::VectorFieldOutOfRange)));
    }
    for i in 0..3_usize {
        let f = field.wrapping_add(i);
        let value = machine.edicts.field(number, f).ok_or_else(|| {
            machine.divergence(progs, Divergence::Load(FieldFault::FieldSlotMissing))
        })?;
        write_slot(
            machine,
            progs,
            add_operand(machine, progs, st.c, i32_from(i))?,
            value,
        )?;
    }
    Ok(NEXT)
}

// ---------------------------------------------------------------------------
// VM-12 field addresses and pointer stores
// ---------------------------------------------------------------------------

/// `ADDRESS` (VM-12.1, VM-12.2, VM-12.3): write a pointer to field slot b of entity a.
fn address(
    machine: &mut Machine,
    progs: &Progs<'_>,
    server: ServerState,
    st: &Statement,
) -> Result<Control, RunError> {
    let entity = read_slot(machine, progs, st.a)?.as_i32();
    let field = read_slot(machine, progs, st.b)?.as_i32();
    // VM-12.2: ADDRESS on the world fails while the server is active, succeeds while loading.
    if entity == 0 && server == ServerState::Active {
        return Err(machine.run_error(progs, ErrorKind::AssignmentToWorld));
    }
    // VM-12.3: ADDRESS checks neither the field index nor whether the edict is free, but the
    // pointer must name a real field slot of a real edict to have meaning; resolve it.
    let number = machine
        .edicts
        .num_for_edict(EntityRef::from_bits(entity))
        .map_err(|_| machine.divergence(progs, Divergence::Pointer(FieldFault::NotAnEdict)))?;
    let field_index = usize::try_from(field)
        .ok()
        .filter(|f| *f < machine.edicts.layout().entityfields())
        .ok_or_else(|| {
            machine.divergence(progs, Divergence::Pointer(FieldFault::FieldOutOfRange))
        })?;
    let pointer = machine
        .edicts
        .layout()
        .pointer_value(number, field_index)
        .ok_or_else(|| {
            machine.divergence(progs, Divergence::Pointer(FieldFault::PointerOutOfRange))
        })?;
    write_slot(machine, progs, st.c, Slot::from(pointer))?;
    Ok(NEXT)
}

/// Resolves the pointer in `b` to an edict number and field slot (VM-12.4, VM-12.5).
fn storep_target(
    machine: &Machine,
    progs: &Progs<'_>,
    pointer_slot: i32,
) -> Result<(EdictNumber, usize), RunError> {
    let pointer = read_slot(machine, progs, pointer_slot)?.as_i32();
    // VM-12.5: a pointer that does not name a field slot of an edict fails without writing.
    let (number, field) =
        pointer_target(machine, PointerRef::from_bits(pointer)).ok_or_else(|| {
            machine.divergence(progs, Divergence::Pointer(FieldFault::PointerNotAField))
        })?;
    Ok((number, field))
}

/// The edict number and field slot a pointer value names, by inverting EDICT-1.6.
fn pointer_target(machine: &Machine, pointer: PointerRef) -> Option<(EdictNumber, usize)> {
    machine
        .edicts
        .layout()
        .pointer_slot(pointer)
        .filter(|&(number, _)| number.index() < machine.edicts.num_edicts())
}

/// `STOREP_F`, `STOREP_S`, `STOREP_ENT`, `STOREP_FLD`, `STOREP_FNC` (VM-12.4): copy a to the
/// field slot the pointer in b names.
fn storep_scalar(
    machine: &mut Machine,
    progs: &Progs<'_>,
    st: &Statement,
) -> Result<Control, RunError> {
    let (number, field) = storep_target(machine, progs, st.b)?;
    let value = read_slot(machine, progs, st.a)?;
    // The pointer names an edict's field slot (VM-12.4), so the write cannot miss.
    let _written = machine.edicts.set_field(number, field, value);
    Ok(NEXT)
}

/// `STOREP_V` (VM-12.4): copy a, a+1, a+2 to the pointer's field slot and the next two.
fn storep_vector(
    machine: &mut Machine,
    progs: &Progs<'_>,
    st: &Statement,
) -> Result<Control, RunError> {
    let (number, field) = storep_target(machine, progs, st.b)?;
    // VM-12.5: all three field slots must be of the one edict; checked before any write.
    let last = field
        .checked_add(2)
        .filter(|f| *f < machine.edicts.layout().entityfields());
    if last.is_none() {
        return Err(machine.divergence(
            progs,
            Divergence::Pointer(FieldFault::VectorPointerOutOfRange),
        ));
    }
    for i in 0..3_usize {
        let value = read_slot(
            machine,
            progs,
            add_operand(machine, progs, st.a, i32_from(i))?,
        )?;
        // A vector whose y or z would fall past the edict's last field drops that component.
        let _written = machine
            .edicts
            .set_field(number, field.wrapping_add(i), value);
    }
    Ok(NEXT)
}

// ---------------------------------------------------------------------------
// VM-13 jumps
// ---------------------------------------------------------------------------

/// `GOTO` (VM-13.1): next statement is the current one plus the signed offset `a`.
fn goto(machine: &Machine, st: &Statement) -> Result<Control, RunError> {
    Ok(Control::Jump(machine.xstatement.wrapping_add(st.a)))
}

/// When a conditional jump jumps: `IF` on a non-zero pattern, `IFNOT` on zero (VM-13.2).
#[derive(Clone, Copy, Debug)]
enum JumpWhen {
    Nonzero,
    Zero,
}

impl JumpWhen {
    const fn jumps(self, nonzero: bool) -> bool {
        match self {
            Self::Nonzero => nonzero,
            Self::Zero => !nonzero,
        }
    }
}

/// `IF` and `IFNOT` (VM-13.2, VM-13.3): test the 32-bit pattern in `a`, jump by `b`.
fn branch(
    machine: &Machine,
    progs: &Progs<'_>,
    st: &Statement,
    when: JumpWhen,
) -> Result<Control, RunError> {
    let value = read_slot(machine, progs, st.a)?;
    // VM-13.3: the test is on the 32-bit pattern, so -0.0 and every NaN are true.
    let nonzero = value.bits() != 0;
    if when.jumps(nonzero) {
        Ok(Control::Jump(machine.xstatement.wrapping_add(st.b)))
    } else {
        Ok(NEXT)
    }
}

// ---------------------------------------------------------------------------
// VM-14 calls
// ---------------------------------------------------------------------------

/// `CALL0` to `CALL8` (VM-14): call the function in `a` with `argc` arguments.
fn call(
    machine: &mut Machine,
    progs: &Progs<'_>,
    server: ServerState,
    builtins: &mut dyn Builtins,
    observer: &mut dyn Observer,
    st: &Statement,
    argc: usize,
) -> Result<Control, RunError> {
    let index = read_slot(machine, progs, st.a)?.as_i32();
    // VM-14.1: set pr_argc to the number in the opcode name.
    machine.argc = argc;

    // VM-14.2: a function index of 0 fails with NULL function.
    if index == 0 {
        return Err(machine.run_error(progs, ErrorKind::NullCallFunction));
    }
    // VM-14.3: a negative index or one not less than numfunctions fails.
    let Some(function) = exec::function_at(progs, index) else {
        return Err(machine.divergence(progs, Divergence::CallOutOfRange { index }));
    };

    match function.builtin_number() {
        Some(number) => {
            // VM-14.7: a builtin; its number is -first_statement, below the table's limit.
            let limit = builtins.limit();
            let number = u32::try_from(number)
                .ok()
                .filter(|n| limit.is_none_or(|limit| *n < limit));
            let Some(number) = number else {
                return Err(machine.run_error(progs, ErrorKind::BadBuiltin));
            };
            if number == 0 {
                // LIMIT-5.2: builtin 0 cannot be called; but first_statement 0 is not
                // negative, so builtin_number never returns 0. Reject defensively.
                return Err(machine.run_error(progs, ErrorKind::BadBuiltin));
            }
            // VM-14.7: call the builtin directly; no frame, no locals, no parameter copy.
            let mut context = Context {
                machine,
                progs,
                server,
                observer,
            };
            builtins.call(number, &mut context)?;
            Ok(Control::Next {
                builtin: Some(number),
            })
        }
        None => {
            // VM-14.5: a QC function is entered; execution continues at its first_statement.
            exec::enter_function(machine, progs, index, &function)?;
            Ok(NEXT)
        }
    }
}

// ---------------------------------------------------------------------------
// VM-16 returning
// ---------------------------------------------------------------------------

/// `DONE` and `RETURN` (VM-16.1, VM-16.2, VM-16.5): copy a, a+1, a+2 to OFS_RETURN, one slot
/// at a time in order, bit-exact.
fn done(machine: &mut Machine, progs: &Progs<'_>, st: &Statement) -> Result<Control, RunError> {
    let ret = i32::from(PROGS_OFS_RETURN);
    for i in 0..3_i32 {
        let value = read_slot(machine, progs, add_operand(machine, progs, st.a, i)?)?;
        write_slot(machine, progs, add_operand(machine, progs, ret, i)?, value)?;
    }
    Ok(Control::Return)
}

// ---------------------------------------------------------------------------
// VM-17 STATE
// ---------------------------------------------------------------------------

/// `STATE` (VM-17): set the entity in `self`'s nextthink, frame and think fields.
fn state(machine: &mut Machine, progs: &Progs<'_>, st: &Statement) -> Result<Control, RunError> {
    // VM-17.1: STATE acts on the entity in the global self (slot 28).
    let self_value = read_slot(machine, progs, SELF_SLOT)?.as_i32();
    let number = machine
        .edicts
        .num_for_edict(EntityRef::from_bits(self_value))
        .map_err(|_| machine.divergence(progs, Divergence::StateSelf))?;

    // VM-17.1: nextthink = (float)((double)time + 0.1), 0.1 the double.
    let time = read_slot(machine, progs, TIME_SLOT)?.as_f32();
    let next_think = state_think_time(time);
    // A progs with fewer fields than nextthink, frame or think has nowhere to write them; the
    // write is dropped.
    let _written = machine
        .edicts
        .set_field(number, FIELD_NEXTTHINK, Slot::from_f32(next_think));

    // VM-17.2: frame = a's float only when a != frame as VM-6.2 compares.
    let a = read_slot(machine, progs, st.a)?;
    let frame_now = machine
        .edicts
        .field(number, FIELD_FRAME)
        .unwrap_or(Slot::ZERO);
    // VM-17.2: set frame only when the new float is not equal to the current frame; -0
    // equals +0, a NaN new value is not equal to anything so it is written.
    #[expect(
        clippy::float_cmp,
        reason = "spec VM-17.2: id's C if (ed->v.frame != a->_float), an exact IEEE comparison"
    )]
    let differs = a.as_f32() != frame_now.as_f32();
    if differs {
        let _written = machine.edicts.set_field(number, FIELD_FRAME, a);
    }

    // VM-17.3: think = the 32-bit pattern in b, a function index, not checked.
    let b = read_slot(machine, progs, st.b)?;
    let _written = machine.edicts.set_field(number, FIELD_THINK, b);
    Ok(NEXT)
}

/// `(float)((double)time + 0.1)`, 0.1 the double nearest to 0.1 (VM-17.1).
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec VM-17.1: id's C (float)((double)time + 0.1); the double sum is rounded to f32 as the field stores a float"
)]
fn state_think_time(time: f32) -> f32 {
    (f64::from(time) + VM_STATE_THINK_INTERVAL) as f32
}

/// The think interval VM-17.1 fixes, `0.1` as a double.
///
/// `VM_STATE_THINK_INTERVAL` in VM-17 (`docs/spec/vm.md`).
const VM_STATE_THINK_INTERVAL: f64 = 0.1;

/// The `self` and `time` global slots and the `frame`, `think` and `nextthink` field slots
/// from id's `progdefs.q1` (VM-17.4).
const SELF_SLOT: i32 = SysGlobal::SelfEntity.ofs();
const TIME_SLOT: i32 = SysGlobal::Time.ofs();
const FIELD_FRAME: usize = SysField::Frame.slot();
const FIELD_THINK: usize = SysField::Think.slot();
const FIELD_NEXTTHINK: usize = SysField::Nextthink.slot();

/// `usize` as `i32`, saturating; a slot index is always small here.
fn i32_from(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}
