// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_rlight.c, R_AnimateLight
// Ported from andrei-drexler/ironwail@08d578136ff43d7d1ef38e636dfbfd3e844be7cd: Quake/gl_rlight.c, R_AnimateLight

//! Light styles (REND-5): the brightness each of the 64 styles has at a time, from the light
//! strings the progs set with `lightstyle`, which scale a face's lightmaps and the light an
//! alias model is shaded by.

/// `MAX_LIGHTSTYLES`: how many styles there are (REND-5.1).
pub const MAX_LIGHTSTYLES: usize = 64;

/// A style's value while no light string is set for it (REND-5.2).
pub(crate) const UNSET_STYLE: i32 = 256;

/// `k*22`: what one letter of a light string is worth (REND-5.3).
const LETTER_STEP: i32 = 22;

/// The light strings' frames each second, `(int)(cl.time*10)` (REND-5.3).
const FRAMES_PER_SECOND: f32 = 10.0;

/// Ironwail's "abrupt" change, which `r_lerplightstyles 1` does not blend: two letters
/// `('m' - 'a') / 2` apart or more, 6 (REND-5.8).
const ABRUPT_STEP: i32 = 6;

/// How a style goes from one letter to the next, Ironwail's `r_lerplightstyles` (REND-5.8):
/// "0=off; 1=skip abrupt transitions; 2=always lerp".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StyleLerp {
    /// `r_lerplightstyles 1`, Ironwail's default: blended toward the next letter, except where
    /// the two are [`ABRUPT_STEP`] apart or more, such as a flicker.
    #[default]
    SkipAbrupt,
    /// `r_lerplightstyles 2`: every change blended.
    Always,
    /// id's `R_AnimateLight`: each letter held for its tenth of a second (REND-5.3).
    Off,
}

/// The value of every light style at one time, `d_lightstylevalue` (REND-5): an 8.8 fraction
/// of a lightmap's stored light, 256 being the light as the map was lit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightStyles([i32; MAX_LIGHTSTYLES]);

impl Default for LightStyles {
    fn default() -> Self {
        Self::unset()
    }
}

impl LightStyles {
    /// Every style at 256, as before any light string is set (REND-5.2).
    #[must_use]
    pub const fn unset() -> Self {
        Self([UNSET_STYLE; MAX_LIGHTSTYLES])
    }

    /// `R_AnimateLight` at `time` seconds, the letters blended as `lerp` says: each style
    /// whose string `strings` gives is, with [`StyleLerp::Off`], its letter for frame
    /// `(int)(time*10)`, `(letter - 'a') * 22`, and otherwise Ironwail's blend toward the next
    /// letter (REND-5.8); a style with no string, or an empty one, is 256 (REND-5.2, REND-5.3).
    #[must_use]
    pub fn at<'s>(strings: impl Fn(usize) -> Option<&'s [u8]>, time: f32, lerp: StyleLerp) -> Self {
        let mut values = [UNSET_STYLE; MAX_LIGHTSTYLES];
        for (style, value) in values.iter_mut().enumerate() {
            let Some(string) = strings(style).filter(|s| !s.is_empty()) else {
                continue;
            };
            *value = match lerp {
                StyleLerp::Off => letter_value(string, truncate(time * FRAMES_PER_SECOND)),
                StyleLerp::SkipAbrupt | StyleLerp::Always => lerped_value(string, time, lerp),
            };
        }
        Self(values)
    }

    /// The value of style `style`; 0 for a style past the 64, which `R_BuildLightMap` never
    /// reads, since 255 ends a face's styles.
    #[must_use]
    pub fn value(&self, style: u8) -> i32 {
        self.0.get(usize::from(style)).copied().unwrap_or(0)
    }

    /// The value as `R_BuildLightMap` multiplies by it, `unsigned scale` (REND-5.4).
    #[must_use]
    pub(crate) fn scale(&self, style: u8) -> u32 {
        self.value(style).cast_unsigned()
    }
}

/// `(map[k] - 'a') * 22` for frame `frame`, `k = frame % length` (REND-5.3).
fn letter_value(string: &[u8], frame: i32) -> i32 {
    letter(string, frame).wrapping_mul(LETTER_STEP)
}

/// `map[k] - 'a'` with `k = frame % length`. C's `%` keeps the sign of `frame`: a negative
/// time picks no letter, which REND-5.3 writes as the first.
fn letter(string: &[u8], frame: i32) -> i32 {
    let length = i32::try_from(string.len()).unwrap_or(i32::MAX);
    let k = frame
        .checked_rem(length)
        .and_then(|k| usize::try_from(k).ok())
        .unwrap_or(0);
    let letter = string.get(k).copied().unwrap_or(b'a');
    i32::from(letter).wrapping_sub(i32::from(b'a'))
}

/// Ironwail's `R_AnimateLight` (REND-5.8): `f = cl.time * 10.0; base = floor(f); i =
/// (int)base; f -= base;`, the letters `k` of frame `i` and `n` of the next, wrapping to the
/// first, `n = k` for an abrupt change unless `lerp` is [`StyleLerp::Always`], and
/// `(int)(k*22 + (n-k)*22*f)`, in double.
fn lerped_value(string: &[u8], time: f32, lerp: StyleLerp) -> i32 {
    let f = f64::from(time) * f64::from(FRAMES_PER_SECOND);
    let base = f.floor();
    let i = truncate_f64(base);
    let f = f - base;
    let k_index = i.checked_rem(i32::try_from(string.len()).unwrap_or(i32::MAX));
    let k = letter(string, i);
    // `n = k + 1; if (n == cl_lightstyle[j].length) n = 0;`
    let n = match k_index.and_then(|k| usize::try_from(k).ok()) {
        Some(at) if at.saturating_add(1) < string.len() => letter(string, i.saturating_add(1)),
        _ => letter(string, 0),
    };
    let n = if lerp != StyleLerp::Always && n.wrapping_sub(k).saturating_abs() >= ABRUPT_STEP {
        k
    } else {
        n
    };
    let step = f64::from(LETTER_STEP);
    truncate_f64(f64::from(k) * step + f64::from(n.wrapping_sub(k)) * step * f)
}

/// C's double to int conversion, toward zero (REND-5.8); a value past `i32` is pinned to the
/// nearer end.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec REND-5.8: Ironwail assigns the double to an int, which truncates toward zero"
)]
fn truncate_f64(value: f64) -> i32 {
    value as i32
}

/// C's float to int conversion, toward zero (REND-5.3); a time past `i32` is pinned to the
/// nearer end.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec REND-5.3: id assigns the float to an int, which truncates toward zero"
)]
fn truncate(value: f32) -> i32 {
    value as i32
}

#[cfg(test)]
mod tests;
