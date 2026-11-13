// SPDX-License-Identifier: GPL-3.0-or-later

//! The UV editor (P6-22, MAP-13), after TrenchBroom's: faces picked with Alt-click, the first
//! one's polygon drawn over its texture's tiles in texel space, where a drag moves the texture
//! (snapped to whole texels, and to a corner near a tile's edge), Shift-drag turns it and
//! Ctrl-drag scales it; and buttons that fit, justify, centre, flip, reset or snap the texture
//! of every selected face, or carry the first one's alignment onto others. Each edit is a
//! [`BrushAction`] the shell runs as one `map.face_uv` or `map.face_align` step.

use eframe::egui::{self, Color32, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use qs_workbench::map_uv::{Axis, FaceRef, JustifyTo, UvChoice};

use crate::map_panes::Projection;
use crate::map_scene::{FaceShape, Scene, single};
use crate::map_tools::BrushAction;
use crate::strings::Text;
use crate::theme::{Theme, Token};

/// The UV view's side, in points.
const VIEW_SIZE: f32 = 220.0;
/// The space inside the view's edge, in points.
const VIEW_MARGIN: f32 = 8.0;
/// How near a tile's edge a corner must come, in points, for a drag to snap it there.
const SNAP_REACH: f32 = 6.0;
/// Degrees a Shift-drag turns the texture per point across.
const ROTATE_PER_POINT: f64 = 0.5;
/// The step a Shift-drag's turn snaps to, in degrees.
const ROTATE_STEP: f64 = 15.0;
/// Points across a Ctrl-drag takes to double the scale.
const SCALE_DOUBLING: f64 = 100.0;
/// The step a Ctrl-drag's scale factor snaps to.
const SCALE_STEP: f64 = 0.125;
/// The most tile edges drawn along each axis, so a tiny texture on a huge face stays cheap.
const MOST_TILE_EDGES: usize = 64;
/// The width of the selected faces' outline, in points.
const FACE_OUTLINE: f32 = 3.0;
/// The width of the UV view's lines, in points.
const VIEW_LINE: f32 = 1.0;

/// What a drag in the UV view does, from the keys held as it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragMode {
    /// Moves the texture.
    Shift,
    /// Turns it.
    Rotate,
    /// Scales it.
    Scale,
}

/// A drag in the UV view under way.
#[derive(Clone, Copy, Debug, PartialEq)]
struct UvDrag {
    mode: DragMode,
    moved: Vec2,
}

/// How a picked face joins the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PickMode {
    /// It becomes the selection.
    Replace,
    /// It is added, or taken out when it is in already.
    Toggle,
}

/// The faces selected and the UV editor's state.
#[derive(Debug, Default)]
pub(crate) struct UvEditor {
    /// The faces, the first picked first, each with the plane it had when picked.
    faces: Vec<(FaceRef, ([f32; 3], f32))>,
    /// The face whose alignment Paste carries, with the plane it had when copied.
    copied: Option<(FaceRef, ([f32; 3], f32))>,
    drag: Option<UvDrag>,
}

impl UvEditor {
    /// Whether no face is selected.
    pub(crate) fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// The faces selected, the first picked first.
    pub(crate) fn faces(&self) -> Vec<FaceRef> {
        self.faces.iter().map(|&(face, _)| face).collect()
    }

    /// Takes the face picked, or none, into the selection as `mode` says.
    pub(crate) fn pick(&mut self, picked: Option<FaceRef>, scene: &Scene, mode: PickMode) {
        let found = picked.and_then(|at| scene.face(at).map(|face| (at, face.plane)));
        match (found, mode) {
            (Some(found), PickMode::Toggle) => {
                if let Some(place) = self.faces.iter().position(|&(at, _)| at == found.0) {
                    self.faces.remove(place);
                } else {
                    self.faces.push(found);
                }
            }
            (Some(found), PickMode::Replace) => self.faces = vec![found],
            (None, PickMode::Toggle) => {}
            (None, PickMode::Replace) => self.faces.clear(),
        }
    }

    /// Clears the selection.
    pub(crate) fn clear(&mut self) {
        self.faces.clear();
    }

    /// Forgets the selected faces `scene` no longer has where they were: no face is at the
    /// place, or one with another plane is.
    pub(crate) fn keep_found(&mut self, scene: &Scene) {
        self.faces
            .retain(|&(at, plane)| scene.face(at).is_some_and(|face| face.plane == plane));
        if self
            .copied
            .is_some_and(|(at, plane)| scene.face(at).is_none_or(|face| face.plane != plane))
        {
            self.copied = None;
        }
    }

    /// Outlines the selected faces.
    pub(crate) fn draw(
        &self,
        painter: &Painter,
        projection: &Projection,
        theme: &Theme,
        scene: &Scene,
    ) {
        let stroke = Stroke::new(FACE_OUTLINE, theme.color(Token::Focus));
        for &(at, _) in &self.faces {
            let Some(face) = scene.face(at) else {
                continue;
            };
            let corners: Vec<[f32; 3]> = face.polygon.iter().map(|p| p.map(single)).collect();
            let next = corners.iter().cycle().skip(1);
            for (&a, &b) in corners.iter().zip(next) {
                projection.line(painter, [a, b], stroke);
            }
        }
    }

    /// The selected faces' heading, the UV view and the buttons in the Properties panel, with
    /// each texture's size from `size_of`; what they asked for.
    pub(crate) fn properties(
        &mut self,
        ui: &mut Ui,
        scene: &Scene,
        size_of: &dyn Fn(&str) -> Option<[u32; 2]>,
    ) -> Vec<BrushAction> {
        let mut actions = Vec::new();
        let faces = self.faces();
        let Some(&first) = faces.first() else {
            return actions;
        };
        ui.label(egui::RichText::new(Text::MapFaces.get()).strong());
        ui.label(egui::RichText::new(Text::MapFacesHint.get()).weak());
        if let Some(face) = scene.face(first) {
            let size = size_of(&face.texture);
            if let Some(choice) = self.view(ui, face, size) {
                actions.push(BrushAction::Uv(faces.clone(), choice));
            }
        }
        let buttons = [
            (Text::MapUvFit, UvChoice::Fit),
            (Text::MapUvLeft, justify(JustifyTo::Left)),
            (Text::MapUvRight, justify(JustifyTo::Right)),
            (Text::MapUvTop, justify(JustifyTo::Top)),
            (Text::MapUvBottom, justify(JustifyTo::Bottom)),
            (Text::MapUvCentre, justify(JustifyTo::Centre)),
            (Text::MapUvFlipS, UvChoice::Flip { axis: Axis::S }),
            (Text::MapUvFlipT, UvChoice::Flip { axis: Axis::T }),
            (Text::MapUvReset, UvChoice::Reset),
            (Text::MapUvSnap, UvChoice::Snap),
        ];
        ui.horizontal_wrapped(|ui| {
            for (text, choice) in buttons {
                if ui.button(text.get()).clicked() {
                    actions.push(BrushAction::Uv(faces.clone(), choice));
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(Text::MapCopyAlignment.get()).clicked() {
                self.copied = self.faces.first().copied();
            }
            if let Some((from, _)) = self.copied
                && ui
                    .add_enabled(
                        faces.iter().any(|&at| at != from),
                        egui::Button::new(Text::MapPasteAlignment.get()),
                    )
                    .clicked()
            {
                let onto = faces.iter().copied().filter(|&at| at != from).collect();
                actions.push(BrushAction::AlignFaces(from, onto));
            }
        });
        actions
    }

    /// The UV view of `face`, whose texture is `size` texels if known: its polygon over the
    /// texture's tiles, and the drag; the edit a finished drag asks for.
    fn view(&mut self, ui: &mut Ui, face: &FaceShape, size: Option<[u32; 2]>) -> Option<UvChoice> {
        let side = ui.available_width().min(VIEW_SIZE);
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::drag());
        let corners: Vec<[f64; 2]> = face
            .polygon
            .iter()
            .filter_map(|&p| face.texels(p))
            .collect();
        let tile = size
            .filter(|&[w, h]| w > 0 && h > 0)
            .map(|[w, h]| [f64::from(w), f64::from(h)]);
        let frame = Frame::around(&corners, tile, rect)?;
        if response.drag_started() {
            let mode = ui.input(|i| {
                if i.modifiers.shift {
                    DragMode::Rotate
                } else if i.modifiers.command {
                    DragMode::Scale
                } else {
                    DragMode::Shift
                }
            });
            self.drag = Some(UvDrag {
                mode,
                moved: Vec2::ZERO,
            });
        }
        if let Some(drag) = &mut self.drag {
            let delta = response.drag_delta();
            drag.moved = Vec2::new(drag.moved.x + delta.x, drag.moved.y + delta.y);
        }
        let shifted = self
            .drag
            .filter(|drag| drag.mode == DragMode::Shift)
            .map_or([0.0; 2], |drag| {
                frame.texture_move(drag.moved, &corners, tile)
            });
        let painter = ui.painter_at(rect);
        let visuals = ui.visuals();
        painter.rect_filled(rect, 0.0, visuals.extreme_bg_color);
        if let Some(tile) = tile {
            frame.draw_tiles(&painter, tile, shifted, visuals.weak_text_color());
        }
        let outline = Stroke::new(VIEW_LINE, visuals.selection.stroke.color);
        let points: Vec<Pos2> = corners.iter().map(|&c| frame.screen_of(c)).collect();
        if points.len() > 1 {
            let mut closed = points;
            closed.extend(closed.first().copied());
            painter.add(egui::Shape::line(closed, outline));
        }
        if !response.drag_stopped() {
            return None;
        }
        let drag = self.drag.take()?;
        let across = f64::from(drag.moved.x);
        let choice = match drag.mode {
            DragMode::Shift => UvChoice::Adjust {
                shift: [-shifted[0], -shifted[1]],
                rotate: 0.0,
                scale: [1.0, 1.0],
            },
            DragMode::Rotate => UvChoice::Adjust {
                shift: [0.0; 2],
                rotate: (across * ROTATE_PER_POINT / ROTATE_STEP).round() * ROTATE_STEP,
                scale: [1.0, 1.0],
            },
            DragMode::Scale => {
                let factor = (across / SCALE_DOUBLING).exp2();
                let factor = ((factor / SCALE_STEP).round() * SCALE_STEP).max(SCALE_STEP);
                UvChoice::Adjust {
                    shift: [0.0; 2],
                    rotate: 0.0,
                    scale: [factor, factor],
                }
            }
        };
        let unchanged = UvChoice::Adjust {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0, 1.0],
        };
        (choice != unchanged).then_some(choice)
    }
}

/// `UvChoice::Justify` to `to`.
const fn justify(to: JustifyTo) -> UvChoice {
    UvChoice::Justify { to }
}

/// How the UV view maps texels to the screen: the texel at its least corner and the points
/// per texel.
#[derive(Clone, Copy, Debug)]
struct Frame {
    least: [f64; 2],
    greatest: [f64; 2],
    scale: f64,
    origin: Pos2,
}

impl Frame {
    /// The frame showing `corners` in `rect`, widened to whole tiles of `tile` when its size
    /// is known; none for a face with no corners or no extent.
    fn around(corners: &[[f64; 2]], tile: Option<[f64; 2]>, rect: Rect) -> Option<Self> {
        let [mut ls, mut lt] = *corners.first()?;
        let [mut gs, mut gt] = [ls, lt];
        for &[s, t] in corners {
            (ls, lt, gs, gt) = (ls.min(s), lt.min(t), gs.max(s), gt.max(t));
        }
        if let Some([ws, wt]) = tile {
            let widen = |lo: f64, hi: f64, size: f64| {
                let lo = (lo / size).floor() * size;
                (lo, ((hi / size).ceil() * size).max(lo + size))
            };
            (ls, gs) = widen(ls, gs, ws);
            (lt, gt) = widen(lt, gt, wt);
        }
        let span = (gs - ls).max(gt - lt);
        if span <= 0.0 {
            return None;
        }
        let inside = f64::from(rect.width() - VIEW_MARGIN * 2.0);
        Some(Self {
            least: [ls, lt],
            greatest: [gs, gt],
            scale: inside / span,
            origin: Pos2::new(rect.min.x + VIEW_MARGIN, rect.min.y + VIEW_MARGIN),
        })
    }

    /// Where the texel `at` is drawn.
    fn screen_of(self, [s, t]: [f64; 2]) -> Pos2 {
        let [ls, lt] = self.least;
        Pos2::new(
            self.origin.x + single((s - ls) * self.scale),
            self.origin.y + single((t - lt) * self.scale),
        )
    }

    /// How far, in texels, a drag of `moved` points moves the texture: whole texels, then
    /// snapped so a tile's edge meets the corner nearest one, when one comes within
    /// [`SNAP_REACH`].
    fn texture_move(self, moved: Vec2, corners: &[[f64; 2]], tile: Option<[f64; 2]>) -> [f64; 2] {
        let by = [moved.x, moved.y].map(|m| (f64::from(m) / self.scale).round());
        let Some([ws, wt]) = tile else {
            return by;
        };
        let reach = f64::from(SNAP_REACH) / self.scale;
        let [bs, bt] = by;
        [
            snapped(bs, corners.iter().map(|&[s, _]| s), ws, reach),
            snapped(bt, corners.iter().map(|&[_, t]| t), wt, reach),
        ]
    }

    /// Draws the edges of the tiles of size `tile`, moved by `by` texels.
    fn draw_tiles(
        self,
        painter: &Painter,
        [ws, wt]: [f64; 2],
        [bs, bt]: [f64; 2],
        colour: Color32,
    ) {
        let stroke = Stroke::new(VIEW_LINE, colour);
        let rect = painter.clip_rect();
        let [ls, lt] = self.least;
        let [gs, gt] = self.greatest;
        for s in edges(ls, gs, ws, bs) {
            let x = self.screen_of([s, lt]).x;
            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                stroke,
            );
        }
        for t in edges(lt, gt, wt, bt) {
            let y = self.screen_of([ls, t]).y;
            painter.line_segment(
                [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                stroke,
            );
        }
    }
}

/// `by` moved so the one of `corners` nearest a multiple of `size` past it is on it, when it
/// is within `reach`.
fn snapped(by: f64, corners: impl Iterator<Item = f64>, size: f64, reach: f64) -> f64 {
    let mut nearest: Option<f64> = None;
    for corner in corners {
        let at = corner - by;
        let to_edge = at - (at / size).round() * size;
        if to_edge.abs() <= reach && nearest.is_none_or(|n| to_edge.abs() < n.abs()) {
            nearest = Some(to_edge);
        }
    }
    by + nearest.unwrap_or_default()
}

/// The texels from `lo` to `hi` that are multiples of `size` moved by `by`, at most
/// [`MOST_TILE_EDGES`] of them.
fn edges(lo: f64, hi: f64, size: f64, by: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut at = by + ((lo - by) / size).ceil() * size;
    while at <= hi && out.len() < MOST_TILE_EDGES {
        out.push(at);
        at += size;
    }
    out
}

#[cfg(test)]
mod tests;
