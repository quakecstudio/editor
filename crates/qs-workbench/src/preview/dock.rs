// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the game's window docks beside the IDE (P3-11, ENG-9): to the right of the IDE's
//! window when the screen has room for it, else over the IDE's top right corner, below its
//! toolbar, and above other windows while the IDE has the focus, so it comes to the front with
//! the IDE and stays behind another app. The host is told with `window/place` (BRIDGE-4.10).

use qs_engine::WindowPlacement;
use serde::{Deserialize, Serialize};

/// The gap between the IDE's window and the game's, in points.
const GAP: f32 = 8.0;
/// The narrowest the game's window is, in points.
const MIN_WIDTH: f32 = 480.0;
/// The widest it is, in points.
const MAX_WIDTH: f32 = 960.0;
/// The game's window's width per unit of height: the host's first size, 1280 by 800.
const ASPECT: f32 = 1.6;
/// How far below the IDE's top the window sits when it overlaps the IDE: below the title bar
/// and the toolbar.
const BELOW_TOOLBAR: f32 = 88.0;
/// The share of the IDE's width the window takes when it overlaps the IDE.
const OVERLAP_SHARE: f32 = 0.4;

/// The IDE's window as the desktop has it, in logical points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DockFrame {
    /// The left edge of the IDE's window, its frame included.
    pub x: f32,
    /// Its top edge.
    pub y: f32,
    /// Its width.
    pub width: f32,
    /// Its height.
    pub height: f32,
    /// The width of the screen the window is on, when the windowing system says. Its origin is
    /// not reported (egui's `ViewportInfo` gives a monitor's size only), so the screens are
    /// taken to be this wide, side by side from the desktop's 0.
    #[serde(default)]
    pub screen_width: Option<f32>,
    /// Whether the IDE's window has the keyboard focus.
    pub focused: bool,
}

impl DockFrame {
    /// Where the game's window goes beside this IDE window. The room right of the IDE is
    /// reckoned on the IDE's own screen: its left edge's place on that screen, `x` within the
    /// screen's width from the desktop's 0, so an IDE on a screen right of the primary (`x`
    /// past its width) or left of it (`x` below 0) has the room its own screen leaves (P3-42).
    #[must_use]
    pub fn placement(&self) -> WindowPlacement {
        let right = self.x + self.width;
        let room = self
            .screen_width
            .filter(|&screen| screen > 0.0)
            .map_or(MAX_WIDTH + GAP, |screen| {
                screen - (self.x.rem_euclid(screen) + self.width) - GAP
            });
        let (x, y, width) = if room >= MIN_WIDTH {
            (right + GAP, self.y, room.min(MAX_WIDTH))
        } else {
            let width = (self.width * OVERLAP_SHARE).clamp(MIN_WIDTH, MAX_WIDTH);
            (right - width - GAP, self.y + BELOW_TOOLBAR, width)
        };
        WindowPlacement {
            x,
            y,
            width,
            height: (width / ASPECT).max(1.0),
            above: self.focused,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DockFrame;

    /// The IDE at (100, 50), 1200 by 900, on a screen `screen` wide, focused.
    fn ide(screen: Option<f32>) -> DockFrame {
        DockFrame {
            x: 100.0,
            y: 50.0,
            width: 1200.0,
            height: 900.0,
            screen_width: screen,
            focused: true,
        }
    }

    // No spec statement: with room on the screen the game's window goes right of the IDE, top
    // aligned, a gap between them, as wide as the room up to 960 and 1.6 wide per unit high
    #[test]
    fn with_room_the_window_docks_to_the_right() {
        let wide = ide(Some(2560.0)).placement();
        assert_eq!(
            (wide.x, wide.y, wide.width, wide.height),
            (1308.0, 50.0, 960.0, 600.0)
        );
        assert!(wide.above);
        // 2000 - 1300 - 8 = 692 points of room.
        let room = ide(Some(2000.0)).placement();
        assert_eq!((room.x, room.width), (1308.0, 692.0));
        assert_eq!(room.height, 692.0 / 1.6);
        // Exactly the narrowest: still beside.
        let tight = ide(Some(1300.0 + 8.0 + 480.0)).placement();
        assert_eq!((tight.x, tight.width), (1308.0, 480.0));
        // No screen size: as if there were room.
        let unknown = ide(None).placement();
        assert_eq!((unknown.x, unknown.width), (1308.0, 960.0));
    }

    // No spec statement: with no room the window overlaps the IDE's top right corner below its
    // toolbar, at 40% of the IDE's width kept between 480 and 960
    #[test]
    fn with_no_room_the_window_overlaps_the_corner() {
        let full = ide(Some(1300.0 + 8.0 + 479.0)).placement();
        assert_eq!(
            (full.x, full.y, full.width),
            (1300.0 - 480.0 - 8.0, 138.0, 480.0)
        );
        let mut big = ide(Some(1300.0));
        big.width = 3000.0;
        let placed = big.placement();
        assert_eq!(placed.width, 960.0);
        assert_eq!(placed.x, 100.0 + 3000.0 - 960.0 - 8.0);
    }

    // No spec statement: the room is the IDE's own screen's, whether that screen is the primary,
    // one right of it or one left of it at a negative x (P3-42)
    #[test]
    fn the_room_is_on_the_ides_own_screen() {
        let on = |x: f32| {
            let mut frame = ide(Some(2560.0));
            frame.x = x;
            frame.placement()
        };
        // A second screen at 2560: the IDE at 2660 has 2560 - 100 - 1200 - 8 = 1252 beside it.
        let right = on(2660.0);
        assert_eq!((right.x, right.y, right.width), (3868.0, 50.0, 960.0));
        // A screen left of the primary: the IDE at -2460 is 100 into it.
        let left = on(-2460.0);
        assert_eq!((left.x, left.width), (-1252.0, 960.0));
        // Near the right edge of a second screen: no room, so over the corner.
        let full = on(2560.0 + 1300.0);
        assert_eq!(full.x, 2560.0 + 1300.0 + 1200.0 - 480.0 - 8.0);
        assert_eq!(full.y, 138.0);
        // A screen of no width is no screen size.
        let mut none = ide(Some(0.0));
        none.x = 5000.0;
        assert_eq!(none.placement().x, 6208.0);
    }

    // No spec statement: the window stays above other windows only while the IDE is focused
    #[test]
    fn the_window_is_above_only_with_the_ide_focused() {
        let mut frame = ide(Some(2560.0));
        frame.focused = false;
        assert!(!frame.placement().above);
    }
}
