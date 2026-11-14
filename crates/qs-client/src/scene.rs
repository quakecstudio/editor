// SPDX-License-Identifier: GPL-3.0-or-later

//! What the client hands the renderer each frame: the entities to draw with their
//! interpolation, the dynamic lights, the particles, the eye and the weapon, and the blend
//! over the view.

use crate::client::ModelIndex;
use crate::view::Refdef;

/// How an alias model's poses blend this frame (CLIENT-7.3).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PoseBlend {
    /// The pose blended from.
    pub previous: usize,
    /// The pose blended to.
    pub current: usize,
    /// How far from `previous` to `current`, 0 to 1.
    pub blend: f32,
}

/// An entity's slot, `cl_entities`' index; 0 for a temporary or a static entity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EntitySlot(usize);

impl EntitySlot {
    /// The slot of a temporary or a static entity.
    pub const NONE: Self = Self(0);

    /// Slot `index`.
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// The slot's index.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }

    /// The player's index, 0 for slot 1, when the slot is a client's of `maxclients`.
    #[must_use]
    pub fn player(self, maxclients: usize) -> Option<usize> {
        (1..=maxclients)
            .contains(&self.0)
            .then(|| self.0.saturating_sub(1))
    }
}

/// One entity to draw.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SceneEntity {
    /// Its slot.
    pub entity: EntitySlot,
    /// Its model precache index.
    pub model: ModelIndex,
    /// Where it is drawn, interpolated (CLIENT-5.4, CLIENT-7.4).
    pub origin: [f32; 3],
    /// Its angles, interpolated.
    pub angles: [f32; 3],
    /// Its frame, for a sprite or a brush model; an alias model draws `poses`.
    pub frame: i32,
    /// The client slot whose colours it wears, 0 for none (CLIENT-1.6).
    pub colormap: i32,
    /// Its skin.
    pub skin: i32,
    /// `syncbase` for a sprite's or an alias skin's animation (CLIENT-2.4).
    pub syncbase: f32,
    /// The poses an alias model blends (CLIENT-7.3); `None` for another model.
    pub poses: Option<PoseBlend>,
}

/// A dynamic light, `dlight_t` (CLIENT-5.7).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DynamicLight {
    /// Its origin.
    pub origin: [f32; 3],
    /// Its radius.
    pub radius: f32,
    /// `minlight`: the light below which it adds nothing (REND-7.2).
    pub minlight: f32,
}

/// A particle to draw (PART-3.2).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SceneParticle {
    /// Its origin.
    pub origin: [f32; 3],
    /// Its palette colour.
    pub color: u8,
}

/// A frame's scene.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    /// The client's clock.
    pub time: f32,
    /// `cl.maxclients`: entities 1 to this are players (REND-4.11).
    pub maxclients: usize,
    /// The entities in the visible list, the temporary entities last (CLIENT-5.6, CLIENT-6.4).
    pub entities: Vec<SceneEntity>,
    /// The static entities (CLIENT-1.3).
    pub statics: Vec<SceneEntity>,
    /// The eye and the weapon (VIEW-3).
    pub view: Refdef,
    /// The weapon's pose blend, when it has a model.
    pub weapon_poses: Option<PoseBlend>,
    /// The blend over the view, RGBA (VIEW-4.6).
    pub blend: [f32; 4],
    /// The live dynamic lights (CLIENT-5.7).
    pub lights: Vec<DynamicLight>,
    /// The live particles (PART-3).
    pub particles: Vec<SceneParticle>,
    /// Each light style's string (CLIENT-1.4, REND-5), empty when unset.
    pub lightstyles: Vec<Vec<u8>>,
    /// Each client slot's colours, shirt high and trousers low, `cl.scores[i].colors`
    /// (CLIENT-1.6), which its player's skin is drawn in (REND-4.13).
    pub colors: Vec<i32>,
}
