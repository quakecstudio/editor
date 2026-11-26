// SPDX-License-Identifier: GPL-3.0-or-later

//! QuakeC Studio, the desktop app (P2-17): the window, its toolbar and docked panels on egui
//! (eframe with the wgpu renderer, ADR-0003), themed from `docs/design/tokens.toml`
//! (ADR-0010), with the macOS menu bar through `muda`.
//!
//! The layout's state and the commands that change it are `qs-layout`'s, and the open project
//! and file `qs-workbench`'s; this crate draws them and runs the commands when a control is
//! used.
//!
//! # Layer
//!
//! apps. It depends on `qs-layout`, `qs-workbench` and `qs-commands` (L5), `qs-editor-view`
//! and `qs-render` (L7), `qs-log` and `qs-paths` (L0), and `eframe`.

mod about;
pub mod agent;
pub mod bench;
mod code_style;
mod command_line;
mod debugger;
mod disk_watch;
pub mod display;
mod editor;
mod editors;
mod entities;
mod fallback;
mod features;
mod finder;
mod game_guide;
mod game_view;
mod git_panel;
mod help;
pub mod icons;
pub mod keymap;
pub mod launch;
mod map_browser;
pub mod map_editor;
mod map_feature;
mod map_fields;
mod map_groups;
mod map_issues;
mod map_leaks;
mod map_logic;
mod map_outline;
mod map_panes;
mod map_prefabs;
mod map_profiles;
mod map_review;
mod map_scene;
mod map_select;
mod map_shading;
mod map_tools;
mod map_uv;
pub mod map_view;
mod markdown;
pub mod menu;
mod multibuffer;
mod new_project;
#[cfg(target_os = "macos")]
pub mod open_documents;
pub mod palette;
mod pick;
mod picker;
pub mod project;
pub mod shell;
pub mod strings;
mod terminal;
mod texture_browser;
mod texture_import;
pub mod theme;
mod update;
mod welcome;

pub use about::ICON;

#[cfg(test)]
mod snapshots;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

use eframe::egui;
use qs_layout::Layout;
use qs_workbench::{CheckAfterTyping, Config};

use crate::display::Display;
use crate::shell::Shell;
use crate::theme::Theme;

/// Whether the app's look follows the system's display settings (P7-12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Follow {
    /// It reads them at the first frame and each time the window gains focus.
    System {
        /// Whether the window had focus at the last frame.
        focused: Focused,
    },
    /// It keeps the look it started with, so tests and tools draw the same each run.
    Fixed,
}

/// Whether the window has the keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focused {
    /// No frame has run yet.
    NotYet,
    /// It has.
    Yes,
    /// It has not.
    No,
}

/// The app: the window's state and, on macOS, the menu bar.
pub struct App {
    shell: Shell,
    #[cfg(target_os = "macos")]
    menu: Option<menu::native::MenuBar>,
    /// The Map menu's keys waiting a frame for the map view to have the keyboard (P7-17).
    map_keys: Vec<egui::KeyboardShortcut>,
    /// The display settings the look was made for.
    display: Display,
    follow: Follow,
}

impl App {
    /// The app with the layout read from `layout_file`, or the default layout when it cannot
    /// be read; the theme is applied to `ctx`.
    ///
    /// # Errors
    ///
    /// [`AppError`] when the built-in theme or the window's commands are broken.
    pub fn new(ctx: &egui::Context, layout_file: Option<PathBuf>) -> Result<Self, AppError> {
        let mut app = Self::with_analysis(
            ctx,
            layout_file,
            Config::default(),
            CheckAfterTyping::OffTheFrame,
        )?;
        app.follow = Follow::System {
            focused: Focused::NotYet,
        };
        Ok(app)
    }

    /// As [`App::new`], but each edit is checked on the frame that sends it, for tests and
    /// tools that drive frames by hand and read the check's results on the next one.
    ///
    /// # Errors
    ///
    /// As [`App::new`].
    pub fn checked_on_the_frame(
        ctx: &egui::Context,
        layout_file: Option<PathBuf>,
    ) -> Result<Self, AppError> {
        Self::with_analysis(
            ctx,
            layout_file,
            Config::default(),
            CheckAfterTyping::OnTheFrame,
        )
    }

    /// As [`App::new`], analysing projects as `config` says over their project files and
    /// checking each edit where `check` says.
    ///
    /// # Errors
    ///
    /// As [`App::new`].
    pub fn with_analysis(
        ctx: &egui::Context,
        layout_file: Option<PathBuf>,
        config: Config,
        check: CheckAfterTyping,
    ) -> Result<Self, AppError> {
        let layout = match &layout_file {
            Some(path) => Layout::read(path).unwrap_or_else(|error| {
                tracing::warn!(%error, "layout not read; using the default");
                Layout::default()
            }),
            None => Layout::default(),
        };
        let display = Display::default();
        let theme = Theme::load_with(display.mode, display.contrast).map_err(AppError::Theme)?;
        theme.apply(ctx);
        display.motion.apply(ctx);
        let shell =
            Shell::new(layout, layout_file, theme, config, check).map_err(AppError::Commands)?;
        Ok(Self {
            #[cfg(target_os = "macos")]
            menu: None,
            map_keys: Vec::new(),
            shell,
            display,
            follow: Follow::Fixed,
        })
    }

    /// Follows the system's display settings: its appearance, which egui reports, on every
    /// frame, and the accessibility settings at the first frame and when the window gains
    /// focus; the look is remade when they changed.
    fn follow_display(&mut self, ctx: &egui::Context) {
        self.follow_display_with(ctx, Display::read);
    }

    /// [`App::follow_display`], reading the settings with `read`.
    fn follow_display_with(
        &mut self,
        ctx: &egui::Context,
        read: impl FnOnce(&egui::Context) -> Display,
    ) {
        let Follow::System { focused } = self.follow else {
            return;
        };
        let now = if ctx.input(|input| input.focused) {
            Focused::Yes
        } else {
            Focused::No
        };
        self.follow = Follow::System { focused: now };
        let display = match (focused, now) {
            (Focused::NotYet, _) | (Focused::No, Focused::Yes) => read(ctx),
            (Focused::Yes | Focused::No, _) => Display {
                mode: Display::mode(ctx),
                ..self.display
            },
        };
        self.show_display(ctx, display);
    }

    /// Remakes the look for `display` when it differs from the look's.
    fn show_display(&mut self, ctx: &egui::Context, display: Display) {
        if display == self.display {
            return;
        }
        match Theme::load_with(display.mode, display.contrast) {
            Ok(theme) => {
                theme.apply(ctx);
                self.shell.set_theme(theme);
            }
            Err(error) => tracing::warn!(%error, "the theme for the display was not loaded"),
        }
        display.motion.apply(ctx);
        self.display = display;
    }

    /// Installs the macOS menu bar, on the main thread, as `muda` requires.
    #[cfg(target_os = "macos")]
    pub fn install_menu(&mut self, ctx: &egui::Context) {
        let ctx = ctx.clone();
        let wake = move || ctx.request_repaint();
        match menu::native::MenuBar::install(self.shell.layout(), self.shell.keymap(), wake) {
            Ok(menu) => self.menu = Some(menu),
            Err(error) => tracing::warn!(%error, "no menu bar"),
        }
    }

    /// The window's state.
    #[must_use]
    pub const fn shell(&self) -> &Shell {
        &self.shell
    }

    /// The window's state, to run its commands.
    pub const fn shell_mut(&mut self) -> &mut Shell {
        &mut self.shell
    }

    /// Carries out what a chosen menu item asks (P7-17). An item with a key gives the key to
    /// this frame's input, since the menu took it before the window saw it: the window then
    /// does what the key does where the keyboard is.
    fn carry_out(&mut self, ctx: &egui::Context, chosen: menu::Chosen) {
        use menu::{Chosen, MenuChoice, SystemItem};
        let press = |shortcut: egui::KeyboardShortcut| {
            let mut modifiers = shortcut.modifiers;
            // The platform's modifier as a key event of this platform carries it.
            match keymap::Platform::HERE {
                keymap::Platform::Mac => modifiers.mac_cmd |= modifiers.command,
                keymap::Platform::Other => modifiers.ctrl |= modifiers.command,
            }
            ctx.input_mut(|input| {
                input.events.push(egui::Event::Key {
                    key: shortcut.logical_key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                });
            });
        };
        match chosen {
            Chosen::Key(shortcut) => press(shortcut),
            Chosen::MapKey(shortcut) if self.shell.map_has_keyboard() => press(shortcut),
            Chosen::MapKey(shortcut) => {
                self.shell.give_map_keyboard();
                self.map_keys.push(shortcut);
                ctx.request_repaint();
            }
            Chosen::Run(id) => self.shell.run_chosen(id),
            Chosen::Choice(choice) => match choice {
                MenuChoice::OpenFolder => self.shell.open_folder(),
                MenuChoice::NewProject => self.shell.ask_new_project(),
                MenuChoice::RandomMap => self.shell.new_random_map(),
                MenuChoice::GameFiles => self.shell.show_game_guide(),
                MenuChoice::ExportMod => self.shell.export_mod(),
                MenuChoice::Zoom(step) => self.shell.zoom(step),
                MenuChoice::Toggle(panel) => {
                    if let Err(error) = self.shell.toggle(panel) {
                        tracing::warn!(%error, "panel toggle failed");
                    }
                }
                MenuChoice::FileFinder | MenuChoice::Palette => {
                    if let Some(key) = choice.window_key() {
                        press(key);
                    }
                }
            },
            // What egui makes of the clipboard's keys, which the text and the map view read.
            Chosen::Edit(SystemItem::Cut) => {
                ctx.input_mut(|input| input.events.push(egui::Event::Cut));
            }
            Chosen::Edit(SystemItem::Copy) => {
                ctx.input_mut(|input| input.events.push(egui::Event::Copy));
            }
            Chosen::Edit(SystemItem::Paste) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
            Chosen::Edit(SystemItem::SelectAll) => {
                press(egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND,
                    egui::Key::A,
                ));
            }
            // macOS carries these out itself; the window is not told of them.
            Chosen::Edit(
                SystemItem::Hide
                | SystemItem::HideOthers
                | SystemItem::ShowAll
                | SystemItem::Quit
                | SystemItem::Minimize
                | SystemItem::Maximize
                | SystemItem::Fullscreen,
            ) => {}
        }
    }

    /// Draws one frame, after running the menu items chosen since the last.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        #[cfg(target_os = "macos")]
        let chosen = self
            .menu
            .as_ref()
            .map_or_default(menu::native::MenuBar::chosen);
        #[cfg(not(target_os = "macos"))]
        let chosen = Vec::new();
        self.frame_after(ui, chosen);
    }

    /// Draws one frame, after carrying out the menu items `chosen`.
    pub fn frame_after(&mut self, ui: &mut egui::Ui, chosen: Vec<menu::Chosen>) {
        self.follow_display(ui.ctx());
        // The keys that waited for the map view: pressed now, wherever the keyboard is.
        for shortcut in std::mem::take(&mut self.map_keys) {
            self.carry_out(ui.ctx(), menu::Chosen::Key(shortcut));
        }
        for chosen in chosen {
            self.carry_out(ui.ctx(), chosen);
        }
        #[cfg(target_os = "macos")]
        {
            let opened = open_documents::take();
            if !opened.is_empty() {
                self.shell.open_paths(opened);
            }
        }
        self.shell.ui(ui);
        #[cfg(target_os = "macos")]
        if let Some(menu) = &mut self.menu {
            let map = if self.shell.map_shown() {
                menu::native::MapShown::Shown
            } else {
                menu::native::MapShown::Hidden
            };
            menu.sync(self.shell.layout(), self.shell.keymap(), map);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
        help::open_links(ui.ctx());
    }
}

/// Why the app could not start.
#[derive(Debug)]
pub enum AppError {
    /// The built-in theme is missing a value.
    Theme(theme::ThemeError),
    /// The window's commands could not be registered.
    Commands(qs_commands::RegisterError),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Theme(error) => error.fmt(f),
            Self::Commands(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for AppError {}
