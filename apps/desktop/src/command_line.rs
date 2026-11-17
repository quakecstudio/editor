// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line docked above the status bar (UI-23, #971), as AutoCAD's command window:
//! a command is typed by its id, its name or a start of either and run with Enter, and each of
//! its parameters is asked for in turn. Tab completes what is typed, Up and Down bring back
//! the lines entered before, and Escape gives up the command being entered. What is typed is
//! read by [`qs_commands::line::CommandLine`]; this module draws it.
//!
//! Every string comes from [`crate::strings`]; a test fails on a string literal here.

use eframe::egui::{self, Id, Key, Modifiers, RichText, TextEdit, Ui, accesskit};
use qs_commands::Registry;
#[cfg(test)]
use qs_commands::line::Prompt;
use qs_commands::line::{CommandLine, Entered};
use serde_json::Value;

use crate::pick::Pick;
use crate::shell::{Area, FocusNow, name_area, name_field};
use crate::strings::{self, Text};
use crate::theme::{Theme, Token};

/// The most names listed when what is typed names several commands.
const LISTED: usize = 6;

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
        egui::Panel::bottom(Id::new(Area::CommandLine))
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
        let field = Id::new(Area::CommandLineField);
        let mut run = None;
        if ui.memory(|memory| memory.has_focus(field)) {
            run = self.keys(ui, registry, field);
        }
        let muted = theme.color(Token::TextMuted);
        ui.label(RichText::new(self.asked()).monospace().color(muted));
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
        name_field(ui, &response, Text::CommandLine);
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
        }
        match &self.note {
            Some(Note::Ran(id)) => {
                ui.label(RichText::new(strings::command_ran(id)).small().color(muted));
            }
            Some(Note::Problem(problem)) => {
                let error = theme.color(Token::StatusError);
                ui.label(RichText::new(problem).small().color(error));
            }
            Some(Note::Choices(names)) => {
                let listed: Vec<&str> = names.iter().take(LISTED).map(String::as_str).collect();
                let more = names.len().saturating_sub(listed.len());
                let text = strings::command_choices(&listed, more);
                ui.label(RichText::new(text).small().color(muted));
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

    /// The keys the line takes while its field has the keyboard.
    fn keys<C: 'static>(
        &mut self,
        ui: &Ui,
        registry: &Registry<C>,
        field: Id,
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
            self.complete(registry);
        }
        if up {
            self.recall(Recall::Older);
        }
        if down {
            self.recall(Recall::Newer);
        }
        if enter {
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
        self.recalled = Some(back);
    }
}

/// Which way Up and Down go through the entered lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Recall {
    /// To the line entered before the one shown.
    Older,
    /// To the line entered after it.
    Newer,
}
