// SPDX-License-Identifier: GPL-3.0-or-later

//! The code editor (P2-18, UI-4): the open file in `qs-editor-view`, coloured by the syntax and
//! the semantic model, with find and replace, hover, completion, go-to-definition, references,
//! rename and save.
//!
//! The editor owns its text and its undo history, as every code editor does. What it asks of
//! the project goes through the workbench commands (`qs-workbench`), so the palette and the
//! agent ask the same way: [`Editor::show`] returns [`Request`]s and the shell runs them. An
//! edit reaches the project as `file.update` once typing pauses for [`SETTLE`] seconds, and at
//! once before any question about the text, so every answer is about the text on screen.
//!
//! The keys are VS Code's: F12 goes to the definition, Shift+F12 lists the references, F2
//! renames, Ctrl+Space completes, Cmd+F finds and Cmd+S saves. Every string comes from
//! [`crate::strings`].

use std::path::{Path, PathBuf};

use eframe::egui::{
    Align2, Area, Button, Frame, Id, Key, KeyboardShortcut, Modifiers, Order, Pos2, RichText,
    TextEdit, Ui,
};
use qs_editor_view::find::{Case, Query};
use qs_editor_view::{EditorView, Position, Selection, ViewStyle};
use qs_workbench::{Candidate, OpenFile, Place};

use crate::code_style::CodeStyler;
use crate::project::Jump;
use crate::strings::{Text, match_count};
use crate::theme::{Theme, Token};

/// How long typing pauses, in seconds, before the project is checked again.
pub(crate) const SETTLE: f64 = 0.3;

/// How long the pointer rests on a name, in seconds, before its hover shows.
pub(crate) const HOVER_DELAY: f64 = 0.5;

/// How many lines show above the caret when a file is opened at a place.
const CONTEXT_LINES: usize = 5;

/// The find bar's query and replacement fields' widths, in points.
const FIND_WIDTH: f32 = 192.0;
const REPLACE_WIDTH: f32 = 144.0;

/// The most completion candidates the popup lists.
const MAX_CANDIDATES: usize = 12;

/// Save.
pub(crate) const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
/// Find.
pub(crate) const FIND: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
/// Complete.
pub(crate) const COMPLETE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::CTRL, Key::Space);
/// Go to the definition.
pub(crate) const DEFINITION: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F12);
/// List the references.
pub(crate) const REFERENCES: KeyboardShortcut = KeyboardShortcut::new(Modifiers::SHIFT, Key::F12);
/// Rename.
pub(crate) const RENAME: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F2);

/// What the editor asks the shell to do, by way of the workbench commands.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Request {
    /// `file.update` with the editor's text.
    Update(String),
    /// `file.save`.
    Save,
    /// `symbol.definition` at a byte offset, then `file.open` there.
    Definition(usize),
    /// `symbol.references` at a byte offset.
    References(usize),
    /// `symbol.hover` at a byte offset, shown at a point.
    Hover(usize, Pos2),
    /// `symbol.complete` at a byte offset.
    Complete(usize),
    /// `symbol.rename` at a byte offset to a name.
    Rename(usize, String),
    /// `file.open` at a place.
    Open(Jump),
}

/// A popup over the editor.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Popup {
    /// What a name is, at the pointer.
    Hover {
        /// Where it shows.
        at: Pos2,
        /// The declaration and its comment.
        text: String,
    },
    /// The names that may be written at the caret.
    Complete {
        /// Every candidate, as `symbol.complete` gave them.
        candidates: Vec<Candidate>,
        /// The candidate picked, among those the typed prefix matches.
        selected: usize,
    },
    /// Every declaration and use of a name.
    References(Vec<Place>),
    /// The new name for the name at a byte offset.
    Rename {
        /// The name's offset.
        offset: usize,
        /// The new name as typed.
        name: String,
    },
    /// A line of explanation, such as that a name has no definition.
    Note(Text),
}

/// The find bar's fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FindBar {
    /// The text to find.
    pub(crate) query: String,
    /// The text to replace it with.
    pub(crate) with: String,
    /// Whether case must match.
    pub(crate) case: Case,
}

impl FindBar {
    fn query(&self) -> Query {
        Query {
            text: self.query.clone(),
            case: self.case,
        }
    }
}

/// The open file in the editor.
pub(crate) struct Editor {
    pub(crate) path: PathBuf,
    generation: u64,
    version: u64,
    pub(crate) view: EditorView,
    /// The view's revision last sent with `file.update`.
    sent: u64,
    /// The view's revision when it was last seen, and when it changed to it, in egui seconds.
    seen: (u64, f64),
    styler: CodeStyler,
    pub(crate) find: Option<FindBar>,
    pub(crate) popup: Option<Popup>,
    /// The text position the pointer rests on, and since when.
    resting: Option<(Position, f64)>,
}

impl Editor {
    /// The view of `open`, its caret where `open` puts it.
    pub(crate) fn new(id: Id, open: &OpenFile, theme: &Theme) -> Self {
        let text = String::from_utf8_lossy(open.text());
        let view = EditorView::new(id, &text);
        let revision = view.state().revision();
        let mut editor = Self {
            path: open.path().to_path_buf(),
            generation: open.generation(),
            version: open.version(),
            view,
            sent: revision,
            seen: (revision, 0.0),
            styler: CodeStyler::new(open, theme),
            find: None,
            popup: None,
            resting: None,
        };
        editor.place(open);
        editor
    }

    /// Brings the editor up to date with `open`, the same file: the text a rename or its undo
    /// gave it, the caret another `file.open` put, and the colours of the last check.
    pub(crate) fn sync(&mut self, open: &OpenFile, theme: &Theme) {
        if open.version() != self.version {
            self.version = open.version();
            self.view
                .state_mut()
                .replace_text(&String::from_utf8_lossy(open.text()));
            self.sent = self.view.state().revision();
            self.seen.0 = self.sent;
        }
        if open.generation() != self.generation {
            self.place(open);
        }
        if !self.styler.is_current(open) {
            self.styler = CodeStyler::new(open, theme);
        }
    }

    /// Moves the caret to where `open` puts it and scrolls it into view, a few lines down.
    fn place(&mut self, open: &OpenFile) {
        let location = open.location();
        let line = usize::try_from(location.line().saturating_sub(1)).unwrap_or(usize::MAX);
        let chars = usize::try_from(location.column().saturating_sub(1)).unwrap_or(usize::MAX);
        let state = self.view.state_mut();
        let col = state.buffer().byte_column(line, chars);
        state.set_selections(&[Selection::caret(Position::new(line, col))]);
        self.view.scroll_to(line.saturating_sub(CONTEXT_LINES));
        self.generation = open.generation();
    }

    /// The editor's text, when it changed since it was last sent; marks it sent.
    pub(crate) fn unsent(&mut self) -> Option<String> {
        let revision = self.view.state().revision();
        (revision != self.sent).then(|| {
            self.sent = revision;
            self.view.state().buffer().text()
        })
    }

    /// The byte offset of the primary caret.
    fn caret_offset(&self) -> usize {
        let state = self.view.state();
        state.buffer().offset(state.primary().head)
    }

    /// The start of the name the primary caret is in or just after, and the part of it before
    /// the caret.
    fn word_before_caret(&self) -> (Position, String) {
        let state = self.view.state();
        let head = state.primary().head;
        let line = state.buffer().line(head.line);
        let before = line.get(..head.col).unwrap_or_default();
        let start = before
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
            .last()
            .map_or(head.col, |(at, _)| at);
        let word = before.get(start..).unwrap_or_default().to_owned();
        (Position::new(head.line, start), word)
    }

    /// The candidates the typed prefix matches, in the order given.
    fn matching<'c>(&self, candidates: &'c [Candidate]) -> Vec<&'c Candidate> {
        let (_, prefix) = self.word_before_caret();
        let prefix = prefix.to_lowercase();
        candidates
            .iter()
            .filter(|c| c.label.to_lowercase().starts_with(&prefix))
            .take(MAX_CANDIDATES)
            .collect()
    }

    /// Replaces the name before the caret with `label`.
    fn accept(&mut self, label: &str) {
        let (start, _) = self.word_before_caret();
        let state = self.view.state_mut();
        let head = state.primary().head;
        state.set_selections(&[Selection {
            anchor: start,
            head,
        }]);
        state.insert(label);
    }

    /// Draws the find bar, the editor and its popup for one frame, and returns what it asks of
    /// the project.
    pub(crate) fn show(&mut self, ui: &mut Ui, theme: &Theme, style: &ViewStyle) -> Vec<Request> {
        let mut requests = Vec::new();
        let now = ui.input(|i| i.time);
        let focused = ui.memory(|m| m.has_focus(self.view.id()));
        if focused || self.popup.is_some() {
            self.keys(ui, &mut requests);
        }
        if let Some(find) = self.find.as_mut() {
            find_bar(ui, theme, find, self.view.state_mut());
        }
        self.view.show(ui, style, &self.styler);
        self.after_edit(ui, now, &mut requests);
        self.rest(ui, now, &mut requests);
        self.popup_ui(ui, theme, &mut requests);
        requests
    }

    /// The editor's own keys, taken before the view sees them.
    fn keys(&mut self, ui: &mut Ui, requests: &mut Vec<Request>) {
        let pressed = |ui: &mut Ui, shortcut: &KeyboardShortcut| {
            ui.input_mut(|i| i.consume_shortcut(shortcut))
        };
        if let Some(Popup::Complete {
            candidates,
            selected,
        }) = &self.popup
        {
            let shown = self.matching(candidates).len();
            let picked = self
                .matching(candidates)
                .get(*selected)
                .map(|c| c.label.clone());
            let mut selected = *selected;
            if pressed(ui, &KeyboardShortcut::new(Modifiers::NONE, Key::ArrowDown)) {
                selected = selected.saturating_add(1).min(shown.saturating_sub(1));
            }
            if pressed(ui, &KeyboardShortcut::new(Modifiers::NONE, Key::ArrowUp)) {
                selected = selected.saturating_sub(1);
            }
            let accept = pressed(ui, &KeyboardShortcut::new(Modifiers::NONE, Key::Enter))
                || pressed(ui, &KeyboardShortcut::new(Modifiers::NONE, Key::Tab));
            if let Some(Popup::Complete { selected: s, .. }) = &mut self.popup {
                *s = selected;
            }
            if accept {
                if let Some(label) = picked {
                    self.accept(&label);
                }
                self.popup = None;
            }
        }
        let escape = KeyboardShortcut::new(Modifiers::NONE, Key::Escape);
        if self.popup.is_some() && pressed(ui, &escape) {
            self.popup = None;
        } else if self.find.is_some() && pressed(ui, &escape) {
            self.find = None;
        }
        if pressed(ui, &SAVE) {
            requests.push(Request::Save);
        }
        if pressed(ui, &FIND) {
            self.find = Some(self.find.take().unwrap_or_default());
        }
        let offset = self.caret_offset();
        if pressed(ui, &COMPLETE) {
            requests.push(Request::Complete(offset));
        }
        if pressed(ui, &REFERENCES) {
            requests.push(Request::References(offset));
        }
        if pressed(ui, &DEFINITION) {
            requests.push(Request::Definition(offset));
        }
        if pressed(ui, &RENAME) {
            let (_, word) = self.word_before_caret();
            self.popup = Some(Popup::Rename { offset, name: word });
        }
    }

    /// After the view took this frame's input: closes a completion the caret left, and sends
    /// the text once typing has paused.
    fn after_edit(&mut self, ui: &Ui, now: f64, requests: &mut Vec<Request>) {
        let revision = self.view.state().revision();
        if revision != self.seen.0 {
            self.seen = (revision, now);
            if matches!(self.popup, Some(Popup::Hover { .. } | Popup::Note(_))) {
                self.popup = None;
            }
        }
        if let Some(Popup::Complete { candidates, .. }) = &self.popup
            && self.matching(candidates).is_empty()
        {
            self.popup = None;
        }
        if revision != self.sent {
            let waited = now - self.seen.1;
            // First, so a save or a question asked this frame is about this text.
            if waited >= SETTLE
                && let Some(text) = self.unsent()
            {
                requests.insert(0, Request::Update(text));
            } else {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs_f64(SETTLE));
            }
        }
    }

    /// Asks for the hover of the name the pointer has rested on for [`HOVER_DELAY`].
    fn rest(&mut self, ui: &Ui, now: f64, requests: &mut Vec<Request>) {
        let hovered = self.view.hovered();
        let pointer = ui.input(|i| i.pointer.hover_pos());
        match (hovered, self.resting) {
            (None, _) => self.resting = None,
            (Some(at), Some((was, since))) if at == was => {
                // A hover never covers a popup asked for or shown already.
                if now - since >= HOVER_DELAY && self.popup.is_none() && requests.is_empty() {
                    if let Some(pointer) = pointer {
                        let offset = self.view.state().buffer().offset(at);
                        requests.push(Request::Hover(offset, pointer));
                    }
                    // Asked once per rest: the timer starts again only when the pointer moves.
                    self.resting = Some((at, f64::INFINITY));
                } else if since.is_finite() {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_secs_f64(HOVER_DELAY));
                }
            }
            (Some(at), _) => {
                self.resting = Some((at, now));
                if matches!(self.popup, Some(Popup::Hover { .. })) {
                    self.popup = None;
                }
            }
        }
    }

    /// Draws the popup, if any, below the caret or at the pointer.
    fn popup_ui(&mut self, ui: &Ui, theme: &Theme, requests: &mut Vec<Request>) {
        let Some(popup) = self.popup.clone() else {
            return;
        };
        let below_caret = self
            .view
            .caret_rect()
            .map_or_else(|| ui.max_rect().left_top(), |rect| rect.left_bottom());
        let at = match &popup {
            Popup::Hover { at, .. } => Pos2::new(at.x, at.y + theme.space(3)),
            Popup::Complete { .. }
            | Popup::References(_)
            | Popup::Rename { .. }
            | Popup::Note(_) => below_caret,
        };
        let mut close = false;
        Area::new(Id::new((self.view.id(), Text::Editor)))
            .order(Order::Foreground)
            .fixed_pos(at)
            .pivot(Align2::LEFT_TOP)
            .show(ui.ctx(), |ui| {
                Frame::popup(ui.style())
                    .fill(theme.color(Token::SurfaceRaised))
                    .show(ui, |ui| match popup {
                        Popup::Hover { text, .. } => {
                            ui.label(RichText::new(text).monospace());
                        }
                        Popup::Note(text) => {
                            ui.label(
                                RichText::new(text.get()).color(theme.color(Token::TextMuted)),
                            );
                        }
                        Popup::Complete {
                            candidates,
                            selected,
                        } => {
                            let mut accepted = None;
                            for (index, candidate) in
                                self.matching(&candidates).into_iter().enumerate()
                            {
                                let row = Button::selectable(
                                    index == selected,
                                    RichText::new(candidate.label.as_str()).monospace(),
                                )
                                .right_text(
                                    RichText::new(candidate.detail.as_str())
                                        .small()
                                        .color(theme.color(Token::TextMuted)),
                                );
                                if ui.add(row).clicked() {
                                    accepted = Some(candidate.label.clone());
                                }
                            }
                            if let Some(label) = accepted {
                                self.accept(&label);
                                close = true;
                            }
                        }
                        Popup::References(places) => {
                            ui.label(RichText::new(Text::References.get()).strong());
                            for place in places {
                                let label = place_text(&place, &self.path);
                                if ui.add(Button::new(label).frame(false)).clicked() {
                                    requests.push(Request::Open(Jump {
                                        path: place.path.clone(),
                                        offset: place.start,
                                    }));
                                    close = true;
                                }
                            }
                        }
                        Popup::Rename { offset, mut name } => {
                            ui.label(Text::RenameTo.get());
                            let field = ui.add(TextEdit::singleline(&mut name).code_editor());
                            field.request_focus();
                            let enter = ui.input(|i| i.key_pressed(Key::Enter));
                            if enter && !name.is_empty() {
                                requests.push(Request::Rename(offset, name.clone()));
                                close = true;
                            } else if let Some(Popup::Rename { name: kept, .. }) = &mut self.popup {
                                *kept = name;
                            }
                        }
                    });
            });
        if close {
            self.popup = None;
            ui.memory_mut(|m| m.request_focus(self.view.id()));
        }
    }
}

/// A reference's row: its file's name, unless it is `current`, and its line and column.
fn place_text(place: &Place, current: &Path) -> String {
    let line = place.location.line();
    let column = place.location.column();
    if place.path == current {
        format!(":{line}:{column}")
    } else {
        let name = place.path.file_name().map_or_else(
            || place.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        format!("{name}:{line}:{column}")
    }
}

/// The find bar: the query and the replacement, the case switch, and the buttons that find
/// and replace in `state`.
fn find_bar(ui: &mut Ui, theme: &Theme, find: &mut FindBar, state: &mut qs_editor_view::EditState) {
    let query = find.query();
    let count = query.matches(state.buffer()).len();
    Frame::NONE
        .fill(theme.color(Token::SurfaceTabs))
        .inner_margin(theme.space(1))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let field = ui.add(
                    TextEdit::singleline(&mut find.query)
                        .hint_text(Text::Find.get())
                        .desired_width(FIND_WIDTH),
                );
                if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    state.find_next(&query);
                    field.request_focus();
                }
                let mut matching = find.case == Case::Match;
                if ui
                    .toggle_value(&mut matching, Text::MatchCase.get())
                    .changed()
                {
                    find.case = if matching { Case::Match } else { Case::Ignore };
                }
                ui.label(
                    RichText::new(match_count(count))
                        .small()
                        .color(theme.color(Token::TextMuted)),
                );
                if ui.button(Text::FindPrevious.get()).clicked() {
                    state.find_previous(&query);
                }
                if ui.button(Text::FindNext.get()).clicked() {
                    state.find_next(&query);
                }
                ui.add(
                    TextEdit::singleline(&mut find.with)
                        .hint_text(Text::Replace.get())
                        .desired_width(REPLACE_WIDTH),
                );
                if ui.button(Text::Replace.get()).clicked() {
                    state.replace_next(&query, &find.with);
                }
                if ui.button(Text::ReplaceAll.get()).clicked() {
                    state.replace_all(&query, &find.with);
                }
            });
        });
}

#[cfg(test)]
mod tests;
