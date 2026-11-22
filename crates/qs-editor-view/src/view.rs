// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor widget: input to edits, and the visible rows drawn from the layout cache.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use egui::output::IMEOutput;
use egui::text::CCursor;
use egui::{
    Color32, CornerRadius, Event, EventFilter, FontFamily, FontId, Galley, Id, Key, Modifiers,
    Pos2, Rect, Response, Sense, Ui, UiBuilder, Vec2, accesskit,
};

use crate::buffer::Position;
use crate::diff::Diff;
use crate::fold::{Fold, Folds};
use crate::layout::{LayoutCache, Preedit, Styler, inlay_job, line_job};
use crate::scrollbar::{self, Axis};
use crate::selection::{EditState, Extend, Motion, Selection};
use crate::vim::{Mode, Vim, VimKey};

/// The colours and font the view draws with; the app fills it from its theme tokens.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewStyle {
    /// The code font.
    pub font: FontId,
    /// The editor's background, `surface.editor`.
    pub background: Color32,
    /// The line the primary caret is on, `surface.current_line`.
    pub current_line: Color32,
    /// Selected text's background, `select.focus`.
    pub selection: Color32,
    /// Text no styler run colours, `text.code`.
    pub text: Color32,
    /// Line numbers, `text.faint`.
    pub line_number: Color32,
    /// The carets, `accent`.
    pub caret: Color32,
    /// The background of a line new since the diff's base, `diff.added`.
    pub added: Color32,
    /// The background of a line the diff's base had and the text no longer does,
    /// `status.error_bg`.
    pub removed: Color32,
    /// The text of such a line, `text.on_error`.
    pub removed_text: Color32,
    /// The background of the line the debugger is paused on, `debug.paused_line`.
    pub paused_line: Color32,
    /// The gutter's mark on that line, `status.warn`.
    pub paused_mark: Color32,
    /// The gutter's mark on a line with a breakpoint, `status.error`.
    pub breakpoint: Color32,
    /// The indent guides, `border`.
    pub indent_guide: Color32,
    /// The gutter's mark on a line added since `HEAD`, `status.ok`.
    pub added_mark: Color32,
    /// The gutter's mark on a line changed since `HEAD`, `status.warn`.
    pub modified_mark: Color32,
    /// The gutter's mark where lines were removed since `HEAD`, `status.error`.
    pub removed_mark: Color32,
    /// Inlay hints, `text.faint`.
    pub inlay: Color32,
    /// An error's note at its line's end, `status.error`.
    pub note_error: Color32,
    /// A warning's note at its line's end, `status.warn`.
    pub note_warning: Color32,
    /// The mark before a note that has a quick fix, `accent`.
    pub lightbulb: Color32,
    /// The scroll bar's thumb, `border`.
    pub scroll_thumb: Color32,
    /// The thumb under the pointer or dragged, `text.faint`.
    pub scroll_thumb_active: Color32,
}

impl Default for ViewStyle {
    /// The dark theme's tokens (`docs/design/tokens.toml`), in the 13 px code size.
    fn default() -> Self {
        Self {
            font: FontId::new(13.0, FontFamily::Monospace),
            background: Color32::from_rgb(0x1a, 0x19, 0x18),
            current_line: Color32::from_rgb(0x22, 0x21, 0x20),
            selection: Color32::from_rgb(0x2e, 0x36, 0x40),
            text: Color32::from_rgb(0xd9, 0xd2, 0xc7),
            line_number: Color32::from_rgb(0x8e, 0x87, 0x7d),
            caret: Color32::from_rgb(0xee, 0x8b, 0x45),
            added: Color32::from_rgb(0x24, 0x33, 0x1f),
            removed: Color32::from_rgb(0x3a, 0x22, 0x20),
            removed_text: Color32::from_rgb(0xf2, 0xd2, 0xcc),
            paused_line: Color32::from_rgb(0x3a, 0x31, 0x22),
            paused_mark: Color32::from_rgb(0xe2, 0xb0, 0x4a),
            breakpoint: Color32::from_rgb(0xec, 0x7a, 0x6c),
            indent_guide: Color32::from_rgb(0x2a, 0x28, 0x26),
            added_mark: Color32::from_rgb(0x7f, 0xb0, 0x69),
            modified_mark: Color32::from_rgb(0xe2, 0xb0, 0x4a),
            removed_mark: Color32::from_rgb(0xec, 0x7a, 0x6c),
            inlay: Color32::from_rgb(0x8e, 0x87, 0x7d),
            note_error: Color32::from_rgb(0xec, 0x7a, 0x6c),
            note_warning: Color32::from_rgb(0xe2, 0xb0, 0x4a),
            lightbulb: Color32::from_rgb(0xee, 0x8b, 0x45),
            scroll_thumb: Color32::from_rgb(0x2a, 0x28, 0x26),
            scroll_thumb_active: Color32::from_rgb(0x8e, 0x87, 0x7d),
        }
    }
}

/// The space between the line numbers and the text, and before the numbers, in points.
const GUTTER_PAD: f32 = 12.0;
/// The fold mark's half width is at most a row over this: a quarter.
const FOLD_MARK_PARTS: f32 = 4.0;
/// A breakpoint's radius is at most a row over this: a third.
const BREAKPOINT_PARTS: f32 = 3.0;
/// A fixable note's dot's radius is a row over this: a quarter.
const FIX_DOT_PARTS: f32 = 4.0;
/// A character's most UTF-8 bytes.
const MAX_UTF8_BYTES: u8 = 4;
/// The caret's width in points.
const CARET_WIDTH: f32 = 2.0;
/// What the scroll bar's id adds to the view's.
const SCROLL_BAR_ID: &str = "scroll-bar";
/// The id, under the view's, of the scroll bar across it.
const SCROLL_BAR_ACROSS_ID: &str = "scroll-bar-across";
/// The characters kept in view past a caret scrolled into view across.
const REVEAL_CHARS: f32 = 4.0;
/// The part of the view's width those characters take at most: one in this many.
const REVEAL_VIEW_PARTS: f32 = 4.0;
/// The rounding of the scroll bar's thumb's corners, in points.
const SCROLL_THUMB_ROUNDING: u8 = 3;
/// The gutter's mark on a removed line's row.
const REMOVED_MARK: &str = "-";
/// What a folded line shows after its text.
const FOLDED_MARK: &str = "\u{2026}";
/// How many spaces make one indent level, for the indent guides.
const INDENT_SPACES: usize = 4;
/// The accessible name of the editor.
const LABEL: &str = "Code editor";

/// How serious a line's note is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteSeverity {
    /// An error.
    Error,
    /// A warning.
    Warning,
}

/// Whether a note has a quick fix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fixable {
    /// It has none.
    No,
    /// It has one: a lightbulb before it, which a click opens.
    Yes,
}

/// A diagnostic's message shown at the end of its line, as Zed's inline diagnostics and VS
/// Code's Error Lens show them (P4-34).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineNote {
    /// The line, from 0.
    pub line: usize,
    /// The message.
    pub text: String,
    /// How serious it is.
    pub severity: NoteSeverity,
    /// Whether it has a quick fix.
    pub fixable: Fixable,
}

/// An edit suggested at a line's end, in faint text, and after it the words that say which
/// key takes it, in a pill, as the approved play-loop mockup shows a next-edit suggestion
/// (P4-11, UI-7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineSuggestion {
    /// The line, from 0.
    pub line: usize,
    /// What is suggested.
    pub text: String,
    /// The pill's words.
    pub accept: String,
}

/// Whether long lines wrap at the view's width (P4-32).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wrap {
    /// Each line is one row, however long.
    #[default]
    Off,
    /// A line longer than the view is wide wraps onto more rows, at word ends where it can.
    Width,
}

/// How the editor reads keys (P4-37).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyMode {
    /// As every code editor does: typing inserts, the arrows move.
    #[default]
    Standard,
    /// Vim's modes, motions and operators ([`crate::vim`]); the arrows and the command
    /// shortcuts still work as in [`KeyMode::Standard`].
    Vim,
}

/// The editor: its text, selections, scroll position, IME composition and layout cache.
#[derive(Debug)]
pub struct EditorView {
    id: Id,
    state: EditState,
    /// The first visible line.
    first_line: usize,
    /// Scrolled points not yet a whole line.
    scroll_rest: f32,
    /// Where on the thumb of the scroll bar down the view a drag of it took hold, in points
    /// below its top; and the same of the bar across, in points right of its left.
    scroll_grab: Option<f32>,
    scroll_grab_across: Option<f32>,
    /// The points of the text scrolled past the view's left edge, with wrap off (P7-16).
    scroll_x: f32,
    /// The widest line drawn in the last frame, in points.
    widest_shown: f32,
    /// The width of the text's longest line, as last measured.
    longest: Option<Measured>,
    /// The primary caret's place when it was last scrolled into view across.
    revealed: Position,
    /// The visible lines in the last frame, for paging.
    visible: usize,
    /// IME composition text at the primary caret, not yet in the text.
    preedit: Option<String>,
    /// The text position under the pointer in the last frame, if it was over the text.
    hovered: Option<Position>,
    /// The base text the inline diff is from, and the diff with the text at a revision.
    diff: Option<DiffState>,
    /// The text at `HEAD` the gutter marks changes against, and the diff with the text at a
    /// revision (P4-35).
    gutter_base: Option<DiffState>,
    /// Where the primary caret was drawn in the last frame, if it was on screen.
    caret_rect: Option<Rect>,
    /// The lines, from 0, the gutter marks with a breakpoint.
    breakpoints: BTreeSet<usize>,
    /// The line, from 0, the debugger is paused on.
    paused: Option<usize>,
    /// The line whose gutter was clicked in the last frame, not yet taken.
    gutter_click: Option<usize>,
    /// Vim's state, in [`KeyMode::Vim`].
    vim: Option<Vim>,
    /// The foldable regions and the folded ones (P4-32).
    folds: Folds,
    /// Whether long lines wrap.
    wrap: Wrap,
    /// The inlay hints by line: each byte column and the text shown before it (P4-34).
    inlays: BTreeMap<usize, Vec<(usize, String)>>,
    /// Changes whenever the inlay hints do, so their lines are laid out again.
    inlay_revision: u64,
    /// The notes at the ends of lines, by line (P4-34).
    notes: BTreeMap<usize, LineNote>,
    /// The suggested edit shown at a line's end, in place of that line's note (P4-11).
    suggestion: Option<LineSuggestion>,
    /// Where each fixable note was drawn in the last frame, and its line.
    note_rects: Vec<(Rect, usize)>,
    /// The line of the fixable note a click landed on, not yet taken.
    note_click: Option<usize>,
    /// The width lines wrap at in the last frame, in points.
    wrap_width: f32,
    cache: LayoutCache,
}

impl EditorView {
    /// A view of `text` with the egui id `id`, one caret at the start.
    #[must_use]
    pub fn new(id: Id, text: &str) -> Self {
        Self {
            id,
            state: EditState::new(text),
            first_line: 0,
            scroll_rest: 0.0,
            scroll_grab: None,
            scroll_grab_across: None,
            scroll_x: 0.0,
            widest_shown: 0.0,
            longest: None,
            revealed: Position::default(),
            visible: 1,
            preedit: None,
            hovered: None,
            diff: None,
            gutter_base: None,
            caret_rect: None,
            breakpoints: BTreeSet::new(),
            paused: None,
            gutter_click: None,
            vim: None,
            folds: Folds::default(),
            wrap: Wrap::Off,
            inlays: BTreeMap::new(),
            inlay_revision: 0,
            notes: BTreeMap::new(),
            suggestion: None,
            note_rects: Vec::new(),
            note_click: None,
            wrap_width: f32::INFINITY,
            cache: LayoutCache::default(),
        }
    }

    /// Shows `hints`, each text drawn before its position, as inlay hints (P4-34).
    pub fn set_inlays(&mut self, hints: impl IntoIterator<Item = (Position, String)>) {
        let mut inlays: BTreeMap<usize, Vec<(usize, String)>> = BTreeMap::new();
        for (at, text) in hints {
            inlays.entry(at.line).or_default().push((at.col, text));
        }
        for line in inlays.values_mut() {
            line.sort_by_key(|(col, _)| *col);
        }
        if inlays != self.inlays {
            self.inlays = inlays;
            self.inlay_revision = self.inlay_revision.wrapping_add(1);
        }
    }

    /// Shows `notes` at the ends of their lines, one per line, the first given kept.
    pub fn set_notes(&mut self, notes: impl IntoIterator<Item = LineNote>) {
        self.notes.clear();
        for note in notes {
            self.notes.entry(note.line).or_insert(note);
        }
    }

    /// Shows `suggestion` at the end of its line, in place of that line's note, or none.
    pub fn set_suggestion(&mut self, suggestion: Option<LineSuggestion>) {
        self.suggestion = suggestion;
    }

    /// Whether `line` is one of the rows the view draws: scrolled to, and in no closed
    /// fold.
    pub fn shows_line(&mut self, line: usize) -> bool {
        let first = self
            .first_line
            .min(self.state.buffer().line_count().saturating_sub(1));
        self.rows(first, self.visible)
            .iter()
            .any(|row| matches!(row, Row::Line { line: shown, .. } if *shown == line))
    }

    /// The suggestion shown, if one is.
    #[must_use]
    pub const fn suggestion(&self) -> Option<&LineSuggestion> {
        self.suggestion.as_ref()
    }

    /// The line whose fixable note a click landed on since this was last asked.
    pub const fn take_note_click(&mut self) -> Option<usize> {
        self.note_click.take()
    }

    /// Wraps long lines, or not.
    pub const fn set_wrap(&mut self, wrap: Wrap) {
        self.wrap = wrap;
    }

    /// Whether long lines wrap.
    #[must_use]
    pub const fn wrap(&self) -> Wrap {
        self.wrap
    }

    /// The foldable regions and the folded ones.
    #[must_use]
    pub const fn folds(&self) -> &Folds {
        &self.folds
    }

    /// The folds, to fold or unfold; a fold that hides the primary caret's line is undone in
    /// the next frame, so the caret always shows.
    pub const fn folds_mut(&mut self) -> &mut Folds {
        &mut self.folds
    }

    /// Reads keys as `mode` does; a change to Vim starts in Normal mode.
    pub fn set_key_mode(&mut self, mode: KeyMode) {
        match (mode, &self.vim) {
            (KeyMode::Vim, None) => self.vim = Some(Vim::new()),
            (KeyMode::Standard, Some(_)) => self.vim = None,
            (KeyMode::Vim, Some(_)) | (KeyMode::Standard, None) => {}
        }
    }

    /// Vim's mode, in [`KeyMode::Vim`].
    #[must_use]
    pub fn vim_mode(&self) -> Option<Mode> {
        self.vim.as_ref().map(Vim::mode)
    }

    /// Marks the gutter of `lines`, from 0, with a breakpoint, and no other line.
    pub fn set_breakpoints(&mut self, lines: impl IntoIterator<Item = usize>) {
        self.breakpoints = lines.into_iter().collect();
    }

    /// The lines, from 0, the gutter marks with a breakpoint.
    #[must_use]
    pub const fn breakpoints(&self) -> &BTreeSet<usize> {
        &self.breakpoints
    }

    /// Highlights `line`, from 0, as the one the debugger is paused on, or no line.
    pub const fn set_paused(&mut self, line: Option<usize>) {
        self.paused = line;
    }

    /// The line, from 0, whose gutter a click landed on since this was last asked, to set or
    /// clear a breakpoint there; such a click moves no caret.
    pub const fn take_gutter_click(&mut self) -> Option<usize> {
        self.gutter_click.take()
    }

    /// The text and its selections.
    #[must_use]
    pub fn state(&self) -> &EditState {
        &self.state
    }

    /// The text and its selections, to change from outside the view (go to a definition, apply
    /// an edit).
    pub fn state_mut(&mut self) -> &mut EditState {
        &mut self.state
    }

    /// The egui id the view takes focus and AccessKit nodes by.
    #[must_use]
    pub fn id(&self) -> Id {
        self.id
    }

    /// Gives the view the egui id `id`, as when it moves to another pane.
    pub const fn set_id(&mut self, id: Id) {
        self.id = id;
    }

    /// The first visible line.
    #[must_use]
    pub fn first_line(&self) -> usize {
        self.first_line
    }

    /// Scrolls so `line` is the first visible line, clamped to the text.
    pub fn scroll_to(&mut self, line: usize) {
        self.first_line = line.min(self.state.buffer().line_count().saturating_sub(1));
    }

    /// Shows the text's changes since `base` inline: new lines tinted, and the base's removed
    /// lines as rows of their own above where they were; `None` shows no diff.
    pub fn set_diff_base(&mut self, base: Option<String>) {
        self.diff = base.map(|base| DiffState {
            diff: Diff::new(&base, &self.state.buffer().text()),
            revision: self.state.revision(),
            base,
        });
    }

    /// Marks in the gutter the lines changed since `base`, the file's text at `HEAD`: a bar
    /// on each added or changed line and a mark where lines were removed, as VS Code and Zed
    /// mark them (P4-35); `None` marks none.
    pub fn set_gutter_base(&mut self, base: Option<String>) {
        self.gutter_base = base.map(|base| DiffState {
            diff: Diff::new(&base, &self.state.buffer().text()),
            revision: self.state.revision(),
            base,
        });
    }

    /// The gutter's diff as of the text's current revision, if one is marked.
    #[must_use]
    pub fn gutter_diff(&mut self) -> Option<&Diff> {
        let revision = self.state.revision();
        if let Some(state) = self.gutter_base.as_mut()
            && state.revision != revision
        {
            state.diff = Diff::new(&state.base, &self.state.buffer().text());
            state.revision = revision;
        }
        self.gutter_base.as_ref().map(|state| &state.diff)
    }

    /// The inline diff as of the text's current revision, if one is shown.
    #[must_use]
    pub fn diff(&mut self) -> Option<&Diff> {
        self.refresh_diff();
        self.diff.as_ref().map(|state| &state.diff)
    }

    /// Works the diff out again when the text changed since it was.
    fn refresh_diff(&mut self) {
        let revision = self.state.revision();
        if let Some(state) = self.diff.as_mut()
            && state.revision != revision
        {
            state.diff = Diff::new(&state.base, &self.state.buffer().text());
            state.revision = revision;
        }
    }

    /// The rows from line `first`, at most `count`: each line, after the diff's removed lines
    /// before it.
    fn rows(&mut self, first: usize, count: usize) -> Vec<Row> {
        self.refresh_diff();
        let line_count = self.state.buffer().line_count();
        let diff = self.diff.as_ref().map(|state| &state.diff);
        // The line count itself stands for the end, after the last line, where lines removed
        // at the end are drawn.
        let folds = &self.folds;
        (first..=line_count)
            .filter(|line| *line == line_count || !folds.is_hidden(*line))
            .flat_map(|line| {
                let removed = diff
                    .map_or_default(|d| d.removed_before(line))
                    .iter()
                    .map(|text| Row::Removed(text.clone()));
                let this = (line < line_count).then(|| Row::Line {
                    line,
                    added: diff.is_some_and(|d| d.is_added(line)),
                });
                removed.chain(this)
            })
            .take(count)
            .collect()
    }

    /// The points of the text scrolled past the view's left edge: 0 while long lines wrap.
    #[must_use]
    pub const fn scrolled_across(&self) -> f32 {
        self.scroll_x
    }

    /// Where the primary caret was in the last frame, `None` when it was off screen: where a
    /// popup about the text at the caret goes.
    #[must_use]
    pub fn caret_rect(&self) -> Option<Rect> {
        self.caret_rect
    }

    /// The text position under the pointer in the last frame, `None` when the pointer is not
    /// over the text.
    #[must_use]
    pub fn hovered(&self) -> Option<Position> {
        self.hovered
    }

    /// The IME composition text shown at the primary caret, if any.
    #[must_use]
    pub fn preedit(&self) -> Option<&str> {
        self.preedit.as_deref()
    }

    /// The layout cache, for measurements.
    #[must_use]
    pub fn cache(&self) -> &LayoutCache {
        &self.cache
    }

    /// Draws the view in all the space `ui` has left and handles this frame's input when it has
    /// focus. `styler` colours the lines.
    pub fn show(&mut self, ui: &mut Ui, style: &ViewStyle, styler: &dyn Styler) -> Response {
        let rect = ui.available_rect_before_wrap();
        let response = ui.interact(rect, self.id, Sense::click_and_drag());
        let row_height = ui.fonts_mut(|f| f.row_height(&style.font)).ceil();
        let char_width = ui.fonts_mut(|f| f.glyph_width(&style.font, '0'));
        let digits = digit_count(self.state.buffer().line_count());
        let gutter = GUTTER_PAD.mul_add(2.0, char_width * points(digits));
        self.visible = whole((rect.height() / row_height).ceil()).max(1);
        let text_left = rect.left() + gutter;

        if response.clicked() || response.drag_started() {
            ui.memory_mut(|m| m.request_focus(self.id));
        }
        let focus = if ui.memory(|m| m.has_focus(self.id)) {
            Focus::Focused
        } else {
            Focus::Unfocused
        };
        if focus == Focus::Focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    self.id,
                    EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                );
            });
        }
        let pixels_per_point = ui.ctx().pixels_per_point();
        self.wrap_width = match self.wrap {
            Wrap::Off => f32::INFINITY,
            // Whole points, so a frame a fraction wider lays nothing out again.
            // Short of the scroll bar, which would cover a line's last characters.
            Wrap::Width => (rect.right() - text_left - CARET_WIDTH - scrollbar::WIDTH)
                .floor()
                .max(char_width),
        };
        self.cache.begin(
            styler.revision(),
            &style.font,
            pixels_per_point,
            (self.wrap_width, self.inlay_revision),
        );
        // A caret moved into a folded region, by a key or from outside, unfolds it.
        let head = self.state.primary().head.line;
        self.folds.reveal(head);
        self.first_line = self.folds.shown_at_or_before(self.first_line);

        // Over the view, its scroll bars included, which are widgets of their own above it.
        if ui.rect_contains_pointer(rect) {
            let delta = ui.input(|i| i.smooth_scroll_delta);
            // The greater of the two ways only, as Zed scrolls: a swipe down a trackpad carries
            // a little sideways with it, which would drift a long line across.
            if delta.y.abs() >= delta.x.abs() {
                self.scroll(delta.y, row_height);
            } else {
                self.scroll_x -= delta.x;
            }
        }
        let thumbs = self.scroll_bars(ui, style, styler, (rect, text_left, char_width));
        let text_left_scrolled = text_left - self.scroll_x;
        // A press places the caret where the button went down, which for a drag is behind the
        // pointer by the drag threshold; each frame of a drag then moves the head to the pointer.
        let modifiers = ui.input(|i| i.modifiers);
        if response.clicked() || response.drag_started() {
            let origin = ui
                .input(|i| i.pointer.press_origin())
                .or_else(|| response.interact_pointer_pos());
            if let Some(origin) = origin {
                let at = self.position_at(
                    ui,
                    style,
                    styler,
                    origin,
                    rect.top(),
                    text_left_scrolled,
                    row_height,
                );
                let on_fold_mark =
                    origin.x >= text_left - GUTTER_PAD && self.folds.start_at(at.line).is_some();
                let on_note = self
                    .note_rects
                    .iter()
                    .find(|(rect, _)| rect.contains(origin))
                    .map(|(_, line)| *line);
                if origin.x < text_left {
                    if response.clicked() && on_fold_mark {
                        self.folds.toggle_start(at.line);
                    } else if response.clicked() {
                        self.gutter_click = Some(at.line);
                    }
                } else if let (true, Some(line)) = (response.clicked(), on_note) {
                    self.note_click = Some(line);
                } else {
                    self.press(at, modifiers);
                }
            }
        }
        self.hovered = match response.hover_pos() {
            Some(pos) if pos.x >= text_left => Some(self.position_at(
                ui,
                style,
                styler,
                pos,
                rect.top(),
                text_left_scrolled,
                row_height,
            )),
            Some(_) | None => None,
        };
        if response.dragged()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let at = self.position_at(
                ui,
                style,
                styler,
                pos,
                rect.top(),
                text_left_scrolled,
                row_height,
            );
            self.extend_to(at);
        }
        if focus == Focus::Focused {
            self.input(ui);
        }
        // A caret a key moved is in view across in this frame; the bar's thumb follows in
        // the next.
        let before = self.scroll_x;
        self.across(ui, style, styler, (text_left, rect.right(), char_width));
        #[expect(clippy::float_cmp, reason = "any change at all is one to draw")]
        if self.scroll_x != before {
            ui.ctx().request_repaint();
        }

        self.paint(ui, style, styler, rect, gutter, row_height, focus);
        for (shape, active) in thumbs.into_iter().flatten() {
            let colour = if active {
                style.scroll_thumb_active
            } else {
                style.scroll_thumb
            };
            ui.painter_at(rect).rect_filled(
                shape,
                CornerRadius::same(SCROLL_THUMB_ROUNDING),
                colour,
            );
        }
        self.cache.end();
        response
    }

    /// This frame's input to the two scroll bars of the view in `rect`, whose text starts at
    /// `text_left` in characters `char_width` wide: the view scrolled as they say, and each
    /// bar's thumb to draw with whether the pointer is on its bar.
    fn scroll_bars(
        &mut self,
        ui: &Ui,
        style: &ViewStyle,
        styler: &dyn Styler,
        (rect, text_left, char_width): (Rect, f32, f32),
    ) -> [Option<(Rect, bool)>; 2] {
        let across = self.across(ui, style, styler, (text_left, rect.right(), char_width));
        let lines = self.state.buffer().line_count();
        // The rows that fit whole: a last row cut by the view's edge is not one to page by.
        let down = scrollbar::Track::of_lines(
            rect.top(),
            rect.height(),
            lines,
            self.visible.saturating_sub(1).max(1),
        );
        let down_bar = Rect::from_min_max(
            Pos2::new(rect.right() - scrollbar::WIDTH, rect.top()),
            rect.max,
        );
        let across_bar = Rect::from_min_max(
            Pos2::new(text_left, rect.bottom() - scrollbar::WIDTH),
            Pos2::new(rect.right() - scrollbar::WIDTH, rect.bottom()),
        );
        let (first, down_thumb) = self.bar(ui, Axis::Down, down, down_bar, self.first_line);
        self.first_line = self.folds.shown_at_or_before(first);
        let scrolled = scrollbar::nearest(f64::from(self.scroll_x));
        let (scrolled, across_thumb) = self.bar(ui, Axis::Across, across, across_bar, scrolled);
        if across_thumb.is_some_and(|(_, active)| active) {
            self.scroll_x = scrollbar::span(scrolled);
        }
        [down_thumb, across_thumb]
    }

    /// One of the view's scroll bars (P7-16), in `bar`: a drag of its thumb scrolls to the
    /// place under it, and a click beside the thumb pages toward the click. Where the view is
    /// scrolled to after this frame's input, it being at `first` before; and the thumb to
    /// draw, with whether the pointer is on the bar or dragging it, none when the view shows
    /// everything, which leaves the bar's place to the text.
    fn bar(
        &mut self,
        ui: &Ui,
        axis: Axis,
        track: scrollbar::Track,
        bar: Rect,
        first: usize,
    ) -> (usize, Option<(Rect, bool)>) {
        let (id, grab) = match axis {
            Axis::Down => (SCROLL_BAR_ID, &mut self.scroll_grab),
            Axis::Across => (SCROLL_BAR_ACROSS_ID, &mut self.scroll_grab_across),
        };
        if track.thumb(first).is_none() {
            *grab = None;
            return (first, None);
        }
        let response = ui.interact(bar, self.id.with(id), Sense::click_and_drag());
        let to = track.follow(&response, axis, grab, first);
        let active = response.hovered() || response.dragged();
        let thumb = track
            .thumb(to)
            .map(|thumb| (thumb.shape(axis, bar), active));
        (to, thumb)
    }

    /// The track of the scroll bar across the view, whose text is from `left` to `right` and
    /// whose characters are `char_width` wide, after the primary caret, when it has moved,
    /// is scrolled into view and the scroll is held to the widest line's end. Wrapped lines
    /// are never wider than the view, and so never scrolled.
    fn across(
        &mut self,
        ui: &Ui,
        style: &ViewStyle,
        styler: &dyn Styler,
        (left, right, char_width): (f32, f32, f32),
    ) -> scrollbar::Track {
        let view = (right - left - scrollbar::WIDTH).max(char_width);
        if self.wrap == Wrap::Width {
            self.scroll_x = 0.0;
            return scrollbar::Track::of_width(left, view, 0, 0);
        }
        let revision = self.state.revision();
        let longest = match &self.longest {
            Some(known) if known.revision == revision && known.font == style.font => known.width,
            Some(_) | None => {
                let buffer = self.state.buffer();
                let line = (0..buffer.line_count())
                    .max_by_key(|&line| buffer.line(line).len())
                    .unwrap_or(0);
                // Laid out once for its width and not kept: the cache holds the lines drawn.
                let job = line_job(buffer.line(line), &[], &style.font, style.text, None);
                let width = ui.fonts_mut(|f| f.layout_job(job)).size().x;
                self.longest = Some(Measured {
                    revision,
                    font: style.font.clone(),
                    width,
                });
                width
            }
        };
        // Any wider line that was drawn counts too: a line of tabs or wide characters can
        // be wider than a longer one.
        let mut content = longest.max(self.widest_shown);
        let head = self.state.primary().head;
        if head != self.revealed {
            self.revealed = head;
            let galley = self.galley(ui, style, styler, head.line, None);
            let chars = self.char_index(head.line, head.col, None);
            let caret = galley.pos_from_cursor(CCursor::new(chars)).min.x;
            content = content.max(caret);
            // A few characters past the caret stay in view, as far as the view has room.
            let margin = (char_width * REVEAL_CHARS).min(view / REVEAL_VIEW_PARTS);
            if caret < self.scroll_x + margin {
                self.scroll_x = caret - margin;
            } else if caret > self.scroll_x + view - margin {
                self.scroll_x = caret - view + margin;
            }
        }
        // Room for the caret after the widest line's last character.
        let content = scrollbar::nearest(f64::from(content + char_width));
        let track =
            scrollbar::Track::of_width(left, view, content, scrollbar::nearest(f64::from(view)));
        self.scroll_x = self.scroll_x.clamp(0.0, scrollbar::span(track.last()));
        track
    }

    /// Scrolls by `delta` points of wheel movement, whole lines at a time.
    pub(crate) fn scroll(&mut self, delta: f32, row_height: f32) {
        self.scroll_rest -= delta / row_height;
        let lines = self.scroll_rest.trunc();
        if lines.abs() < 1.0 {
            return;
        }
        self.scroll_rest -= lines;
        let count = whole(lines.abs());
        let last = self.state.buffer().line_count().saturating_sub(1);
        // Line by line over the lines shown, a folded region counting as its first line.
        for _ in 0..count {
            self.first_line = if lines < 0.0 {
                self.folds
                    .shown_at_or_before(self.first_line.saturating_sub(1))
            } else {
                self.folds
                    .shown_at_or_after(self.first_line.saturating_add(1))
                    .min(last)
            };
        }
    }

    /// The text position under the pointer at `pos`.
    #[expect(
        clippy::too_many_arguments,
        reason = "the frame's metrics, computed once in show"
    )]
    fn position_at(
        &mut self,
        ui: &Ui,
        style: &ViewStyle,
        styler: &dyn Styler,
        pos: Pos2,
        top: f32,
        text_left: f32,
        row_height: f32,
    ) -> Position {
        let last = self.state.buffer().line_count().saturating_sub(1);
        let rows = self.rows(self.first_line, self.visible.saturating_add(1));
        let mut row_top = top;
        let mut after = self.first_line;
        for row in rows {
            match row {
                // A removed line's row places the caret at the start of the line after it.
                Row::Removed(_) => {
                    if pos.y < row_top + row_height {
                        return if after > last {
                            self.state.buffer().end()
                        } else {
                            Position::new(after, 0)
                        };
                    }
                    row_top += row_height;
                }
                Row::Line { line, .. } => {
                    let galley = self.galley(ui, style, styler, line, None);
                    let height = line_height(&galley, row_height);
                    after = line.saturating_add(1);
                    if pos.y < row_top + height {
                        let at = Vec2::new(pos.x - text_left, (pos.y - row_top).max(0.0));
                        let cursor = galley.cursor_from_pos(at);
                        return Position::new(line, self.byte_of(line, cursor.index.0));
                    }
                    row_top += height;
                }
            }
        }
        // Below the last row drawn: its line's end, or the text's.
        let line = after.saturating_sub(1).min(last);
        let galley = self.galley(ui, style, styler, line, None);
        let cursor = galley.cursor_from_pos(Vec2::new(pos.x - text_left, galley.size().y));
        Position::new(line, self.byte_of(line, cursor.index.0))
    }

    /// The character index in `line`'s laid-out text of byte column `col`: past the preedit
    /// when after it, and past the inlay hints at or before it.
    fn char_index(&self, line: usize, col: usize, preedit: Option<Preedit<'_>>) -> usize {
        let mut chars = self.state.buffer().char_column(line, col);
        if let Some(p) = preedit
            && col >= p.col
        {
            chars = chars.saturating_add(p.text.chars().count());
        }
        let inlays = self.inlays.get(&line).filter(|_| preedit.is_none());
        for (at, hint) in inlays.into_iter().flatten() {
            if *at <= col {
                chars = chars.saturating_add(hint.chars().count());
            }
        }
        chars
    }

    /// The byte column of `line` at character `index` of its laid-out text, past the inlay
    /// hints before it; an index inside a hint is the column the hint stands before.
    fn byte_of(&self, line: usize, index: usize) -> usize {
        let buffer = self.state.buffer();
        let mut shown = 0_usize;
        for (col, hint) in self.inlays.get(&line).into_iter().flatten() {
            let start = buffer.char_column(line, *col).saturating_add(shown);
            if index < start {
                break;
            }
            let len = hint.chars().count();
            if index < start.saturating_add(len) {
                return *col;
            }
            shown = shown.saturating_add(len);
        }
        buffer.byte_column(line, index.saturating_sub(shown))
    }

    /// A press at `at`: Shift extends the primary selection to it, Alt adds a caret there, and
    /// otherwise it is the only caret.
    fn press(&mut self, at: Position, modifiers: Modifiers) {
        if modifiers.shift {
            self.extend_to(at);
        } else if modifiers.alt {
            let mut selections = vec![Selection::caret(at)];
            selections.extend_from_slice(self.state.selections());
            self.state.set_selections(&selections);
        } else {
            self.state.set_selections(&[Selection::caret(at)]);
        }
        self.preedit = None;
    }

    /// Moves the primary selection's head to `at`, keeping its anchor and the other selections.
    fn extend_to(&mut self, at: Position) {
        let primary = self.state.primary();
        let mut selections = vec![Selection {
            anchor: primary.anchor,
            head: at,
        }];
        selections.extend(
            self.state
                .selections()
                .iter()
                .filter(|s| **s != primary)
                .copied(),
        );
        self.state.set_selections(&selections);
        self.preedit = None;
    }

    /// This frame's keyboard, clipboard and IME events.
    fn input(&mut self, ui: &Ui) {
        let events = ui.input(|i| i.events.clone());
        let mut moved = false;
        for event in events {
            moved |= match event {
                Event::Text(text) if !text.chars().any(char::is_control) => {
                    if let Some(vim) = &mut self.vim {
                        let mut moved = false;
                        for c in text.chars() {
                            moved |= self.state.vim_key(vim, VimKey::Char(c));
                        }
                        moved
                    } else {
                        self.state.insert(&text);
                        true
                    }
                }
                Event::Paste(text) => {
                    self.state.insert(&text);
                    true
                }
                Event::Copy => {
                    ui.ctx().copy_text(self.state.selected_text());
                    false
                }
                Event::Cut => {
                    ui.ctx().copy_text(self.state.selected_text());
                    self.state.insert("");
                    true
                }
                Event::Ime(ime) => self.ime(ime),
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => self.key(key, modifiers),
                _ => false,
            };
        }
        if moved {
            self.reveal();
        }
    }

    /// An IME event: a preedit shows composition text at the primary caret, a commit inserts it.
    fn ime(&mut self, event: egui::ImeEvent) -> bool {
        match event {
            egui::ImeEvent::Preedit { text, .. } => {
                self.preedit = (!text.is_empty()).then_some(text);
                true
            }
            egui::ImeEvent::Commit(text) => {
                self.preedit = None;
                if text.is_empty() {
                    return false;
                }
                self.state.insert(&text);
                true
            }
            egui::ImeEvent::DeleteSurrounding {
                before_chars,
                after_chars,
            } => {
                for _ in 0..before_chars {
                    self.state.backspace();
                }
                for _ in 0..after_chars {
                    self.state.delete();
                }
                true
            }
            #[expect(
                deprecated,
                reason = "egui no longer sends them, but the variants remain"
            )]
            egui::ImeEvent::Enabled | egui::ImeEvent::Disabled => false,
        }
    }

    /// A key press; true when it moved a caret or changed the text.
    fn key(&mut self, key: Key, modifiers: Modifiers) -> bool {
        if let Some(vim) = &mut self.vim
            && !modifiers.command
            && !modifiers.alt
        {
            let vim_key = match key {
                Key::Escape => Some(VimKey::Escape),
                Key::Enter => Some(VimKey::Enter),
                Key::Backspace => Some(VimKey::Backspace),
                _ => None,
            };
            if let Some(vim_key) = vim_key {
                self.preedit = None;
                return self.state.vim_key(vim, vim_key);
            }
        }
        let extend = if modifiers.shift {
            Extend::Select
        } else {
            Extend::Move
        };
        let page = self.visible.saturating_sub(1).max(1);
        let motion = match key {
            Key::ArrowDown if modifiers.alt && modifiers.command => {
                self.state.add_caret(true);
                return true;
            }
            Key::ArrowUp if modifiers.alt && modifiers.command => {
                self.state.add_caret(false);
                return true;
            }
            Key::A if modifiers.command => {
                self.state.select_all();
                return true;
            }
            Key::Z if modifiers.command && modifiers.shift => return self.state.redo(),
            Key::Z if modifiers.command => return self.state.undo(),
            Key::Escape => {
                self.state.single_caret();
                self.preedit = None;
                return true;
            }
            Key::Enter => {
                self.state.newline();
                return true;
            }
            Key::Tab => {
                self.state.insert("\t");
                return true;
            }
            Key::Backspace => {
                self.state.backspace();
                return true;
            }
            Key::Delete => {
                self.state.delete();
                return true;
            }
            Key::ArrowLeft => Motion::Left,
            Key::ArrowRight => Motion::Right,
            Key::ArrowUp if modifiers.command => Motion::TextStart,
            Key::ArrowDown if modifiers.command => Motion::TextEnd,
            Key::ArrowUp => Motion::Up,
            Key::ArrowDown => Motion::Down,
            Key::Home => Motion::Home,
            Key::End => Motion::End,
            Key::PageUp => Motion::PageUp(page),
            Key::PageDown => Motion::PageDown(page),
            _ => return false,
        };
        self.state.motion(motion, extend);
        // A line motion that lands in a folded region goes past it, as the rows show, rather
        // than unfolding it.
        let head = self.state.primary().head;
        if self.folds.is_hidden(head.line) {
            let line = match motion {
                Motion::Up | Motion::PageUp(_) => self.folds.shown_at_or_before(head.line),
                _ => self.folds.shown_at_or_after(head.line),
            };
            let at = self.state.buffer().clamp(Position::new(line, head.col));
            match extend {
                Extend::Select => self.extend_to(at),
                Extend::Move => self.state.set_selections(&[Selection::caret(at)]),
            }
        }
        true
    }

    /// Scrolls the primary caret into view.
    fn reveal(&mut self) {
        let line = self.state.primary().head.line;
        if line < self.first_line {
            self.first_line = line;
            return;
        }
        if !self.folds.any_folded() {
            let last_visible = self
                .first_line
                .saturating_add(self.visible.saturating_sub(1));
            if line > last_visible {
                self.first_line = line.saturating_sub(self.visible.saturating_sub(1));
            }
            return;
        }
        // With folds, the rows above the caret are the lines shown: walk back from the caret
        // over a screen of them.
        let mut first = line;
        let mut shown = 1_usize;
        while shown < self.visible && first > self.first_line {
            first = self.folds.shown_at_or_before(first.saturating_sub(1));
            shown = shown.saturating_add(1);
        }
        if first > self.first_line {
            self.first_line = first;
        }
    }

    /// Line `line`'s layout, from the cache unless `preedit` is spliced into it.
    fn galley(
        &mut self,
        ui: &Ui,
        style: &ViewStyle,
        styler: &dyn Styler,
        line: usize,
        preedit: Option<Preedit<'_>>,
    ) -> Arc<Galley> {
        let buffer = self.state.buffer();
        let text = buffer.line(line);
        let wrap_width = self.wrap_width;
        let inlays = self.inlays.get(&line).filter(|_| preedit.is_none());
        let make = || {
            let spans = styler.spans(line, text);
            let mut job = match inlays {
                Some(inlays) => {
                    inlay_job(text, &spans, &style.font, (style.text, style.inlay), inlays)
                }
                None => line_job(text, &spans, &style.font, style.text, preedit),
            };
            job.wrap.max_width = wrap_width;
            ui.fonts_mut(|f| f.layout_job(job))
        };
        match (preedit, buffer.line_id(line)) {
            (None, Some(id)) => self.cache.get(id, make),
            (Some(_), _) | (None, None) => make(),
        }
    }

    /// Paints the visible lines, the selections, the carets and the gutter, and publishes the
    /// IME caret and the AccessKit nodes.
    #[expect(
        clippy::too_many_arguments,
        reason = "the frame's metrics, computed once in show"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "one pass over the frame's rows, each drawn whole where its top is known"
    )]
    fn paint(
        &mut self,
        ui: &mut Ui,
        style: &ViewStyle,
        styler: &dyn Styler,
        rect: Rect,
        gutter: f32,
        row_height: f32,
        focus: Focus,
    ) {
        let painter = ui.painter_at(rect);
        let mut note_rects = Vec::new();
        painter.rect_filled(rect, CornerRadius::ZERO, style.background);
        let line_count = self.state.buffer().line_count();
        let first = self.first_line.min(line_count.saturating_sub(1));
        let rows = self.rows(first, self.visible);
        let changes = self.gutter_diff().cloned();
        let primary = self.state.primary();
        let selections = self.state.selections().to_vec();
        let composing = self.preedit.clone();
        let text_left = rect.left() + gutter;
        // The text is drawn scrolled across, and only right of the gutter.
        let text_x = text_left - self.scroll_x;
        let text_painter = painter.with_clip_rect(
            Rect::from_min_max(Pos2::new(text_left, rect.top()), rect.max).intersect(rect),
        );
        let mut widest = 0.0_f32;
        let mut ime_caret = None;
        let mut runs: Vec<(Rect, String, Vec<u8>)> = Vec::new();
        let mut top = rect.top();
        for kind in rows {
            if top >= rect.bottom() {
                break;
            }
            let (line, added) = match kind {
                Row::Line { line, added } => (line, added),
                Row::Removed(text) => {
                    let row_rect = Rect::from_min_size(
                        Pos2::new(rect.left(), top),
                        Vec2::new(rect.width(), row_height),
                    );
                    removed_row(
                        ui,
                        (&painter, &text_painter),
                        style,
                        &text,
                        (row_rect, text_left, text_x),
                    );
                    top += row_height;
                    continue;
                }
            };
            let preedit = match &composing {
                Some(text) if line == primary.head.line => Some(Preedit {
                    col: primary.head.col,
                    text: text.as_str(),
                }),
                Some(_) | None => None,
            };
            let galley = self.galley(ui, style, styler, line, preedit);
            let height = line_height(&galley, row_height);
            widest = widest.max(galley.size().x);
            let row_rect =
                Rect::from_min_size(Pos2::new(rect.left(), top), Vec2::new(rect.width(), height));
            let paused = self.paused == Some(line);
            if paused {
                painter.rect_filled(row_rect, CornerRadius::ZERO, style.paused_line);
            } else if added {
                painter.rect_filled(row_rect, CornerRadius::ZERO, style.added);
            } else if line == primary.head.line {
                painter.rect_filled(row_rect, CornerRadius::ZERO, style.current_line);
            }
            let buffer = self.state.buffer();
            // A byte column's character index in the galley, past the preedit when after it.
            let chars_of = |col: usize| self.char_index(line, col, preedit);
            let text_top = top + (height - galley.size().y).max(0.0) / 2.0;
            indent_guides(
                &text_painter,
                style,
                &galley,
                buffer.line(line),
                (text_x, text_top),
            );
            let line_end = buffer.line(line).len();
            paint_selections(
                &text_painter,
                style,
                &galley,
                (&selections, line, line_end),
                &chars_of,
                (text_x, text_top, top, height),
            );
            let number = (line.saturating_add(1).to_string(), style.line_number);
            text_row(
                (&painter, &text_painter),
                style,
                Arc::clone(&galley),
                (top, height),
                (text_left, text_x),
                number,
            );
            let end = galley.rows.last().map_or(0.0, |row| row.rect().right());
            let at = Pos2::new(text_x + end + NOTE_GAP, text_top);
            if let Some(suggestion) = self.suggestion.as_ref().filter(|s| s.line == line) {
                paint_suggestion(&text_painter, style, suggestion, at, row_height);
            } else if let Some(note) = self.notes.get(&line)
                && let Some(clickable) = paint_note(&text_painter, style, note, at, row_height)
            {
                note_rects.push((clickable, line));
            }
            if let Some(fold) = self.folds.start_at(line) {
                fold_mark(
                    (&painter, &text_painter),
                    style,
                    &galley,
                    fold,
                    (text_left, text_x, top),
                    row_height,
                );
            }
            let marks = Marks {
                breakpoint: self.breakpoints.contains(&line),
                paused,
            };
            marks.paint(&painter, style, Pos2::new(rect.left(), top), row_height);
            if let Some(diff) = &changes {
                change_mark(&painter, style, diff, line, (rect.left(), top, height));
            }
            for selection in selections.iter().filter(|s| s.head.line == line) {
                let chars = match preedit {
                    Some(p) if *selection == primary => buffer
                        .char_column(line, p.col)
                        .saturating_add(p.text.chars().count()),
                    Some(_) | None => chars_of(selection.head.col),
                };
                let at = galley.pos_from_cursor(CCursor::new(chars));
                // On a wrapped line the caret is on its row; an unwrapped line's fills the row.
                let caret_top = if galley.rows.len() <= 1 {
                    top
                } else {
                    text_top + at.min.y
                };
                let caret = Rect::from_min_size(
                    Pos2::new(text_x + at.min.x, caret_top + 1.0),
                    Vec2::new(CARET_WIDTH, row_height - 2.0),
                );
                if focus == Focus::Focused {
                    text_painter.rect_filled(caret, CornerRadius::ZERO, style.caret);
                }
                if *selection == primary {
                    ime_caret = Some(caret);
                }
            }
            let mut value = buffer.line(line).to_owned();
            value.push('\n');
            let lengths = value
                .chars()
                .map(|c| u8::try_from(c.len_utf8()).unwrap_or(MAX_UTF8_BYTES))
                .collect();
            runs.push((row_rect, value, lengths));
            top += height;
        }
        self.caret_rect = ime_caret;
        self.widest_shown = widest;
        self.note_rects = note_rects;
        if focus == Focus::Focused
            && let Some(cursor_rect) = ime_caret
        {
            ui.output_mut(|o| {
                o.ime = Some(IMEOutput {
                    purpose: egui::IMEPurpose::Normal,
                    rect,
                    cursor_rect,
                    should_interrupt_composition: false,
                });
            });
        }
        self.accessibility(ui, runs, first);
    }

    /// The AccessKit tree: the editor as a multi-line text input and one text run per visible
    /// line, each its text and a newline with each character's UTF-8 length, and the primary
    /// selection between the runs it starts and ends on, clamped to the visible lines. egui
    /// parents a node by `Ui`, so each run is in a child `Ui` parented to the editor.
    fn accessibility(&self, ui: &mut Ui, runs: Vec<(Rect, String, Vec<u8>)>, first: usize) {
        let mut ids = Vec::with_capacity(runs.len());
        for (row, (row_rect, value, lengths)) in runs.into_iter().enumerate() {
            let child = ui.new_child(
                UiBuilder::new()
                    .id_salt(("text-run", row))
                    .max_rect(row_rect)
                    .accessibility_parent(self.id),
            );
            let id = child.unique_id();
            ids.push(id);
            ui.ctx().accesskit_node_builder(id, |node| {
                node.set_role(accesskit::Role::TextRun);
                node.set_value(value);
                node.set_character_lengths(lengths);
                node.set_bounds(accesskit::Rect {
                    x0: row_rect.min.x.into(),
                    y0: row_rect.min.y.into(),
                    x1: row_rect.max.x.into(),
                    y1: row_rect.max.y.into(),
                });
            });
        }
        let buffer = self.state.buffer();
        let primary = self.state.primary();
        let position = |at: Position| {
            let row = at.line.checked_sub(first)?;
            let id = ids.get(row)?;
            Some(accesskit::TextPosition {
                node: id.accesskit_id(),
                character_index: buffer.char_column(at.line, at.col),
            })
        };
        let selection = position(primary.anchor)
            .zip(position(primary.head))
            .map(|(anchor, focus)| accesskit::TextSelection { anchor, focus });
        ui.ctx().accesskit_node_builder(self.id, |node| {
            node.set_role(accesskit::Role::MultilineTextInput);
            node.set_label(LABEL);
            if let Some(selection) = selection {
                node.set_text_selection(selection);
            }
        });
    }
}

/// The width of one character of `galley`'s font, from its first row, or 0 for an empty line.
fn char_width_of(galley: &Galley) -> f32 {
    galley
        .rows
        .first()
        .and_then(|row| row.glyphs.first())
        .map_or(0.0, |glyph| glyph.advance_width)
}

/// The number of decimal digits of `n`, at least 2 so the gutter does not jump at line 10.
fn digit_count(n: usize) -> usize {
    n.to_string().len().max(2)
}

/// `n` as points: a count of visible rows or digits, small enough to be exact; a count past
/// `u16::MAX` saturates.
fn points(n: usize) -> f32 {
    u16::try_from(n).map_or_else(|_| f32::from(u16::MAX), f32::from)
}

/// `x`, a non-negative count of rows on screen, as a whole number, saturating at `u16::MAX`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=u16::MAX first, so the cast is exact"
)]
fn whole(x: f32) -> usize {
    usize::from(x.clamp(0.0, f32::from(u16::MAX)) as u16)
}

/// The width of the text's longest line, measured at a revision of the text in a font.
#[derive(Debug)]
struct Measured {
    revision: u64,
    font: FontId,
    width: f32,
}

/// The inline diff's base text, and its diff with the text as of a revision.
#[derive(Debug)]
struct DiffState {
    base: String,
    diff: Diff,
    revision: u64,
}

/// One row the view draws: a line of the text, or a line the diff's base had.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Row {
    /// A line of the text, and whether it is new since the diff's base.
    Line { line: usize, added: bool },
    /// A line of the base that the text no longer has.
    Removed(String),
}

/// The marks in a line's gutter, before its number.
#[derive(Clone, Copy)]
struct Marks {
    /// The line has a breakpoint: a dot.
    breakpoint: bool,
    /// The debugger is paused on it: an arrow, over the dot.
    paused: bool,
}

impl Marks {
    /// Paints them centred in the space before the numbers of the row whose top left is
    /// `corner`.
    fn paint(self, painter: &egui::Painter, style: &ViewStyle, corner: Pos2, row_height: f32) {
        let mark = Pos2::new(corner.x + GUTTER_PAD / 2.0, corner.y + row_height / 2.0);
        let radius = (GUTTER_PAD / 2.0 - 1.0).min(row_height / BREAKPOINT_PARTS);
        if self.breakpoint {
            painter.circle_filled(mark, radius, style.breakpoint);
        }
        if self.paused {
            let arrow = vec![
                Pos2::new(mark.x - radius, mark.y - radius),
                Pos2::new(mark.x, mark.y - radius),
                Pos2::new(mark.x + radius, mark.y),
                Pos2::new(mark.x, mark.y + radius),
                Pos2::new(mark.x - radius, mark.y + radius),
            ];
            painter.add(egui::Shape::convex_polygon(
                arrow,
                style.paused_mark,
                egui::Stroke::NONE,
            ));
        }
    }
}

/// Draws one row's text `galley` at `text_left`, centred in the row at `top` of `row_height`,
/// and `gutter`'s text right-aligned before it in its colour: a line's number, or a removed
/// line's mark.
fn text_row(
    (painter, text_painter): (&egui::Painter, &egui::Painter),
    style: &ViewStyle,
    galley: Arc<Galley>,
    (top, row_height): (f32, f32),
    (text_left, text_x): (f32, f32),
    gutter: (String, Color32),
) {
    let text_top = top + (row_height - galley.size().y).max(0.0) / 2.0;
    let (label, color) = gutter;
    text_painter.galley(Pos2::new(text_x, text_top), galley, style.text);
    painter.text(
        Pos2::new(text_left - GUTTER_PAD, text_top),
        egui::Align2::RIGHT_TOP,
        label,
        style.font.clone(),
        color,
    );
}

/// Paints the bands of `selections` on `line`, whose text ends at byte `line_end`, drawn as
/// `galley` at `text_left` and `text_top` in its row from `top`, `height` tall; `chars_of` turns
/// a byte column into the galley's character index.
fn paint_selections(
    painter: &egui::Painter,
    style: &ViewStyle,
    galley: &Galley,
    (selections, line, line_end): (&[Selection], usize, usize),
    chars_of: &dyn Fn(usize) -> usize,
    (text_left, text_top, top, height): (f32, f32, f32, f32),
) {
    for selection in selections {
        let (start, finish) = (selection.start(), selection.end());
        if selection.is_caret() || line < start.line || line > finish.line {
            continue;
        }
        let from = if line == start.line { start.col } else { 0 };
        let to = if line == finish.line {
            finish.col
        } else {
            line_end
        };
        let past_end = if line == finish.line {
            0.0
        } else {
            char_width_of(galley).max(CARET_WIDTH)
        };
        let one_row = galley.rows.len() <= 1;
        for band in bands(galley, chars_of(from), chars_of(to), past_end) {
            let mut band = band.translate(Vec2::new(text_left, text_top));
            if one_row {
                // An unwrapped line's band fills its row, as the line's tint does.
                band = Rect::from_x_y_ranges(band.x_range(), top..=top + height);
            }
            painter.rect_filled(band, CornerRadius::ZERO, style.selection);
        }
    }
}

/// Paints a line the diff's base had, `text`, in `row_rect` with its text at `text_left`.
fn removed_row(
    ui: &Ui,
    painters: (&egui::Painter, &egui::Painter),
    style: &ViewStyle,
    text: &str,
    (row_rect, text_left, text_x): (Rect, f32, f32),
) {
    painters
        .0
        .rect_filled(row_rect, CornerRadius::ZERO, style.removed);
    let galley =
        ui.fonts_mut(|f| f.layout_job(line_job(text, &[], &style.font, style.removed_text, None)));
    let gutter = (REMOVED_MARK.to_owned(), style.removed_text);
    text_row(
        painters,
        style,
        galley,
        (row_rect.top(), row_rect.height()),
        (text_left, text_x),
        gutter,
    );
}

/// The space between a line's text and its note, in points.
const NOTE_GAP: f32 = 24.0;

/// Paints `note` from `at`, the lightbulb first when it has a quick fix; the rectangle a click
/// opens the fix from, for a fixable note.
fn paint_note(
    painter: &egui::Painter,
    style: &ViewStyle,
    note: &LineNote,
    at: Pos2,
    row_height: f32,
) -> Option<Rect> {
    let color = match note.severity {
        NoteSeverity::Error => style.note_error,
        NoteSeverity::Warning => style.note_warning,
    };
    let mut x = at.x;
    if note.fixable == Fixable::Yes {
        let radius = row_height / FIX_DOT_PARTS;
        painter.circle_filled(
            Pos2::new(x + radius, at.y + row_height / 2.0),
            radius,
            style.lightbulb,
        );
        x += radius * 2.0 + radius;
    }
    let drawn = painter.text(
        Pos2::new(x, at.y),
        egui::Align2::LEFT_TOP,
        &note.text,
        style.font.clone(),
        color,
    );
    (note.fixable == Fixable::Yes).then(|| Rect::from_min_max(at, drawn.max))
}

/// The space between a suggestion's text and its pill, and the pill's padding at each side
/// and its corners' radius, in points: the mockup's 12, 6 and 4.
const PILL_GAP: f32 = 12.0;
const PILL_PAD: f32 = 6.0;
const PILL_RADIUS: f32 = 4.0;

/// Paints `suggestion` from `at`: its text in the inlay hints' faint colour, then its pill,
/// a rounded outline around the key's words.
fn paint_suggestion(
    painter: &egui::Painter,
    style: &ViewStyle,
    suggestion: &LineSuggestion,
    at: Pos2,
    row_height: f32,
) {
    let text = painter.text(
        at,
        egui::Align2::LEFT_TOP,
        &suggestion.text,
        style.font.clone(),
        style.inlay,
    );
    let words = Pos2::new(text.right() + PILL_GAP + PILL_PAD, at.y);
    let words = painter.text(
        words,
        egui::Align2::LEFT_TOP,
        &suggestion.accept,
        style.font.clone(),
        style.inlay,
    );
    let pill = Rect::from_min_max(
        Pos2::new(words.left() - PILL_PAD, at.y),
        Pos2::new(words.right() + PILL_PAD, at.y + row_height),
    );
    painter.rect_stroke(
        pill,
        PILL_RADIUS,
        egui::Stroke::new(1.0, style.indent_guide),
        egui::StrokeKind::Inside,
    );
}

/// The width of a change mark in the gutter, in points.
const CHANGE_MARK_WIDTH: f32 = 3.0;

/// The length of each dash of a changed line's mark, and of the gap after it, in points.
const CHANGE_DASH: f32 = 3.0;

/// Paints `line`'s change mark against `HEAD` at the gutter's left edge, `left`, in its row
/// from `top`, `height` tall: a solid bar on an added line, a dashed one on a changed line
/// (one with lines removed before it too), and a short square at the top where lines were
/// removed. The three differ in shape as well as colour, so colour is never the only sign
/// (P7-12; Apple's Human Interface Guidelines, Accessibility: "Convey information with more
/// than color alone").
fn change_mark(
    painter: &egui::Painter,
    style: &ViewStyle,
    diff: &Diff,
    line: usize,
    (left, top, height): (f32, f32, f32),
) {
    let removed = !diff.removed_before(line).is_empty();
    let (color, bar_height, dashed) = match (diff.is_added(line), removed) {
        (true, false) => (style.added_mark, height, false),
        (true, true) => (style.modified_mark, height, true),
        (false, true) => (style.removed_mark, CHANGE_MARK_WIDTH, false),
        (false, false) => return,
    };
    if !dashed {
        let bar = Rect::from_min_size(
            Pos2::new(left, top),
            Vec2::new(CHANGE_MARK_WIDTH, bar_height),
        );
        painter.rect_filled(bar, CornerRadius::ZERO, color);
        return;
    }
    let bottom = top + bar_height;
    let mut from = top;
    while from < bottom {
        let to = (from + CHANGE_DASH).min(bottom);
        let dash = Rect::from_min_max(
            Pos2::new(left, from),
            Pos2::new(left + CHANGE_MARK_WIDTH, to),
        );
        painter.rect_filled(dash, CornerRadius::ZERO, color);
        from = to + CHANGE_DASH;
    }
}

/// How tall line `galley` is drawn: one row, or each of its wrapped rows.
fn line_height(galley: &Galley, row_height: f32) -> f32 {
    galley.size().y.ceil().max(row_height)
}

/// The rectangles, relative to `galley`, that a selection of its characters `from..to` covers:
/// one for each wrapped row it touches, the last widened by `past_end` for a selection that
/// goes on past the line's end.
fn bands(galley: &Galley, from: usize, to: usize, past_end: f32) -> Vec<Rect> {
    let mut out = Vec::new();
    let mut start = 0_usize;
    let last = galley.rows.len().saturating_sub(1);
    for (index, placed) in galley.rows.iter().enumerate() {
        let count = placed.row.char_count_excluding_newline().0;
        let end = start.saturating_add(count);
        let (a, b) = (from.max(start), to.min(end));
        // A selection that only touches a wrapped row's end is drawn on the next row.
        let touches = a < b || (a == b && index == last && from <= end && to >= start);
        if touches || (a <= b && index == last) {
            let row = placed.rect();
            let left = row.left()
                + placed
                    .row
                    .x_offset(egui::text::CharIndex(a.saturating_sub(start)));
            let mut right = row.left()
                + placed
                    .row
                    .x_offset(egui::text::CharIndex(b.saturating_sub(start)));
            if index == last {
                right += past_end;
            }
            if right > left {
                out.push(Rect::from_min_max(
                    Pos2::new(left, row.top()),
                    Pos2::new(right, row.bottom()),
                ));
            }
        }
        start = end;
        if start >= to {
            break;
        }
    }
    out
}

/// The indent levels of `text`'s leading white space: the character index each level after
/// the first starts at, a tab one level and [`INDENT_SPACES`] spaces another.
fn indent_levels(text: &str) -> Vec<usize> {
    let mut levels = Vec::new();
    let mut spaces = 0_usize;
    for (index, c) in text.chars().enumerate() {
        match c {
            '\t' => {
                spaces = 0;
                levels.push(index.saturating_add(1));
            }
            ' ' => {
                spaces = spaces.saturating_add(1);
                if spaces == INDENT_SPACES {
                    spaces = 0;
                    levels.push(index.saturating_add(1));
                }
            }
            _ => break,
        }
    }
    // A guide at the start of the text, after the last level, is drawn by no level.
    if text.chars().all(char::is_whitespace) {
        levels.clear();
    }
    levels.pop();
    levels
}

/// Paints a thin vertical guide at each indent level of `text`, drawn as `galley` at `origin`,
/// over the first row (P4-32).
fn indent_guides(
    painter: &egui::Painter,
    style: &ViewStyle,
    galley: &Galley,
    text: &str,
    origin: (f32, f32),
) {
    let Some(first) = galley.rows.first() else {
        return;
    };
    let (left, top) = origin;
    for level in indent_levels(text) {
        let x = left + galley.pos_from_cursor(CCursor::new(level)).min.x;
        let height = galley.size().y;
        painter.line_segment(
            [
                Pos2::new(x, top),
                Pos2::new(x, top + height.max(first.rect().height())),
            ],
            egui::Stroke::new(1.0, style.indent_guide),
        );
    }
}

/// Whether the view has the keyboard focus, which draws its caret and takes its keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    /// Focused.
    Focused,
    /// Not focused.
    Unfocused,
}

/// Paints a fold region's mark in the gutter before the text at `origin`: a right-pointing
/// triangle when folded, a down-pointing one when open; a folded line also gets an ellipsis
/// after its text.
fn fold_mark(
    (painter, text_painter): (&egui::Painter, &egui::Painter),
    style: &ViewStyle,
    galley: &Galley,
    fold: Fold,
    origin: (f32, f32, f32),
    row_height: f32,
) {
    let (text_left, text_x, top) = origin;
    let centre = Pos2::new(text_left - GUTTER_PAD / 2.0, top + row_height / 2.0);
    let r = (GUTTER_PAD / 2.0 - 2.0).min(row_height / FOLD_MARK_PARTS);
    let points = if fold == Fold::Folded {
        vec![
            Pos2::new(centre.x - r / 2.0, centre.y - r),
            Pos2::new(centre.x + r / 2.0, centre.y),
            Pos2::new(centre.x - r / 2.0, centre.y + r),
        ]
    } else {
        vec![
            Pos2::new(centre.x - r, centre.y - r / 2.0),
            Pos2::new(centre.x + r, centre.y - r / 2.0),
            Pos2::new(centre.x, centre.y + r / 2.0),
        ]
    };
    painter.add(egui::Shape::convex_polygon(
        points,
        style.line_number,
        egui::Stroke::NONE,
    ));
    if fold == Fold::Folded {
        let end = galley.rows.last().map_or(0.0, |row| row.rect().right());
        let at = Pos2::new(text_x + end + GUTTER_PAD / 2.0, top);
        text_painter.text(
            at,
            egui::Align2::LEFT_TOP,
            FOLDED_MARK,
            style.font.clone(),
            style.line_number,
        );
    }
}
