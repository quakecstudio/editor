// SPDX-License-Identifier: GPL-3.0-or-later

//! The Entities outliner and the Properties panel (P3-13 to P3-15): the running game's edicts by
//! classname with a filter, and the selected edict's fields, editable while the game is paused.
//! What a click asks for is returned to the shell, which runs it through its command.

use std::borrow::Cow;
use std::collections::BTreeMap;

use eframe::egui::{self, RichText, Ui};
use qs_workbench::entities::Entities;
use qs_workbench::tools::FunctionFound;
use qs_workbench::{PlayState, Preview};

use crate::icons::{self, Icon, Toggled};
use crate::strings::Text;
use crate::suggest::{Offer, Offers, Suggest, Taken};
use crate::theme::{Theme, Token};
use qs_workbench::ids::Edict;

/// The width of an entity or function field's box, in points.
const OFFER_WIDTH: f32 = 180.0;

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

/// The text of each field being edited, by field.
#[derive(Debug, Default)]
struct Edits(BTreeMap<String, Edit>);

impl Edits {
    /// The text of `field`'s box, the game's value `shown` until something is typed in it,
    /// and again once the game's value changes under it, as a step or a set field changes
    /// it.
    fn text(&mut self, field: &str, shown: &str) -> &mut String {
        let fresh = || Edit {
            text: shown.to_owned(),
            from: shown.to_owned(),
        };
        let edit = self.0.entry(field.to_owned()).or_insert_with(fresh);
        if edit.from != shown {
            *edit = fresh();
        }
        &mut edit.text
    }
}

/// The Properties panel's own state: the text of each field being edited, and the functions a
/// function field offers.
#[derive(Debug, Default)]
pub(crate) struct Properties {
    edits: Edits,
    /// The edict the edits are for; another selection drops them.
    edict: Option<Edict>,
    /// The functions of the project's server QuakeC, each with its file, read once a pause
    /// (UI-24); `None` until then.
    functions: Option<Vec<Offer>>,
}

impl Properties {
    /// Keeps the boxes' texts for `edict` while the game is paused, and for nothing else:
    /// what was typed is for one pause of one edict, so a box never shows a value read
    /// before the game last ran. The window calls it every frame, whether or not the panel
    /// is drawn, so a pause the panel did not see ends the texts too.
    pub(crate) fn follow(&mut self, edict: Option<Edict>, game: PlayState) {
        if self.edict != edict || game != PlayState::Paused {
            self.edits.0.clear();
        }
        if game != PlayState::Paused {
            self.functions = None;
        }
        self.edict = edict;
    }

    /// Whether the functions a function field offers are to be read: the game is paused and
    /// they were not read in this pause, so a reload's are offered at the next.
    pub(crate) const fn functions_unread(&self, game: PlayState) -> bool {
        matches!(game, PlayState::Paused) && self.functions.is_none()
    }

    /// Keeps `functions`, which a function field offers until the game runs again.
    pub(crate) fn set_functions(&mut self, functions: Vec<FunctionFound>) {
        let offers = functions
            .into_iter()
            .map(|function| Offer {
                name: function.name,
                detail: function.file,
            })
            .collect();
        self.functions = Some(offers);
    }

    /// The text of `field`'s box, the game's value `shown` until something is typed in it,
    /// and again once the game's value changes under it, as a step or a set field changes
    /// it.
    #[cfg(test)]
    fn text(&mut self, field: &str, shown: &str) -> &mut String {
        self.edits.text(field, shown)
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

    /// The edicts in use as an entity field offers them while paused (UI-24), written as the
    /// panel writes an entity: `monster_army #7`.
    fn edicts(&self) -> Vec<Offer> {
        self.entities
            .groups("")
            .into_iter()
            .flat_map(|group| {
                group.edicts.into_iter().map(move |edict| Offer {
                    name: format!("{} #{edict}", group.classname),
                    detail: String::new(),
                })
            })
            .collect()
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
    // Made only while a field is edited (UI-24): the edicts in use, and the server's
    // functions, read once a pause.
    let edicts = |_: &str| Cow::Owned(shown.edicts());
    let functions = state.functions.as_deref().unwrap_or_default();
    let functions = |_: &str| Cow::Borrowed(functions);
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
                            let text = state.edits.text(&field.name, &shown);
                            let offers: Option<Offers<'_>> = if field.is_entity() {
                                Some(Offers::Made(&edicts))
                            } else if field.is_function() {
                                Some(Offers::Made(&functions))
                            } else {
                                None
                            };
                            let set = match offers {
                                Some(offers) => {
                                    let field = Suggest {
                                        name: &field.name,
                                        offers,
                                        width: OFFER_WIDTH,
                                    };
                                    let entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                    match field.take(ui, theme, text).1 {
                                        Some(Taken::Offer(value)) => Some(value),
                                        Some(Taken::Typed(value)) if entered => Some(value),
                                        Some(Taken::Typed(_)) | None => None,
                                    }
                                }
                                None => {
                                    let response = ui.add(
                                        egui::TextEdit::singleline(text)
                                            .code_editor()
                                            .desired_width(f32::INFINITY),
                                    );
                                    let response = response.accessible_name(&field.name);
                                    let entered = response.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                    entered.then(|| text.clone())
                                }
                            };
                            if let Some(value) = set.filter(|value| *value != shown) {
                                action = Some(EntityAction::SetField {
                                    field: field.name.clone(),
                                    value,
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
