// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-2001 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2007-2008 Kristian Duske
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/gl_screen.c, AdaptFovx

//! Which engine's look a frame takes (REND-13): QuakeSpasm's by default, GLQuake's as a mode a
//! caller chooses, as ADR-0012 keeps the loaders' modes.

use crate::mesh::GAMMA;

/// The look of a frame: how the palette is corrected, how textures are filtered, whether
/// fullbright texels glow, how bright light can be and how wide the view is (REND-13).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Look {
    /// QuakeSpasm's defaults: `gamma 1`, trilinear filtering, `gl_fullbrights 1`,
    /// `gl_overbright 1` and `fov_adapt 1` (REND-13.1).
    #[default]
    QuakeSpasm,
    /// GLQuake's, as REND-1 to REND-5 describe it (REND-13.1).
    GlQuake,
}

impl Look {
    /// The palette's gamma when the caller gives none: QuakeSpasm's `gamma` "1" leaves the
    /// palette as it is (REND-13.2), GLQuake's 0.7 brightens it (REND-3.1).
    #[must_use]
    pub const fn gamma(self) -> f32 {
        match self {
            Self::QuakeSpasm => 1.0,
            Self::GlQuake => GAMMA,
        }
    }

    /// How far `R_BuildLightMap` shifts a texel's sum: 8 with `gl_overbright`, 7 in GLQuake
    /// (REND-13.5, REND-5.4).
    pub(crate) const fn light_shift(self) -> u32 {
        match self {
            Self::QuakeSpasm => 8,
            Self::GlQuake => 7,
        }
    }

    /// What the world's shader multiplies a lightmap texel by: `result.rgb *= 2.0` with
    /// `gl_overbright` (REND-13.5), 1 in GLQuake.
    pub(crate) const fn light_scale(self) -> f32 {
        match self {
            Self::QuakeSpasm => 2.0,
            Self::GlQuake => 1.0,
        }
    }

    /// Whether a world texture's or skin's fullbright texels, indices 224 to 255, glow at full
    /// brightness (REND-13.4).
    pub(crate) const fn fullbrights(self) -> bool {
        match self {
            Self::QuakeSpasm => true,
            Self::GlQuake => false,
        }
    }

    /// Whether mipmapped textures blend between their two nearest levels: QuakeSpasm's
    /// `GL_LINEAR_MIPMAP_LINEAR` (REND-13.3), not GLQuake's `GL_LINEAR_MIPMAP_NEAREST`
    /// (REND-3.3).
    pub(crate) const fn trilinear(self) -> bool {
        match self {
            Self::QuakeSpasm => true,
            Self::GlQuake => false,
        }
    }

    /// `Draw_FadeScreen`'s black over the view behind a menu: QuakeSpasm's "`glColor4f (0, 0,
    /// 0, 0.5)`", GLQuake's 0.8 (REND-13.7).
    #[must_use]
    pub const fn fade_alpha(self) -> f32 {
        match self {
            Self::QuakeSpasm => 0.5,
            Self::GlQuake => 0.8,
        }
    }

    /// The console background's alpha for a console `lines` high on a screen `height` high,
    /// `forced` up or not: QuakeSpasm's "`alpha = (con_forcedup) ? 1.0f : scr_conalpha.value`",
    /// `scr_conalpha` "0.5"; GLQuake's opaque past three quarters of the screen, else
    /// `1.2 * lines` over that (CON-3.2, REND-13.7).
    #[must_use]
    pub fn console_alpha(self, lines: i32, height: i32, forced: bool) -> f32 {
        match self {
            Self::QuakeSpasm if forced => 1.0,
            Self::QuakeSpasm => 0.5,
            Self::GlQuake => {
                let three_quarters = height.saturating_mul(3) >> 2;
                if lines > three_quarters {
                    1.0
                } else {
                    (1.2 * f32_of(lines) / f32_of(three_quarters.max(1))).min(1.0)
                }
            }
        }
    }

    /// The gamma the whole frame is raised to for the `gamma` setting `gamma` (REND-13.9):
    /// QuakeSpasm's `GLSLGamma_GammaCorrect` applies it, GLQuake's palette ramps reach no
    /// screen, so 1 there.
    #[must_use]
    pub const fn frame_gamma(self, gamma: f32) -> f32 {
        match self {
            Self::QuakeSpasm => gamma,
            Self::GlQuake => 1.0,
        }
    }

    /// The gamma the client's blend table is built with for `gamma` (REND-13.9, VIEW-4.7):
    /// GLQuake's `V_UpdatePalette` runs the blend through it; QuakeSpasm's `R_PolyBlend` draws
    /// the blend as it is and corrects the whole frame instead, so 1 there.
    #[must_use]
    pub const fn blend_gamma(self, gamma: f32) -> f32 {
        match self {
            Self::QuakeSpasm => 1.0,
            Self::GlQuake => gamma,
        }
    }

    /// Whether the 2D layer is the window's own size, QuakeSpasm's canvases at scale 1, rather
    /// than GLQuake's 640 wide layer (REND-13.7).
    #[must_use]
    pub const fn native_2d(self) -> bool {
        match self {
            Self::QuakeSpasm => true,
            Self::GlQuake => false,
        }
    }

    /// The field of view across for `fov_x` on an image `aspect` wide per unit of height:
    /// QuakeSpasm's `AdaptFovx`, which keeps the height a 4:3 image of `fov_x` would have
    /// (REND-13.6); GLQuake's is `fov_x` (REND-3.4).
    #[must_use]
    pub fn fov_x(self, fov_x: f32, aspect: f32) -> f32 {
        match self {
            Self::GlQuake => fov_x,
            Self::QuakeSpasm => adapt_fov_x(fov_x, aspect),
        }
    }
}

/// `AdaptFovx (fov_x, width, height)` with `aspect` for `width / height`: "if ((x = height /
/// width) == 0.75) return fov_x; a = atan(0.75 / x * tan(fov_x / 360 * M_PI)); a = a * 360 /
/// M_PI;", the double results stored in C's float `a` (REND-13.6). An aspect that is not a
/// positive number keeps `fov_x`.
fn adapt_fov_x(fov_x: f32, aspect: f32) -> f32 {
    let x = 1.0 / aspect;
    #[expect(
        clippy::float_cmp,
        reason = "spec REND-13.6: AdaptFovx compares height / width with 0.75 exactly"
    )]
    let four_by_three = x == 0.75;
    if four_by_three || !x.is_finite() || x <= 0.0 {
        return fov_x;
    }
    let a = (0.75 / f64::from(x) * (f64::from(fov_x) / 360.0 * core::f64::consts::PI).tan()).atan();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec REND-13.6: the double stored in AdaptFovx's float a"
    )]
    let a = (a * 360.0 / core::f64::consts::PI) as f32;
    a
}

/// A pixel count as a float, exactly for any screen.
fn f32_of(n: i32) -> f32 {
    f32::from(i16::try_from(n).unwrap_or(i16::MAX))
}

#[cfg(test)]
mod tests;
