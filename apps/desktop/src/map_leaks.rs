// SPDX-License-Identifier: GPL-3.0-or-later

//! The map's leak and texture diagnostics in the map view (P6-11, MAP-7): what `map.leaks`
//! last found, shown in the Properties panel; the leak's trail drawn in every pane from the
//! entity the outside reaches out to the void (QBSP-9.8); the brushes whose textures the
//! map's WADs lack drawn in the error colour; and the camera flown along the trail when the
//! leak is chosen.

use std::collections::BTreeSet;

use eframe::egui::{Painter, RichText, Stroke, Ui};
use qs_render::Camera;
use qs_workbench::map_compile::MapReport;
use qs_workbench::map_edit::MapRevision;

use crate::map_panes::Projection;
use crate::map_scene::{BrushShape, Scene};
use crate::strings::{Text, leak_found, missing_textures};
use crate::theme::{Theme, Token};

/// How fast the camera flies along a leak's trail, in map units a second.
const FLY_SPEED: f32 = 320.0;
/// The width of the trail's line, in points.
const TRAIL_WIDTH: f32 = 2.0;
/// The radius of the dot at each point of the trail, in points.
const TRAIL_DOT: f32 = 3.0;

/// The camera's way along a leak's trail.
#[derive(Clone, Debug, PartialEq)]
struct Flight {
    /// The trail's points, the entity's origin first.
    trail: Vec<[f32; 3]>,
    /// How far along it the camera has come, in map units.
    travelled: f32,
}

impl Flight {
    /// Moves `camera` `distance` further along the trail, looking along the part it is on;
    /// whether the trail goes on past where it now is.
    fn advance(&mut self, camera: &mut Camera, distance: f32) -> bool {
        self.travelled += distance;
        let mut left = self.travelled;
        for pair in self.trail.windows(2) {
            let (Some(&from), Some(&to)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            let along = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
            let length = along.iter().map(|c| c * c).sum::<f32>().sqrt();
            if length <= 0.0 {
                continue;
            }
            let ahead = left < length;
            let t = left.min(length) / length;
            camera.origin = [
                from[0] + along[0] * t,
                from[1] + along[1] * t,
                from[2] + along[2] * t,
            ];
            camera.yaw = along[1].atan2(along[0]).to_degrees();
            camera.pitch = (-along[2] / length).asin().to_degrees();
            if ahead {
                return true;
            }
            left -= length;
        }
        false
    }
}

/// What the map view shows of the map's last check.
#[derive(Debug, Default)]
pub(crate) struct LeakView {
    /// The report and the map's edits it was made at.
    report: Option<(MapReport, MapRevision)>,
    flight: Option<Flight>,
}

impl LeakView {
    /// Takes the report `map.leaks` made of the map at `edits`.
    pub(crate) fn set_report(&mut self, report: MapReport, edits: MapRevision) {
        self.report = Some((report, edits));
        self.flight = None;
    }

    /// The report, when it was made of the map at `edits`: an edit since leaves it out of
    /// date until the next check.
    fn current(&self, edits: MapRevision) -> Option<&MapReport> {
        self.report
            .as_ref()
            .filter(|(_, made)| *made == edits)
            .map(|(report, _)| report)
    }

    /// The report shown, for tests.
    #[cfg(test)]
    pub(crate) fn report(&self) -> Option<&MapReport> {
        self.report.as_ref().map(|(report, _)| report)
    }

    /// Whether the camera is flying along the trail, for tests.
    #[cfg(test)]
    pub(crate) const fn flying(&self) -> bool {
        self.flight.is_some()
    }

    /// The report in the Properties panel, with the leak to fly along.
    pub(crate) fn properties(&mut self, ui: &mut Ui, theme: &Theme, edits: MapRevision) {
        let Some(report) = self.current(edits) else {
            ui.separator();
            return;
        };
        let error = theme.color(Token::StatusError);
        let trail = report.leak.clone();
        if let Some(origin) = trail.first() {
            ui.label(RichText::new(leak_found(*origin)).color(error));
            if ui.button(Text::MapFlyLeak.get()).clicked() {
                self.flight = Some(Flight {
                    trail,
                    travelled: 0.0,
                });
            }
        } else {
            ui.label(RichText::new(Text::MapSealed.get()).color(theme.color(Token::StatusOk)));
        }
        if let Some(report) = self.current(edits)
            && !report.missing_textures.is_empty()
        {
            ui.label(RichText::new(missing_textures(&report.missing_textures)).color(error));
        }
        ui.separator();
    }

    /// Flies `camera` along the trail by `seconds` of flight; whether it is still flying.
    pub(crate) fn fly(&mut self, camera: &mut Camera, seconds: f32) -> bool {
        let Some(flight) = self.flight.as_mut() else {
            return false;
        };
        if !flight.advance(camera, FLY_SPEED * seconds) {
            self.flight = None;
        }
        self.flight.is_some()
    }

    /// Draws the trail and the brushes whose textures are missing, through `projection`.
    pub(crate) fn draw(
        &self,
        painter: &Painter,
        projection: &Projection,
        theme: &Theme,
        scene: &Scene,
        edits: MapRevision,
    ) {
        let Some(report) = self.current(edits) else {
            return;
        };
        let stroke = Stroke::new(TRAIL_WIDTH, theme.color(Token::StatusError));
        if !report.missing_textures.is_empty() {
            let missing: BTreeSet<&str> =
                report.missing_textures.iter().map(String::as_str).collect();
            let lacks = |brush: &BrushShape| {
                brush
                    .textures
                    .iter()
                    .any(|name| missing.contains(name.as_str()))
            };
            for brush in scene.brushes.iter().filter(|brush| lacks(brush)) {
                for line in &brush.lines {
                    projection.line(painter, *line, stroke);
                }
            }
        }
        for pair in report.leak.windows(2) {
            if let (Some(&from), Some(&to)) = (pair.first(), pair.get(1)) {
                projection.line(painter, [from, to], stroke);
            }
        }
        for &point in &report.leak {
            if let Some(at) = projection.point(point) {
                painter.circle_filled(at, TRAIL_DOT, stroke.color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use qs_render::Camera;

    use super::Flight;

    // No spec statement: P6-11: the camera flies along the trail, facing along each part,
    // and stops at its end
    #[test]
    fn the_camera_flies_along_the_trail() {
        let mut flight = Flight {
            trail: vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [100.0, 0.0, -50.0]],
            travelled: 0.0,
        };
        let mut camera = Camera::new([9.0; 3], 45.0);
        assert!(flight.advance(&mut camera, 40.0));
        assert_eq!(camera.origin, [40.0, 0.0, 0.0]);
        assert_eq!((camera.yaw, camera.pitch), (0.0, 0.0));
        assert!(flight.advance(&mut camera, 85.0));
        assert_eq!(camera.origin, [100.0, 0.0, -25.0]);
        assert_eq!(camera.pitch, 90.0, "looking straight down");
        assert!(!flight.advance(&mut camera, 100.0));
        assert_eq!(camera.origin, [100.0, 0.0, -50.0]);
        let mut still = Flight {
            trail: vec![[1.0; 3]],
            travelled: 0.0,
        };
        assert!(!still.advance(&mut camera, 10.0));
    }
}
