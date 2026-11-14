// SPDX-License-Identifier: GPL-3.0-or-later

//! Git in the window (P4-35, UI-19): a project's status, the hunks of each change to stage and
//! unstage, commits, branches, a file's history and blame, and its merge conflicts resolved one
//! side at a time.
//!
//! The model is Zed's git panel and VS Code's Source Control view: every operation is the git
//! command line's, run in the repository and read through its machine-readable output
//! (`git status --porcelain`, `git diff`, `git blame --porcelain`, `git for-each-ref`), so what
//! the window shows is what `git status` says. A hunk is staged as `git add -p` stages one: its
//! patch applied to the index (`git apply --cached`), and unstaged by applying it in reverse.
//! Merge conflicts are git's markers in the file's text (`<<<<<<<`, `|||||||`, `=======`,
//! `>>>>>>>`), which [`conflicts`] finds and [`resolve`] replaces with a side.
//!
//! # Layer
//!
//! L0, beside `qs-checkpoint`: git's formats and its command line, with no other crate.

mod conflict;
mod diff;
#[cfg(test)]
mod tests;

use std::ffi::OsStr;
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

pub use conflict::{Conflict, ConflictIndex, Side, conflicts, resolve};
pub use diff::{FileDiff, Hunk, HunkIndex};

/// Why a git operation failed.
#[derive(Debug)]
pub enum GitError {
    /// The `git` program could not be run: it is not installed or not on the path.
    NoGit(String),
    /// A git command ended with an error: the command and what git wrote to stderr.
    Failed {
        /// The arguments after `git`.
        command: String,
        /// What git wrote to stderr, trimmed.
        message: String,
    },
    /// Git's output was not what the command gives.
    Unexpected(String),
    /// The file has no hunk at this index.
    NoHunk(HunkIndex),
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoGit(error) => write!(f, "git could not be run: {error}"),
            Self::Failed { command, message } => write!(f, "git {command}: {message}"),
            Self::Unexpected(what) => write!(f, "git's output was not as expected: {what}"),
            Self::NoHunk(index) => write!(f, "the file has no hunk {}", index.get()),
        }
    }
}

impl std::error::Error for GitError {}

/// How a file differs, in the index or the working tree, as `git status --porcelain` says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
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

impl Change {
    /// The change one of `git status --porcelain`'s two columns writes as `code`.
    const fn of(code: u8) -> Option<Self> {
        Some(match code {
            b' ' => Self::None,
            b'M' => Self::Modified,
            b'A' => Self::Added,
            b'D' => Self::Deleted,
            b'R' => Self::Renamed,
            b'C' => Self::Copied,
            b'T' => Self::TypeChanged,
            b'?' => Self::Untracked,
            b'U' => Self::Unmerged,
            _ => return None,
        })
    }

    /// The letter `git status --porcelain` writes for it.
    #[must_use]
    pub const fn code(self) -> char {
        match self {
            Self::None => ' ',
            Self::Modified => 'M',
            Self::Added => 'A',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::Copied => 'C',
            Self::TypeChanged => 'T',
            Self::Untracked => '?',
            Self::Unmerged => 'U',
        }
    }
}

/// One file `git status` lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStatus {
    /// Its path in the repository, `/` between its parts.
    pub path: String,
    /// How the index differs from `HEAD`.
    pub index: Change,
    /// How the working tree differs from the index.
    pub worktree: Change,
}

impl FileStatus {
    /// The line `git status --porcelain` writes for it, without a rename's old path.
    #[must_use]
    pub fn porcelain(&self) -> String {
        format!(
            "{}{} {}",
            self.index.code(),
            self.worktree.code(),
            self.path
        )
    }

    /// Whether both sides changed it in a merge.
    #[must_use]
    pub const fn is_conflicted(&self) -> bool {
        matches!(self.index, Change::Unmerged) || matches!(self.worktree, Change::Unmerged)
    }
}

/// A repository's status: its branch and the files that differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// The branch checked out; `None` when `HEAD` is detached.
    pub branch: Option<String>,
    /// The files that differ, in git's order.
    pub files: Vec<FileStatus>,
}

/// A branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    /// Its name.
    pub name: String,
    /// Whether it is checked out.
    pub current: bool,
}

/// A commit in a file's history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    /// Its full id.
    pub id: String,
    /// Its author's name.
    pub author: String,
    /// Its author date, `YYYY-MM-DD`.
    pub date: String,
    /// The first line of its message.
    pub summary: String,
}

/// Who last changed one line, as `git blame` says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlameLine {
    /// The line, from 1.
    pub line: usize,
    /// The commit's full id; all zeros for a line not yet committed.
    pub commit: String,
    /// Its author's name.
    pub author: String,
    /// The first line of its message.
    pub summary: String,
}

/// Which side of the index a diff is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffOf {
    /// The working tree against the index: what is not staged.
    Unstaged,
    /// The index against `HEAD`: what is staged.
    Staged,
}

/// A git repository's working tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    root: PathBuf,
}

impl Repo {
    /// The repository whose working tree holds `dir`, if any.
    ///
    /// # Errors
    ///
    /// Git could not be run.
    pub fn discover(dir: &Path) -> Result<Option<Self>, GitError> {
        let output = run_in(dir, &["rev-parse", "--show-toplevel"], None)?;
        if !output.status.success() {
            return Ok(None);
        }
        let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        Ok(Some(Self {
            root: PathBuf::from(root),
        }))
    }

    /// The working tree's root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The path of `path`, a file under the root, as git names it: relative, `/` between its
    /// parts.
    ///
    /// # Errors
    ///
    /// [`GitError::Unexpected`] when the path is not under the root.
    pub fn relative(&self, path: &Path) -> Result<String, GitError> {
        let relative = if path.is_absolute() {
            let root = self
                .root
                .canonicalize()
                .unwrap_or_else(|_| self.root.clone());
            let full = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            full.strip_prefix(&root)
                .map(Path::to_path_buf)
                .map_err(|_| GitError::Unexpected(format!("{} is outside", path.display())))?
        } else {
            path.to_path_buf()
        };
        let parts: Vec<String> = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        Ok(parts.join("/"))
    }

    /// Runs git with `args` in the root, `input` on its stdin; its stdout when it succeeds.
    fn git(&self, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, GitError> {
        let output = run_in(&self.root, args, input)?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(GitError::Failed {
                command: args.join(" "),
                message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }

    /// The branch and the files that differ, as `git status --porcelain` lists them, untracked
    /// files one by one.
    ///
    /// # Errors
    ///
    /// Git failed or its output was not the porcelain format.
    pub fn status(&self) -> Result<Status, GitError> {
        let out = self.git(
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            None,
        )?;
        let mut files = Vec::new();
        let mut entries = out.split(|b| *b == 0).filter(|e| !e.is_empty());
        while let Some(entry) = entries.next() {
            let (Some(x), Some(y), Some(path)) = (entry.first(), entry.get(1), entry.get(3..))
            else {
                return Err(GitError::Unexpected(String::from_utf8_lossy(entry).into()));
            };
            let (Some(index), Some(worktree)) = (Change::of(*x), Change::of(*y)) else {
                return Err(GitError::Unexpected(String::from_utf8_lossy(entry).into()));
            };
            // A rename or copy is followed by its old path, as an entry of its own.
            if matches!(index, Change::Renamed | Change::Copied) {
                entries.next();
            }
            files.push(FileStatus {
                path: String::from_utf8_lossy(path).into_owned(),
                index,
                worktree,
            });
        }
        let branch = String::from_utf8_lossy(&self.git(&["branch", "--show-current"], None)?)
            .trim()
            .to_owned();
        Ok(Status {
            branch: (!branch.is_empty()).then_some(branch),
            files,
        })
    }

    /// The hunks of `path`'s unstaged or staged changes.
    ///
    /// # Errors
    ///
    /// Git failed, or its diff could not be read.
    pub fn diff(&self, path: &Path, of: DiffOf) -> Result<FileDiff, GitError> {
        let relative = self.relative(path)?;
        let mut args = vec!["diff", "--no-color", "--no-ext-diff", "-U3"];
        if of == DiffOf::Staged {
            args.push("--cached");
        }
        args.extend(["--", relative.as_str()]);
        let out = self.git(&args, None)?;
        FileDiff::parse(&String::from_utf8_lossy(&out))
    }

    /// Stages hunk `index` of `path`'s unstaged changes, as `git add -p` would.
    ///
    /// # Errors
    ///
    /// [`GitError::NoHunk`] when there is no such hunk, or git refused the patch.
    pub fn stage_hunk(&self, path: &Path, index: HunkIndex) -> Result<(), GitError> {
        let diff = self.diff(path, DiffOf::Unstaged)?;
        let patch = diff.patch(index).ok_or(GitError::NoHunk(index))?;
        self.git(&["apply", "--cached", "-"], Some(patch.as_bytes()))
            .map(drop)
    }

    /// Unstages hunk `index` of `path`'s staged changes.
    ///
    /// # Errors
    ///
    /// [`GitError::NoHunk`] when there is no such hunk, or git refused the patch.
    pub fn unstage_hunk(&self, path: &Path, index: HunkIndex) -> Result<(), GitError> {
        let diff = self.diff(path, DiffOf::Staged)?;
        let patch = diff.patch(index).ok_or(GitError::NoHunk(index))?;
        self.git(
            &["apply", "--cached", "--reverse", "-"],
            Some(patch.as_bytes()),
        )
        .map(drop)
    }

    /// Stages all of `path`: its changes, or the file when it is new or deleted.
    ///
    /// # Errors
    ///
    /// Git failed.
    pub fn stage(&self, path: &Path) -> Result<(), GitError> {
        let relative = self.relative(path)?;
        self.git(&["add", "--all", "--", relative.as_str()], None)
            .map(drop)
    }

    /// Unstages all of `path`, as `git restore --staged` does.
    ///
    /// # Errors
    ///
    /// Git failed.
    pub fn unstage(&self, path: &Path) -> Result<(), GitError> {
        let relative = self.relative(path)?;
        // A repository with no commit yet has no HEAD to restore from: the file leaves the
        // index instead.
        if self
            .git(&["rev-parse", "--verify", "-q", "HEAD"], None)
            .is_err()
        {
            return self
                .git(&["rm", "--cached", "-q", "--", relative.as_str()], None)
                .map(drop);
        }
        self.git(&["restore", "--staged", "--", relative.as_str()], None)
            .map(drop)
    }

    /// Commits what is staged with `message`; the new commit's id.
    ///
    /// # Errors
    ///
    /// Git failed: nothing is staged, the message is empty, or no author is configured.
    pub fn commit(&self, message: &str) -> Result<String, GitError> {
        self.git(&["commit", "-q", "-F", "-"], Some(message.as_bytes()))?;
        let id = self.git(&["rev-parse", "HEAD"], None)?;
        Ok(String::from_utf8_lossy(&id).trim().to_owned())
    }

    /// The local branches, by name.
    ///
    /// # Errors
    ///
    /// Git failed.
    pub fn branches(&self) -> Result<Vec<Branch>, GitError> {
        let out = self.git(
            &[
                "for-each-ref",
                "--format=%(HEAD)%00%(refname:short)",
                "refs/heads",
            ],
            None,
        )?;
        Ok(String::from_utf8_lossy(&out)
            .lines()
            .filter_map(|line| line.split_once('\0'))
            .map(|(head, name)| Branch {
                name: name.to_owned(),
                current: head == "*",
            })
            .collect())
    }

    /// Checks out branch `name`, as `git switch` does: it refuses when local changes would be
    /// lost.
    ///
    /// # Errors
    ///
    /// Git failed.
    pub fn switch(&self, name: &str) -> Result<(), GitError> {
        self.git(&["switch", "-q", "--", name], None).map(drop)
    }

    /// Creates branch `name` at `HEAD` and checks it out, as `git switch -c` does.
    ///
    /// # Errors
    ///
    /// Git failed: the name is taken or not a valid branch name.
    pub fn create_branch(&self, name: &str) -> Result<(), GitError> {
        self.git(&["switch", "-q", "-c", name], None).map(drop)
    }

    /// The last `limit` commits that changed `path`, the newest first, following renames.
    ///
    /// # Errors
    ///
    /// Git failed.
    pub fn log(&self, path: &Path, limit: usize) -> Result<Vec<Commit>, GitError> {
        let relative = self.relative(path)?;
        let count = format!("-n{limit}");
        let out = self.git(
            &[
                "log",
                "--follow",
                count.as_str(),
                "--format=%H%x00%an%x00%as%x00%s",
                "--",
                relative.as_str(),
            ],
            None,
        )?;
        let text = String::from_utf8_lossy(&out);
        text.lines()
            .map(|line| {
                let mut parts = line.splitn(4, '\0');
                match (parts.next(), parts.next(), parts.next(), parts.next()) {
                    (Some(id), Some(author), Some(date), Some(summary)) => Ok(Commit {
                        id: id.to_owned(),
                        author: author.to_owned(),
                        date: date.to_owned(),
                        summary: summary.to_owned(),
                    }),
                    _ => Err(GitError::Unexpected(line.to_owned())),
                }
            })
            .collect()
    }

    /// Who last changed each line of `path` as it is in the working tree.
    ///
    /// # Errors
    ///
    /// Git failed: the file is not tracked, for one.
    pub fn blame(&self, path: &Path) -> Result<Vec<BlameLine>, GitError> {
        let relative = self.relative(path)?;
        let out = self.git(&["blame", "--porcelain", "--", relative.as_str()], None)?;
        parse_blame(&String::from_utf8_lossy(&out))
    }

    /// `path`'s text at `HEAD`, for the editor's gutter marks; `None` when `HEAD` has no such
    /// file, as for a new file or a repository with no commit.
    ///
    /// # Errors
    ///
    /// Git could not be run.
    pub fn head_text(&self, path: &Path) -> Result<Option<String>, GitError> {
        let relative = self.relative(path)?;
        let spec = format!("HEAD:{relative}");
        let output = run_in(&self.root, &["show", spec.as_str()], None)?;
        Ok(output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned()))
    }
}

/// Reads `git blame --porcelain`: each line's header names its commit, whose author and
/// summary are given the first time it appears.
fn parse_blame(text: &str) -> Result<Vec<BlameLine>, GitError> {
    let mut known: Vec<(String, String, String)> = Vec::new();
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(header) = lines.next() {
        let mut words = header.split(' ');
        let (Some(commit), Some(_), Some(final_line)) = (words.next(), words.next(), words.next())
        else {
            return Err(GitError::Unexpected(header.to_owned()));
        };
        let line: usize = final_line
            .parse()
            .map_err(|_| GitError::Unexpected(header.to_owned()))?;
        let (mut author, mut summary) = (None, None);
        for field in lines.by_ref() {
            if field.starts_with('\t') {
                break;
            }
            if let Some(name) = field.strip_prefix("author ") {
                author = Some(name.to_owned());
            } else if let Some(text) = field.strip_prefix("summary ") {
                summary = Some(text.to_owned());
            }
        }
        if let (Some(author), Some(summary)) = (&author, &summary) {
            known.push((commit.to_owned(), author.clone(), summary.clone()));
        }
        let (author, summary) = known
            .iter()
            .find(|(id, _, _)| id == commit)
            .map(|(_, author, summary)| (author.clone(), summary.clone()))
            .unwrap_or_default();
        out.push(BlameLine {
            line,
            commit: commit.to_owned(),
            author,
            summary,
        });
    }
    Ok(out)
}

/// Runs git with `args` in `dir`, `input` on its stdin, in the C locale with no pager, prompt
/// or colour, paths written as they are.
fn run_in(dir: &Path, args: &[&str], input: Option<&[u8]>) -> Result<Output, GitError> {
    let mut command = Command::new("git");
    command
        .args(["-c", "core.quotepath=off", "-c", "color.ui=false"])
        .args(args.iter().map(OsStr::new))
        .current_dir(dir)
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    let no_git = |error: io::Error| GitError::NoGit(error.to_string());
    let mut child = command.spawn().map_err(no_git)?;
    if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(bytes).map_err(no_git)?;
    }
    child.wait_with_output().map_err(no_git)
}
