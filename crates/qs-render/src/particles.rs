// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/r_part.c, R_DrawParticles
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_rmisc.c, R_InitParticleTexture

//! Particles as GLQuake draws them (PART-3.2): one triangle each, with the particle texture,
//! in the particle's colour, scaled up with its distance "to keep particles from
//! disapearing".

use qs_client::SceneParticle;

use crate::mesh::{Image, Palette};
use crate::sprite::Eye;
use crate::vector::{add, scale, sub};
use crate::{CHANNEL_MAX, RGBA_BYTES};

/// `VectorScale (vup, 1.5, up)`: the triangle's side along each axis (PART-3.2).
const SIDE: f32 = 1.5;
/// `if (scale < 20) scale = 1`: nearer than this a particle is drawn at scale 1 (PART-3.2).
const NEAR: f32 = 20.0;
/// `scale = 1 + scale * 0.004` (PART-3.2).
const GROWTH: f32 = 0.004;
/// The side of `dottexture` (REND-7, `R_InitParticleTexture`).
const DOT_SIDE: usize = 8;
/// `dottexture[8][8]`: the particle's shape, 1 where it is drawn.
const DOT_TEXTURE: [[u8; DOT_SIDE]; DOT_SIDE] = [
    [0, 1, 1, 0, 0, 0, 0, 0],
    [1, 1, 1, 1, 0, 0, 0, 0],
    [1, 1, 1, 1, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
];

/// One corner of a particle's triangle on the GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleVertex {
    /// Its position in the map.
    pub position: [f32; 3],
    /// Its coordinates in the particle texture.
    pub texture: [f32; 2],
    /// The particle's colour, RGBA 0 to 1.
    pub color: [f32; 4],
}

/// A particle's triangle's corners.
const TRIANGLE_CORNERS: usize = 3;

/// `R_InitParticleTexture`: the 8 by 8 texture, white with `dottexture` as its alpha; the
/// table is read `dottexture[x][y]` into `data[y][x]`, so it is transposed.
#[must_use]
pub fn particle_texture() -> Image {
    let mut rgba = Vec::with_capacity(DOT_SIDE * DOT_SIDE * RGBA_BYTES);
    for y in 0..DOT_SIDE {
        for x in 0..DOT_SIDE {
            let alpha = DOT_TEXTURE
                .get(x)
                .and_then(|row| row.get(y))
                .copied()
                .unwrap_or(0);
            rgba.extend([
                CHANNEL_MAX,
                CHANNEL_MAX,
                CHANNEL_MAX,
                alpha.saturating_mul(CHANNEL_MAX),
            ]);
        }
    }
    Image::square(DOT_SIDE, rgba)
}

/// `R_DrawParticles`' triangles for `particles` seen from `eye`, each in its palette colour
/// (PART-3.2).
#[must_use]
pub fn particle_triangles(
    particles: &[SceneParticle],
    palette: &Palette,
    eye: &Eye,
) -> Vec<ParticleVertex> {
    let up = scale(eye.up, SIDE);
    let right = scale(eye.right, SIDE);
    let mut out = Vec::with_capacity(particles.len().saturating_mul(TRIANGLE_CORNERS));
    for particle in particles {
        // "hack a scale up to keep particles from disapearing"
        let distance = {
            let d = sub(particle.origin, eye.origin);
            d[0] * eye.forward[0] + d[1] * eye.forward[1] + d[2] * eye.forward[2]
        };
        let scale_by = if distance < NEAR {
            1.0
        } else {
            1.0 + distance * GROWTH
        };
        let [r, g, b, a] = palette.rgba(particle.color);
        let color = [r, g, b, a].map(|c| f32::from(c) / f32::from(CHANNEL_MAX));
        let corner = |position, texture| ParticleVertex {
            position,
            texture,
            color,
        };
        out.extend([
            corner(particle.origin, [0.0, 0.0]),
            corner(add(particle.origin, scale(up, scale_by)), [1.0, 0.0]),
            corner(add(particle.origin, scale(right, scale_by)), [0.0, 1.0]),
        ]);
    }
    out
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use qs_client::SceneParticle;

    use super::{particle_texture, particle_triangles};
    use crate::mesh::Palette;
    use crate::sprite::Eye;

    fn palette() -> Palette {
        let bytes: Vec<u8> = (0..=255_u8).flat_map(|i| [i, 1, 0]).collect();
        Palette::read(&bytes).unwrap()
    }

    fn eye() -> Eye {
        Eye {
            origin: [0.0; 3],
            forward: [1.0, 0.0, 0.0],
            right: [0.0, -1.0, 0.0],
            up: [0.0, 0.0, 1.0],
        }
    }

    // spec PART-3.2: the particle texture is white with `dottexture` as its alpha, read
    // transposed
    #[test]
    fn the_particle_texture_is_the_dot_transposed() {
        let image = particle_texture();
        assert_eq!((image.width, image.height), (8, 8));
        let alpha = |x: usize, y: usize| image.rgba[(y * 8 + x) * 4 + 3];
        assert_eq!(&image.rgba[..4], [255, 255, 255, 0]);
        // dottexture[0][1] = 1 goes to data[1][0]: x 0, y 1. The dot is symmetric, so the
        // transposition shows only in the reading.
        assert_eq!(alpha(0, 1), 255);
        assert_eq!(alpha(1, 0), 255);
        assert_eq!(alpha(3, 1), 255);
        assert_eq!(alpha(3, 0), 0);
        assert_eq!(alpha(0, 3), 0);
        assert_eq!(alpha(4, 1), 0);
        assert_eq!(alpha(7, 7), 0);
    }

    // spec PART-3.2: one triangle at the origin, up and right by 1.5 times the scale, which
    // is 1 within 20 units of the eye and grows by 0.004 a unit beyond
    #[test]
    fn a_particle_is_a_triangle_that_grows_with_distance() {
        let particles = [
            SceneParticle {
                origin: [10.0, 0.0, 0.0],
                color: 9,
            },
            SceneParticle {
                origin: [100.0, 5.0, 5.0],
                color: 250,
            },
        ];
        let out = particle_triangles(&particles, &palette(), &eye());
        assert_eq!(out.len(), 6);
        let positions: Vec<[f32; 3]> = out.iter().map(|v| v.position).collect();
        assert_eq!(
            positions[..3],
            [[10.0, 0.0, 0.0], [10.0, 0.0, 1.5], [10.0, -1.5, 0.0]]
        );
        // 100 along the forward: 1 + 0.4, and 1.5 * 1.4 = 2.1.
        assert_eq!(positions[3], [100.0, 5.0, 5.0]);
        assert_eq!(positions[4], [100.0, 5.0, 5.0 + 2.1]);
        assert_eq!(positions[5], [100.0, 5.0 - 2.1, 5.0]);
        let textures: Vec<[f32; 2]> = out.iter().take(3).map(|v| v.texture).collect();
        assert_eq!(textures, [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        assert_eq!(out[0].color, [9.0 / 255.0, 1.0 / 255.0, 0.0, 1.0]);
        assert_eq!(out[5].color, [250.0 / 255.0, 1.0 / 255.0, 0.0, 1.0]);
        // At exactly 20 the scale is already 1 + 0.08.
        let edge = particle_triangles(
            &[SceneParticle {
                origin: [20.0, 0.0, 0.0],
                color: 0,
            }],
            &palette(),
            &eye(),
        );
        assert_eq!(edge[1].position, [20.0, 0.0, 1.5 * 1.08]);
    }
}
