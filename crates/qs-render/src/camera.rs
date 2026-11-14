// SPDX-License-Identifier: GPL-3.0-or-later

//! A free camera in Quake's axes: x forward at yaw 0, y left, z up; yaw turns about z and
//! pitch looks down when positive, as Quake's `angles` do.

use crate::sprite::angle_vectors;
use crate::vector::{cross, dot};

/// An angle in degrees, the editor camera's: it has no C declaration to follow.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct Degrees(f32);

impl Degrees {
    /// `degrees` degrees.
    #[must_use]
    pub const fn new(degrees: f32) -> Self {
        Self(degrees)
    }

    /// The degrees.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }

    /// The angle in radians.
    #[must_use]
    pub fn to_radians(self) -> f32 {
        self.0.to_radians()
    }
}

/// Where the camera is and where it looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Its position in map units.
    pub origin: [f32; 3],
    /// Its yaw, about z, 0 looking down +x.
    pub yaw: Degrees,
    /// Its pitch, positive looking down, kept within ±89.
    pub pitch: Degrees,
    /// Its field of view across; the height's follows from the image's shape (REND-3.4).
    pub fov_x: Degrees,
}

/// The nearest and farthest distances drawn, in map units: `MYgluPerspective (r_refdef.fov_y,
/// screenaspect, 4, 4096)` in GLQuake's `R_SetupGL` (REND-3.4).
const NEAR: f32 = 4.0;
const FAR: f32 = 4096.0;
/// `fov`, the field of view across, in degrees: `scr_fov = {"fov","90"}` (REND-3.4).
const FOV_X: f32 = 90.0;
/// A turn, which the yaw wraps at, and the pitch's furthest up or down.
const FULL_TURN: f32 = 360.0;
const PITCH_LIMIT: f32 = 89.0;

impl Camera {
    /// A camera at `origin` looking along `yaw`, level, with GLQuake's `fov 90` (REND-3.4).
    #[must_use]
    pub const fn new(origin: [f32; 3], yaw: f32) -> Self {
        Self {
            origin,
            yaw: Degrees::new(yaw),
            pitch: Degrees::new(0.0),
            fov_x: Degrees::new(FOV_X),
        }
    }

    /// The unit vectors it looks along: forward, right and up.
    #[must_use]
    pub fn axes(&self) -> [[f32; 3]; 3] {
        let (sy, cy) = self.yaw.to_radians().sin_cos();
        let (sp, cp) = self.pitch.to_radians().sin_cos();
        let forward = [cp * cy, cp * sy, -sp];
        let right = [sy, -cy, 0.0];
        let up = cross(right, forward);
        [forward, right, up]
    }

    /// Moves it back from the middle of the box `[min, max]` along the way it faces, far
    /// enough that the box's bounding sphere fits inside the narrower of its half fields of view
    /// in an image `aspect` times as wide as it is high, plus `margin` map units, keeping where
    /// it looks.
    pub fn frame(&mut self, [min, max]: [[f32; 3]; 2], aspect: f32, margin: f32) {
        let [x0, y0, z0] = min;
        let [x1, y1, z1] = max;
        let middle = [(x0 + x1) / 2.0, (y0 + y1) / 2.0, (z0 + z1) / 2.0];
        let half = [(x1 - x0) / 2.0, (y1 - y0) / 2.0, (z1 - z0) / 2.0];
        let radius = half.iter().map(|h| h * h).sum::<f32>().sqrt();
        let across = (self.fov_x.to_radians() / 2.0).tan();
        let down = across / aspect.max(f32::EPSILON);
        let narrower = across.min(down).max(f32::EPSILON);
        // A sphere fits a cone of half-angle `a` at `r / sin(a)`, at least `r / tan(a)`.
        let distance = radius * (1.0 + 1.0 / (narrower * narrower)).sqrt() + margin;
        let [[fx, fy, fz], _, _] = self.axes();
        let [mx, my, mz] = middle;
        self.origin = [mx - fx * distance, my - fy * distance, mz - fz * distance];
    }

    /// Moves it by `forward`, `right` and `up` map units along its axes.
    pub fn travel(&mut self, forward: f32, right: f32, up: f32) {
        let [f, r, _] = self.axes();
        for ((o, f), r) in self.origin.iter_mut().zip(f).zip(r) {
            *o += f * forward + r * right;
        }
        self.origin[2] += up;
    }

    /// Turns it by `yaw` and `pitch` degrees, the pitch kept within ±89.
    pub fn turn(&mut self, yaw: f32, pitch: f32) {
        self.yaw = Degrees::new((self.yaw.get() + yaw).rem_euclid(FULL_TURN));
        self.pitch = Degrees::new((self.pitch.get() + pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT));
    }

    /// The ray from the eye through the point `(x, y)` of an image `aspect` wide per unit of
    /// height, `x` and `y` from -1 at the left and bottom to 1 at the right and top, as clip
    /// space has them: its origin and its unit direction.
    #[must_use]
    pub fn ray(&self, aspect: f32, [x, y]: [f32; 2]) -> ([f32; 3], [f32; 3]) {
        let [f, r, u] = self.axes();
        let half_x = (self.fov_x.to_radians() / 2.0).tan();
        let half_y = half_x / aspect.max(f32::EPSILON);
        let (a, b) = (x * half_x, y * half_y);
        let along = |f: f32, r: f32, u: f32| f + r * a + u * b;
        let direction = [
            along(f[0], r[0], u[0]),
            along(f[1], r[1], u[1]),
            along(f[2], r[2], u[2]),
        ];
        let length = dot(direction, direction).sqrt().max(f32::EPSILON);
        (self.origin, direction.map(|v| v / length))
    }

    /// The matrix from map coordinates to wgpu's clip space (depth 0 to 1) for an image
    /// `aspect` wide per unit of height, column-major as WGSL reads it.
    #[must_use]
    pub fn clip_from_world(&self, aspect: f32) -> [[f32; 4]; 4] {
        clip_from_axes(self.origin, self.axes(), self.fov_x.get(), aspect)
    }

    /// The matrix of a view from `origin` along `angles` (pitch, yaw and roll, as
    /// `r_refdef.viewangles`) with a field of view of `fov_x` degrees across (REND-3.4).
    #[must_use]
    pub fn clip_from_view(
        origin: [f32; 3],
        angles: [f32; 3],
        fov_x: f32,
        aspect: f32,
    ) -> [[f32; 4]; 4] {
        let (forward, right, up) = angle_vectors(angles);
        clip_from_axes(origin, [forward, right, up], fov_x, aspect)
    }
}

/// The matrix of a view from `o` along the axes `f`, `r` and `u` with `fov_x` across.
fn clip_from_axes(o: [f32; 3], [f, r, u]: [[f32; 3]; 3], fov_x: f32, aspect: f32) -> [[f32; 4]; 4] {
    // The view's rows: x right, y up, z backwards, as a right-handed view space looks
    // down -z.
    let view = [
        [r[0], u[0], -f[0], 0.0],
        [r[1], u[1], -f[1], 0.0],
        [r[2], u[2], -f[2], 0.0],
        [-dot(r, o), -dot(u, o), dot(f, o), 1.0],
    ];
    // REND-3.4: `CalcFov`: `x = width/tan(fov_x/360*M_PI)`, `fov_y = 2 atan(height/x)`.
    let half_x = (fov_x.to_radians() / 2.0).tan();
    let half_y = half_x / aspect.max(f32::EPSILON);
    let x = 1.0 / half_x;
    let y = 1.0 / half_y;
    let range = FAR / (NEAR - FAR);
    let projection = [
        [x, 0.0, 0.0, 0.0],
        [0.0, y, 0.0, 0.0],
        [0.0, 0.0, range, -1.0],
        [0.0, 0.0, range * NEAR, 0.0],
    ];
    multiply(&projection, &view)
}

/// `a * b` of two column-major matrices.
fn multiply(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0_f32; 4]; 4];
    for (column, b_column) in out.iter_mut().zip(b) {
        for (row, value) in column.iter_mut().enumerate() {
            *value = a
                .iter()
                .zip(b_column)
                .map(|(a_column, &k)| a_column.get(row).copied().unwrap_or(0.0) * k)
                .sum();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::indexing_slicing,
        reason = "test code: a failed index is a failed test"
    )]

    use super::Camera;

    /// `m * v` of a column-major matrix and a point, divided by w.
    fn project(m: &[[f32; 4]; 4], v: [f32; 3]) -> [f32; 3] {
        let p = [v[0], v[1], v[2], 1.0];
        let mut out = [0.0_f32; 4];
        for (column, &k) in m.iter().zip(&p) {
            for (o, c) in out.iter_mut().zip(column) {
                *o += c * k;
            }
        }
        [out[0] / out[3], out[1] / out[3], out[2] / out[3]]
    }

    #[test]
    fn a_point_ahead_is_in_the_middle_and_one_to_the_right_is_right() {
        let camera = Camera::new([0.0, 0.0, 0.0], 0.0);
        let m = camera.clip_from_world(1.0);
        let ahead = project(&m, [100.0, 0.0, 0.0]);
        assert!(ahead[0].abs() < 1e-5 && ahead[1].abs() < 1e-5, "{ahead:?}");
        assert!((0.0..1.0).contains(&ahead[2]), "{ahead:?}");
        // y is left in Quake, so -y is to the right of the screen.
        assert!(project(&m, [100.0, -10.0, 0.0])[0] > 0.0);
        assert!(project(&m, [100.0, 0.0, 10.0])[1] > 0.0);
        // Nearer is nearer in depth.
        assert!(project(&m, [50.0, 0.0, 0.0])[2] < ahead[2]);
    }

    // spec REND-3.4: fov 90 across, the height's from CalcFov, drawn from 4 to 4096 units
    #[test]
    fn the_view_is_glquakes_fov_90() {
        let camera = Camera::new([0.0, 0.0, 0.0], 0.0);
        // At 2:1 a point 45 degrees to the right is at the edge, and one 1/4 as far up as it is
        // ahead is at the top: tan(fov_y/2) = tan(45)/2.
        let m = camera.clip_from_world(2.0);
        let edge = project(&m, [100.0, -100.0, 0.0]);
        assert!((edge[0] - 1.0).abs() < 1e-4, "{edge:?}");
        let top = project(&m, [100.0, 0.0, 50.0]);
        assert!((top[1] - 1.0).abs() < 1e-4, "{top:?}");
        // The near plane at 4 is depth 0 and the far plane at 4096 is depth 1.
        assert!(project(&m, [4.0, 0.0, 0.0])[2].abs() < 1e-4);
        assert!((project(&m, [4096.0, 0.0, 0.0])[2] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn turning_and_travelling_follow_the_axes() {
        let mut camera = Camera::new([0.0, 0.0, 0.0], 0.0);
        camera.turn(90.0, 0.0);
        camera.travel(10.0, 0.0, 0.0);
        assert!(
            (camera.origin[1] - 10.0).abs() < 1e-4,
            "{:?}",
            camera.origin
        );
        camera.turn(0.0, 200.0);
        assert!((camera.pitch.get() - 89.0).abs() < f32::EPSILON);
        camera.turn(-450.0, 0.0);
        assert!(camera.yaw.get().abs() < f32::EPSILON);
    }

    // No spec statement: P5-16: framing a box keeps the camera's direction and puts the whole
    // box in view, its corners inside both half fields of view, in a wide image too
    #[test]
    fn a_framed_box_is_wholly_in_view() {
        let mut camera = Camera::new([0.0, 0.0, 0.0], 30.0);
        camera.pitch = super::Degrees::new(20.0);
        let bounds = [[1000.0, 2000.0, -50.0], [1100.0, 2032.0, 400.0]];
        let aspect = 16.0 / 9.0;
        camera.frame(bounds, aspect, 8.0);
        assert!(
            (camera.yaw.get() - 30.0).abs() < f32::EPSILON
                && (camera.pitch.get() - 20.0).abs() < f32::EPSILON
        );
        let [forward, right, up] = camera.axes();
        let across = (camera.fov_x.to_radians() / 2.0).tan();
        let down = across / aspect;
        for corner in 0..8_u8 {
            let p = [0, 1, 2].map(|axis| {
                if corner & (1 << axis) == 0 {
                    bounds[0][axis]
                } else {
                    bounds[1][axis]
                }
            });
            let v = [0, 1, 2].map(|axis| p[axis] - camera.origin[axis]);
            let dot = |a: [f32; 3]| a[0] * v[0] + a[1] * v[1] + a[2] * v[2];
            let depth = dot(forward);
            assert!(depth > 0.0);
            assert!(
                dot(right).abs() / depth <= across,
                "corner {corner} off the side"
            );
            assert!(
                dot(up).abs() / depth <= down,
                "corner {corner} off the top or bottom"
            );
        }
        // The middle of the box is straight ahead.
        let middle = [1050.0, 2016.0, 175.0];
        let v = [0, 1, 2].map(|axis| middle[axis] - camera.origin[axis]);
        let ahead = v[0] * forward[0] + v[1] * forward[1] + v[2] * forward[2];
        let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        assert!((ahead - length).abs() < 0.01);
    }
}
