// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2017 Eric Wasylishen
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/ltface.cc, mod_round_to_neg_inf, Mottle

//! The mottle of a minimum light (LIGHT-17.6): a value noise that tiles in world space, so
//! that a face's minimum light is uneven the same way whatever its lightmap's scale.

use crate::{Vec3, to_f32};

/// The noise's side in cells, `mottle_texsize`.
const SIZE: i32 = 256;
/// A cell's side in world units: `world_to_tex` is its reciprocal.
const CELL: f32 = 16.0;
/// The most the mottle adds, just under: `(res / 255.0f) * 48.0f`.
const RANGE: f32 = 48.0;
/// The greatest value of the permutation.
const FULL: f32 = 255.0;

/// `MottlePermutation256`: 0 to 255 shuffled.
const PERMUTATION: [u8; SIZE as usize] = [
    11, 255, 250, 82, 217, 9, 144, 93, 136, 153, 55, 71, 73, 204, 96, 180, 126, 8, 50, 46, 113, 91,
    238, 143, 30, 215, 191, 243, 65, 58, 208, 33, 86, 1, 182, 118, 83, 115, 207, 52, 94, 112, 205,
    48, 99, 254, 117, 101, 157, 140, 72, 242, 244, 154, 10, 135, 155, 168, 125, 183, 148, 116, 187,
    166, 25, 156, 177, 231, 165, 57, 221, 105, 28, 211, 127, 41, 142, 253, 146, 87, 122, 229, 162,
    137, 194, 174, 167, 15, 220, 26, 235, 3, 39, 80, 88, 42, 202, 12, 97, 53, 70, 123, 170, 110,
    214, 192, 173, 84, 169, 188, 64, 102, 147, 158, 100, 69, 213, 193, 43, 20, 13, 237, 171, 103,
    32, 190, 223, 150, 131, 206, 85, 124, 163, 18, 139, 132, 79, 29, 216, 232, 178, 74, 24, 141,
    201, 181, 152, 4, 7, 159, 134, 212, 226, 245, 164, 239, 47, 66, 27, 40, 197, 81, 78, 219, 228,
    241, 121, 23, 120, 230, 76, 252, 199, 184, 45, 203, 161, 89, 16, 21, 119, 5, 209, 196, 68, 130,
    195, 176, 225, 233, 128, 22, 248, 179, 249, 61, 108, 138, 145, 31, 49, 107, 56, 172, 224, 210,
    6, 160, 189, 104, 200, 44, 175, 133, 77, 62, 106, 92, 186, 227, 14, 38, 247, 37, 17, 222, 36,
    75, 129, 185, 251, 240, 54, 151, 2, 98, 149, 0, 63, 218, 60, 198, 19, 59, 90, 246, 234, 67, 51,
    109, 95, 236, 35, 34, 114, 111,
];

/// `mod_round_to_neg_inf(x, mottle_texsize)`: the remainder that is never negative.
const fn wrapped(x: i32) -> i32 {
    x.rem_euclid(SIZE)
}

/// The permutation's entry `at`, `MottlePermutation256[at % 256]`.
fn entry(at: i32) -> i32 {
    usize::try_from(wrapped(at))
        .ok()
        .and_then(|at| PERMUTATION.get(at))
        .copied()
        .map_or(0, i32::from)
}

/// `tex`: the noise at a whole cell, the permutation walked by `x`, then `y`, then `z`.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec LIGHT-17.6: a value of 0 to 255 is a float exactly"
)]
fn cell(x: i32, y: i32, z: i32) -> f32 {
    entry(entry(entry(x).saturating_add(y)).saturating_add(z)) as f32
}

/// `mix`: `a` at 0 and `b` at 1.
fn mix(a: f32, b: f32, by: f32) -> f32 {
    a * (1.0 - by) + b * by
}

/// The floor of a coordinate as a whole number, `static_cast<int>(floor(x))`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec LIGHT-17.6: C converts the floored coordinate to an int"
)]
fn floored(value: f32) -> i32 {
    value.floor() as i32
}

/// `Mottle`: what the mottle adds to a minimum light at `point`, 0 to just under 48: the
/// noise at the eight cells about the point, a cell 16 units, mixed by where the point lies
/// among them.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec LIGHT-17.6: C takes the floored coordinate from the coordinate as floats"
)]
pub(crate) fn mottle(point: Vec3) -> f32 {
    let scaled = point.map(|c| c * to_f32(f64::from(1.0 / CELL)));
    let floor = scaled.map(floored);
    let [fx, fy, fz] = [0, 1, 2].map(|axis: usize| {
        let (at, whole) = (scaled.get(axis), floor.get(axis));
        at.zip(whole).map_or(0.0, |(at, whole)| at - *whole as f32)
    });
    let [x, y, z] = floor.map(wrapped);
    let next = |v: i32| v.saturating_add(1);
    let along_x = |y: i32, z: i32| mix(cell(x, y, z), cell(next(x), y, z), fx);
    let layer = |z: i32| mix(along_x(y, z), along_x(next(y), z), fy);
    let noise = mix(layer(z), layer(next(z)), fz);
    (noise / FULL) * RANGE
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::indexing_slicing,
        reason = "test code: an index past the table is a failed test"
    )]

    use super::*;

    // spec LIGHT-17.6: at a cell's corner the mottle is the permutation's value there over
    // 255, times 48; the noise tiles every 256 cells and is the same below zero.
    #[test]
    fn the_mottle_is_the_noise_at_a_corner_and_tiles() {
        // The origin's cell: the permutation walked three times from 0.
        assert_eq!(PERMUTATION[0], 11);
        assert_eq!(
            cell(0, 0, 0),
            f32::from(PERMUTATION[usize::from(PERMUTATION[11])])
        );
        let at_origin = mottle([0.0; 3]);
        assert_eq!(at_origin, cell(0, 0, 0) / 255.0 * 48.0);
        assert_eq!(mottle([4096.0, 0.0, 0.0]), at_origin);
        assert_eq!(mottle([0.0, -4096.0, 4096.0]), at_origin);
        assert_eq!(mottle([-16.0, 0.0, 0.0]), cell(255, 0, 0) / 255.0 * 48.0);
        assert_eq!(wrapped(-1), 255);
        assert_eq!(wrapped(-256), 0);
        assert_eq!(wrapped(256), 0);
    }

    // spec LIGHT-17.6: between corners it is mixed by where the point lies, on each axis in
    // turn, and stays from 0 to under 48.
    #[test]
    fn the_mottle_is_mixed_between_corners() {
        let (a, b) = (cell(0, 0, 0), cell(1, 0, 0));
        assert_eq!(mottle([8.0, 0.0, 0.0]), (a * 0.5 + b * 0.5) / 255.0 * 48.0);
        let (c, d) = (cell(0, 1, 0), cell(0, 0, 1));
        assert_eq!(
            mottle([0.0, 4.0, 0.0]),
            (a * 0.75 + c * 0.25) / 255.0 * 48.0
        );
        assert_eq!(
            mottle([0.0, 0.0, 12.0]),
            (a * 0.25 + d * 0.75) / 255.0 * 48.0
        );
        for step in 0..400_u16 {
            let at = f32::from(step) * 3.7 - 700.0;
            let value = mottle([at, at * 0.5, -at]);
            assert!((0.0..48.0).contains(&value), "{value} at {at}");
        }
    }
}
