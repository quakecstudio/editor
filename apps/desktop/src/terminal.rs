// SPDX-License-Identifier: GPL-3.0-or-later

//! The bottom panel's Terminal tab (P4-36, UI-20): a row of terminal tabs, each its title and
//! how its process stands, the shown tab's output, and for a shell an input box. The tabs are
//! the workbench's (`qs_workbench::terminal`); every change goes through a command.
//!
//! Every string comes from [`crate::strings`]; a test fails on a string literal here.

use eframe::egui::{self, RichText, Ui};
use qs_workbench::terminal::{Exit, Running, Terminal, Terminals};

use crate::icons::Icon;
use crate::shell::name_field;
use crate::strings::{Text, exit_status};
use crate::theme::{Theme, Token};

/// The most lines of a tab's output drawn; older ones are kept but not shown.
const SHOWN_LINES: usize = 2000;

/// The Terminal tab's own state: the tab shown and the shell line being typed.
#[derive(Debug, Default)]
pub(crate) struct TerminalView {
    /// The tab shown, from 0.
    pub(crate) selected: usize,
    line: String,
    /// How many tabs there were in the last frame, to show a new one when it opens.
    seen: usize,
}

impl TerminalView {
    /// Notes the tabs open now; true when one was opened since the last frame, which is then
    /// the one shown.
    pub(crate) fn opened(&mut self, count: usize) -> bool {
        let opened = count > self.seen;
        if opened {
            self.selected = count.saturating_sub(1);
        }
        self.seen = count;
        self.selected = self.selected.min(count.saturating_sub(1));
        opened
    }
}

/// What a click or a typed line in the Terminal tab asks for.
pub(crate) enum TerminalAction {
    /// Open a shell in a new tab.
    New,
    /// Write the line to the tab.
    Send(usize, String),
    /// Close the tab.
    Close(usize),
}

/// A tab's status: Running, or how its process ended.
pub(crate) fn status(tab: &Terminal) -> String {
    match tab.exit() {
        None => Text::TerminalRunning.get().to_owned(),
        Some(Exit::Code(code)) => exit_status(code),
        Some(Exit::Killed) => Text::TerminalKilled.get().to_owned(),
    }
}

/// The Terminal tab: what a click or a typed line asked for, if anything.
pub(crate) fn terminal_ui(
    ui: &mut Ui,
    theme: &Theme,
    terminals: &Terminals,
    view: &mut TerminalView,
) -> Option<TerminalAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        for (index, tab) in terminals.tabs().iter().enumerate() {
            let label = match tab.exit() {
                None => format!("{} {}", Icon::Play.char(), tab.title()),
                Some(exit) if exit.succeeded() => tab.title(),
                Some(_) => format!("{} {}", Icon::Error.char(), tab.title()),
            };
            let response = ui
                .selectable_label(view.selected == index, label)
                .on_hover_text(status(tab));
            if response.clicked() {
                view.selected = index;
            }
        }
        if ui.button(Text::NewTerminal.get()).clicked() {
            action = Some(TerminalAction::New);
        }
        if !terminals.tabs().is_empty() && ui.button(Text::CloseTerminal.get()).clicked() {
            action = Some(TerminalAction::Close(view.selected));
        }
    });
    let Some(tab) = terminals.tabs().get(view.selected) else {
        ui.add(
            egui::Label::new(
                RichText::new(Text::NoTerminals.get()).color(theme.color(Token::TextMuted)),
            )
            .wrap(),
        );
        return action;
    };
    let colour = match tab.exit() {
        None => theme.color(Token::TextMuted),
        Some(exit) if exit.succeeded() => theme.color(Token::StatusOk),
        Some(_) => theme.color(Token::StatusError),
    };
    ui.label(RichText::new(status(tab)).color(colour));
    let shell = matches!(tab.running(), Running::Shell) && tab.exit().is_none();
    if shell {
        egui::Panel::bottom(egui::Id::new(Text::TerminalCommand.get()))
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let typed = ui.add(
                    egui::TextEdit::singleline(&mut view.line)
                        .hint_text(Text::TerminalCommand.get())
                        .code_editor()
                        .desired_width(f32::INFINITY),
                );
                name_field(ui, &typed, Text::TerminalCommand);
                if typed.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    typed.request_focus();
                    let line = std::mem::take(&mut view.line);
                    action = Some(TerminalAction::Send(view.selected, line));
                }
            });
    }
    let text = tab.text();
    let breaks: Vec<usize> = text
        .char_indices()
        .filter_map(|(at, c)| (c == '\n').then_some(at))
        .collect();
    // The last SHOWN_LINES lines: from after the line break that many before the end.
    let start = breaks
        .len()
        .checked_sub(SHOWN_LINES.saturating_add(1))
        .and_then(|cut| breaks.get(cut))
        .map_or(0, |at| at.saturating_add(1));
    let shown = text.get(start..).unwrap_or_default().trim_end();
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(shown)
                        .monospace()
                        .color(theme.color(Token::TextCode)),
                )
                .wrap(),
            );
        });
    action
}
