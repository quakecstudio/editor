// SPDX-License-Identifier: GPL-3.0-or-later

//! Inline help (P7-11, QA-16): a small `?` button beside each panel's heading, each tab row,
//! the toolbars and each dialog. Its tooltip says what it helps with; a click shows a short
//! explanation in place and a link to the section of the website's user guide that covers it,
//! which opens in the browser. The app itself makes no request.
//!
//! The guide's pages and anchors are listed in `help-links.txt`, which the site's check reads
//! (`scripts/check-docs.py` in quakecstudio/site), so a renamed heading fails there.

use eframe::egui::{self, Button, Response, RichText, Ui, Vec2};

use crate::icons::{ICON_BUTTON, mark};
use crate::strings::{self, Text};
use crate::theme::{Theme, Token};

/// The user guide on the website.
pub(crate) const GUIDE: &str = "https://www.quakecstudio.com/docs/";

/// The smallest a help button is, in points: macOS's minimum control size in Apple's Human
/// Interface Guidelines, Accessibility, "Offer sufficiently sized controls" (20 by 20 pt).
const HELP_SIZE: f32 = 20.0;

/// The widest a help popup's text runs, in points.
const HELP_WIDTH: f32 = 280.0;

/// What a help button explains.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Topic {
    /// The toolbar's play controls.
    Toolbar,
    /// The Project section.
    Project,
    /// The Symbols section.
    Symbols,
    /// The running game's entities.
    Entities,
    /// The running game's selected entity.
    Properties,
    /// The map's layers and entities in the left panel.
    MapOutline,
    /// The map editor's Properties: compiling and the selection's keys.
    MapProperties,
    /// The agent panel.
    Agent,
    /// The open file's outline.
    Outline,
    /// The Git tab.
    Git,
    /// The Problems tab.
    Problems,
    /// The Console tab.
    Console,
    /// The Debugger tab.
    Debugger,
    /// The Terminal tab.
    Terminal,
    /// The code editor.
    Editor,
    /// The project search.
    Search,
    /// The Game tab.
    Game,
    /// The map view and its tools.
    MapView,
    /// The map editor's Textures tab.
    MapTextures,
    /// The map editor's Entities tab.
    MapEntities,
    /// The map editor's Prefabs tab.
    MapPrefabs,
    /// The map editor's Issues tab.
    MapIssues,
    /// The window that finds the game's files.
    GameGuide,
    /// The texture import window.
    TextureImport,
}

impl Topic {
    /// Every topic.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 24] = [
        Self::Toolbar,
        Self::Project,
        Self::Symbols,
        Self::Entities,
        Self::Properties,
        Self::MapOutline,
        Self::MapProperties,
        Self::Agent,
        Self::Outline,
        Self::Git,
        Self::Problems,
        Self::Console,
        Self::Debugger,
        Self::Terminal,
        Self::Editor,
        Self::Search,
        Self::Game,
        Self::MapView,
        Self::MapTextures,
        Self::MapEntities,
        Self::MapPrefabs,
        Self::MapIssues,
        Self::GameGuide,
        Self::TextureImport,
    ];

    /// What the help is for, as the app names it.
    pub(crate) const fn title(self) -> Text {
        match self {
            Self::Toolbar => Text::Toolbar,
            Self::Project => Text::Project,
            Self::Symbols => Text::Symbols,
            Self::Entities => Text::Entities,
            Self::MapEntities => Text::MapEntitiesTitle,
            Self::Properties => Text::Properties,
            Self::MapProperties => Text::MapPropertiesTitle,
            Self::MapOutline => Text::Map,
            Self::MapView => Text::MapViewTitle,
            Self::Agent => Text::Agent,
            Self::Outline => Text::Outline,
            Self::Git => Text::Git,
            Self::Problems => Text::Problems,
            Self::Console => Text::Console,
            Self::Debugger => Text::Debugger,
            Self::Terminal => Text::Terminal,
            Self::Editor => Text::Editor,
            Self::Search => Text::ProjectSearch,
            Self::Game => Text::Game,
            Self::MapTextures => Text::MapTextures,
            Self::MapPrefabs => Text::MapPrefabs,
            Self::MapIssues => Text::MapIssues,
            Self::GameGuide => Text::GameGuide,
            Self::TextureImport => Text::MapImportTexture,
        }
    }

    /// The explanation the popup shows.
    pub(crate) const fn summary(self) -> Text {
        match self {
            Self::Toolbar => Text::HelpToolbar,
            Self::Project => Text::HelpProject,
            Self::Symbols => Text::HelpSymbols,
            Self::Entities => Text::HelpEntities,
            Self::Properties => Text::HelpProperties,
            Self::MapOutline => Text::HelpMapOutline,
            Self::MapProperties => Text::HelpMapProperties,
            Self::Agent => Text::HelpAgent,
            Self::Outline => Text::HelpOutline,
            Self::Git => Text::HelpGit,
            Self::Problems => Text::HelpProblems,
            Self::Console => Text::HelpConsole,
            Self::Debugger => Text::HelpDebugger,
            Self::Terminal => Text::HelpTerminal,
            Self::Editor => Text::HelpEditor,
            Self::Search => Text::HelpSearch,
            Self::Game => Text::HelpGame,
            Self::MapView => Text::HelpMapView,
            Self::MapTextures => Text::HelpMapTextures,
            Self::MapEntities => Text::HelpMapEntities,
            Self::MapPrefabs => Text::HelpMapPrefabs,
            Self::MapIssues => Text::HelpMapIssues,
            Self::GameGuide => Text::HelpGameGuide,
            Self::TextureImport => Text::HelpTextureImport,
        }
    }

    /// The guide's page and the anchor of the section that covers it, as `page#anchor`.
    pub(crate) const fn link(self) -> &'static str {
        match self {
            Self::Toolbar => "play-loop.html#the-toolbar",
            Self::Project | Self::Symbols | Self::Problems => "opening-a-mod.html#what-you-see",
            Self::Entities | Self::Properties => "play-loop.html#entities-and-properties",
            Self::MapOutline => "map-editor.html#2-open-a-map",
            Self::MapProperties | Self::MapEntities => "map-editor.html#3-edit-entities",
            Self::Agent => "agent.html#give-it-a-task",
            Self::Outline | Self::Editor => "opening-a-mod.html#moving-around-the-code",
            Self::Git => "opening-a-mod.html#git",
            Self::Console => "play-loop.html#the-console",
            Self::Debugger => "debugging.html#breakpoints",
            Self::Terminal => "opening-a-mod.html#the-terminal",
            Self::Search => "opening-a-mod.html#searching-the-project",
            Self::Game => "play-loop.html#save-to-reload",
            Self::MapView | Self::MapTextures | Self::MapPrefabs | Self::TextureImport => {
                "map-editor.html#4-build-with-brushes"
            }
            Self::MapIssues => "map-editor.html#6-find-a-leak",
            Self::GameGuide => "troubleshooting.html#play-says-it-cannot-find-quake",
        }
    }

    /// The address of the guide's section.
    pub(crate) fn url(self) -> String {
        format!("{GUIDE}{}", self.link())
    }
}

/// A `?` button for `topic`: its tooltip names what it helps with, and a click opens a popup
/// with the explanation and a link to the guide.
pub(crate) fn button(ui: &mut Ui, theme: &Theme, topic: Topic) -> Response {
    // Keyed by the topic, so the popup's state does not move with the widgets before it, and a
    // tab row's button for another tab is another button, whose popup starts closed.
    ui.push_id(topic, |ui| {
        let name = strings::help_for(topic.title().get());
        let button = Button::new(
            RichText::new("?")
                .small()
                .color(theme.color(Token::TextMuted)),
        )
        .min_size(Vec2::splat(HELP_SIZE))
        .corner_radius(HELP_SIZE / 2.0);
        let response = ui.add(button).on_hover_text(&name);
        // A screen reader reads the button by what it helps with, not by its `?`.
        mark(ui, &response, &name, ICON_BUTTON);
        egui::Popup::from_toggle_button_response(&response).show(|ui| {
            ui.set_max_width(HELP_WIDTH);
            ui.label(topic.summary().get());
            let link = ui.link(Text::HelpReadMore.get());
            // A link to a screen reader, named by its text, and opened by Enter or a click.
            ui.ctx().accesskit_node_builder(link.id, |node| {
                node.set_role(egui::accesskit::Role::Link);
                node.set_label(Text::HelpReadMore.get());
            });
            if link.clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(topic.url()));
            }
        });
        response
    })
    .inner
}

/// Opens each guide link the frame asked for in the system's browser.
pub(crate) fn open_links(ctx: &egui::Context) {
    for url in take_links(ctx) {
        match browser(&url).spawn() {
            // The opener hands the address to the browser and exits; its status is collected
            // so it does not linger.
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
            }
            Err(error) => tracing::warn!(%url, %error, "the browser could not be opened"),
        }
    }
}

/// The guide links the frame asked to open, taken from egui's output, which has no browser of
/// its own here; any other request is left where it is.
fn take_links(ctx: &egui::Context) -> Vec<String> {
    ctx.output_mut(|output| {
        let mut urls = Vec::new();
        output.commands.retain(|command| match command {
            egui::OutputCommand::OpenUrl(open) if open.url.starts_with(GUIDE) => {
                urls.push(open.url.clone());
                false
            }
            egui::OutputCommand::OpenUrl(_)
            | egui::OutputCommand::CopyText(_)
            | egui::OutputCommand::CopyImage(_) => true,
        });
        urls
    })
}

/// The system's command that opens `url` in the browser.
fn browser(url: &str) -> std::process::Command {
    #[cfg(target_os = "macos")]
    let (program, before): (&str, &[&str]) = ("open", &[]);
    // `start`'s first quoted argument is the window's title, so an empty one comes first.
    #[cfg(target_os = "windows")]
    let (program, before): (&str, &[&str]) = ("cmd", &["/C", "start", ""]);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let (program, before): (&str, &[&str]) = ("xdg-open", &[]);
    let mut command = std::process::Command::new(program);
    command.args(before).arg(url);
    command
}

#[cfg(test)]
mod tests;
