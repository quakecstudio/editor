// SPDX-License-Identifier: GPL-3.0-or-later

//! Links between entities (P5-13, MAP-27): `map.link` points a key the code fires entities by
//! (MAP-8.8) at another entity, naming that entity when it has no `targetname`; and the
//! numbering a duplicate takes, so a copy of `door1` is the next door and its targets the next
//! targets, without typing either.

use qs_commands::{Command, Outcome};
use qs_map::{Entity, MapFile};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{EntityIndex, MapDocument, MapEdited, edit_map};
use crate::{HasWorkbench, WorkbenchError};

/// The id of `map.link`.
pub const MAP_LINK: &str = "map.link";

/// The key an entity is fired by, which every targeting key names (MAP-8.8).
pub(super) const TARGETNAME: &[u8] = b"targetname";

/// The parameters of `map.link`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinkParams {
    /// The entity whose key to set.
    pub entity: EntityIndex,
    /// The key, one the code fires entities by: `target`, `killtarget` and the like.
    pub key: String,
    /// The entity it names; given a `targetname` when it has none.
    pub target: EntityIndex,
}

/// A value whose end is a number: the text before it and the number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Numbered<'a> {
    stem: &'a [u8],
    number: u64,
}

impl<'a> Numbered<'a> {
    /// `value` split at its trailing digits, when it has some.
    fn of(value: &'a [u8]) -> Option<Self> {
        let digits = value
            .iter()
            .rev()
            .take_while(|b| b.is_ascii_digit())
            .count();
        let (stem, number) = value.split_at(value.len().checked_sub(digits)?);
        let number = core::str::from_utf8(number).ok()?.parse().ok()?;
        Some(Self { stem, number })
    }

    /// The value with `number` in place of its own.
    fn with(self, number: u64) -> Vec<u8> {
        let mut value = self.stem.to_vec();
        value.extend_from_slice(number.to_string().as_bytes());
        value
    }
}

impl MapDocument {
    /// The next number after every `targetname` of `file` made of `stem` and a number, so
    /// `stem` with it names no entity yet.
    fn next_number(file: &MapFile, stem: &[u8]) -> u64 {
        file.entities
            .iter()
            .filter_map(|entity| entity.value(TARGETNAME).and_then(Numbered::of))
            .filter(|numbered| numbered.stem == stem)
            .map(|numbered| numbered.number)
            .max()
            .map_or(1, |last| last.saturating_add(1))
    }

    /// Numbers the copy `copy` of an entity onward: a `targetname` ending in a number takes
    /// the next number free for its stem, and each key in `targeting` whose value ends in a
    /// number moves on by as much, so a duplicated link in a chain names the next one.
    pub(super) fn number_onward(
        &self,
        copy: &mut Entity,
        targeting: &[String],
    ) -> Result<(), WorkbenchError> {
        let Some(name) = copy.value(TARGETNAME).and_then(Numbered::of) else {
            return Ok(());
        };
        let next = Self::next_number(&self.file, name.stem);
        let Some(step) = next.checked_sub(name.number) else {
            return Ok(());
        };
        let mut pairs = vec![(TARGETNAME.to_vec(), name.with(next))];
        for key in targeting {
            let moved = copy
                .value(key.as_bytes())
                .and_then(Numbered::of)
                .and_then(|value| Some(value.with(value.number.checked_add(step)?)));
            if let Some(value) = moved {
                pairs.push((key.as_bytes().to_vec(), value));
            }
        }
        for (key, value) in pairs {
            copy.set(&key, &value)
                .map_err(|error| self.map_error(error))?;
        }
        Ok(())
    }

    /// Sets `key` of `entity` to the `targetname` of `target`, giving `target` its classname
    /// and the next free number first when it has none.
    fn link(
        &mut self,
        entity: EntityIndex,
        key: &str,
        target: EntityIndex,
    ) -> Result<(), WorkbenchError> {
        self.entity_mut(entity)?;
        let found = self.entity_mut(target)?;
        let name = match found.value(TARGETNAME).filter(|name| !name.is_empty()) {
            Some(name) => name.to_vec(),
            None => {
                let stem = found.classname().unwrap_or_default().to_vec();
                let mut name = stem.clone();
                let next = Self::next_number(&self.file, &stem);
                name.extend_from_slice(next.to_string().as_bytes());
                let set = self.entity_mut(target)?.set(TARGETNAME, &name);
                set.map_err(|error| self.map_error(error))?;
                name
            }
        };
        let set = self.entity_mut(entity)?.set(key.as_bytes(), &name);
        set.map_err(|error| self.map_error(error))
    }
}

/// `map.link`.
pub(super) struct MapLink;

impl<C: HasWorkbench + 'static> Command<C> for MapLink {
    const ID: &'static str = MAP_LINK;
    const SUMMARY: &'static str = "Points a key of an entity of the open map, one the code \
        fires entities by such as target or killtarget, at another entity: the key takes the \
        other's targetname, and the other its classname and the next free number as one when \
        it has none.";
    type Params = LinkParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: LinkParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = edit_map(context, |document| {
            document.link(params.entity, &params.key, params.target)
        })?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(params.entity),
            },
            step,
        ))
    }
}

#[cfg(test)]
mod tests;
