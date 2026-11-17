// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/write.cc, LightFace_ScaleAndClamp, Lightsurf_HasColor, Lightmap_AvgBrightness, Lightmap_MaxBrightness, CalculateLightmapStyles, FloodFillTransparent, IntegerDownsampleImage, WriteSingleLightmap

//! A face's lightmaps as bytes (LIGHT-18): each sample scaled and clamped, the styles kept
//! and their order, hidden samples filled from those beside them, the samples of a luxel
//! averaged, and each luxel's colour and grey byte.

use qs_bsp::BSP_MAXLIGHTMAPS;

use crate::extended::keys::World;
use crate::extended::math::{add, scale};
use crate::extended::points::Patch;
use crate::extended::shade::{Lightmap, Lightmaps, scaled_and_clamped};
use crate::face::Lit;
use crate::{AXES, Rgb, Sampling, Vec3, to_f32};

/// `LIGHT_EQUAL_EPSILON`: how far apart two channels are for a sample to have a colour
/// (LIGHT-18.6).
const EQUAL_EPSILON: f32 = 0.001;
/// A lightmap no sample of which is this bright is not kept (LIGHT-18.2).
const DIMMEST_KEPT: f32 = 1.0;
/// The highest style a face's lightmap may have.
const MAX_STYLE: i32 = 254;
/// The brightest a channel is written.
const BRIGHTEST: f32 = 255.0;
/// The channels of a colour, as a divisor of their sum.
const CHANNELS: f64 = 3.0;

/// A sample's colour and whether it was seen: a hidden one takes no part in an average
/// until it is filled.
#[derive(Clone, Copy)]
struct Texel {
    colour: Vec3,
    seen: bool,
}

/// `LightSample_Brightness`.
fn brightness(colour: Vec3) -> f32 {
    to_f32(f64::from(colour[0] + colour[1] + colour[2]) / CHANNELS)
}

/// A count as a float.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec LIGHT-18.2: a count of samples is far within a float's whole numbers"
)]
const fn count_of(value: usize) -> f32 {
    value as f32
}

/// A channel as the byte C's conversion makes of it: cut to a whole number.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "spec LIGHT-18.5: ericw-tools stores each channel in a byte, truncating"
)]
const fn byte_of(value: f32) -> u8 {
    value as u8
}

/// `FloodFillTransparent`: each hidden texel given the mean of the seen ones about it, row by
/// row, a filled one counting as seen at once, until none is hidden or all are (LIGHT-18.3).
fn flood_fill(texels: &mut [Texel], width: usize, height: usize) {
    loop {
        let mut hidden = 0_usize;
        for y in 0..height {
            for x in 0..width {
                let at = y.saturating_mul(width).saturating_add(x);
                if texels.get(at).is_none_or(|texel| texel.seen) {
                    continue;
                }
                let mut sum = [0.0; AXES];
                let mut seen = 0_usize;
                for ny in y.saturating_sub(1)..=y.saturating_add(1).min(height.saturating_sub(1)) {
                    for nx in x.saturating_sub(1)..=x.saturating_add(1).min(width.saturating_sub(1))
                    {
                        let beside = texels.get(ny.saturating_mul(width).saturating_add(nx));
                        if let Some(beside) = beside.filter(|texel| texel.seen) {
                            sum = add(sum, beside.colour);
                            seen = seen.saturating_add(1);
                        }
                    }
                }
                if seen == 0 {
                    hidden = hidden.saturating_add(1);
                } else if let Some(texel) = texels.get_mut(at) {
                    *texel = Texel {
                        colour: scale(sum, 1.0 / count_of(seen)),
                        seen: true,
                    };
                }
            }
        }
        if hidden == 0 || hidden == texels.len() {
            return;
        }
    }
}

/// `IntegerDownsampleImage`: each luxel the mean of its `factor` by `factor` samples that
/// were seen, or of them all when none was (LIGHT-18.4).
fn downsample(texels: &[Texel], width: usize, height: usize, factor: usize) -> Vec<Vec3> {
    if factor <= 1 {
        return texels.iter().map(|texel| texel.colour).collect();
    }
    let (out_width, out_height) = (
        width.checked_div(factor).unwrap_or(0),
        height.checked_div(factor).unwrap_or(0),
    );
    let mut out = Vec::with_capacity(out_width.saturating_mul(out_height));
    for y in 0..out_height {
        for x in 0..out_width {
            let (mut seen_sum, mut seen) = ([0.0; AXES], 0_usize);
            let (mut all_sum, mut all) = ([0.0; AXES], 0_usize);
            for dy in 0..factor {
                for dx in 0..factor {
                    let (sx, sy) = (
                        x.saturating_mul(factor).saturating_add(dx),
                        y.saturating_mul(factor).saturating_add(dy),
                    );
                    if sx >= width || sy >= height {
                        continue;
                    }
                    let Some(texel) = texels.get(sy.saturating_mul(width).saturating_add(sx))
                    else {
                        continue;
                    };
                    all_sum = add(all_sum, texel.colour);
                    all = all.saturating_add(1);
                    if texel.seen {
                        seen_sum = add(seen_sum, texel.colour);
                        seen = seen.saturating_add(1);
                    }
                }
            }
            out.push(if seen > 0 {
                scale(seen_sum, 1.0 / count_of(seen))
            } else {
                scale(all_sum, 1.0 / count_of(all))
            });
        }
    }
    out
}

/// A lit face: its lightmaps as bytes, and whether any of its samples has a colour.
pub(crate) struct Finished {
    /// The lightmaps; none when no style is kept.
    pub(crate) lit: Option<Lit>,
    pub(crate) coloured: bool,
}

/// `FinishLightmapSurface`, `CalculateLightmapStyles` and `WriteSingleLightmap`: the
/// lightmaps of `patch` as bytes (LIGHT-18).
pub(crate) fn finish(
    world: &World,
    patch: &Patch,
    mut lightmaps: Lightmaps,
    sampling: Sampling,
) -> Finished {
    let mut coloured = false;
    for lightmap in &mut lightmaps.all {
        for colour in &mut lightmap.samples {
            *colour = scaled_and_clamped(world, *colour, patch.maxlight, patch.lightcolorscale);
            coloured |= (colour[0] - colour[1]).abs() > EQUAL_EPSILON
                || (colour[0] - colour[2]).abs() > EQUAL_EPSILON;
        }
    }
    // The styles kept: those a light reached, with a sample of brightness 1 or more, the
    // brightest on average first, the later of two as bright before the earlier.
    let mut kept: Vec<(f32, usize, i32, &Lightmap)> = Vec::new();
    for (index, lightmap) in lightmaps.all.iter().enumerate() {
        let Some(style) = lightmap.style.filter(|style| *style <= MAX_STYLE) else {
            continue;
        };
        let mut most = 0.0_f32;
        let mut total = 0.0_f32;
        for &colour in &lightmap.samples {
            let bright = brightness(colour);
            most = most.max(bright);
            total += bright;
        }
        if most < DIMMEST_KEPT {
            continue;
        }
        kept.push((
            total / count_of(lightmap.samples.len()),
            index,
            style,
            lightmap,
        ));
    }
    kept.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    kept.truncate(BSP_MAXLIGHTMAPS);
    if kept.is_empty() {
        return Finished {
            lit: None,
            coloured,
        };
    }
    let factor = match sampling {
        Sampling::Fast => 1,
        Sampling::Extra => 2,
    };
    let mut lit = Lit {
        styles: Vec::with_capacity(kept.len()),
        grey: Vec::new(),
        rgb: Vec::new(),
    };
    for (_, _, style, lightmap) in kept {
        lit.styles.push(u8::try_from(style).unwrap_or(u8::MAX));
        let mut texels: Vec<Texel> = lightmap
            .samples
            .iter()
            .zip(&patch.samples)
            .map(|(&colour, sample)| Texel {
                colour,
                seen: !sample.occluded,
            })
            .collect();
        flood_fill(&mut texels, patch.width, patch.height);
        for colour in downsample(&texels, patch.width, patch.height, factor) {
            let (rgb, grey) = bytes_of(colour);
            lit.rgb.push(rgb);
            lit.grey.push(grey);
        }
    }
    Finished {
        lit: Some(lit),
        coloured,
    }
}

/// A luxel's colour as bytes, scaled down when a channel is past 255, and its grey byte, the
/// brightest channel (LIGHT-18.5).
fn bytes_of(colour: Vec3) -> (Rgb, u8) {
    let most = |c: Vec3| c[0].max(c[1]).max(c[2]);
    let mut colour = colour;
    let brightest = most(colour);
    if brightest > BRIGHTEST {
        colour = scale(colour, BRIGHTEST / brightest);
    }
    (
        colour.map(byte_of),
        byte_of(most(colour).clamp(0.0, BRIGHTEST)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn texel(value: f32, seen: bool) -> Texel {
        Texel {
            colour: [value; 3],
            seen,
        }
    }

    // spec LIGHT-18.3: a hidden sample takes the mean of the seen samples about it, and once
    // filled counts as seen for those after it.
    #[test]
    fn hidden_samples_are_filled_from_those_beside_them() {
        let mut row = [
            texel(90.0, true),
            texel(0.0, false),
            texel(0.0, false),
            texel(30.0, true),
        ];
        flood_fill(&mut row, 4, 1);
        let values: Vec<f32> = row.iter().map(|t| t.colour[0]).collect();
        // The second is the first's 90; the third is the mean of that 90 and the last's 30.
        assert_eq!(values, [90.0, 90.0, 60.0, 30.0]);
        assert!(row.iter().all(|t| t.seen));
        let mut none = [texel(5.0, false), texel(7.0, false)];
        flood_fill(&mut none, 2, 1);
        assert!(none.iter().all(|t| !t.seen), "nothing seen fills nothing");
        let mut far = [
            texel(0.0, false),
            texel(0.0, false),
            texel(0.0, false),
            texel(8.0, true),
        ];
        flood_fill(&mut far, 4, 1);
        assert!(far.iter().all(|t| t.seen && t.colour[0] == 8.0));
    }

    // spec LIGHT-18.4: a luxel is the mean of its samples that were seen, or of all of them
    // when none was.
    #[test]
    fn a_luxel_is_the_mean_of_its_seen_samples() {
        let texels = [
            texel(10.0, true),
            texel(20.0, true),
            texel(40.0, false),
            texel(40.0, false),
            texel(30.0, true),
            texel(100.0, false),
            texel(80.0, false),
            texel(40.0, false),
        ];
        let out = downsample(&texels, 4, 2, 2);
        assert_eq!(out, [[20.0; 3], [50.0; 3]]);
        assert_eq!(downsample(&texels, 4, 2, 1).len(), 8);
    }

    // spec LIGHT-18.5: a channel past 255 scales the colour down to it; each channel is cut
    // to a byte, and the grey byte is the brightest.
    #[test]
    fn a_luxels_bytes_are_cut_and_the_grey_is_the_brightest() {
        assert_eq!(bytes_of([10.9, 20.2, 5.0]), ([10, 20, 5], 20));
        assert_eq!(bytes_of([510.0, 255.0, 0.0]), ([255, 127, 0], 255));
        assert_eq!(bytes_of([0.0; 3]), ([0; 3], 0));
    }
}
