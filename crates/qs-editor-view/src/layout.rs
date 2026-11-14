// SPDX-License-Identifier: GPL-3.0-or-later

//! How lines are styled, and the cache of their laid-out text.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Galley, Stroke};

use crate::buffer::{LineId, floor_char};

/// One styled run of a line: a byte range and how it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct StyledSpan {
    /// The bytes of the line it covers.
    pub range: Range<usize>,
    /// Its colour.
    pub color: Color32,
    /// Whether it is set in italics.
    pub italic: bool,
}

/// What colours a line: syntax or semantic highlighting, which P2-18 provides. The view keys its
/// layout cache by the revision, so a styler that colours differently returns a new one.
pub trait Styler {
    /// Changes whenever any line would be styled differently.
    fn revision(&self) -> u64;

    /// The styled runs of line `line`, whose text is `text`, in order and not overlapping; text
    /// no run covers is drawn in the view's text colour.
    fn spans(&self, line: usize, text: &str) -> Vec<StyledSpan>;
}

/// A styler that colours nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct Plain;

impl Styler for Plain {
    fn revision(&self) -> u64 {
        0
    }

    fn spans(&self, _line: usize, _text: &str) -> Vec<StyledSpan> {
        Vec::new()
    }
}

/// IME composition text placed into a line: its byte offset and text, drawn underlined.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Preedit<'a> {
    pub(crate) col: usize,
    pub(crate) text: &'a str,
}

/// The layout job of line `text` with `inlays`, each text drawn in `color` before its byte
/// offset, as inlay hints are (P4-34): the styler's runs and the gaps in `plain` around them.
pub(crate) fn inlay_job(
    text: &str,
    spans: &[StyledSpan],
    font: &FontId,
    (plain, color): (Color32, Color32),
    inlays: &[(usize, String)],
) -> LayoutJob {
    let mut job = LayoutJob::default();
    let format = |color: Color32, italic: bool| TextFormat {
        font_id: font.clone(),
        color,
        italics: italic,
        ..TextFormat::default()
    };
    let mut at = 0;
    let mut pieces: Vec<(Range<usize>, Color32, bool)> = Vec::new();
    for span in spans {
        let start = span.range.start.clamp(at, text.len());
        let end = span.range.end.clamp(start, text.len());
        if start > at {
            pieces.push((at..start, plain, false));
        }
        if end > start {
            pieces.push((start..end, span.color, span.italic));
        }
        at = end;
    }
    if at < text.len() {
        pieces.push((at..text.len(), plain, false));
    }
    let mut next = inlays.iter().peekable();
    for (range, piece_color, italic) in pieces {
        let mut start = range.start;
        while let Some((col, hint)) = next.peek() {
            let col = floor_char(text, *col);
            if col >= range.end {
                break;
            }
            if col > start {
                job.append(
                    text.get(start..col).unwrap_or(""),
                    0.0,
                    format(piece_color, italic),
                );
                start = col;
            }
            job.append(hint, 0.0, format(color, true));
            next.next();
        }
        job.append(
            text.get(start..range.end).unwrap_or(""),
            0.0,
            format(piece_color, italic),
        );
    }
    for (_, hint) in next {
        job.append(hint, 0.0, format(color, true));
    }
    if job.sections.is_empty() {
        job.append("", 0.0, format(plain, false));
    }
    job
}

/// The layout job of line `text`: its styler's runs, the gaps in `plain`, and a preedit spliced
/// in at its byte offset, underlined.
pub(crate) fn line_job(
    text: &str,
    spans: &[StyledSpan],
    font: &FontId,
    plain: Color32,
    preedit: Option<Preedit<'_>>,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    let format = |color: Color32, italic: bool, underline: bool| TextFormat {
        font_id: font.clone(),
        color,
        italics: italic,
        underline: if underline {
            Stroke::new(1.0, color)
        } else {
            Stroke::NONE
        },
        ..TextFormat::default()
    };
    let mut pieces: Vec<(Range<usize>, Color32, bool)> = Vec::new();
    let mut at = 0;
    for span in spans {
        let start = span.range.start.clamp(at, text.len());
        let end = span.range.end.clamp(start, text.len());
        if start > at {
            pieces.push((at..start, plain, false));
        }
        if end > start {
            pieces.push((start..end, span.color, span.italic));
        }
        at = end;
    }
    if at < text.len() {
        pieces.push((at..text.len(), plain, false));
    }
    let split = preedit.map(|p| floor_char(text, p.col));
    let mut placed = false;
    let mut place = |job: &mut LayoutJob| {
        if let Some(preedit) = preedit
            && !placed
        {
            job.append(preedit.text, 0.0, format(plain, false, true));
            placed = true;
        }
    };
    for (range, color, italic) in pieces {
        match split {
            Some(col) if range.start < col && col < range.end => {
                job.append(
                    text.get(range.start..col).unwrap_or(""),
                    0.0,
                    format(color, italic, false),
                );
                place(&mut job);
                job.append(
                    text.get(col..range.end).unwrap_or(""),
                    0.0,
                    format(color, italic, false),
                );
            }
            Some(col) if col == range.start => {
                place(&mut job);
                job.append(
                    text.get(range).unwrap_or(""),
                    0.0,
                    format(color, italic, false),
                );
            }
            Some(_) | None => {
                job.append(
                    text.get(range).unwrap_or(""),
                    0.0,
                    format(color, italic, false),
                );
            }
        }
    }
    place(&mut job);
    // A job with no section is laid out zero points tall, so an empty line has one empty
    // section in the font, to be as tall as every other line.
    if job.sections.is_empty() {
        job.append("", 0.0, format(plain, false, false));
    }
    job
}

/// A styler's revision, [`Styler::revision`]: a layout made with another is laid out again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StyleRevision(u64);

impl StyleRevision {
    /// Revision `revision`.
    pub(crate) const fn new(revision: u64) -> Self {
        Self(revision)
    }

    /// The revision.
    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

/// What a cached layout was made for: the line's text, the styler's revision, the font, the
/// screen's scale and the width lines wrap at.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    revision: u64,
    font: FontId,
    pixels_per_point: f32,
    wrap_width: f32,
    inlays: u64,
}

/// The laid-out text of each line, by line identity. An edit gives a line a new [`LineId`], so
/// only the edited lines are laid out again; a change of styler revision, font or scale clears
/// the cache. Lines not drawn in a frame are dropped at its end.
#[derive(Debug, Default)]
pub struct LayoutCache {
    key: Option<Key>,
    lines: HashMap<LineId, Arc<Galley>>,
    used: HashSet<LineId>,
    misses: u64,
}

impl LayoutCache {
    /// Starts a frame laid out with `revision`, `font`, `pixels_per_point`, lines wrapped at
    /// `wrap_width` (infinite for none) and the inlay hints of revision `inlays`, clearing the
    /// cache when any differs from the last frame's.
    pub(crate) fn begin(
        &mut self,
        revision: StyleRevision,
        font: &FontId,
        pixels_per_point: f32,
        (wrap_width, inlays): (f32, u64),
    ) {
        let key = Key {
            revision: revision.get(),
            font: font.clone(),
            pixels_per_point,
            wrap_width,
            inlays,
        };
        if self.key.as_ref() != Some(&key) {
            self.lines.clear();
            self.key = Some(key);
        }
        self.used.clear();
    }

    /// Line `id`'s layout, made by `make` when the cache has none.
    pub(crate) fn get(&mut self, id: LineId, make: impl FnOnce() -> Arc<Galley>) -> Arc<Galley> {
        self.used.insert(id);
        if let Some(galley) = self.lines.get(&id) {
            return Arc::clone(galley);
        }
        self.misses = self.misses.saturating_add(1);
        let galley = make();
        self.lines.insert(id, Arc::clone(&galley));
        galley
    }

    /// Ends the frame: lines not used in it are dropped.
    pub(crate) fn end(&mut self) {
        let used = &self.used;
        self.lines.retain(|id, _| used.contains(id));
    }

    /// How many layouts the cache has had to make, for tests and measurements.
    #[must_use]
    pub fn misses(&self) -> u64 {
        self.misses
    }

    /// How many lines it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether it holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}
