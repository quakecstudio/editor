// SPDX-License-Identifier: GPL-3.0-or-later

//! The issue browser and map search (P6-19, MAP-28), in the tab below the map view: what
//! `map.issues` last found, each issue with the entity it is about, which a click selects, or
//! the place it is about, such as a trap (`map.playability`), which a click frames, and
//! its quick fix as a button; and a search box taking text or a query in TrenchBroom's query
//! language, whose matches Find selects, each also selecting its own. Each is a [`MapAction`]
//! the shell runs as a command.

use eframe::egui::{self, RichText, Ui};
use qs_map_edit::uv::FaceRef;
use qs_map_edit::{EntityIndex, MapFound, MapIssue, MapRevision, MapSearchParams};

use crate::map_browser::FILTER_WIDTH;
use crate::map_editor::MapAction;
use crate::strings::{
    Text, brush_place, entity_place, face_place, fix_label, issue_count, issue_line,
    map_found_count,
};
use crate::theme::{Theme, Token};

/// The issue browser's state: the issues last found and the search.
#[derive(Debug, Default)]
pub(crate) struct IssueBrowser {
    /// The issues `map.issues` last gave; none before the map is checked.
    issues: Option<Vec<MapIssue>>,
    /// The search as typed.
    query: String,
    /// What the last search found.
    found: Option<MapFound>,
    /// The map's edits the issues and matches were found at.
    seen: Option<MapRevision>,
    /// The place of an issue chosen that is about a place rather than an entity, for the view
    /// to frame.
    look: Option<[f64; 3]>,
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

    /// The place of the issue last chosen that is about a place, once.
    pub(crate) const fn take_look(&mut self) -> Option<[f64; 3]> {
        self.look.take()
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
    fn issue_list(&mut self, ui: &mut Ui, theme: &Theme) -> Vec<MapAction> {
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
        let mut look = None;
        egui::ScrollArea::vertical()
            .id_salt(ui.scope_id().with(core::any::TypeId::of::<MapIssue>()))
            .show_rows(ui, row, issues.len(), |ui, shown| {
                for issue in issues.get(shown).unwrap_or_default() {
                    ui.horizontal(|ui| {
                        if let Some(fix) = &issue.fix
                            && ui.button(fix_label(fix)).clicked()
                        {
                            actions.push(MapAction::Fix(fix.clone()));
                        }
                        let text = issue_line(issue.line, &issue.message);
                        match (issue.entity, issue.point) {
                            (Some(entity), _) => {
                                let button = egui::Button::new(text).frame(false).truncate();
                                if ui.add(button).clicked() {
                                    actions.push(MapAction::Select(Some(entity)));
                                }
                            }
                            (None, Some(point)) => {
                                let button = egui::Button::new(text).frame(false).truncate();
                                if ui.add(button).clicked() {
                                    look = Some(point);
                                }
                            }
                            (None, None) => {
                                ui.add(egui::Label::new(text).truncate());
                            }
                        }
                    });
                }
            });
        if look.is_some() {
            self.look = look;
        }
        actions
    }

    /// The search box, its Find button and what it found, each row selecting its own.
    fn search(&mut self, ui: &mut Ui, theme: &Theme) -> Vec<MapAction> {
        let mut actions = Vec::new();
        let field = ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text(Text::MapSearchQuery.get())
                .desired_width(FILTER_WIDTH),
        );
        let field = field.accessible_name(Text::MapSearchQuery.get());
        let field = field.on_hover_text(Text::MapSearchSyntax.get());
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button(Text::Find.get()).clicked() || entered {
            actions.push(MapAction::Find(MapSearchParams {
                query: self.query.clone(),
            }));
        }
        let Some(found) = &self.found else {
            return actions;
        };
        ui.label(
            RichText::new(map_found_count(
                found.entities.len(),
                found.brushes.len(),
                found.faces.len(),
            ))
            .color(theme.color(Token::TextMuted)),
        );
        let rows: Vec<(Found, String)> = found
            .entities
            .iter()
            .map(|&entity| (Found::Entity(entity), entity_place(entity.get())))
            .chain(
                found
                    .brushes
                    .iter()
                    .map(|&(entity, brush)| (Found::Entity(entity), brush_place(entity, brush))),
            )
            .chain(
                found
                    .faces
                    .iter()
                    .map(|&face| (Found::Face(face), face_place(face))),
            )
            .collect();
        let row = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical()
            .id_salt(ui.scope_id().with(core::any::TypeId::of::<MapFound>()))
            .show_rows(ui, row, rows.len(), |ui, shown| {
                for (place, text) in rows.get(shown).unwrap_or_default() {
                    if ui.add(egui::Button::new(text).frame(false)).clicked() {
                        actions.push(match *place {
                            Found::Entity(entity) => MapAction::Select(Some(entity)),
                            Found::Face(face) => MapAction::SelectFound(MapFound {
                                faces: vec![face],
                                ..MapFound::default()
                            }),
                        });
                    }
                }
            });
        actions
    }
}

/// What a row of the search's matches selects.
#[derive(Clone, Copy, Debug)]
enum Found {
    /// An entity, or the entity of a brush.
    Entity(EntityIndex),
    /// A face.
    Face(FaceRef),
}

#[cfg(test)]
mod tests;
