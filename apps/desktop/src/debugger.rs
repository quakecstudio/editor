// SPDX-License-Identifier: GPL-3.0-or-later

//! The Debugger tab and the console's command box (P3-16, P3-17): the call stack, the watches
//! and the breakpoints of the running game, the step buttons, and a line typed into the game's
//! console. What a click or a typed line asks for is returned to the shell, which runs it
//! through its command (mockup 02).

use std::path::PathBuf;

use eframe::egui::{self, RichText, Ui};
use qs_workbench::debug::Location;
use qs_workbench::{Preview, StepBy};

use crate::strings::Text;
use crate::theme::{Theme, Token};

/// The debugger's columns: the call stack, the variables and the watches.
const COLUMNS: usize = 3;

/// What the Debugger tab or the console's box asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DebugAction {
    /// Step the stopped game.
    Step(StepBy),
    /// Add a watch.
    Watch(String),
    /// Take away the watch at this place in the list.
    Unwatch(usize),
    /// Set or clear the breakpoint at a line of a file, relative to the project's root.
    Breakpoint {
        /// The file.
        file: PathBuf,
        /// The line, from 1.
        line: u32,
    },
    /// Show a line of a file, relative to the project's root, in the editor.
    Show {
        /// The file.
        file: PathBuf,
        /// The line, from 1.
        line: u32,
    },
    /// Run a line in the game's console.
    Console(String),
}

/// The text typed in the watch and console boxes, kept between frames.
#[derive(Debug, Default)]
pub(crate) struct DebugInput {
    watch: String,
    console: String,
    filter: String,
}

/// A frame's place, written as the call stack shows it: `weapons.qc:33`, or `engine`.
pub(crate) fn place(location: &Location) -> String {
    match (&location.file, location.line) {
        _ if location.engine => Text::EngineFrame.get().to_owned(),
        (Some(file), Some(line)) => {
            let name = file.file_name().unwrap_or(file.as_os_str());
            format!("{}:{line}", name.to_string_lossy())
        }
        (Some(file), None) => file.to_string_lossy().into_owned(),
        (None, _) => String::new(),
    }
}

/// The step buttons, then the call stack, the watches and the breakpoints side by side.
pub(crate) fn debugger_ui(
    ui: &mut Ui,
    theme: &Theme,
    preview: &Preview,
    input: &mut DebugInput,
) -> Option<DebugAction> {
    let debugger = preview.debugger();
    let stopped = debugger.stopped().is_some();
    let mut action = None;
    ui.horizontal(|ui| {
        for (text, kind) in [
            (Text::StepOver, StepBy::Over),
            (Text::StepIn, StepBy::In),
            (Text::StepOut, StepBy::Out),
        ] {
            if ui
                .add_enabled(stopped, egui::Button::new(text.get()))
                .clicked()
            {
                action = Some(DebugAction::Step(kind));
            }
        }
        if let Some(error) = debugger.error() {
            ui.label(RichText::new(error).color(theme.color(Token::StatusError)));
        }
    });
    let muted = theme.color(Token::TextMuted);
    ui.columns(COLUMNS, |columns| {
        let [stack, watches, breakpoints] = columns else {
            return;
        };
        stack.label(RichText::new(Text::CallStack.get()).strong());
        if !stopped {
            stack.add(egui::Label::new(RichText::new(Text::NotStopped.get()).color(muted)).wrap());
        }
        for (depth, frame) in debugger.stack().iter().enumerate() {
            let text = format!("{}  {}", frame.function, place(frame));
            let row = stack.selectable_label(depth == 0, RichText::new(text).monospace());
            if row.clicked()
                && let (Some(file), Some(line)) = (frame.file.clone(), frame.line)
            {
                action = Some(DebugAction::Show { file, line });
            }
        }

        watches.label(RichText::new(Text::Watches.get()).strong());
        egui::Grid::new(Text::Watches.get())
            .num_columns(COLUMNS)
            .show(watches, |ui| {
                for (index, watch) in debugger.watches().iter().enumerate() {
                    ui.label(RichText::new(&watch.expression).monospace());
                    let value = watch.value.as_deref().unwrap_or(Text::NotRead.get());
                    ui.label(
                        RichText::new(value)
                            .monospace()
                            .color(theme.color(Token::CodeConstant)),
                    );
                    if ui.small_button(Text::Remove.get()).clicked() {
                        action = Some(DebugAction::Unwatch(index));
                    }
                    ui.end_row();
                }
            });
        let typed = watches.add(
            egui::TextEdit::singleline(&mut input.watch)
                .hint_text(Text::AddWatch.get())
                .code_editor()
                .desired_width(f32::INFINITY),
        );
        if typed.lost_focus()
            && watches.input(|i| i.key_pressed(egui::Key::Enter))
            && !input.watch.trim().is_empty()
        {
            action = Some(DebugAction::Watch(std::mem::take(&mut input.watch)));
        }

        breakpoints.label(RichText::new(Text::Breakpoints.get()).strong());
        if debugger.breakpoints().is_empty() {
            breakpoints.add(
                egui::Label::new(RichText::new(Text::NoBreakpoints.get()).color(muted)).wrap(),
            );
        }
        for breakpoint in debugger.breakpoints() {
            breakpoints.horizontal(|ui| {
                let mut set = true;
                if ui.checkbox(&mut set, "").changed() {
                    action = Some(DebugAction::Breakpoint {
                        file: breakpoint.file.clone(),
                        line: breakpoint.line,
                    });
                }
                let name = format!(
                    "{}:{} {}",
                    breakpoint.file.to_string_lossy(),
                    breakpoint.line,
                    breakpoint.function.as_deref().unwrap_or_default()
                );
                if ui.link(RichText::new(name).monospace()).clicked() {
                    action = Some(DebugAction::Show {
                        file: breakpoint.file.clone(),
                        line: breakpoint.line,
                    });
                }
            });
        }
    });
    action
}

/// The console while a game runs, as mockup 01 draws it: the lines the filter shows, newest at
/// the bottom, an error's in the error colour; under them a `]` prompt whose line Enter runs in
/// the game, and the filter.
pub(crate) fn console(
    ui: &mut Ui,
    theme: &Theme,
    preview: &Preview,
    input: &mut DebugInput,
) -> Option<DebugAction> {
    let mut action = None;
    egui::Panel::bottom(egui::Id::new(Text::ConsoleCommand.get()))
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            ui.columns(2, |columns| {
                let [command, filter] = columns else {
                    return;
                };
                command.horizontal(|ui| {
                    ui.label(
                        RichText::new(Text::ConsolePrompt.get())
                            .monospace()
                            .color(theme.color(Token::Accent)),
                    );
                    let typed = ui.add(
                        egui::TextEdit::singleline(&mut input.console)
                            .hint_text(Text::ConsoleCommand.get())
                            .code_editor()
                            .frame(egui::Frame::NONE)
                            .desired_width(f32::INFINITY),
                    );
                    let entered =
                        typed.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if entered && !input.console.trim().is_empty() {
                        // The box keeps the keyboard so the next command can be typed at once.
                        typed.request_focus();
                        action = Some(DebugAction::Console(std::mem::take(&mut input.console)));
                    }
                });
                filter.add(
                    egui::TextEdit::singleline(&mut input.filter)
                        .hint_text(Text::FilterConsole.get())
                        .desired_width(f32::INFINITY),
                );
            });
        });
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            for line in preview.console().filter(|line| line.matches(&input.filter)) {
                let colour = if line.is_error() {
                    theme.color(Token::StatusError)
                } else {
                    theme.color(Token::TextCode)
                };
                ui.label(
                    RichText::new(line.text.trim_end())
                        .monospace()
                        .color(colour),
                );
            }
        });
    action
}
