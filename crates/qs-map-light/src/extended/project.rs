// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2016 Eric Wasylishen
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/entities.cc, Matrix4x4_CM_Projection_Inf, Matrix4x4_CM_NewRotation, Matrix4x4_CM_NewTranslation, Matrix4_Multiply, Matrix4x4_CM_ModelViewMatrix, Matrix4x4_CM_MakeModelViewProj, CalcFov
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/ltface.cc, Matrix4x4_CM_Transform4, Matrix4x4_CM_Project, LightFace_SampleMipTex

//! A light that projects a texture (LIGHT-10.11): the light is a projector at its origin,
//! looking along `_project_mangle` with a field of view of `_project_fov`, and a sample it
//! lights takes the colour of the texture where the sample falls in its picture.

use super::scene::Map;
use crate::{AXES, Vec3, to_f32};

/// A 4 by 4 matrix, column by column.
type Matrix = [f32; CELLS];
/// A matrix's cells and its side.
const CELLS: usize = 16;
const SIDE: usize = 4;
/// The near plane of the projector, `Matrix4x4_CM_Projection_Inf(proj, fovx, fovy, 4.0f)`.
const NEAR: f32 = 4.0;
/// The least and the greatest field of view `CalcFov` takes.
const FOV_MIN: f32 = 1.0;
const FOV_MAX: f32 = 179.0;
/// Degrees in a whole and in a half turn.
const WHOLE_TURN: f64 = 360.0;
const HALF_TURN: f64 = 180.0;
/// `proj[10] = -1 * 0.5`: the depth's scale with no far plane.
const DEPTH: f32 = -0.5;
/// A projected texel is counted twice, `result *= 2`.
const GAIN: f32 = 2.0;

/// A light's projector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Projection {
    /// World to the projector's picture, `projectionmatrix`.
    matrix: Matrix,
    /// The texture projected, by its number.
    texture: usize,
    /// Its width and height.
    size: (u32, u32),
}

/// A count as a float, far within a float's whole numbers.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec LIGHT-10.11: a texture's side is far within a float's whole numbers"
)]
const fn float_of(value: u32) -> f32 {
    value as f32
}

/// The whole part of a texel coordinate, `const int sbase = sfrac`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "spec LIGHT-10.11: C cuts the coordinate, from 0 to the texture's side, to an int"
)]
const fn whole(value: f32) -> u32 {
    value as u32
}

/// `Matrix4_Multiply`: `a` times `b`.
fn multiplied(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = [0.0; CELLS];
    for (at, slot) in out.iter_mut().enumerate() {
        let (column, row) = (
            at.checked_div(SIDE).unwrap_or(0),
            at.checked_rem(SIDE).unwrap_or(0),
        );
        let mut sum = 0.0_f32;
        for k in 0..SIDE {
            let left = a.get(k.saturating_mul(SIDE).saturating_add(row));
            let right = b.get(column.saturating_mul(SIDE).saturating_add(k));
            if let (Some(left), Some(right)) = (left, right) {
                sum += left * right;
            }
        }
        *slot = sum;
    }
    out
}

/// `Matrix4x4_CM_NewRotation`: a turn of `degrees` about the axis `x`, `y`, `z`.
fn rotation(degrees: f32, [x, y, z]: Vec3) -> Matrix {
    let radians = f64::from(degrees) * core::f64::consts::PI / HALF_TURN;
    let (c, s) = (to_f32(radians.cos()), to_f32(radians.sin()));
    [
        x * x * (1.0 - c) + c,
        y * x * (1.0 - c) + z * s,
        x * z * (1.0 - c) - y * s,
        0.0,
        x * y * (1.0 - c) - z * s,
        y * y * (1.0 - c) + c,
        y * z * (1.0 - c) + x * s,
        0.0,
        x * z * (1.0 - c) + y * s,
        y * z * (1.0 - c) - x * s,
        z * z * (1.0 - c) + c,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

/// `Matrix4x4_CM_Projection_Inf`: a projection with no far plane, `fovx` by `fovy` degrees.
#[expect(
    clippy::float_cmp,
    reason = "spec LIGHT-10.11: `if (fovx == fovy)`, the two fields of view as given"
)]
fn projection(fovx: f32, fovy: f32) -> Matrix {
    let half_tan = |fov: f32| {
        to_f32(f64::from(NEAR) * (f64::from(fov) * core::f64::consts::PI / WHOLE_TURN).tan())
    };
    let ymax = half_tan(fovy);
    let ymin = -ymax;
    let (xmin, xmax) = if fovx == fovy {
        (ymin, ymax)
    } else {
        let xmax = half_tan(fovx);
        (-xmax, xmax)
    };
    // Column by column.
    [
        (2.0 * NEAR) / (xmax - xmin),
        0.0,
        0.0,
        0.0,
        0.0,
        (2.0 * NEAR) / (ymax - ymin),
        0.0,
        0.0,
        (xmax + xmin) / (xmax - xmin),
        (ymax + ymin) / (ymax - ymin),
        DEPTH,
        -1.0,
        0.0,
        0.0,
        -2.0 * NEAR,
        0.0,
    ]
}

/// `CalcFov`: the field of view across `height` that goes with `fov` across `width`.
fn other_fov(fov: f32, width: f32, height: f32) -> f32 {
    let half = to_f32(f64::from(fov / to_f32(WHOLE_TURN)) * core::f64::consts::PI);
    let reach = width / half.tan();
    let angle = (height / reach).atan();
    to_f32(f64::from(angle * to_f32(WHOLE_TURN)) / core::f64::consts::PI)
}

impl Projection {
    /// `Matrix4x4_CM_MakeModelViewProj` as `LoadEntities` calls it: the projector at `origin`
    /// turned by `angles`, yaw, pitch and roll, showing texture `texture` of `size`; `fov` is
    /// across its longer side. None for a `fov` outside 1 to 179, which `CalcFov` refuses.
    pub(crate) fn new(
        angles: Vec3,
        origin: Vec3,
        fov: f32,
        (texture, size): (usize, (u32, u32)),
    ) -> Option<Self> {
        if !(FOV_MIN..=FOV_MAX).contains(&fov) {
            return None;
        }
        let (width, height) = (float_of(size.0), float_of(size.1));
        let (fovx, fovy) = if width > height {
            (fov, other_fov(fov, width, height))
        } else {
            (other_fov(fov, height, width), fov)
        };
        // `Matrix4x4_CM_ModelViewMatrix`: Quake's axes to the picture's, then roll, pitch and
        // yaw, then the origin taken away.
        let view: Matrix = [
            0.0, 0.0, -1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let [yaw, pitch, roll] = angles;
        let view = multiplied(&view, &rotation(-roll, [1.0, 0.0, 0.0]));
        let view = multiplied(&view, &rotation(pitch, [0.0, 1.0, 0.0]));
        let view = multiplied(&view, &rotation(-yaw, [0.0, 0.0, 1.0]));
        let [x, y, z] = origin;
        let moved: Matrix = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -x, -y, -z, 1.0,
        ];
        let view = multiplied(&view, &moved);
        Some(Self {
            matrix: multiplied(&projection(fovx, fovy), &view),
            texture,
            size,
        })
    }

    /// `Matrix4x4_CM_Project`: where `point` falls in the picture, each way from 0 to 1;
    /// none for a point behind the projector or outside its picture.
    fn picture_at(&self, point: Vec3) -> Option<(f32, f32)> {
        // `Matrix4x4_CM_Transform4` of the point and 1: each column by its part.
        let [px, py, pz] = point;
        let mut out = [0.0_f32; SIDE];
        for (column, part) in self
            .matrix
            .as_chunks::<SIDE>()
            .0
            .iter()
            .zip([px, py, pz, 1.0])
        {
            for (slot, cell) in out.iter_mut().zip(column) {
                *slot += cell * part;
            }
        }
        let [x, y, z, w] = out;
        if z < 0.0 {
            return None;
        }
        let (s, t, depth) = (
            (1.0 + x / w) / 2.0,
            (1.0 + y / w) / 2.0,
            (1.0 + z / w) / 2.0,
        );
        if depth > 1.0 || s <= 0.0 || s >= 1.0 || t <= 0.0 || t >= 1.0 {
            return None;
        }
        Some((s, t))
    }
}

impl Map<'_> {
    /// `LightFace_SampleMipTex`: the colour `projection` gives `point`, the four texels
    /// about where it falls mixed by how near each is, counted twice; none where the
    /// projector does not reach.
    pub(crate) fn projected(&self, projection: &Projection, point: Vec3) -> Option<Vec3> {
        let (s, t) = projection.picture_at(point)?;
        let (width, height) = projection.size;
        let across = s * float_of(width.saturating_sub(1));
        let left = whole(across);
        let s_part = across - float_of(left);
        let down = (1.0 - t) * float_of(height.saturating_sub(1));
        let top = whole(down);
        let t_part = down - float_of(top);
        let texel = |right: u32, below: u32| {
            let x = left.saturating_add(right).checked_rem(width).unwrap_or(0);
            let y = top.saturating_add(below).checked_rem(height).unwrap_or(0);
            self.texel_colour(projection.texture, x, y)
        };
        let mut out = [0.0; AXES];
        for (colour, weight) in [
            (texel(0, 0), (1.0 - s_part) * (1.0 - t_part)),
            (texel(1, 0), s_part * (1.0 - t_part)),
            (texel(0, 1), (1.0 - s_part) * t_part),
            (texel(1, 1), s_part * t_part),
        ] {
            for (slot, channel) in out.iter_mut().zip(colour) {
                *slot += f32::from(channel) * weight;
            }
        }
        Some(out.map(|c| c * GAIN))
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::*;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    // spec LIGHT-10.11: the field of view the other way keeps the texture's shape: the same
    // for a square, and for a texture twice as wide the angle whose tangent is half.
    #[test]
    fn the_other_field_of_view_keeps_the_textures_shape() {
        assert!(near(other_fov(90.0, 64.0, 64.0), 90.0));
        let narrow = other_fov(90.0, 128.0, 64.0);
        assert!(near(narrow, 2.0 * 0.5_f32.atan().to_degrees()));
        assert_eq!(
            Projection::new([0.0; 3], [0.0; 3], 0.5, (0, (64, 64))),
            None
        );
        assert_eq!(
            Projection::new([0.0; 3], [0.0; 3], 180.0, (0, (64, 64))),
            None
        );
        assert!(Projection::new([0.0; 3], [0.0; 3], 179.0, (0, (64, 64))).is_some());
    }

    // spec LIGHT-10.11: a projector looking along x from the origin sees a point ahead in
    // the middle of its picture, y to its left and z up; nothing behind it or outside its
    // field of view.
    #[test]
    fn a_projector_sees_what_is_in_its_picture() {
        let looking = Projection::new([0.0; 3], [0.0; 3], 90.0, (0, (64, 64))).unwrap();
        let (s, t) = looking.picture_at([100.0, 0.0, 0.0]).unwrap();
        assert!(near(s, 0.5) && near(t, 0.5), "{s} {t}");
        // 90 degrees across: a point as far to the side as ahead is at the picture's edge.
        let (s, t) = looking.picture_at([100.0, 50.0, 25.0]).unwrap();
        assert!(near(s, 0.25) && near(t, 0.625), "{s} {t}");
        assert_eq!(looking.picture_at([-100.0, 0.0, 0.0]), None);
        assert_eq!(looking.picture_at([100.0, 101.0, 0.0]), None);
        assert_eq!(looking.picture_at([100.0, 0.0, -101.0]), None);
        // Turned a quarter turn of yaw, it looks along y; moved, it sees from where it is.
        let turned =
            Projection::new([90.0, 0.0, 0.0], [10.0, 0.0, 0.0], 90.0, (0, (64, 64))).unwrap();
        let (s, t) = turned.picture_at([10.0, 100.0, 0.0]).unwrap();
        assert!(near(s, 0.5) && near(t, 0.5), "{s} {t}");
        assert_eq!(turned.picture_at([100.0, 0.0, 0.0]), None);
        // Pitched up 90, it looks along z.
        let up = Projection::new([0.0, 90.0, 0.0], [0.0; 3], 90.0, (0, (64, 64))).unwrap();
        assert!(up.picture_at([0.0, 0.0, 100.0]).is_some());
        assert_eq!(up.picture_at([100.0, 0.0, 0.0]), None);
    }
}
