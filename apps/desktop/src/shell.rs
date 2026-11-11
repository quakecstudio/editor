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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eframe::egui::containers::panel::PanelState;
use eframe::egui::{
    self, Align, Button, Id, Layout as EguiLayout, Response, RichText, Sense, Ui, accesskit,
};
use eframe::egui_wgpu::RenderState;
use qs_commands::remote::Host;
use qs_commands::{CommandError, RegisterError, Registry, Session};
use qs_editor_view::{Position, ViewStyle};
use qs_layout::{
    Focus, HasLayout, Layout, Panel, RESIZE_PANEL, ResizePanelParams, TOGGLE_PANEL,
    TogglePanelParams,
};
use qs_render::{Camera, MapMesh};
use qs_settings::{Scope as SettingsScope, Setting, SettingsFile, SettingsStore};
use qs_templates::{GitHub, NEW_PROJECT, NewProjectParams, Template};
use qs_workbench::checkpoints::{RESTORE_CHECKPOINT, RestoreCheckpointParams};
use qs_workbench::debug::Location;
use qs_workbench::dock::DockFrame;
use qs_workbench::export::{ExportParams, Exported, PROJECT_EXPORT};
use qs_workbench::game_files::GameFiles;
use qs_workbench::staged::HunksParams;
use qs_workbench::{ACCEPT_HUNKS, REJECT_HUNKS};
use qs_workbench::{
    AtParams, CHOOSE_GAME, CLOSE_FILE, COMPLETE, Candidate, CheckAfterTyping, ChooseGameParams,
    CloseFileParams, Config, DEFINITION, DOCK, EntryKind, FsLoader, GAME, HOVER, HasPreview,
    HasWorkbench, HoverInfo, OPEN_FILE, OPEN_PROJECT, OnLoss, OpenFile, OpenFileParams,
    OpenProjectParams, PAUSE, PLAY, POLL, Place, PlayParams, PlayState, Preview, PreviewConfig,
    PreviewStatus, REFERENCES, RELOAD, RENAME, RESTART, RESUME, RecentProjects, ReloadParams,
    RenameParams, SAVE_FILE, STEP, STOP, SaveFileParams, TAKE_CHECKED, UPDATE_FILE,
    UpdateFileParams, Workbench,
};
use qs_workbench::{
    BREAKPOINT, BreakpointParams, CONSOLE, ConsoleParams, STEP_BY, StepByParams, UNWATCH,
    UnwatchParams, WATCH, WatchParams,
};
use qs_workbench::{SELECT, SET_FIELD, SelectParams, SelectedEntity, SetFieldParams};

use crate::agent::{self, AgentAction, AgentPanel};
use crate::debugger::{self, DebugAction, DebugInput};
use crate::editor::{Editor, Popup, Request};
use crate::entities::{self, EntityAction, Outliner, Properties};
use crate::fallback::{SystemFonts, families};
use crate::finder::FileFinder;
use crate::game_guide::{self, Guide, GuideAction};
use crate::icons::Icon;
use crate::keymap::{self, Scope};
use crate::map_view::{self, MapView};
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
    /// The play loop: the preview host, when it runs.
    pub preview: Preview,
}

impl HasPreview for Workspace {
    fn workbench_and_preview(&mut self) -> (&mut Workbench<FsLoader>, &mut Preview) {
        (&mut self.workbench, &mut self.preview)
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
    Panel(Panel),
    Palette,
    Finder,
    Editor,
    Section(Text),
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
    /// The map view.
    Map,
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
    /// The file finder.
    pub(crate) finder: FileFinder,
    symbol_scope: SymbolScope,
    editor: Option<Editor>,
    system_fonts: SystemFonts,
    /// The window's wgpu device and formats, which the map view draws with.
    render_state: Option<RenderState>,
    map: Option<MapView>,
    /// The editor's file and text revision whose characters have fonts.
    covered: Option<(PathBuf, u64)>,
    /// The count of focus commands the shell has applied (`Layout::focus_requests`): the
    /// layout's when the window opens, so the keyboard moves only when a focus command runs.
    focus_applied: u64,
    /// The last play-loop command's error, shown on the toolbar until the next one.
    preview_error: Option<String>,
    /// What the last export did: the zip's path, or why it failed (P4-28).
    export_note: Option<Result<PathBuf, String>>,
    /// Whether the guide to the game's files is shown (P3-21), and what it last found.
    guide: (Guide, Option<GameFiles>),
    /// The user's settings file, which keeps the game folder chosen (P3-44).
    settings_file: Option<PathBuf>,
    /// Where the app's menu is: the system's menu bar, or the toolbar in the window (P3-44).
    menu: MenuPlace,
    /// The Entities outliner's filter.
    outliner: Outliner,
    /// The Properties panel's edits.
    properties: Properties,
    /// The text typed in the Debugger tab's watch box and the console's command box.
    debug_input: DebugInput,
    /// Where the game last stopped that the editor was taken to.
    shown_stop: Option<Location>,
    /// The line, from 1, of the file to show once it is open in the editor.
    pending_line: Option<(PathBuf, u32)>,
    /// The editors of the tabs not shown, with their carets and undo histories.
    parked: BTreeMap<PathBuf, Editor>,
    /// The projects opened most recently, and the file they are kept in.
    recent: (RecentProjects, Option<PathBuf>),
    /// The IDE's window as last given to `preview.dock`.
    dock_frame: Option<DockFrame>,
    /// The project open in the last frame, to see when another opens.
    seen_root: Option<PathBuf>,
    /// Why the last project asked for did not open.
    project_error: Option<String>,
    /// The socket other processes run the window's commands on (AGT-7), once it is served.
    host: Option<Host>,
    /// The agent panel's threads and provider (P4-09).
    agent: AgentPanel,
    /// An inline edit's answer, while the model writes it (P4-10).
    inline: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
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
        let workspace = Workspace {
            layout,
            // P3-35: the check after typing, off the frame in the app.
            workbench: Workbench::with_check(FsLoader, config, check),
            preview: Preview::new(preview_config()),
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
            finder: FileFinder::default(),
            symbol_scope: SymbolScope::default(),
            editor: None,
            system_fonts: SystemFonts::new(&Theme::bundled_fonts()),
            render_state: None,
            map: None,
            covered: None,
            focus_applied: layout.focus_requests,
            preview_error: None,
            export_note: None,
            guide: (Guide::Hidden, None),
            settings_file: None,
            menu: MenuPlace::default(),
            outliner: Outliner::default(),
            properties: Properties::default(),
            debug_input: DebugInput::default(),
            shown_stop: None,
            pending_line: None,
            parked: BTreeMap::new(),
            recent: (RecentProjects::default(), None),
            seen_root: None,
            dock_frame: None,
            project_error: None,
            host: None,
            agent: AgentPanel::new(Box::new(agent::provider_model)),
            inline: None,
            ctx: None,
        })
    }

    /// Answers other processes' commands on `host` from now on, each frame, in the window's
    /// session (AGT-7): `quakec-ide mcp --connect` runs the agent's tools here.
    pub fn set_host(&mut self, host: Host) {
        self.host = Some(host);
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
        self.editor
            .as_ref()
            .map(|editor| editor.view.state().buffer().text())
    }

    /// Gives the editor the keyboard, as a click in it does.
    pub fn focus_editor(&self, ctx: &egui::Context) {
        ctx.memory_mut(|m| m.request_focus(Id::new(EDITOR_AREA)));
    }

    /// Reads the recent projects from `file`, and keeps them there.
    pub fn set_recent_file(&mut self, file: PathBuf) {
        self.recent = (RecentProjects::load(&file), Some(file));
    }

    /// Reads and keeps the user's settings in `file` (P3-44): the game folder chosen before is
    /// Play's again when it still holds a game, and is left unused, as missing, when it does
    /// not.
    pub fn set_settings_file(&mut self, file: PathBuf) {
        let saved = SettingsFile::load(&file)
            .map_err(|error| tracing::warn!(%error, "settings not read"))
            .ok()
            .and_then(|settings| settings.game.folder);
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
        let chosen: Option<GameFiles> =
            self.run(CHOOSE_GAME, &ChooseGameParams { path: path.clone() });
        if chosen.as_ref().is_some_and(|files| files.state.plays()) {
            self.preview_error = None;
            if let Some(file) = &self.settings_file {
                let value = serde_json::to_value(&path).ok();
                let saved = SettingsStore::open(file.clone(), None).and_then(|mut store| {
                    store.put(SettingsScope::User, Setting::GameFolder, value)
                });
                if let Err(error) = saved {
                    tracing::warn!(%error, "the game folder was not saved");
                }
            }
        }
        chosen
    }

    /// The projects opened most recently, the latest first.
    #[must_use]
    pub fn recent_projects(&self) -> &[PathBuf] {
        self.recent.0.projects()
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
        &self.agent
    }

    /// The report of agent thread `index`, once it has ended.
    #[must_use]
    pub fn agent_report(&self, index: usize) -> Option<&qs_agent::RunReport> {
        self.agent.report(index)
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
            .editor
            .as_ref()
            .map(|editor| editor.view.state().buffer().text())
            .unwrap_or_default();
        let prompt = qs_agent::inline_prompt(path, &file, selection, instruction);
        let ctx = self.ctx.clone();
        let answered = self.agent.ask_once(prompt, move || {
            if let Some(ctx) = &ctx {
                ctx.request_repaint();
            }
        })?;
        self.inline = Some(answered);
        Ok(())
    }

    /// Shows an inline edit's answer in the editor once it has come; whether it has.
    fn take_inline(&mut self) -> bool {
        let Some(answer) = self
            .inline
            .as_ref()
            .and_then(|answered| answered.try_recv().ok())
        else {
            return false;
        };
        self.show_inline(answer);
        true
    }

    /// Shows an inline edit's answer, or why there is none, in the editor.
    fn show_inline(&mut self, answer: Result<String, String>) {
        self.inline = None;
        if let Some(editor) = self.editor.as_mut() {
            match answer {
                Ok(text) => editor.propose(&text),
                Err(why) => editor.inline_failed(why),
            }
        }
    }

    /// Waits for an inline edit's answer and shows it. For tests.
    #[cfg(test)]
    pub(crate) fn wait_inline(&mut self) {
        if let Some(answer) = self
            .inline
            .as_ref()
            .and_then(|answered| answered.recv().ok())
        {
            self.show_inline(answer);
        }
    }

    /// The agent panel, to drive its threads.
    pub const fn agent_mut(&mut self) -> &mut AgentPanel {
        &mut self.agent
    }

    /// Keeps the agent's provider configuration in `file` (P4-07).
    pub fn set_agent_file(&mut self, file: PathBuf) {
        self.agent.set_config_file(file);
    }

    /// Starts an agent thread on `task` over the open project (P4-09), each `@` mention it
    /// names attached (UI-8); why it could not.
    ///
    /// # Errors
    ///
    /// No project open, no provider set up, no model or no checkpoint.
    pub fn start_agent(&mut self, task: &str, ctx: &egui::Context) -> Result<(), String> {
        let root = self.agent_root()?;
        let live = self.live();
        let task = qs_agent::expand_mentions(&mut self.session, &root, task, &live);
        self.start_thread(&root, &task, ctx)
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
        let root = self.agent_root()?;
        self.start_thread(&root, &qs_agent::ask_about(entity), ctx)
    }

    /// The open project's folder, which an agent thread works in.
    fn agent_root(&self) -> Result<PathBuf, String> {
        self.workbench()
            .root()
            .map(Path::to_path_buf)
            .ok_or_else(|| Text::NoProject.get().to_owned())
    }

    /// Starts a thread on `task` as given, over the project in `root`, its calls decided by the
    /// profile and tool rules the project's settings hold now (P4-30).
    fn start_thread(&mut self, root: &Path, task: &str, ctx: &egui::Context) -> Result<(), String> {
        if let Some(permissions) = self.agent_permissions(root) {
            self.agent.set_permissions(permissions);
        }
        let ctx = ctx.clone();
        self.agent
            .start(&mut self.session, root, task, move || ctx.request_repaint())
    }

    /// What the running game shows now, for `@entity` and `@error` (PRIN-2): the selected
    /// edict with its fields as the Properties panel writes them, and the console's last error.
    fn live(&self) -> qs_agent::Live {
        let preview = &self.session.context().preview;
        let entities = preview.entities();
        let entity = entities.selected().map(|selected| qs_agent::EntityState {
            edict: selected.edict,
            classname: selected.classname.clone(),
            fields: selected
                .fields
                .iter()
                .map(|field| (field.name.clone(), entities.show(&field.value)))
                .collect(),
        });
        let error = preview
            .console()
            .filter(|line| line.is_error())
            .last()
            .map(|line| line.text.clone());
        qs_agent::Live { entity, error }
    }

    /// The user's and the project at `root`'s settings, when the window keeps a settings file.
    fn settings_store(&self, root: &Path) -> Option<SettingsStore> {
        let user = self.settings_file.clone()?;
        SettingsStore::open(user, Some(qs_settings::project_settings_path(root)))
            .map_err(|error| tracing::warn!(%error, "settings not read"))
            .ok()
    }

    /// The agent's profile and tool rules as the settings set them (P4-30).
    fn agent_permissions(&self, root: &Path) -> Option<qs_agent::Permissions> {
        let store = self.settings_store(root)?;
        let value = |setting| store.value(setting).0;
        Some(qs_agent::Permissions {
            profile: serde_json::from_value(value(Setting::AgentProfile)).unwrap_or_default(),
            rules: serde_json::from_value(value(Setting::ToolPermissions)).unwrap_or_default(),
        })
    }

    /// Keeps `permissions` in the open project's settings, the profile and the rules.
    fn keep_agent_permissions(&self, permissions: &qs_agent::Permissions) {
        let Some(root) = self.workbench().root().map(Path::to_path_buf) else {
            return;
        };
        let Some(mut store) = self.settings_store(&root) else {
            return;
        };
        for (setting, value) in [
            (
                Setting::AgentProfile,
                serde_json::to_value(permissions.profile),
            ),
            (
                Setting::ToolPermissions,
                serde_json::to_value(&permissions.rules),
            ),
        ] {
            let kept = value.map_err(|error| error.to_string()).and_then(|value| {
                store
                    .put(SettingsScope::Project, setting, Some(value))
                    .map_err(|error| error.to_string())
            });
            if let Err(error) = kept {
                tracing::warn!(%error, "the agent's permissions were not kept");
            }
        }
    }

    /// Waits for the next thing agent thread `index`'s model sends and takes it; `false` when
    /// its model has ended. For tests.
    #[cfg(test)]
    pub(crate) fn wait_agent(&mut self, index: usize) -> bool {
        self.agent.wait(&mut self.session, index)
    }

    /// Does what the agent panel asked for.
    pub(crate) fn agent_action(&mut self, action: AgentAction, ctx: &egui::Context) {
        match action {
            AgentAction::Start(task) => {
                let started = self.start_agent(&task, ctx);
                self.agent.set_error(started.err());
            }
            AgentAction::SaveSetup => self.agent.save_setup_to_keychain(),
            AgentAction::Accept(hunks) => {
                let _accepted: Option<serde_json::Value> =
                    self.run(ACCEPT_HUNKS, &HunksParams { hunks });
            }
            AgentAction::Reject(hunks) => {
                let _rejected: Option<serde_json::Value> =
                    self.run(REJECT_HUNKS, &HunksParams { hunks });
            }
            AgentAction::Permit(index, permit) => {
                if let Some(kept) = self.agent.permit(&mut self.session, index, permit) {
                    self.keep_agent_permissions(&kept);
                }
            }
            AgentAction::Profile(profile) => {
                let mut permissions = self.agent.permissions().clone();
                permissions.profile = profile;
                self.keep_agent_permissions(&permissions);
                self.agent.set_permissions(permissions);
            }
            AgentAction::Restore(number) => {
                let _restored: Option<serde_json::Value> =
                    self.run(RESTORE_CHECKPOINT, &RestoreCheckpointParams { number });
            }
        }
    }

    /// Asks for a new folder with the system's dialog, makes a project of `template` there
    /// (`project.new`, P4-17) and opens it, saying why when it could not.
    pub fn new_project(&mut self, template: Template) {
        let picked = rfd::FileDialog::new()
            .set_title(Text::NewProjectTitle.get())
            .save_file();
        let Some(folder) = picked else {
            return;
        };
        let params = NewProjectParams {
            template,
            path: folder.clone(),
        };
        let made = serde_json::to_value(params)
            .map_err(|error| error.to_string())
            .and_then(|params| {
                self.execute(NEW_PROJECT, params)
                    .map_err(|error| error.to_string())
            });
        match made {
            Ok(_) => self.open_asked(&folder),
            Err(error) => self.project_error = Some(error),
        }
    }

    /// Exports the open project for players (`project.export`, P4-28), checked against the game
    /// folder Play reads (the first install found when none was chosen); the status bar says
    /// where the zip is or why there is none.
    pub fn export_mod(&mut self) {
        let game = self
            .session
            .context()
            .preview
            .game_folder()
            .map(Path::to_path_buf);
        let params = ExportParams {
            game,
            ..ExportParams::default()
        };
        let exported = serde_json::to_value(params)
            .map_err(|error| error.to_string())
            .and_then(|params| {
                self.execute(PROJECT_EXPORT, params)
                    .map_err(|error| error.to_string())
            })
            .and_then(|answer| {
                serde_json::from_value::<Exported>(answer).map_err(|error| error.to_string())
            });
        self.export_note = Some(exported.map(|exported| exported.path));
    }

    /// What the last export did, for tests.
    #[cfg(test)]
    pub(crate) const fn export_note(&self) -> Option<&Result<PathBuf, String>> {
        self.export_note.as_ref()
    }

    /// Opens the project in `folder`, saying why when it does not open; a recent project that
    /// no longer opens comes off the list.
    fn open_asked(&mut self, folder: &Path) {
        // A folder with no project file opens empty, as Zed opens any folder; one that is not
        // there any more, as a recent project may be, does not.
        let opened = if folder.is_dir() {
            self.open_project(folder).map_err(|error| error.to_string())
        } else {
            Err(format!("{} {}", Text::FolderGone.get(), folder.display()))
        };
        match opened {
            Ok(()) => self.project_error = None,
            Err(error) => {
                self.project_error = Some(error.to_string());
                self.recent.0.forget(folder);
                self.save_recent();
            }
        }
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
        let root = self
            .session
            .context()
            .workbench
            .root()
            .map(Path::to_path_buf);
        if root == self.seen_root {
            return;
        }
        if let Some(root) = &root
            && self.recent.0.opened(root)
        {
            self.save_recent();
        }
        self.seen_root = root;
    }

    /// Writes the recent projects to their file, if they have one.
    fn save_recent(&self) {
        if let Some(file) = &self.recent.1
            && let Err(error) = self.recent.0.save(file)
        {
            tracing::warn!(%error, "recent projects not written");
        }
    }

    /// Gives the shell the window's wgpu device and formats, which the map view draws with.
    pub fn set_render_state(&mut self, state: Option<RenderState>) {
        self.render_state = state;
    }

    /// Shows `mesh` in the map view from `camera`, and the view; false when the window has no
    /// wgpu device to draw it with.
    pub fn show_map(&mut self, mesh: &MapMesh, camera: Camera) -> bool {
        let Some(state) = &self.render_state else {
            return false;
        };
        map_view::install(state, mesh);
        self.map = Some(MapView::new(camera));
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
        self.map.as_ref()
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
        let layout = self.layout().as_written();
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
        // P4-09: what the agent's models sent since the last frame; their calls run here.
        if self.agent.pump(&mut self.session) {
            ui.ctx().request_repaint();
        }
        self.ctx = Some(ui.ctx().clone());
        if self.take_inline() {
            ui.ctx().request_repaint();
        }
        // AGT-7: commands sent over the socket run first, so the frame draws what they did.
        if let Some(host) = &self.host
            && host.answer(&mut self.session) > 0
        {
            ui.ctx().request_repaint();
        }
        if self.session.context().preview.state() != PlayState::Stopped {
            let polled: Option<PreviewStatus> = self.run(POLL, &serde_json::json!({}));
            // BRIDGE-3.5: a click in the paused game picked an edict; open its spawn function.
            if let Some(classname) = polled.and_then(|status| status.picked) {
                self.entity_action(EntityAction::Open(classname), ui.ctx());
            }
        }
        self.follow_stop();
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
        self.keys(ui);
        let pending = self.pending_focus();
        self.toolbar(ui);
        self.reset_notice(ui);
        self.game_guide(ui);
        self.status_bar(ui);
        for panel in [Panel::Left, Panel::Right, Panel::Bottom] {
            self.panel(ui, panel, pending == Some(Focus::Panel(panel)));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(self.theme.color(Token::SurfaceEditor)))
            .show(ui, |ui| {
                name_area(ui, accesskit::Role::Pane, Text::Centre);
                self.centre(ui, pending == Some(Focus::Editor));
            });
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
                self.pending_line = Some((path.clone(), line));
                self.jump(Jump { path, offset: 0 });
            }
            Some((path, None)) => {
                let offset = self.parked.get(&path).map_or(0, Editor::caret_offset);
                self.jump(Jump { path, offset });
            }
            None => {}
        }
        let dragging = ui.input(|input| input.pointer.any_down());
        self.save_if_changed(dragging);
    }

    /// The shortcuts pressed this frame: the palette's, and while it is closed each binding's
    /// command (`keymap`), the editor's own keys left to it while it has the keyboard.
    fn keys(&mut self, ui: &Ui) {
        if ui.input_mut(|i| i.consume_shortcut(&keymap::OPEN_PALETTE)) {
            self.finder.close();
            self.palette.open();
            return;
        }
        if ui.input_mut(|i| i.consume_shortcut(&keymap::OPEN_FOLDER)) {
            self.open_folder();
            return;
        }
        if ui.input_mut(|i| i.consume_shortcut(&keymap::OPEN_FILE_FINDER)) {
            self.palette.close();
            self.finder.open();
            return;
        }
        if self.palette.is_open() || self.finder.is_open() {
            return;
        }
        // The editor, with its own undo history, takes its keys itself.
        let editing = ui.memory(|m| m.has_focus(Id::new(EDITOR_AREA)));
        for binding in keymap::in_match_order() {
            if editing && binding.scope == Scope::Shell {
                continue;
            }
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
        let mut open_guide = false;
        let mut export_asked = false;
        let mut control = None;
        let preview = &self.session.context().preview;
        let state = preview.state();
        let docked = if preview.docks() {
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
            self.palette.open();
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
        // P3-21: Play that failed for want of the game opens the guide to its files.
        if failed && id == PLAY {
            let files: Option<GameFiles> = self.run(GAME, &serde_json::json!({}));
            if let Some(files) = files {
                self.guide = (Guide::for_files(&files), Some(files));
            }
        }
    }

    /// Opens the guide to the game's files (P3-21), from the File menu: what the game folder,
    /// or the first install found, holds and lacks, and the folder picker.
    pub fn show_game_guide(&mut self) {
        let files: Option<GameFiles> = self.run(GAME, &serde_json::json!({}));
        self.guide = (Guide::Shown, files);
    }

    /// The guide to the game's files while it is shown: a chosen folder is read in place and
    /// kept for Play when it plays (`preview.choose_game`), so Play works without a restart.
    fn game_guide(&mut self, ui: &mut Ui) {
        let (shown, Some(files)) = &self.guide else {
            return;
        };
        match game_guide::window(ui.ctx(), &self.theme, *shown, files) {
            Some(GuideAction::Close) => self.guide.0 = Guide::Hidden,
            Some(GuideAction::ChooseFolder) => {
                let picked = rfd::FileDialog::new()
                    .set_title(Text::ChooseGameTitle.get())
                    .pick_folder();
                if let Some(path) = picked {
                    let chosen = self.choose_game_folder(path);
                    if chosen.is_some() {
                        self.guide.1 = chosen;
                    }
                }
            }
            None => {}
        }
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
                if let Some(entity) = self.live().entity {
                    let started = self.ask_about(&entity, ctx);
                    self.agent.set_error(started.err());
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

    /// Runs what the Debugger tab, the console's box or a click in the editor's gutter asked
    /// for, through its command, or shows a line in the editor.
    fn debug_action(&mut self, action: DebugAction) {
        let ran = match action {
            DebugAction::Step(kind) => self
                .run::<_, PreviewStatus>(STEP_BY, &StepByParams { kind })
                .is_some(),
            DebugAction::Watch(expression) => self
                .run::<_, ()>(WATCH, &WatchParams { expression })
                .is_some(),
            DebugAction::Unwatch(index) => self
                .run::<_, PreviewStatus>(UNWATCH, &UnwatchParams { index })
                .is_some(),
            DebugAction::Breakpoint { file, line } => self
                .run::<_, ()>(BREAKPOINT, &BreakpointParams { file, line })
                .is_some(),
            DebugAction::Console(command) => self
                .run::<_, ()>(CONSOLE, &ConsoleParams { command })
                .is_some(),
            DebugAction::Show { file, line } => {
                self.show_line(&file, line);
                true
            }
        };
        if !ran {
            self.preview_error = Some(Text::EditFailed.get().to_owned());
        }
    }

    /// Opens `file`, relative to the project's root, at `line`, from 1.
    fn show_line(&mut self, file: &Path, line: u32) {
        let Some(root) = self.session.context().workbench.root() else {
            return;
        };
        let path = root.join(file);
        self.pending_line = Some((path.clone(), line));
        self.jump(Jump { path, offset: 0 });
    }

    /// Takes the editor to where the game stopped, once for each stop.
    fn follow_stop(&mut self) {
        let stop = self
            .session
            .context()
            .preview
            .debugger()
            .stopped()
            .map(|(_, at)| at.clone());
        if stop == self.shown_stop {
            return;
        }
        if let Some(Location {
            file: Some(file),
            line: Some(line),
            ..
        }) = &stop
        {
            self.show_line(file, *line);
        }
        self.shown_stop = stop;
    }

    /// Reloads the running game after a save, its error shown on the toolbar.
    fn reload_after_save(&mut self) {
        if self.session.context().preview.state() == PlayState::Stopped {
            return;
        }
        // P3-10: a reload that would lose a value waits for the modder (`reset_notice`).
        let params = ReloadParams {
            on_loss: OnLoss::Ask,
        };
        let reloaded: Option<PreviewStatus> = self.run(RELOAD, &params);
        if reloaded.is_none() {
            self.preview_error = Some(Text::ReloadFailed.get().to_owned());
        }
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
                name_area(ui, accesskit::Role::Status, Text::StatusBar);
                ui.horizontal_centered(|ui| {
                    let muted = theme.color(Token::TextMuted);
                    let (errors, warnings) =
                        project::counts(self.session.context().workbench.problems());
                    for (icon, value) in [(Icon::Error, errors), (Icon::Warning, warnings)] {
                        ui.label(RichText::new(count(icon, value)).small().color(muted));
                    }
                    let running = match self.session.context().preview.state() {
                        PlayState::Stopped => Text::PreviewStopped,
                        PlayState::Playing => Text::PreviewRunning,
                        PlayState::Paused => Text::PreviewPaused,
                    };
                    ui.label(RichText::new(running.get()).small().color(muted));
                    match &self.export_note {
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
                });
            });
    }

    /// Draws `panel`, giving it the keyboard when `focus`: its first heading, or the bottom
    /// panel's selected tab, takes it.
    fn panel(&mut self, ui: &mut Ui, panel: Panel, focus: bool) {
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
        let preview = &self.session.context().preview;
        let tree = &mut self.outliner;
        let properties = &mut self.properties;
        let debug_input = &mut self.debug_input;
        let agent = &mut self.agent;
        let mut agent_action = None;
        let recent_projects = self.recent.0.projects();
        let project_error = self.project_error.as_deref();
        let mut open: Option<LeftAction> = None;
        let mut entity_action = None;
        let mut debug_action = None;
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
                let name = match panel {
                    Panel::Left => Text::LeftPanel,
                    Panel::Right => Text::RightPanel,
                    Panel::Bottom => Text::BottomPanel,
                };
                name_area(ui, accesskit::Role::Pane, name);
                match panel {
                    Panel::Left => {
                        let recent = (recent_projects, project_error);
                        match left(ui, theme, (workbench, recent), scope, focus) {
                            Some(LeftAction::Jump(to)) => jump = Some(to),
                            asked => open = asked,
                        }
                        ui.add_space(theme.space(3));
                        if preview.state() == PlayState::Stopped {
                            section(ui, theme, Text::Entities, Text::NotPlaying);
                        } else {
                            heading(ui, theme, Text::Entities);
                            entity_action = entities::outliner(ui, theme, preview.entities(), tree);
                        }
                    }
                    Panel::Right => {
                        let asked =
                            right(ui, theme, (workbench, preview), (properties, agent), focus);
                        (entity_action, agent_action) = asked;
                    }
                    Panel::Bottom => {
                        match bottom(
                            ui,
                            theme,
                            bottom_tab,
                            (workbench, preview, debug_input),
                            focus,
                        ) {
                            Some(BottomAction::Jump(to)) => jump = Some(to),
                            Some(BottomAction::Debug(action)) => debug_action = Some(action),
                            None => {}
                        }
                    }
                }
            });
        if let Some(jump) = jump {
            self.jump(jump);
        }
        if let Some(action) = entity_action {
            self.entity_action(action, ui.ctx());
        }
        if let Some(action) = debug_action {
            self.debug_action(action);
        }
        if let Some(action) = agent_action {
            self.agent_action(action, ui.ctx());
        }
        match open {
            Some(LeftAction::Open(folder)) => self.open_asked(&folder),
            Some(LeftAction::OpenFolder) => self.open_folder(),
            Some(LeftAction::New(template)) => self.new_project(template),
            Some(LeftAction::Jump(_)) | None => {}
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
                    let saved: Option<()> = self.run(SAVE_FILE, &SaveFileParams { path });
                    if saved.is_some() {
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
                Request::InlineEdit {
                    selection,
                    instruction,
                } => self
                    .start_inline(&path, &selection, &instruction)
                    .err()
                    .map(Popup::InlineFailed),
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
        let workbench = &self.session.context().workbench;
        let tabs = workbench.tabs();
        self.parked.retain(|path, _| tabs.contains(path));
        let open = workbench.open();
        match (open, &mut self.editor) {
            (None, _) => self.editor = None,
            (Some(open), Some(editor)) if editor.path == open.path() => {
                editor.sync(open, &self.theme);
            }
            (Some(open), _) => {
                if let Some(old) = self.editor.take()
                    && tabs.contains(&old.path)
                {
                    self.parked.insert(old.path.clone(), old);
                }
                let editor = match self.parked.remove(open.path()) {
                    Some(mut editor) => {
                        editor.sync(open, &self.theme);
                        editor
                    }
                    None => Editor::new(Id::new(EDITOR_AREA), open, &self.theme),
                };
                self.editor = Some(editor);
            }
        }
        if let Some(editor) = self.editor.as_mut()
            && let Some((_, line)) = self.pending_line.take_if(|(path, _)| *path == editor.path)
        {
            editor.go_to_line(line_index(line));
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

    /// Draws the centre: its tabs and the one shown. When `focus`, the editor takes the
    /// keyboard, or the shown tab when no file is open or another tab is shown.
    fn centre(&mut self, ui: &mut Ui, focus: bool) {
        self.sync_editor();
        self.cover_characters(ui.ctx());
        let editor_shown = self.centre_tab == CentreTab::Editor && self.editor.is_some();
        if focus && editor_shown {
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
                        let shown = editing && open == Some(path.as_path());
                        if let Some(action) =
                            file_tab(ui, theme, path, workbench.is_unsaved(path), shown)
                        {
                            chosen = Some(action);
                        }
                    }
                    let centre = if workbench.tabs().is_empty() {
                        [
                            (CentreTab::Editor, Text::Editor),
                            (CentreTab::Game, Text::Game),
                            (CentreTab::Map, Text::Map),
                        ]
                        .as_slice()
                    } else {
                        [(CentreTab::Game, Text::Game), (CentreTab::Map, Text::Map)].as_slice()
                    };
                    for &(tab, text) in centre {
                        let response = ui.selectable_value(&mut self.centre_tab, tab, text.get());
                        if focus && !editor_shown && self.centre_tab == tab {
                            response.request_focus();
                        }
                    }
                });
            });
        let switched = chosen.is_some();
        match chosen {
            Some(TabAction::Show(path)) => {
                let offset = self
                    .parked
                    .get(&path)
                    .or(self.editor.as_ref().filter(|e| e.path == path))
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
        let theme = &self.theme;
        if self.centre_tab == CentreTab::Editor
            && let Some(editor) = &mut self.editor
        {
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
            let requests = editor.show(ui, theme, &view_style(theme));
            let clicked = editor.view.take_gutter_click();
            if let (Some(file), Some(line)) = (file, clicked) {
                let line = u32::try_from(line.saturating_add(1)).unwrap_or(u32::MAX);
                self.debug_action(DebugAction::Breakpoint { file, line });
            }
            // The panels drawn before the editor this frame show what the requests change on
            // the next.
            if !requests.is_empty() {
                ui.ctx().request_repaint();
            }
            self.handle(requests);
            return;
        }
        if self.centre_tab == CentreTab::Map
            && let Some(map) = &mut self.map
        {
            map.ui(ui);
            return;
        }
        let (fill, message) = match self.centre_tab {
            CentreTab::Editor => (Token::SurfaceEditor, Text::NoFileOpen),
            CentreTab::Game => (Token::SurfaceViewport, Text::GameStopped),
            CentreTab::Map => (Token::SurfaceViewport, Text::NoMap),
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
    let frame = egui::Frame::NONE.inner_margin(egui::Margin::symmetric(BREADCRUMB_INDENT, 4));
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

/// What a click on a file's tab asks for.
enum TabAction {
    /// Show the file in the editor.
    Show(PathBuf),
    /// Close its tab.
    Close(PathBuf),
}

/// A file's tab: its name, a dot when it is unsaved, and a button that closes it.
fn file_tab(
    ui: &mut Ui,
    theme: &Theme,
    path: &Path,
    unsaved: bool,
    shown: bool,
) -> Option<TabAction> {
    let name = path
        .file_name()
        .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy());
    let label = if unsaved {
        format!("{name} {}", Text::UnsavedMark.get())
    } else {
        name.into_owned()
    };
    let mut action = None;
    let tab = ui
        .selectable_label(shown, label)
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
fn name_area(ui: &Ui, role: accesskit::Role, name: Text) {
    ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
        node.set_role(role);
        node.set_label(name.get());
    });
}

/// Names a widget that egui named by its text, or not at all, for a screen reader (QA-9):
/// its node's label becomes `name`, and nothing else about it changes, so the widget's own
/// output events are not sent again.
pub(crate) fn name_field(ui: &Ui, response: &Response, name: Text) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_label(name.get());
    });
}

/// The right panel: the Properties of the selected entity while the game runs, then the agent
/// panel (P4-09); what either asked for. Its first heading takes the keyboard when `focus`.
fn right(
    ui: &mut Ui,
    theme: &Theme,
    (workbench, preview): (&Workbench<FsLoader>, &Preview),
    (properties, agent): (&mut Properties, &mut AgentPanel),
    focus: bool,
) -> (Option<EntityAction>, Option<AgentAction>) {
    let mut entity_action = None;
    let first = if preview.state() == PlayState::Stopped {
        section(ui, theme, Text::Properties, Text::NotPlaying)
    } else {
        let first = heading(ui, theme, Text::Properties);
        entity_action = entities::properties(ui, theme, preview, properties);
        first
    };
    if focus {
        first.request_focus();
    }
    ui.add_space(theme.space(3));
    heading(ui, theme, Text::Agent);
    let agent_action = agent.ui(ui, theme, &workbench.staged_hunks());
    (entity_action, agent_action)
}

/// A panel section: its heading and, until it has content, one line of explanation; the
/// heading's response.
fn section(ui: &mut Ui, theme: &Theme, text: Text, empty: Text) -> Response {
    let heading = heading(ui, theme, text);
    note(ui, theme, empty);
    heading
}

/// A panel section's heading: a heading to a screen reader, and where the keyboard lands when
/// a focus command names the panel, so Tab moves on into the section.
fn heading(ui: &mut Ui, theme: &Theme, text: Text) -> Response {
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

/// The share of the left panel's height the Project tree may take.
const TREE_SHARE: f32 = 0.35;
/// The share of the left panel's height the Symbols list may take.
const SYMBOLS_SHARE: f32 = 0.4;

/// The left panel's Project and Symbols sections; the row chosen, if any.
fn left(
    ui: &mut Ui,
    theme: &Theme,
    (workbench, (recent, error)): (&Workbench<FsLoader>, (&[PathBuf], Option<&str>)),
    scope: &mut SymbolScope,
    focus: bool,
) -> Option<LeftAction> {
    let height = ui.available_height();
    let mut jump = None;
    let project = heading(ui, theme, Text::Project);
    if focus {
        project.request_focus();
    }
    match workbench.root() {
        None => {
            if let Some(action) = no_project(ui, theme, recent, error) {
                return Some(action);
            }
        }
        Some(root) => {
            ui.label(RichText::new(project::root_name(root)).strong());
            let open = workbench.open().map(OpenFile::path);
            egui::ScrollArea::vertical()
                .id_salt(Area::Section(Text::Project))
                .max_height(height * TREE_SHARE)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let rows = project::Rows {
                        open,
                        unsaved: &|path| workbench.is_unsaved(path),
                        mark: theme.color(Token::StatusWarn),
                    };
                    jump = project::tree_ui(ui, workbench.tree(), &rows);
                });
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
    jump.or(symbols).map(LeftAction::Jump)
}

/// What a click in the left panel asks for.
enum LeftAction {
    /// Open a file at a place.
    Jump(Jump),
    /// Open this project folder.
    Open(PathBuf),
    /// Ask for a project folder with the system's dialog.
    OpenFolder,
    /// Make a new project from a template.
    New(Template),
}

/// The Project section with no project open: an Open Folder button, why the last one did not
/// open, and the recent projects, each a row that opens it, as Zed's welcome page offers them.
fn no_project(
    ui: &mut Ui,
    theme: &Theme,
    recent: &[PathBuf],
    error: Option<&str>,
) -> Option<LeftAction> {
    let mut action = None;
    note(ui, theme, Text::NoProject);
    ui.horizontal_wrapped(|ui| {
        if ui.button(Text::OpenFolderButton.get()).clicked() {
            action = Some(LeftAction::OpenFolder);
        }
        for (template, text) in [
            (Template::Id1, Text::NewId1Menu),
            (Template::Rerelease, Text::NewRereleaseMenu),
        ] {
            if ui.button(text.get()).clicked() {
                action = Some(LeftAction::New(template));
            }
        }
    });
    if let Some(error) = error {
        ui.add(
            egui::Label::new(RichText::new(error).color(theme.color(Token::StatusError))).wrap(),
        );
    }
    if recent.is_empty() {
        return action;
    }
    ui.add_space(theme.space(2));
    ui.label(
        RichText::new(Text::RecentProjects.get())
            .small()
            .color(theme.color(Token::TextMuted)),
    );
    for folder in recent {
        let name = project::root_name(folder);
        let row = ui
            .add(Button::new(name).frame_when_inactive(false))
            .on_hover_text(folder.to_string_lossy());
        if row.clicked() {
            action = Some(LeftAction::Open(folder.clone()));
        }
    }
    action
}

/// The editor's colours and font, from the theme's tokens.
pub(crate) fn view_style(theme: &Theme) -> ViewStyle {
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
        paused_line: theme.color(Token::DebugPausedLine),
        paused_mark: theme.color(Token::StatusWarn),
        breakpoint: theme.color(Token::StatusError),
    }
}

/// A line of muted text, wrapped to the panel's width.
fn note(ui: &mut Ui, theme: &Theme, text: Text) {
    ui.add(egui::Label::new(RichText::new(text.get()).color(theme.color(Token::TextMuted))).wrap());
}

/// What a click or a typed line in the bottom panel asks for.
enum BottomAction {
    /// Open the problem's place.
    Jump(Jump),
    /// What the Debugger tab or the console's box asked for.
    Debug(DebugAction),
}

/// The bottom panel: its tabs, and the Problems list, the console or the Debugger tab; what a
/// click in it asked for, if anything.
fn bottom(
    ui: &mut Ui,
    theme: &Theme,
    tab: &mut BottomTab,
    (workbench, preview, input): (&Workbench<FsLoader>, &Preview, &mut DebugInput),
    focus: bool,
) -> Option<BottomAction> {
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
            let response = ui.selectable_value(tab, value, label);
            if focus && *tab == value {
                response.request_focus();
            }
        }
    });
    ui.separator();
    match tab {
        BottomTab::Problems => {
            project::problems_ui(ui, theme, problems, workbench.root()).map(BottomAction::Jump)
        }
        BottomTab::Console if preview.state() != PlayState::Stopped => {
            debugger::console(ui, theme, preview, input).map(BottomAction::Debug)
        }
        BottomTab::Console => {
            note(ui, theme, Text::ConsoleIdle);
            None
        }
        BottomTab::Debugger => {
            debugger::debugger_ui(ui, theme, preview, input).map(BottomAction::Debug)
        }
    }
}

/// A line counted from 1, as the bridge and the debugger count them, counted from 0 as the
/// editor does.
fn line_index(line: u32) -> usize {
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
    if ui.add_enabled(state == PlayState::Playing, pause).clicked() {
        chosen = Some(PAUSE);
    }
    for (icon, text, id) in [
        (Icon::Step, Text::Step, STEP),
        (Icon::Restart, Text::RestartMap, RESTART),
    ] {
        let button = Button::new(labelled(icon, text)).frame(false);
        if ui.add_enabled(running, button).clicked() {
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
        .circle_filled(rect.center(), size / 4.0, colour);
}

/// How often a frame reads the running host's events.
const PREVIEW_POLL: std::time::Duration = std::time::Duration::from_millis(50);

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
