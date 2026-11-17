// SPDX-License-Identifier: GPL-3.0-or-later

//! The bottom panel's Git tab (P4-35, UI-19), as Zed's git panel and VS Code's Source Control
//! view lay it out: the branch, to switch or create one; each changed file with its hunks, each
//! staged or unstaged on its own, and a conflicted file's conflicts, each resolved to a side; the
//! commit message and Commit; and the open file's history and blame. Every change goes through
//! a `git.*` command; the tab shows what the last refresh read.
//!
//! Every string comes from [`crate::strings`]; a test fails on a string literal here.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText, Ui};
use qs_workbench::git::{
    ConflictIndex, ConflictSide, GIT_BLAME, GIT_BRANCHES, GIT_COMMIT, GIT_CONFLICTS,
    GIT_CREATE_BRANCH, GIT_DIFF, GIT_LOG, GIT_RESOLVE, GIT_STAGE, GIT_STATUS, GIT_SWITCH,
    GIT_UNSTAGE, GitBlameLine, GitBranch, GitBranchParams, GitChange, GitCommit, GitCommitParams,
    GitConflict, GitDiffParams, GitFile, GitHunk, GitLogParams, GitPathParams, GitResolveParams,
    GitSide, GitStageParams, GitStatus, HunkIndex,
};

use crate::features::{Commands, ShellRequest};
use crate::shell::name_field;
use crate::strings::{Text, conflict_at};
use crate::theme::{SECTION_GAP, Theme, Token};

/// How many of the open file's commits the Git tab lists.
const HISTORY: usize = 50;

/// How many characters of a commit id are shown.
const SHORT_ID: usize = 8;
/// The new branch name field's width.
const BRANCH_FIELD_WIDTH: f32 = 160.0;

/// What the last refresh read from the repository.
#[derive(Clone, Debug, Default)]
pub(crate) struct GitData {
    /// The branch and the changed files; `None` when the project is not in a repository.
    pub(crate) status: Option<GitStatus>,
    /// Each changed file's unstaged and staged hunks, by its full path.
    pub(crate) hunks: Vec<(PathBuf, Vec<GitHunk>, Vec<GitHunk>)>,
    /// Each conflicted file's conflicts.
    pub(crate) conflicts: Vec<(PathBuf, Vec<GitConflict>)>,
    /// The local branches.
    pub(crate) branches: Vec<GitBranch>,
    /// The open file's commits, the newest first.
    pub(crate) history: Vec<GitCommit>,
    /// The open file's blame, when it is shown.
    pub(crate) blame: Vec<GitBlameLine>,
    /// Why the last git command failed, if it did.
    pub(crate) error: Option<String>,
}

/// Whether the open file's blame is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BlameShown {
    /// Hidden.
    #[default]
    Hidden,
    /// Shown.
    Shown,
}

/// The Git tab's state.
#[derive(Debug, Default)]
pub(crate) struct GitView {
    /// What the last refresh read.
    pub(crate) data: GitData,
    /// The commit message being written.
    message: String,
    /// The name of a branch to create.
    new_branch: String,
    /// Whether the open file's blame is shown.
    pub(crate) blame: BlameShown,
    /// Whether the data is to be read again before it is next shown.
    pub(crate) stale: bool,
}

/// What a click in the Git tab asks for.
pub(crate) enum GitAction {
    /// Read the repository again.
    Refresh,
    /// Stage a file, or one of its unstaged hunks.
    Stage(PathBuf, Option<HunkIndex>),
    /// Unstage a file, or one of its staged hunks.
    Unstage(PathBuf, Option<HunkIndex>),
    /// Commit what is staged with this message.
    Commit(String),
    /// Check out this branch.
    Switch(String),
    /// Create this branch and check it out.
    CreateBranch(String),
    /// Resolve a file's conflict to a side.
    Resolve(PathBuf, ConflictIndex, ConflictSide),
    /// Show or hide the open file's blame.
    ToggleBlame,
    /// Open a file at a line, from 1.
    Open(PathBuf, usize),
}

/// The letter `git status` shows for `change`.
const fn letter(change: GitChange) -> char {
    match change {
        GitChange::None => ' ',
        GitChange::Modified => 'M',
        GitChange::Added => 'A',
        GitChange::Deleted => 'D',
        GitChange::Renamed => 'R',
        GitChange::Copied => 'C',
        GitChange::TypeChanged => 'T',
        GitChange::Untracked => '?',
        GitChange::Unmerged => 'U',
    }
}

/// The first `SHORT_ID` characters of `id`.
fn short(id: &str) -> &str {
    id.get(..SHORT_ID).unwrap_or(id)
}

/// A line of muted text.
fn muted(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.label(RichText::new(text).color(theme.color(Token::TextMuted)));
}

/// A heading within the tab.
fn heading(ui: &mut Ui, theme: &Theme, text: Text) {
    ui.add_space(theme.space(1));
    ui.label(RichText::new(text.get()).strong());
}

/// The Git tab: what a click asked for, if anything. `open` is the file in the editor.
pub(crate) fn git_ui(
    ui: &mut Ui,
    theme: &Theme,
    view: &mut GitView,
    open: Option<&PathBuf>,
) -> Option<GitAction> {
    let mut action = None;
    let GitView {
        data,
        message,
        new_branch,
        blame,
        ..
    } = view;
    let data = &*data;
    ui.horizontal(|ui| {
        if let Some(status) = &data.status {
            ui.label(Text::Branch.get());
            let current = status.branch.clone().unwrap_or_default();
            egui::ComboBox::from_id_salt(Text::Branch.get())
                .selected_text(current.as_str())
                .show_ui(ui, |ui| {
                    for branch in &data.branches {
                        if ui.selectable_label(branch.current, &branch.name).clicked()
                            && !branch.current
                        {
                            action = Some(GitAction::Switch(branch.name.clone()));
                        }
                    }
                });
            let typed = ui.add(
                egui::TextEdit::singleline(new_branch)
                    .hint_text(Text::NewBranchName.get())
                    .desired_width(BRANCH_FIELD_WIDTH),
            );
            name_field(ui, &typed, Text::NewBranchName);
            let name = new_branch.trim().to_owned();
            if ui
                .add_enabled(
                    !name.is_empty(),
                    egui::Button::new(Text::CreateBranch.get()),
                )
                .clicked()
            {
                action = Some(GitAction::CreateBranch(name));
                new_branch.clear();
            }
        }
        if ui.button(Text::Refresh.get()).clicked() {
            action = Some(GitAction::Refresh);
        }
    });
    if let Some(error) = &data.error {
        ui.label(RichText::new(error).color(theme.color(Token::StatusError)));
    }
    let Some(status) = &data.status else {
        muted(ui, theme, Text::NotARepository.get());
        return action;
    };
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            heading(ui, theme, Text::Changes);
            if status.files.is_empty() {
                muted(ui, theme, Text::NoChanges.get());
            }
            for file in &status.files {
                if let Some(asked) = file_ui(ui, theme, data, file) {
                    action = Some(asked);
                }
            }
            heading(ui, theme, Text::Commit);
            let typed = ui.add(
                egui::TextEdit::multiline(message)
                    .hint_text(Text::CommitMessage.get())
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            name_field(ui, &typed, Text::CommitMessage);
            let text = message.trim().to_owned();
            if ui
                .add_enabled(!text.is_empty(), egui::Button::new(Text::Commit.get()))
                .clicked()
            {
                action = Some(GitAction::Commit(text));
                message.clear();
            }
            if let Some(path) = open
                && let Some(asked) = history_ui(ui, theme, data, *blame, path)
            {
                action = Some(asked);
            }
        });
    action
}

/// One changed file: its status letters and path, Stage and Unstage, its hunks each with its
/// own, and its conflicts each with a side to keep.
fn file_ui(ui: &mut Ui, theme: &Theme, data: &GitData, file: &GitFile) -> Option<GitAction> {
    let mut action = None;
    let path = &file.full_path;
    ui.horizontal(|ui| {
        let letters = format!("{}{}", letter(file.staged), letter(file.unstaged));
        ui.label(
            RichText::new(letters)
                .monospace()
                .color(theme.color(Token::Accent)),
        );
        if ui.link(&file.path).clicked() {
            action = Some(GitAction::Open(path.clone(), 1));
        }
        if file.unstaged != GitChange::None && ui.button(Text::Stage.get()).clicked() {
            action = Some(GitAction::Stage(path.clone(), None));
        }
        if !matches!(file.staged, GitChange::None | GitChange::Untracked)
            && ui.button(Text::Unstage.get()).clicked()
        {
            action = Some(GitAction::Unstage(path.clone(), None));
        }
    });
    let hunks = data.hunks.iter().find(|(p, _, _)| p == path);
    if let Some((_, unstaged, staged)) = hunks {
        for (index, hunk) in unstaged.iter().enumerate() {
            if hunk_ui(ui, theme, hunk, Text::StageHunk) {
                action = Some(GitAction::Stage(path.clone(), Some(HunkIndex::new(index))));
            }
        }
        for (index, hunk) in staged.iter().enumerate() {
            if hunk_ui(ui, theme, hunk, Text::UnstageHunk) {
                action = Some(GitAction::Unstage(
                    path.clone(),
                    Some(HunkIndex::new(index)),
                ));
            }
        }
    }
    let conflicts = data.conflicts.iter().find(|(p, _)| p == path);
    for (index, conflict) in conflicts
        .map_or_default(|(_, c)| c.as_slice())
        .iter()
        .enumerate()
    {
        ui.horizontal(|ui| {
            ui.add_space(theme.space(2));
            if ui.link(conflict_at(conflict.start)).clicked() {
                action = Some(GitAction::Open(path.clone(), conflict.start));
            }
            for (side, text) in [
                (ConflictSide::Ours, Text::KeepOurs),
                (ConflictSide::Theirs, Text::KeepTheirs),
                (ConflictSide::Both, Text::KeepBoth),
            ] {
                if ui.button(text.get()).clicked() {
                    action = Some(GitAction::Resolve(
                        path.clone(),
                        ConflictIndex::new(index),
                        side,
                    ));
                }
            }
        });
    }
    action
}

/// One hunk: its `@@` line and its changed lines, and a button; whether it was clicked.
fn hunk_ui(ui: &mut Ui, theme: &Theme, hunk: &GitHunk, button: Text) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.add_space(theme.space(2));
        let header = hunk.text.lines().next().unwrap_or_default();
        ui.label(
            RichText::new(header)
                .monospace()
                .color(theme.color(Token::TextMuted)),
        );
        clicked = ui.small_button(button.get()).clicked();
    });
    for line in hunk.text.lines().skip(1) {
        let color = if line.starts_with('+') {
            theme.color(Token::StatusOk)
        } else if line.starts_with('-') {
            theme.color(Token::StatusError)
        } else {
            continue;
        };
        ui.horizontal(|ui| {
            ui.add_space(theme.space(SECTION_GAP));
            ui.label(RichText::new(line).monospace().color(color));
        });
    }
    clicked
}

/// The open file's history and, when shown, its blame; a click on a blamed line opens it.
fn history_ui(
    ui: &mut Ui,
    theme: &Theme,
    data: &GitData,
    blame: BlameShown,
    path: &Path,
) -> Option<GitAction> {
    let mut action = None;
    heading(ui, theme, Text::History);
    for commit in &data.history {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(short(&commit.id))
                    .monospace()
                    .color(theme.color(Token::TextMuted)),
            );
            ui.label(RichText::new(&commit.date).color(theme.color(Token::TextMuted)));
            ui.label(&commit.author);
            ui.label(&commit.summary);
        });
    }
    let toggle = match blame {
        BlameShown::Hidden => Text::ShowBlame,
        BlameShown::Shown => Text::HideBlame,
    };
    if ui.button(toggle.get()).clicked() {
        action = Some(GitAction::ToggleBlame);
    }
    if blame == BlameShown::Shown {
        for line in &data.blame {
            let row = ui.horizontal(|ui| {
                ui.label(
                    RichText::new(line.line.to_string())
                        .monospace()
                        .color(theme.color(Token::TextFaint)),
                );
                ui.label(
                    RichText::new(short(&line.commit))
                        .monospace()
                        .color(theme.color(Token::TextMuted)),
                );
                ui.label(&line.author);
                ui.label(RichText::new(&line.summary).color(theme.color(Token::TextMuted)));
            });
            if row.response.interact(egui::Sense::click()).clicked() {
                action = Some(GitAction::Open(path.to_path_buf(), line.line));
            }
        }
    }
    action
}

/// Whether the Git tab is on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabShown {
    /// Its tab is chosen and the bottom panel is open.
    Shown,
    /// It is not on screen.
    Hidden,
}

/// The Git tab's feature (P4-35): what it shows and the file it was last read for.
#[derive(Default)]
pub(crate) struct GitFeature {
    /// What the tab shows.
    pub(crate) view: GitView,
    /// The open file the repository was last read for.
    read_for: Option<PathBuf>,
}

impl GitFeature {
    /// Reads the repository again for the tab while it is `shown`, when an action made the
    /// last read stale or another file was opened; a hidden tab is read again when next shown.
    pub(crate) fn refresh(&mut self, commands: &mut Commands<'_>, shown: TabShown) {
        let open = commands
            .session()
            .context()
            .workbench
            .open()
            .map(|o| o.path().to_path_buf());
        if shown == TabShown::Hidden {
            self.view.stale = true;
            return;
        }
        if !self.view.stale && self.read_for == open {
            return;
        }
        self.view.stale = false;
        self.read_for.clone_from(&open);
        let blame = self.view.blame;
        self.view.data = GitData::read(commands, open.as_deref(), blame);
    }

    /// Runs what the tab asked for, through its command, then reads the repository again;
    /// the editor is then brought up to date, or a file opened at a line.
    pub(crate) fn apply(
        &mut self,
        action: GitAction,
        commands: &mut Commands<'_>,
        shown: TabShown,
    ) -> ShellRequest {
        let params =
            |value: Result<serde_json::Value, serde_json::Error>| value.unwrap_or_default();
        let ran = match action {
            GitAction::Refresh => Ok(serde_json::Value::Null),
            GitAction::Stage(path, hunk) => commands.execute(
                GIT_STAGE,
                params(serde_json::to_value(GitStageParams { path, hunk })),
            ),
            GitAction::Unstage(path, hunk) => commands.execute(
                GIT_UNSTAGE,
                params(serde_json::to_value(GitStageParams { path, hunk })),
            ),
            GitAction::Commit(message) => commands.execute(
                GIT_COMMIT,
                params(serde_json::to_value(GitCommitParams { message })),
            ),
            GitAction::Switch(branch) => commands.execute(
                GIT_SWITCH,
                params(serde_json::to_value(GitBranchParams { branch })),
            ),
            GitAction::CreateBranch(branch) => commands.execute(
                GIT_CREATE_BRANCH,
                params(serde_json::to_value(GitBranchParams { branch })),
            ),
            GitAction::Resolve(path, conflict, side) => commands.execute(
                GIT_RESOLVE,
                params(serde_json::to_value(GitResolveParams {
                    path,
                    conflict,
                    side,
                })),
            ),
            GitAction::ToggleBlame => {
                self.view.blame = match self.view.blame {
                    BlameShown::Hidden => BlameShown::Shown,
                    BlameShown::Shown => BlameShown::Hidden,
                };
                Ok(serde_json::Value::Null)
            }
            GitAction::Open(path, line) => {
                let line = u32::try_from(line).unwrap_or(u32::MAX);
                return ShellRequest::ShowLine { path, line };
            }
        };
        self.view.stale = true;
        self.refresh(commands, shown);
        if let Err(error) = ran {
            self.view.data.error = Some(error.to_string());
        }
        ShellRequest::RefreshEditor
    }
}

/// A `git.*` command's file parameter.
fn path_params(path: &Path) -> GitPathParams {
    GitPathParams {
        path: path.to_path_buf(),
    }
}

impl GitData {
    /// What the Git tab shows, read through the `git.*` commands (P4-35): the status, each changed
    /// file's hunks or a conflicted file's conflicts, the branches, and the open file's history and,
    /// when shown, its blame. Outside a repository there is no status and no error.
    fn read(commands: &mut Commands<'_>, open: Option<&Path>, blame: BlameShown) -> Self {
        let mut data = Self::default();
        let mut read = |id: &str, params: serde_json::Value| {
            commands
                .execute(id, params)
                .map_err(|error| error.to_string())
        };
        let status = read(GIT_STATUS, serde_json::json!({})).and_then(|v| {
            serde_json::from_value::<Option<GitStatus>>(v).map_err(|e| e.to_string())
        });
        let status = match status {
            Ok(Some(status)) => status,
            Ok(None) => return data,
            Err(error) => {
                data.error = Some(error);
                return data;
            }
        };
        let to = |params: Result<serde_json::Value, serde_json::Error>| params.unwrap_or_default();
        for file in &status.files {
            let path = &file.full_path;
            let conflicted =
                file.staged == GitChange::Unmerged || file.unstaged == GitChange::Unmerged;
            if conflicted {
                let found = read(GIT_CONFLICTS, to(serde_json::to_value(path_params(path))))
                    .ok()
                    .and_then(|v| serde_json::from_value::<Vec<GitConflict>>(v).ok())
                    .unwrap_or_default();
                data.conflicts.push((path.clone(), found));
                continue;
            }
            let mut hunks = |side: GitSide| {
                let params = GitDiffParams {
                    path: path.clone(),
                    side,
                };
                read(GIT_DIFF, to(serde_json::to_value(params)))
                    .ok()
                    .and_then(|v| serde_json::from_value::<Vec<GitHunk>>(v).ok())
                    .unwrap_or_default()
            };
            let unstaged = hunks(GitSide::Unstaged);
            let staged = hunks(GitSide::Staged);
            data.hunks.push((path.clone(), unstaged, staged));
        }
        data.status = Some(status);
        data.branches = read(GIT_BRANCHES, serde_json::json!({}))
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        if let Some(path) = open {
            let params = GitLogParams {
                path: path.to_path_buf(),
                limit: Some(HISTORY),
            };
            data.history = read(GIT_LOG, to(serde_json::to_value(params)))
                .ok()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();
            if blame == BlameShown::Shown {
                data.blame = read(GIT_BLAME, to(serde_json::to_value(path_params(path))))
                    .ok()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default();
            }
        }
        data
    }
}
