// SPDX-License-Identifier: GPL-3.0-or-later

//! The prefab browser (P6-23, MAP-33), in the tab below the map view: the `.map` files of the
//! project's `prefabs` folder, each dragged into the view to copy it in where it is dropped,
//! as TrenchBroom's prefabs are. Saving the selection as a prefab is in the brushes'
//! Properties.

use std::path::PathBuf;

use eframe::egui::{self, Pos2, RichText, Sense, Ui, WidgetInfo, WidgetType};
use qs_workbench::map_edit::Prefab;

use crate::map_editor::MapAction;
use crate::strings::{Text, prefab_count};
use crate::theme::{Theme, Token};

/// The space between the pointer and the name a dragged prefab shows, in points.
const DRAG_GAP: f32 = 12.0;

/// What a prefab dragged from the browser carries: its file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PrefabDrag(pub(crate) PathBuf);

/// The prefab browser's state: the prefabs last listed.
#[derive(Debug, Default)]
pub(crate) struct PrefabBrowser {
    prefabs: Option<Vec<Prefab>>,
}

impl PrefabBrowser {
    /// Takes the prefabs `map.prefabs` listed.
    pub(crate) fn set_prefabs(&mut self, prefabs: Vec<Prefab>) {
        self.prefabs = Some(prefabs);
    }

    /// The header's part: Refresh, and how many prefabs there are; the list is asked for the
    /// first time it is shown.
    pub(crate) fn header(&self, ui: &mut Ui, theme: &Theme) -> Vec<MapAction> {
        let mut actions = Vec::new();
        if ui.button(Text::Refresh.get()).clicked() || self.prefabs.is_none() {
            actions.push(MapAction::ListPrefabs);
        }
        if let Some(prefabs) = &self.prefabs {
            ui.label(
                RichText::new(prefab_count(prefabs.len())).color(theme.color(Token::TextMuted)),
            );
        }
        actions
    }

    /// The prefabs, each dragged into the view to place it.
    pub(crate) fn body(&self, ui: &mut Ui, theme: &Theme) {
        let prefabs = self.prefabs.as_deref().unwrap_or_default();
        if prefabs.is_empty() {
            ui.label(RichText::new(Text::MapNoPrefabs.get()).color(theme.color(Token::TextMuted)));
            return;
        }
        ui.label(RichText::new(Text::MapPrefabHint.get()).color(theme.color(Token::TextMuted)));
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                for prefab in prefabs {
                    let galley = ui.painter().layout_no_wrap(
                        prefab.name.clone(),
                        egui::TextStyle::Button.resolve(ui.style()),
                        theme.color(Token::Text),
                    );
                    let pad = ui.spacing().button_padding;
                    let text = galley.size();
                    let size = egui::vec2(text.x + pad.x * 2.0, text.y + pad.y * 2.0);
                    let (rect, response) = ui.allocate_exact_size(size, Sense::drag());
                    response.widget_info(|| {
                        WidgetInfo::labeled(WidgetType::Button, true, &prefab.name)
                    });
                    response.dnd_set_drag_payload(PrefabDrag(prefab.path.clone()));
                    let visuals = ui.style().interact(&response);
                    ui.painter()
                        .rect_filled(rect, visuals.corner_radius, visuals.bg_fill);
                    ui.painter().galley(
                        Pos2::new(rect.min.x + pad.x, rect.min.y + pad.y),
                        galley,
                        theme.color(Token::Text),
                    );
                }
            });
        });
        if let Some(PrefabDrag(path)) =
            egui::DragAndDrop::payload::<PrefabDrag>(ui.ctx()).as_deref()
            && let Some(at) = ui.ctx().pointer_latest_pos()
        {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new(core::any::TypeId::of::<PrefabDrag>()),
            ));
            let name = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            painter.text(
                Pos2::new(at.x + DRAG_GAP, at.y + DRAG_GAP),
                egui::Align2::LEFT_TOP,
                name,
                egui::TextStyle::Monospace.resolve(ui.style()),
                theme.color(Token::Accent),
            );
        }
    }
}

#[cfg(test)]
mod tests;
