// SPDX-License-Identifier: GPL-3.0-or-later

//! Keeping an overview as TrenchBroom does (P5-14, MAP-26): an edit inside a linked group is
//! copied into the groups linked to it in the same undo step (MAP-9.7); moving a group moves
//! what is in it and its transformation; layers are hidden, locked or left out of an export
//! (MAP-9.9); and any selection can be hidden, isolated or locked in the editor. An edit that
//! would change a locked entity is refused.

use std::collections::BTreeSet;
use std::path::PathBuf;

use qs_commands::{Command, NoParams, Outcome};
use qs_map::{
    ContainerId, EntityPlace, LayerSetting, LayerSwitch, MapFile, Moves, RoomKind, Transform,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{EntityIndex, MapDocument, MapEdited, edit_map};
use crate::{HasWorkbench, WorkbenchError};

/// The id of `map.hide`.
pub const MAP_HIDE: &str = "map.hide";
/// The id of `map.isolate`.
pub const MAP_ISOLATE: &str = "map.isolate";
/// The id of `map.show_all`.
pub const MAP_SHOW_ALL: &str = "map.show_all";
/// The id of `map.lock`.
pub const MAP_LOCK: &str = "map.lock";
/// The id of `map.unlock_all`.
pub const MAP_UNLOCK_ALL: &str = "map.unlock_all";
/// The id of `map.set_layer`.
pub const MAP_SET_LAYER: &str = "map.set_layer";
/// The id of `map.layers`.
pub const MAP_LAYERS: &str = "map.layers";
/// The id of `map.export`.
pub const MAP_EXPORT: &str = "map.export";

/// The key holding a layer's or group's name (MAP-9.1).
pub(super) const NAME_KEY: &[u8] = b"_tb_name";

/// The entities hidden and locked in the editor, by their place in the map.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ViewState {
    hidden: BTreeSet<EntityIndex>,
    locked: BTreeSet<EntityIndex>,
}

impl ViewState {
    /// Follows the entities to where `moves` put them, forgetting those that are gone.
    fn follow(&mut self, moves: &Moves) {
        let follow = |set: &BTreeSet<EntityIndex>| -> BTreeSet<EntityIndex> {
            set.iter()
                .filter_map(|entity| moves.of(EntityPlace::new(entity.get())))
                .map(|place| EntityIndex::new(place.get()))
                .collect()
        };
        self.hidden = follow(&self.hidden);
        self.locked = follow(&self.locked);
    }

    /// Forgets every hidden and locked entity, whose places no longer name the same entities.
    pub(super) fn forget(&mut self) {
        self.hidden.clear();
        self.locked.clear();
    }

    /// Forgets `entity`, deleted, and moves the entities after it back by one.
    pub(super) fn deleted(&mut self, entity: EntityIndex) {
        let shift = |set: &BTreeSet<EntityIndex>| -> BTreeSet<EntityIndex> {
            set.iter()
                .filter(|&&e| e != entity)
                .map(|&e| {
                    if e > entity {
                        EntityIndex::new(e.get().saturating_sub(1))
                    } else {
                        e
                    }
                })
                .collect()
        };
        self.hidden = shift(&self.hidden);
        self.locked = shift(&self.locked);
    }
}

/// Which layer setting `map.set_layer` changes (MAP-9.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LayerOption {
    /// Hidden in the editor.
    Hidden,
    /// Locked against edits.
    Locked,
    /// Left out of an exported map.
    OmitFromExport,
}

impl LayerOption {
    /// The setting it names.
    const fn setting(self) -> LayerSetting {
        match self {
            Self::Hidden => LayerSetting::Hidden,
            Self::Locked => LayerSetting::Locked,
            Self::OmitFromExport => LayerSetting::OmitFromExport,
        }
    }
}

/// Whether `map.set_layer` turns a setting on or off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Switch {
    /// On.
    On,
    /// Off.
    Off,
}

impl Switch {
    /// The map's switch for it.
    const fn map(self) -> LayerSwitch {
        match self {
            Self::On => LayerSwitch::On,
            Self::Off => LayerSwitch::Off,
        }
    }
}

/// The parameters of `map.hide`, `map.isolate` and `map.lock`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntityListParams {
    /// The entities.
    pub entities: Vec<EntityIndex>,
}

/// The parameters of `map.set_layer`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLayerParams {
    /// The layer's own entity.
    pub layer: EntityIndex,
    /// The setting.
    pub setting: LayerOption,
    /// On or off.
    pub switch: Switch,
}

/// The parameters of `map.export`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    /// The `.map` to write.
    pub path: PathBuf,
}

/// One layer, as `map.layers` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LayerInfo {
    /// Its own entity.
    pub entity: EntityIndex,
    /// Its `_tb_name`.
    pub name: String,
    /// Hidden in the editor.
    pub hidden: bool,
    /// Locked against edits.
    pub locked: bool,
    /// Left out of an exported map.
    pub omit_from_export: bool,
    /// The entities in it.
    pub entities: usize,
}

impl MapDocument {
    /// The entities of `file` in a layer with `setting` set (MAP-9.9), the layer's own entity
    /// with them.
    fn in_layers(file: &MapFile, setting: LayerOption) -> BTreeSet<EntityIndex> {
        let set: BTreeSet<ContainerId> = file
            .entities
            .iter()
            .filter_map(|entity| match entity.container() {
                Some((RoomKind::Layer, id)) => {
                    let settings = entity.layer_settings();
                    let on = match setting {
                        LayerOption::Hidden => settings.hidden,
                        LayerOption::Locked => settings.locked,
                        LayerOption::OmitFromExport => settings.omit_from_export,
                    };
                    on.then_some(id)
                }
                Some((RoomKind::Group, _)) | None => None,
            })
            .collect();
        if set.is_empty() {
            return BTreeSet::new();
        }
        file.layer_of_each()
            .into_iter()
            .enumerate()
            .filter(|(_, layer)| layer.is_some_and(|layer| set.contains(&layer)))
            .map(|(place, _)| EntityIndex::new(place))
            .collect()
    }

    /// The entities hidden in the editor: by `map.hide` or `map.isolate`, or in a hidden layer.
    #[must_use]
    pub fn hidden_entities(&self) -> BTreeSet<EntityIndex> {
        let mut hidden = Self::in_layers(&self.file, LayerOption::Hidden);
        hidden.extend(self.view.hidden.iter().copied());
        hidden
    }

    /// Whether `entity` is hidden: by `map.hide` or `map.isolate`, or in a hidden layer.
    #[must_use]
    pub fn is_hidden(&self, entity: EntityIndex) -> bool {
        self.hidden_entities().contains(&entity)
    }

    /// The entities of `file` that are locked: by `map.lock`, or in a locked layer.
    fn locked_in(&self, file: &MapFile) -> BTreeSet<EntityIndex> {
        let mut locked = Self::in_layers(file, LayerOption::Locked);
        locked.extend(self.view.locked.iter().copied());
        locked
    }

    /// Whether `entity` is locked: by `map.lock`, or in a locked layer.
    #[must_use]
    pub fn is_locked(&self, entity: EntityIndex) -> bool {
        self.locked_in(&self.file).contains(&entity)
    }

    /// Refuses an edit of `entity` when it is locked.
    pub(super) fn editable(&self, entity: EntityIndex) -> Result<(), WorkbenchError> {
        if self.is_locked(entity) {
            return Err(WorkbenchError::Locked { entity });
        }
        Ok(())
    }

    /// The first entity of `locked` that differs between `was` and `now`, an entity at a place
    /// in `was` found at the place `moved` gives in `now`; a layer's own entity compared
    /// without its settings, so a locked layer can be unlocked.
    fn changed_locked(
        locked: &BTreeSet<EntityIndex>,
        was: &MapFile,
        now: &MapFile,
        moved: impl Fn(EntityIndex) -> Option<EntityIndex>,
    ) -> Option<EntityIndex> {
        let bare = |entity: Option<&qs_map::Entity>| {
            entity.map(|entity| {
                let mut entity = entity.clone();
                for setting in [
                    LayerSetting::Hidden,
                    LayerSetting::Locked,
                    LayerSetting::OmitFromExport,
                ] {
                    // Clearing a key cannot fail; a failure leaves it, and the entity differs.
                    let _cleared = entity.set_layer_setting(setting, LayerSwitch::Off);
                }
                entity
            })
        };
        locked.iter().copied().find(|&entity| {
            let then = bare(was.entities.get(entity.get()));
            let after = bare(moved(entity).and_then(|place| now.entities.get(place.get())));
            then != after
        })
    }

    /// After an edit of the map from `before`: refuses it when it changed a locked entity, and
    /// copies each change inside a linked group into the groups linked to it (MAP-9.7), the
    /// selection and the hidden and locked entities following the entities they were; refused
    /// too when a copy would change a locked entity.
    pub(super) fn after_edit(&mut self, before: &MapFile) -> Result<(), WorkbenchError> {
        // An edit that removes entities is checked by the command that removes them.
        if self.file.entities.len() >= before.entities.len() {
            let locked = self.locked_in(before);
            if let Some(entity) = Self::changed_locked(&locked, before, &self.file, Some) {
                return Err(WorkbenchError::Locked { entity });
            }
        }
        let edited = self.file.clone();
        let moves = self
            .file
            .update_changed(before)
            .map_err(|error| WorkbenchError::Link { error })?;
        let follow = |entity: EntityIndex| {
            moves
                .of(EntityPlace::new(entity.get()))
                .map(|place| EntityIndex::new(place.get()))
        };
        let locked = self.locked_in(&edited);
        if let Some(entity) = Self::changed_locked(&locked, &edited, &self.file, follow) {
            return Err(WorkbenchError::Locked { entity });
        }
        self.selected = self.selected.and_then(follow);
        self.view.follow(&moves);
        Ok(())
    }

    /// Moves `entity` by `by`; a group with everything in it, however deep, each group's
    /// transformation moved too (MAP-9.5), so the groups linked to them stay as they are.
    pub(super) fn move_with_group(
        &mut self,
        entity: EntityIndex,
        by: [f64; 3],
    ) -> Result<(), WorkbenchError> {
        let group = self
            .file
            .entities
            .get(entity.get())
            .and_then(|e| match e.container() {
                Some((RoomKind::Group, id)) => Some(id),
                Some((RoomKind::Layer, _)) | None => None,
            });
        let Some(id) = group else {
            return self.move_entity(entity, by);
        };
        let found = self.entity_mut(entity)?;
        // A group's own entity has no origin: only its brushes move.
        if found.has_brushes() {
            self.move_entity(entity, by)?;
        }
        let found = self.entity_mut(entity)?;
        if let Some(transform) = found.transform() {
            let set = found.set_transform(&Transform::translation(by).after(&transform));
            set.map_err(|error| self.map_error(error))?;
        }
        for member in self.file.group_members(id) {
            self.move_with_group(EntityIndex::new(member.get()), by)?;
        }
        Ok(())
    }

    /// The map's layers (MAP-9.1, MAP-9.9).
    fn layers(&self) -> Vec<LayerInfo> {
        let layers = self.file.layer_of_each();
        self.file
            .entities
            .iter()
            .enumerate()
            .filter_map(|(place, entity)| match entity.container() {
                Some((RoomKind::Layer, id)) => {
                    let settings = entity.layer_settings();
                    let own = place;
                    Some(LayerInfo {
                        entity: EntityIndex::new(place),
                        name: entity
                            .value(NAME_KEY)
                            .map(|name| String::from_utf8_lossy(name).into_owned())
                            .unwrap_or_default(),
                        hidden: settings.hidden,
                        locked: settings.locked,
                        omit_from_export: settings.omit_from_export,
                        entities: layers
                            .iter()
                            .zip(0..)
                            .filter(|&(layer, place)| *layer == Some(id) && place != own)
                            .count(),
                    })
                }
                Some((RoomKind::Group, _)) | None => None,
            })
            .collect()
    }
}

/// The view change a command makes to the open map's hidden and locked entities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ViewChange {
    /// Hide the entities.
    Hide,
    /// Hide every other entity.
    Isolate,
    /// Lock the entities.
    Lock,
}

impl ViewChange {
    /// Makes the change to `document` for `entities`.
    fn apply(
        self,
        document: &mut MapDocument,
        entities: Vec<EntityIndex>,
    ) -> Result<(), WorkbenchError> {
        for &entity in &entities {
            document.entity_mut(entity)?;
        }
        let chosen: BTreeSet<EntityIndex> = entities.into_iter().collect();
        match self {
            Self::Hide => document.view.hidden.extend(chosen),
            Self::Isolate => {
                document.view.hidden = (1..document.file.entities.len())
                    .map(EntityIndex::new)
                    .filter(|entity| !chosen.contains(entity))
                    .collect();
            }
            Self::Lock => document.view.locked.extend(chosen),
        }
        document.touch();
        Ok(())
    }
}

/// One of `map.hide`, `map.isolate` and `map.lock`.
pub(super) trait ViewKind: Send + Sync {
    /// The command's id.
    const ID: &'static str;
    /// Its summary.
    const SUMMARY: &'static str;
    /// The change it makes.
    const CHANGE: ViewChange;
}

/// `map.hide`.
pub(super) struct Hide;

impl ViewKind for Hide {
    const ID: &'static str = MAP_HIDE;
    const SUMMARY: &'static str = "Hides entities of the open map in the editor until \
        map.show_all (TrenchBroom's Hide Selection); the map is not changed.";
    const CHANGE: ViewChange = ViewChange::Hide;
}

/// `map.isolate`.
pub(super) struct Isolate;

impl ViewKind for Isolate {
    const ID: &'static str = MAP_ISOLATE;
    const SUMMARY: &'static str = "Hides every entity of the open map but these in the editor \
        until map.show_all (TrenchBroom's Isolate Selection); the map is not changed.";
    const CHANGE: ViewChange = ViewChange::Isolate;
}

/// `map.lock`.
pub(super) struct Lock;

impl ViewKind for Lock {
    const ID: &'static str = MAP_LOCK;
    const SUMMARY: &'static str = "Locks entities of the open map against edits until \
        map.unlock_all; the map is not changed.";
    const CHANGE: ViewChange = ViewChange::Lock;
}

/// A view command of kind `V`.
pub(super) struct MapView<V>(core::marker::PhantomData<V>);

impl<V> MapView<V> {
    /// The command.
    pub(super) const fn new() -> Self {
        Self(core::marker::PhantomData)
    }
}

impl<C: HasWorkbench + 'static, V: ViewKind + 'static> Command<C> for MapView<V> {
    const ID: &'static str = V::ID;
    const SUMMARY: &'static str = V::SUMMARY;
    type Params = EntityListParams;
    type Output = ();
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: EntityListParams,
    ) -> Result<Outcome<(), C>, WorkbenchError> {
        let document = context.workbench().map_mut()?;
        V::CHANGE.apply(document, params.entities)?;
        Ok(Outcome::outside_history(()))
    }
}

/// `map.show_all` and `map.unlock_all`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ViewReset {
    /// Show every entity hidden in the editor.
    ShowAll,
    /// Unlock every entity locked in the editor.
    UnlockAll,
}

/// `map.show_all`.
pub(super) struct ShowAll;

impl<C: HasWorkbench + 'static> Command<C> for ShowAll {
    const ID: &'static str = MAP_SHOW_ALL;
    const SUMMARY: &'static str = "Shows every entity of the open map hidden in the editor \
        (TrenchBroom's Show All); a hidden layer stays hidden.";
    type Params = NoParams;
    type Output = ();
    type Error = WorkbenchError;

    fn run(&self, context: &mut C, _params: NoParams) -> Result<Outcome<(), C>, WorkbenchError> {
        context
            .workbench()
            .map_mut()?
            .reset_view(ViewReset::ShowAll);
        Ok(Outcome::outside_history(()))
    }
}

/// `map.unlock_all`.
pub(super) struct UnlockAll;

impl<C: HasWorkbench + 'static> Command<C> for UnlockAll {
    const ID: &'static str = MAP_UNLOCK_ALL;
    const SUMMARY: &'static str = "Unlocks every entity of the open map locked in the editor; \
        a locked layer stays locked.";
    type Params = NoParams;
    type Output = ();
    type Error = WorkbenchError;

    fn run(&self, context: &mut C, _params: NoParams) -> Result<Outcome<(), C>, WorkbenchError> {
        context
            .workbench()
            .map_mut()?
            .reset_view(ViewReset::UnlockAll);
        Ok(Outcome::outside_history(()))
    }
}

impl MapDocument {
    /// Clears the editor's hidden or locked entities.
    fn reset_view(&mut self, reset: ViewReset) {
        match reset {
            ViewReset::ShowAll => self.view.hidden.clear(),
            ViewReset::UnlockAll => self.view.locked.clear(),
        }
        self.touch();
    }
}

/// `map.set_layer`.
pub(super) struct MapSetLayer;

impl<C: HasWorkbench + 'static> Command<C> for MapSetLayer {
    const ID: &'static str = MAP_SET_LAYER;
    const SUMMARY: &'static str = "Turns a layer of the open map hidden, locked or left out of \
        exports on or off, written as TrenchBroom writes it.";
    type Params = SetLayerParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: SetLayerParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = edit_map(context, |document| {
            let layer = document.entity_mut(params.layer)?;
            if !matches!(layer.container(), Some((RoomKind::Layer, _))) {
                return Err(WorkbenchError::NotLayer {
                    entity: params.layer,
                });
            }
            let set = layer.set_layer_setting(params.setting.setting(), params.switch.map());
            set.map_err(|error| document.map_error(error))
        })?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(params.layer),
            },
            step,
        ))
    }
}

/// `map.layers`.
pub(super) struct MapLayers;

impl<C: HasWorkbench + 'static> Command<C> for MapLayers {
    const ID: &'static str = MAP_LAYERS;
    const SUMMARY: &'static str = "The open map's TrenchBroom layers: each one's name, whether \
        it is hidden, locked or left out of exports, and how many entities are in it.";
    type Params = NoParams;
    type Output = Vec<LayerInfo>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        _params: NoParams,
    ) -> Result<Outcome<Vec<LayerInfo>, C>, WorkbenchError> {
        let document = context.workbench().map().ok_or(WorkbenchError::NoMap)?;
        Ok(Outcome::unchanged(document.layers()))
    }
}

/// `map.export`.
pub(super) struct MapExport;

impl<C: HasWorkbench + 'static> Command<C> for MapExport {
    const ID: &'static str = MAP_EXPORT;
    const SUMMARY: &'static str = "Writes the open map to another .map without the layers \
        marked to be left out of exports and the entities in them (TrenchBroom's Export).";
    type Params = ExportParams;
    type Output = ();
    type Error = WorkbenchError;

    fn run(&self, context: &mut C, params: ExportParams) -> Result<Outcome<(), C>, WorkbenchError> {
        let document = context.workbench().map().ok_or(WorkbenchError::NoMap)?;
        let text = document.file.without_omitted_layers().to_bytes();
        std::fs::write(&params.path, text).map_err(|error| WorkbenchError::Save {
            path: params.path.clone(),
            error,
        })?;
        Ok(Outcome::outside_history(()))
    }
}

#[cfg(test)]
mod tests;
