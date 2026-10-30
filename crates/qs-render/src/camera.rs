// SPDX-License-Identifier: GPL-3.0-or-later

//! A free camera in Quake's axes: x forward at yaw 0, y left, z up; yaw turns about z and
//! pitch looks down when positive, as Quake's `angles` do.

/// Where the camera is and where it looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Its position in map units.
    pub origin: [f32; 3],
    /// Its yaw in degrees, about z, 0 looking down +x.
    pub yaw: f32,
    /// Its pitch in degrees, positive looking down, kept within ±89.
    pub pitch: f32,
    /// Its field of view across, in degrees; the height's follows from the image's shape
    /// (REND-3.4).
    pub fov_x: f32,
}

/// The nearest and farthest distances drawn, in map units: `MYgluPerspective (r_refdef.fov_y,
/// screenaspect, 4, 4096)` in GLQuake's `R_SetupGL` (REND-3.4).
const NEAR: f32 = 4.0;
const FAR: f32 = 4096.0;
/// `fov`, the field of view across, in degrees: `scr_fov = {"fov","90"}` (REND-3.4).
const FOV_X: f32 = 90.0;

impl Camera {
    /// A camera at `origin` looking along `yaw`, level, with GLQuake's `fov 90` (REND-3.4).
    #[must_use]
    pub const fn new(origin: [f32; 3], yaw: f32) -> Self {
        Self {
            origin,
            yaw,
            pitch: 0.0,
            fov_x: FOV_X,
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
        self.yaw = (self.yaw + yaw).rem_euclid(360.0);
        self.pitch = (self.pitch + pitch).clamp(-89.0, 89.0);
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
        let [f, r, u] = self.axes();
        let o = self.origin;
        // The view's rows: x right, y up, z backwards, as a right-handed view space looks
        // down -z.
        let view = [
            [r[0], u[0], -f[0], 0.0],
            [r[1], u[1], -f[1], 0.0],
            [r[2], u[2], -f[2], 0.0],
            [-dot(r, o), -dot(u, o), dot(f, o), 1.0],
        ];
        // REND-3.4: `CalcFov`: `x = width/tan(fov_x/360*M_PI)`, `fov_y = 2 atan(height/x)`.
        let half_x = (self.fov_x.to_radians() / 2.0).tan();
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
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
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
        assert!((camera.pitch - 89.0).abs() < f32::EPSILON);
        camera.turn(-450.0, 0.0);
        assert!(camera.yaw.abs() < f32::EPSILON);
    }
}
