// SPDX-License-Identifier: GPL-3.0-or-later

//! The check after typing, off the frame (P3-35, decided by Justin on 2026-11-09): a thread
//! with its own analysis host mirrors the workbench's (the project, the configuration and each
//! open text) and works out the problems, the symbols and the open file's outline and
//! highlighting, which the frame takes once they arrive. The analysis database is
//! single-threaded (`qc_sema::Db` caches `Rc` syntax trees), so the thread owns a host of its
//! own rather than sharing the workbench's.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread::JoinHandle;

use qc_ide::{AnalysisHost, Config, Highlight, Span, Symbol, SyntaxClass};
use qc_project::Loader;

use qc_project::normalize;

use crate::{Problem, problems};

/// What the workbench tells the thread, in the order it happened.
enum Job {
    /// The configuration over the project file changed.
    Config(Config),
    /// The project in this directory was opened.
    Open(PathBuf),
    /// This file's text changed; `generation` is the workbench's count at the change, and
    /// `open` the file open in the editor then.
    Text {
        path: PathBuf,
        text: Vec<u8>,
        generation: u64,
        open: Option<PathBuf>,
    },
}

/// What the thread worked out after the changes up to `generation`.
#[derive(Debug)]
pub(crate) struct Checked {
    /// The workbench's count at the last change taken.
    pub(crate) generation: u64,
    /// Every diagnostic of the project.
    pub(crate) problems: Vec<Problem>,
    /// The project's symbols.
    pub(crate) symbols: Vec<Symbol>,
    /// The open file and its outline, highlighting and syntax classes, when one was open.
    pub(crate) file: Option<FileFacts>,
}

/// What was worked out for the file open in the editor.
#[derive(Debug)]
pub(crate) struct FileFacts {
    pub(crate) path: PathBuf,
    pub(crate) outline: Vec<Symbol>,
    pub(crate) highlights: Vec<Highlight>,
    pub(crate) classes: Vec<(Span, SyntaxClass)>,
    pub(crate) folds: Vec<Span>,
}

/// The thread and its two channels. Dropping it closes the jobs channel, and the thread ends
/// after the job it is on.
#[derive(Debug)]
pub(crate) struct Checker {
    jobs: Sender<Job>,
    results: Receiver<Checked>,
    _thread: JoinHandle<()>,
}

impl Checker {
    /// Starts the thread, its host reading files through `loader` with `config`.
    pub(crate) fn spawn<L: Loader + Send + 'static>(loader: L, config: Config) -> Self {
        let (jobs, inbox) = channel::<Job>();
        let (outbox, results) = channel::<Checked>();
        let thread =
            std::thread::spawn(move || run(AnalysisHost::new(loader, config), &inbox, &outbox));
        Self {
            jobs,
            results,
            _thread: thread,
        }
    }

    /// Tells the thread the configuration changed.
    pub(crate) fn config(&self, config: Config) {
        self.send(Job::Config(config));
    }

    /// Tells the thread the project at `root` was opened.
    pub(crate) fn open(&self, root: &Path) {
        self.send(Job::Open(root.to_path_buf()));
    }

    /// Tells the thread `path`'s text changed at `generation`, with `open` the editor's file.
    pub(crate) fn text(&self, path: &Path, text: Vec<u8>, generation: u64, open: Option<&Path>) {
        self.send(Job::Text {
            path: path.to_path_buf(),
            text,
            generation,
            open: open.map(Path::to_path_buf),
        });
    }

    fn send(&self, job: Job) {
        if self.jobs.send(job).is_err() {
            tracing::warn!("the background check has stopped");
        }
    }

    /// The newest result that has arrived, without waiting; `None` when none has.
    pub(crate) fn take(&self) -> Option<Checked> {
        let mut newest = None;
        loop {
            match self.results.try_recv() {
                Ok(checked) => newest = Some(checked),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return newest,
            }
        }
    }

    /// The next result, waiting for it; `None` when the thread has stopped.
    pub(crate) fn wait(&self) -> Option<Checked> {
        self.results.recv().ok()
    }
}

/// The thread: each time a job comes, it takes every job waiting, so a burst of typing is
/// checked once, then sends what the changes left.
fn run<L: Loader>(mut host: AnalysisHost<L>, inbox: &Receiver<Job>, outbox: &Sender<Checked>) {
    let mut generation = 0;
    let mut open: Option<PathBuf> = None;
    while let Ok(first) = inbox.recv() {
        let mut job = Some(first);
        let mut changed_text = false;
        while let Some(next) = job.take() {
            match next {
                Job::Config(config) => host.set_config(config),
                Job::Open(root) => {
                    if let Err(error) = host.open(&root) {
                        tracing::warn!(%error, "the background check could not open the project");
                    }
                    open = None;
                }
                Job::Text {
                    path,
                    text,
                    generation: at,
                    open: editor,
                } => {
                    if let Err(error) = host.set_overlay(&path, text) {
                        tracing::debug!(%error, "the background check kept the project as it was");
                    }
                    generation = at;
                    open = editor.map(|path| normalize(&path));
                    changed_text = true;
                }
            }
            job = inbox.try_recv().ok();
        }
        if !changed_text {
            continue;
        }
        let file = open.as_ref().map(|path| FileFacts {
            path: path.clone(),
            outline: host.document_symbols(path),
            highlights: host.highlights(path),
            classes: host.syntax_classes(path),
            folds: host.folding_ranges(path),
        });
        let checked = Checked {
            generation,
            problems: problems(&mut host),
            symbols: host.workspace_symbols(),
            file,
        };
        if outbox.send(checked).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
