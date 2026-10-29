// SPDX-License-Identifier: GPL-3.0-or-later

//! World collision, id's `world.c` (`docs/spec/engine/world.md`): the hulls of brush models
//! and boxes, the contents of a point, a line through a hull, the area nodes entities are
//! linked in, triggers touched when an entity moves, and the trace of a box moved through the
//! world.
//!
//! The server owns the entities; this module reads them through [`Host`], which also runs a
//! trigger's touch, so a touch that moves or removes entities changes the area as id's does.
//!
//! Implements WORLD-1 to WORLD-7.

mod area;
mod hull;
mod trace;

use qc_vm::RunError;
use qs_bsp::Bsp;

pub use area::{
    AREA_DEPTH, Area, FL_ITEM, FL_MONSTER, LeafTree, MAX_ENT_LEAFS, MoveKind, box_on_plane_side,
    link_edict, sv_move, test_entity_position,
};
pub use hull::{
    BoxHull, BrushModel, CONTENTS_CURRENT_0, CONTENTS_CURRENT_DOWN, CONTENTS_EMPTY, CONTENTS_SOLID,
    CONTENTS_WATER, ClipNode, EntityHull, HULL1, HULL2, Hull, HullSet, Plane, hull_for_entity,
};
pub use trace::{DIST_EPSILON, Trace, TracePlane, clip_move_to_entity, recursive_hull_check};

/// Why a query of the world failed.
#[derive(Debug)]
pub enum CollisionError {
    /// A walk reached a clipnode outside its hull, id's `Sys_Error` with this message
    /// (WORLD-2.1, WORLD-3.2).
    BadNode(&'static str),
    /// `Sys_Error ("SOLID_BSP without MOVETYPE_PUSH")` (WORLD-1.4).
    SolidBspWithoutPush,
    /// `Sys_Error ("MOVETYPE_PUSH with a non bsp model")` (WORLD-1.4).
    PushWithoutBsp,
    /// `Sys_Error ("Trigger in clipping list")` (WORLD-7.2).
    TriggerInClipList,
    /// A trigger's touch failed.
    Touch(Box<RunError>),
}

impl core::fmt::Display for CollisionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadNode(message) => f.write_str(message),
            Self::SolidBspWithoutPush => f.write_str("SOLID_BSP without MOVETYPE_PUSH"),
            Self::PushWithoutBsp => f.write_str("MOVETYPE_PUSH with a non bsp model"),
            Self::TriggerInClipList => f.write_str("Trigger in clipping list"),
            Self::Touch(error) => write!(f, "a touch failed: {}", error.message()),
        }
    }
}

impl core::error::Error for CollisionError {}

/// An entity's `solid`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Solid {
    /// `SOLID_NOT` (0): nothing touches or blocks it.
    Not,
    /// `SOLID_TRIGGER` (1): touched, never blocking.
    Trigger,
    /// `SOLID_BBOX` (2).
    BBox,
    /// `SOLID_SLIDEBOX` (3).
    SlideBox,
    /// `SOLID_BSP` (4): clipped by its brush model's hulls.
    Bsp,
    /// Any other value, which id compares equal to none of them.
    Other(f32),
}

impl Solid {
    /// The `solid` a progs stored.
    #[must_use]
    pub fn from_float(value: f32) -> Self {
        match value.to_bits() {
            b if b == 0.0_f32.to_bits() || b == (-0.0_f32).to_bits() => Self::Not,
            b if b == 1.0_f32.to_bits() => Self::Trigger,
            b if b == 2.0_f32.to_bits() => Self::BBox,
            b if b == 3.0_f32.to_bits() => Self::SlideBox,
            b if b == 4.0_f32.to_bits() => Self::Bsp,
            _ => Self::Other(value),
        }
    }
}

/// What the world reads of an entity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    /// Whether the edict is free.
    pub free: bool,
    /// `solid`.
    pub solid: Solid,
    /// Whether `movetype` is `MOVETYPE_PUSH` (WORLD-1.4).
    pub pusher: bool,
    /// `modelindex`.
    pub modelindex: f32,
    /// `origin`.
    pub origin: [f32; 3],
    /// `mins`.
    pub mins: [f32; 3],
    /// `maxs`.
    pub maxs: [f32; 3],
    /// `absmin`.
    pub absmin: [f32; 3],
    /// `absmax`.
    pub absmax: [f32; 3],
    /// `size`.
    pub size: [f32; 3],
    /// `(int)flags`.
    pub flags: i32,
    /// The edict `owner` names, id's `PROG_TO_EDICT`.
    pub owner: Option<usize>,
    /// `touch`, a function number.
    pub touch: i32,
}

/// The server the world queries: its entities, and the touch of a trigger.
pub trait Host {
    /// The world's collision state.
    fn collision(&self) -> &Collision;
    /// The world's collision state, to link and unlink entities.
    fn collision_mut(&mut self) -> &mut Collision;
    /// Entity `n`, or `None` past the last edict.
    fn body(&self, n: usize) -> Option<Body>;
    /// Sets entity `n`'s `absmin` and `absmax` (WORLD-6.2).
    fn set_abs(&mut self, n: usize, absmin: [f32; 3], absmax: [f32; 3]);
    /// Runs trigger `trigger`'s touch with `other` as `other`, saving and restoring `self` and
    /// `other` and setting `time` to the server's (WORLD-6.6).
    ///
    /// # Errors
    ///
    /// [`CollisionError::Touch`] when the touch fails.
    fn touch(&mut self, trigger: usize, other: usize) -> Result<(), CollisionError>;
}

/// The world's collision state: the hulls of every brush model, the area nodes and the leaves
/// each entity is in.
#[derive(Clone, Debug, PartialEq)]
pub struct Collision {
    sets: Vec<HullSet>,
    /// Each model index's brush model, if it is one.
    models: Vec<Option<BrushModel>>,
    area: Area,
    leafs: LeafTree,
    /// Each entity's leaves (WORLD-6.3).
    entity_leafs: Vec<Vec<i32>>,
}

impl Collision {
    /// `SV_ClearWorld` for the map `bsp`: its world model is model index 1 and its submodel
    /// `*n` index `1 + n`, as the server's model list has them (WORLD-5.1).
    #[must_use]
    pub fn new(bsp: &Bsp<'_>) -> Self {
        let (mins, maxs) = bsp
            .models
            .first()
            .map_or(([0.0; 3], [0.0; 3]), |m| (m.mins, m.maxs));
        let mut collision = Self {
            sets: vec![HullSet::new(bsp)],
            models: vec![None],
            area: Area::new(mins, maxs),
            leafs: LeafTree::new(bsp),
            entity_leafs: Vec::new(),
        };
        for model in &bsp.models {
            collision.models.push(Some(brush(0, model)));
        }
        collision
    }

    /// Makes model index `index` model 0 of `bsp`, such as an item's `maps/b_bh25.bsp`.
    pub fn add_brush(&mut self, index: usize, bsp: &Bsp<'_>) {
        let set = self.sets.len();
        self.sets.push(HullSet::new(bsp));
        if self.models.len() <= index {
            self.models.resize(index.saturating_add(1), None);
        }
        if let (Some(slot), Some(model)) = (self.models.get_mut(index), bsp.models.first()) {
            *slot = Some(brush(set, model));
        }
    }

    /// The brush model of model index `modelindex`, id's `sv.models[(int)modelindex]`.
    #[must_use]
    pub fn model(&self, modelindex: f32) -> Option<&BrushModel> {
        let index = whole(modelindex)?;
        self.models.get(index)?.as_ref()
    }

    /// The hull sets brush models name.
    #[must_use]
    pub fn sets(&self) -> &[HullSet] {
        &self.sets
    }

    /// The area nodes.
    #[must_use]
    pub const fn area(&self) -> &Area {
        &self.area
    }

    /// The leaves entity `n` is in (WORLD-6.3).
    #[must_use]
    pub fn leafs_of(&self, n: usize) -> &[i32] {
        self.entity_leafs.get(n).map_or(&[], Vec::as_slice)
    }

    /// `SV_UnlinkEdict` (WORLD-6.8).
    pub fn unlink(&mut self, n: usize) {
        self.area.unlink(n);
    }

    /// `SV_PointContents`: the contents of `p` in the world's hull 0, a current read as water
    /// (WORLD-2.2).
    ///
    /// # Errors
    ///
    /// The errors of [`Hull::point_contents`].
    pub fn point_contents(&self, p: [f32; 3]) -> Result<i32, CollisionError> {
        let contents = self.true_point_contents(p)?;
        Ok(
            if (CONTENTS_CURRENT_DOWN..=CONTENTS_CURRENT_0).contains(&contents) {
                CONTENTS_WATER
            } else {
                contents
            },
        )
    }

    /// `SV_TruePointContents`: the contents of `p` in the world's hull 0 (WORLD-2.2).
    ///
    /// # Errors
    ///
    /// The errors of [`Hull::point_contents`].
    pub fn true_point_contents(&self, p: [f32; 3]) -> Result<i32, CollisionError> {
        let set = self.sets.first().ok_or(CollisionError::PushWithoutBsp)?;
        let world = self
            .models
            .get(1)
            .copied()
            .flatten()
            .map_or([0; 3], |m| m.heads);
        let hull = set.hull(0, world);
        hull.point_contents(0, p)
    }

    fn set_leafs(&mut self, n: usize, leafs: Vec<i32>) {
        if self.entity_leafs.len() <= n {
            self.entity_leafs.resize(n.saturating_add(1), Vec::new());
        }
        if let Some(slot) = self.entity_leafs.get_mut(n) {
            *slot = leafs;
        }
    }
}

fn brush(set: usize, model: &qs_bsp::Model) -> BrushModel {
    let [h0, h1, h2, _] = model.headnode;
    BrushModel {
        set,
        heads: [h0, h1, h2],
        mins: model.mins,
        maxs: model.maxs,
    }
}

/// `(int)value` as an index; `None` for a negative or NaN value.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "spec WORLD-1.4: id indexes sv.models by (int)modelindex, which truncates"
)]
fn whole(value: f32) -> Option<usize> {
    (value >= 0.0).then_some(value as usize)
}

#[cfg(test)]
mod tests;
