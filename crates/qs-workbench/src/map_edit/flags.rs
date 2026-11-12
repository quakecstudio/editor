// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.set_flag` (P5-13, MAP-27): sets or clears one spawnflag on an entity and on others with
//! it in one step, each entity's other flags kept, as the spawnflag checkboxes ask.

use qs_commands::{Command, Outcome};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{EntityIndex, MapDocument, MapEdited, edit_map};
use crate::{HasWorkbench, WorkbenchError};

/// The id of `map.set_flag`.
pub const MAP_SET_FLAG: &str = "map.set_flag";

/// The key holding an entity's spawnflags (EDICT-8.4).
const SPAWNFLAGS: &[u8] = b"spawnflags";

/// What `map.set_flag` does to the flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FlagChange {
    /// Sets it.
    Set,
    /// Clears it.
    Clear,
}

/// The parameters of `map.set_flag`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetFlagParams {
    /// The entity.
    pub entity: EntityIndex,
    /// Other entities to change too, in the same step.
    #[serde(default)]
    pub also: Vec<EntityIndex>,
    /// The flag, its value.
    pub flag: u32,
    /// Whether to set or clear it.
    pub change: FlagChange,
}

impl MapDocument {
    /// Sets or clears `flag` in `entity`'s spawnflags, read as the whole number before any
    /// point and 0 when they are not a number.
    fn change_flag(
        &mut self,
        entity: EntityIndex,
        flag: u32,
        change: FlagChange,
    ) -> Result<(), WorkbenchError> {
        let found = self.entity_mut(entity)?;
        let bits: u32 = found
            .value(SPAWNFLAGS)
            .and_then(|value| core::str::from_utf8(value).ok())
            .and_then(|value| value.trim().split('.').next())
            .and_then(|whole| whole.parse().ok())
            .unwrap_or(0);
        let bits = match change {
            FlagChange::Set => bits | flag,
            FlagChange::Clear => bits & !flag,
        };
        let set = found.set(SPAWNFLAGS, bits.to_string().as_bytes());
        set.map_err(|error| self.map_error(error))
    }
}

/// `map.set_flag`.
pub(super) struct MapSetFlag;

impl<C: HasWorkbench + 'static> Command<C> for MapSetFlag {
    const ID: &'static str = MAP_SET_FLAG;
    const SUMMARY: &'static str = "Sets or clears one spawnflag of an entity of the open map, \
        and of the entities in also, in one step, keeping each entity's other flags.";
    type Params = SetFlagParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: SetFlagParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = edit_map(context, |document| {
            for entity in core::iter::once(params.entity).chain(params.also.iter().copied()) {
                document.change_flag(entity, params.flag, params.change)?;
            }
            Ok(())
        })?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(params.entity),
            },
            step,
        ))
    }
}
