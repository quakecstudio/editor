// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor widget: input to edits, and the visible rows drawn from the layout cache.

use std::sync::Arc;

use egui::output::IMEOutput;
use egui::text::CCursor;
use egui::{
    Color32, CornerRadius, Event, EventFilter, FontFamily, FontId, Galley, Id, Key, Modifiers,
    Pos2, Rect, Response, Sense, Ui, UiBuilder, Vec2, accesskit,
};

use crate::buffer::Position;
use crate::layout::{LayoutCache, Preedit, Styler, line_job};
use crate::selection::{EditState, Extend, Motion, Selection};

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
        }
    }
}

/// The space between the line numbers and the text, and before the numbers, in points.
const GUTTER_PAD: f32 = 12.0;
/// The caret's width in points.
const CARET_WIDTH: f32 = 2.0;
/// The accessible name of the editor.
const LABEL: &str = "Code editor";

/// The editor: its text, selections, scroll position, IME composition and layout cache.
#[derive(Debug)]
pub struct EditorView {
    id: Id,
    state: EditState,
    /// The first visible line.
    first_line: usize,
    /// Scrolled points not yet a whole line.
    scroll_rest: f32,
    /// The visible lines in the last frame, for paging.
    visible: usize,
    /// IME composition text at the primary caret, not yet in the text.
    preedit: Option<String>,
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
            visible: 1,
            preedit: None,
            cache: LayoutCache::default(),
        }
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

    /// The first visible line.
    #[must_use]
    pub fn first_line(&self) -> usize {
        self.first_line
    }

    /// Scrolls so `line` is the first visible line, clamped to the text.
    pub fn scroll_to(&mut self, line: usize) {
        self.first_line = line.min(self.state.buffer().line_count().saturating_sub(1));
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
        let focused = ui.memory(|m| m.has_focus(self.id));
        if focused {
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
        self.cache
            .begin(styler.revision(), &style.font, pixels_per_point);

        if response.hovered() {
            let delta = ui.input(|i| i.smooth_scroll_delta.y);
            self.scroll(delta, row_height);
        }
        // A press places the caret where the button went down, which for a drag is behind the
        // pointer by the drag threshold; each frame of a drag then moves the head to the pointer.
        let modifiers = ui.input(|i| i.modifiers);
        if response.clicked() || response.drag_started() {
            let origin = ui
                .input(|i| i.pointer.press_origin())
                .or_else(|| response.interact_pointer_pos());
            if let Some(origin) = origin {
                let at =
                    self.position_at(ui, style, styler, origin, rect.top(), text_left, row_height);
                self.press(at, modifiers);
            }
        }
        if response.dragged()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let at = self.position_at(ui, style, styler, pos, rect.top(), text_left, row_height);
            self.extend_to(at);
        }
        if focused {
            self.input(ui);
        }

        self.paint(ui, style, styler, rect, gutter, row_height, focused);
        self.cache.end();
        response
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
        self.first_line = if lines < 0.0 {
            self.first_line.saturating_sub(count)
        } else {
            self.first_line.saturating_add(count).min(last)
        };
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
        let row = whole(((pos.y - top) / row_height).max(0.0));
        let line = self
            .first_line
            .saturating_add(row)
            .min(self.state.buffer().line_count().saturating_sub(1));
        let galley = self.galley(ui, style, styler, line, None);
        let cursor = galley.cursor_from_pos(Vec2::new(pos.x - text_left, 0.0));
        Position::new(line, self.state.buffer().byte_column(line, cursor.index.0))
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
                    self.state.insert(&text);
                    true
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
        true
    }

    /// Scrolls the primary caret into view.
    fn reveal(&mut self) {
        let line = self.state.primary().head.line;
        let last_visible = self
            .first_line
            .saturating_add(self.visible.saturating_sub(1));
        if line < self.first_line {
            self.first_line = line;
        } else if line > last_visible {
            self.first_line = line.saturating_sub(self.visible.saturating_sub(1));
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
        let make = || {
            let spans = styler.spans(line, text);
            let job = line_job(text, &spans, &style.font, style.text, preedit);
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
    fn paint(
        &mut self,
        ui: &mut Ui,
        style: &ViewStyle,
        styler: &dyn Styler,
        rect: Rect,
        gutter: f32,
        row_height: f32,
        focused: bool,
    ) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, style.background);
        let line_count = self.state.buffer().line_count();
        let first = self.first_line.min(line_count.saturating_sub(1));
        let end = first.saturating_add(self.visible).min(line_count);
        let primary = self.state.primary();
        let selections = self.state.selections().to_vec();
        let composing = self.preedit.clone();
        let text_left = rect.left() + gutter;
        let mut ime_caret = None;
        let mut runs: Vec<(Rect, String, Vec<u8>)> = Vec::new();
        for (row, line) in (first..end).enumerate() {
            let top = rect.top() + points(row) * row_height;
            let row_rect = Rect::from_min_size(
                Pos2::new(rect.left(), top),
                Vec2::new(rect.width(), row_height),
            );
            if line == primary.head.line {
                painter.rect_filled(row_rect, CornerRadius::ZERO, style.current_line);
            }
            let preedit = match &composing {
                Some(text) if line == primary.head.line => Some(Preedit {
                    col: primary.head.col,
                    text: text.as_str(),
                }),
                Some(_) | None => None,
            };
            let galley = self.galley(ui, style, styler, line, preedit);
            let buffer = self.state.buffer();
            let x_of = |col: usize| {
                let mut chars = buffer.char_column(line, col);
                if let Some(p) = preedit
                    && col >= p.col
                {
                    chars = chars.saturating_add(p.text.chars().count());
                }
                text_left + galley.pos_from_cursor(CCursor::new(chars)).min.x
            };
            let line_end = buffer.line(line).len();
            for selection in &selections {
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
                let mut right = x_of(to);
                if line != finish.line {
                    right += char_width_of(&galley).max(CARET_WIDTH);
                }
                let band = Rect::from_min_max(
                    Pos2::new(x_of(from), top),
                    Pos2::new(right, top + row_height),
                );
                painter.rect_filled(band, CornerRadius::ZERO, style.selection);
            }
            let text_top = top + (row_height - galley.size().y).max(0.0) / 2.0;
            painter.galley(
                Pos2::new(text_left, text_top),
                Arc::clone(&galley),
                style.text,
            );
            painter.text(
                Pos2::new(text_left - GUTTER_PAD, text_top),
                egui::Align2::RIGHT_TOP,
                line.saturating_add(1).to_string(),
                style.font.clone(),
                style.line_number,
            );
            for selection in selections.iter().filter(|s| s.head.line == line) {
                let mut col = selection.head.col;
                if *selection == primary
                    && let Some(p) = preedit
                {
                    col = col.saturating_add(p.text.len());
                }
                let x = match preedit {
                    Some(p) if *selection == primary => {
                        let chars = buffer
                            .char_column(line, p.col)
                            .saturating_add(p.text.chars().count());
                        text_left + galley.pos_from_cursor(CCursor::new(chars)).min.x
                    }
                    Some(_) | None => x_of(col),
                };
                let caret = Rect::from_min_size(
                    Pos2::new(x, top + 1.0),
                    Vec2::new(CARET_WIDTH, row_height - 2.0),
                );
                if focused {
                    painter.rect_filled(caret, CornerRadius::ZERO, style.caret);
                }
                if *selection == primary {
                    ime_caret = Some(caret);
                }
            }
            let mut value = buffer.line(line).to_owned();
            value.push('\n');
            let lengths = value
                .chars()
                .map(|c| u8::try_from(c.len_utf8()).unwrap_or(4))
                .collect();
            runs.push((row_rect, value, lengths));
        }
        if focused && let Some(cursor_rect) = ime_caret {
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
    u16::try_from(n).map_or(f32::from(u16::MAX), f32::from)
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
