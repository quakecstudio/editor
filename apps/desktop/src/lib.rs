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

pub mod agent;
pub mod bench;
mod code_style;
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
#[cfg(target_os = "macos")]
pub mod menu;
mod multibuffer;
#[cfg(target_os = "macos")]
pub mod open_documents;
pub mod palette;
mod picker;
pub mod project;
pub mod shell;
pub mod strings;
mod terminal;
mod texture_browser;
mod texture_import;
pub mod theme;

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
    menu: Option<menu::MenuBar>,
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
    pub fn install_menu(&mut self) {
        match menu::MenuBar::install(self.shell.layout()) {
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

    /// Draws one frame, after running the menu items chosen since the last.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        self.follow_display(ui.ctx());
        #[cfg(target_os = "macos")]
        if let Some(menu) = &self.menu {
            for choice in menu.chosen() {
                match choice {
                    menu::MenuChoice::OpenFolder => self.shell.open_folder(),
                    menu::MenuChoice::NewProject(template) => self.shell.new_project(template),
                    menu::MenuChoice::GameFiles => self.shell.show_game_guide(),
                    menu::MenuChoice::ExportMod => self.shell.export_mod(),
                    menu::MenuChoice::Zoom(step) => self.shell.zoom(step),
                    menu::MenuChoice::Toggle(panel) => {
                        if let Err(error) = self.shell.toggle(panel) {
                            tracing::warn!(%error, "panel toggle failed");
                        }
                    }
                }
            }
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
        if let Some(menu) = &self.menu {
            menu.sync(self.shell.layout());
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
