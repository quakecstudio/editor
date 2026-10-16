// SPDX-License-Identifier: GPL-3.0-or-later

//! Source files: reading them, and naming spans across them.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use qc_lexer::{LineIndex, Span};

/// Index of a file in a [`SourceMap`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub u32);

/// A byte range in one file of a [`SourceMap`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileSpan {
    /// The file.
    pub file: FileId,
    /// The bytes in it.
    pub span: Span,
}

/// A file the preprocessor has read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    /// Its path, as resolved (QC-9.2).
    pub path: PathBuf,
    /// Its bytes.
    pub text: Vec<u8>,
    /// Its line starts (QC-1.2).
    pub lines: LineIndex,
}

/// Every file read while preprocessing a project, in the order read. A file included twice
/// is read, and listed, twice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    /// Adds a file and returns its id.
    pub fn add(&mut self, path: PathBuf, text: Vec<u8>) -> FileId {
        let id = FileId(u32::try_from(self.files.len()).unwrap_or(u32::MAX));
        let lines = LineIndex::new(&text);
        self.files.push(SourceFile { path, text, lines });
        id
    }

    /// The file with id `id`.
    #[must_use]
    pub fn file(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(usize::try_from(id.0).ok()?)
    }

    /// The bytes a span covers.
    #[must_use]
    pub fn text(&self, span: FileSpan) -> Option<&[u8]> {
        self.file(span.file)?.text.get(span.span.range())
    }

    /// The 1-based line a span starts on.
    #[must_use]
    pub fn line(&self, span: FileSpan) -> Option<u32> {
        Some(self.file(span.file)?.lines.line(span.span.start))
    }

    /// Every file, in the order read.
    #[must_use]
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }
}

/// Reads source files for the preprocessor.
pub trait Loader {
    /// The bytes of the file at `path`.
    ///
    /// # Errors
    ///
    /// Whatever stops the file being read.
    fn load(&mut self, path: &Path) -> io::Result<Vec<u8>>;
}

/// Reads files from the file system.
#[derive(Clone, Copy, Debug, Default)]
pub struct FsLoader;

impl Loader for FsLoader {
    fn load(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }
}

/// Reads files from memory: for tests, and for editor buffers not yet saved.
#[derive(Clone, Debug, Default)]
pub struct MemoryLoader {
    files: HashMap<PathBuf, Vec<u8>>,
}

impl MemoryLoader {
    /// An empty loader.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces the file at `path`.
    pub fn insert(&mut self, path: impl Into<PathBuf>, text: impl Into<Vec<u8>>) {
        self.files.insert(path.into(), text.into());
    }
}

impl Loader for MemoryLoader {
    fn load(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such file"))
    }
}
