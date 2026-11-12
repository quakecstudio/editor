// SPDX-License-Identifier: GPL-3.0-or-later

//! The map editor's brush tools (P6-08, MAP-2): TrenchBroom's Brush, Clip and Vertex tools
//! beside Select. Brush draws a box on the ground with a drag; Clip cuts the selected brush by
//! the vertical plane through a line dragged on the ground, keeping either side or both; Vertex
//! drags a corner of the selected brush across the plane it lies in. Clicking with any of them
//! selects a brush, Shift+click adds one, and the Properties panel subtracts, merges and
//! deletes the brushes selected. Each edit is a [`BrushAction`] the shell runs as a
//! `map.brush_*` command, so each has undo.

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::{self, Painter, Pos2, Response, Stroke, Ui};
use qs_workbench::map_edit::{BrushIndex, EntityIndex, KeepPart};

use crate::map_editor::{GRID, Projection, box_lines};
use crate::map_scene::{Scene, single};
use crate::strings::Text;
use crate::theme::{Theme, Token};

/// The height of a box the Brush tool draws, in map units: four grid steps.
const BRUSH_HEIGHT: f32 = 64.0;
/// The height of the clip plane drawn while a cut line is dragged, in map units.
const CLIP_HEIGHT: f32 = 256.0;
/// How near the pointer must be to a corner, in points, for the Vertex tool to take it.
const VERTEX_REACH: f32 = 8.0;
/// The radius of a corner's handle, in points.
const VERTEX_DOT: f32 = 3.0;
/// The smallest ray component counted as crossing a plane rather than running along it.
const ALONG_PLANE: f32 = 1e-6;

/// A brush: the entity holding it and its place among the entity's brushes.
pub(crate) type BrushRef = (EntityIndex, BrushIndex);

/// The tool the map view's left button uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MapTool {
    /// Select, move and link entities.
    #[default]
    Select,
    /// Draw a box brush.
    Brush,
    /// Cut a brush by a plane.
    Clip,
    /// Move a brush's corners.
    Vertex,
}

impl MapTool {
    /// Every tool, in the toolbar's order.
    const ALL: [Self; 4] = [Self::Select, Self::Brush, Self::Clip, Self::Vertex];

    const fn text(self) -> Text {
        match self {
            Self::Select => Text::MapToolSelect,
            Self::Brush => Text::MapToolBrush,
            Self::Clip => Text::MapToolClip,
            Self::Vertex => Text::MapToolVertex,
        }
    }

    /// What to do with the tool, shown beside the toolbar.
    const fn hint(self) -> Option<Text> {
        match self {
            Self::Select => None,
            Self::Brush => Some(Text::MapBrushHint),
            Self::Clip => Some(Text::MapClipHint),
            Self::Vertex => Some(Text::MapVertexHint),
        }
    }
}

/// A brush edit the map editor asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum BrushAction {
    /// Create a box brush in the world from one corner to the other.
    Create([f64; 3], [f64; 3]),
    /// Cut a brush by the plane through three points, keeping the parts chosen.
    Clip(EntityIndex, BrushIndex, [[f64; 3]; 3], KeepPart),
    /// Move a corner of a brush from one point to another.
    Vertex(EntityIndex, BrushIndex, [f64; 3], [f64; 3]),
    /// Subtract a brush from the others of its entity.
    Subtract(EntityIndex, BrushIndex),
    /// Merge brushes of an entity into their hull.
    Merge(EntityIndex, Vec<BrushIndex>),
    /// Delete a brush.
    Delete(EntityIndex, BrushIndex),
}

/// What the left button is dragging.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// A box or a cut line, from this point on the ground.
    Ground([f32; 3]),
    /// A corner of a brush, from where it is.
    Corner(BrushRef, [f64; 3]),
}

/// The brush tools' state.
#[derive(Debug, Default)]
pub(crate) struct BrushTools {
    tool: MapTool,
    /// Which parts of a clipped brush stay.
    keep: KeepPart,
    /// The brushes selected, the first chosen first.
    chosen: Vec<BrushRef>,
    /// The corners each selected brush had when it was last seen: an edit or undo that moves
    /// brushes along leaves a place naming another brush, whose corners differ.
    seen: BTreeMap<BrushRef, Vec<[f64; 3]>>,
    /// The brush the last clip or vertex move asked for reshapes, which stays selected.
    reshaped: Option<BrushRef>,
    drag: Option<Drag>,
}

/// A ray in map space: where it starts and its direction.
pub(crate) type Ray = ([f32; 3], [f32; 3]);

/// Where `ray` meets the horizontal plane at height `z`, ahead of its start.
fn at_height((start, direction): Ray, z: f32) -> Option<[f32; 3]> {
    if direction[2].abs() < ALONG_PLANE {
        return None;
    }
    let t = (z - start[2]) / direction[2];
    (t > 0.0).then(|| [start[0] + direction[0] * t, start[1] + direction[1] * t, z])
}

/// `point` on the grid, as the commands snap it.
fn snapped(point: [f32; 3]) -> [f64; 3] {
    point.map(|c| (f64::from(c) / GRID).round() * GRID)
}

/// `point` on the grid, for drawing.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a grid point of a map is well inside f32's range"
)]
fn snapped_f32(point: [f32; 3]) -> [f32; 3] {
    snapped(point).map(|c| c as f32)
}

/// The box from `a` to `b` on the ground, `BRUSH_HEIGHT` tall.
fn ground_box(a: [f32; 3], b: [f32; 3]) -> [[f32; 3]; 2] {
    [
        [a[0].min(b[0]), a[1].min(b[1]), a[2]],
        [a[0].max(b[0]), a[1].max(b[1]), a[2] + BRUSH_HEIGHT],
    ]
}

impl BrushTools {
    /// The tool the left button uses.
    pub(crate) const fn tool(&self) -> MapTool {
        self.tool
    }

    /// Forgets the selected brushes `scene` no longer has where they were: none is at the
    /// place, or another brush is, other than the one the last clip or vertex move reshaped.
    pub(crate) fn keep_found(&mut self, scene: &Scene) {
        let reshaped = self.reshaped.take();
        let seen = &mut self.seen;
        self.chosen.retain(|&chosen| {
            let Some(shape) = scene.brush(chosen.0, chosen.1) else {
                return false;
            };
            let same = seen.get(&chosen) == Some(&shape.vertices);
            if same || reshaped == Some(chosen) {
                seen.insert(chosen, shape.vertices.clone());
            }
            same || reshaped == Some(chosen)
        });
        let chosen = &self.chosen;
        self.seen.retain(|brush, _| chosen.contains(brush));
    }

    /// The tool selector, and the clip's side when clipping.
    pub(crate) fn toolbar(&mut self, ui: &mut Ui) {
        for tool in MapTool::ALL {
            ui.selectable_value(&mut self.tool, tool, tool.text().get());
        }
        if self.tool == MapTool::Clip {
            ui.separator();
            ui.selectable_value(&mut self.keep, KeepPart::Back, Text::MapKeepRight.get());
            ui.selectable_value(&mut self.keep, KeepPart::Front, Text::MapKeepLeft.get());
            ui.selectable_value(&mut self.keep, KeepPart::Both, Text::KeepBoth.get());
        }
        if let Some(hint) = self.tool.hint() {
            ui.separator();
            ui.weak(hint.get());
        }
    }

    /// Reads the left button in the view for the tool chosen, other than Select; `ray` turns
    /// a point of the view into a ray. What it asked for.
    pub(crate) fn input(
        &mut self,
        ui: &Ui,
        response: &Response,
        scene: &Scene,
        hidden: &BTreeSet<EntityIndex>,
        ray: impl Fn(Pos2) -> Ray,
        projection: &Projection,
    ) -> Vec<BrushAction> {
        let mut actions = Vec::new();
        if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            let (start, direction) = ray(at);
            let picked = scene.pick_brush(start, direction, hidden);
            let shift = ui.input(|i| i.modifiers.shift);
            if let Some(brush) = picked
                && let Some(shape) = scene.brush(brush.0, brush.1)
            {
                self.seen.insert(brush, shape.vertices.clone());
            }
            match picked {
                Some(brush) if shift => {
                    if let Some(place) = self.chosen.iter().position(|&b| b == brush) {
                        self.chosen.remove(place);
                    } else {
                        self.chosen.push(brush);
                    }
                }
                Some(brush) => self.chosen = vec![brush],
                None if shift => {}
                None => self.chosen.clear(),
            }
        }
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(at) = ui.input(|i| i.pointer.press_origin())
        {
            self.drag = match self.tool {
                MapTool::Select => None,
                MapTool::Brush | MapTool::Clip => at_height(ray(at), 0.0).map(Drag::Ground),
                MapTool::Vertex => self
                    .corner_near(scene, projection, at)
                    .map(|(b, v)| Drag::Corner(b, v)),
            };
        }
        if response.drag_stopped()
            && let Some(drag) = self.drag.take()
            && let Some(at) = ui.ctx().pointer_latest_pos()
        {
            let finished = self.finish(drag, ray(at));
            if let Some(
                BrushAction::Clip(entity, brush, ..) | BrushAction::Vertex(entity, brush, ..),
            ) = finished
            {
                self.reshaped = Some((entity, brush));
            }
            actions.extend(finished);
        }
        actions
    }

    /// The corner of a selected brush nearest `at` in the view, within reach.
    fn corner_near(
        &self,
        scene: &Scene,
        projection: &Projection,
        at: Pos2,
    ) -> Option<(BrushRef, [f64; 3])> {
        let mut best: Option<(f32, BrushRef, [f64; 3])> = None;
        for &(entity, brush) in &self.chosen {
            let Some(shape) = scene.brush(entity, brush) else {
                continue;
            };
            for &corner in &shape.vertices {
                let Some(drawn) = projection.point(corner.map(single)) else {
                    continue;
                };
                let distance = drawn.distance(at);
                if distance <= VERTEX_REACH && best.is_none_or(|(d, _, _)| distance < d) {
                    best = Some((distance, (entity, brush), corner));
                }
            }
        }
        best.map(|(_, brush, corner)| (brush, corner))
    }

    /// The edit a drag ending on `ray` makes.
    #[expect(
        clippy::float_cmp,
        reason = "both ends are snapped to the grid: equal is the same grid point"
    )]
    fn finish(&self, drag: Drag, ray: Ray) -> Option<BrushAction> {
        match (self.tool, drag) {
            (MapTool::Brush, Drag::Ground(from)) => {
                let to = at_height(ray, from[2])?;
                let [min, max] = ground_box(from, to);
                Some(BrushAction::Create(snapped(min), snapped(max)))
            }
            (MapTool::Clip, Drag::Ground(from)) => {
                let &(entity, brush) = self.chosen.first()?;
                let (a, b) = (snapped(from), snapped(at_height(ray, from[2])?));
                (a != b).then(|| {
                    let above = [b[0], b[1], b[2] + 1.0];
                    BrushAction::Clip(entity, brush, [a, b, above], self.keep)
                })
            }
            (MapTool::Vertex, Drag::Corner((entity, brush), from)) => {
                let to = at_height(ray, single(from[2]))?;
                let [x, y, _] = snapped(to);
                Some(BrushAction::Vertex(entity, brush, from, [x, y, from[2]]))
            }
            (MapTool::Select | MapTool::Brush | MapTool::Clip | MapTool::Vertex, _) => None,
        }
    }

    /// Draws the selected brushes, the Vertex tool's corners and the drag under way.
    pub(crate) fn draw(
        &self,
        painter: &Painter,
        projection: &Projection,
        theme: &Theme,
        scene: &Scene,
        pointer: Option<Ray>,
    ) {
        let accent = Stroke::new(2.0, theme.color(Token::Accent));
        for &(entity, brush) in &self.chosen {
            let Some(shape) = scene.brush(entity, brush) else {
                continue;
            };
            for line in &shape.lines {
                projection.line(painter, *line, accent);
            }
            if self.tool == MapTool::Vertex {
                for &corner in &shape.vertices {
                    if let Some(at) = projection.point(corner.map(single)) {
                        painter.circle_filled(at, VERTEX_DOT, accent.color);
                    }
                }
            }
        }
        let focus = Stroke::new(2.0, theme.color(Token::Focus));
        let Some(drag) = self.drag else {
            return;
        };
        let from = match drag {
            Drag::Ground(from) => snapped_f32(from),
            Drag::Corner(_, from) => from.map(single),
        };
        let Some(to) = pointer.and_then(|ray| at_height(ray, from[2])) else {
            return;
        };
        let to = match drag {
            Drag::Ground(_) => snapped_f32(to),
            Drag::Corner(..) => {
                let [x, y, _] = snapped_f32(to);
                [x, y, from[2]]
            }
        };
        match self.tool {
            MapTool::Brush => {
                for line in box_lines(ground_box(from, to)) {
                    projection.line(painter, line, focus);
                }
            }
            MapTool::Clip => {
                let up = |p: [f32; 3]| [p[0], p[1], p[2] + CLIP_HEIGHT];
                for line in [
                    [from, to],
                    [to, up(to)],
                    [up(to), up(from)],
                    [up(from), from],
                ] {
                    projection.line(painter, line, focus);
                }
            }
            MapTool::Vertex => {
                projection.line(painter, [from, to], focus);
                if let Some(at) = projection.point(to) {
                    painter.circle_filled(at, VERTEX_DOT, focus.color);
                }
            }
            MapTool::Select => {}
        }
    }

    /// The selected brushes' buttons in the Properties panel; what they asked for.
    pub(crate) fn properties(&mut self, ui: &mut Ui) -> Vec<BrushAction> {
        let mut actions = Vec::new();
        let Some(&(entity, first)) = self.chosen.first() else {
            return actions;
        };
        ui.label(egui::RichText::new(Text::MapBrushes.get()).strong());
        let one_entity = self.chosen.iter().all(|&(e, _)| e == entity);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.chosen.len() == 1,
                    egui::Button::new(Text::MapSubtract.get()),
                )
                .clicked()
            {
                actions.push(BrushAction::Subtract(entity, first));
            }
            let merge = self.chosen.len() >= 2 && one_entity;
            if ui
                .add_enabled(merge, egui::Button::new(Text::MapMerge.get()))
                .clicked()
            {
                let brushes = self.chosen.iter().map(|&(_, b)| b).collect();
                actions.push(BrushAction::Merge(entity, brushes));
            }
            if ui.button(Text::MapDeleteBrush.get()).clicked() {
                let mut doomed = self.chosen.clone();
                // Last first, so each index still names its brush when its turn comes.
                doomed.sort_by(|a, b| b.cmp(a));
                actions.extend(doomed.into_iter().map(|(e, b)| BrushAction::Delete(e, b)));
            }
        });
        if !actions.is_empty() {
            self.chosen.clear();
        }
        ui.separator();
        actions
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::panic,
        reason = "test code: a failed unwrap or a wrong variant is a failed test"
    )]

    use super::{BrushAction, BrushTools, Drag, MapTool, at_height};
    use crate::map_scene::{BrushShape, Scene};
    use qs_workbench::map_edit::{BrushIndex, EntityIndex, KeepPart};

    fn down_from(x: f32, y: f32) -> super::Ray {
        ([x, y, 100.0], [0.0, 0.0, -1.0])
    }

    // No spec statement: P6-08: a ray meets the ground ahead of it, not behind or along it
    #[test]
    fn a_ray_meets_the_ground_ahead() {
        assert_eq!(at_height(down_from(3.0, 4.0), 0.0), Some([3.0, 4.0, 0.0]));
        assert_eq!(at_height(([0.0, 0.0, 100.0], [0.0, 0.0, 1.0]), 0.0), None);
        assert_eq!(at_height(([0.0, 0.0, 100.0], [1.0, 0.0, 0.0]), 0.0), None);
    }

    // No spec statement: P6-08: a box drawn on the ground snaps to the grid and is 64 tall
    #[test]
    fn a_drag_on_the_ground_draws_a_box() {
        let tools = BrushTools {
            tool: MapTool::Brush,
            ..BrushTools::default()
        };
        let made = tools.finish(Drag::Ground([70.0, 3.0, 0.0]), down_from(-20.0, 60.0));
        assert_eq!(
            made,
            Some(BrushAction::Create([-16.0, 0.0, 0.0], [64.0, 64.0, 64.0]))
        );
    }

    // No spec statement: P6-08: a cut line needs a selected brush and two grid points apart;
    // the plane is vertical through it, with the side chosen
    #[test]
    fn a_cut_line_clips_the_selected_brush() {
        let brush = (EntityIndex::new(0), BrushIndex::new(1));
        let mut tools = BrushTools {
            tool: MapTool::Clip,
            keep: KeepPart::Both,
            ..BrushTools::default()
        };
        let line = |tools: &BrushTools, to| tools.finish(Drag::Ground([0.0, 0.0, 0.0]), to);
        assert_eq!(line(&tools, down_from(64.0, 0.0)), None, "nothing selected");
        tools.chosen.push(brush);
        assert_eq!(
            line(&tools, down_from(3.0, 2.0)),
            None,
            "the same grid point"
        );
        let Some(BrushAction::Clip(_, _, points, keep)) = line(&tools, down_from(64.0, 0.0)) else {
            panic!("a clip");
        };
        assert_eq!(keep, KeepPart::Both);
        assert_eq!(points, [[0.0; 3], [64.0, 0.0, 0.0], [64.0, 0.0, 1.0]]);
    }

    // No spec statement: P6-08: a corner is dragged across the plane it lies in, to the grid
    #[test]
    fn a_corner_moves_at_its_height() {
        let brush = (EntityIndex::new(0), BrushIndex::new(0));
        let tools = BrushTools {
            tool: MapTool::Vertex,
            ..BrushTools::default()
        };
        let moved = tools
            .finish(
                Drag::Corner(brush, [64.0, 64.0, 64.0]),
                down_from(90.0, 70.0),
            )
            .unwrap();
        assert_eq!(
            moved,
            BrushAction::Vertex(brush.0, brush.1, [64.0; 3], [96.0, 64.0, 64.0])
        );
        let off_grid = tools
            .finish(
                Drag::Corner(brush, [64.0, 64.0, 10.0]),
                down_from(90.0, 70.0),
            )
            .unwrap();
        assert_eq!(
            off_grid,
            BrushAction::Vertex(brush.0, brush.1, [64.0, 64.0, 10.0], [96.0, 64.0, 10.0]),
            "a corner off the grid keeps its height"
        );
    }

    /// A scene holding one brush of the world at each place, with these corners.
    fn scene(corners: &[[f64; 3]]) -> Scene {
        let mut scene = Scene::default();
        for (place, &corner) in corners.iter().enumerate() {
            scene.brushes.push(BrushShape {
                entity: EntityIndex::new(0),
                brush: BrushIndex::new(place),
                lines: Vec::new(),
                vertices: vec![corner],
                planes: Vec::new(),
            });
        }
        scene
    }

    // No spec statement: P6-08: a selected brush stays selected while its place holds it; an
    // edit or undo that puts another brush there, or none, drops it, but the brush a clip or
    // vertex move reshaped stays
    #[test]
    fn the_selection_follows_the_brushes() {
        let (a, b) = ([0.0; 3], [64.0; 3]);
        let second = (EntityIndex::new(0), BrushIndex::new(1));
        let mut tools = BrushTools {
            chosen: vec![second],
            ..BrushTools::default()
        };
        tools.seen.insert(second, vec![b]);
        tools.keep_found(&scene(&[a, b]));
        assert_eq!(tools.chosen, vec![second], "unchanged");
        tools.keep_found(&scene(&[b, a]));
        assert!(tools.chosen.is_empty(), "another brush is there");
        assert!(tools.seen.is_empty());
        tools.chosen = vec![second];
        tools.seen.insert(second, vec![b]);
        tools.reshaped = Some(second);
        tools.keep_found(&scene(&[a, [32.0; 3]]));
        assert_eq!(tools.chosen, vec![second], "reshaped by the last edit");
        assert_eq!(tools.seen.get(&second), Some(&vec![[32.0; 3]]));
        assert_eq!(tools.reshaped, None);
        tools.keep_found(&scene(&[a]));
        assert!(tools.chosen.is_empty(), "gone");
    }
}
