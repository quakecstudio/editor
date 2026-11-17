// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2017 Eric Wasylishen
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/bounce.cc, Face_ShouldBounce, MakeBounceLight, MakeBounceLightsThread, MakeBounceLights
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/ltface.cc, SurfaceLight_ColorAtDist, GetSurfaceLighting, SurfaceLight_SphereCull, LightFace_SurfaceLight, LightPoint_SurfaceLight
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/common/polylib.hh, area, center, remove_colinear, plane

//! Bounce lighting (LIGHT-20): each face that casts shadow gives back the light that fell on
//! it, as a light of its own a unit above its middle, once for each bounce.

use crate::extended::keys::World;
use crate::extended::math::{Aabb, add, cross, dot, is_zero, length, normalize, scale, sub};
use crate::extended::points::Patch;
use crate::extended::shade::Lightmaps;
use crate::{AXES, Vec3, to_f32};

/// The least a face's area for it to bounce light.
const LEAST_AREA: f32 = 1.0;
/// `DIST_EPSILON`: how near a straight line two edges are for the corner between them to be
/// dropped.
const DIST_EPSILON: f64 = 0.0001;
/// A bounce's contribution at most this on every channel is not traced (LIGHT-20.5).
pub(crate) const BOUNCE_GATE: f32 = 0.01;
/// The nearest a sample is taken to be to a bounce light.
const NEAREST: f32 = 0.01;
/// `LIGHT_ANGLE_EPSILON`: how far behind a bounce light, or a sample's face, the other may
/// be.
const ANGLE_EPSILON: f32 = 0.01;
/// The distance a bounce light's brightness is clamped at, and a surface light's.
pub(crate) const BOUNCE_CLAMP: f32 = 128.0;
/// What a box of what a bounce light sees may miss a face's box by (LIGHT-10.8).
const BOUNDS_EPSILON: f32 = 0.001;
/// Half, of a triangle's cross product and of `_bouncescale`.
const HALF: f32 = 0.5;

/// The light a face gives back in one style on one bounce (`surfacelight_t::per_style_t`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Emission {
    /// The bounce it is of, from 0.
    pub(crate) depth: usize,
    pub(crate) style: i32,
    /// Its brightness, the brightest channel of the light given back times the face's area.
    pub(crate) intensity: f32,
    /// Its colour, the brightest channel at most 1.
    pub(crate) colour: Vec3,
}

/// A face that gives light back (`surfacelight_t`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Emitter {
    /// A unit above the face's middle.
    pub(crate) pos: Vec3,
    pub(crate) normal: Vec3,
    /// What can be seen from it, when lights are culled by it (LIGHT-10.8).
    pub(crate) bounds: Option<Aabb>,
    pub(crate) emissions: Vec<Emission>,
}

/// `winding_base_t::remove_colinear`: `points` without the corners its two edges run
/// straight through.
fn without_colinear(points: &[Vec3]) -> Vec<Vec3> {
    let count = points.len();
    let mut out = Vec::with_capacity(count);
    for (index, &here) in points.iter().enumerate() {
        let next = index.checked_add(1).and_then(|n| n.checked_rem(count));
        let before = index
            .checked_add(count)
            .and_then(|n| n.checked_sub(1))
            .and_then(|n| n.checked_rem(count));
        let (Some(&next), Some(&before)) = (
            next.and_then(|n| points.get(n)),
            before.and_then(|n| points.get(n)),
        ) else {
            continue;
        };
        let (v1, v2) = (normalize(sub(next, here)), normalize(sub(here, before)));
        if f64::from(dot(v1, v2)) < 1.0 - DIST_EPSILON {
            out.push(here);
        }
    }
    out
}

/// `winding_base_t::area`: the triangles fanned from the first point, summed in the order
/// of the points.
fn area(points: &[Vec3]) -> f32 {
    let Some(&first) = points.first() else {
        return 0.0;
    };
    let mut total = 0.0_f32;
    for pair in points.windows(2).skip(1) {
        if let [a, b] = *pair {
            let triangle = f64::from(length(cross(sub(a, first), sub(b, first))));
            total = to_f32(f64::from(total) + f64::from(HALF) * triangle);
        }
    }
    total
}

/// What a face's bounce needs of the map.
pub(crate) struct Bouncer<'a> {
    pub(crate) world: &'a World,
    /// Whether each face may bounce light: it always casts shadow, has a lightmap, is not
    /// `skip` and has not `_bounce -1` (`Face_ShouldBounce`).
    pub(crate) bounces: &'a [bool],
    /// Each face's corners, moved with its model.
    pub(crate) corners: &'a [Vec<Vec3>],
    /// The colour each face's texture gives the light it bounces
    /// (`Face_LookupTextureBounceColor`).
    pub(crate) texture: &'a [Vec3],
}

impl Bouncer<'_> {
    /// `MakeBounceLightsThread`: the light `face` gives back on bounce `depth`, from what its
    /// lightmaps took since the last, added to its emitter `slot`; whether it gives any.
    /// The light counted is taken off the lightmaps (LIGHT-20.2, LIGHT-20.3).
    pub(crate) fn emit(
        &self,
        face: usize,
        lightmaps: &mut Lightmaps,
        slot: &mut Option<Emitter>,
        depth: usize,
    ) -> bool {
        if !self.bounces.get(face).copied().unwrap_or(false) || lightmaps.all.is_empty() {
            return false;
        }
        let Some(corners) = self.corners.get(face) else {
            return false;
        };
        let face_area = area(corners);
        if face_area < LEAST_AREA {
            return false;
        }
        let corners = without_colinear(corners);
        let samples = lightmaps
            .all
            .first()
            .map_or(1.0, |lm| count_of(lm.samples.len()));
        let styled = self.world.bouncestyled.get();
        let mut sums: Vec<(i32, Vec3)> = Vec::new();
        for lightmap in &mut lightmaps.all {
            let Some(style) = lightmap.style else {
                continue;
            };
            if style != 0 && !styled {
                continue;
            }
            let taken = core::mem::take(&mut lightmap.bounce);
            if !is_zero(taken) {
                sums.push((style, scale(taken, 1.0 / samples)));
            }
        }
        if sums.is_empty() {
            return false;
        }
        let texture = self.texture.get(face).copied().unwrap_or_default();
        // `winding_base_t::plane` and `center`.
        let (Some(&p0), Some(&p1), Some(&p2)) = (corners.first(), corners.get(1), corners.get(2))
        else {
            return false;
        };
        let normal = normalize(cross(sub(p0, p1), sub(p2, p1)));
        let mut middle = [0.0; AXES];
        for &corner in &corners {
            middle = add(middle, corner);
        }
        let pos = add(scale(middle, 1.0 / count_of(corners.len())), normal);
        for (style, sum) in sums {
            // `MakeBounceLight`.
            let mut colour = [
                sum[0] * texture[0],
                sum[1] * texture[1],
                sum[2] * texture[2],
            ];
            let intensity = colour[0].max(colour[1]).max(colour[2]);
            if intensity <= 0.0 {
                continue;
            }
            if intensity > 1.0 {
                colour = scale(colour, 1.0 / intensity);
            }
            let emitter = slot.get_or_insert_with(|| Emitter {
                pos,
                normal,
                bounds: None,
                emissions: Vec::new(),
            });
            emitter.emissions.push(Emission {
                depth,
                style,
                intensity: intensity * face_area,
                colour,
            });
        }
        true
    }
}

/// A count as a float.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec LIGHT-20.2: a count of samples is far within a float's whole numbers"
)]
const fn count_of(value: usize) -> f32 {
    value as f32
}

impl Emission {
    /// `SurfaceLight_ColorAtDist`: `colour` at `intensity` falling with the square of the
    /// distance, which is taken as at least [`BOUNCE_CLAMP`], scaled by `by` (LIGHT-20.4).
    fn colour_at(&self, world: &World, by: f32, dist: f32) -> Vec3 {
        let emission = self;
        let reach = (world.scaledist.get() * dist).max(BOUNCE_CLAMP);
        scale(emission.colour, emission.intensity / (reach * reach) * by)
    }
}

impl Emitter {
    /// `SurfaceLight_SphereCull`: whether `emission` cannot reach `patch`: what the emitter
    /// sees misses the face's box, or the whole of its light is too dim at the far side of
    /// the sphere around the face (LIGHT-20.5).
    pub(crate) fn is_culled(&self, world: &World, emission: &Emission, patch: &Patch) -> bool {
        if self
            .bounds
            .is_some_and(|bounds| bounds.is_apart_from(patch.extents.bounds, BOUNDS_EPSILON))
        {
            return true;
        }
        let dist = length(sub(patch.extents.origin, self.pos)) + patch.extents.radius;
        emission
            .colour_at(world, 1.0, dist)
            .iter()
            .all(|c| *c <= BOUNCE_GATE)
    }

    /// `GetSurfaceLighting`: what `emission` gives a point `dist` from the emitter along the
    /// unit `dir`, whose normal is `normal`; nothing for a point behind the emitter, or one
    /// whose face is turned from it (LIGHT-20.4).
    pub(crate) fn lighting(
        &self,
        world: &World,
        emission: &Emission,
        (dir, dist): (Vec3, f32),
        normal: Option<Vec3>,
    ) -> Vec3 {
        let from_emitter = dot(self.normal, dir);
        let to_emitter = normal.map_or(1.0, |normal| dot(scale(dir, -1.0), normal));
        if from_emitter < -ANGLE_EPSILON || to_emitter < -ANGLE_EPSILON {
            return [0.0; AXES];
        }
        let factor = (from_emitter * to_emitter).max(0.0);
        if factor <= 0.0 {
            return [0.0; AXES];
        }
        emission.colour_at(world, world.bouncescale.get() * HALF * factor, dist)
    }

    /// The unit direction and the distance from the emitter to `point`, the distance at
    /// least 0.01.
    pub(crate) fn toward(&self, point: Vec3) -> (Vec3, f32) {
        let away = sub(point, self.pos);
        let dist = length(away).max(NEAREST);
        (away.map(|c| c / dist), dist)
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
    use crate::extended::shade::Lightmap;

    const SQUARE: [Vec3; 5] = [
        [0.0, 0.0, 0.0],
        [0.0, 32.0, 0.0],
        [0.0, 64.0, 0.0],
        [64.0, 64.0, 0.0],
        [64.0, 0.0, 0.0],
    ];

    fn lightmaps(style: Option<i32>, bounce: Vec3) -> Lightmaps {
        let mut lightmaps = Lightmaps::new(4);
        lightmaps.all.push(Lightmap {
            style,
            samples: vec![[0.0; 3]; 4],
            bounce,
        });
        lightmaps
    }

    // spec LIGHT-20.2: a face gives back the mean of the light its samples took, times its
    // texture's colour, from a unit above its middle; its brightness is its brightest channel
    // times its area, and the light counted is taken off.
    #[test]
    fn a_face_gives_back_the_mean_of_its_light_from_above_its_middle() {
        let world = World::default();
        let bouncer = Bouncer {
            world: &world,
            bounces: &[true, false],
            corners: &[SQUARE.to_vec(), SQUARE.to_vec()],
            texture: &[[0.5; 3], [0.5; 3]],
        };
        let mut lit = lightmaps(Some(0), [800.0, 400.0, 0.0]);
        let mut slot = None;
        assert!(bouncer.emit(0, &mut lit, &mut slot, 0));
        let emitter = slot.clone().unwrap();
        // The corner on the straight edge is dropped: the middle is that of the four.
        assert_eq!(emitter.pos, [32.0, 32.0, 1.0]);
        assert_eq!(emitter.normal, [0.0, 0.0, 1.0]);
        let emission = emitter.emissions.first().copied().unwrap();
        // 800 over 4 samples is 200, by the texture's half is 100: past 1, so the colour is
        // scaled to it and the brightness keeps it, times the area of 4096.
        assert_eq!(emission.colour, [1.0, 0.5, 0.0]);
        assert_eq!(emission.intensity, 100.0 * 4096.0);
        assert_eq!((emission.depth, emission.style), (0, 0));
        assert_eq!(lit.all.first().map(|lm| lm.bounce), Some([0.0; 3]));
        assert!(
            !bouncer.emit(0, &mut lit, &mut slot, 1),
            "nothing new to give back"
        );
        assert!(!bouncer.emit(1, &mut lightmaps(Some(0), [8.0; 3]), &mut None, 0));
        assert!(!bouncer.emit(0, &mut Lightmaps::new(4), &mut None, 0));
    }

    // spec LIGHT-20.3: only the lightmap of style 0 bounces, unless _bouncestyled is set.
    #[test]
    fn only_style_zero_bounces_unless_styled() {
        let mut world = World::default();
        let corners = [SQUARE.to_vec()];
        let texture = [[0.5; 3]];
        let bouncer = Bouncer {
            world: &world,
            bounces: &[true],
            corners: &corners,
            texture: &texture,
        };
        assert!(!bouncer.emit(0, &mut lightmaps(Some(5), [8.0; 3]), &mut None, 0));
        assert!(!bouncer.emit(0, &mut lightmaps(None, [8.0; 3]), &mut None, 0));
        world.bouncestyled.set(true, Source::Map);
        let bouncer = Bouncer {
            world: &world,
            bounces: &[true],
            corners: &corners,
            texture: &texture,
        };
        let mut slot = None;
        assert!(bouncer.emit(0, &mut lightmaps(Some(5), [8.0; 3]), &mut slot, 0));
        assert_eq!(
            slot.and_then(|e| e.emissions.first().map(|e| e.style)),
            Some(5)
        );
    }

    // spec LIGHT-20.4: a bounce light's light falls with the square of the distance, taken
    // as at least 128, by the cosines at the emitter and at the sample and half of
    // _bouncescale; nothing reaches behind the emitter.
    #[test]
    fn a_bounce_falls_with_the_square_of_the_distance_and_both_cosines() {
        let world = World::default();
        let emitter = Emitter {
            pos: [0.0, 0.0, 1.0],
            normal: [0.0, 0.0, 1.0],
            bounds: None,
            emissions: Vec::new(),
        };
        let emission = Emission {
            depth: 0,
            style: 0,
            intensity: 128.0 * 128.0 * 100.0,
            colour: [1.0, 0.5, 0.0],
        };
        let above = [0.0, 0.0, 257.0];
        let (dir, dist) = emitter.toward(above);
        assert_eq!((dir, dist), ([0.0, 0.0, 1.0], 256.0));
        let facing = emitter.lighting(&world, &emission, (dir, dist), Some([0.0, 0.0, -1.0]));
        // 100 at 128 units is 25 at 256, by a half.
        assert_eq!(facing, [12.5, 6.25, 0.0]);
        let near = emitter.lighting(&world, &emission, ([0.0, 0.0, 1.0], 8.0), None);
        assert_eq!(near, [50.0, 25.0, 0.0], "nearer than 128 is as at 128");
        let turned = emitter.lighting(&world, &emission, (dir, dist), Some([0.0, 0.0, 1.0]));
        assert_eq!(turned, [0.0; 3]);
        let below = emitter.lighting(&world, &emission, ([0.0, 0.0, -1.0], 64.0), None);
        assert_eq!(below, [0.0; 3]);
        assert_eq!(emitter.toward(emitter.pos).1, 0.01);
    }

    #[test]
    fn a_windings_area_and_corners() {
        assert_eq!(area(&SQUARE), 4096.0);
        assert_eq!(without_colinear(&SQUARE).len(), 4);
        assert_eq!(area(&[]), 0.0);
    }
}
