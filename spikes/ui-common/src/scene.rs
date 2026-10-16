// SPDX-License-Identifier: GPL-3.0-or-later
//! The 3D viewport's content: a spinning, face-coloured box standing in for a map preview.
//!
//! Both prototypes draw it with wgpu using this mesh, shader and matrix, so the viewport costs
//! the same GPU work in each; what differs is how the framework hosts it.

/// One vertex: position and colour.
pub type Vertex = [f32; 6];

/// A face: its outward normal, two in-plane axes and its colour.
type Face = ([f32; 3], [f32; 3], [f32; 3], [f32; 3]);

/// The shader: transforms by one uniform matrix and passes the colour through.
pub const SHADER: &str = r"
struct Uniforms { mvp: mat4x4<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @location(1) color: vec3<f32>) -> Out {
    var out: Out;
    out.pos = u.mvp * vec4<f32>(pos, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: Out) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
";

/// 36 vertices: six faces of a unit box, two counter-clockwise triangles each.
#[must_use]
pub fn cube() -> Vec<Vertex> {
    // (normal axis, sign, colour) per face; colours from the theme's accent family.
    let faces: [Face; 6] = [
        (
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.93, 0.55, 0.27],
        ),
        (
            [-1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0],
            [0.66, 0.30, 0.09],
        ),
        (
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            [0.53, 0.75, 0.82],
        ),
        (
            [0.0, -1.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.33, 0.47, 0.51],
        ),
        (
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.55, 0.77, 0.49],
        ),
        (
            [0.0, 0.0, -1.0],
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.89, 0.69, 0.29],
        ),
    ];
    let mut out = Vec::with_capacity(36);
    for (n, u, v, color) in faces {
        let point = |su: f32, sv: f32| -> Vertex {
            let p = [
                0.5 * n[0] + 0.5 * su * u[0] + 0.5 * sv * v[0],
                0.5 * n[1] + 0.5 * su * u[1] + 0.5 * sv * v[1],
                0.5 * n[2] + 0.5 * su * u[2] + 0.5 * sv * v[2],
            ];
            [p[0], p[1], p[2], color[0], color[1], color[2]]
        };
        let (a, b, c, d) = (
            point(-1.0, -1.0),
            point(1.0, -1.0),
            point(1.0, 1.0),
            point(-1.0, 1.0),
        );
        out.extend_from_slice(&[a, b, c, a, c, d]);
    }
    out
}

/// A column-major 4x4 matrix, as WGSL's `mat4x4<f32>` expects.
pub type Mat4 = [[f32; 4]; 4];

fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0f32; 4]; 4];
    for (c, col) in out.iter_mut().enumerate() {
        for (r, cell) in col.iter_mut().enumerate() {
            let mut sum = 0.0;
            for k in 0..4 {
                let a_rk = a.get(k).and_then(|col| col.get(r)).copied().unwrap_or(0.0);
                let b_kc = b.get(c).and_then(|col| col.get(k)).copied().unwrap_or(0.0);
                sum += a_rk * b_kc;
            }
            *cell = sum;
        }
    }
    out
}

/// The model-view-projection matrix at time `t` seconds for a viewport `aspect` wide per unit
/// tall, in wgpu's 0..1 depth convention.
#[must_use]
pub fn mvp(t: f32, aspect: f32) -> Mat4 {
    let (sy, cy) = (t * 0.9).sin_cos();
    let (sx, cx) = (0.45f32).sin_cos();
    let rot_y: Mat4 = [
        [cy, 0.0, -sy, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [sy, 0.0, cy, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let rot_x: Mat4 = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, cx, sx, 0.0],
        [0.0, -sx, cx, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let view: Mat4 = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, -2.6, 1.0],
    ];
    let fov = 1.0f32;
    let f = 1.0 / (fov / 2.0).tan();
    let (near, far) = (0.1f32, 10.0f32);
    let proj: Mat4 = [
        [f / aspect.max(0.01), 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), -1.0],
        [0.0, 0.0, near * far / (near - far), 0.0],
    ];
    mul(&proj, &mul(&view, &mul(&rot_x, &rot_y)))
}

/// The matrix as the 64 bytes a uniform buffer holds.
#[must_use]
pub fn mat_bytes(m: &Mat4) -> Vec<u8> {
    m.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
}

/// The vertices as bytes for a vertex buffer.
#[must_use]
pub fn vertex_bytes(vertices: &[Vertex]) -> Vec<u8> {
    vertices
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cube_has_six_outward_faces() {
        let cube = cube();
        assert_eq!(cube.len(), 36);
        for tri in cube.chunks(3) {
            let [a, b, c] = [tri[0], tri[1], tri[2]];
            let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let centre = [
                (a[0] + b[0] + c[0]) / 3.0,
                (a[1] + b[1] + c[1]) / 3.0,
                (a[2] + b[2] + c[2]) / 3.0,
            ];
            let outward = n[0] * centre[0] + n[1] * centre[1] + n[2] * centre[2];
            assert!(outward > 0.0, "triangle faces inward: {tri:?}");
        }
    }

    #[test]
    fn the_matrix_puts_the_cube_in_front_of_the_camera() {
        // The origin maps to the matrix's last column (column-major).
        let m = mvp(0.0, 1.0);
        let [x, y, z, w] = m[3];
        assert!((w - 2.6).abs() < 1e-5, "w = {w}");
        assert!(x.abs() < 1e-5 && y.abs() < 1e-5);
        let depth = z / w;
        assert!((0.0..1.0).contains(&depth), "depth = {depth}");
    }
}
