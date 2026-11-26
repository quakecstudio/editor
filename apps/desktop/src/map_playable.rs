// SPDX-License-Identifier: GPL-3.0-or-later

//! Whether the open map can be finished, in the map view (L-77, MAP-36): what `map.playability`
//! found after the last compile, kept until the map changes, said in the Properties panel under
//! the leak check, and each place a player cannot leave drawn in every pane, as a leak's trail
//! is: the box of the player's places in it, and a dot where a player comes into it.

use eframe::egui::{Painter, RichText, Stroke, Ui};
use qs_map_edit::{EntityIndex, MapRevision};
use qs_workbench::map_playable::{ExitReach, Playability};

use crate::map_panes::Projection;
use crate::strings::{Text, not_judged_count, trap_count, unreached_count};
use crate::theme::{Theme, Token};

/// The player's box below and above its origin, and half of it across (`VEC_HULL_MIN`,
/// `VEC_HULL_MAX`): a trap's box is drawn around the boxes of the places in it.
const FEET: f32 = 24.0;
const HEAD: f32 = 32.0;
const HALF: f32 = 16.0;
/// The width of a trap's lines, and the radius of its dot, in points.
const TRAP_WIDTH: f32 = 2.0;
const TRAP_DOT: f32 = 4.0;

/// What `map.playability` last found, and the map's edits it was found at.
#[derive(Debug, Default)]
pub(crate) struct PlayView {
    found: Option<(Playability<EntityIndex>, MapRevision)>,
}

impl PlayView {
    /// Takes what `map.playability` found in the map at `at`.
    pub(crate) fn set(&mut self, found: Playability<EntityIndex>, at: MapRevision) {
        self.found = Some((found, at));
    }

    /// Forgets what was found when the map is no longer at the edits it was found at.
    pub(crate) fn follow(&mut self, edits: MapRevision) {
        if self.found.as_ref().is_some_and(|(_, at)| *at != edits) {
            self.found = None;
        }
    }

    /// What was found, for tests.
    #[cfg(test)]
    pub(crate) fn found(&self) -> Option<&Playability<EntityIndex>> {
        self.found.as_ref().map(|(found, _)| found)
    }

    /// What was found, in the Properties panel: whether the exit is reached, and how many
    /// traps, unreached things and entities not judged there are.
    pub(crate) fn properties(&self, ui: &mut Ui, theme: &Theme) {
        let Some((found, _)) = &self.found else {
            return;
        };
        let (exit, token) = match found.exit {
            ExitReach::Reached => (Text::MapExitReached, Token::StatusOk),
            ExitReach::NotReached => (Text::MapExitNotReached, Token::StatusError),
            ExitReach::NoExit => (Text::MapNoExit, Token::StatusWarn),
        };
        ui.label(RichText::new(exit.get()).color(theme.color(token)));
        let error = theme.color(Token::StatusError);
        if !found.traps.is_empty() {
            ui.label(RichText::new(trap_count(found.traps.len())).color(error));
        }
        if !found.unreached.is_empty() {
            let warn = theme.color(Token::StatusWarn);
            ui.label(RichText::new(unreached_count(found.unreached.len())).color(warn));
        }
        if !found.not_judged.is_empty() {
            let muted = theme.color(Token::TextMuted);
            ui.label(RichText::new(not_judged_count(found.not_judged.len())).color(muted));
        }
        ui.separator();
    }

    /// Draws each trap through `projection`: the box around the player's places in it and a
    /// dot where a player comes into it.
    pub(crate) fn draw(&self, painter: &Painter, projection: &Projection, theme: &Theme) {
        let Some((found, _)) = &self.found else {
            return;
        };
        let stroke = Stroke::new(TRAP_WIDTH, theme.color(Token::StatusError));
        for trap in &found.traps {
            let [x0, y0, z0] = trap.min;
            let [x1, y1, z1] = trap.max;
            let (x0, y0, z0) = (x0 - HALF, y0 - HALF, z0 - FEET);
            let (x1, y1, z1) = (x1 + HALF, y1 + HALF, z1 + HEAD);
            // The box's edges: from each corner of its least face up, and round both faces.
            let face = |z: f32| [[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]];
            let (bottom, top) = (face(z0), face(z1));
            let after = bottom
                .iter()
                .cycle()
                .skip(1)
                .zip(top.iter().cycle().skip(1));
            for ((&low, &high), (&low_next, &high_next)) in bottom.iter().zip(&top).zip(after) {
                projection.line(painter, [low, high], stroke);
                projection.line(painter, [low, low_next], stroke);
                projection.line(painter, [high, high_next], stroke);
            }
            if let Some(at) = projection.point(trap.at) {
                painter.circle_filled(at, TRAP_DOT, stroke.color);
            }
        }
    }
}
