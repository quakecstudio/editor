// SPDX-License-Identifier: GPL-3.0-or-later

//! Source files: reading them, and naming spans across them.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use qc_lexer::{LineIndex, Span};

/// Index of a file in a [`SourceMap`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(u32);

impl FileId {
    /// The id of the file at `index` in its [`SourceMap`].
    pub(crate) const fn new(index: u32) -> Self {
        Self(index)
    }

    /// Its index in its [`SourceMap`].
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

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
    /// Its text, the bytes the preprocessor reads, shared with the preprocessor's readers of it.
    pub text: Rc<[u8]>,
    /// Its line starts (QC-1.2).
    pub lines: LineIndex,
    /// Where its text stands in the file's bytes.
    pub origin: TextOrigin,
}

/// Where the text of a [`SourceFile`], which the preprocessor reads, stands in the file's bytes:
/// under an FTEQCC dialect FTEQCC's file loader may skip or decode some, and writes a line feed
/// after them (FTEF-14.64).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TextOrigin {
    /// The file's bytes, one byte of the text for each: the vanilla dialect's text (QC-1.1).
    #[default]
    Bytes,
    /// The file's bytes from this offset on, past the UTF-8 byte order mark FTEQCC skips or from
    /// the start, one byte of the text for each, and then the line feed FTEQCC's loader writes
    /// after them, which stands at the file's end.
    Shifted(usize),
    /// Decoded from UTF-16 or UTF-32: for each byte of the text, the loader's line feed at its
    /// end included, and then for its end, the offset in the file of the code unit it came from.
    Decoded(Vec<usize>),
}

impl SourceFile {
    /// The bytes of the file that `span`, a span of its text, came from.
    #[must_use]
    pub fn file_span(&self, span: Span) -> Span {
        Span::new(self.file_offset(span.start), self.file_offset(span.end))
    }

    /// The offset in the file of the byte at `offset` of the text; past the text's end, the
    /// offset after the last code unit read.
    fn file_offset(&self, offset: usize) -> usize {
        match &self.origin {
            TextOrigin::Bytes => offset,
            // The loader's line feed, the text's last byte, and the text's end stand at the
            // file's end.
            TextOrigin::Shifted(skipped) => offset
                .min(self.text.len().saturating_sub(1))
                .saturating_add(*skipped),
            TextOrigin::Decoded(offsets) => offsets
                .get(offset)
                .or_else(|| offsets.last())
                .copied()
                .unwrap_or_default(),
        }
    }
}

/// Every file read while preprocessing a project, in the order read. A file included twice
/// is read, and listed, twice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    /// Adds a file whose text stands in its bytes as `origin` says, and returns its id.
    pub fn add(&mut self, path: PathBuf, text: Vec<u8>, origin: TextOrigin) -> FileId {
        let id = FileId::new(u32::try_from(self.files.len()).unwrap_or(u32::MAX));
        let lines = LineIndex::new(&text);
        let text = Rc::from(text);
        self.files.push(SourceFile {
            path,
            text,
            lines,
            origin,
        });
        id
    }

    /// The file with id `id`.
    #[must_use]
    pub fn file(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(usize::try_from(id.index()).ok()?)
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
        let entries = fs::read_dir(dir).ok()?.filter_map(Result::ok);
        stored_entry(name, entries.map(|entry| entry.file_name()))
    }
}

/// Of a directory's `entries`, the one named `name`, or else the least of those whose names
/// differ from it only in case (PRJ-7.2): a directory may list two such entries, in no order,
/// so the least is the same on every run.
pub(crate) fn stored_entry(
    name: &OsStr,
    entries: impl IntoIterator<Item = OsString>,
) -> Option<OsString> {
    let mut folded: Vec<OsString> = Vec::new();
    for entry in entries {
        if entry == name {
            return Some(entry);
        }
        if same_ignoring_case(&entry, name) {
            folded.push(entry);
        }
    }
    folded.into_iter().min()
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
