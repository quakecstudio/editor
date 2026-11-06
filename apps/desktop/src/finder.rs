// SPDX-License-Identifier: GPL-3.0-or-later

//! The file finder (Cmd+P), modelled on Zed's (`crates/file_finder`): a search over the
//! project's files, a match in a file's name before one in its folders, each row the name with
//! the matched characters in the accent and the path beside it; before anything is typed the
//! files opened most recently come first, the one being edited not selected; `name:line` opens
//! the file at that line; Enter opens the selected file. Drawn as the command palette is
//! (mockup 04).
//!
//! Every string comes from [`crate::strings`]; a test fails on a string literal here.

use std::path::{Path, PathBuf};

use eframe::egui::{
    self, Align, Align2, FontFamily, FontId, Id, Key, Layout, Modifiers, Order, RichText, Sense,
    TextEdit, Ui, Vec2, accesskit,
};
use qs_commands::palette::find;

use crate::icons::Icon;
use crate::palette::highlighted;
use crate::shell::name_field;
use crate::strings::{Text, file_count};
use crate::theme::{Theme, Token};

/// The finder's width, in points: the palette's.
const WIDTH: f32 = 640.0;
/// How far below the window's top it opens, in points: the palette's.
const TOP: f32 = 96.0;
/// The most rows shown before the list scrolls.
const ROWS: f32 = 10.0;

/// One file the finder can open: its path, its name and its path under the project.
#[derive(Clone, Debug)]
struct Entry {
    path: PathBuf,
    name: String,
    shown: String,
}

/// The finder's state.
#[derive(Clone, Debug, Default)]
pub(crate) struct FileFinder {
    open: bool,
    query: String,
    selected: usize,
    /// Whether the query field takes focus in the next frame.
    focus: bool,
    /// Whether the selection is to be placed afresh, as when it opens.
    focus_first: bool,
}

impl FileFinder {
    /// Whether it is showing.
    pub(crate) const fn is_open(&self) -> bool {
        self.open
    }

    /// Opens it with an empty query, its field taking the keyboard.
    pub(crate) fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.focus = true;
        self.focus_first = true;
    }

    /// Closes it.
    pub(crate) const fn close(&mut self) {
        self.open = false;
    }

    /// Draws it over `files`, the most recently opened first, named under `root`, and returns
    /// the file chosen and the line asked for, which closes it; `open` is the file being edited.
    pub(crate) fn show(
        &mut self,
        ui: &Ui,
        id: Id,
        theme: &Theme,
        (root, open): (Option<&Path>, Option<&Path>),
        files: &[PathBuf],
    ) -> Option<(PathBuf, Option<u32>)> {
        if !self.open {
            return None;
        }
        let entries: Vec<Entry> = files
            .iter()
            .map(|path| Entry {
                path: path.clone(),
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                shown: root
                    .and_then(|root| path.strip_prefix(root).ok())
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned(),
            })
            .collect();
        let mut chosen = None;
        egui::Area::new(id)
            .order(Order::Foreground)
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, TOP))
            .show(ui.ctx(), |ui| {
                egui::Frame::NONE
                    .fill(theme.color(Token::SurfaceRaised))
                    .stroke(egui::Stroke::new(1.0, theme.color(Token::BorderStrong)))
                    .corner_radius(theme.radius.card)
                    .inner_margin(theme.space(2))
                    .show(ui, |ui| {
                        ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
                            node.set_role(accesskit::Role::Dialog);
                            node.set_label(Text::FileFinder.get());
                        });
                        ui.set_width(WIDTH);
                        ui.set_max_width(WIDTH);
                        chosen = self.list_ui(ui, theme, &entries, open);
                    });
            });
        if chosen.is_some() {
            self.close();
        }
        chosen
    }

    /// The search field and the matching files.
    fn list_ui(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        entries: &[Entry],
        open: Option<&Path>,
    ) -> Option<(PathBuf, Option<u32>)> {
        let (query, line) = split_line(&self.query);
        let found = find(entries, |e| (e.name.as_str(), e.shown.as_str()), query);
        let (up, down, enter, escape) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if escape {
            self.close();
            return None;
        }
        let last = found.len().saturating_sub(1);
        if std::mem::take(&mut self.focus_first) {
            // As Zed's `skip_focus_for_active_in_search`: the file being edited, listed first,
            // is not the one Enter opens, unless a line in it is asked for.
            let first_is_open = found
                .first()
                .is_some_and(|m| Some(m.item.path.as_path()) == open);
            self.selected = usize::from(first_is_open && line.is_none() && found.len() > 1);
        }
        self.selected = self.selected.min(last);
        if down {
            self.selected = self.selected.saturating_add(1).min(last);
        }
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(Icon::Search.char().to_string()));
            let field = TextEdit::singleline(&mut self.query)
                .hint_text(Text::SearchFiles.get())
                .frame(egui::Frame::NONE)
                .desired_width(WIDTH - 200.0);
            let response = ui.add(field);
            name_field(ui, &response, Text::SearchFiles);
            if self.focus {
                response.request_focus();
                self.focus = false;
            }
            if response.changed() {
                // The matches for the new query are worked out next frame.
                self.focus_first = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(file_count(found.len(), entries.len()))
                        .color(theme.color(Token::TextMuted)),
                );
            });
        });
        ui.separator();
        let font = FontId::new(theme.type_scale.code.size, FontFamily::Monospace);
        let (text, muted, accent) = (
            theme.color(Token::Text),
            theme.color(Token::TextMuted),
            theme.color(Token::Accent),
        );
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .max_height(theme.size.row * ROWS)
            .show(ui, |ui| {
                if found.is_empty() {
                    ui.label(RichText::new(Text::NoFileMatch.get()).color(muted));
                }
                for (index, item) in found.iter().enumerate() {
                    let selected = index == self.selected;
                    let response = ui
                        .horizontal(|ui| {
                            let name =
                                highlighted(&item.item.name, &item.name_hits, text, accent, &font);
                            ui.label(name);
                            let path = highlighted(
                                &item.item.shown,
                                &item.text_hits,
                                muted,
                                accent,
                                &font,
                            );
                            ui.label(path);
                        })
                        .response
                        .interact(Sense::click());
                    if selected {
                        ui.painter().rect_stroke(
                            response.rect,
                            theme.radius.card,
                            egui::Stroke::new(1.0, theme.color(Token::Focus)),
                            egui::StrokeKind::Inside,
                        );
                        if up || down {
                            response.scroll_to_me(None);
                        }
                    }
                    if response.clicked() {
                        clicked = Some(index);
                    }
                }
            });
        let pick = clicked.or_else(|| enter.then_some(self.selected))?;
        Some((found.get(pick)?.item.path.clone(), line))
    }
}

/// The query and the line it asks for: `world.qc:12` is `world.qc` at line 12, as Zed's finder
/// reads a row after a colon; a colon not followed by a line number is part of the query.
pub(crate) fn split_line(query: &str) -> (&str, Option<u32>) {
    match query.rsplit_once(':') {
        Some((name, line)) => match line.trim().parse::<u32>() {
            Ok(line) if line > 0 => (name, Some(line)),
            Ok(_) | Err(_) => (query, None),
        },
        None => (query, None),
    }
}
