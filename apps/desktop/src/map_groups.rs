// SPDX-License-Identifier: GPL-3.0-or-later

//! Keeping an overview of a map (P5-14, MAP-26): the map's TrenchBroom layers, each hidden,
//! locked or left out of exports with a checkbox, and TrenchBroom's keys to isolate, hide and
//! show the selection again. Each is a [`GroupAction`] the shell runs as a `map.*` command.

use eframe::egui::{Key, Modifiers, RichText, Ui};
use qs_workbench::map_edit::{EntityIndex, LayerInfo, LayerOption, Switch};

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
}

/// The map's layers, each with its settings as checkboxes; what was changed.
pub(crate) fn layers_ui(ui: &mut Ui, theme: &Theme, layers: &[LayerInfo]) -> Vec<GroupAction> {
    let mut actions = Vec::new();
    if layers.is_empty() {
        return actions;
    }
    ui.label(RichText::new(Text::MapLayers.get()).strong());
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

/// The selection's buttons: hide, isolate and lock it, and show and unlock everything.
pub(crate) fn selection_ui(ui: &mut Ui, selection: &[EntityIndex]) -> Vec<GroupAction> {
    let mut actions = Vec::new();
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
/// Ctrl+Shift+I; Cmd for Ctrl on macOS).
pub(crate) fn overview_keys(ui: &Ui, selection: &[EntityIndex]) -> Vec<GroupAction> {
    let mut actions = Vec::new();
    ui.input_mut(|i| {
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
