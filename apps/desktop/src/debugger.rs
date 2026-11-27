// SPDX-License-Identifier: GPL-3.0-or-later

//! The Debugger tab and the console's command box (P3-16, P3-17): the call stack, the watches
//! and the breakpoints of the running game, the step buttons, and a line typed into the game's
//! console. What a click or a typed line asks for is returned to the shell, which runs it
//! through its command (mockup 02).

use std::borrow::Cow;
use std::path::PathBuf;

use eframe::egui::{self, RichText, Ui};
use qs_sim::ConsoleName;
use qs_workbench::WatchName;
use qs_workbench::debug::Location;
use qs_workbench::ids::WatchIndex;
use qs_workbench::{
    BREAKPOINT, BreakpointParams, CONSOLE, ConsoleParams, Preview, PreviewStatus, STEP_BY, StepBy,
    StepByParams, UNWATCH, UnwatchParams, WATCH, WatchParams,
};

use crate::features::{Commands, ShellRequest};
use crate::strings::{Text, key_detail};
use crate::suggest::{Offer, Offers, Suggest, Taken};
use crate::theme::{Theme, Token};

/// The debugger's columns: the call stack, the watches and the breakpoints.
const PANEL_COLUMNS: usize = 3;
/// The watches grid's columns: the expression, its value and its Remove button.
const WATCH_COLUMNS: usize = 3;

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

/// The text typed in the watch and console boxes, kept between frames, and what the console
/// box offers.
#[derive(Debug)]
pub(crate) struct DebugInput {
    watch: String,
    console: String,
    filter: String,
    /// What the console box offers.
    console_names: ConsoleNames,
    /// What a watch may read where the game stopped, each with its type, and the place they
    /// were read for.
    watch_names: (Option<Location>, Vec<Offer>),
}

impl Default for DebugInput {
    fn default() -> Self {
        let names = ConsoleName::all()
            .into_iter()
            .map(|name| match name {
                ConsoleName::Command(name) => Offer {
                    name,
                    detail: Text::ConsoleOfferCommand.get().to_owned(),
                },
                ConsoleName::Setting { name, default } => Offer {
                    name: name.to_owned(),
                    detail: key_detail(Text::ConsoleOfferSetting.get(), Some(default), None),
                },
            })
            .collect();
        Self {
            watch: String::new(),
            console: String::new(),
            filter: String::new(),
            console_names: ConsoleNames {
                names,
                levels: None,
            },
            watch_names: (None, Vec::new()),
        }
    }
}

/// What the console box offers.
#[derive(Debug)]
struct ConsoleNames {
    /// The console's commands and settings, each with what it is (CON-4.3).
    names: Vec<Offer>,
    /// The levels `map` and `changelevel` take, read once a game runs; `None` until then.
    levels: Option<Vec<String>>,
}

impl ConsoleNames {
    /// What the console box offers for `typed`: after `map` or `changelevel` and a space,
    /// that command with each level; otherwise every command and setting.
    fn for_line(&self, typed: &str) -> Cow<'_, [Offer]> {
        let typed = typed.trim_start();
        let command = ConsoleName::LEVEL_COMMANDS.iter().find(|command| {
            typed
                .strip_prefix(**command)
                .is_some_and(|rest| rest.starts_with(' '))
        });
        match command {
            Some(command) => Cow::Owned(
                self.levels
                    .iter()
                    .flatten()
                    .map(|level| Offer {
                        name: format!("{command} {level}"),
                        detail: Text::ConsoleOfferLevel.get().to_owned(),
                    })
                    .collect(),
            ),
            None => Cow::Borrowed(&self.names),
        }
    }
}

impl DebugInput {
    /// Whether the levels are to be read: a game runs and they have not been.
    pub(crate) const fn levels_unread(&self) -> bool {
        self.console_names.levels.is_none()
    }

    /// Keeps the levels `map` and `changelevel` offer, or with `None` forgets them, so the next
    /// game reads them again.
    pub(crate) fn set_levels(&mut self, levels: Option<Vec<String>>) {
        self.console_names.levels = levels;
    }

    /// Whether the names a watch may read are to be read again: the game stopped somewhere
    /// else, or no longer is.
    pub(crate) fn watch_names_stale(&self, stopped: Option<&Location>) -> bool {
        self.watch_names.0.as_ref() != stopped
    }

    /// Keeps `names`, what a watch may read where the game stopped at `stopped`.
    pub(crate) fn set_watch_names(&mut self, stopped: Option<Location>, names: Vec<WatchName>) {
        let offers = names
            .into_iter()
            .map(|name| Offer {
                name: name.expression,
                detail: name.detail,
            })
            .collect();
        self.watch_names = (stopped, offers);
    }

    /// The watch box, which offers what a watch may read where the game stopped (UI-24); the
    /// watch to add once one is taken or Enter is pressed on what is typed.
    fn watch_box(&mut self, ui: &mut Ui, theme: &Theme) -> Option<DebugAction> {
        let field = Suggest {
            name: Text::AddWatch.get(),
            offers: Offers::Given(&self.watch_names.1),
            width: ui.available_width(),
        };
        let (_, taken) = field.take(ui, theme, &mut self.watch);
        let entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let expression = match taken {
            Some(Taken::Offer(expression)) => expression,
            Some(Taken::Typed(expression)) if entered && !expression.is_empty() => expression,
            Some(Taken::Typed(_)) | None => return None,
        };
        self.watch.clear();
        Some(DebugAction::Watch(expression))
    }

    /// The console's `]` prompt and its box, which offers the commands and settings as they
    /// are typed (UI-24); the line to run once Enter is pressed on it.
    fn command_box(&mut self, ui: &mut Ui, theme: &Theme) -> Option<DebugAction> {
        ui.label(
            RichText::new(Text::ConsolePrompt.get())
                .monospace()
                .color(theme.color(Token::Accent)),
        );
        let names = &self.console_names;
        let offers = |typed: &str| names.for_line(typed);
        let field = Suggest {
            name: Text::ConsoleCommand.get(),
            offers: Offers::Made(&offers),
            width: ui.available_width(),
        };
        let (typed, taken) = field.take(ui, theme, &mut self.console);
        let entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
        match taken {
            // An offer taken is put in the box, a space after it for its words.
            Some(Taken::Offer(mut line)) => {
                line.push(' ');
                typed.request_focus();
                // The caret keeps its place by count, which leaves it in the
                // middle of a line put in whole: after it, typing goes on at the end.
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), typed.id) {
                    let end = egui::text::CCursor::new(line.chars().count());
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::one(end)));
                    state.store(ui.ctx(), typed.id);
                }
                self.console = line;
            }
            Some(Taken::Typed(line)) if entered && !line.is_empty() => {
                // The box keeps the keyboard so the next command can be typed at
                // once.
                self.console.clear();
                typed.request_focus();
                return Some(DebugAction::Console(line));
            }
            Some(Taken::Typed(_)) | None => {}
        }
        None
    }
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
    ui.columns(PANEL_COLUMNS, |columns| {
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
            .num_columns(WATCH_COLUMNS)
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
        if let Some(watch) = input.watch_box(watches, theme) {
            action = Some(watch);
        }

        breakpoints.label(RichText::new(Text::Breakpoints.get()).strong());
        if debugger.breakpoints().is_empty() {
            breakpoints.add(
                egui::Label::new(RichText::new(Text::NoBreakpoints.get()).color(muted)).wrap(),
            );
        }
        for breakpoint in debugger.breakpoints() {
            breakpoints.horizontal(|ui| {
                let mut name = format!("{}:{}", breakpoint.file.to_string_lossy(), breakpoint.line);
                if let Some(function) = &breakpoint.function {
                    name.push(' ');
                    name.push_str(function);
                }
                let mut set = true;
                let check = ui.checkbox(&mut set, "");
                let check = check.accessible_name(format!("{} {name}", Text::BreakpointAt.get()));
                if check.changed() {
                    action = Some(DebugAction::Breakpoint {
                        file: breakpoint.file.clone(),
                        line: breakpoint.line,
                    });
                }
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
    egui::Panel::bottom(egui::Id::unique(Text::ConsoleCommand.get()))
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            ui.columns(2, |columns| {
                let [command, filter] = columns else {
                    return;
                };
                if let Some(line) = command.horizontal(|ui| input.command_box(ui, theme)).inner {
                    action = Some(line);
                }
                let field = filter.add(
                    egui::TextEdit::singleline(&mut input.filter)
                        .hint_text(Text::FilterConsole.get())
                        .desired_width(f32::INFINITY),
                );
                field.accessible_name(Text::FilterConsole.get());
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

/// The Debugger tab's feature (P3-16): what is being typed in its boxes.
#[derive(Default)]
pub(crate) struct DebuggerFeature {
    /// The watch and console boxes.
    pub(crate) input: DebugInput,
}

impl DebugAction {
    /// Runs what the Debugger tab, the console's box or a click in the editor's gutter asked
    /// for, through its command, saying on the toolbar when it failed; or asks for a line to be
    /// shown in the editor.
    pub(crate) fn run(self, commands: &mut Commands<'_>) -> Option<ShellRequest> {
        let ran = match self {
            Self::Step(kind) => commands
                .run::<_, PreviewStatus>(STEP_BY, &StepByParams { kind })
                .is_some(),
            Self::Watch(expression) => commands
                .run::<_, ()>(WATCH, &WatchParams { expression })
                .is_some(),
            Self::Unwatch(index) => commands
                .run::<_, PreviewStatus>(
                    UNWATCH,
                    &UnwatchParams {
                        index: WatchIndex::new(index),
                    },
                )
                .is_some(),
            Self::Breakpoint { file, line } => commands
                .run::<_, ()>(BREAKPOINT, &BreakpointParams { file, line })
                .is_some(),
            Self::Console(command) => commands
                .run::<_, ()>(CONSOLE, &ConsoleParams { command })
                .is_some(),
            Self::Show { file, line } => return Some(ShellRequest::ShowProjectLine { file, line }),
        };
        if !ran {
            commands.edit_failed();
        }
        None
    }
}

#[cfg(test)]
mod tests;
