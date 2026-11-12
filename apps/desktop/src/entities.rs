// SPDX-License-Identifier: GPL-3.0-or-later

//! The Entities outliner and the Properties panel (P3-13 to P3-15): the running game's edicts by
//! classname with a filter, and the selected edict's fields, editable while the game is paused.
//! What a click asks for is returned to the shell, which runs it through its command.

use std::collections::BTreeMap;

use eframe::egui::{self, RichText, Ui};
use qs_workbench::entities::Entities;
use qs_workbench::{PlayState, Preview};

use crate::strings::Text;
use crate::theme::{Theme, Token};
use qs_workbench::ids::Edict;

/// What a click in the outliner or the Properties panel asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EntityAction {
    /// Select this edict.
    Select(Edict),
    /// Set this field of the selected edict to this text.
    SetField {
        /// The field.
        field: String,
        /// The value as typed.
        value: String,
    },
    /// Open this function's definition.
    Open(String),
    /// Select this edict and start an agent thread about it, its state attached (UI-9).
    Ask(Edict),
}

/// The outliner's own state: its filter text.
#[derive(Debug, Default)]
pub(crate) struct Outliner {
    filter: String,
}

impl Outliner {
    /// The outliner with `filter` typed in it.
    pub(crate) fn with_filter(filter: &str) -> Self {
        Self {
            filter: filter.to_owned(),
        }
    }
}

/// The Properties panel's own state: the text of each field being edited.
#[derive(Debug, Default)]
pub(crate) struct Properties {
    edits: BTreeMap<String, String>,
    /// The edict the edits are for; another selection drops them.
    edict: Option<Edict>,
}

/// The live count and the edicts by classname, filtered; a click selects an edict.
pub(crate) fn outliner(
    ui: &mut Ui,
    theme: &Theme,
    entities: &Entities,
    outliner: &mut Outliner,
) -> Option<EntityAction> {
    let muted = theme.color(Token::TextMuted);
    ui.label(
        RichText::new(format_live(entities.len()))
            .small()
            .color(muted),
    );
    ui.add(
        egui::TextEdit::singleline(&mut outliner.filter)
            .hint_text(Text::FilterEntities.get())
            .desired_width(f32::INFINITY),
    );
    let selected = entities.selected().map(|s| s.edict);
    let mut action = None;
    egui::ScrollArea::vertical()
        .id_salt(Text::Entities.get())
        .max_height(ui.available_height() / 2.0)
        .show(ui, |ui| {
            for group in entities.groups(&outliner.filter) {
                let name = if group.classname.is_empty() {
                    Text::NoClassname.get().to_owned()
                } else {
                    group.classname.clone()
                };
                match group.edicts.as_slice() {
                    [edict] => {
                        let row = format!("{name}  #{edict}");
                        let response =
                            ui.selectable_label(selected == Some(Edict::new(*edict)), row);
                        if response.clicked() {
                            action = Some(EntityAction::Select(Edict::new(*edict)));
                        }
                        ask_menu(&response, *edict, &mut action);
                    }
                    edicts => {
                        let header = format!("{name}  {}", edicts.len());
                        egui::CollapsingHeader::new(header)
                            .id_salt(&group.classname)
                            .show(ui, |ui| {
                                for edict in edicts {
                                    let row = format!("#{edict}");
                                    let response = ui.selectable_label(
                                        selected == Some(Edict::new(*edict)),
                                        row,
                                    );
                                    if response.clicked() {
                                        action = Some(EntityAction::Select(Edict::new(*edict)));
                                    }
                                    ask_menu(&response, *edict, &mut action);
                                }
                            });
                    }
                }
            }
        });
    action
}

/// A row's right-click menu: Ask about this (UI-9).
fn ask_menu(response: &egui::Response, edict: u32, action: &mut Option<EntityAction>) {
    response.context_menu(|ui| {
        if ui.button(Text::AskAboutThis.get()).clicked() {
            *action = Some(EntityAction::Ask(Edict::new(edict)));
            ui.close();
        }
    });
}

/// The live count, as the outliner's heading shows it.
fn format_live(count: usize) -> String {
    format!("{count} {}", Text::Live.get())
}

/// The selected edict's fields: read only while the game runs, editable while it is paused,
/// a function's name opening its definition.
pub(crate) fn properties(
    ui: &mut Ui,
    theme: &Theme,
    preview: &Preview,
    state: &mut Properties,
) -> Option<EntityAction> {
    let entities = preview.entities();
    let Some(selected) = entities.selected() else {
        state.edits.clear();
        state.edict = None;
        ui.label(RichText::new(Text::NothingSelected.get()).color(theme.color(Token::TextMuted)));
        return None;
    };
    if state.edict != Some(selected.edict) {
        state.edits.clear();
        state.edict = Some(selected.edict);
    }
    let paused = preview.state() == PlayState::Paused;
    ui.label(
        RichText::new(format!("{}  #{}", selected.classname, selected.edict))
            .color(theme.color(Token::Accent)),
    );
    if !paused {
        ui.label(
            RichText::new(Text::ReadOnlyWhilePlaying.get())
                .small()
                .color(theme.color(Token::TextMuted)),
        );
    }
    let mut action = None;
    if let Some(spawn) = Some(selected.classname.clone()).filter(|c| !c.is_empty())
        && ui.link(Text::OpenSpawnFunction.get()).clicked()
    {
        action = Some(EntityAction::Open(spawn));
    }
    if ui.button(Text::AskAboutThis.get()).clicked() {
        action = Some(EntityAction::Ask(selected.edict));
    }
    egui::ScrollArea::vertical()
        .id_salt(Text::Properties.get())
        .show(ui, |ui| {
            egui::Grid::new(Text::Properties.get())
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    for field in &selected.fields {
                        ui.label(RichText::new(&field.name).monospace());
                        let shown = preview
                            .pending(selected.edict, &field.name)
                            .map_or_else(|| entities.show(&field.value), str::to_owned);
                        if paused {
                            let text = state
                                .edits
                                .entry(field.name.clone())
                                .or_insert_with(|| shown.clone());
                            let response = ui.add(
                                egui::TextEdit::singleline(text)
                                    .code_editor()
                                    .desired_width(f32::INFINITY),
                            );
                            let entered = response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if entered && *text != shown {
                                action = Some(EntityAction::SetField {
                                    field: field.name.clone(),
                                    value: text.clone(),
                                });
                            }
                        } else if field.is_function() && !shown.is_empty() {
                            if ui.link(RichText::new(&shown).monospace()).clicked() {
                                action = Some(EntityAction::Open(shown));
                            }
                        } else {
                            ui.label(RichText::new(shown).monospace());
                        }
                        ui.end_row();
                    }
                });
        });
    let edited: Vec<&str> = selected
        .fields
        .iter()
        .filter(|f| preview.pending(selected.edict, &f.name).is_some())
        .map(|f| f.name.as_str())
        .collect();
    if !edited.is_empty() {
        ui.label(
            RichText::new(format!(
                "{} {}",
                edited.join(", "),
                Text::SentOnResume.get()
            ))
            .small()
            .color(theme.color(Token::Accent)),
        );
    }
    if matches!(action, Some(EntityAction::SetField { .. })) {
        // The field is read again from the game; its text follows the new value.
        state.edits.clear();
    }
    action
}
