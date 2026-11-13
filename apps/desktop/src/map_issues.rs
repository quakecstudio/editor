// SPDX-License-Identifier: GPL-3.0-or-later

//! The issue browser and map search (P6-19, MAP-28), in the tab below the map view: what
//! `map.issues` last found, each issue with the entity it is about, which a click selects, and
//! its quick fix as a button; and a search by class, key, value and texture whose matches
//! select the same way. Each is a [`MapAction`] the shell runs as a command.

use eframe::egui::{self, RichText, Ui};
use qs_workbench::map_edit::{EntityIndex, MapFound, MapIssue, MapRevision, MapSearchParams};

use crate::map_browser::FILTER_WIDTH;
use crate::map_editor::MapAction;
use crate::strings::{
    Text, brush_place, entity_place, fix_label, issue_count, issue_line, map_found_count,
};
use crate::theme::{Theme, Token};

/// The search's fields, as typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SearchFields {
    classname: String,
    key: String,
    value: String,
    texture: String,
    /// A box as six numbers: the least corner, then the greatest.
    within: String,
}

impl SearchFields {
    /// The search the fields ask for: each part typed, trimmed; the box when it is six
    /// numbers.
    fn params(&self) -> MapSearchParams {
        let part = |text: &str| {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_owned())
        };
        let numbers: Vec<f64> = self
            .within
            .split_whitespace()
            .map_while(|word| word.parse().ok())
            .collect();
        let within = match numbers.as_slice() {
            &[x0, y0, z0, x1, y1, z1] => Some([[x0, y0, z0], [x1, y1, z1]]),
            _ => None,
        };
        MapSearchParams {
            classname: part(&self.classname),
            key: part(&self.key),
            value: part(&self.value),
            texture: part(&self.texture),
            within,
        }
    }
}

/// The issue browser's state: the issues last found and the search.
#[derive(Debug, Default)]
pub(crate) struct IssueBrowser {
    /// The issues `map.issues` last gave; none before the map is checked.
    issues: Option<Vec<MapIssue>>,
    fields: SearchFields,
    /// What the last search found.
    found: Option<MapFound>,
    /// The map's edits the issues and matches were found at.
    seen: Option<MapRevision>,
}

impl IssueBrowser {
    /// Takes the issues `map.issues` found in the map at `at`.
    pub(crate) fn set_issues(&mut self, issues: Vec<MapIssue>, at: MapRevision) {
        self.follow(at);
        self.issues = Some(issues);
        self.seen = Some(at);
    }

    /// Takes what `map.search` found in the map at `at`.
    pub(crate) fn set_found(&mut self, found: MapFound, at: MapRevision) {
        self.follow(at);
        self.found = Some(found);
        self.seen = Some(at);
    }

    /// Forgets the issues and matches when the map is no longer at the edits they were found
    /// at: their places may name other entities and brushes now.
    pub(crate) fn follow(&mut self, edits: MapRevision) {
        if self.seen.is_some_and(|seen| seen != edits) {
            self.clear();
        }
    }

    /// Forgets the issues and the search's matches.
    pub(crate) fn clear(&mut self) {
        self.issues = None;
        self.found = None;
        self.seen = None;
    }

    /// The header's part: the Check button and how many issues were found.
    pub(crate) fn header(&self, ui: &mut Ui, theme: &Theme) -> Vec<MapAction> {
        let mut actions = Vec::new();
        if ui.button(Text::MapCheckIssues.get()).clicked() {
            actions.push(MapAction::CheckIssues);
        }
        if let Some(issues) = &self.issues {
            let fixable = issues.iter().filter(|issue| issue.fix.is_some()).count();
            ui.label(
                RichText::new(issue_count(issues.len(), fixable))
                    .color(theme.color(Token::TextMuted)),
            );
        }
        actions
    }

    /// The issues, each with its Fix button, beside the search and its matches.
    pub(crate) fn body(&mut self, ui: &mut Ui, theme: &Theme) -> Vec<MapAction> {
        let mut actions = Vec::new();
        ui.columns(2, |columns| {
            if let [left, right] = columns {
                actions.extend(self.issue_list(left, theme));
                actions.extend(self.search(right, theme));
            }
        });
        actions
    }

    /// The issues found, or a prompt to check the map; only the rows in view are laid out.
    fn issue_list(&self, ui: &mut Ui, theme: &Theme) -> Vec<MapAction> {
        let mut actions = Vec::new();
        let Some(issues) = &self.issues else {
            ui.label(RichText::new(Text::MapIssuesHint.get()).color(theme.color(Token::TextMuted)));
            return actions;
        };
        if issues.is_empty() {
            ui.label(RichText::new(Text::MapNoIssues.get()).color(theme.color(Token::TextMuted)));
            return actions;
        }
        let row = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical()
            .id_salt(ui.id().with(core::any::TypeId::of::<MapIssue>()))
            .show_rows(ui, row, issues.len(), |ui, shown| {
                for issue in issues.get(shown).unwrap_or_default() {
                    ui.horizontal(|ui| {
                        if let Some(fix) = &issue.fix
                            && ui.button(fix_label(fix)).clicked()
                        {
                            actions.push(MapAction::Fix(fix.clone()));
                        }
                        let text = issue_line(issue.line, &issue.message);
                        match issue.entity {
                            Some(entity) => {
                                let button = egui::Button::new(text).frame(false).truncate();
                                if ui.add(button).clicked() {
                                    actions.push(MapAction::Select(Some(entity)));
                                }
                            }
                            None => {
                                ui.add(egui::Label::new(text).truncate());
                            }
                        }
                    });
                }
            });
        actions
    }

    /// The search's fields, its Find button and what it found, each selecting its entity.
    fn search(&mut self, ui: &mut Ui, theme: &Theme) -> Vec<MapAction> {
        let mut actions = Vec::new();
        let mut entered = false;
        let fields = &mut self.fields;
        for (label, text) in [
            (Text::MapSearchClass, &mut fields.classname),
            (Text::MapSearchKey, &mut fields.key),
            (Text::MapSearchValue, &mut fields.value),
            (Text::MapSearchTexture, &mut fields.texture),
            (Text::MapSearchWithin, &mut fields.within),
        ] {
            let field = ui.add(
                egui::TextEdit::singleline(text)
                    .hint_text(label.get())
                    .desired_width(FILTER_WIDTH),
            );
            entered |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        }
        ui.horizontal(|ui| {
            if ui.button(Text::Find.get()).clicked() || entered {
                actions.push(MapAction::Find(self.fields.params()));
            }
        });
        let Some(found) = &self.found else {
            return actions;
        };
        ui.label(
            RichText::new(map_found_count(found.entities.len(), found.brushes.len()))
                .color(theme.color(Token::TextMuted)),
        );
        let rows: Vec<(EntityIndex, String)> = found
            .entities
            .iter()
            .map(|&entity| (entity, entity_place(entity.get())))
            .chain(
                found
                    .brushes
                    .iter()
                    .map(|&(entity, brush)| (entity, brush_place(entity, brush))),
            )
            .collect();
        let row = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical()
            .id_salt(ui.id().with(core::any::TypeId::of::<MapFound>()))
            .show_rows(ui, row, rows.len(), |ui, shown| {
                for (entity, text) in rows.get(shown).unwrap_or_default() {
                    if ui.add(egui::Button::new(text).frame(false)).clicked() {
                        actions.push(MapAction::Select(Some(*entity)));
                    }
                }
            });
        actions
    }
}

#[cfg(test)]
mod tests;
