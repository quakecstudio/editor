// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line docked above the status bar (UI-23, #971), as AutoCAD's command window:
//! a command is typed by its id, its name or a start of either, after a `/` or not, and run
//! with Enter, and each of its parameters is asked for in turn. While a command is typed the
//! commands that match are listed above the line with what each does (L-44), as a chat's slash
//! menu and Zed's palette list theirs: the first is marked, Down and Up move the mark, Tab
//! completes to the marked command, and Enter or a click runs it. Up from the top of the list
//! brings back the lines entered before, and Escape gives up the command being entered. What
//! is typed is read by [`qs_commands::line::CommandLine`]; this module draws it.
//!
//! Every string comes from [`crate::strings`]; a test fails on a string literal here.

use eframe::egui::{self, Id, Key, Modifiers, RichText, TextEdit, Ui, accesskit};
use qs_commands::Registry;
#[cfg(test)]
use qs_commands::line::Prompt;
use qs_commands::line::{CommandLine, Entered};
use serde_json::Value;

use crate::pick::Pick;
use crate::shell::{Area, FocusNow, name_area};
use crate::strings::{self, Text};
use crate::theme::{Theme, Token};

/// The most names listed when what is typed names several commands.
const LISTED: usize = 6;
/// The most commands the list above the line shows at once; the mark scrolls it.
const OFFERED: usize = 8;
/// The widest the list above the line is, in points.
const OFFERS_WIDTH: f32 = 720.0;

/// The share of the line's width the field takes; the note has the rest.
const FIELD_SHARE: f32 = 0.4;
/// The space at each end of the line, in points, as the status bar's.
const MARGIN: i8 = 8;

/// What the line says about the last thing entered.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Note {
    /// The command ran: its id.
    Ran(String),
    /// What it could not do, in the status colour for an error.
    Problem(String),
    /// The names what was typed may be completed to.
    Choices(Vec<String>),
}

/// The docked command line: what is typed, and what the line last said.
#[derive(Clone, Debug, Default)]
pub(crate) struct CommandLineView {
    line: CommandLine,
    typed: String,
    note: Option<Note>,
    /// The entered line shown by Up and Down, counted back from the newest, and what was
    /// typed before the first Up, which Down comes back to.
    recalled: Option<usize>,
    draft: String,
    /// Which of the commands offered is marked, from the first.
    marked: usize,
    /// Whether Up or Down put the mark where it is: Enter then runs the marked command
    /// whatever was typed, where a mark nobody moved runs only a command the text names.
    mark_moved: MarkMoved,
    /// Where the field's caret goes before the next frame: to the end, after the line put a
    /// text of its own in the field.
    caret: Caret,
    /// Where the list of offers was drawn in the last frame: a press on it takes the keyboard
    /// from the field a frame before the click is read, and the list must still be there.
    offers_at: Option<egui::Rect>,
}

impl CommandLineView {
    /// What a click in the map view can give the parameter the line asks for now.
    pub(crate) fn asking<C: 'static>(&self, registry: &Registry<C>) -> Option<Pick> {
        let prompt = self.line.prompt()?;
        let schema = registry.get(&prompt.command)?.params();
        Pick::asked(schema, &prompt.field.name)
    }

    /// Takes `value`, picked in the map view, as the parameter asked for, as typing it and
    /// Enter would; the command to run when that completes it.
    pub(crate) fn picked<C: 'static>(
        &mut self,
        registry: &Registry<C>,
        value: String,
    ) -> Option<(String, Value)> {
        self.typed = value;
        self.enter(registry)
    }

    /// Gives up the command being entered and what is typed, as Escape in the field does.
    pub(crate) fn give_up(&mut self) {
        self.line.cancel();
        self.typed.clear();
        self.note = None;
        self.recalled = None;
        self.unmark();
    }

    /// The command `id` the line made ran.
    pub(crate) fn ran(&mut self, id: String) {
        self.note = Some(Note::Ran(id));
    }

    /// The command the line made failed with `error`.
    pub(crate) fn failed(&mut self, error: String) {
        self.note = Some(Note::Problem(error));
    }

    /// What the line asks for now, what is typed, and whether it says anything about the last
    /// line entered.
    #[cfg(test)]
    pub(crate) fn state(&self) -> (Option<Prompt>, &str, bool) {
        (self.line.prompt(), &self.typed, self.note.is_some())
    }

    /// Draws the line above the status bar and returns the command to run, with its
    /// parameters, when one was completed with Enter.
    pub(crate) fn show<C: 'static>(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        registry: &Registry<C>,
        focus: FocusNow,
    ) -> Option<(String, Value)> {
        let mut run = None;
        egui::Panel::bottom(Id::unique(Area::CommandLine))
            .exact_size(theme.size.status_bar)
            .frame(
                egui::Frame::NONE
                    .fill(theme.color(Token::SurfaceChrome))
                    .inner_margin(egui::Margin::symmetric(MARGIN, 0)),
            )
            .show(ui, |ui| {
                name_area(ui, accesskit::Role::Group, Text::CommandLine);
                ui.horizontal_centered(|ui| {
                    run = self.row(ui, theme, registry, focus);
                });
            });
        run
    }

    /// The prompt, the field and the note, in one row.
    fn row<C: 'static>(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        registry: &Registry<C>,
        focus: FocusNow,
    ) -> Option<(String, Value)> {
        let field = Id::unique(Area::CommandLineField);
        let mut run = None;
        if ui.memory(|memory| memory.has_focus(field)) {
            let offers = self.offers(registry);
            run = self.keys(ui, registry, field, &offers);
        }
        let muted = theme.color(Token::TextMuted);
        // The prompt and the note are read, not typed in: the arrow, not the text cursor.
        let fixed = |text: RichText| egui::Label::new(text).selectable(false);
        ui.add(fixed(RichText::new(self.asked()).monospace().color(muted)));
        let hint = match self.asking(registry) {
            Some(_) => Text::CommandLinePick,
            None => Text::CommandLineHint,
        };
        let response = ui.add(
            TextEdit::singleline(&mut self.typed)
                .id(field)
                .font(egui::TextStyle::Monospace)
                .hint_text(hint.get())
                .frame(egui::Frame::NONE)
                .lock_focus(true)
                .desired_width(ui.available_width() * FIELD_SHARE),
        );
        let response = response.accessible_name(Text::CommandLine.get());
        if response.has_focus() {
            // Escape and Tab are the line's own keys: egui would drop or move the focus first.
            let own = egui::EventFilter {
                tab: true,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: true,
            };
            ui.memory_mut(|memory| memory.set_focus_lock_filter(field, own));
        }
        if focus == FocusNow::Take {
            response.request_focus();
        }
        if response.changed() {
            self.recalled = None;
            self.unmark();
        }
        if core::mem::take(&mut self.caret) == Caret::ToEnd
            && let Some(mut state) = TextEdit::load_state(ui.ctx(), field)
        {
            // The field's caret keeps its place by count, which a text put in whole leaves
            // in the middle: after it, typing goes on at the end.
            let end = egui::text::CCursor::new(self.typed.chars().count());
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(end)));
            state.store(ui.ctx(), field);
        }
        if run.is_none() {
            run = self.offers_list(ui, theme, registry, &response);
        }
        match &self.note {
            Some(Note::Ran(id)) => {
                ui.add(fixed(
                    RichText::new(strings::command_ran(id)).small().color(muted),
                ));
            }
            Some(Note::Problem(problem)) => {
                let error = theme.color(Token::StatusError);
                ui.add(fixed(RichText::new(problem).small().color(error)));
            }
            Some(Note::Choices(names)) => {
                let listed: Vec<&str> = names.iter().take(LISTED).map(String::as_str).collect();
                let more = names.len().saturating_sub(listed.len());
                let text = strings::command_choices(&listed, more);
                ui.add(fixed(RichText::new(text).small().color(muted)));
            }
            None => {}
        }
        run
    }

    /// What the line asks for: a command, or the command being entered with the parameter,
    /// its place among the command's, and whether it may be left empty.
    fn asked(&self) -> String {
        let Some(prompt) = self.line.prompt() else {
            return Text::CommandLinePrompt.get().to_owned();
        };
        let (place, count) = prompt.place;
        let field = &prompt.field;
        if field.required {
            format!("{} {} ({place}/{count}):", prompt.command, field.name)
        } else {
            let optional = Text::CommandLineOptional.get();
            format!(
                "{} {} ({place}/{count}, {optional}):",
                prompt.command, field.name
            )
        }
    }

    /// Whether Enter runs the marked command: when the text names it, or a key put the mark
    /// on it.
    fn takes_mark<C: 'static>(&self, registry: &Registry<C>) -> bool {
        self.mark_moved == MarkMoved::Yes || self.marked < registry.named(&self.typed).len()
    }

    /// Puts the mark back on the first command offered, as when the text changes.
    const fn unmark(&mut self) {
        self.marked = 0;
        self.mark_moved = MarkMoved::No;
    }

    /// The ids of the commands offered for what is typed, best first: none while a parameter
    /// is asked for, while an entered line is shown again, or with nothing typed.
    fn offers<C: 'static>(&self, registry: &Registry<C>) -> Vec<String> {
        if self.line.prompt().is_some() || self.recalled.is_some() || self.typed.trim().is_empty() {
            return Vec::new();
        }
        registry
            .listed(&self.typed)
            .into_iter()
            .map(|info| info.id().as_str().to_owned())
            .collect()
    }

    /// Draws the commands offered above the field, the marked one marked, each with what it
    /// does; a click on one runs it as Enter on it would. The list shows while the field has
    /// the keyboard, and while the pointer that took it is still on the list.
    fn offers_list<C: 'static>(
        &mut self,
        ui: &Ui,
        theme: &Theme,
        registry: &Registry<C>,
        field: &egui::Response,
    ) -> Option<(String, Value)> {
        // A press on the list takes the keyboard from the field a frame before its click is
        // read: the list stays for that press alone, not for a pointer resting on it.
        let pressing = ui.input(|i| i.pointer.primary_down() || i.pointer.primary_released());
        let on_list = pressing
            && self
                .offers_at
                .zip(ui.ctx().pointer_interact_pos())
                .is_some_and(|(rect, pointer)| rect.contains(pointer));
        let offers = self.offers(registry);
        if offers.is_empty() || !(field.has_focus() || on_list) {
            self.offers_at = None;
            return None;
        }
        self.marked = self.marked.min(offers.len().saturating_sub(1));
        // The mark is kept in sight: the list starts where it must to show it.
        let first = self.marked.saturating_add(1).saturating_sub(OFFERED);
        // The row Enter would run is the one marked; a list found by summaries alone has
        // none until a key puts the mark on a row.
        let takes_mark = self.takes_mark(registry);
        let muted = theme.color(Token::TextMuted);
        let mut clicked = None;
        let area =
            egui::Area::new(Id::unique(Area::CommandLineField).with(Text::CommandLineOffers))
                .order(egui::Order::Foreground)
                .pivot(egui::Align2::LEFT_BOTTOM)
                .fixed_pos(field.rect.left_top())
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        name_area(ui, accesskit::Role::ListBox, Text::CommandLineOffers);
                        ui.set_max_width(OFFERS_WIDTH);
                        for (index, id) in offers.iter().enumerate().skip(first).take(OFFERED) {
                            let summary = registry.get(id).map_or_default(|info| info.summary());
                            ui.horizontal(|ui| {
                                let name = RichText::new(id).monospace();
                                let marked = takes_mark && index == self.marked;
                                if ui.selectable_label(marked, name).clicked() {
                                    clicked = Some(id.clone());
                                }
                                let what = RichText::new(summary).small().color(muted);
                                ui.add(egui::Label::new(what).truncate().selectable(false));
                            });
                        }
                        let more = offers.len().saturating_sub(first.saturating_add(OFFERED));
                        if more > 0 {
                            let text = strings::more_commands(more);
                            ui.add(
                                egui::Label::new(RichText::new(text).small().color(muted))
                                    .selectable(false),
                            );
                        }
                    });
                });
        self.offers_at = Some(area.response.rect);
        let id = clicked?;
        self.typed = id;
        self.unmark();
        field.request_focus();
        self.enter(registry)
    }

    /// The keys the line takes while its field has the keyboard; `offers` are the commands
    /// listed above it.
    fn keys<C: 'static>(
        &mut self,
        ui: &Ui,
        registry: &Registry<C>,
        field: Id,
        offers: &[String],
    ) -> Option<(String, Value)> {
        let (enter, escape, back, tab, up, down) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
                // Before Tab, which egui takes whatever Shift is held.
                i.consume_key(Modifiers::SHIFT, Key::Tab),
                i.consume_key(Modifiers::NONE, Key::Tab),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
            )
        });
        if escape {
            if self.line.prompt().is_none() && self.typed.is_empty() {
                ui.memory_mut(|memory| memory.surrender_focus(field));
            }
            self.give_up();
        }
        // Tab completes what is typed; with nothing typed, and with Shift, it moves the
        // keyboard on or back as it does anywhere, so the line is no trap.
        if back {
            ui.memory_mut(|memory| memory.move_focus(egui::FocusDirection::Previous));
        } else if tab && self.typed.trim().is_empty() && self.line.prompt().is_none() {
            ui.memory_mut(|memory| memory.move_focus(egui::FocusDirection::Next));
        } else if tab {
            match offers.get(self.marked) {
                // The marked command, whole: the text now names it, so the mark goes back
                // to the first of what it lists.
                Some(marked) => {
                    marked.clone_into(&mut self.typed);
                    self.unmark();
                }
                None => self.complete(registry),
            }
            self.caret = Caret::ToEnd;
        }
        // Down goes into the list and Up back out of it; Up from its top brings back the
        // lines entered before, and Down then comes forward through them to what was typed.
        if up {
            match self.marked.checked_sub(1) {
                Some(above) if !offers.is_empty() => {
                    self.marked = above;
                    self.mark_moved = MarkMoved::Yes;
                }
                Some(_) | None => self.recall(Recall::Older),
            }
        }
        if down {
            if self.recalled.is_some() || offers.is_empty() {
                self.recall(Recall::Newer);
            } else {
                self.marked = self
                    .marked
                    .saturating_add(1)
                    .min(offers.len().saturating_sub(1));
                self.mark_moved = MarkMoved::Yes;
            }
        }
        if enter {
            // The marked command, when the text names it or the mark was put on it: a
            // command the list only found by its summary is not run for a slip of the keys.
            if self.takes_mark(registry)
                && let Some(marked) = offers.get(self.marked)
            {
                marked.clone_into(&mut self.typed);
            }
            self.unmark();
            return self.enter(registry);
        }
        None
    }

    /// Enters what is typed.
    fn enter<C: 'static>(&mut self, registry: &Registry<C>) -> Option<(String, Value)> {
        let typed = core::mem::take(&mut self.typed);
        self.recalled = None;
        self.note = None;
        match self.line.enter(registry, &typed) {
            Entered::Nothing | Entered::Ask(_) => None,
            Entered::Run { id, params } => Some((id, params)),
            Entered::Unknown(name) => {
                self.note = Some(Note::Problem(strings::command_unknown(&name)));
                self.typed = typed;
                None
            }
            Entered::Ambiguous(names) => {
                self.note = Some(Note::Choices(names));
                self.typed = typed;
                None
            }
            Entered::Refused(refused) => {
                self.note = Some(Note::Problem(refused.to_string()));
                self.typed = typed;
                None
            }
        }
    }

    /// Completes what is typed: to the one name it starts, else as far as every name it
    /// starts agrees, listing them.
    fn complete<C: 'static>(&mut self, registry: &Registry<C>) {
        let names = self.line.completions(registry, &self.typed);
        let Some(first) = names.first() else {
            return;
        };
        let shared = names.iter().fold(first.as_str(), |shared, name| {
            let same = shared
                .char_indices()
                .zip(name.chars())
                .find(|((_, a), b)| a != b)
                .map_or_else(|| shared.len().min(name.len()), |((at, _), _)| at);
            shared.get(..same).unwrap_or(shared)
        });
        // A name after the dot is typed, so what the ids share may not start with it.
        if shared.len() > self.typed.trim().len() && shared.starts_with(self.typed.trim()) {
            shared.clone_into(&mut self.typed);
        } else if let [only] = names.as_slice() {
            only.clone_into(&mut self.typed);
        }
        self.note = match names.len() {
            1 => None,
            _ => Some(Note::Choices(names)),
        };
    }

    /// Shows an entered line: the one before the line shown, or the one after it; after the
    /// newest, what was typed before the first Up. With no line to show, nothing changes.
    fn recall(&mut self, recall: Recall) {
        let history = self.line.history();
        let back = match (recall, self.recalled) {
            (Recall::Older, None) => 0,
            (Recall::Older, Some(back)) => back.saturating_add(1),
            (Recall::Newer, None) => return,
            (Recall::Newer, Some(0)) => {
                self.typed = core::mem::take(&mut self.draft);
                self.caret = Caret::ToEnd;
                self.recalled = None;
                return;
            }
            (Recall::Newer, Some(back)) => back.saturating_sub(1),
        };
        let back = back.min(history.len().saturating_sub(1));
        let Some(line) = history.iter().rev().nth(back) else {
            return;
        };
        if self.recalled.is_none() {
            self.draft = core::mem::take(&mut self.typed);
        }
        line.clone_into(&mut self.typed);
        self.caret = Caret::ToEnd;
        self.recalled = Some(back);
    }
}

/// Where the field's caret is put.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Caret {
    /// Where the field has it.
    #[default]
    AsIs,
    /// After the last character.
    ToEnd,
}

/// Whether Up or Down moved the mark in the list of offers since the text last changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum MarkMoved {
    /// The mark is where the list put it.
    #[default]
    No,
    /// A key put it where it is.
    Yes,
}

/// Which way Up and Down go through the entered lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Recall {
    /// To the line entered before the one shown.
    Older,
    /// To the line entered after it.
    Newer,
}
