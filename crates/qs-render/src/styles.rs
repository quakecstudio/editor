// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_rlight.c, R_AnimateLight

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

    /// `R_AnimateLight` at `time` seconds: each style whose string `strings` gives is its
    /// letter for frame `(int)(time*10)`, `(letter - 'a') * 22`; a style with no string, or an
    /// empty one, is 256 (REND-5.2, REND-5.3).
    #[must_use]
    pub fn at<'s>(strings: impl Fn(usize) -> Option<&'s [u8]>, time: f32) -> Self {
        let frame = truncate(time * FRAMES_PER_SECOND);
        let mut values = [UNSET_STYLE; MAX_LIGHTSTYLES];
        for (style, value) in values.iter_mut().enumerate() {
            let Some(string) = strings(style).filter(|s| !s.is_empty()) else {
                continue;
            };
            let length = i32::try_from(string.len()).unwrap_or(i32::MAX);
            // C's `%` keeps the sign of `i`: a negative time picks no letter, which REND-5.3
            // writes as the first.
            let k = frame
                .checked_rem(length)
                .and_then(|k| usize::try_from(k).ok())
                .unwrap_or(0);
            let letter = string.get(k).copied().unwrap_or(b'a');
            *value = i32::from(letter)
                .wrapping_sub(i32::from(b'a'))
                .wrapping_mul(LETTER_STEP);
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
