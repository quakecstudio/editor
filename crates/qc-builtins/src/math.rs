// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_cmds.c, PF_makevectors, PF_normalize, PF_vlen, PF_vectoyaw, PF_vectoangles, PF_random, PF_rint, PF_floor, PF_ceil, PF_fabs, PF_ftos, PF_vtos
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/mathlib.c, AngleVectors, anglemod
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/common.c, Q_atof

//! The pure math and string builtins, exact without an engine (BUILTIN-4.1).
//!
//! Implements the float-to-int rule of BUILTIN-2.1 and BUILTIN-2.2, the math of BUILTIN-6,
//! the string formatting of BUILTIN-7, and `anglemod` of BUILTIN-15.5. The arithmetic is
//! id's C: `float` operations round to f32, a `double` constant makes the expression double,
//! and the result is rounded to f32 when stored (BUILTIN-6). Each function gives the
//! expression as id writes it, because that decides the last bit.

use core::f64::consts::PI;

use qc_vm::convert::{double_to_float, double_to_int, float_to_int, int_to_float};

/// The mask id's `PF_random` applies to `rand()` (BUILTIN-6.6).
///
/// `BUILTIN_RANDOM_MASK` in BUILTIN-6: `(rand () & 0x7fff) / ((float)0x7fff)`.
pub const BUILTIN_RANDOM_MASK: u32 = 0x7fff;

/// `makevectors(ang)`: the forward, right and up vectors of `AngleVectors` (BUILTIN-6.1).
///
/// Returns `(v_forward, v_right, v_up)`. Each component angle `a` is
/// `(f32)(a * (M_PI*2/360))` in double, `sy`/`cy`/`sp`/`cp`/`sr`/`cr` its f32 sine and cosine
/// (computed in double) for yaw (`ang_y`), pitch (`ang_x`) and roll (`ang_z`); the rest is f32.
///
/// id's `AngleVectors` stores the angle in a `float` (`angle = angles[YAW] * (M_PI*2 / 360)`),
/// so the double product is rounded to f32 before `sin`/`cos`. `deg_to_rad` does the same
/// `(f32)(...)` round; a 360-degree yaw therefore gives id's `sin` of `~1.7e-7`, not the
/// `~-2.4e-16` of the un-rounded `2*PI` (BUILTIN-6.1).
#[must_use]
pub fn makevectors(ang: [f32; 3]) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let deg_to_rad =
        |a: f32| -> f64 { f64::from(double_to_float(f64::from(a) * (PI * 2.0 / 360.0))) };
    let (sy, cy) = sin_cos_f32(deg_to_rad(ang[1]));
    let (sp, cp) = sin_cos_f32(deg_to_rad(ang[0]));
    let (sr, cr) = sin_cos_f32(deg_to_rad(ang[2]));
    let forward = [cp * cy, cp * sy, -sp];
    #[expect(
        clippy::neg_multiply,
        reason = "spec BUILTIN-6.1: AngleVectors' -1*... form, kept verbatim; negation is exact in IEEE"
    )]
    let right = [
        (-1.0 * sr * sp * cy) + (-1.0 * cr * -sy),
        (-1.0 * sr * sp * sy) + (-1.0 * cr * cy),
        -1.0 * sr * cp,
    ];
    let up = [
        (cr * sp * cy) + (-sr * -sy),
        (cr * sp * sy) + (-sr * cy),
        cr * cp,
    ];
    (forward, right, up)
}

/// `vlen(v)` = `(f32)sqrt(x*x + y*y + z*z)`, the sum in f32 and the root in double (BUILTIN-6.2).
#[must_use]
pub fn vlen(v: [f32; 3]) -> f32 {
    let sum = (v[0] * v[0]) + (v[1] * v[1]) + (v[2] * v[2]);
    double_to_float(f64::from(sum).sqrt())
}

/// `normalize(v)` (BUILTIN-6.3): `'0 0 0'` at length 0, else `(x,y,z) * (f32)(1/len)`.
#[must_use]
pub fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = vlen(v);
    if is_zero_f32(len) {
        return [0.0, 0.0, 0.0];
    }
    let inv = double_to_float(1.0 / f64::from(len));
    [v[0] * inv, v[1] * inv, v[2] * inv]
}

/// `vectoyaw(v)` (BUILTIN-6.4): a whole yaw from 0 to 359.
#[must_use]
pub fn vectoyaw(v: [f32; 3]) -> f32 {
    if is_zero_f32(v[0]) && is_zero_f32(v[1]) {
        return 0.0;
    }
    int_to_float(whole_degrees(f64::from(v[1]), f64::from(v[0])))
}

/// `vectoangles(v)` (BUILTIN-6.5): `'pitch yaw 0'`.
#[must_use]
pub fn vectoangles(v: [f32; 3]) -> [f32; 3] {
    let yaw;
    let pitch;
    if is_zero_f32(v[0]) && is_zero_f32(v[1]) {
        yaw = 0i32;
        pitch = if v[2] > 0.0 { 90 } else { 270 };
    } else {
        yaw = whole_degrees(f64::from(v[1]), f64::from(v[0]));
        let fwd = f64::from((v[0] * v[0]) + (v[1] * v[1])).sqrt();
        pitch = whole_degrees(f64::from(v[2]), fwd);
    }
    [int_to_float(pitch), int_to_float(yaw), 0.0]
}

/// `random()` from a 15-bit source value (BUILTIN-6.6): `(r & 0x7fff) / (f32)0x7fff`.
///
/// The mock supplies the `rand()` value; `r & 0x7fff` keeps the low 15 bits as id does.
#[must_use]
pub fn random(rand_value: u32) -> f32 {
    let masked = rand_value & BUILTIN_RANDOM_MASK;
    int_to_float(u32_to_i32(masked)) / int_to_float(u32_to_i32(BUILTIN_RANDOM_MASK))
}

/// `rint(f)` (BUILTIN-6.7): halves round away from zero; 0 and -0 give +0.
#[must_use]
pub fn rint(f: f32) -> f32 {
    let shifted = if f > 0.0 {
        f64::from(f) + 0.5
    } else {
        f64::from(f) - 0.5
    };
    int_to_float(double_to_int(shifted))
}

/// `floor(f)` (BUILTIN-6.8): C's `floor` in double, rounded to f32, keeping a zero's sign.
#[must_use]
pub fn floor(f: f32) -> f32 {
    double_to_float(f64::from(f).floor())
}

/// `ceil(f)` (BUILTIN-6.8): C's `ceil` in double, rounded to f32, keeping a zero's sign.
#[must_use]
pub fn ceil(f: f32) -> f32 {
    double_to_float(f64::from(f).ceil())
}

/// `fabs(f)` (BUILTIN-6.8): `f` with the sign bit cleared.
#[must_use]
pub fn fabs(f: f32) -> f32 {
    f.abs()
}

/// `anglemod(a)` (BUILTIN-15.5): wrap to 0 up to 360, truncated to a multiple of 360/65536.
#[must_use]
pub fn anglemod(a: f32) -> f32 {
    let scaled = f64::from(a) * (65536.0 / 360.0);
    let whole = double_to_int(scaled);
    let masked = whole & 65535;
    let result = (360.0 / 65536.0) * f64::from(masked);
    double_to_float(result)
}

/// `ftos(f)` (BUILTIN-7.1, BUILTIN-7.6): decimal text, with the fixed NaN/infinity forms.
#[must_use]
pub fn ftos(f: f32) -> String {
    if f.is_finite() {
        let as_int = float_to_int(f);
        #[expect(
            clippy::float_cmp,
            reason = "spec BUILTIN-7.1: the integer form when f == (int)f, an exact IEEE comparison"
        )]
        if f == int_to_float(as_int) {
            return format!("{as_int}");
        }
    }
    component_5_1(f)
}

/// `vtos(v)` (BUILTIN-7.2, BUILTIN-7.7): `'%5.1f %5.1f %5.1f'` with the fixed NaN/infinity forms.
#[must_use]
pub fn vtos(v: [f32; 3]) -> String {
    format!(
        "'{} {} {}'",
        component_5_1(v[0]),
        component_5_1(v[1]),
        component_5_1(v[2]),
    )
}

/// One `vtos` component: `%5.1f`, or the fixed NaN/infinity text (BUILTIN-7.7).
fn component_5_1(f: f32) -> String {
    if f.is_nan() {
        return "  nan".to_string();
    }
    if f.is_infinite() {
        return if f < 0.0 { " -inf" } else { "  inf" }.to_string();
    }
    format_5_1(f)
}

/// C's `printf("%5.1f", f)`: one decimal, right aligned in at least 5 characters (BUILTIN-7.1).
///
/// `f` is finite. Rust's `{:.1}` and C's `printf` both round half to even (`FE_TONEAREST`), so
/// the printed digit agrees for the exact binary value.
fn format_5_1(f: f32) -> String {
    format!("{:>5.1}", f64::from(f))
}

// ----- small helpers that keep the no-panic, no-`as` conventions -----

/// id's `(int)(atan2(y, x) * 180 / M_PI)`, plus 360 when negative: a whole angle from 0 to
/// 359 (BUILTIN-6.4, BUILTIN-6.5).
fn whole_degrees(y: f64, x: f64) -> i32 {
    let degrees = double_to_int(y.atan2(x) * 180.0 / PI);
    if degrees < 0 {
        degrees.wrapping_add(360)
    } else {
        degrees
    }
}

/// Sine and cosine of a double angle, each rounded to f32 (BUILTIN-6.1, BUILTIN-6.9).
fn sin_cos_f32(angle: f64) -> (f32, f32) {
    (double_to_float(angle.sin()), double_to_float(angle.cos()))
}

/// Whether an f32 is +0 or -0 (BUILTIN-6.3, BUILTIN-6.4).
fn is_zero_f32(f: f32) -> bool {
    f == 0.0
}

/// A non-negative `u32` (0 to 0x7fff here) as `i32`, without a cast.
fn u32_to_i32(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(0)
}

/// `Q_atof(s)` (BUILTIN-11.7): id's number parser for a setting's text.
///
/// Implements BUILTIN-11.7, id's `Q_atof` in `WinQuake/common.c`: an optional leading `-`;
/// then a hexadecimal number after `0x` or `0X`, the code of the character after a `'`, or
/// decimal digits with at most one `.`; it stops at the first other character, with no
/// exponent and no leading spaces. The result is id's `(float)(sign * total)`.
#[must_use]
pub fn q_atof(text: &str) -> f32 {
    let bytes = text.as_bytes();
    let mut i = 0;
    // id: skip a leading '-' only; no spaces are skipped (BUILTIN-11.7, `" 5"` is 0).
    let sign = if bytes.first() == Some(&b'-') {
        i = 1;
        -1.0_f64
    } else {
        1.0
    };

    // id: a leading "0x" or "0X" reads a hexadecimal integer.
    if bytes.get(i) == Some(&b'0')
        && matches!(bytes.get(i.saturating_add(1)), Some(&b'x') | Some(&b'X'))
    {
        let mut total: f64 = 0.0;
        let mut j = i.saturating_add(2);
        while let Some(&c) = bytes.get(j) {
            let digit = match c {
                b'0'..=b'9' => i32::from(c.wrapping_sub(b'0')),
                b'a'..=b'f' => i32::from(c.wrapping_sub(b'a')).saturating_add(10),
                b'A'..=b'F' => i32::from(c.wrapping_sub(b'A')).saturating_add(10),
                _ => break,
            };
            total = total * 16.0 + f64::from(digit);
            j = j.saturating_add(1);
        }
        return double_to_float(sign * total);
    }

    // id: a leading '\'' takes the code of the next character.
    if bytes.get(i) == Some(&b'\'') {
        let code = bytes.get(i.saturating_add(1)).copied().unwrap_or(0);
        return double_to_float(sign * f64::from(code));
    }

    // id: decimal digits with at most one '.', stopping at the first other character.
    let mut total: f64 = 0.0;
    let mut decimal: Option<i32> = None;
    let mut j = i;
    while let Some(&c) = bytes.get(j) {
        if c == b'.' {
            if decimal.is_some() {
                break;
            }
            decimal = Some(0);
            j = j.saturating_add(1);
            continue;
        }
        if !c.is_ascii_digit() {
            break;
        }
        total = total * 10.0 + f64::from(c.wrapping_sub(b'0'));
        if let Some(places) = decimal.as_mut() {
            *places = places.saturating_add(1);
        }
        j = j.saturating_add(1);
    }
    if let Some(places) = decimal {
        // id: while (decimal-- > 0) total /= 10.
        for _ in 0..places {
            total /= 10.0;
        }
    }
    double_to_float(sign * total)
}
