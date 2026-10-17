// SPDX-License-Identifier: GPL-3.0-or-later

//! Source files: reading them, and naming spans across them.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
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

    /// How the directory holding `path` spells the entry `path` names: the entry whose name
    /// is the last component of `path`, or, when there is none, the one whose name differs
    /// from it only in case. `None` when there is neither, or the loader cannot tell.
    ///
    /// A file opened through a name in the wrong case has a stored name that differs from the
    /// name used, which is how the project graph finds names a case-sensitive file system
    /// would not open (PRJ-7.2).
    fn stored_name(&mut self, path: &Path) -> Option<OsString> {
        let _ = path;
        None
    }
}

/// Whether two names are the same when case is ignored, as a case-insensitive file system
/// compares them: each lowered as Unicode lowers it.
fn same_ignoring_case(a: &OsStr, b: &OsStr) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

/// Reads files from the file system.
#[derive(Clone, Copy, Debug, Default)]
pub struct FsLoader;

impl Loader for FsLoader {
    fn load(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    /// Lists the directory holding `path`, so the answer is the same whether or not the host
    /// file system ignores case.
    fn stored_name(&mut self, path: &Path) -> Option<OsString> {
        let name = path.file_name()?;
        let dir = path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut folded: Vec<OsString> = Vec::new();
        for entry in fs::read_dir(dir).ok()?.filter_map(Result::ok) {
            let entry = entry.file_name();
            if entry == name {
                return Some(entry);
            }
            if same_ignoring_case(&entry, name) {
                folded.push(entry);
            }
        }
        // The directory may list two entries that differ only in case; `read_dir` does not
        // order them, so take the least to be the same on every run (PRJ-7.2).
        folded.into_iter().min()
    }
}

/// Reads files from memory: for tests, and for editor buffers not yet saved.
#[derive(Clone, Debug, Default)]
pub struct MemoryLoader {
    files: HashMap<PathBuf, Vec<u8>>,
    ignore_case: bool,
}

impl MemoryLoader {
    /// An empty loader that matches paths exactly, as a case-sensitive file system does.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty loader that matches paths as a case-insensitive file system does: a path
    /// whose components differ from a file's only in case opens that file.
    #[must_use]
    pub fn case_insensitive() -> Self {
        Self {
            ignore_case: true,
            ..Self::default()
        }
    }

    /// Adds or replaces the file at `path`.
    pub fn insert(&mut self, path: impl Into<PathBuf>, text: impl Into<Vec<u8>>) {
        self.files.insert(path.into(), text.into());
    }

    /// The first `count` components of `path`, as names.
    fn prefix(path: &Path, count: usize) -> Vec<&OsStr> {
        path.components()
            .take(count)
            .map(|c| c.as_os_str())
            .collect()
    }

    /// Of the held paths whose first components match `path`'s ignoring case, the least,
    /// cut to as many components as `path` has.
    fn folded(&self, path: &Path) -> Option<Vec<&OsStr>> {
        let wanted = Self::prefix(path, usize::MAX);
        // Two held keys may cut to prefixes that differ only in case; the map does not order
        // them, so take the least to be the same on every run (PRJ-7.2).
        self.files
            .keys()
            .map(|key| Self::prefix(key, wanted.len()))
            .filter(|prefix| {
                prefix.len() == wanted.len()
                    && prefix
                        .iter()
                        .zip(&wanted)
                        .all(|(a, b)| same_ignoring_case(a, b))
            })
            .min()
    }
}

impl Loader for MemoryLoader {
    fn load(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        let mut found = self.files.get(path);
        if found.is_none() && self.ignore_case {
            found = self
                .folded(path)
                .and_then(|p| self.files.get(&p.iter().collect::<PathBuf>()));
        }
        found
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such file"))
    }

    fn stored_name(&mut self, path: &Path) -> Option<OsString> {
        let wanted = Self::prefix(path, usize::MAX);
        let exact = self
            .files
            .keys()
            .any(|key| Self::prefix(key, wanted.len()) == wanted);
        if exact {
            return path.file_name().map(OsStr::to_os_string);
        }
        if !self.ignore_case {
            return None;
        }
        self.folded(path)?.last().map(|n| n.to_os_string())
    }
}
