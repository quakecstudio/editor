// SPDX-License-Identifier: GPL-3.0-or-later

//! The first-run guide to the game's files (P3-21): shown when no game data is found or only
//! part of it, it says what the folder holds and lacks, how to get the files on this OS and
//! from which store, and offers a folder picker. The files are read in place, never copied.

use eframe::egui::{self, RichText, Ui};
use qs_workbench::game_files::{GameFiles, GameState};

use crate::strings::Text;
use crate::theme::{Theme, Token};

/// What a click in the guide asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GuideAction {
    /// Pick a folder and choose it as the game's.
    ChooseFolder,
    /// Close the guide.
    Close,
}

/// Whether the guide is on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Guide {
    /// Not shown.
    #[default]
    Hidden,
    /// Shown.
    Shown,
}

impl Guide {
    /// The guide for `files`: shown unless the registered game is found.
    pub(crate) const fn for_files(files: &GameFiles) -> Self {
        match files.state {
            GameState::Found => Self::Hidden,
            GameState::SharewareOnly | GameState::RereleaseOnly | GameState::Missing => Self::Shown,
        }
    }
}

/// The text that says what `state` is.
const fn summary(state: GameState) -> Text {
    match state {
        GameState::Found => Text::GameFound,
        GameState::SharewareOnly => Text::GameSharewareOnly,
        GameState::RereleaseOnly => Text::GameRereleaseOnly,
        GameState::Missing => Text::GameMissing,
    }
}

/// How to get the files on a Mac, which no store sells the original game for.
#[cfg(target_os = "macos")]
const fn how_to_get() -> Text {
    Text::GameHowMac
}

/// How to get the files on Windows and Linux.
#[cfg(not(target_os = "macos"))]
const fn how_to_get() -> Text {
    Text::GameHowDesktop
}

/// The guide's body for `files`; a click returns what it asks for.
pub(crate) fn guide(ui: &mut Ui, theme: &Theme, files: &GameFiles) -> Option<GuideAction> {
    let muted = theme.color(Token::TextMuted);
    let colour = if files.state.plays() {
        theme.color(Token::StatusOk)
    } else {
        theme.color(Token::StatusWarn)
    };
    ui.label(RichText::new(summary(files.state).get()).color(colour));
    if let Some(folder) = &files.folder {
        ui.label(
            RichText::new(folder.display().to_string())
                .small()
                .color(muted),
        );
    }
    if !files.missing.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(Text::GameLacks.get());
            ui.label(RichText::new(files.missing.join(", ")).monospace());
        });
    }
    if files.state != GameState::Found {
        ui.add_space(theme.space(2));
        ui.label(how_to_get().get());
        ui.label(RichText::new(Text::GameStores.get()).color(muted));
    }
    ui.label(RichText::new(Text::GameInPlace.get()).small().color(muted));
    ui.add_space(theme.space(2));
    let mut action = None;
    ui.horizontal(|ui| {
        if ui.button(Text::ChooseGameFolder.get()).clicked() {
            action = Some(GuideAction::ChooseFolder);
        }
        if ui.button(Text::NotNow.get()).clicked() {
            action = Some(GuideAction::Close);
        }
    });
    action
}

/// The guide as a window over the app while `shown` is [`Guide::Shown`].
pub(crate) fn window(
    ctx: &egui::Context,
    theme: &Theme,
    shown: Guide,
    files: &GameFiles,
) -> Option<GuideAction> {
    if shown == Guide::Hidden {
        return None;
    }
    let mut action = None;
    egui::Window::new(Text::GameGuide.get())
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            action = guide(ui, theme, files);
        });
    action
}

#[cfg(test)]
mod tests;
