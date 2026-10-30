// SPDX-License-Identifier: GPL-3.0-or-later

//! The window (UI-2): the toolbar, the left (Project, Entities), centre (editor, game), right
//! (Properties, agent) and bottom (Problems, Console, Debugger) panels, and the status bar,
//! laid out as the approved play-loop mockup (`docs/design/mockups/01-play-loop.png`).
//!
//! The left panel lists the open project's files and symbols and the bottom panel its problems
//! (`qs-workbench`, [`crate::project`]); choosing one opens its file in the editor at that place.
//! The game, Entities, Properties and the agent are placeholders until the preview host and the
//! agent land. Showing, hiding and resizing a panel and opening a project or a file run a
//! command (`qs-layout`, `qs-workbench`), so a menu, a keybinding, the CLI and the agent do it
//! the same way, and the layout is written back when it changes.
//!
//! Every string comes from [`crate::strings`]; a test fails on a string literal here.

use std::path::{Path, PathBuf};

use eframe::egui::containers::panel::PanelState;
use eframe::egui::{self, Align, Button, Id, Layout as EguiLayout, RichText, Sense, Ui};
use qs_commands::{CommandError, RegisterError, Registry, Session};
use qs_editor_view::{Position, ViewStyle};
use qs_layout::{
    HasLayout, Layout, Panel, RESIZE_PANEL, ResizePanelParams, TOGGLE_PANEL, TogglePanelParams,
};
use qs_workbench::{
    AtParams, COMPLETE, Candidate, Config, DEFINITION, FsLoader, HOVER, HasWorkbench, HoverInfo,
    OPEN_FILE, OPEN_PROJECT, OpenFile, OpenFileParams, OpenProjectParams, Place, REFERENCES,
    RENAME, RenameParams, SAVE_FILE, SaveFileParams, UPDATE_FILE, UpdateFileParams, Workbench,
};

use crate::editor::{Editor, Popup, Request};
use crate::fallback::{SystemFonts, families};
use crate::icons::Icon;
use crate::keymap;
use crate::palette::Palette;
use crate::project::{self, Jump, SymbolScope};
use crate::strings::Text;
use crate::theme::{Theme, Token, semibold};

/// What the window's commands work on.
#[derive(Debug)]
pub struct Workspace {
    /// The panels' layout.
    pub layout: Layout,
    /// The open project and file.
    pub workbench: Workbench<FsLoader>,
}

impl HasWorkbench for Workspace {
    type Loader = FsLoader;

    fn workbench(&mut self) -> &mut Workbench<FsLoader> {
        &mut self.workbench
    }
}

impl HasLayout for Workspace {
    fn layout_mut(&mut self) -> &mut Layout {
        &mut self.layout
    }
}

/// The areas of the window, each egui's id for it. One enum, so no two share an id: a
/// field-less enum hashes only its variant's place, so two enums' first variants would.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) enum Area {
    Toolbar,
    StatusBar,
    Panel(Panel),
    Palette,
    Editor,
    Section(Text),
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

/// The editor's area, whose egui id is the editor view's.
pub(crate) const EDITOR_AREA: Area = Area::Editor;

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
    /// The command palette.
    pub(crate) palette: Palette,
    symbol_scope: SymbolScope,
    editor: Option<Editor>,
    system_fonts: SystemFonts,
    /// The editor's file and text revision whose characters have fonts.
    covered: Option<(PathBuf, u64)>,
}

impl Shell {
    /// The window with `layout`, kept in `layout_file`, drawn in `theme`, analysing projects as
    /// `config` says over their project files.
    ///
    /// # Errors
    ///
    /// [`RegisterError`] when the window's commands cannot be registered.
    pub fn new(
        layout: Layout,
        layout_file: Option<PathBuf>,
        theme: Theme,
        config: Config,
    ) -> Result<Self, RegisterError> {
        let mut registry = Registry::new();
        qs_layout::register(&mut registry)?;
        qs_workbench::register(&mut registry)?;
        let workspace = Workspace {
            layout,
            workbench: Workbench::new(FsLoader, config),
        };
        Ok(Self {
            session: Session::new(workspace, registry),
            theme,
            layout_file,
            saved: layout,
            bottom_tab: BottomTab::default(),
            centre_tab: CentreTab::default(),
            drawn: [None; 3],
            palette: Palette::default(),
            symbol_scope: SymbolScope::default(),
            editor: None,
            system_fonts: SystemFonts::new(&Theme::bundled_fonts()),
            covered: None,
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

    /// Runs the command `id` with `params`, as the palette, a key or a script does.
    ///
    /// # Errors
    ///
    /// [`CommandError`] when the command is unknown, its parameters are wrong or it fails.
    pub fn execute(
        &mut self,
        id: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CommandError> {
        self.session.execute(id, params)
    }

    /// The open project and file.
    #[must_use]
    pub const fn workbench(&self) -> &Workbench<FsLoader> {
        &self.session.context().workbench
    }

    /// The file in the editor and its primary caret, once the editor has drawn it.
    #[must_use]
    pub fn caret(&self) -> Option<(&Path, Position)> {
        self.editor
            .as_ref()
            .map(|editor| (editor.path.as_path(), editor.view.state().primary().head))
    }

    /// The editor, once it has drawn a file.
    #[cfg(test)]
    pub(crate) const fn editor(&self) -> Option<&Editor> {
        self.editor.as_ref()
    }

    /// The editor, once it has drawn a file, to change.
    #[cfg(test)]
    pub(crate) const fn editor_mut(&mut self) -> Option<&mut Editor> {
        self.editor.as_mut()
    }

    /// The editor's text, edits included, once the editor has drawn a file.
    #[must_use]
    pub fn editor_text(&self) -> Option<String> {
        self.editor
            .as_ref()
            .map(|editor| editor.view.state().buffer().text())
    }

    /// Gives the editor the keyboard, as a click in it does.
    pub fn focus_editor(&self, ctx: &egui::Context) {
        ctx.memory_mut(|m| m.request_focus(Id::new(EDITOR_AREA)));
    }

    /// Opens the project in `path`, through its command.
    ///
    /// # Errors
    ///
    /// [`ShellError`] when the command fails.
    pub fn open_project(&mut self, path: &Path) -> Result<(), ShellError> {
        let params = serde_json::to_value(OpenProjectParams {
            path: path.to_path_buf(),
        })?;
        self.session.execute(OPEN_PROJECT, params)?;
        Ok(())
    }

    /// Opens the file a panel's row asks for, through its command, and shows the editor.
    fn jump(&mut self, jump: Jump) {
        let params = OpenFileParams {
            path: jump.path,
            offset: Some(jump.offset),
        };
        let result = serde_json::to_value(params)
            .map_err(ShellError::from)
            .and_then(|params| Ok(self.session.execute(OPEN_FILE, params)?));
        match result {
            Ok(_) => self.centre_tab = CentreTab::Editor,
            Err(error) => tracing::warn!(%error, "file not opened"),
        }
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
        self.keys(ui);
        self.toolbar(ui);
        self.status_bar(ui);
        for panel in [Panel::Left, Panel::Right, Panel::Bottom] {
            self.panel(ui, panel);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(self.theme.color(Token::SurfaceEditor)))
            .show(ui, |ui| self.centre(ui));
        if let Some((id, params)) = self.palette.show(
            ui,
            Id::new(Area::Palette),
            &self.theme,
            self.session.registry(),
        ) {
            match self.session.execute(id.as_str(), params) {
                Ok(_) => self.palette.close(),
                Err(error) => self.palette.failed(error.to_string()),
            }
        }
        let dragging = ui.input(|input| input.pointer.any_down());
        self.save_if_changed(dragging);
    }

    /// The shortcuts pressed this frame: the palette's, and while it is closed and the editor
    /// does not have the keyboard each binding's command (`keymap`).
    fn keys(&mut self, ui: &Ui) {
        if ui.input_mut(|i| i.consume_shortcut(&keymap::OPEN_PALETTE)) {
            self.palette.open();
            return;
        }
        // The palette, and the editor with its own undo history, take their keys themselves.
        let editing = ui.memory(|m| m.has_focus(Id::new(EDITOR_AREA)));
        if self.palette.is_open() || editing {
            return;
        }
        for binding in keymap::BINDINGS {
            if ui.input_mut(|i| i.consume_shortcut(&binding.shortcut))
                && let Err(error) = self.session.execute(
                    binding.command,
                    serde_json::Value::Object(serde_json::Map::new()),
                )
            {
                tracing::warn!(%error, command = binding.command, "shortcut failed");
            }
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let theme = &self.theme;
        let fill = theme.color(Token::SurfaceToolbar);
        let mut toggled = None;
        let mut open_palette = false;
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
                        let search = Button::new(
                            RichText::new(labelled(Icon::Search, Text::SearchCommands))
                                .color(theme.color(Token::TextMuted)),
                        );
                        let shortcut = ui.ctx().format_shortcut(&keymap::OPEN_PALETTE);
                        if ui.add(search).on_hover_text(shortcut).clicked() {
                            open_palette = true;
                        }
                    });
                });
            });
        if let Some(panel) = toggled
            && let Err(error) = self.toggle(panel)
        {
            tracing::warn!(%error, "panel toggle failed");
        }
        if open_palette {
            self.palette.open();
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
                    let (errors, warnings) =
                        project::counts(self.session.context().workbench.problems());
                    for (icon, value) in [(Icon::Error, errors), (Icon::Warning, warnings)] {
                        ui.label(RichText::new(count(icon, value)).small().color(muted));
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
        let scope = &mut self.symbol_scope;
        let workbench = &self.session.context().workbench;
        let mut jump = None;
        let mut expanded = state.visible;
        // A size the layout was given by a command, not by a drag of the panel's edge, is
        // drawn: egui keeps the size it last drew and reads `default_size` only the first time.
        let drawn_size =
            PanelState::load(ui.ctx(), Id::new(Area::Panel(panel))).map(|stored| match panel {
                Panel::Left | Panel::Right => stored.size().x,
                Panel::Bottom => stored.size().y,
            });
        let held = ui.input(|input| input.pointer.any_down());
        let commanded = commanded_size(drawn_size, state.size, held);
        // `exact_size` pins the size range to one point, so it comes after `min_size`, which
        // would widen it again.
        let builder = builder.resizable(true).min_size(qs_layout::MIN_PANEL_SIZE);
        let builder = if commanded {
            builder.exact_size(state.size)
        } else {
            builder.default_size(state.size)
        };
        let shown = builder
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
                        jump = left(ui, theme, workbench, scope);
                        ui.add_space(theme.space(3));
                        section(ui, theme, Text::Entities, Text::NotPlaying);
                    }
                    Panel::Right => {
                        section(ui, theme, Text::Properties, Text::NotPlaying);
                        ui.add_space(theme.space(3));
                        section(ui, theme, Text::Agent, Text::NoAgent);
                    }
                    Panel::Bottom => jump = bottom(ui, theme, bottom_tab, workbench),
                }
            });
        if let Some(jump) = jump {
            self.jump(jump);
        }
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

    /// Runs the command `id` with `params`, logging a failure; its output, `None` when it
    /// failed.
    fn run<P: serde::Serialize, O: serde::de::DeserializeOwned>(
        &mut self,
        id: &str,
        params: &P,
    ) -> Option<O> {
        let result = serde_json::to_value(params)
            .map_err(ShellError::from)
            .and_then(|params| Ok(self.session.execute(id, params)?))
            .and_then(|output| Ok(serde_json::from_value(output)?));
        match result {
            Ok(output) => Some(output),
            Err(error) => {
                tracing::warn!(%error, command = id, "command failed");
                None
            }
        }
    }

    /// Runs what the editor asked for, each through its command, after sending the editor's
    /// unsent text so every answer is about the text on screen.
    fn handle(&mut self, requests: Vec<Request>) {
        for request in requests {
            let Some(editor) = self.editor.as_mut() else {
                return;
            };
            let path = editor.path.clone();
            let unsent = match &request {
                Request::Update(_) => None,
                _ => editor.unsent(),
            };
            if let Some(text) = unsent {
                let _: Option<()> = self.run(
                    UPDATE_FILE,
                    &UpdateFileParams {
                        path: path.clone(),
                        text,
                    },
                );
            }
            let at = |offset| AtParams {
                path: path.clone(),
                offset,
            };
            let popup = match request {
                Request::Update(text) => {
                    let _: Option<()> = self.run(UPDATE_FILE, &UpdateFileParams { path, text });
                    None
                }
                Request::Save => {
                    let _: Option<()> = self.run(SAVE_FILE, &SaveFileParams { path });
                    None
                }
                Request::Definition(offset) => {
                    match self
                        .run::<_, Option<Place>>(DEFINITION, &at(offset))
                        .flatten()
                    {
                        Some(place) => {
                            self.jump(Jump {
                                path: place.path,
                                offset: place.start,
                            });
                            None
                        }
                        None => Some(Popup::Note(Text::NoDefinition)),
                    }
                }
                Request::References(offset) => {
                    let places: Vec<Place> = self.run(REFERENCES, &at(offset)).unwrap_or_default();
                    Some(if places.is_empty() {
                        Popup::Note(Text::NoReferences)
                    } else {
                        Popup::References(places)
                    })
                }
                Request::Hover(offset, point) => self
                    .run::<_, Option<HoverInfo>>(HOVER, &at(offset))
                    .flatten()
                    .map(|info| Popup::Hover {
                        at: point,
                        text: match info.docs {
                            Some(docs) => {
                                let mut text = info.declaration;
                                text.push('\n');
                                text.push('\n');
                                text.push_str(&docs);
                                text
                            }
                            None => info.declaration,
                        },
                    }),
                Request::Complete(offset) => {
                    let candidates: Vec<Candidate> =
                        self.run(COMPLETE, &at(offset)).unwrap_or_default();
                    Some(if candidates.is_empty() {
                        Popup::Note(Text::NoCompletions)
                    } else {
                        Popup::Complete {
                            candidates,
                            selected: 0,
                        }
                    })
                }
                Request::Rename(offset, name) => {
                    let params = RenameParams { path, offset, name };
                    let done: Option<serde_json::Value> = self.run(RENAME, &params);
                    done.is_none().then_some(Popup::Note(Text::RenameFailed))
                }
                Request::Open(jump) => {
                    self.jump(jump);
                    None
                }
            };
            if let Some(popup) = popup
                && let Some(editor) = self.editor.as_mut()
            {
                editor.popup = Some(popup);
            }
        }
    }

    /// Brings the editor up to date with the open file: a new view for another file, the caret
    /// moved for another `file.open` of the same one, none for no file.
    fn sync_editor(&mut self) {
        // Another file's editor sends its edits before it goes: the workbench keeps them, so
        // the file shows them when it is opened again.
        let open_path = self.session.context().workbench.open().map(OpenFile::path);
        if let Some(editor) = self.editor.as_mut()
            && open_path != Some(editor.path.as_path())
            && let Some(text) = editor.unsent()
        {
            let path = editor.path.clone();
            let _: Option<()> = self.run(UPDATE_FILE, &UpdateFileParams { path, text });
        }
        let open = self.session.context().workbench.open();
        match (open, &mut self.editor) {
            (None, _) => self.editor = None,
            (Some(open), Some(editor)) if editor.path == open.path() => {
                editor.sync(open, &self.theme);
            }
            (Some(open), _) => {
                self.editor = Some(Editor::new(Id::new(EDITOR_AREA), open, &self.theme));
            }
        }
    }

    /// Gives egui a system font for each character of the editor's text that the bundled fonts
    /// lack: the whole text of a file just opened, then the caret's line as it is edited.
    fn cover_characters(&mut self, ctx: &egui::Context) {
        let Some(editor) = &self.editor else {
            return;
        };
        let state = editor.view.state();
        let now = (editor.path.clone(), state.revision());
        if self.covered.as_ref() == Some(&now) {
            return;
        }
        let text = match &self.covered {
            Some((path, _)) if *path == now.0 => {
                state.buffer().line(state.primary().head.line).to_owned()
            }
            Some(_) | None => state.buffer().text(),
        };
        self.system_fonts.cover(ctx, &families(), &text);
        self.covered = Some(now);
    }

    fn centre(&mut self, ui: &mut Ui) {
        self.sync_editor();
        self.cover_characters(ui.ctx());
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
        if self.centre_tab == CentreTab::Editor
            && let Some(editor) = &mut self.editor
        {
            let requests = editor.show(ui, theme, &view_style(theme));
            // The panels drawn before the editor this frame show what the requests change on
            // the next.
            if !requests.is_empty() {
                ui.ctx().request_repaint();
            }
            self.handle(requests);
            return;
        }
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
fn section(ui: &mut Ui, theme: &Theme, text: Text, empty: Text) {
    heading(ui, theme, text);
    note(ui, theme, empty);
}

/// A panel section's heading.
fn heading(ui: &mut Ui, theme: &Theme, text: Text) {
    ui.label(
        RichText::new(text.get())
            .family(semibold())
            .size(theme.type_scale.panel_heading.size),
    );
}

/// The share of the left panel's height the Project tree may take.
const TREE_SHARE: f32 = 0.35;
/// The share of the left panel's height the Symbols list may take.
const SYMBOLS_SHARE: f32 = 0.4;

/// The left panel's Project and Symbols sections; the row chosen, if any.
fn left(
    ui: &mut Ui,
    theme: &Theme,
    workbench: &Workbench<FsLoader>,
    scope: &mut SymbolScope,
) -> Option<Jump> {
    let height = ui.available_height();
    let mut jump = None;
    heading(ui, theme, Text::Project);
    match workbench.root() {
        None => note(ui, theme, Text::NoProject),
        Some(root) => {
            ui.label(RichText::new(project::root_name(root)).strong());
            let open = workbench.open().map(OpenFile::path);
            egui::ScrollArea::vertical()
                .id_salt(Area::Section(Text::Project))
                .max_height(height * TREE_SHARE)
                .auto_shrink([false, true])
                .show(ui, |ui| jump = project::tree_ui(ui, workbench.tree(), open));
        }
    }
    ui.add_space(theme.space(3));
    heading(ui, theme, Text::Symbols);
    let symbols = project::symbols_ui(
        ui,
        theme,
        scope,
        workbench.open(),
        workbench.project_symbols(),
        height * SYMBOLS_SHARE,
    );
    jump.or(symbols)
}

/// The editor's colours and font, from the theme's tokens.
fn view_style(theme: &Theme) -> ViewStyle {
    ViewStyle {
        font: egui::FontId::monospace(theme.type_scale.code.size),
        background: theme.color(Token::SurfaceEditor),
        current_line: theme.color(Token::SurfaceCurrentLine),
        selection: theme.color(Token::SelectFocus),
        text: theme.color(Token::TextCode),
        line_number: theme.color(Token::TextFaint),
        caret: theme.color(Token::Accent),
        added: theme.color(Token::DiffAdded),
        removed: theme.color(Token::StatusErrorBg),
        removed_text: theme.color(Token::TextOnError),
    }
}

/// A line of muted text, wrapped to the panel's width.
fn note(ui: &mut Ui, theme: &Theme, text: Text) {
    ui.add(egui::Label::new(RichText::new(text.get()).color(theme.color(Token::TextMuted))).wrap());
}

/// The bottom panel: its tabs, and the Problems list or the selected tab's placeholder; the
/// problem chosen, if any.
fn bottom(
    ui: &mut Ui,
    theme: &Theme,
    tab: &mut BottomTab,
    workbench: &Workbench<FsLoader>,
) -> Option<Jump> {
    let problems = workbench.problems();
    ui.horizontal(|ui| {
        for (value, text) in [
            (BottomTab::Problems, Text::Problems),
            (BottomTab::Console, Text::Console),
            (BottomTab::Debugger, Text::Debugger),
        ] {
            let label = match (value, problems.len()) {
                (BottomTab::Problems, shown @ 1..) => format!("{} {shown}", text.get()),
                _ => text.get().to_owned(),
            };
            ui.selectable_value(tab, value, label);
        }
    });
    ui.separator();
    let message = match tab {
        BottomTab::Problems => return project::problems_ui(ui, theme, problems, workbench.root()),
        BottomTab::Console => Text::ConsoleIdle,
        BottomTab::Debugger => Text::DebuggerIdle,
    };
    note(ui, theme, message);
    None
}

/// An icon and its label, as one string for a button.
/// Whether `layout`, a panel's size in the layout, was set by a command rather than by a drag of
/// the panel's edge, and so is drawn this frame: egui drew the panel at `drawn` last frame, the
/// two differ by half a point or more, and no pointer button is held, as one is during a drag.
pub(crate) fn commanded_size(drawn: Option<f32>, layout: f32, held: bool) -> bool {
    drawn.is_some_and(|drawn| (drawn - layout).abs() >= 0.5) && !held
}

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
