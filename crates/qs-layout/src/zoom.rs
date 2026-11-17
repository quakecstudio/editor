// SPDX-License-Identifier: GPL-3.0-or-later

//! The interface's zoom (P7-12, QA-9): text and icons drawn larger or smaller, up to 300
//! percent, as Apple's Human Interface Guidelines, Accessibility, ask: "Ideally, give people the
//! option to enlarge text by at least 200 percent". A command sets it, so the menu, the keyboard,
//! the CLI and the agent share it, and the layout file keeps it.

use core::fmt;

use qs_commands::{Command, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::HasLayout;

/// The id of the command that sets the zoom.
pub const ZOOM: &str = "view.zoom";

/// How much larger than 100 percent the interface is drawn, as a factor: 1 is 100 percent.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "f32", into = "f32")]
pub struct Zoom(f32);

impl Zoom {
    /// The smallest zoom, 50 percent.
    pub const MIN: f32 = 0.5;
    /// The largest zoom, 300 percent.
    pub const MAX: f32 = 3.0;
    /// 100 percent.
    pub const ACTUAL: Self = Self(1.0);
    /// How much one step in or out adds or takes away: 10 percentage points, as egui's own
    /// Cmd+Plus and Cmd+Minus step (`egui::gui_zoom::zoom_in`), so the menu and the keys agree.
    pub const STEP: f32 = 0.1;

    /// `factor` as a zoom, held to the range.
    #[must_use]
    pub const fn new(factor: f32) -> Self {
        Self(factor.clamp(Self::MIN, Self::MAX))
    }

    /// The factor.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }

    /// One step larger, rounded to a tenth as egui rounds its own.
    #[must_use]
    pub fn zoomed_in(self) -> Self {
        Self::new(tenths(self.0 + Self::STEP))
    }

    /// One step smaller, rounded to a tenth.
    #[must_use]
    pub fn zoomed_out(self) -> Self {
        Self::new(tenths(self.0 - Self::STEP))
    }
}

/// How many tenths make one, for rounding a zoom as egui rounds its own.
const TENTHS: f32 = 10.0;

/// `factor` rounded to the nearest tenth.
fn tenths(factor: f32) -> f32 {
    (factor * TENTHS).round() / TENTHS
}

impl Default for Zoom {
    fn default() -> Self {
        Self::ACTUAL
    }
}

/// A zoom outside the range, or not a number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomError(f32);

impl fmt::Display for ZoomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a zoom of {} is outside {} to {}",
            self.0,
            Zoom::MIN,
            Zoom::MAX
        )
    }
}

impl std::error::Error for ZoomError {}

impl TryFrom<f32> for Zoom {
    type Error = ZoomError;

    fn try_from(factor: f32) -> Result<Self, ZoomError> {
        if (Self::MIN..=Self::MAX).contains(&factor) {
            Ok(Self(factor))
        } else {
            Err(ZoomError(factor))
        }
    }
}

impl From<Zoom> for f32 {
    fn from(zoom: Zoom) -> Self {
        zoom.0
    }
}

/// The parameters of `view.zoom`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ZoomParams {
    /// The zoom, from 0.5 (50 percent) to 3 (300 percent).
    pub zoom: Zoom,
}

/// `view.zoom`.
struct SetZoom;

impl<C: HasLayout + 'static> Command<C> for SetZoom {
    const ID: &'static str = ZOOM;
    const SUMMARY: &'static str = "Draws the interface's text and icons larger or smaller, \
        from 0.5 (50 percent) to 3 (300 percent); 1 is the actual size.";
    type Params = ZoomParams;
    type Output = Zoom;
    type Error = !;

    fn run(&self, context: &mut C, params: ZoomParams) -> Result<Outcome<Zoom, C>, !> {
        let layout = context.layout_mut();
        layout.zoom = params.zoom;
        Ok(Outcome::outside_history(layout.zoom))
    }
}

/// Registers `view.zoom`.
pub(crate) fn register<C: HasLayout + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(SetZoom)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::float_cmp_const,
        reason = "test code: a failed unwrap is a failed test; the factors compared are exact"
    )]

    use qs_commands::{Registry, Session};
    use serde_json::json;

    use super::{ZOOM, Zoom};
    use crate::Layout;

    // No spec statement: P7-12: view.zoom sets the zoom outside the undo history, the layout
    // file keeps it, an older file reads as 100 percent, and a zoom out of range is refused
    #[test]
    fn the_zoom_is_set_kept_and_held_to_its_range() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new(Layout::default(), registry);
        session.execute(ZOOM, json!({ "zoom": 2.0 })).unwrap();
        assert_eq!(session.context().zoom.get(), 2.0);
        assert!(session.undo().is_err());
        let written = toml::to_string(&session.context().as_written()).unwrap();
        let read: Layout = toml::from_str(&written).unwrap();
        assert_eq!(read.zoom.get(), 2.0);
        let older: Layout = toml::from_str("[left]\nvisible = true\nsize = 240.0\n").unwrap();
        assert_eq!(older.zoom, Zoom::ACTUAL);
        for bad in [0.49, 3.01, f64::NAN] {
            assert!(
                session.execute(ZOOM, json!({ "zoom": bad })).is_err(),
                "{bad}"
            );
        }
        assert!(toml::from_str::<Layout>("zoom = 9.0\n").is_err());
        for edge in [Zoom::MIN, Zoom::MAX] {
            session.execute(ZOOM, json!({ "zoom": edge })).unwrap();
            assert_eq!(session.context().zoom.get(), edge);
        }
    }

    // No spec statement: P7-12: a step in or out is 10 percentage points, as egui's keys step,
    // rounded to a tenth and held to 50 to 300 percent
    #[test]
    fn steps_are_ten_points_within_the_range() {
        assert_eq!(Zoom::ACTUAL.zoomed_in().get(), 1.1);
        assert_eq!(Zoom::ACTUAL.zoomed_out().get(), 0.9);
        assert_eq!(Zoom::new(1.5).zoomed_in().zoomed_out().get(), 1.5);
        assert_eq!(Zoom::new(1.54).zoomed_in().get(), 1.6);
        assert_eq!(Zoom::new(2.95).zoomed_in().get(), Zoom::MAX);
        assert_eq!(Zoom::new(0.55).zoomed_out().get(), Zoom::MIN);
        assert_eq!(Zoom::new(10.0).get(), Zoom::MAX);
        assert_eq!(Zoom::new(0.0).get(), Zoom::MIN);
        assert_eq!(Zoom::try_from(0.4), Err(super::ZoomError(0.4)));
        assert_eq!(
            super::ZoomError(0.4).to_string(),
            "a zoom of 0.4 is outside 0.5 to 3"
        );
    }
}
