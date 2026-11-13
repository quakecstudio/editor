// SPDX-License-Identifier: GPL-3.0-or-later

//! The Properties panel's brush tools (P6-18, MAP-15): hollow and intersect beside subtract and
//! merge, a scale and a shear of the selected brushes about their middle, a sweep along a path
//! of steps, a picked face extruded or split, and the edge between two corners the Vertex tool
//! picked split or chamfered. Each asks for a [`BrushAction`] the shell runs as its
//! `map.brush_*` command, with the panel's texture lock.

use eframe::egui::{self, Ui};
use qs_map::MAP_MAX_CHAMFER_FACES;
use qs_workbench::map_edit::{Axis, BrushIndex, EntityIndex};

use super::{BrushAction, BrushTools};
use crate::strings::Text;

/// A step a length field moves by per point dragged, in map units.
const LENGTH_STEP: f64 = 1.0;
/// A step a factor field moves by per point dragged.
const FACTOR_STEP: f64 = 0.01;
/// The wall a hollow starts with, in map units.
const DEFAULT_WALL: f64 = 16.0;
/// How far an extrusion, a split and a chamfer start, in map units.
const DEFAULT_LENGTH: f64 = 16.0;
/// The shear a shear starts with.
const DEFAULT_SHEAR: f64 = 0.5;
/// The origin: where a sweep's path starts, and a step of no length.
const ORIGIN: [f64; 3] = [0.0, 0.0, 0.0];
/// Factors that scale nothing.
const UNSCALED: [f64; 3] = [1.0, 1.0, 1.0];

/// What the panel's tool fields hold.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ToolFields {
    wall: f64,
    scale: [f64; 3],
    along: Axis,
    by: Axis,
    shear: f64,
    step: [f64; 3],
    /// The sweep's path so far: each point the sum of the steps added, from the origin.
    path: Vec<[f64; 3]>,
    extrude: f64,
    out: f64,
    distance: f64,
    faces: u32,
}

impl Default for ToolFields {
    fn default() -> Self {
        Self {
            wall: DEFAULT_WALL,
            scale: UNSCALED,
            along: Axis::X,
            by: Axis::Z,
            shear: DEFAULT_SHEAR,
            step: ORIGIN,
            path: Vec::new(),
            extrude: DEFAULT_LENGTH,
            out: DEFAULT_LENGTH,
            distance: DEFAULT_LENGTH,
            faces: 1,
        }
    }
}

/// An axis's name in the panel.
const fn axis_text(axis: Axis) -> Text {
    match axis {
        Axis::X => Text::MapAxisX,
        Axis::Y => Text::MapAxisY,
        Axis::Z => Text::MapAxisZ,
    }
}

/// A choice of axis.
fn axis_choice(ui: &mut Ui, salt: &str, axis: &mut Axis) {
    egui::ComboBox::from_id_salt(ui.id().with(salt))
        .selected_text(axis_text(*axis).get())
        .show_ui(ui, |ui| {
            for choice in [Axis::X, Axis::Y, Axis::Z] {
                ui.selectable_value(axis, choice, axis_text(choice).get());
            }
        });
}

/// Three drag fields for a point or offset.
fn three(ui: &mut Ui, values: &mut [f64; 3], speed: f64) {
    for value in values {
        ui.add(egui::DragValue::new(value).speed(speed));
    }
}

impl BrushTools {
    /// The brush tools for the selected brushes, `first` of `entity` first and `brushes` all of
    /// it: hollow, intersect, scale, shear and sweep.
    pub(super) fn tool_properties(
        &mut self,
        ui: &mut Ui,
        entity: EntityIndex,
        first: BrushIndex,
        brushes: &[BrushIndex],
    ) -> Vec<BrushAction> {
        let mut actions = Vec::new();
        let lock = self.lock;
        let one_entity = self.chosen.iter().all(|&(e, _)| e == entity);
        let fields = &mut self.fields;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(brushes.len() == 1, egui::Button::new(Text::MapShapeHollow.get()))
                .clicked()
            {
                actions.push(BrushAction::Hollow(entity, first, fields.wall));
            }
            let label = ui.label(Text::MapShapeWall.get());
            ui.add(egui::DragValue::new(&mut fields.wall).speed(LENGTH_STEP))
                .labelled_by(label.id);
            if ui
                .add_enabled(
                    brushes.len() >= 2 && one_entity,
                    egui::Button::new(Text::MapIntersect.get()),
                )
                .clicked()
            {
                actions.push(BrushAction::Intersect(entity, brushes.to_vec()));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(Text::MapScaleBy.get());
            three(ui, &mut fields.scale, FACTOR_STEP);
            if ui
                .add_enabled(one_entity, egui::Button::new(Text::MapScaleBrushes.get()))
                .clicked()
            {
                actions.push(BrushAction::Scale(
                    entity,
                    brushes.to_vec(),
                    fields.scale,
                    lock,
                ));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(Text::MapShearAlong.get());
            axis_choice(ui, "shear along", &mut fields.along);
            ui.label(Text::MapShearBy.get());
            axis_choice(ui, "shear by", &mut fields.by);
            ui.add(egui::DragValue::new(&mut fields.shear).speed(FACTOR_STEP));
            if ui
                .add_enabled(
                    one_entity && fields.along != fields.by,
                    egui::Button::new(Text::MapShearBrushes.get()),
                )
                .clicked()
            {
                actions.push(BrushAction::Shear(
                    entity,
                    brushes.to_vec(),
                    fields.along,
                    fields.by,
                    fields.shear,
                    lock,
                ));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(Text::MapSweepStep.get());
            three(ui, &mut fields.step, LENGTH_STEP);
            if ui.button(Text::MapAddStep.get()).clicked() {
                let last = fields.path.last().copied().unwrap_or(ORIGIN);
                fields.path.push([0, 1, 2].map(|i| {
                    last.get(i).copied().unwrap_or_default()
                        + fields.step.get(i).copied().unwrap_or_default()
                }));
            }
            let steps = fields.path.len();
            if ui
                .add_enabled(
                    brushes.len() == 1 && steps > 0,
                    egui::Button::new(format!("{} ({steps})", Text::MapSweep.get())),
                )
                .clicked()
            {
                let mut path = vec![ORIGIN];
                path.append(&mut fields.path);
                actions.push(BrushAction::Sweep(entity, first, path, lock));
            }
            if ui
                .add_enabled(steps > 0, egui::Button::new(Text::MapClearPath.get()))
                .clicked()
            {
                fields.path.clear();
            }
        });
        actions
    }

    /// The tools for the one face picked: extrude and split.
    pub(super) fn face_properties(&mut self, ui: &mut Ui) -> Vec<BrushAction> {
        let mut actions = Vec::new();
        let [face] = self.uv.faces()[..] else {
            return actions;
        };
        let lock = self.lock;
        let fields = &mut self.fields;
        ui.horizontal_wrapped(|ui| {
            if ui.button(Text::MapExtrude.get()).clicked() {
                actions.push(BrushAction::Extrude(face, fields.extrude, lock));
            }
            ui.add(egui::DragValue::new(&mut fields.extrude).speed(LENGTH_STEP));
            if ui.button(Text::MapSplitFace.get()).clicked() {
                actions.push(BrushAction::SplitFace(face, fields.out));
            }
            let label = ui.label(Text::MapOut.get());
            ui.add(egui::DragValue::new(&mut fields.out).speed(LENGTH_STEP))
                .labelled_by(label.id);
        });
        actions
    }

    /// The tools for the edge between the two corners the Vertex tool picked: split and
    /// chamfer.
    pub(super) fn corner_properties(&mut self, ui: &mut Ui) -> Vec<BrushAction> {
        let mut actions = Vec::new();
        let (Some(&(entity, brush)), &[a, b]) = (self.chosen.first(), self.corners.as_slice())
        else {
            return actions;
        };
        let fields = &mut self.fields;
        ui.horizontal_wrapped(|ui| {
            if ui.button(Text::MapSplitEdge.get()).clicked() {
                actions.push(BrushAction::SplitEdge(entity, brush, [a, b], fields.out));
            }
            let label = ui.label(Text::MapOut.get());
            ui.add(egui::DragValue::new(&mut fields.out).speed(LENGTH_STEP))
                .labelled_by(label.id);
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(Text::MapChamfer.get()).clicked() {
                actions.push(BrushAction::Chamfer(
                    entity,
                    brush,
                    [a, b],
                    fields.distance,
                    fields.faces,
                ));
            }
            let label = ui.label(Text::MapChamferDistance.get());
            ui.add(egui::DragValue::new(&mut fields.distance).speed(LENGTH_STEP))
                .labelled_by(label.id);
            let label = ui.label(Text::MapFaces.get());
            ui.add(egui::DragValue::new(&mut fields.faces).range(1..=MAP_MAX_CHAMFER_FACES))
                .labelled_by(label.id);
        });
        actions
    }
}

#[cfg(test)]
mod tests;
