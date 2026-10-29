// SPDX-License-Identifier: GPL-3.0-or-later

//! QuakeC Studio, the desktop app (P2-17): the window, its toolbar and docked panels on egui
//! (eframe with the wgpu renderer, ADR-0003), themed from `docs/design/tokens.toml`
//! (ADR-0010), with the macOS menu bar through `muda`.
//!
//! The layout's state and the commands that change it are `qs-layout`'s; this crate draws them
//! and runs the commands when a control is used.
//!
//! # Layer
//!
//! apps. It depends on `qs-layout` and `qs-commands` (L5), `qs-log` and `qs-paths` (L0), and
//! `eframe`.

pub mod icons;
#[cfg(target_os = "macos")]
pub mod menu;
pub mod shell;
pub mod strings;
pub mod theme;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use eframe::egui;
use qs_layout::Layout;

use crate::shell::Shell;
use crate::theme::{Mode, Theme};

/// The app: the window's state and, on macOS, the menu bar.
pub struct App {
    shell: Shell,
    #[cfg(target_os = "macos")]
    menu: Option<menu::MenuBar>,
}

impl App {
    /// The app with the layout read from `layout_file`, or the default layout when it cannot
    /// be read; the theme is applied to `ctx`.
    ///
    /// # Errors
    ///
    /// [`AppError`] when the built-in theme or the window's commands are broken.
    pub fn new(ctx: &egui::Context, layout_file: Option<PathBuf>) -> Result<Self, AppError> {
        let layout = match &layout_file {
            Some(path) => Layout::read(path).unwrap_or_else(|error| {
                tracing::warn!(%error, "layout not read; using the default");
                Layout::default()
            }),
            None => Layout::default(),
        };
        let theme = Theme::load(Mode::Dark).map_err(AppError::Theme)?;
        theme.apply(ctx);
        let shell = Shell::new(layout, layout_file, theme).map_err(AppError::Commands)?;
        Ok(Self {
            #[cfg(target_os = "macos")]
            menu: None,
            shell,
        })
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

    /// Draws one frame, after running the menu items chosen since the last.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        #[cfg(target_os = "macos")]
        if let Some(menu) = &self.menu {
            for panel in menu.chosen() {
                if let Err(error) = self.shell.toggle(panel) {
                    tracing::warn!(%error, "panel toggle failed");
                }
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
