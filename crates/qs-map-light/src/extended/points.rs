// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/ltface.cc, CalcPoints, CalcPointNormal, PositionSamplePointOnFace, Lightsurf_Init
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/bsputils.cc, WorldToTexSpace, TexSpaceToWorld, faceextents_t::faceextents_t, faceextents_t::LMCoordToWorld, faceextents_t::worldToLMCoord

//! A face's sample points (LIGHT-12): its lightmap's extents, where each sample lies, which
//! face it is really on, its normal, and whether it is hidden in solid.

use qs_bsp::Face;

use crate::extended::keys::{CHANNEL_DEFAULT, Dirt, FULL};
use crate::extended::math::{
    Aabb, POINT_EQUAL_EPSILON, Plane, add, closest_on_boundary, distance, edge_plane,
    fraction_of_line, inside_distance, interpolated_normal, is_inside, length, scale, sub,
};
use crate::extended::phong::Smoothing;
use crate::extended::scene::{LUXEL, Map, Skin};
use crate::{AXES, Sampling, Vec3, to_f32};

/// `sampleOffPlaneDist`: how far in front of its face a sample lies (LIGHT-12.3).
const OFF_PLANE: f32 = 1.0;
/// How far from [`OFF_PLANE`] a point may be and still be placed on a face.
const OFF_PLANE_SLACK: f32 = 0.1;
/// How many faces away from its own a sample may be placed (LIGHT-12.5).
const MAX_HOPS: usize = 3;
/// How far a sample in solid is moved on each axis to find open space (LIGHT-12.7).
const NUDGE: f32 = 0.5;
/// Half, of a box's size and of a luxel.
const HALF: f32 = 0.5;

/// One sample of a face (`sample_data_t`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sample {
    pub(crate) point: Vec3,
    pub(crate) normal: Vec3,
    /// Whether it is hidden: in solid, or off every face.
    pub(crate) occluded: bool,
    /// How much of the sky above it is closed off, 0 to 1 (LIGHT-19).
    pub(crate) occlusion: f32,
}

/// Where a sample was placed (`position_t`).
struct Placed {
    point: Vec3,
    /// Its normal; none for a sample that could not be placed.
    normal: Option<Vec3>,
}

impl Placed {
    const fn hidden(point: Vec3) -> Self {
        Self {
            point,
            normal: None,
        }
    }
}

/// A face's lightmap in the world (`faceextents_t`, LIGHT-12.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Extents {
    /// The luxel the lightmap starts at on each axis.
    mins: [i32; 2],
    /// Its size less one on each axis.
    pub(crate) extents: [i32; 2],
    /// The texture's axes over the plane's normal, each with its offset: world to texture
    /// space.
    to_texture: [[f32; 4]; 3],
    /// Texture space to the world: each row a world axis.
    to_world: [[f32; 4]; 3],
    /// The middle and radius of the sphere around the face.
    pub(crate) origin: Vec3,
    pub(crate) radius: f32,
    pub(crate) bounds: Aabb,
}

impl Extents {
    /// The extents of a face with `points` on `plane`, textured by `vecs`; none when the
    /// texture's axes do not span the face (`Bad texture axes`).
    fn new(points: &[Vec3], plane: Plane, vecs: [[f32; 4]; 2]) -> Option<Self> {
        let [s, t] = vecs;
        let n = plane.normal;
        let to_texture = [s, t, [n[0], n[1], n[2], -plane.dist]];
        let to_world = inverse(to_texture)?;
        let first = points.first().copied()?;
        let mut bounds = Aabb::at(first);
        let mut low = [f64::INFINITY; 2];
        let mut high = [f64::NEG_INFINITY; 2];
        for &point in points {
            bounds = bounds.with(point);
            for ((axis, low), high) in vecs.iter().zip(&mut low).zip(&mut high) {
                // The texture coordinate is worked in `double` and kept as a float.
                let coordinate = f64::from(to_f32(
                    f64::from(axis[0]) * f64::from(point[0])
                        + f64::from(axis[1]) * f64::from(point[1])
                        + f64::from(axis[2]) * f64::from(point[2])
                        + f64::from(axis[3]),
                ));
                *low = low.min(coordinate);
                *high = high.max(coordinate);
            }
        }
        let luxel = f64::from(LUXEL);
        let floor = low.map(|c| (c / luxel).floor());
        let ceil = high.map(|c| (c / luxel).ceil());
        let half = scale(sub(bounds.maxs, bounds.mins), HALF);
        Some(Self {
            mins: floor.map(whole),
            extents: [whole(ceil[0] - floor[0]), whole(ceil[1] - floor[1])],
            to_texture,
            to_world,
            origin: add(bounds.mins, half),
            radius: length(half),
            bounds,
        })
    }

    /// The lightmap's width in luxels.
    pub(crate) fn width(&self) -> usize {
        usize::try_from(self.extents[0].saturating_add(1)).unwrap_or(0)
    }

    /// The lightmap's height in luxels.
    pub(crate) fn height(&self) -> usize {
        usize::try_from(self.extents[1].saturating_add(1)).unwrap_or(0)
    }

    /// `LMCoordToWorld`: the point of the face's plane at lightmap coordinates `s` and `t`.
    fn world_at(&self, s: f32, t: f32) -> Vec3 {
        let texel = [
            LUXEL * (float_of(self.mins[0]) + s),
            LUXEL * (float_of(self.mins[1]) + t),
        ];
        self.to_world
            .map(|row| row[0] * texel[0] + row[1] * texel[1] + row[3])
    }

    /// `worldToLMCoord`: the lightmap coordinates of `point`.
    fn luxel_at(&self, point: Vec3) -> [f32; 2] {
        let texel = |row: [f32; 4]| {
            row[0] * point[0] + row[1] * point[1] + row[2] * point[2] + row[3]
        };
        [
            texel(self.to_texture[0]) / LUXEL - float_of(self.mins[0]),
            texel(self.to_texture[1]) / LUXEL - float_of(self.mins[1]),
        ]
    }
}

/// A whole number held in a `double`, as an `int`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec LIGHT-12.1: ericw-tools casts the floor and ceiling of the extents to int"
)]
const fn whole(value: f64) -> i32 {
    value as i32
}

/// A luxel number as a float.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec LIGHT-12.1: a luxel number is far within a float's whole numbers"
)]
const fn float_of(value: i32) -> f32 {
    value as f32
}

/// The inverse of the map whose rows are `rows`, each three numbers and an offset, worked in
/// `double`; none when the rows do not span space.
#[expect(
    clippy::float_cmp,
    reason = "spec LIGHT-12.1: ericw-tools tests the determinant against exactly 0"
)]
fn inverse(rows: [[f32; 4]; 3]) -> Option<[[f32; 4]; 3]> {
    let m = rows.map(|row| row.map(f64::from));
    let [a, b, c] = m;
    let cofactor = |p: [f64; 4], q: [f64; 4], i: usize, j: usize| -> Option<f64> {
        Some(p.get(i)? * q.get(j)? - p.get(j)? * q.get(i)?)
    };
    // The columns of the inverse of the three by three part are the cross products of its
    // rows, over the determinant.
    let cross = |p: [f64; 4], q: [f64; 4]| -> Option<[f64; 3]> {
        Some([
            cofactor(p, q, 1, 2)?,
            cofactor(p, q, 2, 0)?,
            cofactor(p, q, 0, 1)?,
        ])
    };
    let (bc, ca, ab) = (cross(b, c)?, cross(c, a)?, cross(a, b)?);
    let det = a[0] * bc[0] + a[1] * bc[1] + a[2] * bc[2];
    if det == 0.0 {
        return None;
    }
    let mut out = [[0.0; 4]; 3];
    for (axis, row) in out.iter_mut().enumerate() {
        let (Some(x), Some(y), Some(z)) = (bc.get(axis), ca.get(axis), ab.get(axis)) else {
            continue;
        };
        let (x, y, z) = (x / det, y / det, z / det);
        *row = [
            to_f32(x),
            to_f32(y),
            to_f32(z),
            to_f32(-(x * a[3] + y * b[3] + z * c[3])),
        ];
    }
    Some(out)
}

/// A face readied for lighting (`lightsurf_t`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Patch {
    pub(crate) face: usize,
    pub(crate) model: usize,
    /// The face's plane, moved with its model.
    pub(crate) plane: Plane,
    pub(crate) extents: Extents,
    /// The samples across and down: the lightmap's size times the samples a luxel takes.
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) samples: Vec<Sample>,
    pub(crate) minlight: f32,
    pub(crate) minlight_color: Vec3,
    pub(crate) maxlight: f32,
    pub(crate) lightcolorscale: f32,
    /// Whether the face takes no dirt.
    pub(crate) nodirt: bool,
    /// Whether its normals are smoothed.
    pub(crate) curved: bool,
    /// Whether it is lit from both sides.
    pub(crate) twosided: bool,
    pub(crate) channels: i32,
}

/// The samples a luxel takes on each axis.
const fn per_luxel(sampling: Sampling) -> usize {
    match sampling {
        Sampling::Fast => 1,
        Sampling::Extra => 2,
    }
}

/// A count as a float.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec LIGHT-12.2: a count of samples is far within a float's whole numbers"
)]
const fn count_of(value: usize) -> f32 {
    value as f32
}

impl Patch {
    /// `Lightsurf_Init` and `CalcPoints`: the face numbered `face` readied, with its samples;
    /// none for a face with no lightmap, fewer than three edges, no model or bad texture
    /// axes (LIGHT-12).
    #[expect(
        clippy::float_cmp,
        reason = "spec LIGHT-13.6: a face is curved when its phong angle is not exactly 0"
    )]
    pub(crate) fn new(
        map: &Map<'_>,
        smoothing: &Smoothing,
        face_index: usize,
        sampling: Sampling,
    ) -> Option<Self> {
        let face = map.face(face_index)?;
        let model_index = map.model_of(face_index)?;
        let model = map.models.get(model_index)?;
        if face.numedges < AXES || !map.is_lightmapped(face) {
            return None;
        }
        let flags = map.flags_of(face);
        let liquid = map.skin(face) == Skin::Liquid;
        let world = &map.world;
        let keys = &model.keys;
        let minlight = if keys.minlight.is_changed() {
            keys.minlight.get()
        } else {
            flags.minlight.unwrap_or_else(|| world.minlight.get())
        };
        let minlight_color = if keys.minlight_color.is_changed() {
            keys.minlight_color.get()
        } else if flags.minlight_color == [0; AXES] {
            world.minlight_color.get()
        } else {
            flags.minlight_color.map(f32::from)
        };
        let plane = map.plane(face);
        let texinfo = map.bsp.texinfo.get(face.texinfo)?;
        let shape = smoothing.shapes.get(face_index)?;
        let mut extents = Extents::new(&shape.points, plane, texinfo.vecs)?;
        let curved = flags.phong_angle != 0.0 || flags.phong_group != 0;
        let extra = per_luxel(sampling);
        let (width, height) = (
            extents.width().saturating_mul(extra),
            extents.height().saturating_mul(extra),
        );
        let step = 1.0 / count_of(extra);
        let start = -HALF + HALF / count_of(extra);
        let phong = curved && world.phong.get();
        let placer = Placer {
            map,
            smoothing,
            extents: &extents,
            offset: model.offset,
            phong,
        };
        let mut samples = Vec::with_capacity(width.saturating_mul(height));
        for t in 0..height {
            for s in 0..width {
                let on_plane = extents.world_at(
                    start + count_of(s) * step,
                    start + count_of(t) * step,
                );
                let placed = placer.place(face_index, add(on_plane, plane.normal), 0);
                samples.push(Sample {
                    point: add(placed.point, model.offset),
                    normal: placed.normal.unwrap_or_default(),
                    occluded: placed.normal.is_none(),
                    occlusion: 0.0,
                });
            }
        }
        // The plane and the sphere follow the model only now: the points were found without.
        let moved = add(scale(plane.normal, plane.dist), model.offset);
        extents.origin = add(extents.origin, model.offset);
        extents.bounds = extents.bounds.moved(model.offset);
        Some(Self {
            face: face_index,
            model: model_index,
            plane: Plane {
                normal: plane.normal,
                dist: crate::extended::math::dot(plane.normal, moved),
            },
            extents,
            width,
            height,
            samples,
            minlight,
            minlight_color,
            maxlight: if keys.maxlight.is_changed() {
                keys.maxlight.get()
            } else {
                flags.maxlight
            },
            lightcolorscale: if keys.lightcolorscale.is_changed() {
                keys.lightcolorscale.get()
            } else {
                flags.lightcolorscale
            },
            // Lit liquids never take dirt.
            nodirt: liquid
                || if keys.dirt.is_changed() {
                    Dirt::of(keys.dirt.get()) == Dirt::Off
                } else {
                    flags.no_dirt
                },
            curved,
            // A liquid with a lightmap, or glass, is lit from both sides.
            twosided: keys.alpha.get() < 1.0 || flags.light_twosided.unwrap_or(liquid),
            channels: flags
                .object_channel_mask
                .unwrap_or_else(|| keys.object_channel_mask.get()),
        })
    }

    /// The minimum light's colour over 255, for a test of a face's settings.
    #[cfg(test)]
    pub(crate) fn minlight_fraction(&self) -> Vec3 {
        scale(self.minlight_color, 1.0 / FULL)
    }

    /// Whether the face is on the default channel only.
    pub(crate) const fn is_on_default_channel(&self) -> bool {
        self.channels & CHANNEL_DEFAULT != 0
    }
}

/// What placing a face's samples needs.
struct Placer<'a> {
    map: &'a Map<'a>,
    smoothing: &'a Smoothing,
    /// The extents of the face the samples are for.
    extents: &'a Extents,
    /// Its model's offset.
    offset: Vec3,
    /// Whether normals are interpolated.
    phong: bool,
}

impl Placer<'_> {
    /// `CalcPointNormal`: where the sample wanted at `wanted` goes, starting from `face`,
    /// `hops` faces from the one it is for (LIGHT-12.4 to LIGHT-12.6).
    fn place(&self, face: usize, wanted: Vec3, hops: usize) -> Placed {
        let Some(shape) = self.smoothing.shapes.get(face) else {
            return Placed::hidden(wanted);
        };
        let Some(plane) = shape.plane else {
            return Placed::hidden(wanted);
        };
        if shape.points.is_empty() || shape.edge_planes.is_empty() {
            return Placed::hidden(wanted);
        }
        let lift = scale(plane.normal, OFF_PLANE);
        let point = add(plane.projected(wanted), lift);
        if is_inside(&shape.edge_planes, point) {
            return self.put(face, point);
        }
        // Of the edges the point is behind, the one it is least past the ends of.
        let mut best: Option<(usize, f32)> = None;
        let corners = &shape.points;
        for (edge, (&v0, &v1)) in corners
            .iter()
            .zip(corners.iter().cycle().skip(1))
            .enumerate()
        {
            let Some(edge_plane) = edge_plane(v0, v1, plane.normal) else {
                continue;
            };
            if edge_plane.distance_to(point) >= POINT_EQUAL_EPSILON {
                continue;
            }
            let along = fraction_of_line(v0, v1, point);
            let span = distance(v0, v1);
            let past = if along < 0.0 {
                along.abs() * span
            } else if along > 1.0 {
                along * span
            } else {
                0.0
            };
            if best.is_none_or(|(_, least)| past < least) {
                best = Some((edge, past));
            }
        }
        if let Some((edge, _)) = best
            && hops < MAX_HOPS
            && let Some(next) = self.smoothing.across(self.map, face, edge)
        {
            return self.place(next, point, hops.saturating_add(1));
        }
        // Within a luxel of the face, the sample snaps to its edge.
        let closest = closest_on_boundary(corners, point);
        let [s0, t0] = self.extents.luxel_at(point);
        let [s1, t1] = self.extents.luxel_at(closest);
        if (s0 - s1).hypot(t0 - t1) <= 1.0 {
            return self.put(face, add(closest, lift));
        }
        Placed::hidden(point)
    }

    /// `PositionSamplePointOnFace`: the sample at `point` of `face`, with its normal; moved
    /// half a unit out of solid when it is in it, hidden when it cannot be (LIGHT-12.7).
    fn put(&self, face: usize, point: Vec3) -> Placed {
        let hidden = Placed::hidden(point);
        let (Some(shape), Some(model)) =
            (self.smoothing.shapes.get(face), self.map.model_of(face))
        else {
            return hidden;
        };
        let Some(plane) = shape.plane else {
            return hidden;
        };
        if shape.edge_planes.is_empty()
            || (plane.distance_to(point) - OFF_PLANE).abs() > OFF_PLANE_SLACK
            || inside_distance(&shape.edge_planes, point) < -POINT_EQUAL_EPSILON
        {
            return hidden;
        }
        let normal = if self.phong {
            match interpolated_normal(&shape.points, &shape.normals, point) {
                Some(normal) => normal,
                None => return hidden,
            }
        } else {
            plane.normal
        };
        if !self.map.in_any_solid(model, add(point, self.offset)) {
            return Placed {
                point,
                normal: Some(normal),
            };
        }
        for x in [-NUDGE, NUDGE] {
            for y in [-NUDGE, NUDGE] {
                for z in [-NUDGE, NUDGE] {
                    let nudged = add(point, [x, y, z]);
                    if !self.map.in_any_solid(model, add(nudged, self.offset)) {
                        return Placed {
                            point: nudged,
                            normal: Some(normal),
                        };
                    }
                }
            }
        }
        hidden
    }
}
