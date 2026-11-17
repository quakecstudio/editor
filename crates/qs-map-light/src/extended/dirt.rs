// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2017 Eric Wasylishen
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/ltface.cc, SetupDirt, TransformToTangentSpace, GetDirtVector, LightFace_CalculateDirt
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/common/qvec.hh, MakeTangentAndBitangentUnnormalized

//! Dirt (LIGHT-19): how much of the space above each sample is closed off, from rays cast
//! over a dome about its normal, which darkens the light it takes in corners and crevices.

use crate::extended::keys::World;
use crate::extended::lights::Lamp;
use crate::extended::math::{add, cross, dot, normalize, scale};
use crate::extended::points::Patch;
use crate::extended::rays::{Asker, Rays, Surface, Texels};
use crate::{AXES, Vec3, to_f32};

/// `DIRT_NUM_ANGLE_STEPS` and `DIRT_NUM_ELEVATION_STEPS`: the rays about the normal, and at
/// each the rays from it outward (LIGHT-19.2).
const ANGLE_STEPS: u8 = 16;
const ELEVATION_STEPS: u8 = 3;
/// Half, of a step: each ray is in the middle of its part of the angle.
const HALF: f32 = 0.5;
/// Degrees to a half turn and in a whole one.
const HALF_TURN: f64 = 180.0;
const WHOLE_TURN: f32 = 360.0;

/// The dome of directions dirt is measured over, about a normal along z.
pub(crate) struct Dirt {
    vectors: Vec<Vec3>,
    /// `_dirtdepth`: how far each ray goes.
    depth: f32,
}

/// `MakeTangentAndBitangentUnnormalized`: two unit directions across `normal`, in the order
/// that turns to it.
fn frame(normal: Vec3) -> (Vec3, Vec3) {
    let mut axis = 0;
    let mut most = 0.0_f32;
    for (index, component) in normal.iter().enumerate() {
        if component.abs() > most {
            most = component.abs();
            axis = index;
        }
    }
    let unit = |at: usize| {
        let mut out = [0.0; AXES];
        if let Some(slot) = out.get_mut(at.checked_rem(AXES).unwrap_or(0)) {
            *slot = 1.0;
        }
        out
    };
    let tangent = cross(normal, unit(axis.saturating_add(1)));
    let bitangent = cross(normal, unit(axis.saturating_add(2)));
    if dot(cross(tangent, bitangent), normal) < 0.0 {
        (normalize(bitangent), normalize(tangent))
    } else {
        (normalize(tangent), normalize(bitangent))
    }
}

impl Dirt {
    /// `SetupDirt`: the dome, when the world, its minimum light, a sun or any light asks for
    /// dirt; none when nothing does (LIGHT-19.1, LIGHT-19.2).
    pub(crate) fn new(world: &World, lamps: &[Lamp]) -> Option<Self> {
        let in_use = world.dirt.get()
            || world.minlight_dirt.get()
            || world.sunlight_dirt.is_on()
            || world.sunlight2_dirt.is_on()
            || lamps.iter().any(|lamp| lamp.keys.dirt.is_on());
        if !in_use {
            return None;
        }
        let radians = |degrees: f32| to_f32(f64::from(degrees) * core::f64::consts::PI / HALF_TURN);
        let angle_step = radians(WHOLE_TURN / f32::from(ANGLE_STEPS));
        let elevation_step = radians(world.dirtangle.get() / f32::from(ELEVATION_STEPS));
        let mut vectors = Vec::new();
        let mut angle = 0.0_f32;
        for _ in 0..ANGLE_STEPS {
            let mut elevation = elevation_step * HALF;
            for _ in 0..ELEVATION_STEPS {
                // In single precision, as C++ works a float's sine and cosine.
                vectors.push([
                    elevation.sin() * angle.cos(),
                    elevation.sin() * angle.sin(),
                    elevation.cos(),
                ]);
                elevation += elevation_step;
            }
            angle += angle_step;
        }
        Some(Self {
            vectors,
            depth: world.dirtdepth.get(),
        })
    }

    /// `LightFace_CalculateDirt`: each sample's occlusion: one less the mean, over the dome,
    /// of how far each ray goes before it meets anything but sky, as a part of the depth
    /// (LIGHT-19.3).
    pub(crate) fn occlude(
        &self,
        patch: &mut Patch,
        rays: &Rays,
        asker: &Asker,
        texels: &dyn Texels,
        stack: &mut Vec<usize>,
    ) {
        let count = u16::try_from(self.vectors.len()).map_or(1.0, f32::from);
        for sample in &mut patch.samples {
            let mut total = 0.0_f32;
            if !sample.occluded {
                let (up, right) = frame(sample.normal);
                for vector in &self.vectors {
                    let dir = add(
                        add(scale(right, vector[0]), scale(up, vector[1])),
                        scale(sample.normal, vector[2]),
                    );
                    let struck = rays.struck((sample.point, dir, self.depth), asker, texels, stack);
                    total += match struck.hit {
                        Some((surface, dist)) if surface != Surface::Sky => self.depth.min(dist),
                        _ => self.depth,
                    };
                }
            }
            sample.occlusion = 1.0 - (total / count) / self.depth;
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: the values are exact in single precision, and a failed unwrap \
                  is a failed test"
    )]

    use super::*;
    use crate::extended::keys::Source;
    use crate::extended::math::length;

    // spec LIGHT-19.2: the dome is 16 directions about the normal at each of three
    // elevations, the middles of three equal parts of _dirtangle.
    #[test]
    fn the_dome_is_forty_eight_directions_within_the_dirt_angle() {
        let mut world = World::default();
        world.dirt.set(true, Source::Map);
        let dirt = Dirt::new(&world, &[]).unwrap();
        assert_eq!(dirt.vectors.len(), 48);
        assert_eq!(dirt.depth, 128.0);
        for vector in &dirt.vectors {
            assert!((length(*vector) - 1.0).abs() < 1e-6);
        }
        // The first is 88 / 6 degrees from the normal, toward x.
        let first = dirt.vectors.first().copied().unwrap_or_default();
        let from_normal = (88.0_f32 / 6.0).to_radians();
        assert!((first[2] - from_normal.cos()).abs() < 1e-6);
        assert!((first[0] - from_normal.sin()).abs() < 1e-6);
        assert_eq!(first[1], 0.0);
        let lowest = dirt
            .vectors
            .iter()
            .fold(1.0_f32, |least, v| least.min(v[2]));
        assert!((lowest - (88.0_f32 * 5.0 / 6.0).to_radians().cos()).abs() < 1e-6);
    }

    // spec LIGHT-19.1: dirt is worked out only when the world, its minimum light, a sun or a
    // light asks for it.
    #[test]
    fn dirt_is_in_use_only_when_something_asks() {
        assert!(Dirt::new(&World::default(), &[]).is_none());
        let mut sun = World::default();
        sun.sunlight2_dirt.set(1.0, Source::Map);
        assert!(Dirt::new(&sun, &[]).is_some());
        let mut off = World::default();
        off.sunlight_dirt.set(-1.0, Source::Map);
        assert!(Dirt::new(&off, &[]).is_none());
        let mut min = World::default();
        min.minlight_dirt.set(true, Source::Map);
        assert!(Dirt::new(&min, &[]).is_some());
    }

    #[test]
    fn a_frame_is_two_unit_directions_across_the_normal() {
        for normal in [[0.0, 0.0, 1.0], [0.0, -1.0, 0.0], [0.6, 0.0, 0.8]] {
            let (up, right) = frame(normal);
            assert!(dot(up, normal).abs() < 1e-6);
            assert!(dot(right, normal).abs() < 1e-6);
            assert!((length(up) - 1.0).abs() < 1e-6);
            assert!(dot(cross(up, right), normal) > 0.0);
        }
    }
}
