// SPDX-License-Identifier: GPL-3.0-or-later

//! What the bridge's editing commands change (BRIDGE-4.1, BRIDGE-4.4): one field written with
//! a typed value, an entity spawned or edited as the map spawns them (EDICT-8), freed, or the
//! player moved.

use qc_progs::Progs;
use qc_types::{DefType, Etype, Slot};

use super::{CLIENT, Spawner, World};
use crate::builtins::SimCall;
use crate::collision::MoveKind;
use crate::spawn::{SpawnHost, apply_key};
use crate::world::QcValue;

/// Why an edit was not made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    /// The edict is not in use (BRIDGE-4.5).
    NoSuchEdict(usize),
    /// The progs has no field of that name.
    NoSuchField(String),
    /// A value of another type than the field's, or naming nothing (BRIDGE-4.4).
    BadValue(String),
    /// The world or the player, which are never freed.
    Kept(usize),
    /// Spawning failed: no classname, or one naming no function, or its function failed.
    Spawn(String),
}

impl core::fmt::Display for EditError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoSuchEdict(n) => write!(f, "edict {n} is not in use"),
            Self::NoSuchField(name) => write!(f, "no field {name}"),
            Self::BadValue(why) => f.write_str(why),
            Self::Kept(n) => write!(f, "edict {n} is never freed"),
            Self::Spawn(why) => write!(f, "spawning: {why}"),
        }
    }
}

impl core::error::Error for EditError {}

impl World {
    /// Writes field `name` of edict `n` with `value`, which must be of the field's type
    /// (BRIDGE-4.4).
    ///
    /// # Errors
    ///
    /// [`EditError`] for an edict not in use, a field the progs does not have, or a value of
    /// another type or naming nothing.
    pub fn set_field_value(
        &mut self,
        progs: &Progs<'_>,
        n: usize,
        name: &str,
        value: &QcValue,
    ) -> Result<(), EditError> {
        let first = self.check_field_value(progs, n, name, value)?;
        let slots = self.slots_of(progs, value)?;
        for (i, slot) in slots.into_iter().enumerate() {
            self.set_field(n, first.saturating_add(i), slot);
        }
        Ok(())
    }

    /// Whether [`World::set_field_value`] would write field `name` of edict `n` with `value`,
    /// writing nothing, for an edit held until the game resumes (BRIDGE-4.4); the field's
    /// first slot.
    ///
    /// # Errors
    ///
    /// The [`EditError`] the write would fail with.
    pub fn check_field_value(
        &self,
        progs: &Progs<'_>,
        n: usize,
        name: &str,
        value: &QcValue,
    ) -> Result<usize, EditError> {
        if !self.in_use(n) {
            return Err(EditError::NoSuchEdict(n));
        }
        let def = progs
            .ed_find_field(name.as_bytes())
            .ok_or_else(|| EditError::NoSuchField(name.to_owned()))?;
        let DefType::Known(etype) = def.def_type() else {
            return Err(EditError::BadValue(format!(
                "{name} has a type no value has"
            )));
        };
        if !matches_type(etype, value) {
            return Err(EditError::BadValue(format!("{name} is not of that type")));
        }
        // A string is made only when written; every other value resolves here.
        if !matches!(value, QcValue::String(_)) {
            self.resolve(progs, value)?;
        }
        Ok(usize::try_from(def.ofs).unwrap_or(usize::MAX))
    }

    /// The slots `value` is stored in, a string made for a string.
    fn slots_of(&mut self, progs: &Progs<'_>, value: &QcValue) -> Result<Vec<Slot>, EditError> {
        match value {
            QcValue::String(text) => Ok(vec![Slot::from_i32(self.make_string(text.clone()))]),
            other => self.resolve(progs, other),
        }
    }

    /// The slots a value other than a string is stored in.
    fn resolve(&self, progs: &Progs<'_>, value: &QcValue) -> Result<Vec<Slot>, EditError> {
        let bad = |what: &str| EditError::BadValue(format!("no {what} of that name or number"));
        Ok(match value {
            QcValue::Float(f) => vec![Slot::from_f32(*f)],
            QcValue::Vector(v) => v.map(Slot::from_f32).to_vec(),
            QcValue::Void | QcValue::String(_) => Vec::new(),
            QcValue::Entity(n) => {
                let number = i32::try_from(*n).map_err(|_| bad("edict"))?;
                let entity = self
                    .machine
                    .edicts()
                    .edict_num(number)
                    .map_err(|_| bad("edict"))?;
                vec![Slot::from_i32(entity.bits())]
            }
            QcValue::Field(name) => {
                let def = progs
                    .ed_find_field(name.as_bytes())
                    .ok_or_else(|| bad("field"))?;
                vec![Slot::from_i32(
                    i32::try_from(def.ofs).map_err(|_| bad("field"))?,
                )]
            }
            QcValue::Function(name) => {
                let target = progs
                    .ed_find_function(name.as_bytes())
                    .ok_or_else(|| bad("function"))?;
                let index = progs
                    .functions()
                    .iter()
                    .position(|f| f == target)
                    .and_then(|i| i32::try_from(i).ok())
                    .ok_or_else(|| bad("function"))?;
                vec![Slot::from_i32(index)]
            }
        })
    }

    /// Spawns an entity of the key-values `pairs` as a map entity is spawned (EDICT-8): an
    /// edict allocated, the pairs applied, then the function its `classname` names run with
    /// `self` the edict; returns its number.
    ///
    /// # Errors
    ///
    /// [`EditError::Spawn`] for no free edict, a pair that does not parse, no `classname` or
    /// one naming no function (the edict is then freed), or a spawn function that fails.
    pub fn spawn_entity(
        &mut self,
        progs: &Progs<'_>,
        pairs: &[(String, String)],
    ) -> Result<usize, EditError> {
        let mut spawner = Spawner { world: self, progs };
        let n = spawner
            .allocate(false)
            .map_err(|e| EditError::Spawn(e.to_string()))?;
        for (key, value) in pairs {
            if let Err(error) = apply_key(&mut spawner, n, key.as_bytes(), value.as_bytes()) {
                spawner.free(n);
                return Err(EditError::Spawn(error.to_string()));
            }
        }
        let classname = pairs
            .iter()
            .find(|(key, _)| key == "classname")
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        let Some(function) = spawner.find_function(classname.as_bytes()) else {
            spawner.free(n);
            return Err(EditError::Spawn(format!("no spawn function {classname:?}")));
        };
        spawner
            .spawn(n, function)
            .map_err(|e| EditError::Spawn(e.to_string()))?;
        Ok(n)
    }

    /// Applies the key-values `pairs` to edict `n` as its spawn would parse them (EDICT-8).
    ///
    /// # Errors
    ///
    /// [`EditError`] for an edict not in use or a pair that does not parse.
    pub fn edit_entity(
        &mut self,
        progs: &Progs<'_>,
        n: usize,
        pairs: &[(String, String)],
    ) -> Result<(), EditError> {
        if !self.in_use(n) {
            return Err(EditError::NoSuchEdict(n));
        }
        let mut spawner = Spawner { world: self, progs };
        for (key, value) in pairs {
            apply_key(&mut spawner, n, key.as_bytes(), value.as_bytes())
                .map_err(|e| EditError::BadValue(e.to_string()))?;
        }
        Ok(())
    }

    /// Frees edict `n`, `ED_Free`, which unlinks it; the world and the player are kept.
    ///
    /// # Errors
    ///
    /// [`EditError`] for an edict not in use, the world or the player.
    pub fn remove_entity(&mut self, progs: &Progs<'_>, n: usize) -> Result<(), EditError> {
        if !self.in_use(n) {
            return Err(EditError::NoSuchEdict(n));
        }
        if n == 0 || n == CLIENT {
            return Err(EditError::Kept(n));
        }
        Spawner { world: self, progs }.free(n);
        Ok(())
    }

    /// Moves the player to `origin`, facing `angles`, as a teleporter does: `setorigin`, the
    /// angles with `fixangle` so the client takes them, and no velocity.
    ///
    /// # Errors
    ///
    /// [`EditError::NoSuchEdict`] before the player has entered the game, or what linking
    /// it fails with.
    pub fn move_player(
        &mut self,
        progs: &Progs<'_>,
        origin: [f32; 3],
        angles: [f32; 3],
    ) -> Result<(), EditError> {
        if self.player.is_none() || !self.in_use(CLIENT) {
            return Err(EditError::NoSuchEdict(CLIENT));
        }
        let mut call = SimCall::frame(&mut self.machine, progs, &mut self.builtins);
        call.set_vector(CLIENT, "origin", origin);
        call.set_vector(CLIENT, "angles", angles);
        call.set_vector(CLIENT, "velocity", [0.0; 3]);
        call.set_float(CLIENT, "fixangle", 1.0);
        call.link(CLIENT, false)
            .map_err(|e| EditError::Spawn(e.to_string()))
    }
}

/// What a line traced through the world hits, for the bridge's `pick` (BRIDGE-4.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineHit {
    /// The edict hit, `None` for the world or nothing.
    pub edict: Option<usize>,
    /// Where the line stopped, `None` when it met nothing.
    pub point: Option<[f32; 3]>,
}

impl World {
    /// `traceline (start, end, FALSE, player)`: the line from `start` to `end` through the
    /// world and its entities, the player passed through (WORLD-8.4).
    ///
    /// # Errors
    ///
    /// [`WorldError::Frame`] when the trace stops the engine (`Sys_Error`).
    pub fn trace_line(
        &mut self,
        progs: &Progs<'_>,
        start: [f32; 3],
        end: [f32; 3],
    ) -> Result<LineHit, super::WorldError> {
        let call = SimCall::frame(&mut self.machine, progs, &mut self.builtins);
        let trace = call
            .trace((start, [0.0; 3], [0.0; 3], end), MoveKind::Normal, CLIENT)
            .map_err(super::WorldError::Frame)?;
        Ok(LineHit {
            edict: trace.ent.filter(|&n| n != 0),
            point: (trace.fraction < 1.0).then_some(trace.endpos),
        })
    }
}

/// Whether `value` is of the type `etype` holds.
pub(crate) fn matches_type(etype: Etype, value: &QcValue) -> bool {
    matches!(
        (etype, value),
        (Etype::Float, QcValue::Float(_))
            | (Etype::Vector, QcValue::Vector(_))
            | (Etype::String, QcValue::String(_))
            | (Etype::Entity, QcValue::Entity(_))
            | (Etype::Field, QcValue::Field(_))
            | (Etype::Function, QcValue::Function(_))
            | (Etype::Void | Etype::Pointer, QcValue::Void)
    )
}

#[cfg(test)]
mod tests;
