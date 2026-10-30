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
use qs_editor_view::{EditorView, Plain, Position, Selection, ViewStyle};
use qs_layout::{
    HasLayout, Layout, Panel, RESIZE_PANEL, ResizePanelParams, TOGGLE_PANEL, TogglePanelParams,
};
use qs_workbench::{
    Config, FsLoader, HasWorkbench, OPEN_FILE, OPEN_PROJECT, OpenFile, OpenFileParams,
    OpenProjectParams, Workbench,
};

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

/// The file in the editor: the view of its text and the `file.open` it shows.
struct Editor {
    path: PathBuf,
    generation: u64,
    view: EditorView,
}

impl Editor {
    /// The view of `open`, its caret where `open` puts it.
    fn new(open: &OpenFile) -> Self {
        let text = String::from_utf8_lossy(open.text());
        let mut editor = Self {
            path: open.path().to_path_buf(),
            generation: open.generation(),
            view: EditorView::new(Id::new(EDITOR_AREA), &text),
        };
        editor.place(open);
        editor
    }

    /// Moves the caret to where `open` puts it and scrolls it into view, a few lines down.
    fn place(&mut self, open: &OpenFile) {
        let location = open.location();
        let line = usize::try_from(location.line().saturating_sub(1)).unwrap_or(usize::MAX);
        let chars = usize::try_from(location.column().saturating_sub(1)).unwrap_or(usize::MAX);
        let state = self.view.state_mut();
        let col = state.buffer().byte_column(line, chars);
        state.set_selections(&[Selection::caret(Position::new(line, col))]);
        self.view.scroll_to(line.saturating_sub(CONTEXT_LINES));
        self.generation = open.generation();
    }
}

/// How many lines show above the caret when a file is opened at a place.
const CONTEXT_LINES: usize = 5;

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

    /// The editor's text, edits included, once the editor has drawn a file.
    #[must_use]
    pub fn editor_text(&self) -> Option<String> {
        self.editor
            .as_ref()
            .map(|editor| editor.view.state().buffer().text())
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

    /// The shortcuts pressed this frame: the palette's, and while it is closed each binding's
    /// command (`keymap`).
    fn keys(&mut self, ui: &Ui) {
        if ui.input_mut(|i| i.consume_shortcut(&keymap::OPEN_PALETTE)) {
            self.palette.open();
            return;
        }
        if self.palette.is_open() {
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

    /// Brings the editor up to date with the open file: a new view for another file, the caret
    /// moved for another `file.open` of the same one, none for no file.
    fn sync_editor(&mut self) {
        let open = self.session.context().workbench.open();
        match (open, &mut self.editor) {
            (None, _) => self.editor = None,
            (Some(open), Some(editor)) if editor.path == open.path() => {
                if editor.generation != open.generation() {
                    editor.place(open);
                }
            }
            (Some(open), _) => self.editor = Some(Editor::new(open)),
        }
    }

    fn centre(&mut self, ui: &mut Ui) {
        self.sync_editor();
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
            editor.view.show(ui, &view_style(theme), &Plain);
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
