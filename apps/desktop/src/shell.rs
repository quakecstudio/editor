// SPDX-License-Identifier: GPL-3.0-or-later

//! The window (UI-2): the toolbar, the left (Project, Entities), centre (editor, game), right
//! (Properties, agent) and bottom (Problems, Console, Debugger) panels, and the status bar,
//! laid out as the approved play-loop mockup (`docs/design/mockups/01-play-loop.png`).
//!
//! The panels are placeholders until the editor, the preview host and the agent land. Showing,
//! hiding and resizing a panel runs a command (`qs-layout`), so a menu, a keybinding, the CLI
//! and the agent do it the same way, and the layout is written back when it changes.
//!
//! Every string comes from [`crate::strings`]; a test fails on a string literal here.

use std::path::PathBuf;

use eframe::egui::containers::panel::PanelState;
use eframe::egui::{self, Align, Button, Id, Layout as EguiLayout, RichText, Sense, Ui};
use qs_commands::{CommandError, RegisterError, Registry, Session};
use qs_layout::{
    HasLayout, Layout, Panel, RESIZE_PANEL, ResizePanelParams, TOGGLE_PANEL, TogglePanelParams,
};

use crate::icons::Icon;
use crate::strings::Text;
use crate::theme::{Theme, Token, semibold};

/// What the window's commands work on.
#[derive(Debug, Default)]
pub struct Workspace {
    /// The panels' layout.
    pub layout: Layout,
}

impl HasLayout for Workspace {
    fn layout_mut(&mut self) -> &mut Layout {
        &mut self.layout
    }
}

/// The areas of the window, each egui's id for it. One enum, so no two share an id: a
/// field-less enum hashes only its variant's place, so two enums' first variants would.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum Area {
    Toolbar,
    StatusBar,
    Panel(Panel),
}

/// The tab shown in the bottom panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BottomTab {
    /// Problems.
    #[default]
    Problems,
    /// Console.
    Console,
    /// The debugger.
    Debugger,
}

/// The tab shown in the centre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CentreTab {
    /// The code editor.
    #[default]
    Editor,
    /// The game.
    Game,
}

/// The window's state.
pub struct Shell {
    session: Session<Workspace>,
    theme: Theme,
    /// Where the layout is kept between runs; `None` writes nothing.
    layout_file: Option<PathBuf>,
    /// The layout last written to `layout_file`.
    saved: Layout,
    bottom_tab: BottomTab,
    centre_tab: CentreTab,
    /// Each panel's size as drawn in the last frame, left, right and bottom.
    pub(crate) drawn: [Option<f32>; 3],
}

impl Shell {
    /// The window with `layout`, kept in `layout_file`, drawn in `theme`.
    ///
    /// # Errors
    ///
    /// [`RegisterError`] when the window's commands cannot be registered.
    pub fn new(
        layout: Layout,
        layout_file: Option<PathBuf>,
        theme: Theme,
    ) -> Result<Self, RegisterError> {
        let mut registry = Registry::new();
        qs_layout::register(&mut registry)?;
        Ok(Self {
            session: Session::new(Workspace { layout }, registry),
            theme,
            layout_file,
            saved: layout,
            bottom_tab: BottomTab::default(),
            centre_tab: CentreTab::default(),
            drawn: [None; 3],
        })
    }

    /// The layout as it is now.
    #[must_use]
    pub const fn layout(&self) -> &Layout {
        &self.session.context().layout
    }

    /// The window's command session.
    #[must_use]
    pub const fn session(&self) -> &Session<Workspace> {
        &self.session
    }

    /// Shows or hides `panel`, through its command.
    ///
    /// # Errors
    ///
    /// [`ShellError`] when the command fails.
    pub fn toggle(&mut self, panel: Panel) -> Result<(), ShellError> {
        let params = serde_json::to_value(TogglePanelParams { panel })?;
        self.session.execute(TOGGLE_PANEL, params)?;
        Ok(())
    }

    /// Sets `panel`'s size, through its command, when it differs by half a point or more.
    ///
    /// # Errors
    ///
    /// [`ShellError`] when the command refuses the size.
    pub fn resize(&mut self, panel: Panel, size: f32) -> Result<(), ShellError> {
        if (self.layout().panel(panel).size - size).abs() < 0.5 {
            return Ok(());
        }
        let params = serde_json::to_value(ResizePanelParams { panel, size })?;
        self.session.execute(RESIZE_PANEL, params)?;
        Ok(())
    }

    /// Writes the layout when it has changed since it was last written, and no drag is under
    /// way.
    pub(crate) fn save_if_changed(&mut self, dragging: bool) {
        let layout = *self.layout();
        if dragging || layout == self.saved {
            return;
        }
        self.saved = layout;
        if let Some(path) = &self.layout_file
            && let Err(error) = layout.write(path)
        {
            tracing::warn!(%error, "layout not saved");
        }
    }

    /// Draws the window for one frame.
    pub fn ui(&mut self, ui: &mut Ui) {
        self.toolbar(ui);
        self.status_bar(ui);
        for panel in [Panel::Left, Panel::Right, Panel::Bottom] {
            self.panel(ui, panel);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(self.theme.color(Token::SurfaceEditor)))
            .show(ui, |ui| self.centre(ui));
        let dragging = ui.input(|input| input.pointer.any_down());
        self.save_if_changed(dragging);
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let theme = &self.theme;
        let fill = theme.color(Token::SurfaceToolbar);
        let mut toggled = None;
        egui::Panel::top(Id::new(Area::Toolbar))
            .exact_size(theme.size.toolbar)
            .frame(egui::Frame::NONE.fill(fill).inner_margin(theme.space(1)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let play = Button::new(
                        RichText::new(labelled(Icon::Play, Text::Play))
                            .color(theme.color(Token::TextOnAccent)),
                    )
                    .fill(theme.color(Token::Accent));
                    let not_yet = Text::NotYetAvailable.get();
                    ui.add_enabled(false, play).on_disabled_hover_text(not_yet);
                    for (icon, text) in [
                        (Icon::Pause, Text::Pause),
                        (Icon::Step, Text::Step),
                        (Icon::Restart, Text::RestartMap),
                    ] {
                        ui.add_enabled(false, Button::new(labelled(icon, text)).frame(false))
                            .on_disabled_hover_text(not_yet);
                    }
                    ui.separator();
                    ui.label(Text::PreviewHost.get());
                    ui.label(
                        RichText::new(Text::PreviewDocked.get())
                            .small()
                            .color(theme.color(Token::TextMuted)),
                    );
                    ui.with_layout(EguiLayout::right_to_left(Align::Center), |ui| {
                        for (panel, icon, text) in [
                            (Panel::Right, Icon::PanelRight, Text::ToggleRightPanel),
                            (Panel::Bottom, Icon::PanelBottom, Text::ToggleBottomPanel),
                            (Panel::Left, Icon::PanelLeft, Text::ToggleLeftPanel),
                        ] {
                            let button = Button::new(icon.char().to_string())
                                .frame(false)
                                .selected(self.session.context().layout.panel(panel).visible);
                            if ui.add(button).on_hover_text(text.get()).clicked() {
                                toggled = Some(panel);
                            }
                        }
                        ui.add_enabled(
                            false,
                            Button::new(
                                RichText::new(labelled(Icon::Search, Text::SearchCommands))
                                    .color(theme.color(Token::TextMuted)),
                            ),
                        )
                        .on_disabled_hover_text(not_yet);
                    });
                });
            });
        if let Some(panel) = toggled
            && let Err(error) = self.toggle(panel)
        {
            tracing::warn!(%error, "panel toggle failed");
        }
    }

    fn status_bar(&self, ui: &mut Ui) {
        let theme = &self.theme;
        egui::Panel::bottom(Id::new(Area::StatusBar))
            .exact_size(theme.size.status_bar)
            .frame(
                egui::Frame::NONE
                    .fill(theme.color(Token::SurfaceChrome))
                    .inner_margin(egui::Margin::symmetric(8, 0)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let muted = theme.color(Token::TextMuted);
                    for icon in [Icon::Error, Icon::Warning] {
                        ui.label(RichText::new(count(icon, 0)).small().color(muted));
                    }
                    ui.label(
                        RichText::new(Text::PreviewStopped.get())
                            .small()
                            .color(muted),
                    );
                });
            });
    }

    fn panel(&mut self, ui: &mut Ui, panel: Panel) {
        let state = *self.layout().panel(panel);
        let theme = &self.theme;
        let builder = match panel {
            Panel::Left => egui::Panel::left(Id::new(Area::Panel(panel))),
            Panel::Right => egui::Panel::right(Id::new(Area::Panel(panel))),
            Panel::Bottom => egui::Panel::bottom(Id::new(Area::Panel(panel))),
        };
        let frame = egui::Frame::NONE
            .fill(theme.color(Token::SurfacePanel))
            .inner_margin(theme.space(1))
            .stroke(egui::Stroke::new(1.0, theme.color(Token::Border)));
        let bottom_tab = &mut self.bottom_tab;
        let mut expanded = state.visible;
        let shown = builder
            .resizable(true)
            .default_size(state.size)
            .min_size(qs_layout::MIN_PANEL_SIZE)
            .frame(frame)
            .show_collapsible(ui, &mut expanded, |ui| {
                // egui 0.36 stores a panel's size as its contents' rect, so the contents take
                // the whole size the panel was given: it neither shrinks to them nor grows
                // with them.
                let given = ui.max_rect().size();
                match panel {
                    Panel::Left | Panel::Right => {
                        ui.set_min_width(given.x);
                        ui.set_max_width(given.x);
                    }
                    Panel::Bottom => {
                        ui.set_min_height(given.y);
                        ui.set_max_height(given.y);
                    }
                }
                match panel {
                    Panel::Left => {
                        section(ui, theme, Text::Project, Text::NoProject);
                        ui.add_space(theme.space(3));
                        section(ui, theme, Text::Entities, Text::NotPlaying);
                    }
                    Panel::Right => {
                        section(ui, theme, Text::Properties, Text::NotPlaying);
                        ui.add_space(theme.space(3));
                        section(ui, theme, Text::Agent, Text::NoAgent);
                    }
                    Panel::Bottom => bottom(ui, theme, bottom_tab),
                }
            });
        // A panel dragged shut is hidden through its command too.
        if expanded != state.visible
            && let Err(error) = self.toggle(panel)
        {
            tracing::warn!(%error, "panel toggle failed");
        }
        // A size that changes while the pointer is held is a drag of the panel's edge: that
        // one goes to the layout, not a size that follows the panel's contents.
        // The panel's size is egui's stored outer size, not its contents' rect.
        let dragging = ui.input(|input| input.pointer.any_down());
        let stored = PanelState::load(ui.ctx(), Id::new(Area::Panel(panel)));
        let size = shown
            .filter(|_| expanded)
            .and(stored)
            .map(|stored| match panel {
                Panel::Left | Panel::Right => stored.size().x,
                Panel::Bottom => stored.size().y,
            });
        let index = match panel {
            Panel::Left => 0,
            Panel::Right => 1,
            Panel::Bottom => 2,
        };
        let previous = self.drawn.get(index).copied().flatten();
        if let Some(slot) = self.drawn.get_mut(index) {
            *slot = size;
        }
        if let Some(size) = dragged_size(previous, size, dragging)
            && let Err(error) = self.resize(panel, size)
        {
            tracing::debug!(%error, "panel resize refused");
        }
    }

    fn centre(&mut self, ui: &mut Ui) {
        let theme = &self.theme;
        egui::Frame::NONE
            .fill(theme.color(Token::SurfaceTabs))
            .inner_margin(egui::Margin::symmetric(8, 0))
            .show(ui, |ui| {
                ui.set_height(theme.size.tabs);
                ui.horizontal_centered(|ui| {
                    for (tab, text) in [
                        (CentreTab::Editor, Text::Editor),
                        (CentreTab::Game, Text::Game),
                    ] {
                        ui.selectable_value(&mut self.centre_tab, tab, text.get());
                    }
                });
            });
        let (fill, message) = match self.centre_tab {
            CentreTab::Editor => (Token::SurfaceEditor, Text::NoFileOpen),
            CentreTab::Game => (Token::SurfaceViewport, Text::GameStopped),
        };
        egui::Frame::NONE.fill(theme.color(fill)).show(ui, |ui| {
            let size = ui.available_size();
            ui.allocate_ui_with_layout(
                size,
                EguiLayout::centered_and_justified(egui::Direction::TopDown),
                |ui| {
                    ui.label(RichText::new(message.get()).color(theme.color(Token::TextMuted)));
                },
            );
            ui.allocate_response(egui::Vec2::ZERO, Sense::hover());
        });
    }
}

/// The size a drag of a panel's edge gave it: its size this frame, when the pointer is held
/// and the size has moved half a point or more since the last frame. A size that changes
/// without a drag, or the first frame's, is not one.
pub(crate) fn dragged_size(previous: Option<f32>, now: Option<f32>, dragging: bool) -> Option<f32> {
    let (previous, now) = (previous?, now?);
    (dragging && (now - previous).abs() >= 0.5).then_some(now)
}

/// A panel section: its heading and, until it has content, one line of explanation.
fn section(ui: &mut Ui, theme: &Theme, heading: Text, empty: Text) {
    ui.label(
        RichText::new(heading.get())
            .family(semibold())
            .size(theme.type_scale.panel_heading.size),
    );
    note(ui, theme, empty);
}

/// A line of muted text, wrapped to the panel's width.
fn note(ui: &mut Ui, theme: &Theme, text: Text) {
    ui.add(egui::Label::new(RichText::new(text.get()).color(theme.color(Token::TextMuted))).wrap());
}

/// The bottom panel: its tabs and the selected one's placeholder.
fn bottom(ui: &mut Ui, theme: &Theme, tab: &mut BottomTab) {
    ui.horizontal(|ui| {
        for (value, text) in [
            (BottomTab::Problems, Text::Problems),
            (BottomTab::Console, Text::Console),
            (BottomTab::Debugger, Text::Debugger),
        ] {
            ui.selectable_value(tab, value, text.get());
        }
    });
    ui.separator();
    let message = match tab {
        BottomTab::Problems => Text::NoProblems,
        BottomTab::Console => Text::ConsoleIdle,
        BottomTab::Debugger => Text::DebuggerIdle,
    };
    note(ui, theme, message);
}

/// An icon and its label, as one string for a button.
fn labelled(icon: Icon, text: Text) -> String {
    let mut label = String::from(icon.char());
    label.push(' ');
    label.push_str(text.get());
    label
}

/// An icon and a count, for the status bar.
fn count(icon: Icon, value: usize) -> String {
    let mut label = String::from(icon.char());
    label.push(' ');
    label.push_str(&value.to_string());
    label
}

/// Why a window command did not run.
#[derive(Debug)]
pub enum ShellError {
    /// Its parameters could not be written as JSON.
    Params(serde_json::Error),
    /// The command failed.
    Command(CommandError),
}

impl From<serde_json::Error> for ShellError {
    fn from(error: serde_json::Error) -> Self {
        Self::Params(error)
    }
}

impl From<CommandError> for ShellError {
    fn from(error: CommandError) -> Self {
        Self::Command(error)
    }
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Params(error) => error.fmt(f),
            Self::Command(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ShellError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Params(error) => Some(error),
            Self::Command(error) => Some(error),
        }
    }
}
