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
use crate::map_editor::{MapAction, MapEditor};
use crate::map_feature::MapFeature;
use crate::project::ProjectFeature;
use crate::project::{self, Jump, SymbolScope};
use crate::shell::{Area, FocusNow, SYMBOLS_SHARE, TREE_SHARE, heading, note, section};
use crate::shell::{BottomTab, CentreTab, ShellError, Workspace};
use crate::strings::Text;
use crate::terminal::TerminalFeature;
use crate::terminal::{TerminalAction, TerminalView, terminal_ui};
use crate::theme::{SECTION_GAP, Theme, Token};
use qs_layout::Panel;

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
    /// Draws the panels of the dock region `panel` from the workspace as the frame began, and
    /// returns what they asked for, in the order the shell carries it out.
    pub(crate) fn region_ui(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        panel: Panel,
        (workspace, (bottom_tab, scope), map_shown): (
            &Workspace,
            (&mut BottomTab, &mut SymbolScope),
            MapShown,
        ),
        focus: FocusNow,
    ) -> Vec<FeatureAction> {
        let workbench = &workspace.workbench;
        let preview = &workspace.preview;
        let map_editor = (map_shown == MapShown::Shown && workbench.map().is_some())
            .then_some(&mut self.map.map_editor);
        let mut jump = None;
        let mut map_actions = Vec::new();
        let mut entity_action = None;
        let mut tab_action = None;
        let mut agent_action = None;
        let mut open: Option<LeftAction> = None;
        match panel {
            Panel::Left => {
                let recent = (
                    self.project.recent_projects(),
                    self.project.error.as_deref(),
                );
                match left(ui, theme, (workbench, recent), scope, focus) {
                    Some(LeftAction::Jump(to)) => jump = Some(to),
                    asked => open = asked,
                }
                ui.add_space(theme.space(SECTION_GAP));
                if preview.state() == PlayState::Stopped {
                    section(ui, theme, Text::Entities, Text::NotPlaying);
                } else {
                    heading(ui, theme, Text::Entities);
                    entity_action =
                        entities::outliner(ui, theme, preview.entities(), &mut self.map.outliner);
                }
                if let Some(editor) = map_editor {
                    ui.add_space(theme.space(SECTION_GAP));
                    heading(ui, theme, Text::Map);
                    let selected = workbench.map().and_then(MapDocument::selected);
                    map_actions = editor.outline_ui(ui, theme, selected);
                }
            }
            Panel::Right => {
                let panels = (&mut self.map.properties, &mut self.agent.panel, map_editor);
                let asked = right(ui, theme, (workbench, preview), panels, focus);
                (entity_action, agent_action, map_actions) = asked;
            }
            Panel::Bottom => {
                match bottom(
                    ui,
                    theme,
                    bottom_tab,
                    (workbench, preview, &mut self.debugger.input),
                    (&mut self.terminal.view, &mut self.git.view),
                    focus,
                ) {
                    Some(BottomAction::Jump(to)) => jump = Some(to),
                    Some(asked) => tab_action = Some(asked),
                    None => {}
                }
            }
        }
        let mut actions: Vec<FeatureAction> = jump.map(FeatureAction::Jump).into_iter().collect();
        actions.push(FeatureAction::Map(map_actions));
        actions.extend(entity_action.map(FeatureAction::Entity));
        actions.extend(tab_action.map(FeatureAction::Bottom));
        actions.extend(agent_action.map(FeatureAction::Agent));
        actions.extend(open.map(FeatureAction::Left));
        actions
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
    /// Show the bottom panel's `tab`, opening the panel.
    ShowBottom(BottomTab),
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

    /// Clears the toolbar's notice.
    pub(crate) fn clear_notice(&mut self) {
        *self.notice = None;
    }

    /// Says on the toolbar that an edit failed.
    pub(crate) fn edit_failed(&mut self) {
        *self.notice = Some(Text::EditFailed.get().to_owned());
    }
}

/// The left panel's Project and Symbols sections; the row chosen, if any.
pub(crate) fn left(
    ui: &mut Ui,
    theme: &Theme,
    (workbench, (recent, error)): (&Workbench<FsLoader>, (&[PathBuf], Option<&str>)),
    scope: &mut SymbolScope,
    focus: FocusNow,
) -> Option<LeftAction> {
    let focus = focus == FocusNow::Take;
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
    ui.add_space(theme.space(SECTION_GAP));
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

/// The right panel: the Properties of the selected entity while the game runs, or the key
/// editor of the open map's while the Map tab shows it (P5-06), then the agent panel (P4-09);
/// what each asked for. Its first heading takes the keyboard when `focus`.
pub(crate) fn right(
    ui: &mut Ui,
    theme: &Theme,
    (workbench, preview): (&Workbench<FsLoader>, &Preview),
    (properties, agent, map_editor): (&mut Properties, &mut AgentPanel, Option<&mut MapEditor>),
    focus: FocusNow,
) -> (Option<EntityAction>, Option<AgentAction>, Vec<MapAction>) {
    let focus = focus == FocusNow::Take;
    let mut entity_action = None;
    let mut map_actions = Vec::new();
    let first = if let Some(map_editor) = map_editor {
        let first = heading(ui, theme, Text::Properties);
        let report = workbench.map().and_then(MapDocument::report);
        map_actions = map_editor.properties(ui, theme, report);
        first
    } else if preview.state() == PlayState::Stopped {
        section(ui, theme, Text::Properties, Text::NotPlaying)
    } else {
        let first = heading(ui, theme, Text::Properties);
        entity_action = entities::properties(ui, theme, preview, properties);
        first
    };
    if focus {
        first.request_focus();
    }
    ui.add_space(theme.space(SECTION_GAP));
    heading(ui, theme, Text::Agent);
    let agent_action = agent.ui(ui, theme, &workbench.staged_hunks());
    (entity_action, agent_action, map_actions)
}

/// The bottom panel: its tabs, and the Problems list, the console or the Debugger tab; what a
/// click in it asked for, if anything.
pub(crate) fn bottom(
    ui: &mut Ui,
    theme: &Theme,
    tab: &mut BottomTab,
    (workbench, preview, input): (&Workbench<FsLoader>, &Preview, &mut DebugInput),
    (terminal_view, git_view): (&mut TerminalView, &mut GitView),
    focus: FocusNow,
) -> Option<BottomAction> {
    let focus = focus == FocusNow::Take;
    let problems = workbench.problems();
    ui.horizontal(|ui| {
        for (value, text) in [
            (BottomTab::Problems, Text::Problems),
            (BottomTab::Console, Text::Console),
            (BottomTab::Debugger, Text::Debugger),
            (BottomTab::Terminal, Text::Terminal),
            (BottomTab::Git, Text::Git),
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
            if !problems.is_empty() && ui.small_button(Text::OpenAsMultibuffer.get()).clicked() {
                return Some(BottomAction::ProblemsAsExcerpts);
            }
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
        BottomTab::Terminal => {
            terminal_ui(ui, theme, workbench.terminals(), terminal_view).map(BottomAction::Terminal)
        }
        BottomTab::Git => {
            let open = workbench.open().map(|open| open.path().to_path_buf());
            git_ui(ui, theme, git_view, open.as_ref()).map(BottomAction::Git)
        }
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
