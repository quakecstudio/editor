// SPDX-License-Identifier: GPL-3.0-or-later

//! The Welcome page: what the centre shows while no project is open, as Zed's welcome tab and
//! TrenchBroom's welcome window do. It gives the three steps from a first launch to a running
//! mod: Quake's files, a project, Play.

use std::path::PathBuf;

use eframe::egui::{self, Button, RichText, Ui};
use qs_templates::Template;
use qs_workbench::game_files::GameFiles;

use crate::features::LeftAction;
use crate::game_guide;
use crate::project;
use crate::strings::Text;
use crate::theme::{Theme, Token};

/// The widest the page's column is drawn, in points.
const PAGE_WIDTH: f32 = 520.0;
/// The side the app's icon is drawn at, in points.
const ICON_SIDE: f32 = 56.0;
/// The step of the spacing scale between the page's parts.
const GAP: usize = 4;
/// The share of the room left beside the column that goes to each side.
const HALF: f32 = 0.5;

/// What a click on the page asks for.
pub(crate) enum WelcomeAction {
    /// Open the guide to the game's files.
    GameFiles,
    /// Open or make a project, as the Project panel does.
    Project(LeftAction),
    /// Open the command palette.
    Palette,
    /// Open the user guide on the website.
    UserGuide,
}

/// What the page draws from.
pub(crate) struct Page<'a> {
    /// The app's icon, when it decoded.
    pub(crate) icon: Option<egui::TextureId>,
    /// What the game folder, or the first install found, holds; `None` before it was looked
    /// for.
    pub(crate) files: Option<&'a GameFiles>,
    /// The projects opened most recently, the latest first.
    pub(crate) recent: &'a [PathBuf],
    /// Play's key, as the system writes it; `None` when it has none.
    pub(crate) play_key: Option<String>,
    /// The command palette's key, as the system writes it.
    pub(crate) palette_key: String,
}

/// A step's number and title.
fn step(ui: &mut Ui, theme: &Theme, number: &str, title: Text) {
    ui.add_space(theme.space(GAP));
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(number)
                .strong()
                .color(theme.color(Token::Accent)),
        );
        ui.label(RichText::new(title.get()).strong());
    });
}

/// The first step: what was found of the game, and the way to its guide.
fn game_step(ui: &mut Ui, theme: &Theme, files: Option<&GameFiles>) -> Option<WelcomeAction> {
    step(ui, theme, "1", Text::WelcomeGame);
    let muted = theme.color(Token::TextMuted);
    if let Some(files) = files {
        let colour = if files.state.plays() {
            theme.color(Token::StatusOk)
        } else {
            theme.color(Token::StatusWarn)
        };
        ui.label(RichText::new(game_guide::summary(files.state).get()).color(colour));
        if let Some(folder) = &files.folder {
            ui.label(
                RichText::new(folder.display().to_string())
                    .small()
                    .color(muted),
            );
        }
    } else {
        ui.label(RichText::new(Text::WelcomeGameWhy.get()).color(muted));
    }
    ui.button(Text::WelcomeGameButton.get())
        .clicked()
        .then_some(WelcomeAction::GameFiles)
}

/// The second step: a new project from a template, a folder to open, and the recent projects.
fn project_step(ui: &mut Ui, theme: &Theme, recent: &[PathBuf]) -> Option<WelcomeAction> {
    step(ui, theme, "2", Text::WelcomeProject);
    let muted = theme.color(Token::TextMuted);
    ui.label(RichText::new(Text::WelcomeProjectHow.get()).color(muted));
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        for (template, text) in [
            (Template::Id1, Text::NewId1Menu),
            (Template::Rerelease, Text::NewRereleaseMenu),
        ] {
            if ui.button(text.get()).clicked() {
                action = Some(LeftAction::New(template));
            }
        }
        if ui.button(Text::OpenFolderButton.get()).clicked() {
            action = Some(LeftAction::OpenFolder);
        }
    });
    if !recent.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(Text::RecentProjects.get())
                    .small()
                    .color(muted),
            );
            for folder in recent {
                let row = ui
                    .add(Button::new(project::root_name(folder)).frame_when_inactive(false))
                    .on_hover_text(folder.to_string_lossy());
                if row.clicked() {
                    action = Some(LeftAction::Open(folder.clone()));
                }
            }
        });
    }
    action.map(WelcomeAction::Project)
}

/// A link a screen reader names by its text.
fn link(ui: &mut Ui, text: Text) -> egui::Response {
    let link = ui.link(text.get());
    ui.ctx().accesskit_node_builder(link.id, |node| {
        node.set_role(egui::accesskit::Role::Link);
        node.set_label(text.get());
    });
    link
}

/// The page's column; a click returns what it asks for.
fn column(ui: &mut Ui, theme: &Theme, page: &Page<'_>) -> Option<WelcomeAction> {
    let muted = theme.color(Token::TextMuted);
    ui.horizontal(|ui| {
        if let Some(icon) = page.icon {
            let side = egui::Vec2::splat(ICON_SIDE);
            ui.add(egui::Image::new((icon, side)).alt_text(Text::AboutIcon.get()));
        }
        ui.vertical(|ui| {
            ui.heading(Text::WelcomeTitle.get());
            ui.label(RichText::new(Text::WelcomeTagline.get()).color(muted));
        });
    });
    let mut action = game_step(ui, theme, page.files);
    action = project_step(ui, theme, page.recent).or(action);
    step(ui, theme, "3", Text::WelcomePlay);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(Text::WelcomePlayHow.get()).color(muted));
        if let Some(key) = &page.play_key {
            ui.label(RichText::new(key).monospace());
        }
    });
    ui.add_space(theme.space(GAP));
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        if link(ui, Text::WelcomePalette).clicked() {
            action = Some(WelcomeAction::Palette);
        }
        ui.label(RichText::new(&page.palette_key).monospace().color(muted));
        ui.add_space(theme.space(GAP));
        if link(ui, Text::WelcomeGuide).clicked() {
            action = Some(WelcomeAction::UserGuide);
        }
    });
    action
}

/// The page, its column centred in the room it is given and scrolled when that is too short;
/// a click returns what it asks for.
pub(crate) fn page(ui: &mut Ui, theme: &Theme, page: &Page<'_>) -> Option<WelcomeAction> {
    let mut action = None;
    egui::Frame::NONE
        .fill(theme.color(Token::SurfaceEditor))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let room = ui.available_rect_before_wrap();
                    let width = PAGE_WIDTH.min(room.width());
                    let corner = egui::pos2(
                        room.left() + (room.width() - width) * HALF,
                        room.top() + theme.space(GAP),
                    );
                    let size = egui::vec2(width, room.height());
                    let mut inside = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(egui::Rect::from_min_size(corner, size))
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    action = column(&mut inside, theme, page);
                    ui.advance_cursor_after_rect(inside.min_rect());
                });
        });
    action
}

#[cfg(test)]
mod tests;
