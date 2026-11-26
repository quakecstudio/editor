// SPDX-License-Identifier: GPL-3.0-or-later

//! The Logic tab below the map view (L-76, MAP-35): Fire runs the selected entity's `use` or
//! `touch` in the simulation with the project's QuakeC (`map.fire`) and lists what the code did
//! in order, each step selecting its entity; a scrubber steps through the trace, and the map
//! view lights the links fired up to the step shown. Check logic lists the issues only running
//! shows (`map.logic_issues`). Each is a [`MapAction`] the shell runs as a command.

use eframe::egui::{self, RichText, Ui};
use qs_map_edit::{EntityIndex, MapRevision};
use qs_workbench::map_logic::{FiredEvent, LogicIssue, LogicIssues, MapFired};

use crate::map_editor::MapAction;
use crate::strings::{Text, fire_summary, fired_line, logic_issue_count};
use crate::theme::{Theme, Token};

/// A firing and its trace, as the tab shows it.
#[derive(Debug)]
struct Shown {
    /// The entity fired.
    entity: EntityIndex,
    trace: MapFired,
    /// How many of the trace's events the scrubber shows as done.
    upto: usize,
}

/// The Logic tab's state: the last trace and the issues last found.
#[derive(Debug, Default)]
pub(crate) struct LogicPane {
    fired: Option<Shown>,
    issues: Option<Vec<LogicIssue>>,
    /// The map's edits the trace and the issues were found at.
    seen: Option<MapRevision>,
}

impl LogicPane {
    /// Takes the trace of `entity` fired in the map at `at`.
    pub(crate) fn set_fired(&mut self, entity: EntityIndex, trace: MapFired, at: MapRevision) {
        self.follow(at);
        self.fired = Some(Shown {
            entity,
            upto: trace.events.len(),
            trace,
        });
        self.seen = Some(at);
    }

    /// Takes the issues `map.logic_issues` found in the map at `at`.
    pub(crate) fn set_issues(&mut self, found: LogicIssues, at: MapRevision) {
        self.follow(at);
        self.issues = Some(found.issues);
        self.seen = Some(at);
    }

    /// Forgets the trace and the issues when the map is no longer at the edits they were found
    /// at: their places may name other entities now.
    pub(crate) fn follow(&mut self, edits: MapRevision) {
        if self.seen.is_some_and(|seen| seen != edits) {
            self.clear();
        }
    }

    /// Forgets the trace and the issues.
    pub(crate) fn clear(&mut self) {
        self.fired = None;
        self.issues = None;
        self.seen = None;
    }

    /// The links the trace has fired up to the step the scrubber shows: each from the entity
    /// whose targets were looked up to the entity found, in the order fired.
    pub(crate) fn lit(&self) -> Vec<(EntityIndex, EntityIndex)> {
        let Some(shown) = &self.fired else {
            return Vec::new();
        };
        shown
            .trace
            .events
            .iter()
            .take(shown.upto)
            .filter_map(|event| match event {
                FiredEvent::Looked {
                    by,
                    found: Some(found),
                    ..
                } => Some((by.entity?, found.entity?)),
                _ => None,
            })
            .collect()
    }

    /// The header's part: Fire for the one entity selected, Check logic, and what the last
    /// trace came to.
    pub(crate) fn header(
        &self,
        ui: &mut Ui,
        theme: &Theme,
        selected: Option<EntityIndex>,
    ) -> Vec<MapAction> {
        let mut actions = Vec::new();
        let fire = ui
            .add_enabled(selected.is_some(), egui::Button::new(Text::MapFire.get()))
            .on_hover_text(Text::MapFireHint.get())
            .on_disabled_hover_text(Text::MapFireHint.get());
        if let Some(entity) = selected
            && fire.clicked()
        {
            actions.push(MapAction::Fire(entity));
        }
        if ui
            .button(Text::MapCheckLogic.get())
            .on_hover_text(Text::MapCheckLogicHint.get())
            .clicked()
        {
            actions.push(MapAction::CheckLogic);
        }
        if let Some(shown) = &self.fired {
            ui.label(
                RichText::new(fire_summary(shown.entity, &shown.trace))
                    .color(theme.color(Token::TextMuted)),
            );
        }
        actions
    }

    /// The trace under its scrubber, beside the issues.
    pub(crate) fn body(&mut self, ui: &mut Ui, theme: &Theme) -> Vec<MapAction> {
        let mut actions = Vec::new();
        ui.columns(2, |columns| {
            if let [left, right] = columns {
                actions.extend(self.trace(left, theme));
                actions.extend(self.issue_list(right, theme));
            }
        });
        actions
    }

    /// The trace's events, those after the scrubber's step muted; a row selects its entity.
    fn trace(&mut self, ui: &mut Ui, theme: &Theme) -> Vec<MapAction> {
        let mut actions = Vec::new();
        let Some(shown) = &mut self.fired else {
            ui.label(RichText::new(Text::MapFireNone.get()).color(theme.color(Token::TextMuted)));
            return actions;
        };
        let count = shown.trace.events.len();
        ui.add(
            egui::Slider::new(&mut shown.upto, 0..=count)
                .text(Text::MapFireStep.get())
                .integer(),
        )
        .on_hover_text(Text::MapFireStepHint.get());
        let upto = shown.upto;
        let row = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical()
            .id_salt(ui.scope_id().with(core::any::TypeId::of::<FiredEvent>()))
            .show_rows(ui, row, count, |ui, rows| {
                let first = rows.start;
                let events = shown.trace.events.get(rows).unwrap_or_default();
                for (index, event) in (first..).zip(events) {
                    let (text, entity) = fired_line(event);
                    let color = if index < upto {
                        theme.color(Token::Text)
                    } else {
                        theme.color(Token::TextMuted)
                    };
                    let text = RichText::new(text).color(color);
                    match entity {
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
                }
            });
        actions
    }

    /// The issues found, or a prompt to check; a row selects the entity it is about.
    fn issue_list(&self, ui: &mut Ui, theme: &Theme) -> Vec<MapAction> {
        let mut actions = Vec::new();
        let Some(issues) = &self.issues else {
            ui.label(RichText::new(Text::MapLogicHint.get()).color(theme.color(Token::TextMuted)));
            return actions;
        };
        ui.label(
            RichText::new(logic_issue_count(issues.len())).color(theme.color(Token::TextMuted)),
        );
        let row = ui.spacing().interact_size.y;
        egui::ScrollArea::vertical()
            .id_salt(ui.scope_id().with(core::any::TypeId::of::<LogicIssue>()))
            .show_rows(ui, row, issues.len(), |ui, rows| {
                for issue in issues.get(rows).unwrap_or_default() {
                    let button = egui::Button::new(issue.message.as_str())
                        .frame(false)
                        .truncate();
                    if ui.add(button).clicked() {
                        actions.push(MapAction::Select(Some(issue.entity)));
                    }
                }
            });
        actions
    }
}
