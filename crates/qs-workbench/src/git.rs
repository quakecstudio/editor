// SPDX-License-Identifier: GPL-3.0-or-later

//! Git in the window (P4-35, UI-19): the `git.*` commands over the open project's repository
//! (`qs-git`), so the window's Git tab, the CLI, the agent and MCP stage, commit and switch
//! branches the same way. A switch or a resolved conflict changes files on disk, and the open
//! project reads them again, as a restored checkpoint does.

use std::fmt;
use std::path::{Path, PathBuf};

use qc_project::normalize;
use qs_commands::{Command, NoParams, Outcome, RegisterError, Registry};
use qs_git::{Change, DiffOf, GitError, Repo, Side};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::editing::Source;
use crate::{HasWorkbench, Workbench, WorkbenchError, problems, tree};

/// The id of the command that lists the repository's changed files.
pub const GIT_STATUS: &str = "git.status";
/// The id of the command that lists a file's hunks.
pub const GIT_DIFF: &str = "git.diff";
/// The id of the command that stages a file or one of its hunks.
pub const GIT_STAGE: &str = "git.stage";
/// The id of the command that unstages a file or one of its hunks.
pub const GIT_UNSTAGE: &str = "git.unstage";
/// The id of the command that commits what is staged.
pub const GIT_COMMIT: &str = "git.commit";
/// The id of the command that lists the branches.
pub const GIT_BRANCHES: &str = "git.branches";
/// The id of the command that checks out a branch.
pub const GIT_SWITCH: &str = "git.switch";
/// The id of the command that creates a branch and checks it out.
pub const GIT_CREATE_BRANCH: &str = "git.create_branch";
/// The id of the command that lists a file's commits.
pub const GIT_LOG: &str = "git.log";
/// The id of the command that says who last changed each line of a file.
pub const GIT_BLAME: &str = "git.blame";
/// The id of the command that lists a file's merge conflicts.
pub const GIT_CONFLICTS: &str = "git.conflicts";
/// The id of the command that resolves a merge conflict to one side.
pub const GIT_RESOLVE: &str = "git.resolve";
/// The id of the command that gives a file's text at `HEAD`.
pub const GIT_HEAD_TEXT: &str = "git.head_text";

/// How many commits `git.log` lists when no limit is given.
pub const LOG_LIMIT: usize = 100;

/// Why a `git.*` command failed.
#[derive(Debug)]
pub enum GitCommandError {
    /// No project is open.
    NoProject,
    /// The project is not in a git repository.
    NoRepository,
    /// Git failed.
    Git(GitError),
    /// Files have unsaved changes, which a switch would leave behind.
    Unsaved(Vec<PathBuf>),
    /// The file has no conflict at this index.
    NoConflict(usize),
    /// Reading the project again failed.
    Workbench(WorkbenchError),
}

impl fmt::Display for GitCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoProject => f.write_str("no project is open"),
            Self::NoRepository => f.write_str("the project is not in a git repository"),
            Self::Git(error) => error.fmt(f),
            Self::Unsaved(paths) => {
                f.write_str("save these files first, which a switch would leave behind:")?;
                for path in paths {
                    write!(f, " {}", path.display())?;
                }
                Ok(())
            }
            Self::NoConflict(index) => write!(f, "the file has no conflict {index}"),
            Self::Workbench(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for GitCommandError {}

impl From<GitError> for GitCommandError {
    fn from(error: GitError) -> Self {
        Self::Git(error)
    }
}

/// How a file differs, as `git status` writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitChange {
    /// Unchanged.
    None,
    /// Modified.
    Modified,
    /// Added.
    Added,
    /// Deleted.
    Deleted,
    /// Renamed.
    Renamed,
    /// Copied.
    Copied,
    /// Its type changed.
    TypeChanged,
    /// Not tracked.
    Untracked,
    /// In a merge conflict.
    Unmerged,
}

impl From<Change> for GitChange {
    fn from(change: Change) -> Self {
        match change {
            Change::None => Self::None,
            Change::Modified => Self::Modified,
            Change::Added => Self::Added,
            Change::Deleted => Self::Deleted,
            Change::Renamed => Self::Renamed,
            Change::Copied => Self::Copied,
            Change::TypeChanged => Self::TypeChanged,
            Change::Untracked => Self::Untracked,
            Change::Unmerged => Self::Unmerged,
        }
    }
}

/// One changed file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GitFile {
    /// Its path in the repository, `/` between its parts.
    pub path: String,
    /// Its full path.
    pub full_path: PathBuf,
    /// How the index differs from `HEAD`: what is staged.
    pub staged: GitChange,
    /// How the working tree differs from the index: what is not.
    pub unstaged: GitChange,
    /// The line `git status --porcelain` writes for it.
    pub porcelain: String,
}

/// What `git.status` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GitStatus {
    /// The branch checked out; none when `HEAD` is detached.
    pub branch: Option<String>,
    /// The changed files, in git's order.
    pub files: Vec<GitFile>,
}

/// One hunk, as `git.diff` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GitHunk {
    /// Its first line in the old text, from 1.
    pub old_start: usize,
    /// Its first line in the new text, from 1.
    pub new_start: usize,
    /// Its text: the `@@` line and the lines with git's ` `, `-` and `+` marks.
    pub text: String,
}

/// Which changes of a file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GitSide {
    /// The working tree's changes, not staged.
    #[default]
    Unstaged,
    /// The index's changes, staged.
    Staged,
}

/// A file of the project.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitPathParams {
    /// The file.
    pub path: PathBuf,
}

/// The parameters of `git.diff`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitDiffParams {
    /// The file.
    pub path: PathBuf,
    /// Its unstaged changes, or its staged ones.
    #[serde(default)]
    pub side: GitSide,
}

/// The parameters of `git.stage` and `git.unstage`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitStageParams {
    /// The file.
    pub path: PathBuf,
    /// One hunk, by its index in `git.diff`'s list of the side it comes from; the whole file
    /// without it.
    #[serde(default)]
    pub hunk: Option<usize>,
}

/// The parameters of `git.commit`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitCommitParams {
    /// The commit message: a summary line, then a blank line and the body.
    pub message: String,
}

/// The parameters of `git.switch` and `git.create_branch`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitBranchParams {
    /// The branch.
    pub branch: String,
}

/// The parameters of `git.log`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitLogParams {
    /// The file.
    pub path: PathBuf,
    /// The most commits to list; 100 without it.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Which side of a conflict to keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConflictSide {
    /// Ours: the checked-out branch's lines.
    Ours,
    /// Theirs: the merged branch's lines.
    Theirs,
    /// Ours, then theirs.
    Both,
}

/// The parameters of `git.resolve`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitResolveParams {
    /// The file.
    pub path: PathBuf,
    /// The conflict, by its index in `git.conflicts`' list.
    pub conflict: usize,
    /// The side to keep.
    pub side: ConflictSide,
}

/// A branch, as `git.branches` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GitBranch {
    /// Its name.
    pub name: String,
    /// Whether it is checked out.
    pub current: bool,
}

/// A commit, as `git.log` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GitCommit {
    /// Its full id.
    pub id: String,
    /// Its author.
    pub author: String,
    /// Its author date, `YYYY-MM-DD`.
    pub date: String,
    /// The first line of its message.
    pub summary: String,
}

/// One line's blame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GitBlameLine {
    /// The line, from 1.
    pub line: usize,
    /// The commit's full id, all zeros for a line not committed.
    pub commit: String,
    /// Its author.
    pub author: String,
    /// The first line of its message.
    pub summary: String,
}

/// A conflict, as `git.conflicts` lists it: its marker lines, from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GitConflict {
    /// The `<<<<<<<` line.
    pub start: usize,
    /// The `=======` line.
    pub middle: usize,
    /// The `>>>>>>>` line.
    pub end: usize,
}

impl<L: qc_project::Loader> Workbench<L> {
    /// The open project's repository.
    fn repo(&self) -> Result<Repo, GitCommandError> {
        let root = self.root().ok_or(GitCommandError::NoProject)?;
        Repo::discover(root)?.ok_or(GitCommandError::NoRepository)
    }

    /// Reads the open tabs that have no unsaved edits again from disk, and the project's files,
    /// after git or another program changed them.
    pub(crate) fn disk_changed(&mut self) -> Result<(), WorkbenchError> {
        let root = self.root.clone().ok_or(WorkbenchError::NoProject)?;
        for path in self.tabs.clone() {
            if self.unsaved.contains(&path) {
                continue;
            }
            if let Ok(text) = std::fs::read(&path)
                && self.host.text(&path).as_deref() != Some(text.as_slice())
            {
                self.set_text(&path, text, Source::Elsewhere)?;
                self.host
                    .remove_overlay(&path)
                    .map_err(WorkbenchError::Project)?;
                self.unsaved.remove(&path);
            }
        }
        self.host.reload().map_err(WorkbenchError::Project)?;
        self.tree = tree(&self.host.files(), &root);
        self.problems = problems(&mut self.host);
        self.symbols = self.host.workspace_symbols();
        Ok(())
    }
}

/// The open project's repository, through `context`.
fn repo<C: HasWorkbench>(context: &mut C) -> Result<Repo, GitCommandError> {
    context.workbench().repo()
}

/// Declares a command over the repository: its struct, id, summary, parameters, output and
/// body, which has `context` and `params` and returns the output.
macro_rules! git_command {
    (
        $name:ident, $id:expr, $summary:expr, $params:ty, $output:ty,
        |$context:ident, $params_name:ident| $body:block
    ) => {
        #[doc = concat!("`", stringify!($name), "`.")]
        struct $name;

        impl<C: HasWorkbench + 'static> Command<C> for $name {
            const ID: &'static str = $id;
            const SUMMARY: &'static str = $summary;
            type Params = $params;
            type Output = $output;
            type Error = GitCommandError;

            fn run(
                &self,
                $context: &mut C,
                $params_name: $params,
            ) -> Result<Outcome<$output, C>, GitCommandError> {
                let output: $output = $body;
                Ok(Outcome::outside_history(output))
            }
        }
    };
}

git_command!(
    StatusCommand,
    GIT_STATUS,
    "Lists the project repository's branch and changed files, staged and not, as git status \
     does; none when the project is not in a git repository.",
    NoParams,
    Option<GitStatus>,
    |context, _params| {
        let repo = match repo(context) {
            Ok(repo) => repo,
            Err(GitCommandError::NoRepository) => return Ok(Outcome::outside_history(None)),
            Err(error) => return Err(error),
        };
        let status = repo.status()?;
        Some(GitStatus {
            branch: status.branch,
            files: status
                .files
                .iter()
                .map(|file| GitFile {
                    porcelain: file.porcelain(),
                    full_path: normalize(&repo.root().join(&file.path)),
                    path: file.path.clone(),
                    staged: file.index.into(),
                    unstaged: file.worktree.into(),
                })
                .collect(),
        })
    }
);

git_command!(
    DiffCommand,
    GIT_DIFF,
    "Lists the hunks of a file's unstaged or staged changes.",
    GitDiffParams,
    Vec<GitHunk>,
    |context, params| {
        let of = match params.side {
            GitSide::Unstaged => DiffOf::Unstaged,
            GitSide::Staged => DiffOf::Staged,
        };
        repo(context)?
            .diff(&params.path, of)?
            .hunks
            .into_iter()
            .map(|hunk| GitHunk {
                old_start: hunk.old_start,
                new_start: hunk.new_start,
                text: hunk.text,
            })
            .collect()
    }
);

git_command!(
    StageCommand,
    GIT_STAGE,
    "Stages a file, or one hunk of its unstaged changes.",
    GitStageParams,
    (),
    |context, params| {
        let repo = repo(context)?;
        match params.hunk {
            Some(hunk) => repo.stage_hunk(&params.path, hunk)?,
            None => repo.stage(&params.path)?,
        }
    }
);

git_command!(
    UnstageCommand,
    GIT_UNSTAGE,
    "Unstages a file, or one hunk of its staged changes.",
    GitStageParams,
    (),
    |context, params| {
        let repo = repo(context)?;
        match params.hunk {
            Some(hunk) => repo.unstage_hunk(&params.path, hunk)?,
            None => repo.unstage(&params.path)?,
        }
    }
);

git_command!(
    CommitCommand,
    GIT_COMMIT,
    "Commits what is staged with a message; the new commit's id.",
    GitCommitParams,
    String,
    |context, params| { repo(context)?.commit(&params.message)? }
);

git_command!(
    BranchesCommand,
    GIT_BRANCHES,
    "Lists the repository's local branches, the checked-out one marked.",
    NoParams,
    Vec<GitBranch>,
    |context, _params| {
        repo(context)?
            .branches()?
            .into_iter()
            .map(|branch| GitBranch {
                name: branch.name,
                current: branch.current,
            })
            .collect()
    }
);

/// Refuses a switch while files have unsaved edits, which git cannot see.
fn no_unsaved<C: HasWorkbench>(context: &mut C) -> Result<(), GitCommandError> {
    let unsaved: Vec<PathBuf> = context.workbench().unsaved.iter().cloned().collect();
    if unsaved.is_empty() {
        Ok(())
    } else {
        Err(GitCommandError::Unsaved(unsaved))
    }
}

git_command!(
    SwitchCommand,
    GIT_SWITCH,
    "Checks out a branch, as git switch does, and reads the project's files again; refused \
     while files have unsaved edits.",
    GitBranchParams,
    (),
    |context, params| {
        no_unsaved(context)?;
        repo(context)?.switch(&params.branch)?;
        context
            .workbench()
            .disk_changed()
            .map_err(GitCommandError::Workbench)?;
    }
);

git_command!(
    CreateBranchCommand,
    GIT_CREATE_BRANCH,
    "Creates a branch at the current commit and checks it out, as git switch -c does.",
    GitBranchParams,
    (),
    |context, params| {
        repo(context)?.create_branch(&params.branch)?;
    }
);

git_command!(
    LogCommand,
    GIT_LOG,
    "Lists the commits that changed a file, the newest first, following renames.",
    GitLogParams,
    Vec<GitCommit>,
    |context, params| {
        repo(context)?
            .log(&params.path, params.limit.unwrap_or(LOG_LIMIT))?
            .into_iter()
            .map(|commit| GitCommit {
                id: commit.id,
                author: commit.author,
                date: commit.date,
                summary: commit.summary,
            })
            .collect()
    }
);

git_command!(
    BlameCommand,
    GIT_BLAME,
    "Says who last changed each line of a file, and in which commit.",
    GitPathParams,
    Vec<GitBlameLine>,
    |context, params| {
        repo(context)?
            .blame(&params.path)?
            .into_iter()
            .map(|line| GitBlameLine {
                line: line.line,
                commit: line.commit,
                author: line.author,
                summary: line.summary,
            })
            .collect()
    }
);

/// The current text of `path`: the editor's when it has unsaved edits, else the disk's, which
/// is what git wrote.
fn current_text<C: HasWorkbench>(context: &mut C, path: &Path) -> Result<String, GitCommandError> {
    let workbench = context.workbench();
    let bytes = if workbench.unsaved.contains(&normalize(path)) {
        workbench.text(path)
    } else {
        std::fs::read(path).ok().or_else(|| workbench.text(path))
    };
    bytes
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .ok_or_else(|| {
            GitCommandError::Workbench(WorkbenchError::NoFile {
                path: path.to_path_buf(),
            })
        })
}

git_command!(
    ConflictsCommand,
    GIT_CONFLICTS,
    "Lists a file's merge conflicts: where each one's markers are.",
    GitPathParams,
    Vec<GitConflict>,
    |context, params| {
        let text = current_text(context, &params.path)?;
        qs_git::conflicts(&text)
            .into_iter()
            .map(|c| GitConflict {
                start: c.start.saturating_add(1),
                middle: c.middle.saturating_add(1),
                end: c.end.saturating_add(1),
            })
            .collect()
    }
);

git_command!(
    ResolveCommand,
    GIT_RESOLVE,
    "Resolves one of a file's merge conflicts by keeping our side, theirs or both, and saves \
     the file.",
    GitResolveParams,
    (),
    |context, params| {
        let text = current_text(context, &params.path)?;
        let side = match params.side {
            ConflictSide::Ours => Side::Ours,
            ConflictSide::Theirs => Side::Theirs,
            ConflictSide::Both => Side::Both,
        };
        let resolved = qs_git::resolve(&text, params.conflict, side)
            .ok_or(GitCommandError::NoConflict(params.conflict))?;
        let workbench = context.workbench();
        workbench
            .set_text(&params.path, resolved.into_bytes(), Source::Elsewhere)
            .and_then(|()| workbench.save(&params.path))
            .map_err(GitCommandError::Workbench)?;
    }
);

git_command!(
    HeadTextCommand,
    GIT_HEAD_TEXT,
    "Gives a file's text at HEAD, which the editor marks its changes against; none for a file \
     HEAD does not have.",
    GitPathParams,
    Option<String>,
    |context, params| { repo(context)?.head_text(&params.path)? }
);

/// Registers the `git.*` commands.
///
/// # Errors
///
/// [`RegisterError`] when an id is already registered.
pub fn register_git<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(StatusCommand)?;
    registry.register(DiffCommand)?;
    registry.register(StageCommand)?;
    registry.register(UnstageCommand)?;
    registry.register(CommitCommand)?;
    registry.register(BranchesCommand)?;
    registry.register(SwitchCommand)?;
    registry.register(CreateBranchCommand)?;
    registry.register(LogCommand)?;
    registry.register(BlameCommand)?;
    registry.register(ConflictsCommand)?;
    registry.register(ResolveCommand)?;
    registry.register(HeadTextCommand)
}

#[cfg(test)]
mod tests;
