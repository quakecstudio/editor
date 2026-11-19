// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's vertical scroll bar (P7-16): where its thumb is for a scroll position, and the
//! line a drag of the thumb or a click beside it scrolls to. Zed's editor is the model: the
//! thumb's height is the part of the file the view shows, its place the part scrolled past,
//! and the file can be scrolled until its last line is the first shown.

/// The bar's width in points.
pub(crate) const WIDTH: f32 = 12.0;
/// The space left and right of the thumb inside the bar, in points.
pub(crate) const INSET: f32 = 3.0;
/// The thumb's least height in points, so it can be grabbed in a long file.
const MIN_THUMB: f32 = 24.0;

/// The thumb: its top and its height, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Thumb {
    /// Its top edge.
    pub(crate) top: f32,
    /// Its height.
    pub(crate) height: f32,
}

impl Thumb {
    /// Whether the height `y` is on the thumb.
    pub(crate) fn holds(self, y: f32) -> bool {
        (self.top..=self.top + self.height).contains(&y)
    }
}

/// The bar's track: where it is, and the file it scrolls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Track {
    top: f32,
    height: f32,
    /// The last line the view can have as its first: the file's last.
    last: usize,
    /// The rows the view shows.
    visible: usize,
}

impl Track {
    /// The track from `top` down `height` points, of a file of `lines` lines in a view of
    /// `visible` rows.
    pub(crate) const fn new(top: f32, height: f32, lines: usize, visible: usize) -> Self {
        Self {
            top,
            height,
            last: lines.saturating_sub(1),
            visible,
        }
    }

    /// The thumb when `first` is the first line shown; none when the view shows every line
    /// at once, or has no height.
    pub(crate) fn thumb(&self, first: usize) -> Option<Thumb> {
        if self.last < self.visible || self.height <= 0.0 {
            return None;
        }
        // Scrolled to its end the view shows the last line first, so the rows there are to
        // scroll through are the lines before it and a view of rows.
        let rows = self.last.saturating_add(self.visible);
        let height = (self.height * part(self.visible, rows))
            .max(MIN_THUMB)
            .min(self.height);
        let travel = self.height - height;
        Some(Thumb {
            top: travel.mul_add(part(first.min(self.last), self.last), self.top),
            height,
        })
    }

    /// The first line shown when the thumb's top is at the height `top`: a drag of the thumb.
    pub(crate) fn line_at(&self, top: f32) -> usize {
        let Some(thumb) = self.thumb(0) else {
            return 0;
        };
        let travel = self.height - thumb.height;
        if travel <= 0.0 {
            return 0;
        }
        let scrolled = ((top - self.top) / travel).clamp(0.0, 1.0);
        nearest(f64::from(scrolled) * count(self.last)).min(self.last)
    }

    /// The first line shown after a click at the height `y` with `first` shown first: a page
    /// toward the click when it is beside the thumb, and no change on the thumb.
    pub(crate) fn paged(&self, first: usize, y: f32) -> usize {
        let Some(thumb) = self.thumb(first) else {
            return first;
        };
        let page = self.visible.saturating_sub(1).max(1);
        if thumb.holds(y) {
            first
        } else if y < thumb.top {
            first.saturating_sub(page)
        } else {
            first.saturating_add(page).min(self.last)
        }
    }
}

/// `n` as a number of lines, exact to `u32::MAX`, past which it saturates.
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

/// The whole number nearest `x`, a count of lines that is not negative.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=u32::MAX first, so the cast is exact"
)]
fn nearest(x: f64) -> usize {
    usize::try_from(x.round().clamp(0.0, f64::from(u32::MAX)) as u32).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::Track;

    // No spec statement: P7-16: the thumb's height is the part of the file the view shows and
    // its place the part scrolled past; at the last line it reaches the track's end
    #[test]
    fn the_thumb_follows_the_scroll_position_and_the_files_length() {
        // 100 lines in a view of 25 rows: 124 rows to scroll through, a thumb of 25 of them.
        let track = Track::new(10.0, 496.0, 100, 25);
        let top = track.thumb(0).unwrap();
        assert!((top.top - 10.0).abs() < 0.001, "{top:?}");
        assert!((top.height - 100.0).abs() < 0.001, "{top:?}");
        let end = track.thumb(99).unwrap();
        assert!((end.top + end.height - 506.0).abs() < 0.001, "{end:?}");
        let past = track.thumb(5_000).unwrap();
        assert_eq!(past, end, "a line past the end is the last");
        let middle = track.thumb(33).unwrap();
        assert!(
            (middle.top - (10.0 + 396.0 / 3.0)).abs() < 0.01,
            "{middle:?}"
        );
        // A file twice as long has a thumb about half as high.
        let longer = Track::new(10.0, 496.0, 200, 25).thumb(0).unwrap();
        assert!(longer.height < top.height * 0.6, "{longer:?}");
    }

    // No spec statement: P7-16: a file the view shows whole has no thumb; a long file's thumb
    // is no shorter than can be grabbed
    #[test]
    fn a_short_file_has_no_thumb_and_a_long_one_a_thumb_to_grab() {
        assert_eq!(Track::new(0.0, 400.0, 10, 20).thumb(0), None);
        assert_eq!(Track::new(0.0, 400.0, 20, 20).thumb(0), None);
        assert!(Track::new(0.0, 400.0, 21, 20).thumb(0).is_some());
        assert_eq!(Track::new(0.0, 0.0, 500, 20).thumb(0), None);
        assert_eq!(Track::new(0.0, 400.0, 0, 20).thumb(0), None);
        let long = Track::new(0.0, 400.0, 50_000, 20).thumb(25_000).unwrap();
        assert!((long.height - 24.0).abs() < 0.001, "{long:?}");
        assert!(long.top > 180.0 && long.top < 196.0, "{long:?}");
    }

    // No spec statement: P7-16: a drag puts the line under the thumb first, the track's ends
    // the file's first and last lines; a click beside the thumb pages toward it
    #[test]
    fn a_drag_scrolls_to_the_line_under_the_thumb_and_a_click_pages() {
        let track = Track::new(10.0, 496.0, 100, 25);
        assert_eq!(track.line_at(10.0), 0);
        assert_eq!(track.line_at(-50.0), 0);
        assert_eq!(track.line_at(406.0), 99);
        assert_eq!(track.line_at(9_000.0), 99);
        for first in [0, 1, 33, 50, 98, 99] {
            let thumb = track.thumb(first).unwrap();
            assert_eq!(track.line_at(thumb.top), first, "the thumb's own place");
        }
        let thumb = track.thumb(50).unwrap();
        assert_eq!(track.paged(50, thumb.top + 1.0), 50, "on the thumb");
        assert_eq!(track.paged(50, thumb.top - 1.0), 26, "above it");
        assert_eq!(
            track.paged(50, thumb.top + thumb.height + 1.0),
            74,
            "below it"
        );
        assert_eq!(track.paged(5, 10.0), 0);
        assert_eq!(track.paged(90, 505.0), 99);
        assert_eq!(
            Track::new(0.0, 400.0, 10, 20).paged(3, 100.0),
            3,
            "no thumb"
        );
        assert_eq!(Track::new(0.0, 400.0, 10, 20).line_at(100.0), 0);
    }
}
