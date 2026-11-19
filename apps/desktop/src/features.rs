// SPDX-License-Identifier: GPL-3.0-or-later

//! The window's features (P6-41): each owns its state and the glue between its panel and its
//! commands, so the shell names none of them. A feature's panel draws from the state the frame
//! began with and returns what was asked; the feature acts after, through [`Commands`], and asks
//! the window or another feature for anything else with a [`ShellRequest`], which the shell
//! carries out at once and in order.

use std::path::PathBuf;

use eframe::egui::{self, Button, RichText, Ui};
use qs_commands::{CommandError, Session};
use qs_map_edit::MapDocument;
use qs_templates::Template;
use qs_workbench::{FsLoader, OpenFile, PlayState, Preview, Workbench};

use crate::agent::AgentFeature;
use crate::agent::{AgentAction, AgentPanel};
use crate::debugger::DebuggerFeature;
use crate::debugger::{self, DebugAction, DebugInput};
use crate::editors::EditorsFeature;
use crate::entities::{self, EntityAction, Properties};
use crate::git_panel::GitFeature;
use crate::git_panel::{GitAction, GitView, git_ui};
use crate::help::Topic;
use crate::icons::Icon;
use crate::map_editor::{MapAction, MapEditor};
use crate::map_feature::MapFeature;
use crate::project::ProjectFeature;
use crate::project::{self, Jump, SymbolScope};
use crate::shell::{Area, FocusNow, heading_with_help, note, section};
use crate::shell::{CentreTab, ShellError, Workspace};
use crate::strings::Text;
use crate::terminal::TerminalFeature;
use crate::terminal::{TerminalAction, TerminalView, terminal_ui};
use crate::theme::{Theme, Token};
use qs_layout::{DockPanel, Panel};

/// The features, each with its state.
pub(crate) struct Features {
    /// The agent panel and an inline edit's answer (P4-09, P4-10).
    pub(crate) agent: AgentFeature,
    /// The project: recent projects, a new project, export, settings and the game guide.
    pub(crate) project: ProjectFeature,
    /// The editors, the picker and the multibuffer.
    pub(crate) editors: EditorsFeature,
    /// The map view and editor, the outliner and the Properties panel.
    pub(crate) map: MapFeature,
    /// The Git tab (P4-35).
    pub(crate) git: GitFeature,
    /// The Terminal tab (P4-36).
    pub(crate) terminal: TerminalFeature,
    /// The Debugger tab and the console's box (P3-16).
    pub(crate) debugger: DebuggerFeature,
}

impl Features {
    /// The features, the agent's over `agent`.
    pub(crate) fn new(agent: AgentPanel) -> Self {
        Self {
            agent: AgentFeature::new(agent),
            project: ProjectFeature::default(),
            editors: EditorsFeature::default(),
            map: MapFeature::default(),
            git: GitFeature::default(),
            terminal: TerminalFeature::default(),
            debugger: DebuggerFeature::default(),
        }
    }
}

/// Whether the map tab is shown in the centre, which puts the map's outline and keys in the
/// dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MapShown {
    /// The centre shows the map.
    Shown,
    /// The centre shows another tab.
    Hidden,
}

/// What a dock region is drawn from: the workspace as the frame began, and the window's
/// choices its panels may change.
pub(crate) struct Dock<'a> {
    /// The workspace, read as the frame began.
    pub(crate) workspace: &'a Workspace,
    /// Which symbols the Symbols section lists.
    pub(crate) scope: &'a mut SymbolScope,
    /// Whether the centre shows the map.
    pub(crate) map_shown: MapShown,
}

/// What a dock region's panel asked for, carried out by the shell after the frame drew it.
pub(crate) enum FeatureAction {
    /// Open a file at a place.
    Jump(Jump),
    /// Edits of the open map.
    Map(Vec<MapAction>),
    /// What the Entities outliner or the Properties panel asked for.
    Entity(EntityAction),
    /// What a bottom tab asked for.
    Bottom(BottomAction),
    /// What the agent panel asked for.
    Agent(AgentAction),
    /// What the Project panel asked for.
    Left(LeftAction),
}

impl Features {
    /// Draws the panel the dock `panel` shows (P7-15), from the workspace as the frame began,
    /// and returns what it asked for, in the order the shell carries it out.
    pub(crate) fn region_ui(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        panel: Panel,
        dock: Dock<'_>,
        focus: FocusNow,
    ) -> Vec<FeatureAction> {
        match dock.workspace.layout.docks.shown(panel) {
            Some(shown) => self.panel_ui(ui, theme, shown, dock, focus),
            None => Vec::new(),
        }
    }

    /// Draws the panel `shown`; its heading takes the keyboard when `focus` says so.
    fn panel_ui(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        shown: DockPanel,
        dock: Dock<'_>,
        focus: FocusNow,
    ) -> Vec<FeatureAction> {
        let Dock {
            workspace,
            scope,
            map_shown,
        } = dock;
        let take = focus == FocusNow::Take;
        let workbench = &workspace.workbench;
        let preview = &workspace.preview;
        let map_editor = (map_shown == MapShown::Shown && workbench.map().is_some())
            .then_some(&mut self.map.map_editor);
        let left = |asked: LeftAction| match asked {
            LeftAction::Jump(to) => FeatureAction::Jump(to),
            asked => FeatureAction::Left(asked),
        };
        match shown {
            DockPanel::Project => {
                let recent = (
                    self.project.recent_projects(),
                    self.project.error.as_deref(),
                );
                project_panel(ui, theme, (workbench, recent), focus)
                    .map(left)
                    .into_iter()
                    .collect()
            }
            DockPanel::Symbols => {
                let heading = heading_with_help(ui, theme, Text::Symbols, Topic::Symbols);
                if take {
                    heading.request_focus();
                }
                let height = ui.available_height();
                project::symbols_ui(
                    ui,
                    theme,
                    scope,
                    workbench.open(),
                    workbench.project_symbols(),
                    height,
                )
                .map(FeatureAction::Jump)
                .into_iter()
                .collect()
            }
            DockPanel::Entities => {
                let heading = if preview.state() == PlayState::Stopped {
                    section(ui, theme, Topic::Entities, Text::NotPlaying)
                } else {
                    let heading = heading_with_help(ui, theme, Text::Entities, Topic::Entities);
                    let asked =
                        entities::outliner(ui, theme, preview.entities(), &mut self.map.outliner);
                    if take {
                        heading.request_focus();
                    }
                    return asked.map(FeatureAction::Entity).into_iter().collect();
                };
                if take {
                    heading.request_focus();
                }
                Vec::new()
            }
            DockPanel::Map => match map_editor {
                Some(editor) => {
                    let heading = heading_with_help(ui, theme, Text::Map, Topic::MapOutline);
                    if take {
                        heading.request_focus();
                    }
                    let selected = workbench.map().and_then(MapDocument::selected);
                    vec![FeatureAction::Map(editor.outline_ui(ui, theme, selected))]
                }
                None => {
                    let heading = section(ui, theme, Topic::MapOutline, Text::MapOutlineIdle);
                    if take {
                        heading.request_focus();
                    }
                    Vec::new()
                }
            },
            DockPanel::Properties => {
                let (entity, map) = properties_panel(
                    ui,
                    theme,
                    (workbench, preview),
                    (&mut self.map.properties, map_editor),
                    focus,
                );
                let mut actions = vec![FeatureAction::Map(map)];
                actions.extend(entity.map(FeatureAction::Entity));
                actions
            }
            DockPanel::Agent => {
                let heading = heading_with_help(ui, theme, Text::Agent, Topic::Agent);
                if take {
                    heading.request_focus();
                }
                self.agent
                    .panel
                    .ui(ui, theme, &workbench.staged_hunks())
                    .map(FeatureAction::Agent)
                    .into_iter()
                    .collect()
            }
            DockPanel::Problems
            | DockPanel::Console
            | DockPanel::Debugger
            | DockPanel::Terminal
            | DockPanel::Git => {
                let asked = bottom_panel(
                    ui,
                    theme,
                    shown,
                    (workbench, preview, &mut self.debugger.input),
                    (&mut self.terminal.view, &mut self.git.view),
                    focus,
                );
                match asked {
                    Some(BottomAction::Jump(to)) => vec![FeatureAction::Jump(to)],
                    Some(asked) => vec![FeatureAction::Bottom(asked)],
                    None => Vec::new(),
                }
            }
        }
    }
}

/// What a feature asks of the window or of another feature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShellRequest {
    /// Open a file at a place, as a panel's row does.
    Jump(Jump),
    /// Open `path` in the editor with its caret at `line`, from 1.
    ShowLine {
        /// The file.
        path: PathBuf,
        /// The line, from 1.
        line: u32,
    },
    /// Open `file`, relative to the project's directory, at `line`, from 1.
    ShowProjectLine {
        /// The file, relative to the project's directory.
        file: PathBuf,
        /// The line, from 1.
        line: u32,
    },
    /// Bring the editor up to date with the open file, its change marks included, after an
    /// action that may have changed it on disk.
    RefreshEditor,
    /// Show `panel` in its dock, opening the dock (P7-15).
    ShowPanel(DockPanel),
    /// Show the centre's `tab`.
    ShowCentre(CentreTab),
    /// Open the guide to the game's files when some are missing (P3-21).
    OpenGameGuide,
}

/// The commands a feature runs, and the toolbar's notice it may set.
pub(crate) struct Commands<'a> {
    session: &'a mut Session<Workspace>,
    notice: &'a mut Option<String>,
}

impl<'a> Commands<'a> {
    /// The commands of `session`, and the toolbar's `notice`.
    pub(crate) const fn new(
        session: &'a mut Session<Workspace>,
        notice: &'a mut Option<String>,
    ) -> Self {
        Self { session, notice }
    }

    /// The session, to read the workspace.
    pub(crate) const fn session(&self) -> &Session<Workspace> {
        self.session
    }

    /// The session, for a feature that runs commands through its own model, as the agent does.
    pub(crate) const fn session_mut(&mut self) -> &mut Session<Workspace> {
        self.session
    }

    /// Runs the command `id` with `params`, logging a failure; its output, `None` when it
    /// failed.
    pub(crate) fn run<P: serde::Serialize, O: serde::de::DeserializeOwned>(
        &mut self,
        id: &str,
        params: &P,
    ) -> Option<O> {
        match self.attempt(id, params) {
            Ok(output) => Some(output),
            Err(error) => {
                tracing::warn!(%error, command = id, "command failed");
                None
            }
        }
    }

    /// Runs the command `id` with `params`: its output, or why it failed.
    pub(crate) fn attempt<P: serde::Serialize, O: serde::de::DeserializeOwned>(
        &mut self,
        id: &str,
        params: &P,
    ) -> Result<O, ShellError> {
        serde_json::to_value(params)
            .map_err(ShellError::from)
            .and_then(|params| Ok(self.session.execute(id, params)?))
            .and_then(|output| Ok(serde_json::from_value(output)?))
    }

    /// Runs the command `id` with `params` as JSON.
    pub(crate) fn execute(
        &mut self,
        id: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CommandError> {
        self.session.execute(id, params)
    }

    /// Puts `notice` on the toolbar, or clears it.
    pub(crate) fn set_notice(&mut self, notice: Option<String>) {
        *self.notice = notice;
    }

    /// Says on the toolbar that an edit failed.
    pub(crate) fn edit_failed(&mut self) {
        *self.notice = Some(Text::EditFailed.get().to_owned());
    }
}

/// A dock panel's status-bar icon (P7-15).
pub(crate) const fn panel_icon(panel: DockPanel) -> Icon {
    match panel {
        DockPanel::Project => Icon::Project,
        DockPanel::Symbols => Icon::Symbols,
        DockPanel::Entities => Icon::Entities,
        DockPanel::Map => Icon::MapOutline,
        DockPanel::Properties => Icon::Properties,
        DockPanel::Agent => Icon::Agent,
        DockPanel::Problems => Icon::Problems,
        DockPanel::Console => Icon::Console,
        DockPanel::Debugger => Icon::Debugger,
        DockPanel::Terminal => Icon::Terminal,
        DockPanel::Git => Icon::Git,
    }
}

/// What a dock panel is: its title and its help.
pub(crate) const fn panel_topic(panel: DockPanel) -> (Text, Topic) {
    match panel {
        DockPanel::Project => (Text::Project, Topic::Project),
        DockPanel::Symbols => (Text::Symbols, Topic::Symbols),
        DockPanel::Entities => (Text::Entities, Topic::Entities),
        DockPanel::Map => (Text::Map, Topic::MapOutline),
        DockPanel::Properties => (Text::Properties, Topic::Properties),
        DockPanel::Agent => (Text::Agent, Topic::Agent),
        DockPanel::Problems => (Text::Problems, Topic::Problems),
        DockPanel::Console => (Text::Console, Topic::Console),
        DockPanel::Debugger => (Text::Debugger, Topic::Debugger),
        DockPanel::Terminal => (Text::Terminal, Topic::Terminal),
        DockPanel::Git => (Text::Git, Topic::Git),
    }
}

/// The Project panel: the open project's files, or how to open one; the row chosen, if any.
/// Its heading takes the keyboard when `focus` says so.
pub(crate) fn project_panel(
    ui: &mut Ui,
    theme: &Theme,
    (workbench, (recent, error)): (&Workbench<FsLoader>, (&[PathBuf], Option<&str>)),
    focus: FocusNow,
) -> Option<LeftAction> {
    let project = heading_with_help(ui, theme, Text::Project, Topic::Project);
    if focus == FocusNow::Take {
        project.request_focus();
    }
    let Some(root) = workbench.root() else {
        return no_project(ui, theme, recent, error);
    };
    ui.label(RichText::new(project::root_name(root)).strong());
    let open = workbench.open().map(OpenFile::path);
    egui::ScrollArea::vertical()
        .id_salt(Area::Section(Text::Project))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let rows = project::Rows {
                open,
                unsaved: &|path| workbench.is_unsaved(path),
                mark: theme.color(Token::StatusWarn),
                review: &|path| workbench.awaits_review(path),
                review_mark: theme.color(Token::Accent),
            };
            project::tree_ui(ui, workbench.tree(), &rows)
        })
        .inner
        .map(LeftAction::Jump)
}

/// The Properties panel: the selected entity's while the game runs, or the key editor of the
/// open map's while the Map tab shows it (P5-06); what it asked for. Its heading takes the
/// keyboard when `take`.
pub(crate) fn properties_panel(
    ui: &mut Ui,
    theme: &Theme,
    (workbench, preview): (&Workbench<FsLoader>, &Preview),
    (properties, map_editor): (&mut Properties, Option<&mut MapEditor>),
    focus: FocusNow,
) -> (Option<EntityAction>, Vec<MapAction>) {
    let mut entity_action = None;
    let mut map_actions = Vec::new();
    let first = if let Some(map_editor) = map_editor {
        let first = heading_with_help(ui, theme, Text::Properties, Topic::MapProperties);
        let report = workbench.map().and_then(MapDocument::report);
        map_actions = map_editor.properties(ui, theme, report);
        first
    } else if preview.state() == PlayState::Stopped {
        section(ui, theme, Topic::Properties, Text::NotPlaying)
    } else {
        let first = heading_with_help(ui, theme, Text::Properties, Topic::Properties);
        entity_action = entities::properties(ui, theme, preview, properties);
        first
    };
    if focus == FocusNow::Take {
        first.request_focus();
    }
    (entity_action, map_actions)
}

/// A bottom panel, `shown`: the Problems list, the console, the Debugger, the terminals or Git,
/// under its heading and help; what a click in it asked for, if anything.
pub(crate) fn bottom_panel(
    ui: &mut Ui,
    theme: &Theme,
    shown: DockPanel,
    (workbench, preview, input): (&Workbench<FsLoader>, &Preview, &mut DebugInput),
    (terminal_view, git_view): (&mut TerminalView, &mut GitView),
    focus: FocusNow,
) -> Option<BottomAction> {
    let problems = workbench.problems();
    let (title, topic) = panel_topic(shown);
    let heading = heading_with_help(ui, theme, title, topic);
    if focus == FocusNow::Take {
        heading.request_focus();
    }
    match shown {
        DockPanel::Problems => {
            if !problems.is_empty() && ui.small_button(Text::OpenAsMultibuffer.get()).clicked() {
                return Some(BottomAction::ProblemsAsExcerpts);
            }
            project::problems_ui(ui, theme, problems, workbench.root()).map(BottomAction::Jump)
        }
        DockPanel::Console if preview.state() != PlayState::Stopped => {
            debugger::console(ui, theme, preview, input).map(BottomAction::Debug)
        }
        DockPanel::Console => {
            note(ui, theme, Text::ConsoleIdle);
            None
        }
        DockPanel::Debugger => {
            debugger::debugger_ui(ui, theme, preview, input).map(BottomAction::Debug)
        }
        DockPanel::Terminal => {
            terminal_ui(ui, theme, workbench.terminals(), terminal_view).map(BottomAction::Terminal)
        }
        DockPanel::Git => {
            let open = workbench.open().map(|open| open.path().to_path_buf());
            git_ui(ui, theme, git_view, open.as_ref()).map(BottomAction::Git)
        }
        DockPanel::Project
        | DockPanel::Symbols
        | DockPanel::Entities
        | DockPanel::Map
        | DockPanel::Properties
        | DockPanel::Agent => None,
    }
}

/// The Project section with no project open: an Open Folder button, why the last one did not
/// open, and the recent projects, each a row that opens it, as Zed's welcome page offers them.
pub(crate) fn no_project(
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

/// What a click in the left panel asks for.
pub(crate) enum LeftAction {
    /// Open a file at a place.
    Jump(Jump),
    /// Open this project folder.
    Open(PathBuf),
    /// Ask for a project folder with the system's dialog.
    OpenFolder,
    /// Make a new project from a template.
    New(Template),
}

/// What a click or a typed line in the bottom panel asks for.
pub(crate) enum BottomAction {
    /// Open the problem's place.
    Jump(Jump),
    /// What the Debugger tab or the console's box asked for.
    Debug(DebugAction),
    /// What the Terminal tab asked for.
    Terminal(TerminalAction),
    /// What the Git tab asked for.
    Git(GitAction),
    /// Show every problem as excerpts in the multibuffer (P4-33).
    ProblemsAsExcerpts,
}
