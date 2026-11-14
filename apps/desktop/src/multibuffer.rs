// SPDX-License-Identifier: GPL-3.0-or-later

//! The multibuffer (P4-33, UI-17), as Zed's Multibuffers page describes it: project search
//! results, a name's references or the Problems list as excerpts of their files, one under
//! another, each editable in place. An edit in an excerpt changes its file's text through
//! `file.replace_range`; Replace all runs `file.replace_all` once per file, so each file is saved
//! and undone on its own; Open goes to the file with the caret where it was in the excerpt.
//!
//! An excerpt is the whole lines its place is on, with one line of context above and below;
//! the excerpts of one file that touch are joined into one.
//!
//! Every string comes from [`crate::strings`]; a test fails on a string literal here.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText, Ui};

use crate::shell::name_field;
use crate::strings::{Text, Truncation, search_count};
use crate::theme::{Theme, Token};

/// How many lines of context an excerpt shows above and below its place.
const CONTEXT: usize = 1;
/// The search and replace fields' width.
const FIELD_WIDTH: f32 = 280.0;

/// What the multibuffer shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Source {
    /// The project search's results.
    #[default]
    Search,
    /// A name's references.
    References,
    /// The Problems list.
    Problems,
}

/// One excerpt: a stretch of whole lines of a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Excerpt {
    /// The file.
    pub(crate) path: PathBuf,
    /// Its first byte in the file.
    pub(crate) start: usize,
    /// The byte after its last, its last line break excluded.
    pub(crate) end: usize,
    /// Its first line, from 1.
    pub(crate) line: usize,
    /// Its text, as edited.
    pub(crate) text: String,
    /// The first place it shows, from its start: where Open goes with no caret in it.
    pub(crate) place: usize,
    /// The caret's byte in the text, while it has been in it.
    caret: Option<usize>,
    /// Whether it may be edited: not when its file has bytes that are not UTF-8, which
    /// `file.read` gives as U+FFFD and an excerpt cannot put back as they were.
    pub(crate) editable: Editable,
}

/// Whether an excerpt may be edited.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Editable {
    /// Its text is its file's.
    #[default]
    Yes,
    /// Its file has bytes it cannot show as they are.
    No,
}

/// The project search's form.
#[derive(Clone, Debug, Default)]
pub(crate) struct SearchForm {
    /// The text to find.
    pub(crate) text: String,
    /// What Replace all puts.
    pub(crate) replacement: String,
    /// Whether case matters.
    pub(crate) case_sensitive: bool,
    /// Whether only whole words are found.
    pub(crate) whole_word: bool,
    /// Whether the text is a regular expression.
    pub(crate) regex: bool,
    /// Whether the search box takes the keyboard on the next frame.
    pub(crate) focus: bool,
}

/// The multibuffer's state.
#[derive(Clone, Debug, Default)]
pub(crate) struct Multibuffer {
    /// What it shows.
    pub(crate) source: Source,
    /// The excerpts, by file in the order found, then by place.
    pub(crate) excerpts: Vec<Excerpt>,
    /// How many places the excerpts show, and whether the search found more than these.
    pub(crate) found: (usize, Truncation),
    /// The search form.
    pub(crate) form: SearchForm,
    /// Why the last search or replace failed, if it did.
    pub(crate) error: Option<String>,
    /// Whether the editor takes the keyboard on the next frame, after Open.
    pub(crate) refocus: bool,
}

/// What a frame of the multibuffer asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MultiAction {
    /// Search the project with the form.
    Search,
    /// Replace every match of the form's search in each file, file by file.
    ReplaceAll,
    /// Excerpt `.0`'s text was edited: its old range of its file becomes `.1`.
    Edit(usize, String),
    /// Open a file at a byte.
    Open(PathBuf, usize),
}

impl Excerpt {
    /// The excerpt of `text`, the file at `path`, around the place at bytes `start..end`.
    #[must_use]
    pub(crate) fn around(path: &Path, text: &[u8], (start, end): (usize, usize)) -> Self {
        let start = start.min(text.len());
        let end = end.clamp(start, text.len());
        let mut first = line_start(text, start);
        for _ in 0..CONTEXT {
            if first == 0 {
                break;
            }
            first = line_start(text, first.saturating_sub(1));
        }
        let mut last = line_end(text, end);
        for _ in 0..CONTEXT {
            if last >= text.len() {
                break;
            }
            last = line_end(text, last.saturating_add(1));
        }
        let line = text
            .get(..first)
            .unwrap_or_default()
            .iter()
            .filter(|b| **b == b'\n')
            .count()
            .saturating_add(1);
        Self {
            path: path.to_path_buf(),
            start: first,
            end: last,
            line,
            text: String::from_utf8_lossy(text.get(first..last).unwrap_or_default()).into_owned(),
            place: start.saturating_sub(first),
            caret: None,
            editable: if std::str::from_utf8(text)
                .is_ok_and(|t| !t.contains(char::REPLACEMENT_CHARACTER))
            {
                Editable::Yes
            } else {
                Editable::No
            },
        }
    }

    /// Where Open goes: the caret's byte in the file while it has been in the excerpt, else
    /// its first place.
    #[must_use]
    pub(crate) fn open_at(&self) -> usize {
        self.start.saturating_add(self.caret.unwrap_or(self.place))
    }
}

/// The byte the line holding byte `at` starts at.
fn line_start(text: &[u8], at: usize) -> usize {
    text.get(..at)
        .and_then(|before| before.iter().rposition(|b| *b == b'\n'))
        .map_or(0, |i| i.saturating_add(1))
}

/// The byte the line holding byte `at` ends at, before its line break (and a `\r` before it).
fn line_end(text: &[u8], at: usize) -> usize {
    let end = text
        .get(at..)
        .and_then(|rest| rest.iter().position(|b| *b == b'\n'))
        .map_or(text.len(), |i| at.saturating_add(i));
    if end > at && text.get(end.saturating_sub(1)) == Some(&b'\r') {
        end.saturating_sub(1)
    } else {
        end
    }
}

impl Multibuffer {
    /// Shows `places`, each a file's byte range, read through `text`; the excerpts of one file
    /// that touch are joined. `truncation` says whether the places were cut short.
    pub(crate) fn show(
        &mut self,
        source: Source,
        places: &[(PathBuf, usize, usize)],
        truncation: Truncation,
        mut text: impl FnMut(&Path) -> Option<Vec<u8>>,
    ) {
        self.source = source;
        self.error = None;
        self.excerpts.clear();
        self.found = (places.len(), truncation);
        let mut files: Vec<&PathBuf> = Vec::new();
        for (path, _, _) in places {
            if !files.contains(&path) {
                files.push(path);
            }
        }
        for path in files {
            let Some(bytes) = text(path) else {
                continue;
            };
            let mut ranges: Vec<(usize, usize)> = places
                .iter()
                .filter(|(p, _, _)| p == path)
                .map(|(_, start, end)| (*start, *end))
                .collect();
            ranges.sort_unstable();
            let mut joined: Vec<Excerpt> = Vec::new();
            for range in ranges {
                let next = Excerpt::around(path, &bytes, range);
                match joined.last_mut() {
                    Some(last) if next.start <= last.end.saturating_add(1) => {
                        last.end = next.end.max(last.end);
                        last.text = String::from_utf8_lossy(
                            bytes.get(last.start..last.end).unwrap_or_default(),
                        )
                        .into_owned();
                    }
                    _ => joined.push(next),
                }
            }
            self.excerpts.extend(joined);
        }
    }

    /// Takes the edit of excerpt `index` to `text`, done in its file: the excerpt ends where
    /// its new text does, and the excerpts after it in the file move by the difference.
    pub(crate) fn edited(&mut self, index: usize, text: String) {
        let Some(excerpt) = self.excerpts.get_mut(index) else {
            return;
        };
        let old = excerpt.end.saturating_sub(excerpt.start);
        let new = text.len();
        excerpt.end = excerpt.start.saturating_add(new);
        excerpt.text = text;
        let path = excerpt.path.clone();
        let after = excerpt.start;
        for other in &mut self.excerpts {
            if other.path == path && other.start > after {
                other.start = other.start.saturating_add(new).saturating_sub(old);
                other.end = other.end.saturating_add(new).saturating_sub(old);
            }
        }
    }

    /// The files the excerpts are of, each once, in their order.
    #[must_use]
    pub(crate) fn files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = Vec::new();
        for excerpt in &self.excerpts {
            if !files.contains(&excerpt.path) {
                files.push(excerpt.path.clone());
            }
        }
        files
    }
}

/// The multibuffer: the search form when it shows search results, then each file's excerpts;
/// what was asked for. `root` is the project directory, which paths are shown under.
pub(crate) fn multibuffer_ui(
    ui: &mut Ui,
    theme: &Theme,
    buffer: &mut Multibuffer,
    root: Option<&Path>,
) -> Option<MultiAction> {
    let mut action = None;
    match buffer.source {
        Source::Search => action = form_ui(ui, &mut buffer.form),
        Source::References => heading(ui, Text::References),
        Source::Problems => heading(ui, Text::Problems),
    }
    if let Some(error) = &buffer.error {
        ui.label(RichText::new(error).color(theme.color(Token::StatusError)));
    }
    let (count, truncation) = buffer.found;
    ui.label(
        RichText::new(search_count(count, buffer.files().len(), truncation))
            .color(theme.color(Token::TextMuted)),
    );
    ui.separator();
    if buffer.excerpts.is_empty() {
        ui.label(RichText::new(Text::NoSearchMatch.get()).color(theme.color(Token::TextMuted)));
        return action;
    }
    egui::ScrollArea::vertical()
        .id_salt(Text::ProjectSearch.get())
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            let mut previous: Option<PathBuf> = None;
            for (index, excerpt) in buffer.excerpts.iter_mut().enumerate() {
                if previous.as_ref() != Some(&excerpt.path) {
                    ui.add_space(theme.space(1));
                    ui.label(RichText::new(shown(&excerpt.path, root)).strong());
                    previous = Some(excerpt.path.clone());
                }
                if let Some(asked) = excerpt_ui(ui, theme, index, excerpt) {
                    action = Some(asked);
                }
            }
        });
    action
}

/// A heading above the excerpts.
fn heading(ui: &mut Ui, text: Text) {
    ui.label(RichText::new(text.get()).strong());
}

/// `path` under `root`, with `/` between its folders, or the whole path when it is not under it.
fn shown(path: &Path, root: Option<&Path>) -> String {
    root.and_then(|root| path.strip_prefix(root).ok())
        .map_or_else(
            || path.display().to_string(),
            |below| {
                below
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            },
        )
}

/// The search form: the text and its options, Search, the replacement and Replace all.
fn form_ui(ui: &mut Ui, form: &mut SearchForm) -> Option<MultiAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        let typed = ui.add(
            egui::TextEdit::singleline(&mut form.text)
                .hint_text(Text::SearchProject.get())
                .desired_width(FIELD_WIDTH),
        );
        name_field(ui, &typed, Text::SearchProject);
        if form.focus {
            typed.request_focus();
            form.focus = false;
        }
        if typed.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            action = Some(MultiAction::Search);
        }
        ui.checkbox(&mut form.case_sensitive, Text::MatchCase.get());
        ui.checkbox(&mut form.whole_word, Text::WholeWord.get());
        ui.checkbox(&mut form.regex, Text::UseRegex.get());
        if ui.button(Text::Find.get()).clicked() {
            action = Some(MultiAction::Search);
        }
    });
    ui.horizontal(|ui| {
        let typed = ui.add(
            egui::TextEdit::singleline(&mut form.replacement)
                .hint_text(Text::Replace.get())
                .desired_width(FIELD_WIDTH),
        );
        name_field(ui, &typed, Text::Replace);
        if ui
            .add_enabled(
                !form.text.is_empty(),
                egui::Button::new(Text::ReplaceAll.get()),
            )
            .clicked()
        {
            action = Some(MultiAction::ReplaceAll);
        }
    });
    action
}

/// One excerpt: its first line's number and Open, then its text, editable.
fn excerpt_ui(
    ui: &mut Ui,
    theme: &Theme,
    index: usize,
    excerpt: &mut Excerpt,
) -> Option<MultiAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(excerpt.line.to_string())
                .monospace()
                .color(theme.color(Token::TextFaint)),
        );
        if ui.small_button(Text::OpenExcerpt.get()).clicked() {
            action = Some(MultiAction::Open(excerpt.path.clone(), excerpt.open_at()));
        }
    });
    let mut text = excerpt.text.clone();
    let output = egui::TextEdit::multiline(&mut text)
        .interactive(excerpt.editable == Editable::Yes)
        .id_salt((Text::ProjectSearch.get(), index))
        .code_editor()
        .desired_rows(1)
        .desired_width(f32::INFINITY)
        .show(ui);
    if let Some(range) = output.cursor_range {
        excerpt.caret = Some(byte_of(&text, range.primary.index.into()));
    }
    if output.response.changed() && text != excerpt.text {
        action = Some(MultiAction::Edit(index, text));
    }
    action
}

/// The byte the `chars`-th character of `text` starts at.
fn byte_of(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(byte, _)| byte)
}

#[cfg(test)]
mod tests;
