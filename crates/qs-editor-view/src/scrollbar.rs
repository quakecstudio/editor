// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's scroll bars (P7-16): where a bar's thumb is for a scroll position, and where a
//! drag of the thumb or a click beside it scrolls to. Zed's editor is the model: the thumb's
//! length is the part of the text the view shows, its place the part scrolled past. Down the
//! view the bar scrolls lines, and the file can be scrolled until its last line is the first
//! shown; across it the bar scrolls points of the widest line, until that line's end is at
//! the view's edge.

use egui::{Pos2, Rect, Response};

/// A bar's width in points.
pub(crate) const WIDTH: f32 = 12.0;
/// The space either side of the thumb inside the bar, in points.
pub(crate) const INSET: f32 = 3.0;
/// The thumb's least length in points, so it can be grabbed in a long file.
const MIN_THUMB: f32 = 24.0;

/// The way a bar runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    /// Down the view's right edge, scrolling lines.
    Down,
    /// Across the view's bottom edge, scrolling points.
    Across,
}

impl Axis {
    /// How far along the bar `pos` is.
    const fn along(self, pos: Pos2) -> f32 {
        match self {
            Self::Down => pos.y,
            Self::Across => pos.x,
        }
    }
}

/// The thumb: where it starts along its track and its length, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Thumb {
    /// Its top or left edge.
    pub(crate) start: f32,
    /// Its length along the track.
    pub(crate) length: f32,
}

impl Thumb {
    /// Whether the place `at` along the track is on the thumb.
    pub(crate) fn holds(self, at: f32) -> bool {
        (self.start..=self.start + self.length).contains(&at)
    }

    /// The thumb as drawn in the bar `bar` of the way `axis`, inset from the bar's sides.
    pub(crate) fn shape(self, axis: Axis, bar: Rect) -> Rect {
        let end = self.start + self.length;
        match axis {
            Axis::Down => Rect::from_min_max(
                Pos2::new(bar.left() + INSET, self.start),
                Pos2::new(bar.right() - INSET, end),
            ),
            Axis::Across => Rect::from_min_max(
                Pos2::new(self.start, bar.top() + INSET),
                Pos2::new(end, bar.bottom() - INSET),
            ),
        }
    }
}

/// A bar's track: where it is, and what it scrolls, in lines or in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Track {
    start: f32,
    length: f32,
    /// The furthest the view can be scrolled; 0 when it shows everything.
    last: usize,
    /// How much the view shows at once.
    shown: usize,
}

impl Track {
    /// The track from `top` down `height` points, of a file of `lines` lines in a view of
    /// `visible` rows: the last line can be scrolled to the view's top.
    pub(crate) const fn of_lines(top: f32, height: f32, lines: usize, visible: usize) -> Self {
        let last = if lines > visible {
            lines.saturating_sub(1)
        } else {
            0
        };
        Self {
            start: top,
            length: height,
            last,
            shown: visible,
        }
    }

    /// The track from `left` across `width` points, of text `content` points wide in a view
    /// `view` points wide: the text's end can be scrolled to the view's edge.
    pub(crate) const fn of_width(left: f32, width: f32, content: usize, view: usize) -> Self {
        Self {
            start: left,
            length: width,
            last: content.saturating_sub(view),
            shown: view,
        }
    }

    /// The furthest the view can be scrolled.
    pub(crate) const fn last(&self) -> usize {
        self.last
    }

    /// The thumb when the view is scrolled to `first`; none when the view shows everything
    /// at once, or the track has no length.
    pub(crate) fn thumb(&self, first: usize) -> Option<Thumb> {
        if self.last == 0 || self.length <= 0.0 {
            return None;
        }
        // What there is to scroll through: all that can be scrolled past, and a view more.
        let all = self.last.saturating_add(self.shown);
        let length = (self.length * part(self.shown, all))
            .max(MIN_THUMB)
            .min(self.length);
        let travel = self.length - length;
        Some(Thumb {
            start: travel.mul_add(part(first.min(self.last), self.last), self.start),
            length,
        })
    }

    /// Where the view is scrolled to when the thumb starts at `start`: a drag of the thumb.
    pub(crate) fn line_at(&self, start: f32) -> usize {
        let Some(thumb) = self.thumb(0) else {
            return 0;
        };
        let travel = self.length - thumb.length;
        if travel <= 0.0 {
            return 0;
        }
        let scrolled = ((start - self.start) / travel).clamp(0.0, 1.0);
        nearest(f64::from(scrolled) * count(self.last)).min(self.last)
    }

    /// Where the view is scrolled to after a click at `at` along the track when it was at
    /// `first`: a page toward the click when it is beside the thumb, and no change on the
    /// thumb.
    pub(crate) fn paged(&self, first: usize, at: f32) -> usize {
        let Some(thumb) = self.thumb(first) else {
            return first;
        };
        let page = self.shown.saturating_sub(1).max(1);
        if thumb.holds(at) {
            first
        } else if at < thumb.start {
            first.saturating_sub(page)
        } else {
            first.saturating_add(page).min(self.last)
        }
    }

    /// Where the view is scrolled to after this frame's drag or click of the bar, whose
    /// interaction is `response`, when it was at `first`. `grab` is where on the thumb a drag
    /// took hold, kept between the frames of the drag.
    pub(crate) fn follow(
        &self,
        response: &Response,
        axis: Axis,
        grab: &mut Option<f32>,
        first: usize,
    ) -> usize {
        let Some(thumb) = self.thumb(first) else {
            *grab = None;
            return first.min(self.last);
        };
        let pointer = response.interact_pointer_pos().map(|pos| axis.along(pos));
        if response.drag_started() {
            // A drag from beside the thumb takes it by its middle.
            *grab = pointer.map(|at| {
                if thumb.holds(at) {
                    at - thumb.start
                } else {
                    thumb.length / 2.0
                }
            });
        }
        let to = match (pointer, *grab) {
            (Some(at), Some(grab)) if response.dragged() => self.line_at(at - grab),
            (Some(at), _) if response.clicked() => self.paged(first, at),
            _ => first,
        };
        if response.drag_stopped() {
            *grab = None;
        }
        to
    }
}

/// `n` as a number of lines or points, exact to `u32::MAX`, past which it saturates.
fn count(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// `some` of `all` as a part from 0 to 1; 0 when there is nothing to take a part of.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a part from 0 to 1: the nearest f32 is the value wanted"
)]
fn part(some: usize, all: usize) -> f32 {
    if all == 0 {
        return 0.0;
    }
    (count(some) / count(all)).clamp(0.0, 1.0) as f32
}

/// `n` points as a length on screen.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a width in points: the nearest f32 is the value wanted"
)]
pub(crate) fn span(n: usize) -> f32 {
    count(n) as f32
}

/// The whole number nearest `x`, a count of lines or points that is not negative.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=u32::MAX first, so the cast is exact"
)]
pub(crate) fn nearest(x: f64) -> usize {
    usize::try_from(x.round().clamp(0.0, f64::from(u32::MAX)) as u32).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use egui::{Pos2, Rect};

    use super::{Axis, Thumb, Track};

    // No spec statement: P7-16: the thumb is drawn along its bar from its start for its
    // length, three points in from the bar's sides
    #[test]
    fn the_thumb_is_drawn_inset_in_its_bar() {
        let thumb = Thumb {
            start: 40.0,
            length: 30.0,
        };
        let down = Rect::from_min_max(Pos2::new(788.0, 0.0), Pos2::new(800.0, 400.0));
        assert_eq!(
            thumb.shape(Axis::Down, down),
            Rect::from_min_max(Pos2::new(791.0, 40.0), Pos2::new(797.0, 70.0))
        );
        let across = Rect::from_min_max(Pos2::new(30.0, 388.0), Pos2::new(788.0, 400.0));
        assert_eq!(
            thumb.shape(Axis::Across, across),
            Rect::from_min_max(Pos2::new(40.0, 391.0), Pos2::new(70.0, 397.0))
        );
    }

    // No spec statement: P7-16: the thumb's height is the part of the file the view shows and
    // its place the part scrolled past; at the last line it reaches the track's end
    #[test]
    fn the_thumb_follows_the_scroll_position_and_the_files_length() {
        // 100 lines in a view of 25 rows: 124 rows to scroll through, a thumb of 25 of them.
        let track = Track::of_lines(10.0, 496.0, 100, 25);
        let top = track.thumb(0).unwrap();
        assert!((top.start - 10.0).abs() < 0.001, "{top:?}");
        assert!((top.length - 100.0).abs() < 0.001, "{top:?}");
        let end = track.thumb(99).unwrap();
        assert!((end.start + end.length - 506.0).abs() < 0.001, "{end:?}");
        let past = track.thumb(5_000).unwrap();
        assert_eq!(past, end, "a line past the end is the last");
        let middle = track.thumb(33).unwrap();
        assert!(
            (middle.start - (10.0 + 396.0 / 3.0)).abs() < 0.01,
            "{middle:?}"
        );
        // A file twice as long has a thumb about half as high.
        let longer = Track::of_lines(10.0, 496.0, 200, 25).thumb(0).unwrap();
        assert!(longer.length < top.length * 0.6, "{longer:?}");
    }

    // No spec statement: P7-16: a file the view shows whole has no thumb; a long file's thumb
    // is no shorter than can be grabbed
    #[test]
    fn a_short_file_has_no_thumb_and_a_long_one_a_thumb_to_grab() {
        assert_eq!(Track::of_lines(0.0, 400.0, 10, 20).thumb(0), None);
        assert_eq!(Track::of_lines(0.0, 400.0, 20, 20).thumb(0), None);
        assert!(Track::of_lines(0.0, 400.0, 21, 20).thumb(0).is_some());
        assert_eq!(Track::of_lines(0.0, 0.0, 500, 20).thumb(0), None);
        assert_eq!(Track::of_lines(0.0, 400.0, 0, 20).thumb(0), None);
        let long = Track::of_lines(0.0, 400.0, 50_000, 20)
            .thumb(25_000)
            .unwrap();
        assert!((long.length - 24.0).abs() < 0.001, "{long:?}");
        assert!(long.start > 180.0 && long.start < 196.0, "{long:?}");
    }

    // No spec statement: P7-16: a drag puts the line under the thumb first, the track's ends
    // the file's first and last lines; a click beside the thumb pages toward it
    #[test]
    fn a_drag_scrolls_to_the_line_under_the_thumb_and_a_click_pages() {
        let track = Track::of_lines(10.0, 496.0, 100, 25);
        assert_eq!(track.line_at(10.0), 0);
        assert_eq!(track.line_at(-50.0), 0);
        assert_eq!(track.line_at(406.0), 99);
        assert_eq!(track.line_at(9_000.0), 99);
        for first in [0, 1, 33, 50, 98, 99] {
            let thumb = track.thumb(first).unwrap();
            assert_eq!(track.line_at(thumb.start), first, "the thumb's own place");
        }
        let thumb = track.thumb(50).unwrap();
        assert_eq!(track.paged(50, thumb.start + 1.0), 50, "on the thumb");
        assert_eq!(track.paged(50, thumb.start - 1.0), 26, "above it");
        assert_eq!(
            track.paged(50, thumb.start + thumb.length + 1.0),
            74,
            "below it"
        );
        assert_eq!(track.paged(5, 10.0), 0);
        assert_eq!(track.paged(90, 505.0), 99);
        assert_eq!(
            Track::of_lines(0.0, 400.0, 10, 20).paged(3, 100.0),
            3,
            "no thumb"
        );
        assert_eq!(Track::of_lines(0.0, 400.0, 10, 20).line_at(100.0), 0);
    }

    // No spec statement: P7-16: across the view the thumb's length is the part of the widest
    // line the view shows, and the line's end scrolls to the view's edge and no further;
    // lines no wider than the view have no thumb
    #[test]
    fn the_bar_across_scrolls_until_the_widest_lines_end() {
        let track = Track::of_width(50.0, 400.0, 1_600, 400);
        assert_eq!(track.last(), 1_200);
        let start = track.thumb(0).unwrap();
        assert!((start.start - 50.0).abs() < 0.001, "{start:?}");
        assert!((start.length - 100.0).abs() < 0.001, "{start:?}");
        let end = track.thumb(1_200).unwrap();
        assert!((end.start + end.length - 450.0).abs() < 0.001, "{end:?}");
        assert_eq!(track.line_at(450.0), 1_200);
        assert_eq!(track.line_at(end.start), 1_200);
        assert_eq!(track.paged(0, 449.0), 399, "a view's width less a point");
        assert_eq!(Track::of_width(50.0, 400.0, 400, 400).thumb(0), None);
        assert_eq!(Track::of_width(50.0, 400.0, 90, 400).thumb(0), None);
    }
}
