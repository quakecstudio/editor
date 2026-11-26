// SPDX-License-Identifier: GPL-3.0-or-later

//! The Entities outliner and the Properties panel (P3-13 to P3-15): the running game's edicts by
//! classname with a filter, and the selected edict's fields, editable while the game is paused.
//! What a click asks for is returned to the shell, which runs it through its command.

use std::collections::BTreeMap;

use eframe::egui::{self, RichText, Ui};
use qs_workbench::entities::Entities;
use qs_workbench::{PlayState, Preview};

use crate::icons::{self, Icon, Toggled};
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

/// The text in a field's box while the game is paused, and the game's value it was made
/// from.
#[derive(Debug)]
struct Edit {
    text: String,
    from: String,
}

/// The Properties panel's own state: the text of each field being edited.
#[derive(Debug, Default)]
pub(crate) struct Properties {
    edits: BTreeMap<String, Edit>,
    /// The edict the edits are for; another selection drops them.
    edict: Option<Edict>,
}

impl Properties {
    /// Keeps the boxes' texts for `edict` while the game is paused, and for nothing else:
    /// what was typed is for one pause of one edict, so a box never shows a value read
    /// before the game last ran. The window calls it every frame, whether or not the panel
    /// is drawn, so a pause the panel did not see ends the texts too.
    pub(crate) fn follow(&mut self, edict: Option<Edict>, game: PlayState) {
        if self.edict != edict || game != PlayState::Paused {
            self.edits.clear();
        }
        self.edict = edict;
    }

    /// The text of `field`'s box, the game's value `shown` until something is typed in it,
    /// and again once the game's value changes under it, as a step or a set field changes
    /// it.
    fn text(&mut self, field: &str, shown: &str) -> &mut String {
        let fresh = || Edit {
            text: shown.to_owned(),
            from: shown.to_owned(),
        };
        let edit = self.edits.entry(field.to_owned()).or_insert_with(fresh);
        if edit.from != shown {
            *edit = fresh();
        }
        &mut edit.text
    }
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
                let icon = Icon::of_class(&group.classname);
                match group.edicts.as_slice() {
                    [edict] => {
                        let row = format!("{name}  #{edict}");
                        let response = icon.row(ui, chosen(selected, *edict), &row);
                        if response.clicked() {
                            action = Some(EntityAction::Select(Edict::new(*edict)));
                        }
                        ask_menu(&response, *edict, &mut action);
                    }
                    edicts => {
                        let title = format!("{name}  {}", edicts.len());
                        let header = egui::CollapsingHeader::new(icon.before(&title))
                            .id_salt(&group.classname)
                            .show(ui, |ui| {
                                for edict in edicts {
                                    let row = format!("#{edict}");
                                    let response = icon.row(ui, chosen(selected, *edict), &row);
                                    if response.clicked() {
                                        action = Some(EntityAction::Select(Edict::new(*edict)));
                                    }
                                    ask_menu(&response, *edict, &mut action);
                                }
                            });
                        icons::mark(ui, &header.header_response, &title, icons::ROW);
                    }
                }
            }
        });
    action
}

/// Whether `edict` is the selected one.
fn chosen(selected: Option<Edict>, edict: u32) -> Toggled {
    if selected == Some(Edict::new(edict)) {
        Toggled::On
    } else {
        Toggled::Off
    }
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

/// What the Properties panel shows of a game: its entities, whether it runs or is paused,
/// and the value typed for a field of an edict while paused that the game has yet to take.
pub(crate) struct Shown<'a> {
    pub(crate) entities: &'a Entities,
    pub(crate) game: PlayState,
    pub(crate) pending: &'a dyn Fn(Edict, &str) -> Option<String>,
}

impl<'a> Shown<'a> {
    /// What the panel shows of `preview`'s game, `pending` its lookup of a typed value.
    pub(crate) fn of(
        preview: &'a Preview,
        pending: &'a dyn Fn(Edict, &str) -> Option<String>,
    ) -> Self {
        Self {
            entities: preview.entities(),
            game: preview.state(),
            pending,
        }
    }
}

/// The selected edict's fields: read only while the game runs, editable while it is paused,
/// a function's name opening its definition.
pub(crate) fn properties(
    ui: &mut Ui,
    theme: &Theme,
    shown: &Shown<'_>,
    state: &mut Properties,
) -> Option<EntityAction> {
    let Shown {
        entities,
        game,
        pending,
    } = *shown;
    let Some(selected) = entities.selected() else {
        state.follow(None, game);
        ui.label(RichText::new(Text::NothingSelected.get()).color(theme.color(Token::TextMuted)));
        return None;
    };
    state.follow(Some(selected.edict), game);
    let paused = game == PlayState::Paused;
    ui.label(
        RichText::new(format!("{}  #{}", selected.classname, selected.edict))
            .color(theme.color(Token::Accent)),
    );
    let mut action = None;
    // The entity's actions as icons, and a lock while its fields cannot be edited.
    ui.horizontal(|ui| {
        if let Some(spawn) = Some(selected.classname.clone()).filter(|c| !c.is_empty())
            && Icon::OpenCode.tool(ui, Text::OpenSpawnFunction).clicked()
        {
            action = Some(EntityAction::Open(spawn));
        }
        if Icon::Agent.tool(ui, Text::AskAboutThis).clicked() {
            action = Some(EntityAction::Ask(selected.edict));
        }
        if !paused {
            // A lock, read as the sentence it stands for.
            let muted = theme.color(Token::TextMuted);
            Icon::Lock.sign(ui, muted, Text::ReadOnlyWhilePlaying);
        }
    });
    egui::ScrollArea::vertical()
        .id_salt(Text::Properties.get())
        .show(ui, |ui| {
            egui::Grid::new(Text::Properties.get())
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    for field in &selected.fields {
                        ui.label(RichText::new(&field.name).monospace());
                        let shown = pending(selected.edict, &field.name)
                            .unwrap_or_else(|| entities.show(&field.value));
                        if paused {
                            let text = state.text(&field.name, &shown);
                            let response = ui.add(
                                egui::TextEdit::singleline(text)
                                    .code_editor()
                                    .desired_width(f32::INFINITY),
                            );
                            let response = response.accessible_name(&field.name);
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
        .filter(|f| pending(selected.edict, &f.name).is_some())
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
    action
}

#[cfg(test)]
mod tests;
