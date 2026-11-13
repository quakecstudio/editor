// SPDX-License-Identifier: GPL-3.0-or-later

//! The map view's panes (P6-09, MAP-3): the 3D view alone, with a top view inset in its
//! corner, or beside top, front and side views, after TrenchBroom's one-, two- and four-pane
//! layouts; and the projection from map space to a pane, in perspective or flat along an axis.

use eframe::egui::{Painter, Pos2, Rect, Stroke, Vec2};
use qs_layout::MapPanes;
use qs_render::Camera;

use crate::map_scene::Line;
use crate::map_tools::Ray;
use crate::strings::Text;

/// The nearest a line is drawn in the 3D view, in map units, as the renderer's near plane.
const NEAR: f32 = 4.0;
/// How far back a 2D view's rays start, in map units: outside any map, whose coordinates
/// stay within ±32768 in BSP29.
const FAR: f32 = 65_536.0;
/// A 2D view's map units per point when it first opens.
const PLAN_SCALE: f32 = 2.0;
/// The fewest and most map units per point a 2D view zooms to.
const PLAN_ZOOM: core::ops::RangeInclusive<f32> = 0.0625..=64.0;
/// How much a point of scrolling zooms a 2D view, as a power of e.
const PLAN_ZOOM_RATE: f32 = 0.005;
/// Half a pane's width or height, where the four-pane layout splits it.
const HALF: f32 = 0.5;
/// The part of the 3D view's width and height the inset top view takes.
const INSET: f32 = 0.35;

/// The panes of `layout` in `rect`, the 3D view first; a later pane lies over an earlier one.
pub(crate) fn panes(layout: MapPanes, rect: Rect) -> Vec<(Pane, Rect)> {
    match layout {
        MapPanes::Single => vec![(Pane::Perspective, rect)],
        MapPanes::Inset => {
            let corner = Rect::from_min_max(
                Pos2::new(
                    rect.right() - rect.width() * INSET,
                    rect.bottom() - rect.height() * INSET,
                ),
                rect.max,
            );
            vec![(Pane::Perspective, rect), (Pane::Plan(Plan::Top), corner)]
        }
        MapPanes::Quad => {
            let (top, bottom) = rect.split_top_bottom_at_fraction(HALF);
            let (a, b) = top.split_left_right_at_fraction(HALF);
            let (c, d) = bottom.split_left_right_at_fraction(HALF);
            vec![
                (Pane::Perspective, a),
                (Pane::Plan(Plan::Top), b),
                (Pane::Plan(Plan::Front), c),
                (Pane::Plan(Plan::Side), d),
            ]
        }
    }
}

/// One pane of the map view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Pane {
    /// The 3D view, through the camera.
    Perspective,
    /// A 2D view along an axis.
    Plan(Plan),
}

impl Pane {
    /// Its name, which a screen reader and the tests find it by.
    pub(crate) const fn text(self) -> Text {
        match self {
            Self::Perspective => Text::MapEditorView,
            Self::Plan(Plan::Top) => Text::MapViewTop,
            Self::Plan(Plan::Front) => Text::MapViewFront,
            Self::Plan(Plan::Side) => Text::MapViewSide,
        }
    }
}

/// The axis a 2D view looks along.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Plan {
    /// Down -z: x to the right, y up the screen.
    Top,
    /// Along +y: x to the right, z up.
    Front,
    /// Along -x: y to the right, z up.
    Side,
}

impl Plan {
    /// `p` in the view's plane: across and up the screen.
    const fn flat(self, [x, y, z]: [f32; 3]) -> [f32; 2] {
        match self {
            Self::Top => [x, y],
            Self::Front => [x, z],
            Self::Side => [y, z],
        }
    }

    /// The ray into the map through the point `[across, up]` of the view's plane.
    const fn ray(self, [a, u]: [f32; 2]) -> Ray {
        match self {
            Self::Top => ([a, u, FAR], [0.0, 0.0, -1.0]),
            Self::Front => ([a, -FAR, u], [0.0, 1.0, 0.0]),
            Self::Side => ([FAR, a, u], [-1.0, 0.0, 0.0]),
        }
    }
}

/// Where a 2D view looks and how near.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlanCamera {
    /// The point of the view's plane at the pane's middle.
    centre: [f32; 2],
    /// Map units per point.
    scale: f32,
}

impl PlanCamera {
    /// A view of `plan` centred on `at`.
    pub(crate) const fn new(plan: Plan, at: [f32; 3]) -> Self {
        Self {
            centre: plan.flat(at),
            scale: PLAN_SCALE,
        }
    }

    /// Moves the view with the pointer dragged by `drag` points, so the map follows it.
    pub(crate) fn pan(&mut self, drag: Vec2) {
        self.centre = [
            self.centre[0] - drag.x * self.scale,
            self.centre[1] + drag.y * self.scale,
        ];
    }

    /// Zooms in for a positive `scroll`, out for a negative one, within `PLAN_ZOOM`.
    pub(crate) fn zoom(&mut self, scroll: f32) {
        let scaled = self.scale * (-scroll * PLAN_ZOOM_RATE).exp();
        self.scale = scaled.clamp(*PLAN_ZOOM.start(), *PLAN_ZOOM.end());
    }
}

/// How a pane sees the map.
#[derive(Clone, Copy, Debug)]
enum Lens {
    /// Through the camera, in perspective.
    Perspective(Camera),
    /// Flat along an axis.
    Plan(Plan, PlanCamera),
}

/// Map space to a pane's points.
pub(crate) struct Projection {
    lens: Lens,
    rect: Rect,
}

impl Projection {
    /// The 3D view through `camera` in `rect`.
    pub(crate) const fn perspective(camera: Camera, rect: Rect) -> Self {
        Self {
            lens: Lens::Perspective(camera),
            rect,
        }
    }

    /// The 2D view of `plan` seen by `camera` in `rect`.
    pub(crate) const fn plan(plan: Plan, camera: PlanCamera, rect: Rect) -> Self {
        Self {
            lens: Lens::Plan(plan, camera),
            rect,
        }
    }

    /// The width of the pane per point of its height.
    fn aspect(&self) -> f32 {
        self.rect.width() / self.rect.height().max(1.0)
    }

    /// The ray into the map through the point `at` of the pane.
    pub(crate) fn ray(&self, at: Pos2) -> Ray {
        match self.lens {
            Lens::Perspective(camera) => {
                let x = (at.x - self.rect.left()) / self.rect.width().max(1.0) * 2.0 - 1.0;
                let y = 1.0 - (at.y - self.rect.top()) / self.rect.height().max(1.0) * 2.0;
                camera.ray(self.aspect(), [x, y])
            }
            Lens::Plan(plan, view) => {
                let middle = self.rect.center();
                plan.ray([
                    view.centre[0] + (at.x - middle.x) * view.scale,
                    view.centre[1] - (at.y - middle.y) * view.scale,
                ])
            }
        }
    }

    /// `p` in view space: across, up and forward; a 2D view has everything ahead.
    fn view(&self, p: [f32; 3]) -> [f32; 3] {
        match self.lens {
            Lens::Perspective(camera) => {
                let o = camera.origin;
                let v = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
                let dot = |a: [f32; 3]| a[0] * v[0] + a[1] * v[1] + a[2] * v[2];
                let [forward, right, up] = camera.axes();
                [dot(right), dot(up), dot(forward)]
            }
            Lens::Plan(plan, view) => {
                let [a, u] = plan.flat(p);
                [a - view.centre[0], u - view.centre[1], FAR]
            }
        }
    }

    /// Where the view-space point `v`, ahead of the near plane, is in the pane.
    fn screen(&self, v: [f32; 3]) -> Pos2 {
        let middle = self.rect.center();
        match self.lens {
            Lens::Perspective(camera) => {
                let half_x = (camera.fov_x.to_radians() / 2.0).tan();
                let half_y = half_x / self.aspect().max(f32::EPSILON);
                let x = v[0] / (v[2] * half_x);
                let y = v[1] / (v[2] * half_y);
                Pos2::new(
                    middle.x + x * self.rect.width() / 2.0,
                    middle.y - y * self.rect.height() / 2.0,
                )
            }
            Lens::Plan(_, view) => {
                Pos2::new(middle.x + v[0] / view.scale, middle.y - v[1] / view.scale)
            }
        }
    }

    /// Where `p` is in the pane, when it is in front of the near plane.
    pub(crate) fn point(&self, p: [f32; 3]) -> Option<Pos2> {
        let v = self.view(p);
        (v[2] >= NEAR).then(|| self.screen(v))
    }

    /// Draws `line`, cut at the near plane.
    pub(crate) fn line(&self, painter: &Painter, [a, b]: Line, stroke: Stroke) {
        let (mut a, mut b) = (self.view(a), self.view(b));
        if a[2] < NEAR && b[2] < NEAR {
            return;
        }
        let cut = |front: [f32; 3], back: [f32; 3]| {
            let t = (NEAR - back[2]) / (front[2] - back[2]);
            [
                back[0] + (front[0] - back[0]) * t,
                back[1] + (front[1] - back[1]) * t,
                NEAR,
            ]
        };
        if a[2] < NEAR {
            a = cut(b, a);
        } else if b[2] < NEAR {
            b = cut(a, b);
        }
        painter.line_segment([self.screen(a), self.screen(b)], stroke);
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Pos2, Rect, Vec2};

    use qs_layout::MapPanes;

    use super::{Pane, Plan, PlanCamera, Projection, panes};

    const RECT: Rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(200.0, 100.0));

    // No spec statement: P6-09: a 2D view puts each axis where it says, and its ray through a
    // point comes back to that point
    #[test]
    fn a_plan_view_projects_and_picks_along_its_axis() {
        for (plan, p, across) in [
            (Plan::Top, [32.0, 16.0, 999.0], [32.0, 16.0]),
            (Plan::Front, [32.0, 999.0, 16.0], [32.0, 16.0]),
            (Plan::Side, [999.0, 32.0, 16.0], [32.0, 16.0]),
        ] {
            let projection = Projection::plan(plan, PlanCamera::new(plan, [0.0; 3]), RECT);
            let at = projection.point(p).unwrap_or(Pos2::ZERO);
            // 2 map units a point, from the pane's middle; up the map is up the screen.
            assert_eq!(
                at,
                Pos2::new(100.0 + across[0] / 2.0, 50.0 - across[1] / 2.0)
            );
            let (start, direction) = projection.ray(at);
            let back = super::Plan::flat(plan, start);
            assert_eq!(back, across, "{plan:?}");
            assert_eq!(direction.iter().map(|c| c.abs()).sum::<f32>(), 1.0);
        }
    }

    // No spec statement: P6-09: panning follows the pointer and zooming stays in its range
    #[test]
    fn a_plan_view_pans_and_zooms() {
        let mut view = PlanCamera::new(Plan::Top, [0.0; 3]);
        view.pan(Vec2::new(10.0, 10.0));
        assert_eq!(view.centre, [-20.0, 20.0]);
        view.zoom(1e6);
        assert_eq!(view.scale, 0.0625);
        view.zoom(-1e6);
        assert_eq!(view.scale, 64.0);
    }

    // No spec statement: P6-09: each layout has the 3D view first and its 2D views after
    #[test]
    fn the_layouts_split_the_view() {
        let of = |layout: MapPanes| -> Vec<Pane> {
            panes(layout, RECT)
                .into_iter()
                .map(|(pane, _)| pane)
                .collect()
        };
        assert_eq!(of(MapPanes::Single), [Pane::Perspective]);
        assert_eq!(
            of(MapPanes::Inset),
            [Pane::Perspective, Pane::Plan(Plan::Top)]
        );
        assert_eq!(
            of(MapPanes::Quad),
            [
                Pane::Perspective,
                Pane::Plan(Plan::Top),
                Pane::Plan(Plan::Front),
                Pane::Plan(Plan::Side)
            ]
        );
        let quad = panes(MapPanes::Quad, RECT);
        let area: f32 = quad.iter().map(|(_, r)| r.area()).sum();
        assert_eq!(area, RECT.area());
    }
}
