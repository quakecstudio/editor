// SPDX-License-Identifier: GPL-3.0-or-later

//! Staged edits (P4-04, AGT-3): a change to several files, staged as hunks that are accepted
//! or rejected one by one. While a hunk waits, the semantic model reads the file with it, so
//! the problems, a build and every language feature see the change before anyone accepts it;
//! nothing is written to disk. A rejected hunk puts back the lines it replaced, byte for byte,
//! and a file whose hunks are all rejected is again exactly the text it had before. Once each
//! hunk of a file is decided, its accepted hunks are the editor's text of it, unsaved, as a
//! rename's are: `file.save` writes it.
//!
//! The hunks are the runs of changed lines of [`qs_diff::script`], Myers' diff, the same the
//! editor shows inline.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};

use qc_project::{Loader, normalize};
use qs_commands::{Command, NoParams, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::editing::Source;
use crate::{HasWorkbench, Workbench, WorkbenchError};

/// The id of the command that stages a change to several files as hunks.
pub const APPLY_EDIT: &str = "edit.apply";

/// The id of the command that accepts staged hunks.
pub const ACCEPT_HUNKS: &str = "edit.accept";

/// The id of the command that rejects staged hunks.
pub const REJECT_HUNKS: &str = "edit.reject";

/// The id of the command that lists the hunks waiting for a decision.
pub const STAGED_EDITS: &str = "edit.staged";

/// A staged hunk's number, unique while the workbench is open.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct HunkId(u32);

impl HunkId {
    /// The number.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for HunkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "hunk {}", self.0)
    }
}

/// What was decided about a hunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Its lines stay.
    Accept,
    /// The lines it replaced come back.
    Reject,
}

/// Where a hunk stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Waiting,
    Decided(Decision),
}

/// One hunk of a staged file: lines `old` of its text before replaced by lines `new` of the
/// text proposed.
#[derive(Clone, Debug)]
struct Hunk {
    id: HunkId,
    old: Range<usize>,
    new: Range<usize>,
    state: State,
}

/// A file with staged hunks.
#[derive(Clone, Debug)]
struct StagedFile {
    /// The file's text before, in lines that keep their `\n`, so joining them gives it back.
    before: Vec<Vec<u8>>,
    /// The text proposed, the same way.
    proposed: Vec<Vec<u8>>,
    /// Whether the file had an editor's text before it was staged; if not, rejecting every
    /// hunk leaves it with none, read from disk again.
    had_text: bool,
    hunks: Vec<Hunk>,
}

impl StagedFile {
    /// The file's text with each hunk that is not rejected: what the semantic model reads.
    fn text(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut at = 0;
        for hunk in &self.hunks {
            let (lines, range) = if hunk.state == State::Decided(Decision::Reject) {
                (&self.before, hunk.old.clone())
            } else {
                (&self.proposed, hunk.new.clone())
            };
            for line in self.before.get(at..hunk.old.start).unwrap_or_default() {
                out.extend_from_slice(line);
            }
            for line in lines.get(range).unwrap_or_default() {
                out.extend_from_slice(line);
            }
            at = hunk.old.end;
        }
        for line in self.before.get(at..).unwrap_or_default() {
            out.extend_from_slice(line);
        }
        out
    }

    /// Whether every hunk is decided.
    fn is_decided(&self) -> bool {
        self.hunks.iter().all(|hunk| hunk.state != State::Waiting)
    }

    /// Whether a hunk was accepted.
    fn any_accepted(&self) -> bool {
        self.hunks
            .iter()
            .any(|hunk| hunk.state == State::Decided(Decision::Accept))
    }
}

/// `text` in lines, each with its `\n`; the last without one when the text does not end in
/// one.
fn lines(text: &[u8]) -> Vec<Vec<u8>> {
    text.split_inclusive(|byte| *byte == b'\n')
        .map(<[u8]>::to_vec)
        .collect()
}

/// `lines` joined, as text a person reads.
fn joined(lines: &[Vec<u8>]) -> String {
    String::from_utf8_lossy(&lines.concat()).into_owned()
}

/// The staged edits of a workbench.
#[derive(Clone, Debug, Default)]
pub(crate) struct Staged {
    files: BTreeMap<PathBuf, StagedFile>,
    next: u32,
}

impl Staged {
    /// Forgets `path`'s hunks, when an editor's text of it takes their place.
    pub(crate) fn forget(&mut self, path: &Path) {
        self.files.remove(path);
    }
}

/// A staged hunk, as it is listed for review.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StagedHunk {
    /// The hunk's number, which `edit.accept` and `edit.reject` take.
    pub id: HunkId,
    /// The file.
    pub path: PathBuf,
    /// The first line it replaces in the file's text before, counting from 1; for a hunk that
    /// only adds lines, the line they go before.
    pub line: usize,
    /// The lines it removes, each with its line feed.
    pub removed: String,
    /// The lines it adds.
    pub added: String,
}

/// One file's proposed text, as `edit.apply` takes it.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposedFile {
    /// The file; one not in the project, or not on disk, is a new file.
    pub path: PathBuf,
    /// Its whole new text.
    pub text: String,
}

/// The parameters of `edit.apply`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyEditParams {
    /// Each file's proposed text.
    pub files: Vec<ProposedFile>,
}

/// The parameters of `edit.accept` and `edit.reject`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HunksParams {
    /// The hunks, by number.
    pub hunks: Vec<HunkId>,
}

impl<L: Loader> Workbench<L> {
    /// Stages each `(path, text)` of `files` as hunks against the file's current text, and lets
    /// the semantic model read the file with them; nothing is written to disk. A file staged
    /// already is staged again against its text from before, its earlier hunks dropped; a file
    /// whose text is unchanged has no hunks. Returns every hunk waiting, in path order.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::Project`] when a project file's proposed text does not open; the
    /// files before it in path order stay staged.
    pub fn stage(
        &mut self,
        files: Vec<(PathBuf, Vec<u8>)>,
    ) -> Result<Vec<StagedHunk>, WorkbenchError> {
        let mut sorted: BTreeMap<PathBuf, Vec<u8>> = BTreeMap::new();
        for (path, text) in files {
            sorted.insert(normalize(&path), text);
        }
        for (path, text) in sorted {
            let (before, had_text) = match self.staged.files.remove(&path) {
                Some(file) => (file.before, file.had_text),
                None => (
                    lines(&self.host.text(&path).unwrap_or_default()),
                    self.unsaved.contains(&path),
                ),
            };
            let proposed = lines(&text);
            let mut hunks = Vec::new();
            for hunk in qs_diff::hunks(&qs_diff::script(&before, &proposed)) {
                let id = HunkId(self.staged.next);
                self.staged.next = self.staged.next.wrapping_add(1);
                hunks.push(Hunk {
                    id,
                    old: hunk.old,
                    new: hunk.new,
                    state: State::Waiting,
                });
            }
            let file = StagedFile {
                before,
                proposed,
                had_text,
                hunks,
            };
            let text = file.text();
            if file.hunks.is_empty() {
                // Nothing changed: the file keeps the text it had, and staging it again
                // starts from that.
                self.finish(&path, &file)?;
                continue;
            }
            self.staged.files.insert(path.clone(), file);
            self.set_text(&path, text, Source::Elsewhere)?;
        }
        Ok(self.staged_hunks())
    }

    /// Every hunk waiting for a decision, in path order and, in a file, in line order.
    #[must_use]
    pub fn staged_hunks(&self) -> Vec<StagedHunk> {
        let mut out = Vec::new();
        for (path, file) in &self.staged.files {
            for hunk in file
                .hunks
                .iter()
                .filter(|hunk| hunk.state == State::Waiting)
            {
                out.push(StagedHunk {
                    id: hunk.id,
                    path: path.clone(),
                    line: hunk.old.start.saturating_add(1),
                    removed: joined(file.before.get(hunk.old.clone()).unwrap_or_default()),
                    added: joined(file.proposed.get(hunk.new.clone()).unwrap_or_default()),
                });
            }
        }
        out
    }

    /// Accepts or rejects each hunk of `ids`, as `decision` says, and gives the semantic
    /// model each file's text with the hunks not rejected. A file whose hunks are all decided
    /// leaves the staging: its text, with the accepted hunks, is the editor's, unsaved; with
    /// none accepted, it is exactly its text from before. Returns every hunk still waiting.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoHunk`] when a number is not a hunk waiting, before anything is
    /// decided; [`WorkbenchError::Project`] when a project file's new text does not open.
    pub fn decide(
        &mut self,
        ids: &[HunkId],
        decision: Decision,
    ) -> Result<Vec<StagedHunk>, WorkbenchError> {
        let waiting = |id: HunkId| {
            self.staged.files.iter().find_map(|(path, file)| {
                file.hunks
                    .iter()
                    .any(|hunk| hunk.id == id && hunk.state == State::Waiting)
                    .then(|| path.clone())
            })
        };
        let mut touched = Vec::new();
        for &id in ids {
            let path = waiting(id).ok_or(WorkbenchError::NoHunk { id })?;
            if !touched.contains(&path) {
                touched.push(path);
            }
        }
        for file in self.staged.files.values_mut() {
            for hunk in &mut file.hunks {
                if ids.contains(&hunk.id) {
                    hunk.state = State::Decided(decision);
                }
            }
        }
        touched.sort();
        for path in touched {
            let Some(file) = self.staged.files.get(&path) else {
                continue;
            };
            if file.is_decided() {
                if let Some(file) = self.staged.files.remove(&path) {
                    self.finish(&path, &file)?;
                }
            } else {
                let text = file.text();
                self.set_text(&path, text, Source::Elsewhere)?;
            }
        }
        Ok(self.staged_hunks())
    }

    /// Gives `path` the text of `file`, every hunk of which is decided: the editor's text when
    /// a hunk was accepted or the file had one before, else none, so it is read from disk.
    fn finish(&mut self, path: &Path, file: &StagedFile) -> Result<(), WorkbenchError> {
        self.set_text(path, file.text(), Source::Elsewhere)?;
        if !file.had_text && !file.any_accepted() {
            self.host
                .remove_overlay(path)
                .map_err(WorkbenchError::Project)?;
            self.unsaved.remove(path);
        }
        Ok(())
    }
}

/// `edit.apply`: stages a change to several files as hunks.
struct ApplyEdit;

impl<C: HasWorkbench + 'static> Command<C> for ApplyEdit {
    const ID: &'static str = APPLY_EDIT;
    const SUMMARY: &'static str = "Stages a change to several files as hunks to accept or \
        reject one by one; the project is checked with them, and nothing is written to disk.";
    type Params = ApplyEditParams;
    type Output = Vec<StagedHunk>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: ApplyEditParams,
    ) -> Result<Outcome<Vec<StagedHunk>, C>, WorkbenchError> {
        let files = params
            .files
            .into_iter()
            .map(|file| (file.path, file.text.into_bytes()))
            .collect();
        Ok(Outcome::outside_history(context.workbench().stage(files)?))
    }
}

/// `edit.accept`: accepts staged hunks.
struct AcceptHunks;

impl<C: HasWorkbench + 'static> Command<C> for AcceptHunks {
    const ID: &'static str = ACCEPT_HUNKS;
    const SUMMARY: &'static str = "Accepts staged hunks: their lines stay in the editor's text, \
        unsaved, once each hunk of their file is decided.";
    type Params = HunksParams;
    type Output = Vec<StagedHunk>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: HunksParams,
    ) -> Result<Outcome<Vec<StagedHunk>, C>, WorkbenchError> {
        let left = context
            .workbench()
            .decide(&params.hunks, Decision::Accept)?;
        Ok(Outcome::outside_history(left))
    }
}

/// `edit.reject`: rejects staged hunks.
struct RejectHunks;

impl<C: HasWorkbench + 'static> Command<C> for RejectHunks {
    const ID: &'static str = REJECT_HUNKS;
    const SUMMARY: &'static str = "Rejects staged hunks: the lines they replaced come back, \
        byte for byte.";
    type Params = HunksParams;
    type Output = Vec<StagedHunk>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: HunksParams,
    ) -> Result<Outcome<Vec<StagedHunk>, C>, WorkbenchError> {
        let left = context
            .workbench()
            .decide(&params.hunks, Decision::Reject)?;
        Ok(Outcome::outside_history(left))
    }
}

/// `edit.staged`: the hunks waiting.
struct StagedEdits;

impl<C: HasWorkbench + 'static> Command<C> for StagedEdits {
    const ID: &'static str = STAGED_EDITS;
    const SUMMARY: &'static str = "Lists the staged hunks waiting to be accepted or rejected.";
    type Params = NoParams;
    type Output = Vec<StagedHunk>;
    type Error = std::convert::Infallible;

    fn run(
        &self,
        context: &mut C,
        _params: NoParams,
    ) -> Result<Outcome<Vec<StagedHunk>, C>, Self::Error> {
        Ok(Outcome::unchanged(context.workbench().staged_hunks()))
    }
}

/// Registers `edit.apply`, `edit.accept`, `edit.reject` and `edit.staged`.
///
/// # Errors
///
/// [`RegisterError`] when one of their ids is taken.
pub fn register_staged<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(ApplyEdit)?;
    registry.register(AcceptHunks)?;
    registry.register(RejectHunks)?;
    registry.register(StagedEdits)
}

#[cfg(test)]
mod tests;
