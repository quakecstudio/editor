// SPDX-License-Identifier: GPL-3.0-or-later

//! Picking (P5-05): what a ray from the eye hits first among a mesh's triangles, so a click
//! in the map view selects the surface, brush entity or texture under the pointer.
//!
//! GLQuake has no picking; the map editors this follows (TrenchBroom, Radiant) cast a ray
//! from the eye through the pointer and take the nearest surface it hits, as here. Each model
//! is placed by its current matrix, so a door is picked where it is drawn.

use crate::mesh::{MapMesh, Vertex};

/// What a ray hit first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// The model hit, an index into [`MapMesh::models`]: 0 for the world.
    pub model: usize,
    /// The texture of the surface hit, an index into [`MapMesh::textures`].
    pub texture: usize,
    /// The first index of the triangle hit, into [`MapMesh::indices`].
    pub triangle: usize,
    /// How far along the ray it was, in map units.
    pub distance: f32,
    /// Where it was, in the map.
    pub point: [f32; 3],
}

impl MapMesh {
    /// The nearest triangle the ray from `origin` along `direction` hits from either side,
    /// each model placed by `placements`' matrix, the world's first, as
    /// [`crate::MapRenderer::set_placements`] draws them; a model with no matrix there by its
    /// placement when the mesh was built. `None` when it hits nothing.
    #[must_use]
    pub fn pick(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        placements: &[[[f32; 4]; 4]],
    ) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        for batch in &self.batches {
            let Some(matrix) = placements
                .get(batch.model)
                .copied()
                .or_else(|| self.models.get(batch.model).map(|p| p.matrix()))
            else {
                continue;
            };
            let first = usize::try_from(batch.first).unwrap_or(usize::MAX);
            let count = usize::try_from(batch.count).unwrap_or(0);
            let indices = self
                .indices
                .get(first..first.saturating_add(count))
                .unwrap_or_default();
            for (n, &[i, j, k]) in indices.as_chunks::<3>().0.iter().enumerate() {
                let corner = |index: u32| -> Option<[f32; 3]> {
                    let Vertex { position, .. } =
                        self.vertices.get(usize::try_from(index).ok()?)?;
                    Some(transform(&matrix, *position))
                };
                let (Some(a), Some(b), Some(c)) = (corner(i), corner(j), corner(k)) else {
                    continue;
                };
                let Some(distance) = intersect(origin, direction, [a, b, c]) else {
                    continue;
                };
                if best.is_none_or(|hit| distance < hit.distance) {
                    best = Some(Hit {
                        model: batch.model,
                        texture: batch.texture,
                        triangle: first.saturating_add(n.saturating_mul(3)),
                        distance,
                        point: add(origin, scale(direction, distance)),
                    });
                }
            }
        }
        best
    }
}

/// `point` moved by the column-major `matrix`.
fn transform(&[c0, c1, c2, c3]: &[[f32; 4]; 4], [x, y, z]: [f32; 3]) -> [f32; 3] {
    let row = |i: usize| -> f32 {
        let at = |column: [f32; 4]| column.get(i).copied().unwrap_or(0.0);
        at(c0) * x + at(c1) * y + at(c2) * z + at(c3)
    };
    [row(0), row(1), row(2)]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f32; 3], by: f32) -> [f32; 3] {
    a.map(|v| v * by)
}

/// The Möller–Trumbore ray and triangle test: how far along the ray it meets the triangle,
/// in front of the origin, from either side.
fn intersect(origin: [f32; 3], direction: [f32; 3], [a, b, c]: [[f32; 3]; 3]) -> Option<f32> {
    let e1 = sub(b, a);
    let e2 = sub(c, a);
    let p = cross(direction, e2);
    // A ray along the triangle's plane has no crossing: its inverse is not finite.
    let inverse = 1.0 / dot(e1, p);
    if !inverse.is_finite() {
        return None;
    }
    let t_vector = sub(origin, a);
    let u = dot(t_vector, p) * inverse;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(t_vector, e1);
    let v = dot(direction, q) * inverse;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = dot(e2, q) * inverse;
    (t > 0.0).then_some(t)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
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

#[cfg(test)]
mod tests;
