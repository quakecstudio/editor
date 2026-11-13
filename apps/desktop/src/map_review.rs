// SPDX-License-Identifier: GPL-3.0-or-later

//! The agent's map edits waiting for review in the map view (P6-21, MAP-29): each hunk of
//! `map.staged`, what it changes and where, with Accept and Reject, and both for every hunk at
//! once, as the agent's code edits are reviewed.

use eframe::egui::{RichText, Ui};
use qs_workbench::map_edit::{MapHunk, MapHunkNumber, MapRevision};

use crate::map_editor::MapAction;
use crate::strings::{Text, map_hunk};
use crate::theme::{Theme, Token};

/// What the review asks for: hunks accepted or rejected, every one when none is named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewAction {
    /// Keep these hunks.
    Accept(Vec<MapHunkNumber>),
    /// Put back what these hunks replaced.
    Reject(Vec<MapHunkNumber>),
}

/// The hunks waiting, read at a revision of the map's edits.
#[derive(Debug, Default)]
pub(crate) struct ReviewView {
    hunks: Vec<MapHunk>,
    read_at: Option<MapRevision>,
}

impl ReviewView {
    /// Whether the hunks were read before the map's edits `edits`.
    pub(crate) fn stale(&self, edits: MapRevision) -> bool {
        self.read_at != Some(edits)
    }

    /// Takes the hunks waiting, read at `edits`.
    pub(crate) fn set_hunks(&mut self, hunks: Vec<MapHunk>, edits: Option<MapRevision>) {
        self.hunks = hunks;
        self.read_at = edits;
    }

    /// How many hunks wait, for tests.
    #[cfg(test)]
    pub(crate) fn waiting(&self) -> usize {
        self.hunks.len()
    }

    /// The hunks in the Properties panel, when any wait; what was asked for.
    pub(crate) fn properties(&self, ui: &mut Ui, theme: &Theme) -> Vec<MapAction> {
        let mut actions = Vec::new();
        if self.hunks.is_empty() {
            return actions;
        }
        ui.label(RichText::new(Text::MapReviewHeading.get()).strong());
        for hunk in &self.hunks {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(map_hunk(hunk)).color(theme.color(Token::StatusWarn)));
                if ui.small_button(Text::AgentAccept.get()).clicked() {
                    actions.push(MapAction::Review(ReviewAction::Accept(vec![hunk.index])));
                }
                if ui.small_button(Text::AgentReject.get()).clicked() {
                    actions.push(MapAction::Review(ReviewAction::Reject(vec![hunk.index])));
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(Text::AgentAcceptAll.get()).clicked() {
                actions.push(MapAction::Review(ReviewAction::Accept(Vec::new())));
            }
            if ui.button(Text::AgentRejectAll.get()).clicked() {
                actions.push(MapAction::Review(ReviewAction::Reject(Vec::new())));
            }
        });
        ui.separator();
        actions
    }
}
