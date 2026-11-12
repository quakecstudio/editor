// SPDX-License-Identifier: GPL-3.0-or-later

//! The entity browser (P5-13, MAP-27): the entity classes the project's QuakeC defines, in the
//! tab below the map view where the mockup has it, each with its model's wireframe, or its box
//! where it has none, in its class's colour. A point class is dragged into the view to place
//! it; a brush class is listed but made from brushes.

use std::collections::HashMap;

use eframe::egui::{self, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType};

use crate::map_editor::{box_lines, rgb};
use crate::map_scene::{ClassLook, Line};
use crate::strings::{Text, class_count};
use crate::theme::{Theme, Token};

/// The side of a class's preview, in points (the mockup's texture cells).
const CELL: f32 = 84.0;
/// The rounding of a preview's corners, in points.
const CORNER: f32 = 4.0;
/// The space kept around a preview's lines, in points.
const MARGIN: f32 = 6.0;
/// The width of the filter field, in points.
const FILTER_WIDTH: f32 = 220.0;
/// The box drawn for a class whose comment and code give none, id's `info_null`'s.
const NO_SIZE: [[f32; 3]; 2] = [[-8.0; 3], [8.0; 3]];
/// The previews' view: turned 45 degrees about the vertical and looked down on at 30, so the
/// three axes show.
const VIEW_TURN: f32 = core::f32::consts::FRAC_PI_4;
const VIEW_TILT: f32 = core::f32::consts::FRAC_PI_6;

/// What a class dragged from the browser carries: its classname.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClassDrag(pub(crate) String);

/// The entity browser's state: the filter typed.
#[derive(Debug, Default)]
pub(crate) struct EntityBrowser {
    filter: String,
}

impl EntityBrowser {
    /// Draws the classes `names`, looked up in `classes` and their models in `models`.
    pub(crate) fn ui(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        names: &[String],
        classes: &HashMap<String, ClassLook>,
        models: &HashMap<String, Vec<Line>>,
    ) {
        let filter = self.filter.trim().to_lowercase();
        let shown: Vec<&String> = names
            .iter()
            .filter(|name| name.to_lowercase().contains(&filter))
            .collect();
        ui.horizontal(|ui| {
            ui.label(RichText::new(Text::Entities.get()).strong());
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text(Text::MapFilterEntities.get())
                    .desired_width(FILTER_WIDTH),
            );
            ui.label(
                RichText::new(class_count(shown.len(), names.len()))
                    .color(theme.color(Token::TextMuted)),
            );
        });
        ui.separator();
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                for name in shown {
                    let look = classes.get(name).cloned().unwrap_or_default();
                    cell(ui, theme, name, &look, models);
                }
            });
        });
        if let Some(ClassDrag(name)) = egui::DragAndDrop::payload::<ClassDrag>(ui.ctx()).as_deref()
            && let Some(at) = ui.ctx().pointer_latest_pos()
        {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new(core::any::TypeId::of::<ClassDrag>()),
            ));
            painter.text(
                Pos2::new(at.x + MARGIN * 2.0, at.y + MARGIN * 2.0),
                egui::Align2::LEFT_TOP,
                name,
                egui::TextStyle::Monospace.resolve(ui.style()),
                theme.color(Token::Accent),
            );
        }
    }
}

/// One class: its preview and its name, dragged to place it when it is a point class.
fn cell(
    ui: &mut Ui,
    theme: &Theme,
    name: &str,
    look: &ClassLook,
    models: &HashMap<String, Vec<Line>>,
) {
    ui.vertical(|ui| {
        ui.set_width(CELL);
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(CELL), Sense::drag());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
        let response = if look.brush {
            response.on_hover_text(Text::MapBrushClass.get())
        } else {
            response.dnd_set_drag_payload(ClassDrag(name.to_owned()));
            response.on_hover_text(Text::MapPlaceHint.get())
        };
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CORNER, theme.color(Token::SurfaceViewport));
        let color = look.color.map_or(theme.color(Token::TextMuted), rgb);
        let model = look.model.as_ref().and_then(|model| models.get(model));
        let lines = match model {
            Some(lines) => lines.clone(),
            None => box_lines(look.size.unwrap_or(NO_SIZE)).to_vec(),
        };
        preview(
            &painter,
            rect.shrink(MARGIN),
            &lines,
            Stroke::new(1.0, color),
        );
        if response.hovered() {
            painter.rect_stroke(
                rect,
                CORNER,
                Stroke::new(1.0, theme.color(Token::Accent)),
                egui::StrokeKind::Inside,
            );
        }
        let text = if look.brush {
            theme.color(Token::TextFaint)
        } else {
            theme.color(Token::TextMuted)
        };
        ui.add(egui::Label::new(RichText::new(name).monospace().color(text)).truncate());
    });
}

/// Draws `lines` in `rect`, seen from above at an angle and scaled to fit.
fn preview(painter: &egui::Painter, rect: Rect, lines: &[Line], stroke: Stroke) {
    let (turn_sin, turn_cos) = VIEW_TURN.sin_cos();
    let (tilt_sin, tilt_cos) = VIEW_TILT.sin_cos();
    let flat = |[x, y, z]: [f32; 3]| {
        let across = x * turn_cos - y * turn_sin;
        let depth = x * turn_sin + y * turn_cos;
        [across, depth * tilt_sin - z * tilt_cos]
    };
    let points: Vec<[[f32; 2]; 2]> = lines.iter().map(|[a, b]| [flat(*a), flat(*b)]).collect();
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for [x, y] in points.iter().flatten() {
        min = [min[0].min(*x), min[1].min(*y)];
        max = [max[0].max(*x), max[1].max(*y)];
    }
    let scale =
        (rect.width() / (max[0] - min[0]).max(1.0)).min(rect.height() / (max[1] - min[1]).max(1.0));
    let middle = [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0];
    let centre = rect.center();
    let place = |[x, y]: [f32; 2]| {
        Pos2::new(
            centre.x + (x - middle[0]) * scale,
            centre.y + (y - middle[1]) * scale,
        )
    };
    for [a, b] in points {
        painter.line_segment([place(a), place(b)], stroke);
    }
}
