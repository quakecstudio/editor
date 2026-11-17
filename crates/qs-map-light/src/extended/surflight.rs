// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2016 Eric Wasylishen
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/surflight.cc, MakeSurfaceLight, IsSurfaceLitFace, MakeSurfaceLightsThread, MakeRadiositySurfaceLights
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/entities.cc, FaceMatchesSurfaceLightTemplate
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/common/qvec.hh, TriangleArea, PolyArea

//! Radiosity surface lights (LIGHT-16.4): a face of a `_surface` light with
//! `_surface_radiosity` 1 gives light as a whole, from a unit above its middle, by its area,
//! as a face gives back the light it took (LIGHT-20), and not as lights put over it.

use super::bounce::{Emission, Emitter, Stage, without_colinear};
use super::lights::{Lamp, Lights, style_for};
use super::math::{add, cross, length, scale, sub};
use super::rays::Rays;
use super::scene::Map;
use crate::face::FaceId;
use crate::{AXES, LightError, Vec3};

/// `SURFLIGHT_Q1` and `SURFLIGHT_RAD`: the two ways a `_surface` light lights.
pub(crate) const BY_LIGHTS: i32 = 0;
const BY_RADIOSITY: i32 = 1;
/// The least a face's area for it to be a surface light.
const LEAST_AREA: f32 = 1.0;
/// Full colour on a channel.
const FULL: f32 = 255.0;
/// Half, of a triangle's cross product.
const HALF: f32 = 0.5;
/// The texture whose faces give no light, `surf_is_emissive`.
const TRIGGER: &[u8] = b"trigger";

/// A face that is a radiosity surface light, as its minimum light reads it
/// (`IsSurfaceLitFace`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SurfaceLit {
    /// The light's brightness.
    pub(crate) light: f32,
    pub(crate) style: i32,
    /// The light's colour, or its texture's mean.
    pub(crate) colour: Vec3,
    /// The light's `_surflight_minlight_scale`.
    pub(crate) minlight_scale: f32,
}

/// Each face's emitter as a radiosity surface light, and what its minimum light reads of
/// it.
pub(crate) type Radiosity = (Vec<Option<Emitter>>, Vec<Option<SurfaceLit>>);

/// `PolyArea`: the triangles fanned from the first point, in single precision.
fn poly_area(points: &[Vec3]) -> f32 {
    let Some(&first) = points.first() else {
        return 0.0;
    };
    let mut total = 0.0_f32;
    for pair in points.windows(2).skip(1) {
        if let [a, b] = *pair {
            total += HALF * length(cross(sub(b, first), sub(a, first)));
        }
    }
    total
}

impl Lights {
    /// `FaceMatchesSurfaceLightTemplate`'s way: `_surface_radiosity` of the light, else the
    /// world's `_surflight_radiosity`.
    pub(crate) fn radiosity_of(&self, lamp: &Lamp) -> i32 {
        match self.dicts.get(lamp.entity.index()) {
            Some(dict) if !dict.get(b"_surface_radiosity").is_empty() => {
                dict.int(b"_surface_radiosity")
            }
            _ => self.world.surflight_radiosity.get(),
        }
    }

    /// The radiosity templates face `face_index` matches, in order: those of its texture's
    /// name, in any case, and its surface light group.
    fn radiosity_templates(&self, map: &Map<'_>, face_index: usize) -> Vec<&Lamp> {
        let Some(face) = map.face(FaceId::new(face_index)) else {
            return Vec::new();
        };
        let name = map.texture_name(face);
        let group = map.flags_of(face).surflight_group;
        self.templates
            .iter()
            .filter(|(lamp, texture)| {
                self.radiosity_of(lamp) == BY_RADIOSITY
                    && lamp.keys.surflight_group.get() == group
                    && name.eq_ignore_ascii_case(texture)
            })
            .map(|(lamp, _)| lamp)
            .collect()
    }

    /// `MakeRadiositySurfaceLights`: each face's emitter as a radiosity surface light, and
    /// what its minimum light reads of it; none for a face that is not one (LIGHT-16.4).
    ///
    /// # Errors
    ///
    /// [`LightError::TooManyStyles`] when a face's `surflight_targetname` is one name too
    /// many.
    pub(crate) fn radiosity(
        &mut self,
        map: &Map<'_>,
        rays: &Rays,
    ) -> Result<Radiosity, LightError> {
        let count = map.bsp.faces.len();
        let mut emitters: Vec<Option<Emitter>> = vec![None; count];
        let mut lit: Vec<Option<SurfaceLit>> = vec![None; count];
        if self.templates.is_empty() {
            return Ok((emitters, lit));
        }
        let mut stack = Vec::new();
        for face_index in 0..count {
            let (Some(face), Some(model)) =
                (map.face(FaceId::new(face_index)), map.model_of(face_index))
            else {
                continue;
            };
            let matching: Vec<Lamp> = self
                .radiosity_templates(map, face_index)
                .into_iter()
                .cloned()
                .collect();
            let Some(first) = matching.first() else {
                continue;
            };
            let flags = map.flags_of(face);
            let mean = map.average_colour(face);
            if let Some(slot) = lit.get_mut(face_index) {
                *slot = Some(SurfaceLit {
                    light: first.keys.light.get(),
                    style: first.keys.style.get(),
                    colour: if first.keys.color.is_changed() {
                        first.keys.color.get()
                    } else {
                        mean
                    },
                    minlight_scale: first.keys.surflight_minlight_scale.get(),
                });
            }
            // `Face_IsEmissive`, and a face too small to give light.
            if map.texture_name(face).eq_ignore_ascii_case(TRIGGER) {
                continue;
            }
            let points = map.points(face);
            let face_area = poly_area(&points);
            if face_area < LEAST_AREA {
                continue;
            }
            for template in &matching {
                let dict = self.dicts.get(template.entity.index());
                let key = |name: &[u8]| dict.map_or(0, |dict| dict.int(name));
                let has = |name: &[u8]| dict.is_some_and(|dict| !dict.get(name).is_empty());
                let directional = !has(b"_surface_spotlight") || key(b"_surface_spotlight") != 0;
                let sky = key(b"_surface_is_sky") != 0;
                let own_style = key(b"style");
                // The colour: the face's own, else the light's, else the world's
                // `_sky_surface` for a sky, else the texture's mean.
                let mut colour = match flags.surflight_color {
                    Some(colour) => colour.map(f32::from),
                    None if template.keys.color.is_changed() => template.keys.color.get(),
                    None if sky && self.world.sky_surface.is_changed() => {
                        scale(self.world.sky_surface.get(), FULL)
                    }
                    None => mean,
                };
                colour = scale(scale(colour, 1.0 / FULL), template.keys.light.get());
                let intensity = colour[0].max(colour[1]).max(colour[2]);
                if intensity == 0.0 {
                    continue;
                }
                if intensity > 1.0 {
                    colour = scale(colour, 1.0 / intensity);
                }
                let Some(slot) = emitters.get_mut(face_index) else {
                    continue;
                };
                if slot.is_none() {
                    let offset = map.models.get(model).map_or([0.0; AXES], |m| m.offset);
                    let moved: Vec<Vec3> = points.iter().map(|p| add(*p, offset)).collect();
                    let corners = without_colinear(&moved);
                    let normal = map.plane(face).normal;
                    let mut middle = [0.0; AXES];
                    for &corner in &corners {
                        middle = add(middle, corner);
                    }
                    let corner_count = u16::try_from(corners.len()).map_or(1.0, f32::from);
                    let pos = add(scale(middle, 1.0 / corner_count), normal);
                    *slot = Some(Emitter {
                        pos,
                        normal,
                        // Culled as a light is: by what it sees with no visibility in the
                        // BSP, else by the leaves the world lists its face in.
                        bounds: map
                            .visibility
                            .is_none()
                            .then(|| rays.visible_bounds(map, pos, &mut stack)),
                        leaves: map
                            .visibility
                            .as_ref()
                            .map_or_else(Vec::new, |seen| seen.leaves_of(face_index).to_vec()),
                        emissions: Vec::new(),
                    });
                }
                let style = match (&flags.surflight_targetname, flags.surflight_style) {
                    (Some(name), _) => style_for(&mut self.styles, &self.world, name)?,
                    (None, Some(style)) => style,
                    (None, None) => own_style,
                };
                if let Some(emitter) = slot {
                    emitter.emissions.push(Emission {
                        stage: Stage::Direct,
                        style,
                        intensity: intensity * face_area,
                        colour,
                        omnidirectional: !directional,
                        rescale: flags.surflight_rescale.unwrap_or(!sky),
                        atten: flags.surflight_atten.unwrap_or(1.0),
                    });
                }
            }
        }
        Ok((emitters, lit))
    }
}
