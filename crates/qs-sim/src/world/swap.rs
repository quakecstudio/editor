// SPDX-License-Identifier: GPL-3.0-or-later

//! A new build of the running map's BSP put in place of the old (BRIDGE-4.14 to BRIDGE-4.16):
//! the world's collision, PVS and brush model bounds made again from it, every edict, field and
//! global kept, and each of the map's brush entities given its model in the new build.

use qc_builtins::mock::Precache;
use qc_progs::Progs;
use qc_types::Slot;
use qc_vm::RunError;
use qs_bsp::{Bsp, Submodel};

use super::{World, field_slot};
use crate::builtins::SimCall;
use crate::world::MapPlace;

/// Why a swap was refused; nothing changed (BRIDGE-4.14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwapError {
    /// The model is past the new BSP's last brush model.
    NoSuchModel(Submodel),
    /// The map's model precache list has no such model: the client read the list when it
    /// joined, so a brush entity added since waits for the map to load again.
    NotPrecached(Submodel),
    /// An edict in use shows a brush model the swap does not place: it is not one of the map's
    /// entities, and its `*n` may be another brush in the new build.
    Unplaced {
        /// The edict.
        edict: usize,
        /// The brush model it shows.
        model: Submodel,
    },
    /// An edict could not be linked again.
    Link(RunError),
}

impl core::fmt::Display for SwapError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoSuchModel(model) => {
                write!(f, "the new map has no brush model {}", model.name())
            }
            Self::NotPrecached(model) => write!(
                f,
                "{} is not precached by the map; reload the map to load it",
                model.name()
            ),
            Self::Unplaced { edict, model } => write!(
                f,
                "edict {edict} shows {} and is not one of the map's entities; reload the map",
                model.name()
            ),
            Self::Link(error) => write!(f, "linking: {}", error.message()),
        }
    }
}

impl core::error::Error for SwapError {}

/// What a swap places: each edict `models` gives a brush model, with the model and its index
/// in the precache list, and every edict in use but the world, which it links again.
struct Placed {
    given: Vec<(usize, Submodel, usize)>,
    in_use: Vec<usize>,
}

impl World {
    /// Puts `bsp`, a new build of the running map, in place of the old (BRIDGE-4.15), each
    /// entity of `models` that is in an edict given its brush model (BRIDGE-4.16), then links
    /// every edict in use again; the number of edicts given a model.
    ///
    /// # Errors
    ///
    /// [`SwapError::NoSuchModel`] or [`SwapError::NotPrecached`] for a model the swap cannot
    /// give, and [`SwapError::Unplaced`] for an edict showing a brush model it does not place,
    /// before anything changes (BRIDGE-4.14); [`SwapError::Link`] when linking fails.
    pub fn swap_bsp(
        &mut self,
        progs: &Progs<'_>,
        bsp: &Bsp<'_>,
        models: &[(MapPlace, Submodel)],
    ) -> Result<usize, SwapError> {
        let Placed { given, in_use } = self.placed(progs, bsp, models)?;
        self.builtins.swap_map(&self.map, bsp);
        for &(edict, model, index) in &given {
            self.give_model(edict, model, index, bsp);
        }
        let mut call = SimCall::frame(&mut self.machine, progs, &mut self.builtins);
        // Linking without touching runs no QuakeC (WORLD-6), so it does not fail part-way.
        in_use
            .into_iter()
            .try_for_each(|n| call.link(n, false))
            .map_err(SwapError::Link)?;
        Ok(given.len())
    }

    /// Whether `bsp` and `models` would swap in, as [`World::swap_bsp`] checks them, with
    /// nothing changed: a host checks a swap before it draws the new build (BRIDGE-4.17).
    ///
    /// # Errors
    ///
    /// As [`World::swap_bsp`]'s refusals.
    pub fn check_swap(
        &self,
        progs: &Progs<'_>,
        bsp: &Bsp<'_>,
        models: &[(MapPlace, Submodel)],
    ) -> Result<(), SwapError> {
        self.placed(progs, bsp, models).map(|_| ())
    }

    /// Each edict `models` places, with its brush model and model index, and the edicts in use
    /// but the world; refused before anything changes (BRIDGE-4.14).
    fn placed(
        &self,
        progs: &Progs<'_>,
        bsp: &Bsp<'_>,
        models: &[(MapPlace, Submodel)],
    ) -> Result<Placed, SwapError> {
        let last = bsp.models.len().saturating_sub(1);
        let mut given = Vec::with_capacity(models.len());
        for &(place, model) in models {
            if model.get() > last {
                return Err(SwapError::NoSuchModel(model));
            }
            let index = self
                .builtins
                .mock
                .engine()
                .precache_index(Precache::Model, &model.name())
                .ok_or(SwapError::NotPrecached(model))?;
            if let Some(edict) = self.map_entity(progs, place) {
                given.push((edict, model, index));
            }
        }
        let in_use: Vec<usize> = (1..self.num_edicts()).filter(|&n| self.in_use(n)).collect();
        for &edict in &in_use {
            if given.iter().any(|&(placed, _, _)| placed == edict) {
                continue;
            }
            if let Some(model) = self.brush_model_of(edict) {
                return Err(SwapError::Unplaced { edict, model });
            }
        }
        Ok(Placed { given, in_use })
    }

    /// The brush model edict `edict` shows by its `modelindex`, when the precache list names a
    /// `*n` there.
    fn brush_model_of(&self, edict: usize) -> Option<Submodel> {
        let index = super::whole(self.field(edict, "modelindex").as_f32())?;
        let engine = self.builtins.mock.engine();
        Submodel::parse(engine.model_precache.get(index)?)
    }

    /// Edict `edict` given brush model `model` of `bsp`, model index `index`, as `setmodel`
    /// gives it: its `model`, `modelindex`, `mins`, `maxs` and `size` (BRIDGE-4.16).
    fn give_model(&mut self, edict: usize, model: Submodel, index: usize, bsp: &Bsp<'_>) {
        let name = self.make_string(model.name().into_bytes());
        self.set_field(edict, field_slot("model"), Slot::from_i32(name));
        let index = u16::try_from(index).map_or(f32::MAX, f32::from);
        self.set_field(edict, field_slot("modelindex"), Slot::from_f32(index));
        let Some(bounds) = bsp.models.get(model.get()) else {
            return;
        };
        let [x, y, z] = bounds.maxs;
        let [lx, ly, lz] = bounds.mins;
        let size = [x - lx, y - ly, z - lz];
        for (field, vector) in [("mins", bounds.mins), ("maxs", bounds.maxs), ("size", size)] {
            let first = field_slot(field);
            for (i, value) in vector.into_iter().enumerate() {
                self.set_field(edict, first.saturating_add(i), Slot::from_f32(value));
            }
        }
    }
}

#[cfg(test)]
mod tests;
