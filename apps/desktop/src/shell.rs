// SPDX-License-Identifier: GPL-3.0-or-later

//! The window (UI-2): the toolbar, the left (Project, Entities), centre (editor, game), right
//! (Properties, agent) and bottom (Problems, Console, Debugger) panels, and the status bar,
//! laid out as the approved play-loop mockup (`docs/design/mockups/01-play-loop.png`).
//!
//! The left panel lists the open project's files and symbols and the bottom panel its problems
//! (`qs-workbench`, [`crate::project`]); choosing one opens its file in the editor at that place.
//! Showing, hiding and resizing a panel and opening a project or a file run a
//! command (`qs-layout`, `qs-workbench`), so a menu, a keybinding, the CLI and the agent do it
//! the same way, and the layout is written back when it changes.
//!
//! Every string comes from [`crate::strings`]; a test fails on a string literal here.

use std::path::{Path, PathBuf};

use eframe::egui::containers::panel::PanelState;
use eframe::egui::{
    self, Align, Button, Id, Layout as EguiLayout, Response, RichText, Sense, Ui, accesskit,
};
use eframe::egui_wgpu::RenderState;
use qs_commands::remote::Host;
use qs_commands::{CommandError, RegisterError, Registry, Session};
use qs_editor_view::{
    Fixable, KeyMode, LineNote, NoteSeverity, Position, Selection, ViewStyle, Wrap,
};
use qs_layout::{
    DockPanel, Docks, EditorRequest, Focus, GO_TO_LINE, HasLayout, Layout, MovePanelParams,
    Outline, Panel, RESIZE_PANEL, ResizePanelParams, ShowPanelParams, SoftWrap, Split,
    TOGGLE_PANEL, TogglePanelParams, WhenShown, Zoom, ZoomParams,
};
use qs_map_edit::opens_in_map_editor;
use qs_render::{Camera, MapMesh};
use qs_settings::SettingsFile;
use qs_templates::{GitHub, Template};
use qs_workbench::TASK_LIST;
use qs_workbench::dock::DockFrame;
use qs_workbench::game_files::GameFiles;
use qs_workbench::ids::ActionIndex;
use qs_workbench::map_profile::MAP_RUN_PROFILE;
use qs_workbench::open_paths::{OPEN_PATHS, OpenPathsParams, PathsOpened, TAKE_OPENED_PATHS};
use qs_workbench::search::{
    REPLACE_IN_FILE, REPLACE_RANGE, ReplaceInFileParams, ReplaceRangeParams, ReplacedInFile,
};
use qs_workbench::terminal::TaskInfo;
use qs_workbench::tools::PROJECT_FILES;
use qs_workbench::{ActionInfo, CODE_ACTIONS, CODE_FIX, Code, CodeActionParams, FORMAT_FILE};
use qs_workbench::{
    AtParams, CHOOSE_GAME, CLOSE_FILE, COMPLETE, Candidate, CheckAfterTyping, ChooseGameParams,
    CloseFileParams, Config, DEFINITION, DOCK, EntryKind, FsLoader, HOVER, HasPreview,
    HasWorkbench, HoverInfo, OPEN_FILE, OpenFile, OpenFileParams, PAUSE, PLAY, POLL, Place,
    PlayParams, PlayState, Preview, PreviewConfig, PreviewStatus, REFERENCES, RELOAD, RENAME,
    RESTART, RESUME, ReloadParams, RenameParams, SAVE_FILE, STEP, STOP, SaveFileParams,
    TAKE_CHECKED, UPDATE_FILE, UpdateFileParams, Workbench,
};
use qs_workbench::{SELECT, SET_FIELD, SelectParams, SelectedEntity, SetFieldParams};

use crate::agent::{self, AgentAction, AgentPanel, ThreadIndex};
use crate::command_line::CommandLineView;
use crate::debugger::DebugAction;
use crate::editor::{Editor, Popup, Request};
use crate::editors::EditorsFeature;
use crate::entities::EntityAction;
use crate::fallback::SystemFonts;
use crate::features::{
    BottomAction, Commands, Dock, FeatureAction, Features, LeftAction, MapShown, ShellRequest,
};
use crate::features::{panel_icon, panel_topic};
use crate::finder::FileFinder;
use crate::game_view::GameSlot;
use crate::git_panel::TabShown;
use crate::help::{self, Topic};
use crate::icons::Icon;
use crate::icons::Toggled;
use crate::keymap::{self, Scope};
use crate::map_editor::MapAction;
#[cfg(test)]
use crate::map_editor::MapEditor;
use crate::map_feature::MapFeature;
use crate::map_profiles::ProfileAction;
use crate::map_view::{self, MapView};
#[cfg(test)]
use crate::multibuffer::Multibuffer;
use crate::multibuffer::{MultiAction, Source as MultiSource, multibuffer_ui};
use crate::palette::Palette;
use crate::picker::{PickerKind, PickerRow};
use crate::project::ProjectFeature;
use crate::project::{self, Jump, SymbolScope};
use crate::strings::panel_button;
use crate::strings::{Text, Truncation};
use crate::theme::{Theme, Token, semibold};
#[cfg(test)]
use qs_map_edit::uv::FaceRef;

/// What the window's commands work on.
#[derive(Debug)]
pub struct Workspace {
    /// The panels' layout.
    pub layout: Layout,
    /// The open project and file.
    pub workbench: Workbench<FsLoader>,
    /// The play loop: the preview host, when it runs.
    pub preview: Preview,
}

impl HasPreview for Workspace {
    fn workbench_and_preview(&mut self) -> (&mut Workbench<FsLoader>, &mut Preview) {
        (&mut self.workbench, &mut self.preview)
    }
}

impl qs_map_edit::HasMap for Workspace {
    type Project = Workbench<FsLoader>;
    type Live = Preview;

    fn map_store(&mut self) -> &mut qs_map_edit::MapStore {
        self.workbench.maps_mut()
    }

    fn map_project(&mut self) -> &mut Workbench<FsLoader> {
        &mut self.workbench
    }

    fn live_map(&mut self) -> Option<&mut Preview> {
        Some(&mut self.preview)
    }
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
    /// The notice below the toolbar while a reload waits (P3-10).
    ResetNotice,
    StatusBar,
    /// The command line above the status bar, and its field (UI-23).
    CommandLine,
    CommandLineField,
    Panel(Panel),
    Palette,
    Finder,
    Editor,
    Section(Text),
    /// The go to symbol picker and the tab switcher (P4-32).
    Picker,
    /// The outline panel beside the editor.
    Outline,
    /// The editor's second pane.
    SecondPane,
    /// The Map tab, when it scrolls (P7-12).
    MapTab,
}

/// Where the app's menu is (P3-44).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuPlace {
    /// The system's menu bar: macOS's, and the window's tests, which draw no menu.
    #[default]
    Native,
    /// The toolbar, on the systems with no menu bar of their own: Windows and Linux.
    InWindow,
}

/// What a click in the status bar asked for (P7-15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusAsk {
    /// Show this panel, or hide its dock when it shows already.
    Show(DockPanel),
    /// Move this panel to that dock.
    Move(DockPanel, Panel),
}

/// The tab shown in the centre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CentreTab {
    /// The code editor.
    #[default]
    Editor,
    /// The game.
    Game,
    /// The map view.
    Map,
    /// The multibuffer: the project search, a name's references or the problems (P4-33).
    Search,
}

/// Which way a zoom item moves the interface's size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomStep {
    /// One step larger.
    In,
    /// One step smaller.
    Out,
    /// Back to 100 percent.
    Actual,
}

/// Whether a pointer button is held, as one is during a drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointerHeld {
    /// A button is down.
    Down,
    /// None is.
    Up,
}

/// Whether an area takes the keyboard focus this frame, as a focus request asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FocusNow {
    /// It takes it.
    Take,
    /// It leaves it where it is.
    Leave,
}

impl FocusNow {
    /// `area`'s, for the focus request `pending`.
    fn of(pending: Option<Focus>, area: Focus) -> Self {
        if pending == Some(area) {
            Self::Take
        } else {
            Self::Leave
        }
    }
}

/// The editor's area, whose egui id is the editor view's.
pub(crate) const EDITOR_AREA: Area = Area::Editor;
/// How often the open map is backed up while it has unsaved edits, in seconds (P6-20).
pub(crate) const MAP_BACKUP_INTERVAL: f64 = 60.0;

/// The window's state.
pub struct Shell {
    session: Session<Workspace>,
    theme: Theme,
    /// Where the layout is kept between runs; `None` writes nothing.
    layout_file: Option<PathBuf>,
    /// The layout last written to `layout_file`.
    saved: Layout,
    /// The zoom factor the last frame drew at.
    zoom_drawn: f32,
    centre_tab: CentreTab,
    /// Each panel's size as drawn in the last frame, left, right and bottom.
    pub(crate) drawn: [Option<f32>; 3],
    /// The command palette.
    pub(crate) palette: Palette,
    /// The command line above the status bar (UI-23).
    pub(crate) command_line: CommandLineView,
    /// The file finder.
    pub(crate) finder: FileFinder,
    symbol_scope: SymbolScope,
    system_fonts: SystemFonts,
    /// The window's wgpu device and formats, which the map view draws with.
    render_state: Option<RenderState>,
    /// The game run in this window, drawn in the Game tab (ENG-10, ADR-0028).
    game: GameSlot,
    /// Whether the Game tab was shown for the game that runs in it, once for each Play.
    game_shown: bool,
    /// The count of focus commands the shell has applied (`Layout::focus_requests`): the
    /// layout's when the window opens, so the keyboard moves only when a focus command runs.
    focus_applied: u64,
    /// The last play-loop command's error, shown on the toolbar until the next one.
    preview_error: Option<String>,
    /// Where the app's menu is: the system's menu bar, or the toolbar in the window (P3-44).
    menu: MenuPlace,
    /// Each feature's state (P6-41).
    features: Features,
    /// The user's settings file, which keeps the game folder and the agent's permissions
    /// (P3-44, P4-30).
    settings_file: Option<PathBuf>,
    /// The key bindings in force, the user's keymap file over the defaults (P4-37).
    keymap: keymap::Keymap,
    /// How the code editor reads keys, from the user's settings (P4-37).
    key_mode: KeyMode,
    /// The IDE's window as last given to `preview.dock`.
    dock_frame: Option<DockFrame>,
    /// The socket other processes run the window's commands on (AGT-7), once it is served.
    host: Option<Host>,
    /// The window's egui context, to ask for a frame from another thread.
    ctx: Option<egui::Context>,
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
        check: CheckAfterTyping,
    ) -> Result<Self, RegisterError> {
        let mut registry = Registry::new();
        qs_layout::register(&mut registry)?;
        qs_workbench::register(&mut registry)?;
        qs_workbench::register_preview(&mut registry)?;
        qs_templates::register(&mut registry, std::sync::Arc::new(GitHub::new()))?;
        let game = GameSlot::default();
        game.set_place(layout.game_place);
        let workspace = Workspace {
            layout,
            // P3-35: the check after typing, off the frame in the app.
            workbench: Workbench::with_check(FsLoader, config, check),
            // ADR-0028: the game runs in this window when the layout puts it in the Game tab.
            preview: Preview::with_embedded(preview_config(), game.host()),
        };
        Ok(Self {
            session: Session::new(workspace, registry),
            theme,
            layout_file,
            saved: layout,
            zoom_drawn: Zoom::ACTUAL.get(),
            centre_tab: CentreTab::default(),
            drawn: [None; 3],
            palette: Palette::default(),
            command_line: CommandLineView::default(),
            finder: FileFinder::default(),
            symbol_scope: SymbolScope::default(),
            system_fonts: SystemFonts::new(&Theme::bundled_fonts()),
            render_state: None,
            game,
            game_shown: false,
            focus_applied: layout.focus_requests,
            preview_error: None,
            menu: MenuPlace::default(),
            settings_file: None,
            features: Features::new(AgentPanel::new(Box::new(agent::provider_model))),
            keymap: keymap::Keymap::defaults(),
            key_mode: KeyMode::Standard,
            dock_frame: None,
            host: None,
            ctx: None,
        })
    }

    /// Answers other processes' commands on `host` from now on, each frame, in the window's
    /// session (AGT-7): `quakec-ide mcp --connect` runs the agent's tools here.
    pub fn set_host(&mut self, host: Host) {
        self.host = Some(host);
    }

    /// Zooms the interface one step `step`, through `view.zoom` (P7-12).
    pub fn zoom(&mut self, step: ZoomStep) {
        let zoom = self.layout().zoom;
        let zoom = match step {
            ZoomStep::In => zoom.zoomed_in(),
            ZoomStep::Out => zoom.zoomed_out(),
            ZoomStep::Actual => Zoom::ACTUAL,
        };
        let _: Option<Zoom> = self.run(qs_layout::ZOOM, &ZoomParams { zoom });
    }

    /// Draws at the layout's zoom: one set by a command is applied, and one egui's keys set
    /// (Cmd+Plus, Cmd+Minus, Cmd+0) is held to the range and kept through `view.zoom`.
    fn follow_zoom(&mut self, ctx: &egui::Context) {
        let drawn = ctx.zoom_factor();
        let asked = self.layout().zoom;
        if (drawn - self.zoom_drawn).abs() > f32::EPSILON {
            let zoom = Zoom::new(drawn);
            let _: Option<Zoom> = self.run(qs_layout::ZOOM, &ZoomParams { zoom });
            ctx.set_zoom_factor(zoom.get());
            self.zoom_drawn = zoom.get();
        } else if (asked.get() - drawn).abs() > f32::EPSILON {
            ctx.set_zoom_factor(asked.get());
            self.zoom_drawn = asked.get();
        }
    }

    /// The theme the window draws with.
    #[must_use]
    pub const fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Draws with `theme` from the next frame, as the system's display settings changed.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
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
        self.features
            .editors
            .editor
            .as_ref()
            .map(|editor| (editor.path.as_path(), editor.view.state().primary().head))
    }

    /// The editor of the second pane, while the editor is split (P4-32).
    #[cfg(test)]
    pub(crate) const fn second_pane_editor(&self) -> Option<&Editor> {
        self.features.editors.second.as_ref()
    }

    /// The editor, once it has drawn a file.
    #[cfg(test)]
    pub(crate) const fn editor(&self) -> Option<&Editor> {
        self.features.editors.editor.as_ref()
    }

    /// The editor, once it has drawn a file, to change.
    #[cfg(test)]
    pub(crate) const fn editor_mut(&mut self) -> Option<&mut Editor> {
        self.features.editors.editor.as_mut()
    }

    /// Replaces the system font fallback, as a test does to choose what the "system" has.
    #[cfg(test)]
    pub(crate) fn set_system_fonts(&mut self, fonts: SystemFonts) {
        self.system_fonts = fonts;
    }

    /// The system fonts given to egui so far.
    #[cfg(test)]
    pub(crate) fn system_fonts_added(&self) -> usize {
        self.system_fonts.added()
    }

    /// The editor's text, edits included, once the editor has drawn a file.
    #[must_use]
    pub fn editor_text(&self) -> Option<String> {
        self.features
            .editors
            .editor
            .as_ref()
            .map(|editor| editor.view.state().buffer().text())
    }

    /// Gives the editor the keyboard, as a click in it does.
    pub fn focus_editor(&self, ctx: &egui::Context) {
        ctx.memory_mut(|m| m.request_focus(Id::new(EDITOR_AREA)));
    }

    /// Reads the user's keymap file `file` over the default bindings, and reads it again
    /// whenever it changes (P4-37).
    pub fn set_keymap_file(&mut self, file: PathBuf) {
        let registry = self.session.registry();
        self.keymap = keymap::Keymap::read(file, &|id: &str| keymap::bindable(registry, id));
        for problem in self.keymap.problems() {
            tracing::warn!(%problem, "keymap");
        }
    }

    /// The key bindings in force.
    #[must_use]
    pub const fn keymap(&self) -> &keymap::Keymap {
        &self.keymap
    }

    /// Reads the recent projects from `file`, and keeps them there.
    pub fn set_recent_file(&mut self, file: PathBuf) {
        self.features.project.set_recent_file(file);
    }

    /// Reads and keeps the user's settings in `file` (P3-44): the game folder chosen before is
    /// Play's again when it still holds a game, and is left unused, as missing, when it does
    /// not.
    pub fn set_settings_file(&mut self, file: PathBuf) {
        let settings = SettingsFile::load(&file)
            .map_err(|error| tracing::warn!(%error, "settings not read"))
            .ok();
        self.key_mode = match settings.as_ref().and_then(|s| s.editor.key_mode) {
            Some(qs_settings::KeyMode::Vim) => KeyMode::Vim,
            Some(qs_settings::KeyMode::Standard) | None => KeyMode::Standard,
        };
        let saved = settings.and_then(|settings| settings.game.folder);
        self.settings_file = Some(file);
        if let Some(path) = saved {
            let chosen: Option<GameFiles> = self.run(CHOOSE_GAME, &ChooseGameParams { path });
            if chosen.is_some_and(|files| !files.state.plays()) {
                tracing::info!("the saved game folder no longer holds a game");
            }
        }
    }

    /// Puts the app's menu in the window, on the toolbar, for the systems with no menu bar of
    /// their own: Windows and Linux (P3-44).
    pub fn set_menu_in_window(&mut self) {
        self.menu = MenuPlace::InWindow;
    }

    /// Chooses `path` as the game folder, read in place (`preview.choose_game`), and keeps it
    /// in the settings file when it plays, so the next start finds it (P3-44).
    pub fn choose_game_folder(&mut self, path: PathBuf) -> Option<GameFiles> {
        let settings = self.settings_file.clone();
        ProjectFeature::choose_game_folder(&mut self.commands(), settings.as_deref(), path)
    }

    /// The projects opened most recently, the latest first.
    #[must_use]
    pub fn recent_projects(&self) -> &[PathBuf] {
        self.features.project.recent_projects()
    }

    /// Asks for a project folder with the system's dialog, as Zed's Open does by default
    /// (`use_system_path_prompts`), and opens it.
    pub fn open_folder(&mut self) {
        let picked = rfd::FileDialog::new()
            .set_title(Text::OpenFolderTitle.get())
            .pick_folder();
        if let Some(folder) = picked {
            self.open_asked(&folder);
        }
    }

    /// The agent panel.
    #[must_use]
    pub const fn agent(&self) -> &AgentPanel {
        &self.features.agent.panel
    }

    /// The report of agent thread `index`, once it has ended.
    #[must_use]
    pub fn agent_report(&self, index: ThreadIndex) -> Option<&qs_agent::RunReport> {
        self.features.agent.panel.report(index)
    }

    /// Asks the agent's model for an inline edit of `selection` in the editor's file at `path`
    /// (P4-10); its answer is shown when [`Shell::ui`] takes it.
    ///
    /// # Errors
    ///
    /// No provider set up, or no model.
    fn start_inline(
        &mut self,
        path: &Path,
        selection: &str,
        instruction: &str,
    ) -> Result<(), String> {
        let file = self
            .features
            .editors
            .editor
            .as_ref()
            .map(|editor| editor.view.state().buffer().text())
            .unwrap_or_default();
        let ctx = self.ctx.clone();
        self.features
            .agent
            .start_inline((path, &file), selection, instruction, ctx)
    }

    /// Shows an inline edit's answer, or why there is none, in the editor.
    fn show_inline(&mut self, answer: Result<String, String>) {
        if let Some(editor) = self.features.editors.editor.as_mut() {
            match answer {
                Ok(text) => editor.propose(&text),
                Err(why) => editor.inline_failed(why),
            }
        }
    }

    /// Waits for an inline edit's answer and shows it. For tests.
    #[cfg(test)]
    pub(crate) fn wait_inline(&mut self) {
        if let Some(answer) = self.features.agent.wait_inline() {
            self.show_inline(answer);
        }
    }

    /// The agent panel, to drive its threads.
    pub const fn agent_mut(&mut self) -> &mut AgentPanel {
        &mut self.features.agent.panel
    }

    /// Keeps the agent's provider configuration in `file` (P4-07).
    pub fn set_agent_file(&mut self, file: PathBuf) {
        self.features.agent.panel.set_config_file(file);
    }

    /// Starts an agent thread on `task` over the open project (P4-09), each `@` mention it
    /// names attached (UI-8); why it could not.
    ///
    /// # Errors
    ///
    /// No project open, no provider set up, no model or no checkpoint.
    pub fn start_agent(&mut self, task: &str, ctx: &egui::Context) -> Result<(), String> {
        let settings = self.settings_file.clone();
        self.features
            .agent
            .start(&mut self.session, task, settings.as_deref(), ctx)
    }

    /// Starts an agent thread about `entity`, its state attached (Ask about this, UI-9); why
    /// it could not. Its field values are attached as they are, never read as mentions.
    ///
    /// # Errors
    ///
    /// As [`Self::start_agent`].
    pub fn ask_about(
        &mut self,
        entity: &qs_agent::EntityState,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let settings = self.settings_file.clone();
        self.features
            .agent
            .ask_about(&mut self.session, entity, settings.as_deref(), ctx)
    }

    /// Waits for the next thing agent thread `index`'s model sends and takes it; `false` when
    /// its model has ended. For tests.
    #[cfg(test)]
    pub(crate) fn wait_agent(&mut self, index: ThreadIndex) -> bool {
        self.features.agent.panel.wait(&mut self.session, index)
    }

    /// Does what the agent panel asked for.
    pub(crate) fn agent_action(&mut self, action: AgentAction, ctx: &egui::Context) {
        let settings = self.settings_file.clone();
        let mut commands = Commands::new(&mut self.session, &mut self.preview_error);
        self.features
            .agent
            .apply(action, &mut commands, settings.as_deref(), ctx);
    }

    /// Asks for a new folder with the system's dialog, makes a project of `template` there
    /// (`project.new`, P4-17) and opens it, saying why when it could not.
    pub fn new_project(&mut self, template: Template) {
        let mut commands = Commands::new(&mut self.session, &mut self.preview_error);
        self.features.project.new_project(&mut commands, template);
    }

    /// Exports the open project for players (`project.export`, P4-28); the status bar says
    /// where the zip is or why there is none.
    pub fn export_mod(&mut self) {
        let mut commands = Commands::new(&mut self.session, &mut self.preview_error);
        self.features.project.export(&mut commands);
    }

    /// What the last export did, for tests.
    #[cfg(test)]
    pub(crate) const fn export_note(&self) -> Option<&Result<PathBuf, String>> {
        self.features.project.export_note.as_ref()
    }

    /// Opens the project in `folder`, saying why when it does not open.
    fn open_asked(&mut self, folder: &Path) {
        let mut commands = Commands::new(&mut self.session, &mut self.preview_error);
        self.features.project.open(&mut commands, folder);
    }

    /// Tells the preview where the IDE's window is and whether it has the focus, through
    /// `preview.dock`, when that changed: the game's window follows it (ENG-9).
    fn dock(&mut self, ui: &Ui) {
        let frame = ui.input(|input| {
            let viewport = input.viewport();
            viewport.outer_rect.map(|outer| DockFrame {
                x: outer.left(),
                y: outer.top(),
                width: outer.width(),
                height: outer.height(),
                screen_width: viewport.monitor_size.map(|size| size.x),
                focused: viewport.focused.unwrap_or(true),
            })
        });
        let Some(frame) = frame else {
            return;
        };
        if self.dock_frame == Some(frame) {
            return;
        }
        self.dock_frame = Some(frame);
        let _: Option<PreviewStatus> = self.run(DOCK, &frame);
    }

    /// Puts the project just opened first in the recent projects.
    fn note_project(&mut self) {
        self.features.project.note_project(self.session.context());
    }

    /// Gives the shell the window's wgpu device and formats, which the map view draws with.
    pub fn set_render_state(&mut self, state: Option<RenderState>) {
        self.game.set_render_state(state.clone());
        self.render_state = state;
    }

    /// Follows where the layout puts the game (ADR-0028): the next Play starts it there, a
    /// game that runs in the other place is stopped, and a game started in the Game tab is
    /// shown.
    fn follow_game_place(&mut self, ctx: &egui::Context) {
        self.game.set_place(self.layout().game_place);
        self.game.follow(ctx, self.centre_tab);
        let preview = &self.session.context().preview;
        if preview.state() == PlayState::Stopped {
            self.game_shown = false;
            return;
        }
        let embedded = preview.is_embedded();
        // The place a Play would start it in now: the tab only with a device to draw on.
        if embedded != self.game.shows() {
            let _stopped: Option<serde_json::Value> = self.run(STOP, &serde_json::json!({}));
            return;
        }
        if embedded && !self.game_shown {
            self.game_shown = true;
            self.centre_tab = CentreTab::Game;
        }
    }

    /// Shows `mesh` in the map view from `camera`, and the view; false when the window has no
    /// wgpu device to draw it with.
    pub fn show_map(&mut self, mesh: &MapMesh, camera: Camera) -> bool {
        let Some(state) = &self.render_state else {
            return false;
        };
        map_view::install(state, mesh);
        self.features.map.map = Some(MapView::new(camera));
        self.centre_tab = CentreTab::Map;
        true
    }

    /// The tab shown in the centre.
    #[must_use]
    pub const fn centre_tab(&self) -> CentreTab {
        self.centre_tab
    }

    /// The map view, once a map is shown.
    #[must_use]
    pub const fn map_view(&self) -> Option<&MapView> {
        self.features.map.map.as_ref()
    }

    /// Opens the project in `path`, through its command.
    ///
    /// # Errors
    ///
    /// [`ShellError`] when the command fails.
    pub fn open_project(&mut self, path: &Path) -> Result<(), ShellError> {
        ProjectFeature::open_project(&mut self.commands(), path)
    }

    /// Opens what macOS or the command line hands the app, through `paths.open`: folders as
    /// projects and files in their projects (PRJ-1.6). The next frame shows them.
    pub fn open_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let params = OpenPathsParams {
            paths: paths.into_iter().collect(),
        };
        let _: Option<PathsOpened> = self.run(OPEN_PATHS, &params);
    }

    /// Shows what `paths.open` opened since the last frame, from this window or another launch
    /// of the app over its socket: the editor for a file (the map editor shows a map by
    /// itself), and the window comes to the front, as Zed's does, when anything opened; why a
    /// path did not open goes on the toolbar.
    fn follow_opened_paths(&mut self, ctx: &egui::Context) {
        let taken: Option<Option<PathsOpened>> =
            self.run(TAKE_OPENED_PATHS, &serde_json::json!({}));
        let Some(Some(opened)) = taken else {
            return;
        };
        for not_opened in &opened.not_opened {
            tracing::warn!(path = %not_opened.path.display(), why = ?not_opened.why, "not opened");
        }
        // The toolbar says why a path did not open, unsaved changes in particular.
        if let Some(first) = opened.not_opened.first() {
            self.preview_error = Some(first.detail.clone());
        }
        if opened.file.is_some() && opened.map.is_none() {
            self.centre_tab = CentreTab::Editor;
        }
        if opened.opened > 0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    /// Opens the file a panel's row asks for, through its command, and shows the editor; a
    /// `.map` or `.bsp` opens in the map editor (P5-06).
    fn jump(&mut self, jump: Jump) {
        if opens_in_map_editor(&jump.path) {
            self.with_map(|map, commands| map.open_map(commands, jump.path));
            return;
        }
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

    /// Carries out the editor action the layout last asked for, once (P4-32).
    fn editor_request(&mut self) {
        let layout = *self.layout();
        if layout.editor_requests == self.features.editors.editor_seen {
            return;
        }
        self.features.editors.editor_seen = layout.editor_requests;
        let Some(request) = layout.editor_request else {
            return;
        };
        match request {
            EditorRequest::AskLine => {
                if let Some(info) = self.session.registry().get(GO_TO_LINE) {
                    self.palette.ask(info);
                }
                return;
            }
            EditorRequest::GoToSymbol => return self.open_symbol_picker(),
            EditorRequest::TabSwitcher => return self.open_tab_switcher(),
            EditorRequest::ProjectSearch => {
                self.features.editors.multibuffer.source = MultiSource::Search;
                self.features.editors.multibuffer.form.focus = true;
                self.centre_tab = CentreTab::Search;
                return;
            }
            EditorRequest::Fold
            | EditorRequest::Unfold
            | EditorRequest::FoldAll
            | EditorRequest::UnfoldAll
            | EditorRequest::GoToLine(_) => {}
        }
        let Some(editor) = self.features.editors.editor.as_mut() else {
            return;
        };
        let line = editor.view.state().primary().head.line;
        let folds = editor.view.folds_mut();
        match request {
            EditorRequest::Fold => {
                folds.fold_at(line);
            }
            EditorRequest::Unfold => {
                folds.unfold_at(line);
            }
            EditorRequest::FoldAll => folds.fold_all(),
            EditorRequest::UnfoldAll => folds.unfold_all(),
            EditorRequest::GoToLine(to) => editor.go_to_line(line_index(to)),
            EditorRequest::AskLine
            | EditorRequest::GoToSymbol
            | EditorRequest::TabSwitcher
            | EditorRequest::ProjectSearch => {}
        }
        // A fold over the caret puts it on the fold's first line, as VS Code does, so the
        // fold is not undone by the caret in it.
        let folds = editor.view.folds();
        if folds.is_hidden(line) {
            let shown = folds.shown_at_or_before(line);
            editor
                .view
                .state_mut()
                .set_selections(&[Selection::caret(Position::new(shown, 0))]);
        }
    }

    /// Opens the picker of the open file's symbols, the one the caret is in chosen.
    fn open_symbol_picker(&mut self) {
        let Some(open) = self.session.context().workbench.open() else {
            return;
        };
        let caret = self
            .features
            .editors
            .editor
            .as_ref()
            .map_or(0, Editor::caret_offset);
        let buffer = self
            .features
            .editors
            .editor
            .as_ref()
            .map(|editor| editor.view.state().buffer());
        let mut rows = Vec::new();
        let mut places = Vec::new();
        let mut selected = 0;
        for (index, symbol) in open.outline().iter().enumerate() {
            let start = symbol.range.span.start;
            let line = buffer
                .map_or(0, |b| b.position(start).line)
                .saturating_add(1);
            rows.push(PickerRow {
                name: symbol.name.clone(),
                detail: line.to_string(),
            });
            places.push(Jump {
                path: symbol.range.path.clone(),
                offset: start,
            });
            if start <= caret {
                selected = index;
            }
        }
        self.palette.close();
        self.finder.close();
        self.features
            .editors
            .picker
            .0
            .open(PickerKind::Symbols, rows, selected);
        self.features.editors.picker.1 = places;
    }

    /// Opens the tab switcher: the open files, the one used most recently first and the one
    /// before it chosen, as Ctrl+Tab goes back to it.
    fn open_tab_switcher(&mut self) {
        let workbench = &self.session.context().workbench;
        let root = workbench.root();
        let mut rows = Vec::new();
        let mut places = Vec::new();
        for path in workbench.history() {
            let offset = self
                .features
                .editors
                .parked
                .get(path)
                .or(self
                    .features
                    .editors
                    .editor
                    .as_ref()
                    .filter(|e| e.path == *path))
                .map_or(0, Editor::caret_offset);
            rows.push(PickerRow {
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                detail: root
                    .and_then(|root| path.strip_prefix(root).ok())
                    .unwrap_or(path)
                    .to_string_lossy()
                    .into_owned(),
            });
            places.push(Jump {
                path: path.clone(),
                offset,
            });
        }
        self.palette.close();
        self.finder.close();
        self.features
            .editors
            .picker
            .0
            .open(PickerKind::Tabs, rows, 1);
        self.features.editors.picker.1 = places;
    }

    /// The editor's second pane (P4-32), while the layout splits the editor: made from the open
    /// file when the split starts, its text kept as the first pane's while both show one file;
    /// a click in it makes it the pane being edited. Draws it and gives what it asked for.
    fn second_pane(&mut self, ui: &mut Ui) {
        let split = self.layout().split;
        if split == Split::None || self.centre_tab != CentreTab::Editor {
            self.features.editors.second = None;
            return;
        }
        let Some(editor) = &self.features.editors.editor else {
            return;
        };
        let theme = &self.theme;
        let second = match &mut self.features.editors.second {
            Some(second) => second,
            None => match self.session.context().workbench.open() {
                Some(open) => self.features.editors.second.insert(Editor::new(
                    Id::new(Area::SecondPane),
                    open,
                    theme,
                )),
                None => return,
            },
        };
        let focused = ui.memory(|m| m.has_focus(second.view.id()));
        if second.path == editor.path && !focused {
            let text = editor.view.state().buffer().text();
            if second.view.state().buffer().text() != text {
                second.view.state_mut().replace_text(&text);
            }
        }
        let wrap = editor.view.wrap();
        second.view.set_wrap(wrap);
        let style = theme.view_style();
        let panel = match split {
            Split::Down => egui::Panel::bottom(Id::new(Area::SecondPane))
                .exact_size((ui.available_height() / 2.0).floor()),
            Split::Right | Split::None => egui::Panel::right(Id::new(Area::SecondPane))
                .exact_size((ui.available_width() / 2.0).floor()),
        };
        panel
            .frame(egui::Frame::NONE.fill(theme.color(Token::SurfaceEditor)))
            .show(ui, |ui| {
                name_area(ui, accesskit::Role::Pane, Text::SecondPane);
                // Its hovers and completions are the pane being edited's to ask for.
                let _ = second.show(ui, theme, &style);
            });
        if ui.memory(|m| m.has_focus(second.view.id())) {
            self.swap_panes(ui.ctx());
        }
    }

    /// Makes the second pane the one being edited and the first the second: its file is opened
    /// in the workbench, after the first pane's edits are sent.
    fn swap_panes(&mut self, ctx: &egui::Context) {
        let (Some(mut first), Some(mut second)) = (
            self.features.editors.editor.take(),
            self.features.editors.second.take(),
        ) else {
            return;
        };
        if let Some(text) = first.unsent() {
            let path = first.path.clone();
            let _: Option<()> = self.run(UPDATE_FILE, &UpdateFileParams { path, text });
        }
        first.view.set_id(Id::new(Area::SecondPane));
        second.view.set_id(Id::new(EDITOR_AREA));
        if first.path == second.path {
            self.features.editors.editor = Some(second);
            self.features.editors.second = Some(first);
        } else {
            let (from, to) = (first.path.clone(), second.path.clone());
            let offset = second.caret_offset();
            self.features.editors.editor = Some(first);
            self.features.editors.parked.insert(to.clone(), second);
            self.jump(Jump { path: to, offset });
            self.sync_editor();
            self.features.editors.second = self.features.editors.parked.remove(&from);
        }
        ctx.memory_mut(|m| m.request_focus(Id::new(EDITOR_AREA)));
    }

    /// The outline panel beside the editor (P4-32): the open file's symbols, the one the caret
    /// is in marked; a click goes to one.
    fn outline_panel(&mut self, ui: &mut Ui) {
        if self.layout().outline != Outline::Shown || self.centre_tab != CentreTab::Editor {
            return;
        }
        let Some(editor) = &self.features.editors.editor else {
            return;
        };
        let Some(open) = self.session.context().workbench.open() else {
            return;
        };
        let theme = &self.theme;
        let caret = editor.caret_offset();
        let current = open
            .outline()
            .iter()
            .rposition(|symbol| symbol.range.span.start <= caret);
        let mut chosen = None;
        egui::Panel::right(Id::new(Area::Outline))
            .default_size(OUTLINE_WIDTH)
            .frame(
                egui::Frame::NONE
                    .fill(theme.color(Token::SurfacePanel))
                    .inner_margin(theme.space(1)),
            )
            .show(ui, |ui| {
                name_area(ui, accesskit::Role::Pane, Text::Outline);
                heading_with_help(ui, theme, Text::Outline, Topic::Outline);
                if open.outline().is_empty() {
                    note(ui, theme, Text::NoOutline);
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (index, symbol) in open.outline().iter().enumerate() {
                        let row = ui.selectable_label(current == Some(index), &symbol.name);
                        if row.clicked() {
                            chosen = Some(Jump {
                                path: symbol.range.path.clone(),
                                offset: symbol.range.span.start,
                            });
                        }
                    }
                });
            });
        if let Some(jump) = chosen {
            self.jump(jump);
        }
    }

    /// Opens the command palette with the open project's tasks (P4-36).
    pub fn open_palette(&mut self) {
        let tasks = if self.session.context().workbench.root().is_some() {
            self.run::<_, Vec<TaskInfo>>(TASK_LIST, &serde_json::json!({}))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        self.features.editors.picker.0.close();
        self.palette.open(tasks);
    }

    /// Reads the editor's inlay hints again when its file's analysis changed (P4-34): each
    /// parameter's name before the argument passed as it.
    fn refresh_inlays(&mut self) {
        self.with_editors(|editors, commands| editors.refresh_inlays(commands));
    }

    /// Marks in the editor's gutter its file's changes against the text at `HEAD` (P4-35);
    /// none outside a repository or for a file `HEAD` does not have.
    fn mark_changes(&mut self) {
        self.with_editors(|editors, commands| editors.mark_changes(commands));
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
    pub(crate) fn save_if_changed(&mut self, pointer: PointerHeld) {
        let layout = self.layout().as_written();
        if pointer == PointerHeld::Down || layout == self.saved {
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
        self.follow_zoom(ui.ctx());
        // P4-09: what the agent's models sent since the last frame; their calls run here.
        if self.features.agent.panel.pump(&mut self.session) {
            ui.ctx().request_repaint();
        }
        // A test drives the models by hand, outside a frame: what they named is taken here.
        if let Some(request) = self.features.agent.after(self.session.context(), ui.ctx()) {
            self.request(request);
        }
        self.ctx = Some(ui.ctx().clone());
        if let Some(answer) = self.features.agent.take_inline() {
            self.show_inline(answer);
            ui.ctx().request_repaint();
        }
        // AGT-7: commands sent over the socket run first, so the frame draws what they did.
        if let Some(host) = &self.host
            && host.answer(&mut self.session) > 0
        {
            ui.ctx().request_repaint();
        }
        self.follow_opened_paths(ui.ctx());
        if self.session.context().preview.state() != PlayState::Stopped {
            let polled: Option<PreviewStatus> = self.run(POLL, &serde_json::json!({}));
            // BRIDGE-3.5: a click in the paused game picked an edict; open its spawn function.
            if let Some(classname) = polled.and_then(|status| status.picked) {
                self.entity_action(EntityAction::Open(classname), ui.ctx());
            }
        }
        self.follow_stop();
        self.follow_game_place(ui.ctx());
        // P3-35: the background check's results for the latest edit, once it has finished.
        if self.run::<_, bool>(TAKE_CHECKED, &serde_json::json!({})) == Some(true) {
            ui.ctx().request_repaint();
        }
        self.note_project();
        self.dock(ui);
        if self.session.context().preview.state() != PlayState::Stopped {
            // The host's events are read each frame; a running game asks for frames.
            ui.ctx().request_repaint_after(PREVIEW_POLL);
        }
        let ctx = ui.ctx().clone();
        let mut commands = Commands::new(&mut self.session, &mut self.preview_error);
        if let Some(request) = self.features.terminal.poll(&mut commands, &ctx) {
            self.request(request);
        }
        let shown = self.git_shown();
        let mut commands = Commands::new(&mut self.session, &mut self.preview_error);
        self.features.git.refresh(&mut commands, shown);
        self.keys(ui);
        let pending = self.pending_focus();
        self.toolbar(ui);
        self.reset_notice(ui);
        self.game_guide(ui);
        match self.status_bar(ui) {
            Some(StatusAsk::Show(panel)) => {
                let params = ShowPanelParams {
                    panel,
                    when_shown: WhenShown::Hide,
                };
                let _: Option<Docks> = self.run(qs_layout::SHOW_PANEL, &params);
            }
            Some(StatusAsk::Move(panel, dock)) => {
                let params = MovePanelParams { panel, dock };
                let _: Option<Docks> = self.run(qs_layout::MOVE_PANEL, &params);
            }
            None => {}
        }
        let entered = self.command_line.show(
            ui,
            &self.theme,
            self.session.registry(),
            FocusNow::of(pending, Focus::CommandLine),
        );
        if let Some((id, params)) = entered {
            match self.session.execute(id.as_str(), params) {
                Ok(_) => self.command_line.ran(id),
                Err(error) => self.command_line.failed(error.to_string()),
            }
        }
        for panel in [Panel::Left, Panel::Right, Panel::Bottom] {
            self.panel(ui, panel, FocusNow::of(pending, Focus::Panel(panel)));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(self.theme.color(Token::SurfaceEditor)))
            .show(ui, |ui| {
                name_area(ui, accesskit::Role::Pane, Text::Centre);
                self.centre(ui, FocusNow::of(pending, Focus::Editor));
            });
        if let Some((id, params)) = self.palette.show(
            ui,
            Id::new(Area::Palette),
            &self.theme,
            self.session.registry(),
            &self.keymap,
        ) {
            match self.session.execute(id.as_str(), params) {
                Ok(_) => self.palette.close(),
                Err(error) => self.palette.failed(error.to_string()),
            }
        }
        let workbench = &self.session.context().workbench;
        // The files opened most recently first, then the rest in the tree's order.
        let mut files = workbench.history().to_vec();
        for entry in workbench.tree() {
            if let EntryKind::File(path) = &entry.kind
                && !files.contains(path)
            {
                files.push(path.clone());
            }
        }
        let open = workbench.open().map(|open| open.path().to_path_buf());
        let chosen = self.finder.show(
            ui,
            Id::new(Area::Finder),
            &self.theme,
            (workbench.root(), open.as_deref()),
            &files,
        );
        match chosen {
            Some((path, Some(line))) => {
                self.features.editors.pending_line = Some((path.clone(), line));
                self.jump(Jump { path, offset: 0 });
            }
            Some((path, None)) => {
                let offset = self
                    .features
                    .editors
                    .parked
                    .get(&path)
                    .map_or(0, Editor::caret_offset);
                self.jump(Jump { path, offset });
            }
            None => {}
        }
        let (picker, places) = &mut self.features.editors.picker;
        if let Some(index) = picker.show(ui, Id::new(Area::Picker), &self.theme)
            && let Some(place) = places.get(index).cloned()
        {
            self.jump(place);
        }
        let pointer = if ui.input(|input| input.pointer.any_down()) {
            PointerHeld::Down
        } else {
            PointerHeld::Up
        };
        self.save_if_changed(pointer);
    }

    /// The shortcuts pressed this frame: the palette's, and while it is closed each binding's
    /// command (`keymap`), the editor's own keys left to it while it has the keyboard.
    fn keys(&mut self, ui: &Ui) {
        if ui.input_mut(|i| i.consume_shortcut(&keymap::OPEN_PALETTE)) {
            self.finder.close();
            self.open_palette();
            return;
        }
        if ui.input_mut(|i| i.consume_shortcut(&keymap::OPEN_FOLDER)) {
            self.open_folder();
            return;
        }
        if ui.input_mut(|i| i.consume_shortcut(&keymap::OPEN_FILE_FINDER)) {
            self.palette.close();
            self.features.editors.picker.0.close();
            self.finder.open();
            return;
        }
        if self.palette.is_open() || self.finder.is_open() {
            return;
        }
        // The editor, with its own undo history, takes its keys itself.
        let editing = ui.memory(|m| m.has_focus(Id::new(EDITOR_AREA)));
        // P4-37: an edit of the keymap file takes effect on the next frame.
        let registry = self.session.registry();
        let bindable = |id: &str| keymap::bindable(registry, id);
        self.keymap.reload_if_changed(&bindable);
        let map_shown =
            self.centre_tab == CentreTab::Map && self.session.context().workbench.map().is_some();
        for binding in self.keymap.in_match_order() {
            let taken = match binding.scope {
                Scope::Everywhere => true,
                Scope::Shell => !editing,
                Scope::Map => !editing && map_shown,
            };
            if !taken {
                continue;
            }
            if !ui.input_mut(|i| i.consume_shortcut(&binding.shortcut)) {
                continue;
            }
            // A profile run by its key shows its notice and reads the portals again, as the
            // button's does.
            if binding.command == MAP_RUN_PROFILE {
                self.with_map(|map, commands| {
                    map.map_actions(commands, vec![MapAction::Profile(ProfileAction::Run)]);
                });
                continue;
            }
            if let Err(error) = self.session.execute(
                &binding.command,
                serde_json::Value::Object(serde_json::Map::new()),
            ) {
                tracing::warn!(%error, command = %binding.command, "shortcut failed");
            }
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let theme = &self.theme;
        let fill = theme.color(Token::SurfaceToolbar);
        let mut toggled = None;
        let mut open_palette = false;
        let mut open_guide = false;
        let mut export_asked = false;
        let mut control = None;
        let preview = &self.session.context().preview;
        let state = preview.state();
        let docked = if preview.is_embedded() {
            Text::PreviewInTab
        } else if preview.docks() {
            Text::PreviewDocked
        } else {
            Text::PreviewOwnWindow
        };
        let failed = self.preview_error.is_some();
        let status = self
            .preview_error
            .clone()
            .or_else(|| preview.status().map(str::to_owned));
        egui::Panel::top(Id::new(Area::Toolbar))
            .exact_size(theme.size.toolbar)
            .frame(egui::Frame::NONE.fill(fill).inner_margin(theme.space(1)))
            .show(ui, |ui| {
                name_area(ui, accesskit::Role::Toolbar, Text::Toolbar);
                ui.horizontal_centered(|ui| {
                    control = play_controls(ui, theme, state);
                    help::button(ui, theme, Topic::Toolbar);
                    ui.separator();
                    ui.label(Text::PreviewHost.get());
                    ui.label(
                        RichText::new(docked.get())
                            .small()
                            .color(theme.color(Token::TextMuted)),
                    );
                    if let Some(status) = &status {
                        let colour = if failed {
                            theme.color(Token::StatusError)
                        } else {
                            theme.color(Token::StatusOk)
                        };
                        status_dot(ui, colour);
                        ui.label(RichText::new(status.as_str()).color(colour));
                    }
                    ui.with_layout(EguiLayout::right_to_left(Align::Center), |ui| {
                        for (panel, icon, text) in [
                            (Panel::Right, Icon::PanelRight, Text::ToggleRightPanel),
                            (Panel::Bottom, Icon::PanelBottom, Text::ToggleBottomPanel),
                            (Panel::Left, Icon::PanelLeft, Text::ToggleLeftPanel),
                        ] {
                            let visible = self.session.context().layout.panel(panel).visible;
                            let button = Button::new(icon.char().to_string())
                                .frame(false)
                                .selected(visible);
                            let response = ui.add(button).on_hover_text(text.get());
                            // The icon is the button's text; its name is the tooltip's. The
                            // button said whether it is selected.
                            name_field(ui, &response, text);
                            if response.clicked() {
                                toggled = Some(panel);
                            }
                        }
                        if self.menu == MenuPlace::InWindow {
                            let files = Button::new(
                                RichText::new(Text::GameFilesMenu.get())
                                    .color(theme.color(Token::TextMuted)),
                            );
                            if ui.add(files).clicked() {
                                open_guide = true;
                            }
                            let export = Button::new(
                                RichText::new(Text::ExportModMenu.get())
                                    .color(theme.color(Token::TextMuted)),
                            );
                            let has_project = self.session.context().workbench.root().is_some();
                            if ui.add_enabled(has_project, export).clicked() {
                                export_asked = true;
                            }
                        }
                        let search = Button::new(
                            RichText::new(labelled(Icon::Search, Text::SearchCommands))
                                .color(theme.color(Token::TextMuted)),
                        );
                        let shortcut = ui.ctx().format_shortcut(&keymap::OPEN_PALETTE);
                        let response = ui.add(search).on_hover_text(shortcut);
                        name_field(ui, &response, Text::SearchCommands);
                        if response.clicked() {
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
            self.open_palette();
        }
        if open_guide {
            self.show_game_guide();
        }
        if export_asked {
            self.export_mod();
        }
        if let Some(id) = control {
            self.control(id);
        }
    }

    /// Runs the play-loop command `id` from the toolbar, keeping its error for the toolbar to
    /// show until the next command.
    fn control(&mut self, id: &str) {
        let result = if id == PLAY {
            serde_json::to_value(PlayParams::default())
                .map_err(ShellError::from)
                .and_then(|params| Ok(self.session.execute(id, params)?))
        } else {
            self.session
                .execute(id, serde_json::json!({}))
                .map_err(ShellError::from)
        };
        let failed = result.is_err();
        self.preview_error = result.err().map(|error| error.to_string());
        if failed && id == PLAY {
            self.open_guide_to_missing_files();
        }
    }

    /// P3-21: Play that failed for want of the game opens the guide to its files.
    fn open_guide_to_missing_files(&mut self) {
        let mut commands = Commands::new(&mut self.session, &mut self.preview_error);
        self.features
            .project
            .open_guide_to_missing_files(&mut commands);
    }

    /// Opens the guide to the game's files (P3-21), from the File menu.
    pub fn show_game_guide(&mut self) {
        let mut commands = Commands::new(&mut self.session, &mut self.preview_error);
        self.features.project.show_game_guide(&mut commands);
    }

    /// The guide to the game's files while it is shown (P3-21).
    fn game_guide(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let mut commands = Commands::new(&mut self.session, &mut self.preview_error);
        let settings = self.settings_file.as_deref();
        self.features
            .project
            .game_guide(&mut commands, settings, (&ctx, &self.theme));
    }

    /// Draws the map editor in the Map tab when a map is open, running what it asks for;
    /// whether it did.
    fn map_tab(&mut self, ui: &mut Ui) -> bool {
        let settings = *self.layout();
        let Some(document) = self.session.context().workbench.map() else {
            return false;
        };
        if self.centre_tab != CentreTab::Map {
            return false;
        }
        let editor = &mut self.features.map.map_editor;
        let theme = &self.theme;
        let state = self.render_state.as_ref();
        // The map editor keeps a least height for its view and browser; when the centre is
        // shorter, as with large text (P7-12), the tab scrolls rather than drawing under the
        // bottom panel. Only its scroll bar scrolls it: the wheel and drags are the view's.
        let height = ui.available_height();
        let actions = egui::ScrollArea::vertical()
            .id_salt(Area::MapTab)
            .scroll_source(egui::scroll_area::ScrollSource {
                scroll_bar: true,
                drag: egui::scroll_area::DragScroll::Never,
                mouse_wheel: false,
            })
            .show(ui, |ui| {
                let size = egui::vec2(ui.available_width(), height);
                ui.allocate_ui(size, |ui| {
                    editor.ui(ui, theme, document, (&settings, state))
                })
                .inner
            })
            .inner;
        self.with_map(|map, commands| map.map_actions(commands, actions));
        if let Some(text) = self.features.map.take_copied() {
            ui.ctx().copy_text(text);
        }
        self.with_map(MapFeature::refresh_map_outline);
        if self.with_map(MapFeature::refresh_map_review) {
            // The Properties panel, drawn before the map this frame, shows them on the next.
            ui.ctx().request_repaint();
        }
        if let Some(document) = self.session.context().workbench.map()
            && let Some(show) = self.features.map.map_editor.filter_stale(document.edits())
            && let Err(error) = self.with_map(|map, commands| map.filter_map(commands, show))
        {
            tracing::warn!(%error, "the map filter was not found again");
            self.features
                .map
                .map_editor
                .set_refused(Some(error.to_string()));
        }
        if let Some(document) = self.session.context().workbench.map()
            && self.features.map.map_editor.keys_stale(document.revision())
        {
            self.with_map(MapFeature::refresh_map_keys);
            // The Properties panel, drawn before the map this frame, shows them on the next.
            ui.ctx().request_repaint();
        }
        true
    }

    /// Runs what the outliner or the Properties panel asked for: a selection or a field's new
    /// value through its command, or the definition of a function opened.
    fn entity_action(&mut self, action: EntityAction, ctx: &egui::Context) {
        let result = match action {
            EntityAction::Ask(edict) => {
                // UI-9: the thread starts with the entity's state as the game has it now.
                if self
                    .run::<_, SelectedEntity>(SELECT, &SelectParams { edict })
                    .is_none()
                {
                    return;
                }
                if let Some(entity) = self.session.context().live().entity {
                    let started = self.ask_about(&entity, ctx);
                    self.features.agent.panel.set_error(started.err());
                }
                return;
            }
            EntityAction::Select(edict) => {
                self.run::<_, SelectedEntity>(SELECT, &SelectParams { edict })
            }
            EntityAction::SetField { field, value } => {
                self.run::<_, SelectedEntity>(SET_FIELD, &SetFieldParams { field, value })
            }
            EntityAction::Open(name) => {
                let place = self
                    .session
                    .context()
                    .workbench
                    .project_symbols()
                    .iter()
                    .find(|symbol| symbol.name.eq_ignore_ascii_case(&name))
                    .map(|symbol| Jump {
                        path: symbol.range.path.clone(),
                        offset: symbol.range.span.start,
                    });
                if let Some(place) = place {
                    self.jump(place);
                }
                return;
            }
        };
        if result.is_none() {
            self.preview_error = Some(Text::EditFailed.get().to_owned());
        }
    }

    /// Opens `file`, relative to the project's root, at `line`, from 1.
    fn show_line(&mut self, file: &Path, line: u32) {
        self.with_editors(|editors, commands| editors.show_line(commands, file, line));
    }

    /// Takes the editor to where the game stopped, once for each stop.
    fn follow_stop(&mut self) {
        self.with_editors(|editors, commands| editors.follow_stop(commands));
    }

    /// Reloads the running game after a save, its error shown on the toolbar.
    fn reload_after_save(&mut self) {
        self.with_editors(|editors, commands| editors.reload_after_save(commands));
    }

    /// Below the toolbar while a reload waits (P3-10, design doc "State honesty"): what it
    /// would reset, and the choice of reloading all the same or restarting the map on the new
    /// program.
    fn reset_notice(&mut self, ui: &mut Ui) {
        let lost = self.session.context().preview.would_reset();
        if lost.is_empty() {
            return;
        }
        let theme = &self.theme;
        let warn = theme.color(Token::StatusWarn);
        let list = lost.join(", ");
        let mut chosen = None;
        egui::Panel::top(Id::new(Area::ResetNotice))
            .frame(
                egui::Frame::NONE
                    .fill(theme.color(Token::SurfaceToolbar))
                    .inner_margin(theme.space(1)),
            )
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(labelled(Icon::Warning, Text::WouldReset)).color(warn));
                    ui.label(list);
                    if ui.button(Text::ReloadAnyway.get()).clicked() {
                        chosen = Some(false);
                    }
                    if ui
                        .button(labelled(Icon::Restart, Text::RestartMap))
                        .clicked()
                    {
                        chosen = Some(true);
                    }
                });
            });
        let Some(restart) = chosen else {
            return;
        };
        let reloaded: Option<PreviewStatus> = self.run(RELOAD, &ReloadParams::default());
        if reloaded.is_none() {
            self.preview_error = Some(Text::ReloadFailed.get().to_owned());
        } else if restart {
            let _: Option<PreviewStatus> = self.run(RESTART, &serde_json::json!({}));
        }
    }

    /// The area a focus command named since the last frame, if one ran (QA-9): the command
    /// is counted, not compared, so naming the panel the keyboard left by a click still
    /// moves the keyboard back.
    fn pending_focus(&mut self) -> Option<Focus> {
        let layout = *self.layout();
        if self.focus_applied == layout.focus_requests {
            return None;
        }
        self.focus_applied = layout.focus_requests;
        Some(layout.focus)
    }

    /// The status bar: the panels' buttons, as Zed's, and the problems and the game's state;
    /// what a click on a panel's button, or its menu, asked for.
    fn status_bar(&self, ui: &mut Ui) -> Option<StatusAsk> {
        let theme = &self.theme;
        let mut asked = None;
        egui::Panel::bottom(Id::new(Area::StatusBar))
            .exact_size(theme.size.status_bar)
            .frame(
                egui::Frame::NONE
                    .fill(theme.color(Token::SurfaceChrome))
                    .inner_margin(egui::Margin::symmetric(8, 0)),
            )
            .show(ui, |ui| {
                name_area(ui, accesskit::Role::Status, Text::StatusBar);
                ui.horizontal_centered(|ui| {
                    // Zed's order: the left dock's panels first, the right and bottom docks'
                    // at the far end.
                    asked = self.panel_buttons(ui, Panel::Left);
                    ui.separator();
                    let muted = theme.color(Token::TextMuted);
                    let (errors, warnings) =
                        project::counts(self.session.context().workbench.problems());
                    for (icon, value, name) in [
                        (Icon::Error, errors, Text::ErrorCount),
                        (Icon::Warning, warnings, Text::WarningCount),
                    ] {
                        let response = ui
                            .label(RichText::new(count(icon, value)).small().color(muted))
                            .on_hover_text(name.get());
                        // Read as its name and number, not the icon's character.
                        ui.ctx().accesskit_node_builder(response.id, |node| {
                            node.set_label(format!("{} {value}", name.get()));
                        });
                    }
                    let running = match self.session.context().preview.state() {
                        PlayState::Stopped => Text::PreviewStopped,
                        PlayState::Playing => Text::PreviewRunning,
                        PlayState::Paused => Text::PreviewPaused,
                    };
                    ui.label(RichText::new(running.get()).small().color(muted));
                    match &self.features.project.export_note {
                        Some(Ok(path)) => {
                            let text = format!("{} {}", Text::ExportedTo.get(), path.display());
                            ui.label(RichText::new(text).small().color(muted));
                        }
                        Some(Err(why)) => {
                            let error = theme.color(Token::StatusError);
                            ui.label(RichText::new(why).small().color(error));
                        }
                        None => {}
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        for dock in [Panel::Right, Panel::Bottom] {
                            asked = asked.or(self.panel_buttons(ui, dock));
                        }
                    });
                });
            });
        asked
    }

    /// The status-bar buttons of `dock`'s panels, each on while its panel shows; a click shows
    /// the panel or hides the dock, and its menu moves it to another dock (P7-15).
    fn panel_buttons(&self, ui: &mut Ui, dock: Panel) -> Option<StatusAsk> {
        let layout = self.layout();
        let docks = layout.docks;
        let open = layout.panel(dock).visible;
        let mut asked = None;
        for panel in docks.panels(dock) {
            let (title, _) = panel_topic(panel);
            let state = if open && docks.shown(dock) == Some(panel) {
                Toggled::On
            } else {
                Toggled::Off
            };
            let name = panel_button(title.get());
            let response =
                panel_icon(panel).toggle_named(ui, state, &name, Text::PanelButtonHint.get());
            if response.clicked() {
                asked = Some(StatusAsk::Show(panel));
            }
            response.context_menu(|ui| {
                for (to, text) in [
                    (Panel::Left, Text::MoveToLeftDock),
                    (Panel::Right, Text::MoveToRightDock),
                    (Panel::Bottom, Text::MoveToBottomDock),
                ] {
                    if to != dock && ui.button(text.get()).clicked() {
                        asked = Some(StatusAsk::Move(panel, to));
                        ui.close();
                    }
                }
            });
        }
        asked
    }

    /// Draws `panel`, giving it the keyboard when `focus`: its first heading, or the bottom
    /// panel's selected tab, takes it.
    fn panel(&mut self, ui: &mut Ui, panel: Panel, focus: FocusNow) {
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
        let scope = &mut self.symbol_scope;
        let workspace = self.session.context();
        let map_shown = if self.centre_tab == CentreTab::Map {
            MapShown::Shown
        } else {
            MapShown::Hidden
        };
        let features = &mut self.features;
        let mut actions = Vec::new();
        let mut expanded = state.visible;
        // A size the layout was given by a command, not by a drag of the panel's edge, is
        // drawn: egui keeps the size it last drew and reads `default_size` only the first time.
        let drawn_size =
            PanelState::load(ui.ctx(), Id::new(Area::Panel(panel))).map(|stored| match panel {
                Panel::Left | Panel::Right => stored.size().x,
                Panel::Bottom => stored.size().y,
            });
        let held = ui.input(|input| input.pointer.any_down());
        // With the interface zoomed past 100% (P7-12) a panel takes no more than its share of
        // the window, so the centre keeps its room; the layout keeps the size asked for, drawn
        // again at 100%, where nothing caps it.
        let window = ui.ctx().content_rect().size();
        let limit = if ui.ctx().zoom_factor() > Zoom::ACTUAL.get() {
            match panel {
                Panel::Left | Panel::Right => window.x * SIDE_PANEL_SHARE,
                Panel::Bottom => window.y * BOTTOM_PANEL_SHARE,
            }
            .max(qs_layout::MIN_PANEL_SIZE)
        } else {
            f32::INFINITY
        };
        let target = state.size.min(limit);
        let commanded = commanded_size(drawn_size, target, held);
        // `exact_size` pins the size range to one point, so it comes after `min_size`, which
        // would widen it again.
        let builder = builder
            .resizable(true)
            .min_size(qs_layout::MIN_PANEL_SIZE)
            .max_size(limit);
        let builder = if commanded {
            builder.exact_size(target)
        } else {
            builder.default_size(target)
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
                let name = match panel {
                    Panel::Left => Text::LeftPanel,
                    Panel::Right => Text::RightPanel,
                    Panel::Bottom => Text::BottomPanel,
                };
                name_area(ui, accesskit::Role::Pane, name);
                let dock = Dock {
                    workspace,
                    scope,
                    map_shown,
                };
                actions = match panel {
                    // The right panel's sections run as long and as wide as their contents, so
                    // it scrolls when they pass its edges, as they do with large text (P7-12),
                    // rather than pushing past the panel. The left panel's lists scroll in
                    // their own shares of it.
                    Panel::Right => {
                        egui::ScrollArea::both()
                            .id_salt(Area::Panel(panel))
                            .auto_shrink([false, false])
                            .show(ui, |ui| features.region_ui(ui, theme, panel, dock, focus))
                            .inner
                    }
                    // Contents wider or taller than the panel, a long name in the outline,
                    // are cut at its edge: laid out in a child of exactly the panel's room, so
                    // egui never stores them as the panel's size, which grew the panel on one
                    // frame and set it back on the next.
                    Panel::Left | Panel::Bottom => {
                        let room = ui.available_rect_before_wrap();
                        let mut inside = ui.new_child(egui::UiBuilder::new().max_rect(room));
                        inside.shrink_clip_rect(room);
                        let asked = features.region_ui(&mut inside, theme, panel, dock, focus);
                        ui.advance_cursor_after_rect(room);
                        asked
                    }
                };
            });
        for action in actions {
            self.apply(action, ui.ctx());
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
        self.commands().run(id, params)
    }

    /// The commands the features run, and the toolbar's notice.
    const fn commands(&mut self) -> Commands<'_> {
        Commands::new(&mut self.session, &mut self.preview_error)
    }

    /// Whether the Git tab is on screen.
    fn git_shown(&self) -> TabShown {
        let layout = self.layout();
        let dock = layout.docks.place(DockPanel::Git);
        if layout.docks.shown(dock) == Some(DockPanel::Git) && layout.panel(dock).visible {
            TabShown::Shown
        } else {
            TabShown::Hidden
        }
    }

    /// Takes `paths` as the watcher does when another program wrote them, for tests, which
    /// do not wait on the clock.
    #[cfg(test)]
    pub(crate) fn files_written(&mut self, paths: Vec<PathBuf>) {
        self.with_map(|map, commands| map.files_changed(commands, paths));
    }

    /// Selects these faces of the open map for the UV editor, as Alt-clicks would.
    #[cfg(test)]
    pub(crate) fn select_map_faces(&mut self, faces: &[FaceRef]) {
        self.features.map.map_editor.select_faces(faces);
    }

    /// The map editor, for tests to find where it drew the map's entities.
    #[cfg(test)]
    pub(crate) const fn map_editor(&self) -> &MapEditor {
        &self.features.map.map_editor
    }

    /// Puts the map editor's 3D camera at `camera`, as a showcase or a script looks from.
    pub const fn set_map_camera(&mut self, camera: Camera) {
        self.features.map.map_editor.set_camera(camera);
    }

    /// Carries out what a dock region's panel asked for, after the frame drew it.
    fn apply(&mut self, action: FeatureAction, ctx: &egui::Context) {
        match action {
            FeatureAction::Jump(jump) => self.jump(jump),
            FeatureAction::Map(map_actions) => {
                self.with_map(|map, commands| map.map_actions(commands, map_actions));
            }
            FeatureAction::Entity(action) => self.entity_action(action, ctx),
            FeatureAction::Bottom(BottomAction::Terminal(action)) => {
                action.run(&mut self.commands());
            }
            FeatureAction::Bottom(BottomAction::Git(action)) => {
                let shown = self.git_shown();
                let mut commands = Commands::new(&mut self.session, &mut self.preview_error);
                let request = self.features.git.apply(action, &mut commands, shown);
                self.request(request);
            }
            FeatureAction::Bottom(BottomAction::Debug(action)) => {
                if let Some(request) = action.run(&mut self.commands()) {
                    self.request(request);
                }
            }
            FeatureAction::Bottom(BottomAction::ProblemsAsExcerpts) => self.problems_as_excerpts(),
            FeatureAction::Bottom(BottomAction::Jump(_)) => {}
            FeatureAction::Agent(action) => self.agent_action(action, ctx),
            FeatureAction::Left(LeftAction::Open(folder)) => self.open_asked(&folder),
            FeatureAction::Left(LeftAction::OpenFolder) => self.open_folder(),
            FeatureAction::Left(LeftAction::New(template)) => self.new_project(template),
            FeatureAction::Left(LeftAction::Jump(_)) => {}
        }
    }

    /// Runs `call` on the editors' feature with the commands, then carries out what it asked
    /// of the window, in order.
    fn with_editors<R>(
        &mut self,
        call: impl FnOnce(&mut EditorsFeature, &mut Commands<'_>) -> R,
    ) -> R {
        let mut commands = Commands::new(&mut self.session, &mut self.preview_error);
        let out = call(&mut self.features.editors, &mut commands);
        self.take_editor_requests();
        out
    }

    /// Carries out what the editors' feature asked of the window.
    fn take_editor_requests(&mut self) {
        for request in self.features.editors.take_requests() {
            self.request(request);
        }
    }

    /// Runs `call` on the map's feature with the commands, then carries out what it asked of
    /// the window, in order.
    fn with_map<R>(&mut self, call: impl FnOnce(&mut MapFeature, &mut Commands<'_>) -> R) -> R {
        let mut commands = Commands::new(&mut self.session, &mut self.preview_error);
        let out = call(&mut self.features.map, &mut commands);
        for request in self.features.map.take_requests() {
            self.request(request);
        }
        out
    }

    /// Carries out what a feature asked of the window or another feature, at once.
    fn request(&mut self, request: ShellRequest) {
        match request {
            ShellRequest::Jump(jump) => self.jump(jump),
            ShellRequest::ShowLine { path, line } => {
                self.features.editors.pending_line = Some((path.clone(), line));
                self.jump(Jump { path, offset: 0 });
            }
            ShellRequest::ShowProjectLine { file, line } => self.show_line(&file, line),
            ShellRequest::RefreshEditor => {
                self.sync_editor();
                self.mark_changes();
            }
            ShellRequest::ShowCentre(tab) => self.centre_tab = tab,
            ShellRequest::OpenGameGuide => self.open_guide_to_missing_files(),
            ShellRequest::ShowPanel(panel) => {
                let params = ShowPanelParams {
                    panel,
                    when_shown: WhenShown::Keep,
                };
                let _: Option<Docks> = self.run(qs_layout::SHOW_PANEL, &params);
            }
        }
    }

    /// Runs the command `id` with `params`: its output, or why it failed.
    fn attempt<P: serde::Serialize, O: serde::de::DeserializeOwned>(
        &mut self,
        id: &str,
        params: &P,
    ) -> Result<O, ShellError> {
        self.commands().attempt(id, params)
    }

    /// Shows `places`, each a file's byte range, as the multibuffer's excerpts from `source`,
    /// in the Search tab (P4-33). `truncation` says whether the places were cut short.
    fn show_places(
        &mut self,
        source: MultiSource,
        places: Vec<(PathBuf, usize, usize)>,
        truncation: Truncation,
    ) {
        self.with_editors(|editors, commands| {
            editors.show_places(commands, source, places, truncation);
        });
    }

    /// Does what the multibuffer asked for (P4-33).
    fn multibuffer_action(&mut self, action: MultiAction) {
        match action {
            MultiAction::Search => self.search_project(),
            MultiAction::ReplaceAll => {
                let form = self.features.editors.multibuffer.form.clone();
                let files: Vec<PathBuf> = self
                    .run(PROJECT_FILES, &serde_json::json!({}))
                    .unwrap_or_default();
                // One file at a time, each its own undoable step, saved.
                for path in files {
                    let params = ReplaceInFileParams {
                        path,
                        text: form.text.clone(),
                        replacement: form.replacement.clone(),
                        case_sensitive: form.case_sensitive,
                        whole_word: form.whole_word,
                        regex: form.regex,
                    };
                    if let Err(error) = self.attempt::<_, ReplacedInFile>(REPLACE_IN_FILE, &params)
                    {
                        self.features.editors.multibuffer.error = Some(error.to_string());
                        return;
                    }
                }
                self.search_project();
            }
            MultiAction::Edit(index, text) => {
                let Some(excerpt) = self.features.editors.multibuffer.excerpts.get(index.get())
                else {
                    return;
                };
                let params = ReplaceRangeParams {
                    path: excerpt.path.clone(),
                    start: excerpt.start,
                    end: excerpt.end,
                    text: text.clone(),
                };
                match self.attempt::<_, ()>(REPLACE_RANGE, &params) {
                    Ok(()) => self.features.editors.multibuffer.edited(index, text),
                    Err(error) => self.features.editors.multibuffer.error = Some(error.to_string()),
                }
            }
            MultiAction::Open(path, offset) => {
                self.jump(Jump { path, offset });
                // The editor takes the keyboard, at the caret it went to, once it is drawn.
                self.features.editors.multibuffer.refocus = true;
            }
        }
    }

    /// Searches the project with the multibuffer's form and shows what it found.
    fn search_project(&mut self) {
        self.with_editors(|editors, commands| editors.search_project(commands));
    }

    /// Shows every problem as the multibuffer's excerpts (P4-33).
    fn problems_as_excerpts(&mut self) {
        self.with_editors(|editors, commands| editors.problems_as_excerpts(commands));
    }

    /// Runs what the editor asked for, as a frame would after the editor drew.
    #[cfg(test)]
    pub(crate) fn handle_requests_for_test(&mut self, requests: Vec<Request>) {
        self.handle(requests);
    }

    /// Runs what the editor asked for, each through its command, after sending the editor's
    /// unsent text so every answer is about the text on screen.
    fn handle(&mut self, requests: Vec<Request>) {
        for request in requests {
            let Some(editor) = self.features.editors.editor.as_mut() else {
                return;
            };
            let path = editor.path.clone();
            // An update's text is marked sent already, so this sends only edits made since.
            if let Some(text) = editor.unsent() {
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
                    let saved: Option<()> =
                        self.run(SAVE_FILE, &SaveFileParams { path: path.clone() });
                    if saved.is_some() {
                        // The editor's own write is no other program's (P5-17).
                        self.features.map.disk_watch.wrote(path);
                        self.reload_after_save();
                    }
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
                    if places.is_empty() {
                        Some(Popup::Note(Text::NoReferences))
                    } else {
                        let places = places.into_iter().map(|p| (p.path, p.start, p.end));
                        self.show_places(
                            MultiSource::References,
                            places.collect(),
                            Truncation::All,
                        );
                        None
                    }
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
                Request::Fixes(offset) => {
                    let params = CodeActionParams {
                        path,
                        offset,
                        index: None,
                    };
                    let fixes: Vec<ActionInfo> =
                        self.run(CODE_ACTIONS, &params).unwrap_or_default();
                    Some(if fixes.is_empty() {
                        Popup::Note(Text::NoQuickFix)
                    } else {
                        Popup::Fixes(offset, fixes.into_iter().map(|f| f.title).collect())
                    })
                }
                Request::Fix(offset, index) => {
                    let params = CodeActionParams {
                        path,
                        offset,
                        index: Some(ActionIndex::new(index)),
                    };
                    let _: Option<ActionInfo> = self.run(CODE_FIX, &params);
                    self.sync_editor();
                    None
                }
                Request::Format => {
                    let _: Option<serde_json::Value> =
                        self.run(FORMAT_FILE, &SaveFileParams { path });
                    self.sync_editor();
                    None
                }
                Request::InlineEdit {
                    selection,
                    instruction,
                } => self
                    .start_inline(&path, &selection, &instruction)
                    .err()
                    .map(Popup::InlineFailed),
            };
            if let Some(popup) = popup
                && let Some(editor) = self.features.editors.editor.as_mut()
            {
                editor.popup = Some(popup);
            }
        }
    }

    /// Brings the editor up to date with the open file: a new view for another file, the caret
    /// moved for another `file.open` of the same one, none for no file.
    fn sync_editor(&mut self) {
        let theme = &self.theme;
        let mut commands = Commands::new(&mut self.session, &mut self.preview_error);
        self.features.editors.sync_editor(&mut commands, theme);
        self.take_editor_requests();
    }

    /// Gives egui a system font for each character of the editor's text that the bundled fonts
    /// lack: the whole text of a file just opened, then the caret's line as it is edited.
    fn cover_characters(&mut self, ctx: &egui::Context) {
        self.features
            .editors
            .cover_characters(&mut self.system_fonts, ctx);
    }

    /// The multibuffer the Search tab shows (P4-33).
    #[cfg(test)]
    pub(crate) const fn multibuffer(&self) -> &Multibuffer {
        &self.features.editors.multibuffer
    }

    /// Draws the Search tab: the multibuffer, and does what it asked for (P4-33).
    fn search_tab(&mut self, ui: &mut Ui) {
        let theme = &self.theme;
        let root = self
            .session
            .context()
            .workbench
            .root()
            .map(Path::to_path_buf);
        let asked = egui::Frame::NONE
            .fill(theme.color(Token::SurfaceEditor))
            .inner_margin(theme.space(2))
            .show(ui, |ui| {
                multibuffer_ui(
                    ui,
                    theme,
                    &mut self.features.editors.multibuffer,
                    root.as_deref(),
                )
            })
            .inner;
        if let Some(action) = asked {
            self.multibuffer_action(action);
            ui.ctx().request_repaint();
        }
    }

    /// Draws the centre: its tabs and the one shown. When `focus` says to take it, the editor
    /// takes the keyboard, or the shown tab when no file is open or another tab is shown.
    #[expect(
        clippy::too_many_lines,
        reason = "the centre's tab bar and the tab it shows, drawn in one pass in the order they \
                  appear"
    )]
    fn centre(&mut self, ui: &mut Ui, focus: FocusNow) {
        self.sync_editor();
        if self.centre_tab != CentreTab::Game {
            // ADR-0028: the game goes on while another tab is shown.
            self.game.run_hidden(ui);
        }
        self.with_map(MapFeature::sync_map);
        let ctx = ui.ctx().clone();
        self.with_map(|map, commands| map.back_up_map(commands, &ctx));
        self.with_map(|map, commands| map.watch_disk(commands, &ctx));
        self.cover_characters(ui.ctx());
        let editor_shown =
            self.centre_tab == CentreTab::Editor && self.features.editors.editor.is_some();
        let refocus = std::mem::take(&mut self.features.editors.multibuffer.refocus);
        if (focus == FocusNow::Take || refocus) && editor_shown {
            self.focus_editor(ui.ctx());
        }
        let theme = &self.theme;
        let workbench = &self.session.context().workbench;
        let open = workbench.open().map(OpenFile::path);
        let mut chosen = None;
        egui::Frame::NONE
            .fill(theme.color(Token::SurfaceTabs))
            .inner_margin(egui::Margin::symmetric(8, 0))
            .show(ui, |ui| {
                ui.set_height(theme.size.tabs);
                ui.horizontal_centered(|ui| {
                    let editing = self.centre_tab == CentreTab::Editor;
                    for path in workbench.tabs() {
                        let shown = if editing && open == Some(path.as_path()) {
                            Shown::Shown
                        } else {
                            Shown::Hidden
                        };
                        let saved = if workbench.is_unsaved(path) {
                            Saved::Unsaved
                        } else {
                            Saved::Saved
                        };
                        if let Some(action) = file_tab(ui, theme, path, saved, shown) {
                            chosen = Some(action);
                        }
                    }
                    let files = if workbench.tabs().is_empty() {
                        OpenFiles::None
                    } else {
                        OpenFiles::Some
                    };
                    let centre = files.centre_tabs();
                    for &(tab, text) in centre {
                        let response = ui.selectable_value(&mut self.centre_tab, tab, text.get());
                        if focus == FocusNow::Take && !editor_shown && self.centre_tab == tab {
                            response.request_focus();
                        }
                    }
                    let topic = match self.centre_tab {
                        CentreTab::Editor => Topic::Editor,
                        CentreTab::Search => Topic::Search,
                        CentreTab::Game => Topic::Game,
                        CentreTab::Map => Topic::MapView,
                    };
                    help::button(ui, theme, topic);
                });
            });
        let switched = chosen.is_some();
        match chosen {
            Some(TabAction::Show(path)) => {
                let offset = self
                    .features
                    .editors
                    .parked
                    .get(&path)
                    .or(self
                        .features
                        .editors
                        .editor
                        .as_ref()
                        .filter(|e| e.path == path))
                    .map_or(0, Editor::caret_offset);
                self.jump(Jump { path, offset });
            }
            Some(TabAction::Close(path)) => {
                let _: Option<()> = self.run(CLOSE_FILE, &CloseFileParams { path });
            }
            None => {}
        }
        if switched {
            self.sync_editor();
        }
        self.editor_request();
        self.refresh_inlays();
        self.outline_panel(ui);
        self.second_pane(ui);
        let wrap = match self.layout().soft_wrap {
            SoftWrap::Off => Wrap::Off,
            SoftWrap::Width => Wrap::Width,
        };
        let theme = &self.theme;
        if self.centre_tab == CentreTab::Editor
            && let Some(editor) = &mut self.features.editors.editor
        {
            editor.view.set_wrap(wrap);
            let context = self.session.context();
            let file = context
                .workbench
                .root()
                .and_then(|root| editor.path.strip_prefix(root).ok())
                .map(Path::to_path_buf);
            if let Some(file) = &file {
                let debugger = context.preview.debugger();
                let lines = debugger.lines_of(file).map(line_index);
                editor.view.set_breakpoints(lines);
                let paused = debugger
                    .stopped()
                    .and_then(|(_, at)| (at.file.as_deref() == Some(file)).then_some(at.line?));
                editor.view.set_paused(paused.map(line_index));
            }
            breadcrumb(
                ui,
                theme,
                context.workbench.root(),
                editor,
                context.workbench.open(),
            );
            editor.view.set_key_mode(self.key_mode);
            editor
                .view
                .set_notes(line_notes(&context.workbench, editor));
            let requests = editor.show(ui, theme, &theme.view_style());
            let mut requests = requests;
            if let Some(line) = editor.view.take_note_click() {
                let offset = editor.view.state().buffer().offset(Position::new(line, 0));
                let first = context
                    .workbench
                    .problems()
                    .iter()
                    .filter(|p| p.path == editor.path)
                    .map(|p| p.diagnostic.span.start)
                    .find(|start| editor.view.state().buffer().position(*start).line == line);
                requests.push(Request::Fixes(first.unwrap_or(offset)));
            }
            let clicked = editor.view.take_gutter_click();
            if let (Some(file), Some(line)) = (file, clicked) {
                let line = u32::try_from(line.saturating_add(1)).unwrap_or(u32::MAX);
                let breakpoint = DebugAction::Breakpoint { file, line };
                if let Some(request) = breakpoint.run(&mut self.commands()) {
                    self.request(request);
                }
            }
            // The panels drawn before the editor this frame show what the requests change on
            // the next.
            if !requests.is_empty() {
                ui.ctx().request_repaint();
            }
            self.handle(requests);
            return;
        }
        if self.map_tab(ui) {
            return;
        }
        if self.centre_tab == CentreTab::Map
            && let Some(map) = &mut self.features.map.map
        {
            map.ui(ui);
            return;
        }
        if self.centre_tab == CentreTab::Search {
            self.search_tab(ui);
            return;
        }
        if self.centre_tab == CentreTab::Game && self.game.ui(ui) {
            return;
        }
        let (fill, message) = match self.centre_tab {
            CentreTab::Editor => (Token::SurfaceEditor, Text::NoFileOpen),
            CentreTab::Game => (Token::SurfaceViewport, Text::GameStopped),
            CentreTab::Map => (Token::SurfaceViewport, Text::NoMap),
            CentreTab::Search => (Token::SurfaceEditor, Text::NoSearchMatch),
        };
        let theme = &self.theme;
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

/// Whether any file is open in the centre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpenFiles {
    /// None is.
    None,
    /// At least one is.
    Some,
}

impl OpenFiles {
    /// The centre's tabs beside the files': the editor's own tab only when no file is open.
    const fn centre_tabs(self) -> &'static [(CentreTab, Text)] {
        if let Self::None = self {
            &[
                (CentreTab::Editor, Text::Editor),
                (CentreTab::Search, Text::ProjectSearch),
                (CentreTab::Game, Text::Game),
                (CentreTab::Map, Text::Map),
            ]
        } else {
            &[
                (CentreTab::Search, Text::ProjectSearch),
                (CentreTab::Game, Text::Game),
                (CentreTab::Map, Text::Map),
            ]
        }
    }
}

/// The open file's path under the project, each folder a step, and the definition the caret
/// is in, as mockup 01 draws it above the code: `src / weapons.qc / W_FireEmber`.
fn breadcrumb(
    ui: &mut Ui,
    theme: &Theme,
    root: Option<&Path>,
    editor: &Editor,
    open: Option<&OpenFile>,
) {
    let path = root
        .and_then(|root| editor.path.strip_prefix(root).ok())
        .unwrap_or(&editor.path);
    let muted = theme.color(Token::TextMuted);
    let separator = Text::PathSeparator.get();
    let frame = egui::Frame::NONE.inner_margin(egui::Margin::symmetric(
        BREADCRUMB_INDENT,
        BREADCRUMB_MARGIN,
    ));
    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (n, step) in path.components().enumerate() {
                if n > 0 {
                    ui.label(RichText::new(separator).small().color(muted));
                }
                ui.label(
                    RichText::new(step.as_os_str().to_string_lossy())
                        .small()
                        .color(muted),
                );
            }
            let symbol = open
                .filter(|open| open.path() == editor.path)
                .and_then(|open| open.symbol_at(editor.caret_offset()));
            if let Some(symbol) = symbol {
                ui.label(RichText::new(separator).small().color(muted));
                ui.label(
                    RichText::new(&symbol.name)
                        .small()
                        .monospace()
                        .color(theme.color(Token::CodeFunction)),
                );
            }
        })
    });
}

/// The breadcrumb's indent, in points: the gutter's padding, so it lines up with the line
/// numbers below it.
const BREADCRUMB_INDENT: i8 = 12;
/// The breadcrumb bar's margin above and below.
const BREADCRUMB_MARGIN: i8 = 4;

/// What a click on a file's tab asks for.
enum TabAction {
    /// Show the file in the editor.
    Show(PathBuf),
    /// Close its tab.
    Close(PathBuf),
}

/// Whether a file's text is saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Saved {
    /// It is.
    Saved,
    /// It has unsaved changes.
    Unsaved,
}

/// Whether a file's tab is the one shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shown {
    /// It is.
    Shown,
    /// Another is, or a centre tab.
    Hidden,
}

/// A file's tab: its name, a dot when it is unsaved, and a button that closes it.
fn file_tab(
    ui: &mut Ui,
    theme: &Theme,
    path: &Path,
    saved: Saved,
    shown: Shown,
) -> Option<TabAction> {
    let name = path
        .file_name()
        .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy());
    let label = if saved == Saved::Unsaved {
        format!("{name} {}", Text::UnsavedMark.get())
    } else {
        name.into_owned()
    };
    let mut action = None;
    let tab = ui
        .selectable_label(shown == Shown::Shown, label)
        .on_hover_text(path.to_string_lossy());
    // A screen reader says it is a tab, not one more button; the tree's row is the button.
    ui.ctx().accesskit_node_builder(tab.id, |node| {
        node.set_role(accesskit::Role::Tab);
    });
    if tab.clicked() {
        action = Some(TabAction::Show(path.to_path_buf()));
    }
    // The close button keeps its place and shows on hover, as Zed's default
    // `show_close_button: "hover"` does, so the tabs do not move under the pointer.
    let close = Button::new(RichText::new(Text::Remove.get()).color(egui::Color32::TRANSPARENT))
        .frame(false);
    let close = ui.add(close).on_hover_text(Text::CloseTabHint.get());
    if tab.hovered() || close.hovered() {
        ui.painter().text(
            close.rect.center(),
            egui::Align2::CENTER_CENTER,
            Text::Remove.get(),
            egui::FontId::default(),
            theme.color(Token::TextMuted),
        );
    }
    if close.clicked() {
        action = Some(TabAction::Close(path.to_path_buf()));
    }
    action
}

/// The size a drag of a panel's edge gave it: its size this frame, when the pointer is held
/// and the size has moved half a point or more since the last frame. A size that changes
/// without a drag, or the first frame's, is not one.
pub(crate) fn dragged_size(previous: Option<f32>, now: Option<f32>, dragging: bool) -> Option<f32> {
    let (previous, now) = (previous?, now?);
    (dragging && (now - previous).abs() >= 0.5).then_some(now)
}

/// Names the area `ui` draws for a screen reader (QA-9): its node, egui's generic container,
/// gets `role` and `name`.
pub(crate) fn name_area(ui: &Ui, role: accesskit::Role, name: Text) {
    ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
        node.set_role(role);
        node.set_label(name.get());
    });
}

/// Names a widget that egui named by its text, or not at all, for a screen reader (QA-9):
/// its node's label becomes `name`, and nothing else about it changes, so the widget's own
/// output events are not sent again.
pub(crate) fn name_field(ui: &Ui, response: &Response, name: Text) {
    name_widget(ui, response, name.get());
}

/// As [`name_field`], for a name that is the user's data, such as a key's.
pub(crate) fn name_widget(ui: &Ui, response: &Response, name: &str) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_label(name);
    });
}

/// A panel section: its heading, named for `topic`, with its help and, until it has content,
/// one line of explanation; the heading's response.
pub(crate) fn section(ui: &mut Ui, theme: &Theme, topic: Topic, empty: Text) -> Response {
    let heading = heading_with_help(ui, theme, topic.title(), topic);
    note(ui, theme, empty);
    heading
}

/// A panel's heading with its help button beside it (P7-11): the heading's response.
pub(crate) fn heading_with_help(ui: &mut Ui, theme: &Theme, text: Text, topic: Topic) -> Response {
    ui.horizontal(|ui| {
        let response = heading(ui, theme, text);
        help::button(ui, theme, topic);
        response
    })
    .inner
}

/// A panel section's heading: a heading to a screen reader, and where the keyboard lands when
/// a focus command names the panel, so Tab moves on into the section.
pub(crate) fn heading(ui: &mut Ui, theme: &Theme, text: Text) -> Response {
    let response = ui.add(
        egui::Label::new(
            RichText::new(text.get())
                .family(semibold())
                .size(theme.type_scale.panel_heading.size),
        )
        .sense(Sense::focusable_noninteractive()),
    );
    // egui gives a label its text as a value; a heading is read by its label.
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(accesskit::Role::Heading);
        node.set_label(text.get());
    });
    response
}

impl Theme {
    /// The editor's colours and font, from the theme's tokens.
    pub(crate) fn view_style(&self) -> ViewStyle {
        ViewStyle {
            font: egui::FontId::monospace(self.type_scale.code.size),
            background: self.color(Token::SurfaceEditor),
            current_line: self.color(Token::SurfaceCurrentLine),
            selection: self.color(Token::SelectFocus),
            text: self.color(Token::TextCode),
            line_number: self.color(Token::TextFaint),
            caret: self.color(Token::Accent),
            added: self.color(Token::DiffAdded),
            removed: self.color(Token::StatusErrorBg),
            removed_text: self.color(Token::TextOnError),
            paused_line: self.color(Token::DebugPausedLine),
            paused_mark: self.color(Token::StatusWarn),
            breakpoint: self.color(Token::StatusError),
            indent_guide: self.color(Token::Border),
            added_mark: self.color(Token::StatusOk),
            modified_mark: self.color(Token::StatusWarn),
            removed_mark: self.color(Token::StatusError),
            inlay: self.color(Token::TextFaint),
            note_error: self.color(Token::StatusError),
            note_warning: self.color(Token::StatusWarn),
            lightbulb: self.color(Token::Accent),
        }
    }
}

/// A line of muted text, wrapped to the panel's width.
pub(crate) fn note(ui: &mut Ui, theme: &Theme, text: Text) {
    ui.add(egui::Label::new(RichText::new(text.get()).color(theme.color(Token::TextMuted))).wrap());
}

/// The diagnostics' notes at the ends of `editor`'s lines (P4-34): each problem of its file at
/// the line its span starts on, a lightbulb on one of a code a quick fix may fix.
fn line_notes(workbench: &Workbench<FsLoader>, editor: &Editor) -> Vec<LineNote> {
    let buffer = editor.view.state().buffer();
    workbench
        .problems()
        .iter()
        .filter(|problem| problem.path == editor.path)
        .map(|problem| {
            let code = problem.diagnostic.code;
            LineNote {
                line: buffer.position(problem.diagnostic.span.start).line,
                text: problem.diagnostic.message.clone(),
                severity: if code.is_error() {
                    NoteSeverity::Error
                } else {
                    NoteSeverity::Warning
                },
                fixable: if FIXABLE.contains(&code) {
                    Fixable::Yes
                } else {
                    Fixable::No
                },
            }
        })
        .collect()
}

/// An inlay hint's text: the parameter's name and a colon, as rust-analyzer shows them.
pub(crate) fn inlay_text(name: &str) -> String {
    let mut text = name.to_owned();
    text.push(':');
    text.push(' ');
    text
}

/// The codes a quick fix may fix (`qc_ide`'s code actions).
const FIXABLE: [Code; 5] = [
    Code::E0301,
    Code::E0318,
    Code::E0319,
    Code::W0321,
    Code::W0322,
];

/// A line counted from 1, as the bridge and the debugger count them, counted from 0 as the
/// editor does.
pub(crate) fn line_index(line: u32) -> usize {
    usize::try_from(line.saturating_sub(1)).unwrap_or(usize::MAX)
}

/// The toolbar's play controls for the game's `state`: the command a click chose, if any.
/// Play starts a stopped game and stops a running one; Pause pauses it and shows it paused;
/// Resume takes Play's place while paused; Step and Restart map need a running game.
fn play_controls(ui: &mut Ui, theme: &Theme, state: PlayState) -> Option<&'static str> {
    let mut chosen = None;
    let (icon, text, id) = match state {
        PlayState::Stopped => (Icon::Play, Text::Play, PLAY),
        PlayState::Playing => (Icon::Play, Text::Playing, STOP),
        PlayState::Paused => (Icon::Play, Text::Resume, RESUME),
    };
    let primary =
        Button::new(RichText::new(labelled(icon, text)).color(theme.color(Token::TextOnAccent)))
            .fill(theme.color(Token::Accent));
    let hover = if state == PlayState::Playing {
        Text::StopHint
    } else {
        text
    };
    if ui.add(primary).on_hover_text(hover.get()).clicked() {
        chosen = Some(id);
    }
    let running = state != PlayState::Stopped;
    let pause_text = if state == PlayState::Paused {
        Text::Paused
    } else {
        Text::Pause
    };
    let pause = Button::new(labelled(Icon::Pause, pause_text))
        .frame(state == PlayState::Paused)
        .selected(state == PlayState::Paused);
    if ui
        .add_enabled(state == PlayState::Playing, pause)
        .on_hover_text(Text::PauseHint.get())
        .on_disabled_hover_text(Text::PauseHint.get())
        .clicked()
    {
        chosen = Some(PAUSE);
    }
    for (icon, text, hint, id) in [
        (Icon::Step, Text::Step, Text::StepHint, STEP),
        (Icon::Restart, Text::RestartMap, Text::RestartHint, RESTART),
    ] {
        let button = Button::new(labelled(icon, text)).frame(false);
        let response = ui
            .add_enabled(running, button)
            .on_hover_text(hint.get())
            .on_disabled_hover_text(hint.get());
        if response.clicked() {
            chosen = Some(id);
        }
    }
    chosen
}

/// Where the play loop finds the host and the game, for this app's executable.
fn preview_config() -> PreviewConfig {
    PreviewConfig::from_env(std::env::current_exe().unwrap_or_default())
}

/// A small filled circle in `colour`, before a status line.
fn status_dot(ui: &mut Ui, colour: egui::Color32) {
    let size = ui.text_style_height(&egui::TextStyle::Small);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), size / STATUS_DOT_PARTS, colour);
}

/// A status dot's radius as a part of the line's height: a quarter.
const STATUS_DOT_PARTS: f32 = 4.0;

/// The most of the window's width a side panel takes when the interface is zoomed in.
const SIDE_PANEL_SHARE: f32 = 0.3;
/// The most of the window's height the bottom panel takes when the interface is zoomed in.
const BOTTOM_PANEL_SHARE: f32 = 0.45;

/// The outline panel's width, in points, as the left panel's Symbols are wide.
const OUTLINE_WIDTH: f32 = 220.0;

/// How often a frame reads the running host's events.
pub(crate) const PREVIEW_POLL: std::time::Duration = std::time::Duration::from_millis(50);

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
