// SPDX-License-Identifier: GPL-3.0-or-later

//! The map outline (P5-16, MAP-31), in the left panel where the mockup has its Map section:
//! every layer, group and entity of the open map in a tree, filtered by text. Choosing a row
//! selects the entity and frames it in the view; the selection's row opens and scrolls into
//! sight; a row dragged onto a layer or group moves into it.

use eframe::egui::{self, RichText, Sense, Ui};
use qs_workbench::map_edit::{EntityIndex, MapRevision, OutlineRow, RowKind};

use crate::strings::{Text, brush_count};
use crate::theme::{Theme, Token};

/// The width of the filter field, in points.
const FILTER_WIDTH: f32 = 180.0;
/// How many frames the outline scrolls to a new selection's row, so the rows around it have
/// opened and been laid out by the last.
const REVEAL_FRAMES: u8 = 8;

/// What the outline asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OutlineAction {
    /// Select the entity and frame it in the view.
    Choose(EntityIndex),
    /// Move the entity or group into the layer or group, the world for the default layer.
    MoveTo(EntityIndex, EntityIndex),
}

/// What a dragged row carries: its entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RowDrag(EntityIndex);

/// The outline's state: its rows, the filter typed, and what they were read for.
#[derive(Debug, Default)]
pub(crate) struct OutlineView {
    rows: Vec<OutlineRow>,
    filter: String,
    /// The map's revision and the filter the rows were read for.
    read: Option<(MapRevision, String)>,
    /// The selection the outline last scrolled to, and the frames it still scrolls to it.
    revealed: Option<EntityIndex>,
    revealing: u8,
}

impl OutlineView {
    /// The filter to read the rows with, when they were read for other edits or another
    /// filter.
    pub(crate) fn stale(&self, revision: MapRevision) -> Option<String> {
        let wanted = (revision, self.filter.trim().to_owned());
        (self.read.as_ref() != Some(&wanted)).then_some(wanted.1)
    }

    /// Takes the rows read at `revision` with `filter`.
    pub(crate) fn set_rows(
        &mut self,
        rows: Vec<OutlineRow>,
        revision: MapRevision,
        filter: String,
    ) {
        self.rows = rows;
        self.read = Some((revision, filter));
    }

    /// Draws the outline with `selected` marked; what was asked for.
    pub(crate) fn ui(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        selected: Option<EntityIndex>,
    ) -> Vec<OutlineAction> {
        let mut actions = Vec::new();
        ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text(Text::MapFilterOutline.get())
                .desired_width(FILTER_WIDTH),
        );
        if selected != self.revealed {
            self.revealed = selected;
            self.revealing = REVEAL_FRAMES;
        }
        let reveal = (self.revealing > 0).then_some(selected).flatten();
        // The opening rows' animation repaints the frames this counts down through.
        self.revealing = self.revealing.saturating_sub(1);
        for row in &self.rows {
            Self::row_ui(ui, theme, row, selected, reveal, &mut actions);
        }
        actions
    }

    /// One row and, for a layer or group, those under it.
    fn row_ui(
        ui: &mut Ui,
        theme: &Theme,
        row: &OutlineRow,
        selected: Option<EntityIndex>,
        reveal: Option<EntityIndex>,
        actions: &mut Vec<OutlineAction>,
    ) {
        let label = match (row.kind, row.label.is_empty()) {
            (RowKind::Layer, true) => Text::MapDefaultLayer.get().to_owned(),
            _ => row.label.clone(),
        };
        let mut text = RichText::new(label).monospace();
        if row.kind == RowKind::LinkedGroup {
            text = text.italics();
        }
        let chosen = selected == Some(row.entity);
        let response = if row.rows.is_empty() || row.kind == RowKind::Entity {
            let line = ui.horizontal(|ui| {
                let response = ui.selectable_label(chosen, text);
                if let Some(name) = &row.targetname {
                    ui.label(RichText::new(name).color(theme.color(Token::Accent)));
                }
                if row.brushes > 0 {
                    ui.label(
                        RichText::new(brush_count(row.brushes))
                            .color(theme.color(Token::TextMuted)),
                    );
                }
                response
            });
            line.inner
        } else {
            let open = reveal.is_some_and(|entity| row.rows.iter().any(|r| r.holds(entity)));
            let header = egui::CollapsingHeader::new(text)
                .id_salt(row.entity)
                .default_open(row.kind == RowKind::Layer)
                .open(open.then_some(true))
                .show(ui, |ui| {
                    for child in &row.rows {
                        Self::row_ui(ui, theme, child, selected, reveal, actions);
                    }
                });
            header.header_response
        };
        if chosen && reveal == Some(row.entity) {
            response.scroll_to_me(Some(egui::Align::Center));
        }
        if response.clicked() && row.kind == RowKind::Entity {
            actions.push(OutlineAction::Choose(row.entity));
        }
        let response = response.interact(Sense::drag());
        if row.kind != RowKind::Layer {
            response.dnd_set_drag_payload(RowDrag(row.entity));
        }
        if row.kind != RowKind::Entity
            && let Some(dragged) = response.dnd_release_payload::<RowDrag>()
            && dragged.0 != row.entity
        {
            actions.push(OutlineAction::MoveTo(dragged.0, row.entity));
        }
    }
}
