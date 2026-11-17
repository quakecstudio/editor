// SPDX-License-Identifier: GPL-3.0-or-later

//! Keeping an overview of a map (P5-14, MAP-26): the map's TrenchBroom layers, each hidden,
//! locked or left out of exports with a checkbox, and TrenchBroom's keys to isolate, hide and
//! show the selection again. Each is a [`GroupAction`] the shell runs as a `map.*` command.

use eframe::egui::{Key, Modifiers, RichText, Ui};
use qs_map_edit::{BrushIndex, EntityIndex, LayerInfo, LayerOption, Switch};

use crate::icons::{Icon, Toggled};
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
        if Icon::Add.tool(ui, Text::MapAddLayer).clicked() {
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
            ui.label(Icon::Layers.before(&layer.name));
            ui.label(
                RichText::new(layer_count(layer.entities)).color(theme.color(Token::TextMuted)),
            );
            // Each setting an icon, framed while it is on, as Zed's panel toggles.
            let settings = [
                (
                    LayerOption::Hidden,
                    layer.hidden,
                    Icon::Hide,
                    Text::MapLayerHidden,
                ),
                (
                    LayerOption::Locked,
                    layer.locked,
                    Icon::Lock,
                    Text::MapLayerLocked,
                ),
                (
                    LayerOption::OmitFromExport,
                    layer.omit_from_export,
                    Icon::Omit,
                    Text::MapLayerOmit,
                ),
            ];
            for (option, set, icon, text) in settings {
                let state = if set { Toggled::On } else { Toggled::Off };
                let name = format!("{}: {}", layer.name, text.get());
                if icon
                    .toggle_named(ui, state, &name, Text::MapLayerSettingHelp.get())
                    .clicked()
                {
                    let switch = if set { Switch::Off } else { Switch::On };
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
    // The selection's actions as one row of icons; one that does not apply is greyed.
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled_ui(grouping.any(), |ui| {
            if Icon::Group.tool(ui, Text::MapGroup).clicked() {
                actions.push(grouping.grouped());
            }
        });
        if let Some(group) = grouping.group {
            for (icon, text, action) in [
                (Icon::Ungroup, Text::MapUngroup, GroupAction::Ungroup(group)),
                (
                    Icon::LinkDuplicate,
                    Text::MapLinkDuplicate,
                    GroupAction::LinkDuplicate(group),
                ),
                (
                    Icon::Separate,
                    Text::MapSeparate,
                    GroupAction::Separate(group),
                ),
            ] {
                if icon.tool(ui, text).clicked() {
                    actions.push(action);
                }
            }
        }
        ui.separator();
        ui.add_enabled_ui(!selection.is_empty(), |ui| {
            for (icon, text, action) in [
                (
                    Icon::Hide,
                    Text::MapHide,
                    GroupAction::Hide(selection.to_vec()),
                ),
                (
                    Icon::Isolate,
                    Text::MapIsolate,
                    GroupAction::Isolate(selection.to_vec()),
                ),
                (
                    Icon::Lock,
                    Text::MapLock,
                    GroupAction::Lock(selection.to_vec()),
                ),
            ] {
                if icon.tool(ui, text).clicked() {
                    actions.push(action);
                }
            }
        });
        if Icon::Show.tool(ui, Text::MapShowAll).clicked() {
            actions.push(GroupAction::ShowAll);
        }
        if Icon::Unlock.tool(ui, Text::MapUnlockAll).clicked() {
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
        if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::G) {
            actions.extend(grouping.group.map(GroupAction::Ungroup));
        } else if i.consume_key(Modifiers::COMMAND, Key::G) && grouping.any() {
            actions.push(grouping.grouped());
        }
        if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::D) {
            actions.extend(grouping.group.map(GroupAction::LinkDuplicate));
        }
        if i.consume_key(Modifiers::COMMAND | Modifiers::ALT, Key::I) {
            if !selection.is_empty() {
                actions.push(GroupAction::Hide(selection.to_vec()));
            }
        } else if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::I) {
            actions.push(GroupAction::ShowAll);
        } else if i.consume_key(Modifiers::COMMAND, Key::I) && !selection.is_empty() {
            actions.push(GroupAction::Isolate(selection.to_vec()));
        }
    });
    actions
}
