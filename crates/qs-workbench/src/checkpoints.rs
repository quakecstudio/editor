// SPDX-License-Identifier: GPL-3.0-or-later

//! Checkpoints of the open project (P4-05, AGT-4): `checkpoint.create` records every file of
//! the project as git objects on a private ref (`qs-checkpoint`), as the agent does before each
//! run; `checkpoint.list` lists them; `checkpoint.restore` puts every file back as a checkpoint
//! holds it, deletions included, and drops the editor's unsaved text of the project, so the
//! IDE and the disk are both exactly as they were.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use qc_project::{Loader, normalize};
use qs_checkpoint::{Checkpoint, Restored, Store};
use qs_commands::{Command, NoParams, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::editing::Source;
use crate::{HasWorkbench, Workbench, WorkbenchError, problems, tree};

/// The id of the command that records a checkpoint of the open project.
pub const CREATE_CHECKPOINT: &str = "checkpoint.create";

/// The id of the command that lists the open project's checkpoints.
pub const LIST_CHECKPOINTS: &str = "checkpoint.list";

/// The id of the command that puts the open project back as a checkpoint holds it.
pub const RESTORE_CHECKPOINT: &str = "checkpoint.restore";

/// A checkpoint, as the commands answer it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CheckpointMade {
    /// Its number, which `checkpoint.restore` takes.
    pub number: u32,
    /// Its git commit's id, in the store.
    pub commit: String,
    /// What it was made for.
    pub label: String,
    /// How many files it holds.
    pub files: usize,
}

impl From<Checkpoint> for CheckpointMade {
    fn from(checkpoint: Checkpoint) -> Self {
        Self {
            number: checkpoint.number,
            commit: checkpoint.commit,
            label: checkpoint.label,
            files: checkpoint.files,
        }
    }
}

/// What `checkpoint.restore` changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CheckpointRestored {
    /// The files written back, by path under the project.
    pub written: Vec<PathBuf>,
    /// The files removed, made after the checkpoint.
    pub removed: Vec<PathBuf>,
    /// The files whose unsaved text in the editor was dropped.
    pub discarded: Vec<PathBuf>,
}

/// The parameters of `checkpoint.create`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCheckpointParams {
    /// What the checkpoint is for, such as the agent run's task; `checkpoint` when absent.
    #[serde(default)]
    pub label: Option<String>,
}

/// The parameters of `checkpoint.restore`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreCheckpointParams {
    /// The checkpoint's number.
    pub number: u32,
}

impl<L: Loader> Workbench<L> {
    /// The checkpoint store of the open project.
    fn store(&self) -> Result<Store, WorkbenchError> {
        let root = self.root.as_deref().ok_or(WorkbenchError::NoProject)?;
        Store::open(root).map_err(WorkbenchError::Checkpoint)
    }

    /// Records every file of the open project, as it is on disk, as the next checkpoint, with
    /// `label` and `time` (seconds since 1970, UTC).
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoProject`], or [`WorkbenchError::Checkpoint`] when a file cannot be
    /// read or the store written.
    pub fn create_checkpoint(
        &mut self,
        label: &str,
        time: i64,
    ) -> Result<CheckpointMade, WorkbenchError> {
        let store = self.store()?;
        let made = store
            .create(label, time)
            .map_err(WorkbenchError::Checkpoint)?;
        Ok(made.into())
    }

    /// The open project's checkpoints, oldest first.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoProject`], or [`WorkbenchError::Checkpoint`] when the store cannot
    /// be read.
    pub fn checkpoints(&mut self) -> Result<Vec<CheckpointMade>, WorkbenchError> {
        let list = self.store()?.list().map_err(WorkbenchError::Checkpoint)?;
        Ok(list.into_iter().map(Into::into).collect())
    }

    /// Puts every file of the open project back as checkpoint `number` holds it, removes the
    /// files made since, and drops every unsaved text of the editor and every staged hunk, so
    /// what the editor and the problems show is the checkpoint's.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoProject`], [`WorkbenchError::Checkpoint`] when there is no such
    /// checkpoint or a file cannot be written, or [`WorkbenchError::Project`] when the project
    /// file put back does not open.
    pub fn restore_checkpoint(
        &mut self,
        number: u32,
    ) -> Result<CheckpointRestored, WorkbenchError> {
        let root = self.root.clone().ok_or(WorkbenchError::NoProject)?;
        let Restored { written, removed } = self
            .store()?
            .restore(number)
            .map_err(WorkbenchError::Checkpoint)?;
        let discarded: Vec<PathBuf> = self.unsaved.iter().cloned().collect();
        let mut touched: Vec<PathBuf> = written
            .iter()
            .chain(&removed)
            .map(|path| normalize(&root.join(path)))
            .chain(discarded.iter().cloned())
            .collect();
        touched.sort();
        touched.dedup();
        for path in &touched {
            self.staged.forget(path);
            if let Ok(text) = std::fs::read(path) {
                // The editor's text of the file becomes the disk's, then is dropped.
                self.set_text(path, text, Source::Elsewhere)?;
            }
            self.host
                .remove_overlay(path)
                .map_err(WorkbenchError::Project)?;
            self.unsaved.remove(path);
        }
        self.tree = tree(&self.host.files(), &root);
        self.problems = problems(&mut self.host);
        self.symbols = self.host.workspace_symbols();
        Ok(CheckpointRestored {
            written,
            removed,
            discarded: discarded.iter().map(|path| relative(&root, path)).collect(),
        })
    }
}

/// `path` under `root`, or `path` itself when it is not under it.
fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

/// Seconds since 1970 now, for a checkpoint's time; 0 when the clock is before 1970.
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
        .unwrap_or(0)
}

/// `checkpoint.create`: records a checkpoint of the open project.
struct CreateCheckpoint;

impl<C: HasWorkbench + 'static> Command<C> for CreateCheckpoint {
    const ID: &'static str = CREATE_CHECKPOINT;
    const SUMMARY: &'static str = "Records every file of the open project, as it is on disk, \
        as a checkpoint to restore, in a git store of its own.";
    type Params = CreateCheckpointParams;
    type Output = CheckpointMade;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: CreateCheckpointParams,
    ) -> Result<Outcome<CheckpointMade, C>, WorkbenchError> {
        let label = params.label.unwrap_or_else(|| "checkpoint".to_owned());
        let made = context.workbench().create_checkpoint(&label, now())?;
        Ok(Outcome::outside_history(made))
    }
}

/// `checkpoint.list`: the open project's checkpoints.
struct ListCheckpoints;

impl<C: HasWorkbench + 'static> Command<C> for ListCheckpoints {
    const ID: &'static str = LIST_CHECKPOINTS;
    const SUMMARY: &'static str = "Lists the open project's checkpoints, oldest first.";
    type Params = NoParams;
    type Output = Vec<CheckpointMade>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        _params: NoParams,
    ) -> Result<Outcome<Vec<CheckpointMade>, C>, WorkbenchError> {
        Ok(Outcome::unchanged(context.workbench().checkpoints()?))
    }
}

/// `checkpoint.restore`: puts the open project back as a checkpoint holds it.
struct RestoreCheckpoint;

impl<C: HasWorkbench + 'static> Command<C> for RestoreCheckpoint {
    const ID: &'static str = RESTORE_CHECKPOINT;
    const SUMMARY: &'static str = "Puts every file of the open project back as a checkpoint \
        holds it, removing the files made since and dropping unsaved edits.";
    type Params = RestoreCheckpointParams;
    type Output = CheckpointRestored;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: RestoreCheckpointParams,
    ) -> Result<Outcome<CheckpointRestored, C>, WorkbenchError> {
        let restored = context.workbench().restore_checkpoint(params.number)?;
        Ok(Outcome::outside_history(restored))
    }
}

/// Registers `checkpoint.create`, `checkpoint.list` and `checkpoint.restore`.
///
/// # Errors
///
/// [`RegisterError`] when one of their ids is taken.
pub fn register_checkpoints<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(CreateCheckpoint)?;
    registry.register(ListCheckpoints)?;
    registry.register(RestoreCheckpoint)
}

#[cfg(test)]
mod tests;
