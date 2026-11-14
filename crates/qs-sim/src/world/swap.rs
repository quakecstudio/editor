// SPDX-License-Identifier: GPL-3.0-or-later

//! A new build of the running map's BSP put in place of the old (BRIDGE-4.14 to BRIDGE-4.16):
//! the world's collision, PVS and brush model bounds made again from it, every edict, field and
//! global kept, and each of the map's brush entities given its model in the new build.

use qc_builtins::mock::Precache;
use qc_progs::Progs;
use qc_types::Slot;
use qs_bsp::Bsp;

use super::{World, field_slot};
use crate::builtins::SimCall;
use crate::world::MapPlace;

/// A brush model of a map, `*n` with `n` from 1 (WORLD-5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BrushModel(usize);

impl BrushModel {
    /// The brush model `name` names, `*n` with `n` a whole number from 1 written with no
    /// leading zero; `None` for another name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        let digits = name.strip_prefix('*')?;
        if digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.parse().ok().filter(|&n| n >= 1).map(Self)
    }

    /// Its name, `*n`.
    #[must_use]
    pub fn name(self) -> String {
        format!("*{}", self.0)
    }

    /// Its place in a BSP's models, the world's being 0.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// Why a swap was refused; nothing changed (BRIDGE-4.14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwapError {
    /// The model is past the new BSP's last brush model.
    NoSuchModel(BrushModel),
    /// The map's model precache list has no such model: the client read the list when it
    /// joined, so a brush entity added since waits for the map to load again.
    NotPrecached(BrushModel),
    /// An edict could not be linked again.
    Link(String),
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
            Self::Link(why) => write!(f, "linking: {why}"),
        }
    }
}

impl core::error::Error for SwapError {}

impl World {
    /// Puts `bsp`, a new build of the running map, in place of the old (BRIDGE-4.15), each
    /// entity of `models` that is in an edict given its brush model (BRIDGE-4.16), then links
    /// every edict in use again; the number of edicts given a model.
    ///
    /// # Errors
    ///
    /// [`SwapError::NoSuchModel`] or [`SwapError::NotPrecached`] for a model the swap cannot
    /// give, before anything changes (BRIDGE-4.14); [`SwapError::Link`] when linking fails.
    pub fn swap_bsp(
        &mut self,
        progs: &Progs<'_>,
        bsp: &Bsp<'_>,
        models: &[(MapPlace, BrushModel)],
    ) -> Result<usize, SwapError> {
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
        self.builtins.swap_map(&self.map, bsp);
        for &(edict, model, index) in &given {
            self.give_model(edict, model, index, bsp);
        }
        let linked: Vec<usize> = (1..self.num_edicts()).filter(|&n| self.in_use(n)).collect();
        let mut call = SimCall::frame(&mut self.machine, progs, &mut self.builtins);
        for n in linked {
            call.link(n, false)
                .map_err(|e| SwapError::Link(e.to_string()))?;
        }
        Ok(given.len())
    }

    /// Edict `edict` given brush model `model` of `bsp`, model index `index`, as `setmodel`
    /// gives it: its `model`, `modelindex`, `mins`, `maxs` and `size` (BRIDGE-4.16).
    fn give_model(&mut self, edict: usize, model: BrushModel, index: usize, bsp: &Bsp<'_>) {
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
