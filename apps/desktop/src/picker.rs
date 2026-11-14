// SPDX-License-Identifier: GPL-3.0-or-later

//! The pickers of the editor's navigation (P4-32, UI-16): go to symbol, the open file's
//! symbols as Zed's `outline::Toggle` lists them, and the tab switcher, the open files the one
//! used most recently first as Zed's `tab_switcher::Toggle` lists them. Each is a search over
//! its rows drawn as the file finder is (mockup 04): type to narrow, the arrows to move, Enter
//! to choose, Escape to close.
//!
//! Every string comes from [`crate::strings`]; a test fails on a string literal here.

use eframe::egui::{
    self, Align, Align2, FontFamily, FontId, Id, Key, Layout, Modifiers, Order, RichText, Sense,
    TextEdit, Ui, Vec2, accesskit,
};
use qs_commands::palette::find;

use crate::icons::Icon;
use crate::palette::highlighted;
use crate::shell::name_field;
use crate::strings::Text;
use crate::theme::{Theme, Token};

/// The picker's width, in points: the palette's.
const WIDTH: f32 = 640.0;
/// How far below the window's top it opens, in points: the palette's.
const TOP: f32 = 96.0;
/// The most rows shown before the list scrolls.
const ROWS: f32 = 10.0;
/// The room beside the query field, for its icon.
const QUERY_MARGIN: f32 = 40.0;

/// What a picker lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PickerKind {
    /// The open file's symbols.
    Symbols,
    /// The open files.
    Tabs,
}

impl PickerKind {
    /// The picker's name and its search box's hint.
    const fn text(self) -> Text {
        match self {
            Self::Symbols => Text::GoToSymbol,
            Self::Tabs => Text::TabSwitcher,
        }
    }
}

/// One row: what it is called, what is shown beside it, and the place it stands for, by index
/// into what the caller listed.
#[derive(Clone, Debug)]
pub(crate) struct PickerRow {
    /// Its name, searched first.
    pub(crate) name: String,
    /// What is shown beside it, searched too.
    pub(crate) detail: String,
}

/// A picker's state.
#[derive(Clone, Debug, Default)]
pub(crate) struct Picker {
    open: Option<PickerKind>,
    rows: Vec<PickerRow>,
    query: String,
    selected: usize,
    /// Whether the query field takes focus in the next frame.
    focus: bool,
}

impl Picker {
    /// Opens it over `rows`, the row at `selected` chosen until the query changes: the tab
    /// switcher starts on the file used before the one shown, as Ctrl+Tab goes back to it.
    pub(crate) fn open(&mut self, kind: PickerKind, rows: Vec<PickerRow>, selected: usize) {
        *self = Self {
            open: Some(kind),
            selected: selected.min(rows.len().saturating_sub(1)),
            rows,
            focus: true,
            ..Self::default()
        };
    }

    /// Closes it.
    pub(crate) fn close(&mut self) {
        *self = Self::default();
    }

    /// Draws it, if open, and returns the index of the row chosen, which closes it.
    pub(crate) fn show(&mut self, ui: &Ui, id: Id, theme: &Theme) -> Option<usize> {
        let kind = self.open?;
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
                            node.set_label(kind.text().get());
                        });
                        ui.set_width(WIDTH);
                        ui.set_max_width(WIDTH);
                        chosen = self.list_ui(ui, theme, kind);
                    });
            });
        if chosen.is_some() {
            self.close();
        }
        chosen
    }

    /// The search field and the matching rows.
    fn list_ui(&mut self, ui: &mut Ui, theme: &Theme, kind: PickerKind) -> Option<usize> {
        let indexed: Vec<(usize, &PickerRow)> = self.rows.iter().enumerate().collect();
        let found = find(
            &indexed,
            |(_, row)| (row.name.as_str(), row.detail.as_str()),
            &self.query,
        );
        let (up, down, enter, escape, tab) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
                // Ctrl+Tab again moves on, as in Zed's tab switcher.
                i.consume_key(Modifiers::CTRL, Key::Tab),
            )
        });
        if escape {
            self.close();
            return None;
        }
        let last = found.len().saturating_sub(1);
        self.selected = self.selected.min(last);
        if down || tab {
            self.selected = if self.selected >= last {
                0
            } else {
                self.selected.saturating_add(1)
            };
        }
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new(Icon::Search.char().to_string()));
            let field = TextEdit::singleline(&mut self.query)
                .hint_text(kind.text().get())
                .frame(egui::Frame::NONE)
                .desired_width(WIDTH - QUERY_MARGIN);
            let response = ui.add(field);
            name_field(ui, &response, kind.text());
            if self.focus {
                response.request_focus();
                self.focus = false;
            }
            changed = response.changed();
        });
        if changed {
            self.selected = 0;
        }
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
                    ui.label(RichText::new(Text::PaletteNoMatch.get()).color(muted));
                }
                for (index, item) in found.iter().enumerate() {
                    let selected = index == self.selected;
                    let row = item.item.1;
                    let response = ui
                        .horizontal(|ui| {
                            ui.label(highlighted(&row.name, &item.name_hits, text, accent, &font));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(highlighted(
                                    &row.detail,
                                    &item.text_hits,
                                    muted,
                                    accent,
                                    &font,
                                ));
                            });
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
                        if up || down || tab {
                            response.scroll_to_me(None);
                        }
                    }
                    if response.clicked() {
                        clicked = Some(index);
                    }
                }
            });
        let pick = clicked.or_else(|| enter.then_some(self.selected))?;
        found.get(pick).map(|item| item.item.0)
    }
}
