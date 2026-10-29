// SPDX-License-Identifier: GPL-3.0-or-later

//! The host filesystem the search path reads, and the case-insensitive path resolution of
//! FS-3.7.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Seek};
use std::path::{Path, PathBuf};

/// What a directory entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// A regular file, or a symbolic link to one.
    File,
    /// A directory. Symbolic links to directories are [`EntryKind::Other`], so a walk never
    /// loops.
    Dir,
    /// Anything else.
    Other,
}

/// One entry of a directory listing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DirEntry {
    /// The entry's name.
    pub name: OsString,
    /// What it is.
    pub kind: EntryKind,
}

/// The entry of `entries` named `name` of kind `kind`, as FS-3.7 matches a game directory or
/// an archive: one with exactly that name, else the first in name order that matches it ASCII
/// case-insensitively, so a path keeps the name on disk.
#[must_use]
pub fn find_entry(mut entries: Vec<DirEntry>, name: &str, kind: EntryKind) -> Option<DirEntry> {
    entries.retain(|entry| entry.kind == kind);
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let index = entries
        .iter()
        .position(|entry| entry.name == name)
        .or_else(|| {
            entries.iter().position(|entry| {
                entry
                    .name
                    .to_str()
                    .is_some_and(|entry_name| entry_name.eq_ignore_ascii_case(name))
            })
        })?;
    entries.into_iter().nth(index)
}

/// The host filesystem: opening files and listing directories.
///
/// [`StdFs`] is the real one. The search path only reads through this trait, so tests can
/// give it a tree held in memory.
pub trait HostFs {
    /// An open file.
    type File: Read + Seek;

    /// Opens the file at `path` for reading.
    ///
    /// # Errors
    ///
    /// When the file does not exist or cannot be opened.
    fn open(&self, path: &Path) -> io::Result<Self::File>;

    /// Lists the directory at `path`.
    ///
    /// # Errors
    ///
    /// When the directory does not exist or cannot be listed.
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>>;
}

/// The host's own filesystem, through `std::fs`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StdFs;

impl HostFs for StdFs {
    type File = File;

    fn open(&self, path: &Path) -> io::Result<File> {
        File::open(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let kind = if file_type.is_dir() {
                EntryKind::Dir
            } else if file_type.is_file()
                || (file_type.is_symlink() && fs::metadata(entry.path()).is_ok_and(|m| m.is_file()))
            {
                EntryKind::File
            } else {
                EntryKind::Other
            };
            entries.push(DirEntry {
                name: entry.file_name(),
                kind,
            });
        }
        Ok(entries)
    }
}

/// Whether `c` separates path components on this host: `/` everywhere, and `\` on Windows,
/// where the engine's paths are handed to a filesystem that treats it as one.
pub(crate) const fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

/// The components of `path`, split on the host's separators, without empty ones.
pub(crate) fn components(path: &str) -> impl Iterator<Item = &str> {
    path.split(is_separator).filter(|part| !part.is_empty())
}

/// Resolves `components` under `root`, matching each name ASCII case-insensitively, and
/// returns the host path if the last one is of kind `last`.
///
/// Implements FS-3.7: an exact match wins, then the first case-insensitive match in name
/// order. `.` and `..` are followed as given.
pub(crate) fn resolve<H: HostFs + ?Sized>(
    host: &H,
    root: &Path,
    components: &[&str],
    last: EntryKind,
) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    let count = components.len();
    for (index, &component) in components.iter().enumerate() {
        if component == "." || component == ".." {
            path.push(component);
            continue;
        }
        let want = if index.saturating_add(1) == count {
            last
        } else {
            EntryKind::Dir
        };
        let found = find_entry(host.read_dir(&path).ok()?, component, want)?;
        path.push(&found.name);
    }
    if count == 0 || components.last().is_some_and(|&c| c == "." || c == "..") {
        return (last == EntryKind::Dir).then_some(path);
    }
    Some(path)
}
