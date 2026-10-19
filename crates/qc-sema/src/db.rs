// SPDX-License-Identifier: GPL-3.0-or-later

//! A salsa-style incremental query database for the semantic model.
//!
//! The editor re-runs the semantic model on every keystroke, so it must recompute only what
//! an edit changed (GOAL-1, the incremental model; the design doc, "Semantic model"). This
//! database holds each file's source text as an *input*, and memoizes the *derived* queries
//! built on top: [`Db::parse`] for one file, [`Db::lower`] for one file's declarations, and
//! [`Db::module_index`] for a whole module's resolved symbol table.
//!
//! Incrementality follows salsa's model in miniature. Every input carries a *revision*, the
//! value of a monotonic clock when it last changed. A memoized query records the highest
//! input revision it read; when asked again it recomputes only if one of the files it
//! depends on has a newer revision. Editing one file bumps only that file's revision, so a
//! module whose other files are unchanged re-parses and re-lowers only the edited file, and
//! re-resolves the module from the mix of fresh and cached per-file results.
//!
//! There is no crate dependency on `salsa`: the engine is small, and the project builds its
//! foundations itself. Every query the database answers also ticks a counter
//! ([`QueryCounts`]), so a test can assert that an edit re-ran only the dependent queries
//! (the issue's second acceptance criterion).

use std::cell::Cell;
use std::collections::HashMap;

use qc_syntax::Parse;

use crate::lower::{FileDecls, lower_file};
use crate::resolve::{ModuleIndex, ResolveOptions, resolve_module};
use crate::symbol::FileId;

/// How many times each memoized query actually ran, as opposed to being served from cache.
///
/// The counts only ever rise. A test records them, makes an edit, and checks the deltas to
/// prove an edit re-ran only the queries that depended on the changed file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueryCounts {
    /// Times [`Db::parse`] parsed a file from scratch.
    pub parse: u64,
    /// Times [`Db::lower`] lowered a file from scratch.
    pub lower: u64,
    /// Times [`Db::module_index`] resolved a module from scratch.
    pub module_index: u64,
}

/// A monotonic revision stamp. A larger value is later.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Revision(u64);

/// One input file: its text and the revision at which it last changed.
#[derive(Clone, Debug)]
struct InputFile {
    text: Vec<u8>,
    revision: Revision,
}

/// A memoized derived value, with the input revision it was computed at.
#[derive(Clone, Debug)]
struct Memo<T> {
    value: T,
    /// The highest revision of any input this value read.
    computed_at: Revision,
}

/// The incremental semantic database.
///
/// Set a file's text with [`Db::set_file_text`], then ask for [`Db::parse`], [`Db::lower`]
/// or [`Db::module_index`]. The database is single-threaded: the syntax tree it caches is
/// built on `Rc` (`qc_syntax::SyntaxNode`), matching the editor's analysis thread.
#[derive(Debug, Default)]
pub struct Db {
    /// The current clock. Bumped each time an input changes.
    clock: Revision,
    /// The dialect every file is parsed in (PRJ-2.8). Vanilla by default.
    dialect: qc_syntax::Dialect,
    /// Whether modules resolve with FTEQCC's autoprototyping (FTE-22.3). Off by default.
    autoproto: bool,
    /// The input files, by id.
    files: HashMap<FileId, InputFile>,
    /// Memoized per-file parses.
    parses: HashMap<FileId, Memo<Parse>>,
    /// Memoized per-file lowerings.
    lowerings: HashMap<FileId, Memo<FileDecls>>,
    /// The memoized module index over an ordered list of files.
    module: Option<Memo<(Vec<FileId>, ModuleIndex)>>,
    /// Query-run counters.
    counts: Cell<QueryCounts>,
}

impl Db {
    /// A new, empty database.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The dialect files are parsed in.
    #[must_use]
    pub fn dialect(&self) -> qc_syntax::Dialect {
        self.dialect
    }

    /// Sets the dialect every file is parsed in (PRJ-2.8), selecting the vanilla or an FTEQCC
    /// profile. Changing it invalidates every cached parse, lowering and module index, since
    /// they depend on the dialect; set it once before adding files in the common case.
    pub fn set_dialect(&mut self, dialect: qc_syntax::Dialect) {
        if self.dialect == dialect {
            return;
        }
        self.dialect = dialect;
        self.clock.0 = self.clock.0.saturating_add(1);
        self.parses.clear();
        self.lowerings.clear();
        self.module = None;
        // Stamp every file with the new revision so dependent queries recompute.
        for file in self.files.values_mut() {
            file.revision = self.clock;
        }
    }

    /// Sets whether modules resolve with FTEQCC's autoprototyping, `-Fautoproto` (FTE-22.3):
    /// every top-level declaration visible to every function body. It changes only the
    /// module index, so only that is recomputed.
    pub fn set_autoproto(&mut self, autoproto: bool) {
        if self.autoproto != autoproto {
            self.autoproto = autoproto;
            self.module = None;
        }
    }

    /// The query-run counters so far.
    #[must_use]
    pub fn counts(&self) -> QueryCounts {
        self.counts.get()
    }

    /// Sets, or replaces, the text of file `file`.
    ///
    /// This is an input change: it bumps the clock and stamps the file with the new
    /// revision, so queries that read this file recompute on their next call and queries
    /// that do not are served from cache. Setting the same bytes a file already has is still
    /// a change: the revision advances. Callers that want to avoid needless recomputation
    /// compare the bytes themselves before calling.
    pub fn set_file_text(&mut self, file: FileId, text: Vec<u8>) {
        self.clock.0 = self.clock.0.saturating_add(1);
        self.files.insert(
            file,
            InputFile {
                text,
                revision: self.clock,
            },
        );
    }

    /// The bytes of file `file`, or an empty slice if it has none.
    #[must_use]
    pub fn file_text(&self, file: FileId) -> &[u8] {
        self.files.get(&file).map_or(&[], |f| f.text.as_slice())
    }

    /// The revision at which file `file` last changed.
    fn file_revision(&self, file: FileId) -> Revision {
        self.files.get(&file).map_or(Revision(0), |f| f.revision)
    }

    fn bump_parse(&self) {
        let mut c = self.counts.get();
        c.parse = c.parse.saturating_add(1);
        self.counts.set(c);
    }

    fn bump_lower(&self) {
        let mut c = self.counts.get();
        c.lower = c.lower.saturating_add(1);
        self.counts.set(c);
    }

    fn bump_module_index(&self) {
        let mut c = self.counts.get();
        c.module_index = c.module_index.saturating_add(1);
        self.counts.set(c);
    }

    /// The parse of file `file`, memoized.
    ///
    /// Recomputes only when the file's text has changed since the cached parse.
    pub fn parse(&mut self, file: FileId) -> Parse {
        let revision = self.file_revision(file);
        if let Some(memo) = self.parses.get(&file)
            && memo.computed_at >= revision
        {
            return memo.value.clone();
        }
        self.bump_parse();
        let value = qc_syntax::parse_with(self.file_text(file), self.dialect);
        self.parses.insert(
            file,
            Memo {
                value: value.clone(),
                computed_at: revision,
            },
        );
        value
    }

    /// The lowered declarations of file `file`, memoized.
    ///
    /// Depends only on the file's parse, so it recomputes only when the file changed.
    pub fn lower(&mut self, file: FileId) -> FileDecls {
        let revision = self.file_revision(file);
        if let Some(memo) = self.lowerings.get(&file)
            && memo.computed_at >= revision
        {
            return memo.value.clone();
        }
        let parse = self.parse(file);
        self.bump_lower();
        let value = lower_file(&parse, self.dialect);
        self.lowerings.insert(
            file,
            Memo {
                value: value.clone(),
                computed_at: revision,
            },
        );
        value
    }

    /// The resolved semantic index of the module made of `files`, in compilation order
    /// (QC-23.3), memoized.
    ///
    /// Recomputes only when the file list changes or one of its files does. The per-file
    /// [`Db::lower`] results it reads are themselves cached, so an edit to one file
    /// re-lowers only that file and then re-resolves the module from the cached rest.
    pub fn module_index(&mut self, files: &[FileId]) -> ModuleIndex {
        let max_revision = files
            .iter()
            .map(|&f| self.file_revision(f))
            .max()
            .unwrap_or(Revision(0));
        if let Some(memo) = &self.module
            && memo.value.0 == files
            && memo.computed_at >= max_revision
        {
            return memo.value.1.clone();
        }
        let lowered: Vec<FileDecls> = files.iter().map(|&f| self.lower(f)).collect();
        self.bump_module_index();
        let options = ResolveOptions {
            dialect: self.dialect,
            autoproto: self.autoproto,
        };
        let index = resolve_module(&lowered, options);
        self.module = Some(Memo {
            value: (files.to_vec(), index.clone()),
            computed_at: max_revision,
        });
        index
    }
}
