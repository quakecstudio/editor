// SPDX-License-Identifier: GPL-3.0-or-later

//! The New Project dialog (L-28): which game the mod is for, asked once and in plain words,
//! then the folder. The game found on this machine decides which is chosen to begin with, as
//! FTEQW takes the rerelease for a folder holding `QuakeEX.kpf` and standard Quake otherwise
//! (`engine/common/fs.c`, `gamemode_info`).

use eframe::egui::{self, RichText, Ui};
use qs_templates::Template;
use qs_workbench::game_files::Games;

use crate::shell::heading;
use crate::strings::Text;
use crate::theme::{Theme, Token};

/// What a click in the dialog asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NewProjectAction {
    /// Ask for the folder and make a project of this template there.
    Make(Template),
    /// Close the dialog and make nothing.
    Close,
}

/// The dialog: hidden, or shown with the game chosen so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum NewProject {
    /// Not shown.
    #[default]
    Hidden,
    /// Shown, over the app, with this template chosen.
    Shown(Template),
}

impl NewProject {
    /// The template chosen to begin with for the `games` found: the rerelease's when the
    /// rerelease is all there is, id's otherwise.
    pub(crate) const fn preselected(games: Games) -> Template {
        match games {
            Games::Rerelease => Template::Rerelease,
            Games::None | Games::Original | Games::Both => Template::Id1,
        }
    }

    /// Shows the dialog, with the template for the `games` found chosen.
    pub(crate) const fn open(&mut self, games: Games) {
        *self = Self::Shown(Self::preselected(games));
    }

    /// Draws the dialog over the app while it shows, as a modal: the window behind takes no
    /// click and no key until it is answered. Cancel, Escape, a click outside it and Choose
    /// Folder hide it; Choose Folder returns the template to make a project of.
    pub(crate) fn show(&mut self, ctx: &egui::Context, theme: &Theme) -> Option<Template> {
        let Self::Shown(chosen) = self else {
            return None;
        };
        let modal = egui::Modal::new(egui::Id::new(Text::NewProjectHeading)).show(ctx, |ui| {
            heading(ui, theme, Text::NewProjectHeading);
            ui.add_space(theme.space(1));
            body(ui, theme, chosen)
        });
        let action = if modal.should_close() {
            Some(NewProjectAction::Close)
        } else {
            modal.inner
        };
        match action? {
            NewProjectAction::Make(template) => {
                *self = Self::Hidden;
                Some(template)
            }
            NewProjectAction::Close => {
                *self = Self::Hidden;
                None
            }
        }
    }
}

/// The dialog's body: the two games, `chosen` the one marked, and its buttons; a click on a
/// button returns what it asks for.
pub(crate) fn body(ui: &mut Ui, theme: &Theme, chosen: &mut Template) -> Option<NewProjectAction> {
    let muted = theme.color(Token::TextMuted);
    ui.label(Text::NewProjectWhich.get());
    ui.add_space(theme.space(1));
    for (template, name, from) in [
        (Template::Id1, Text::GameOriginal, Text::GameOriginalFrom),
        (
            Template::Rerelease,
            Text::GameRerelease,
            Text::GameRereleaseFrom,
        ),
    ] {
        ui.radio_value(chosen, template, name.get());
        ui.indent(name.get(), |ui| {
            ui.label(RichText::new(from.get()).small().color(muted));
        });
    }
    ui.add_space(theme.space(2));
    let mut action = None;
    ui.horizontal(|ui| {
        if ui.button(Text::PaletteCancel.get()).clicked() {
            action = Some(NewProjectAction::Close);
        }
        if ui.button(Text::NewProjectChoose.get()).clicked() {
            action = Some(NewProjectAction::Make(*chosen));
        }
    });
    action
}

#[cfg(test)]
mod tests;
