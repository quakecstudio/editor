// SPDX-License-Identifier: GPL-3.0-or-later

//! What the map view draws of each light (P7-09, LIGHT-7): where its light ends, its colour and,
//! for a spotlight, its cone, read from its keys as [`Light::run`] reads them, so the preview is
//! the light the compile gives.

use crate::entities::{Entities, EntityId, FULL};
use crate::face::{DEFAULT_CONE, float_of};
use crate::{AXES, Light, LightError};

/// A light as the map view draws it (LIGHT-7).
#[derive(Clone, Debug, PartialEq)]
pub struct LightPreview {
    /// Its place in the entity lump, the map's entity it is.
    pub entity: EntityId,
    /// Its origin.
    pub origin: [f32; 3],
    /// How far its light reaches: a face farther lights nothing (LIGHT-7.1).
    pub radius: f32,
    /// Its colour, each channel 0 to 1, white for none (LIGHT-7.2).
    pub colour: [f32; 3],
    /// Its cone, for a spotlight (LIGHT-7.3).
    pub cone: Option<Cone>,
}

/// A spotlight's cone (LIGHT-7.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cone {
    /// Its aim, a unit direction from the light to its target.
    pub aim: [f32; 3],
    /// The whole cone's angle, in degrees.
    pub degrees: f32,
}

impl Light {
    /// The lights of the entity lump `lump`, as the map view draws them (LIGHT-7).
    ///
    /// # Errors
    ///
    /// The [`LightError`] the lump stops `light` with (LIGHT-2).
    pub fn previews(&self, lump: &[u8]) -> Result<Vec<LightPreview>, LightError> {
        let entities = Entities::read(lump, self.dialect, &mut Vec::new())?;
        let mut out = Vec::new();
        for (place, light) in entities.all.iter().enumerate() {
            if !light.is_light() {
                continue;
            }
            let cone = light
                .target_entity
                .and_then(|target| entities.all.get(target.index()))
                .map(|target| {
                    let mut aim = [0.0; AXES];
                    for (a, (t, o)) in aim.iter_mut().zip(target.origin.iter().zip(light.origin)) {
                        *a = t - o;
                    }
                    let length = aim.iter().map(|c| c * c).sum::<f32>().sqrt();
                    if length > 0.0 {
                        aim = aim.map(|c| c / length);
                    }
                    let degrees = if light.angle == 0.0 {
                        crate::to_f32(DEFAULT_CONE)
                    } else {
                        light.angle
                    };
                    Cone { aim, degrees }
                });
            out.push(LightPreview {
                entity: EntityId(place),
                origin: light.origin,
                radius: float_of(light.light) / self.scale.dist,
                colour: light.colour.map_or([1.0; AXES], |c| c.map(|v| v / FULL)),
                cone,
            });
        }
        Ok(out)
    }
}
