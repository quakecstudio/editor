// SPDX-License-Identifier: GPL-3.0-or-later

//! TrenchBroom's linked groups and layer settings (MAP-9.5 to MAP-9.9), described from
//! TrenchBroom's `NodeSerializer`, `MapReader` and `LinkedGroupUtils` (`updateLinkedGroups`,
//! `preserveEntityProperties`), not ported: a change to one linked group is copied into the
//! others, transformed, keeping their protected keys; and a layer can be hidden, locked or
//! left out of an exported map.

use crate::cnum::{c_atof, scan_vector};
use crate::rooms::{RoomKind, container_id};
use crate::{Entity, Item, MapError, MapFile};

/// The key holding a group's link id (MAP-9.5).
const LINK_KEY: &[u8] = b"_tb_linked_group_id";
/// The key holding a linked group's transformation (MAP-9.5).
const TRANSFORMATION_KEY: &[u8] = b"_tb_transformation";
/// The key holding an entity's protected keys (MAP-9.8).
const PROTECTED_KEY: &[u8] = b"_tb_protected_properties";
/// What separates protected keys (MAP-9.8).
const PROTECTED_SEPARATOR: u8 = b';';
/// The key naming the group an entity is in (MAP-9.2).
const GROUP_KEY: &[u8] = b"_tb_group";
/// The key naming the layer an entity is in (MAP-9.2).
const LAYER_KEY: &[u8] = b"_tb_layer";
/// A layer setting's value when set (MAP-9.9).
const SET: &[u8] = b"1";
/// The rows and columns of a transformation.
const SIZE: usize = 4;
/// The column of a transformation that holds its translation.
const TRANSLATION_COLUMN: usize = 3;
/// The numbers of a transformation.
const NUMBERS: usize = 16;
/// Copied coordinates are rounded to this many parts of a unit, so a quarter turn's `cos`
/// writes 0 rather than `6.1e-17`.
const PRECISION: f64 = 1e6;
/// Degrees in half a turn.
const HALF_TURN: f64 = 180.0;
/// `angle`'s values for straight up and down, which turning leaves (`SetMovedir`).
const UP: f64 = -1.0;
const DOWN: f64 = -2.0;

/// A group's transformation, a 4 by 4 matrix by rows (MAP-9.5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    rows: [[f64; SIZE]; SIZE],
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// The identity, a group's transformation when it has none.
    pub const IDENTITY: Self = Self {
        rows: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// A move by `by`.
    #[must_use]
    pub const fn translation(by: [f64; 3]) -> Self {
        let mut rows = Self::IDENTITY.rows;
        rows[0][TRANSLATION_COLUMN] = by[0];
        rows[1][TRANSLATION_COLUMN] = by[1];
        rows[2][TRANSLATION_COLUMN] = by[2];
        Self { rows }
    }

    /// The sixteen numbers of `text`, row by row (MAP-9.5); `None` for any other text.
    #[must_use]
    pub fn parse(text: &[u8]) -> Option<Self> {
        let numbers: Vec<f64> = core::str::from_utf8(text)
            .ok()?
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        let flat: [f64; NUMBERS] = numbers.try_into().ok()?;
        let mut rows = [[0.0; SIZE]; SIZE];
        let (chunks, _) = flat.as_chunks::<SIZE>();
        for (row, chunk) in rows.iter_mut().zip(chunks) {
            *row = *chunk;
        }
        Some(Self { rows })
    }

    /// The text TrenchBroom writes, row by row (MAP-9.5).
    #[must_use]
    pub fn text(&self) -> String {
        self.rows
            .iter()
            .flatten()
            .map(|n| number(*n))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// `self` after `first`: the matrix product `self * first`.
    #[must_use]
    pub fn after(&self, first: &Self) -> Self {
        let mut rows = [[0.0; SIZE]; SIZE];
        for (row, mine) in rows.iter_mut().zip(&self.rows) {
            for (column, out) in row.iter_mut().enumerate() {
                *out = mine
                    .iter()
                    .zip(&first.rows)
                    .map(|(a, other)| a * other.get(column).copied().unwrap_or_default())
                    .sum();
            }
        }
        Self { rows }
    }

    /// The inverse, by Gauss-Jordan elimination with partial pivoting; `None` when there is
    /// none (MAP-9.7).
    #[must_use]
    pub fn inverse(&self) -> Option<Self> {
        let mut left = self.rows;
        let mut right = Self::IDENTITY.rows;
        for column in 0..SIZE {
            let pivot = (column..SIZE).max_by(|&a, &b| {
                let size = |row: usize| left.get(row).and_then(|r| r.get(column)).map(|v| v.abs());
                size(a)
                    .unwrap_or_default()
                    .total_cmp(&size(b).unwrap_or_default())
            })?;
            left.swap(column, pivot);
            right.swap(column, pivot);
            let lead = *left.get(column)?.get(column)?;
            if lead.abs() < f64::EPSILON {
                return None;
            }
            for value in left.get_mut(column)?.iter_mut() {
                *value /= lead;
            }
            for value in right.get_mut(column)?.iter_mut() {
                *value /= lead;
            }
            let (pivot_left, pivot_right) = (*left.get(column)?, *right.get(column)?);
            for row in (0..SIZE).filter(|&row| row != column) {
                let factor = *left.get(row)?.get(column)?;
                for (value, p) in left.get_mut(row)?.iter_mut().zip(pivot_left) {
                    *value -= factor * p;
                }
                for (value, p) in right.get_mut(row)?.iter_mut().zip(pivot_right) {
                    *value -= factor * p;
                }
            }
        }
        Some(Self { rows: right })
    }

    /// Where `point` goes.
    #[must_use]
    pub fn apply(&self, point: [f64; 3]) -> [f64; 3] {
        let [x, y, z] = point;
        let row = |r: &[f64; SIZE]| round(r[0] * x + r[1] * y + r[2] * z + r[TRANSLATION_COLUMN]);
        [row(&self.rows[0]), row(&self.rows[1]), row(&self.rows[2])]
    }

    /// Its turn about the vertical, in degrees.
    #[must_use]
    pub fn yaw(&self) -> f64 {
        self.rows[1][0].atan2(self.rows[0][0]).to_degrees()
    }
}

/// A coordinate rounded to [`PRECISION`], with -0 written as 0.
fn round(value: f64) -> f64 {
    (value * PRECISION).round() / PRECISION + 0.0
}

/// A number as a map writes it: whole numbers with no point.
fn number(value: f64) -> String {
    format!("{}", value + 0.0)
}

/// An entity's place in a map, from 0, the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityPlace(usize);

impl EntityPlace {
    /// The entity at `place`.
    #[must_use]
    pub const fn new(place: usize) -> Self {
        Self(place)
    }

    /// The place.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

impl core::fmt::Display for EntityPlace {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A layer's or group's `_tb_id` (MAP-9.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContainerId(u64);

impl ContainerId {
    /// The container whose `_tb_id` is `id`.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// The id.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A linked group (MAP-9.5).
#[derive(Clone, Debug, PartialEq)]
pub struct LinkedGroup {
    /// Its own entity's place in the map.
    pub entity: EntityPlace,
    /// Its `_tb_id`.
    pub id: ContainerId,
    /// Its `_tb_linked_group_id`.
    pub link: Vec<u8>,
    /// Its `_tb_transformation`, the identity when it has none or one that does not read.
    pub transform: Transform,
}

/// Where each entity of a map went after [`MapFile::update_linked`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Moves {
    to: Vec<Option<EntityPlace>>,
}

impl Moves {
    /// Where the entity that was at `place` is now; `None` when it is gone.
    #[must_use]
    pub fn of(&self, place: EntityPlace) -> Option<EntityPlace> {
        self.to.get(place.get()).copied().flatten()
    }
}

/// A layer's settings (MAP-9.9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerSettings {
    /// `_tb_layer_hidden`.
    pub hidden: bool,
    /// `_tb_layer_locked`.
    pub locked: bool,
    /// `_tb_layer_omit_from_export`.
    pub omit_from_export: bool,
}

/// A layer setting (MAP-9.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerSetting {
    /// Hidden in the editor.
    Hidden,
    /// Locked against edits.
    Locked,
    /// Left out of an exported map.
    OmitFromExport,
}

impl LayerSetting {
    /// The key holding it.
    const fn key(self) -> &'static [u8] {
        match self {
            Self::Hidden => b"_tb_layer_hidden",
            Self::Locked => b"_tb_layer_locked",
            Self::OmitFromExport => b"_tb_layer_omit_from_export",
        }
    }
}

/// Why a linked group could not be updated (MAP-9.7).
#[derive(Debug)]
pub enum LinkError {
    /// The entity at this place is no linked group.
    NotLinked(EntityPlace),
    /// The changed group's transformation has no inverse.
    NotInvertible(EntityPlace),
    /// A linked group holds a group, which is not copied here.
    Nested(EntityPlace),
    /// A copy could not be written.
    Map(MapError),
}

impl core::fmt::Display for LinkError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotLinked(entity) => write!(f, "entity {entity} is no linked group"),
            Self::NotInvertible(entity) => {
                write!(f, "group {entity}'s transformation is not invertible")
            }
            Self::Nested(entity) => write!(
                f,
                "linked group {entity} holds a group; its copies are not updated"
            ),
            Self::Map(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for LinkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Map(error) => Some(error),
            Self::NotLinked(_) | Self::NotInvertible(_) | Self::Nested(_) => None,
        }
    }
}

impl From<MapError> for LinkError {
    fn from(error: MapError) -> Self {
        Self::Map(error)
    }
}

impl Entity {
    /// The linked group this entity is, as read at `place` (MAP-9.5).
    fn linked_group(&self, place: EntityPlace) -> Option<LinkedGroup> {
        let (kind, id) = self.own_container()?;
        let link = self.value(LINK_KEY).filter(|link| !link.is_empty())?;
        (kind == RoomKind::Group).then(|| LinkedGroup {
            entity: place,
            id: ContainerId::new(id),
            link: link.to_vec(),
            transform: self.transform(),
        })
    }

    /// The layer or group this entity is, its kind and id (MAP-9.1).
    #[must_use]
    pub fn container(&self) -> Option<(RoomKind, ContainerId)> {
        self.own_container()
            .map(|(kind, id)| (kind, ContainerId::new(id)))
    }

    /// The layer this entity names in `_tb_layer` (MAP-9.2).
    #[must_use]
    pub fn layer(&self) -> Option<ContainerId> {
        self.value(LAYER_KEY)
            .and_then(container_id)
            .map(ContainerId::new)
    }

    /// Its `_tb_transformation`, the identity when it has none or one that does not read
    /// (MAP-9.5).
    #[must_use]
    pub fn transform(&self) -> Transform {
        self.value(TRANSFORMATION_KEY)
            .and_then(Transform::parse)
            .unwrap_or_default()
    }

    /// Writes `transform` as its `_tb_transformation`, or removes it for the identity, as
    /// TrenchBroom writes it (MAP-9.5).
    ///
    /// # Errors
    ///
    /// As [`Entity::set`].
    pub fn set_transform(&mut self, transform: &Transform) -> Result<(), MapError> {
        if *transform == Transform::IDENTITY {
            self.remove(TRANSFORMATION_KEY);
            return Ok(());
        }
        self.set(TRANSFORMATION_KEY, transform.text().as_bytes())
    }

    /// The group this entity names in `_tb_group` (MAP-9.2).
    fn group(&self) -> Option<ContainerId> {
        self.value(GROUP_KEY)
            .and_then(container_id)
            .map(ContainerId::new)
    }

    /// Its protected keys (MAP-9.8).
    fn protected(&self) -> Vec<Vec<u8>> {
        self.value(PROTECTED_KEY)
            .map(|list| {
                list.split(|&b| b == PROTECTED_SEPARATOR)
                    .filter(|key| !key.is_empty())
                    .map(<[u8]>::to_vec)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Its settings, for a layer's own entity (MAP-9.9).
    #[must_use]
    pub fn layer_settings(&self) -> LayerSettings {
        let set = |setting: LayerSetting| self.value(setting.key()) == Some(SET);
        LayerSettings {
            hidden: set(LayerSetting::Hidden),
            locked: set(LayerSetting::Locked),
            omit_from_export: set(LayerSetting::OmitFromExport),
        }
    }

    /// Sets or clears a layer setting, written as TrenchBroom writes it (MAP-9.9).
    ///
    /// # Errors
    ///
    /// As [`Entity::set`].
    pub fn set_layer_setting(
        &mut self,
        setting: LayerSetting,
        on: LayerSwitch,
    ) -> Result<(), MapError> {
        match on {
            LayerSwitch::On => self.set(setting.key(), SET),
            LayerSwitch::Off => {
                self.remove(setting.key());
                Ok(())
            }
        }
    }

    /// Keeps in this entity, a copy of `source` replacing `replaced`, the value `replaced` had
    /// for every key protected in either, none when it had none, and takes `replaced`'s list
    /// (MAP-9.8).
    fn keep_protected(&mut self, source: &Self, replaced: &Self) -> Result<(), MapError> {
        let mut keys = source.protected();
        keys.extend(replaced.protected());
        keys.sort();
        keys.dedup();
        if keys.is_empty() {
            return Ok(());
        }
        for key in &keys {
            self.remove(key);
            if let Some(value) = replaced.value(key) {
                self.set(key, value)?;
            }
        }
        self.remove(PROTECTED_KEY);
        if let Some(list) = replaced.value(PROTECTED_KEY) {
            self.set(PROTECTED_KEY, list)?;
        }
        Ok(())
    }

    /// This entity moved and turned by `transform` (MAP-9.7): its `origin` when it has one,
    /// every brush's points, and its `angle` or the yaw of its `angles`.
    fn transformed(&self, transform: &Transform) -> Result<Self, MapError> {
        let mut copy = self.clone();
        for item in &mut copy.items {
            if let Item::Brush(brush) = item {
                for face in &mut brush.faces {
                    let points = face.points().map(|p| transform.apply(p));
                    let texture = face.texture().to_vec();
                    let texdef = *face.texdef();
                    face.set(points, &texture, texdef)?;
                }
            }
        }
        if self.value(b"origin").is_some() {
            let origin = transform.apply(self.origin()).map(number).join(" ");
            copy.set(b"origin", origin.as_bytes())?;
        }
        let turn = transform.yaw();
        if turn.abs() > f64::EPSILON {
            if let Some(angles) = self.value(b"angles") {
                let [pitch, yaw, roll] = scan_vector(angles);
                let text = [pitch, turned(yaw, turn), roll].map(number).join(" ");
                copy.set(b"angles", text.as_bytes())?;
            } else if let Some(angle) = self.value(b"angle") {
                let angle = c_atof(angle);
                if ![UP, DOWN].contains(&angle) {
                    copy.set(b"angle", number(turned(angle, turn)).as_bytes())?;
                }
            }
        }
        Ok(copy)
    }
}

/// Whether a layer setting is set or cleared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerSwitch {
    /// Set it.
    On,
    /// Clear it.
    Off,
}

/// `yaw` turned by `turn` degrees, kept from 0 to below 360, rounded.
fn turned(yaw: f64, turn: f64) -> f64 {
    round((yaw + turn).rem_euclid(HALF_TURN * 2.0))
}

impl MapFile {
    /// Every linked group, in file order (MAP-9.5).
    #[must_use]
    pub fn linked_groups(&self) -> Vec<LinkedGroup> {
        self.entities
            .iter()
            .enumerate()
            .filter_map(|(place, entity)| entity.linked_group(EntityPlace::new(place)))
            .collect()
    }

    /// The places of the entities in the group whose id is `id`, in file order (MAP-9.6).
    #[must_use]
    pub fn group_members(&self, id: ContainerId) -> Vec<EntityPlace> {
        self.entities
            .iter()
            .enumerate()
            .filter(|(_, entity)| entity.group() == Some(id))
            .map(|(place, _)| EntityPlace::new(place))
            .collect()
    }

    /// The first linked group whose contents differ from what they were in `before`, the
    /// one a change was made in (MAP-9.6, MAP-9.7).
    #[must_use]
    pub fn changed_linked_group(&self, before: &Self) -> Option<EntityPlace> {
        self.linked_groups()
            .into_iter()
            .find(|group| self.group_contents(group.id) != before.group_contents(group.id))
            .map(|group| group.entity)
    }

    /// A group's contents: its own entity's brushes, then the entities in it (MAP-9.6).
    fn group_contents(&self, id: ContainerId) -> (Vec<&Item>, Vec<&Entity>) {
        let brushes = self
            .entities
            .iter()
            .find(|entity| entity.container() == Some((RoomKind::Group, id)))
            .map(|entity| {
                entity
                    .items
                    .iter()
                    .filter(|item| matches!(item, Item::Brush(_)))
                    .collect()
            })
            .unwrap_or_default();
        let members = self
            .group_members(id)
            .into_iter()
            .filter_map(|place| self.entities.get(place.get()))
            .collect();
        (brushes, members)
    }

    /// The linked group the entity at `place` is in or is, for an edit there to update
    /// (MAP-9.7).
    #[must_use]
    pub fn linked_group_of(&self, place: EntityPlace) -> Option<LinkedGroup> {
        let entity = self.entities.get(place.get())?;
        if let Some(group) = entity.linked_group(place) {
            return Some(group);
        }
        let id = entity.group()?;
        self.linked_groups()
            .into_iter()
            .find(|group| group.id == id)
    }

    /// Copies the contents of the linked group whose own entity is at `source` into every
    /// group linked to it (MAP-9.6 to MAP-9.8): each other group's brushes and entities are
    /// replaced by the source's, transformed by its transformation times the inverse of the
    /// source's, its protected keys kept.
    ///
    /// # Errors
    ///
    /// [`LinkError::NotLinked`] when `source` is no linked group, [`LinkError::NotInvertible`]
    /// when its transformation has no inverse, [`LinkError::Nested`] when a group of the set
    /// holds a group, and [`LinkError::Map`] when a copy cannot be written; the map is then
    /// left as it was. On success, where each entity went: a replaced entity goes to the copy
    /// at its position (MAP-9.6), and one with no copy is gone.
    pub fn update_linked(&mut self, source: EntityPlace) -> Result<Moves, LinkError> {
        let group = self
            .entities
            .get(source.get())
            .and_then(|entity| entity.linked_group(source))
            .ok_or(LinkError::NotLinked(source))?;
        let inverse = group
            .transform
            .inverse()
            .ok_or(LinkError::NotInvertible(source))?;
        let siblings: Vec<LinkedGroup> = self
            .linked_groups()
            .into_iter()
            .filter(|other| other.link == group.link && other.id != group.id)
            .collect();
        for checked in core::iter::once(&group).chain(&siblings) {
            let holds_group = self
                .group_members(checked.id)
                .iter()
                .filter_map(|place| self.entities.get(place.get()))
                .any(|entity| entity.own_container().is_some());
            if holds_group {
                return Err(LinkError::Nested(checked.entity));
            }
        }
        let brushes: Vec<Item> = self
            .entities
            .get(source.get())
            .map(|entity| {
                entity
                    .items
                    .iter()
                    .filter(|item| matches!(item, Item::Brush(_)))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let members: Vec<Entity> = self
            .group_members(group.id)
            .into_iter()
            .filter_map(|place| self.entities.get(place.get()).cloned())
            .collect();
        let mut updated = self.clone();
        let mut origin: Vec<Option<usize>> = (0..self.entities.len()).map(Some).collect();
        for sibling in siblings {
            let transform = sibling.transform.after(&inverse);
            updated.replace_contents(&sibling, &brushes, &members, &transform, &mut origin)?;
        }
        *self = updated;
        let mut to = vec![None; origin.len().max(self.entities.len())];
        for (now, was) in origin.into_iter().enumerate() {
            if let Some(slot) = was.and_then(|was| to.get_mut(was)) {
                *slot = Some(EntityPlace::new(now));
            }
        }
        Ok(Moves { to })
    }

    /// Replaces the contents of `target` with `brushes` and `members` moved by `transform`,
    /// keeping the protected keys of the entities they replace, by position (MAP-9.6, MAP-9.8).
    fn replace_contents(
        &mut self,
        target: &LinkedGroup,
        brushes: &[Item],
        members: &[Entity],
        transform: &Transform,
        origin: &mut Vec<Option<usize>>,
    ) -> Result<(), LinkError> {
        let place = self
            .linked_groups()
            .into_iter()
            .find(|group| group.id == target.id)
            .map(|group| group.entity)
            .ok_or(LinkError::NotLinked(target.entity))?;
        let mut holder = Entity::new();
        holder.items = brushes.to_vec();
        let moved = holder.transformed(transform)?;
        if let Some(own) = self.entities.get_mut(place.get()) {
            own.items.retain(|item| matches!(item, Item::Pair(_)));
            own.items.extend(moved.items);
        }
        let old_places = self.group_members(target.id);
        let old: Vec<Entity> = old_places
            .iter()
            .filter_map(|at| self.entities.get(at.get()).cloned())
            .collect();
        let mut copies = Vec::new();
        for (index, member) in members.iter().enumerate() {
            let mut copy = member.transformed(transform)?;
            copy.set(GROUP_KEY, target.id.get().to_string().as_bytes())?;
            if let Some(replaced) = old.get(index) {
                copy.keep_protected(member, replaced)?;
            }
            copies.push(copy);
        }
        let at = old_places
            .first()
            .map_or(place.get().saturating_add(1), |first| first.get());
        let mut replaced = Vec::new();
        for gone in old_places.iter().rev().map(|gone| gone.get()) {
            if self.entities.get(gone).is_some() {
                self.entities.remove(gone);
                replaced.push(if gone < origin.len() {
                    origin.remove(gone)
                } else {
                    None
                });
            }
        }
        replaced.reverse();
        let at = at.min(self.entities.len());
        for (offset, copy) in copies.into_iter().enumerate() {
            let place = at.saturating_add(offset);
            self.entities.insert(place, copy);
            origin.insert(
                place.min(origin.len()),
                replaced.get(offset).copied().flatten(),
            );
        }
        Ok(())
    }

    /// The map without the layers marked to be left out of an export, and the entities in
    /// them (MAP-9.9).
    #[must_use]
    pub fn without_omitted_layers(&self) -> Self {
        let omitted: Vec<u64> = self
            .entities
            .iter()
            .filter(|entity| entity.layer_settings().omit_from_export)
            .filter_map(|entity| match entity.own_container() {
                Some((RoomKind::Layer, id)) => Some(id),
                Some((RoomKind::Group, _)) | None => None,
            })
            .collect();
        let mut out = self.clone();
        out.entities.retain(|entity| {
            let own = matches!(entity.own_container(), Some((RoomKind::Layer, id)) if omitted.contains(&id));
            let member = entity
                .value(LAYER_KEY)
                .and_then(container_id)
                .is_some_and(|id| omitted.contains(&id));
            !own && !member
        });
        out
    }
}

#[cfg(test)]
mod tests;
