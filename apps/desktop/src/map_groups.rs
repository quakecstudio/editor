// SPDX-License-Identifier: GPL-3.0-or-later

//! Keeping an overview of a map (P5-14, MAP-26): the map's TrenchBroom layers, each hidden,
//! locked or left out of exports with a checkbox, and TrenchBroom's keys to isolate, hide and
//! show the selection again. Each is a [`GroupAction`] the shell runs as a `map.*` command.

use eframe::egui::{Key, Modifiers, RichText, Ui};
use qs_map_edit::{BrushIndex, EntityIndex, LayerInfo, LayerOption, Switch};

use qs_layout::ViewKey;

use crate::keymap;
use crate::strings::{Text, layer_count};
use crate::theme::{Theme, Token};

/// What the layer list or the overview keys ask for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupAction {
    /// Hide the entities in the editor.
    Hide(Vec<EntityIndex>),
    /// Hide every other entity.
    Isolate(Vec<EntityIndex>),
    /// Lock the entities against edits.
    Lock(Vec<EntityIndex>),
    /// Show every entity hidden in the editor.
    ShowAll,
    /// Unlock every entity locked in the editor.
    UnlockAll,
    /// Turn a layer's setting on or off.
    SetLayer(EntityIndex, LayerOption, Switch),
    /// Put entities and world brushes in a new group (P7-08).
    Group(Vec<EntityIndex>, Vec<BrushIndex>),
    /// Take a group's contents out of it.
    Ungroup(EntityIndex),
    /// Copy a group as one linked to it.
    LinkDuplicate(EntityIndex),
    /// Unlink a group from the groups linked to it.
    Separate(EntityIndex),
    /// Add a layer.
    AddLayer,
}

/// What a selection can be grouped from, and the group it is, if it is one (P7-08).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Grouping<'a> {
    /// The entities selected.
    pub(crate) entities: &'a [EntityIndex],
    /// The world's brushes selected.
    pub(crate) brushes: &'a [BrushIndex],
    /// The group the selection is, when the one entity selected is a group's own.
    pub(crate) group: Option<EntityIndex>,
}

impl Grouping<'_> {
    /// Whether there is anything to group.
    pub(crate) fn any(&self) -> bool {
        !self.entities.is_empty() || !self.brushes.is_empty()
    }

    /// The new group of the selection.
    pub(crate) fn grouped(&self) -> GroupAction {
        GroupAction::Group(self.entities.to_vec(), self.brushes.to_vec())
    }
}

/// The map's layers, each with its settings as checkboxes; what was changed.
pub(crate) fn layers_ui(
    ui: &mut Ui,
    theme: &Theme,
    layers: &[LayerInfo],
    current: &mut Option<EntityIndex>,
) -> Vec<GroupAction> {
    let mut actions = Vec::new();
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(Text::MapLayers.get()).strong());
        if ui.button(Text::MapAddLayer.get()).clicked() {
            actions.push(GroupAction::AddLayer);
        }
    });
    // TrenchBroom's current layer: what new brushes and entities go into (P7-08).
    if !layers.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(Text::MapCurrentLayer.get());
            ui.radio_value(current, None, Text::MapDefaultLayer.get());
            for layer in layers {
                ui.radio_value(current, Some(layer.entity), &layer.name);
            }
        });
    }
    if current.is_some_and(|entity| layers.iter().all(|l| l.entity != entity)) {
        *current = None;
    }
    for layer in layers {
        ui.horizontal_wrapped(|ui| {
            ui.label(&layer.name);
            ui.label(
                RichText::new(layer_count(layer.entities)).color(theme.color(Token::TextMuted)),
            );
            let settings = [
                (LayerOption::Hidden, layer.hidden, Text::MapLayerHidden),
                (LayerOption::Locked, layer.locked, Text::MapLayerLocked),
                (
                    LayerOption::OmitFromExport,
                    layer.omit_from_export,
                    Text::MapLayerOmit,
                ),
            ];
            for (option, set, text) in settings {
                let mut on = set;
                if ui.checkbox(&mut on, text.get()).changed() {
                    let switch = if on { Switch::On } else { Switch::Off };
                    actions.push(GroupAction::SetLayer(layer.entity, option, switch));
                }
            }
        });
    }
    actions
}

/// The selection's buttons: hide, isolate and lock it, and show and unlock everything; group
/// it, and ungroup, duplicate linked or separate the group it is.
pub(crate) fn selection_ui(
    ui: &mut Ui,
    selection: &[EntityIndex],
    grouping: Grouping<'_>,
) -> Vec<GroupAction> {
    let mut actions = Vec::new();
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                grouping.any(),
                eframe::egui::Button::new(Text::MapGroup.get()),
            )
            .clicked()
        {
            actions.push(grouping.grouped());
        }
        if let Some(group) = grouping.group {
            for (text, action) in [
                (Text::MapUngroup, GroupAction::Ungroup(group)),
                (Text::MapLinkDuplicate, GroupAction::LinkDuplicate(group)),
                (Text::MapSeparate, GroupAction::Separate(group)),
            ] {
                if ui.button(text.get()).clicked() {
                    actions.push(action);
                }
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        let chosen = !selection.is_empty();
        if ui
            .add_enabled(chosen, eframe::egui::Button::new(Text::MapHide.get()))
            .clicked()
        {
            actions.push(GroupAction::Hide(selection.to_vec()));
        }
        if ui
            .add_enabled(chosen, eframe::egui::Button::new(Text::MapIsolate.get()))
            .clicked()
        {
            actions.push(GroupAction::Isolate(selection.to_vec()));
        }
        if ui
            .add_enabled(chosen, eframe::egui::Button::new(Text::MapLock.get()))
            .clicked()
        {
            actions.push(GroupAction::Lock(selection.to_vec()));
        }
        if ui.button(Text::MapShowAll.get()).clicked() {
            actions.push(GroupAction::ShowAll);
        }
        if ui.button(Text::MapUnlockAll.get()).clicked() {
            actions.push(GroupAction::UnlockAll);
        }
    });
    actions
}

/// TrenchBroom's overview keys while the view has the keyboard (`ActionManager.cpp`:
/// "Menu/View/Isolate" Ctrl+I, "Menu/View/Hide" Ctrl+Alt+I, "Menu/View/Show All"
/// Ctrl+Shift+I; "Menu/Edit/Group" Ctrl+G, "Menu/Edit/Ungroup" Ctrl+Shift+G, "Menu/Edit/Create
/// Linked Duplicate" Ctrl+Shift+D; Cmd for Ctrl on macOS).
pub(crate) fn overview_keys(
    ui: &Ui,
    selection: &[EntityIndex],
    grouping: Grouping<'_>,
) -> Vec<GroupAction> {
    let mut actions = Vec::new();
    ui.input_mut(|i| {
        if keymap::pressed(i, ViewKey::Ungroup) {
            actions.extend(grouping.group.map(GroupAction::Ungroup));
        } else if keymap::pressed(i, ViewKey::Group) && grouping.any() {
            actions.push(grouping.grouped());
        }
        if keymap::pressed(i, ViewKey::LinkedDuplicate) {
            actions.extend(grouping.group.map(GroupAction::LinkDuplicate));
        }
        if keymap::pressed(i, ViewKey::Hide) {
            if !selection.is_empty() {
                actions.push(GroupAction::Hide(selection.to_vec()));
            }
        } else if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::I) {
            actions.push(GroupAction::ShowAll);
        } else if keymap::pressed(i, ViewKey::Isolate) && !selection.is_empty() {
            actions.push(GroupAction::Isolate(selection.to_vec()));
        }
    });
    actions
}
