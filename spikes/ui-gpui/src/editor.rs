// SPDX-License-Identifier: GPL-3.0-or-later
//! The code editor for gpui: a view that owns the document and handles actions, scrolling and
//! the platform input handler (IME), and an element that shapes and paints the visible rows.

use std::ops::Range;

use gpui::{
    App, Bounds, Context, ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle,
    Focusable, FontStyle, GlobalElementId, Hsla, LayoutId, MouseButton, MouseDownEvent, Pixels,
    Role, ScrollWheelEvent, ShapedLine, Style, TextRun, UTF16Selection, UnderlineStyle, Window,
    actions, div, fill, font, point, prelude::*, px, relative, rgb, size,
};
use ui_common::a11y;
use ui_common::document::{Cursor, Document, RowKind, clamp_to_char};
use ui_common::{script, theme};

actions!(
    editor,
    [
        Backspace,
        Newline,
        AddCursorBelow,
        SingleCursor,
        PageDown,
        PageUp
    ]
);

pub const ROW_HEIGHT: f32 = 20.0;
const FONT_SIZE: f32 = 13.0;
const GUTTER: f32 = 64.0;
const FONT_FAMILY: &str = "Menlo";

pub struct Editor {
    pub doc: Document,
    focus: FocusHandle,
    pub visible_rows: usize,
    scroll_accum: f32,
    caret_bounds: Option<Bounds<Pixels>>,
    char_width: Pixels,
    text_origin: Option<gpui::Point<Pixels>>,
}

fn hsla(color: u32) -> Hsla {
    rgb(color).into()
}

impl Editor {
    pub fn new(doc: Document, cx: &mut Context<Self>) -> Self {
        Self {
            doc,
            focus: cx.focus_handle(),
            visible_rows: 40,
            scroll_accum: 0.0,
            caret_bounds: None,
            char_width: px(8.0),
            text_origin: None,
        }
    }

    fn edited(&mut self, cx: &mut Context<Self>) {
        script::reveal(&mut self.doc, self.visible_rows);
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.backspace();
        self.edited(cx);
    }

    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.newline();
        self.edited(cx);
    }

    fn add_cursor_below(&mut self, _: &AddCursorBelow, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.add_cursor_below();
        self.edited(cx);
    }

    fn single_cursor(&mut self, _: &SingleCursor, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.single_cursor();
        cx.notify();
    }

    fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        self.doc
            .scroll_by(isize::try_from(self.visible_rows).unwrap_or(1));
        cx.notify();
    }

    fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        self.doc
            .scroll_by(-isize::try_from(self.visible_rows).unwrap_or(1));
        cx.notify();
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(ROW_HEIGHT));
        self.scroll_accum -= f32::from(delta.y) / ROW_HEIGHT;
        #[expect(clippy::cast_possible_truncation, reason = "a few rows per event")]
        let whole = self.scroll_accum.trunc() as isize;
        if whole != 0 {
            self.doc.scroll_by(whole);
            #[expect(clippy::cast_precision_loss, reason = "a few rows per event")]
            let used = whole as f32;
            self.scroll_accum -= used;
            cx.notify();
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let Some(origin) = self.text_origin else {
            return;
        };
        let row = (f32::from(event.position.y - origin.y) / ROW_HEIGHT).max(0.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped"
        )]
        let row = self.doc.scroll_row.saturating_add(row as usize);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped"
        )]
        let col_chars = (f32::from(event.position.x - origin.x) / f32::from(self.char_width))
            .round()
            .max(0.0) as usize;
        let line = row.min(self.doc.line_count().saturating_sub(1));
        let text = self.doc.line(line);
        let col = text
            .char_indices()
            .nth(col_chars)
            .map_or(text.len(), |(i, _)| i);
        let cursor = Cursor { line, col };
        if event.modifiers.alt {
            if !self.doc.cursors.iter().any(|c| c.line == line) {
                self.doc.cursors.push(cursor);
                self.doc.cursors.sort();
            }
        } else {
            self.doc.cursors = vec![cursor];
        }
        cx.notify();
    }

    /// The primary cursor's line, which is the text the input handler exposes.
    fn current_line(&self) -> &str {
        self.doc.line(self.doc.primary().line)
    }

    fn utf16_to_byte(&self, utf16: usize) -> usize {
        let line = self.current_line();
        let mut count = 0;
        for (byte, c) in line.char_indices() {
            if count >= utf16 {
                return byte;
            }
            count += c.len_utf16();
        }
        line.len()
    }

    fn byte_to_utf16(&self, byte: usize) -> usize {
        let line = self.current_line();
        line.get(..clamp_to_char(line, byte))
            .map_or(0, |s| s.chars().map(char::len_utf16).sum())
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The input handler exposes the primary cursor's line as the document: enough for the IME
/// to compose in place and for the candidate window to sit at the caret.
impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let start = self.utf16_to_byte(range.start);
        let end = self.utf16_to_byte(range.end);
        actual.replace(self.byte_to_utf16(start)..self.byte_to_utf16(end));
        self.current_line().get(start..end).map(str::to_owned)
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let at = self.byte_to_utf16(self.doc.primary().col);
        Some(UTF16Selection {
            range: at..at,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let marked = self.doc.marked.as_ref()?;
        let at = self.byte_to_utf16(self.doc.primary().col);
        Some(at..at + marked.chars().map(char::len_utf16).sum::<usize>())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if text.chars().any(char::is_control) {
            return;
        }
        match range {
            Some(range) if self.doc.marked.is_none() && range.start != range.end => {
                let start = self.utf16_to_byte(range.start);
                let end = self.utf16_to_byte(range.end);
                self.doc.replace_in_line(start..end, text);
            }
            _ => self.doc.insert(text),
        }
        self.edited(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.doc.marked = (!text.is_empty()).then(|| text.to_owned());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.caret_bounds
    }

    fn character_index_for_point(
        &mut self,
        _: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("editor")
            .key_context("Editor")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::add_cursor_below))
            .on_action(cx.listener(Self::single_cursor))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::page_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .size_full()
            .overflow_hidden()
            .bg(hsla(theme::SURFACE_EDITOR))
            .child(EditorElement {
                editor: cx.entity(),
            })
    }
}

struct EditorElement {
    editor: Entity<Editor>,
}

struct RowPaint {
    top: Pixels,
    background: Option<Hsla>,
    gutter: ShapedLine,
    line: ShapedLine,
}

pub struct Prepaint {
    rows: Vec<RowPaint>,
    carets: Vec<Bounds<Pixels>>,
    a11y: (Vec<a11y::TextRun>, Option<(usize, usize)>),
}

impl IntoElement for EditorElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        Some("editor-text".into())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepaint {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "screen rows"
        )]
        let visible = (f32::from(bounds.size.height) / ROW_HEIGHT).ceil().max(1.0) as usize;
        let font_size = px(FONT_SIZE);
        let regular = font(FONT_FAMILY);
        let italic = gpui::Font {
            style: FontStyle::Italic,
            ..regular.clone()
        };
        let text_x = bounds.left() + px(GUTTER);
        let editor = self.editor.read(cx);
        let doc = &editor.doc;
        let rows = doc.rows(doc.scroll_row, visible);
        let primary = doc.primary();
        let mut painted = Vec::with_capacity(rows.len());
        let mut carets = Vec::new();
        let mut ime_caret = None;
        let mut char_width = editor.char_width;
        for (i, row) in rows.iter().enumerate() {
            #[expect(clippy::cast_precision_loss, reason = "screen rows")]
            let top = bounds.top() + px(i as f32 * ROW_HEIGHT);
            let background = match theme::row_background(row.kind) {
                Some(bg) => Some(hsla(bg)),
                None if row.line == Some(primary.line) => Some(hsla(theme::SELECTION_LINE_BG)),
                None => None,
            };
            let gutter_text = match (row.kind, row.number) {
                (RowKind::Removed, _) => "-".to_owned(),
                (RowKind::Added, _) => "+".to_owned(),
                (RowKind::Normal, Some(n)) => n.to_string(),
                (RowKind::Normal, None) => String::new(),
            };
            let gutter_run = TextRun {
                len: gutter_text.len(),
                font: regular.clone(),
                color: hsla(theme::TEXT_FAINT),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let gutter =
                window
                    .text_system()
                    .shape_line(gutter_text.into(), font_size, &[gutter_run], None);
            let marked = match (&doc.marked, row.line) {
                (Some(text), Some(line)) if line == primary.line => {
                    Some((primary.col, text.as_str()))
                }
                _ => None,
            };
            let (text, runs) = row.display(marked);
            let text_runs: Vec<TextRun> = runs
                .iter()
                .map(|run| TextRun {
                    len: run.range.len(),
                    font: if run.italic {
                        italic.clone()
                    } else {
                        regular.clone()
                    },
                    color: hsla(run.color),
                    background_color: None,
                    underline: run.underline.then(|| UnderlineStyle {
                        color: Some(hsla(theme::TEXT)),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    strikethrough: None,
                })
                .collect();
            let line = window
                .text_system()
                .shape_line(text.into(), font_size, &text_runs, None);
            if let Some(line_index) = row.line {
                for cursor in doc.cursors.iter().filter(|c| c.line == line_index) {
                    let mut byte = cursor.col;
                    if *cursor == primary
                        && let Some((_, m)) = marked
                    {
                        byte = byte.saturating_add(m.len());
                    }
                    let caret = Bounds::new(
                        point(text_x + line.x_for_index(byte), top + px(2.0)),
                        size(px(2.0), px(ROW_HEIGHT - 4.0)),
                    );
                    if *cursor == primary {
                        ime_caret = Some(caret);
                    }
                    carets.push(caret);
                }
            }
            if line.len > 0 {
                #[expect(clippy::cast_precision_loss, reason = "short lines")]
                let chars = row.text.chars().count().max(1) as f32;
                if row.text.is_ascii() {
                    char_width = px(f32::from(line.width) / chars);
                }
            }
            painted.push(RowPaint {
                top,
                background,
                gutter,
                line,
            });
        }
        let a11y = a11y::text_runs(&rows, primary);
        self.editor.update(cx, |editor, _| {
            editor.visible_rows = visible;
            editor.caret_bounds = ime_caret;
            editor.char_width = char_width;
            editor.text_origin = Some(point(text_x, bounds.top()));
        });
        Prepaint {
            rows: painted,
            carets,
            a11y,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.editor.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );
        let line_height = px(ROW_HEIGHT);
        let text_top = (line_height - px(FONT_SIZE)) / 2.0 - px(2.0);
        for row in &prepaint.rows {
            if let Some(bg) = row.background {
                window.paint_quad(fill(
                    Bounds::new(
                        point(bounds.left(), row.top),
                        size(bounds.size.width, line_height),
                    ),
                    bg,
                ));
            }
            let gutter_x = bounds.left() + px(GUTTER - 16.0) - row.gutter.width;
            let _ = row.gutter.paint(
                point(gutter_x, row.top + text_top.max(px(0.0))),
                line_height,
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            );
            let _ = row.line.paint(
                point(bounds.left() + px(GUTTER), row.top + text_top.max(px(0.0))),
                line_height,
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            );
        }
        for caret in &prepaint.carets {
            window.paint_quad(fill(*caret, hsla(theme::ACCENT)));
        }
    }

    fn a11y_role(&self) -> Option<Role> {
        Some(Role::MultilineTextInput)
    }

    fn write_a11y_info(&self, node: &mut gpui::accesskit::Node) {
        node.set_label("QuakeC source");
    }

    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Prepaint,
        builder: &mut gpui::A11ySubtreeBuilder,
    ) {
        let (runs, caret) = &prepaint.a11y;
        let mut ids = Vec::with_capacity(runs.len());
        for (i, run) in runs.iter().enumerate() {
            let mut node = gpui::accesskit::Node::new(Role::TextRun);
            node.set_value(run.value.clone());
            node.set_character_lengths(run.character_lengths.clone());
            let id = builder.synthetic_node_id(i);
            builder.push_child(id, node);
            ids.push(id);
        }
        if let Some((row, character_index)) = caret
            && let Some(id) = ids.get(*row)
        {
            let at = gpui::accesskit::TextPosition {
                node: *id,
                character_index: *character_index,
            };
            builder
                .parent_node()
                .set_text_selection(gpui::accesskit::TextSelection {
                    anchor: at,
                    focus: at,
                });
        }
    }
}
