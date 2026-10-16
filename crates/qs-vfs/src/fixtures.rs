// SPDX-License-Identifier: GPL-3.0-or-later

//! Test fixtures: a host filesystem held in memory, and factories for the archives, WAD2
//! files and `pop.lmp` files the tests put in it.

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Cursor};
use std::path::{Component, Path};

use qc_progs::crc::{crc_block, crc_process_byte};
use qs_pak::{PAK_PAK0_COUNT, PAK_PAK0_CRC};

use crate::{DirEntry, EntryKind, FS_POP_TABLE, HostFs};

/// A case-sensitive filesystem in memory, as on Linux. Paths are normalised lexically: `.`
/// is dropped and `..` removes the previous component.
#[derive(Debug, Default)]
pub(crate) struct MemFs {
    files: BTreeMap<Vec<String>, Vec<u8>>,
    dirs: BTreeSet<Vec<String>>,
    /// Paths whose files exist but cannot be opened.
    unreadable: BTreeSet<Vec<String>>,
    /// How many files have been opened.
    pub(crate) opens: Cell<usize>,
}

fn key(path: &Path) -> Vec<String> {
    let mut key: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if key.last().is_some_and(|last| last != "..") {
                    key.pop();
                } else {
                    key.push("..".to_owned());
                }
            }
            Component::RootDir => key.push("/".to_owned()),
            Component::Prefix(prefix) => key.push(prefix.as_os_str().to_string_lossy().into()),
            Component::Normal(name) => key.push(name.to_string_lossy().into()),
        }
    }
    key
}

impl MemFs {
    /// Adds the file `path` holding `bytes`, and every directory above it.
    pub(crate) fn file(mut self, path: &str, bytes: &[u8]) -> Self {
        let path = key(Path::new(path));
        for len in 0..path.len() {
            self.dirs.insert(path[..len].to_vec());
        }
        self.files.insert(path, bytes.to_vec());
        self
    }

    /// Adds the empty directory `path`.
    pub(crate) fn dir(mut self, path: &str) -> Self {
        let path = key(Path::new(path));
        for len in 0..=path.len() {
            self.dirs.insert(path[..len].to_vec());
        }
        self
    }

    /// Makes the file `path` impossible to open.
    pub(crate) fn unreadable(mut self, path: &str) -> Self {
        self.unreadable.insert(key(Path::new(path)));
        self
    }
}

impl HostFs for MemFs {
    type File = Cursor<Vec<u8>>;

    fn open(&self, path: &Path) -> io::Result<Self::File> {
        self.opens.set(self.opens.get() + 1);
        let path = key(path);
        if self.unreadable.contains(&path) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        self.files
            .get(&path)
            .map(|bytes| Cursor::new(bytes.clone()))
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        let path = key(path);
        if !self.dirs.contains(&path) {
            return Err(io::ErrorKind::NotFound.into());
        }
        let mut entries = BTreeMap::new();
        let depth = path.len();
        for file in self.files.keys() {
            if file.len() == depth + 1 && file.starts_with(&path) {
                entries.insert(file[depth].clone(), EntryKind::File);
            }
        }
        for dir in &self.dirs {
            if dir.len() == depth + 1 && dir.starts_with(&path) {
                entries.insert(dir[depth].clone(), EntryKind::Dir);
            }
        }
        Ok(entries
            .into_iter()
            .map(|(name, kind)| DirEntry {
                name: name.into(),
                kind,
            })
            .collect())
    }
}

/// A PAK archive of `entries` (name, data), data first and the directory last, followed by
/// `extra` directory bytes.
pub(crate) fn pak_with_extra(entries: &[(&[u8], &[u8])], extra: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut directory = Vec::new();
    for &(name, bytes) in entries {
        let filepos = i32::try_from(12 + data.len()).unwrap();
        data.extend_from_slice(bytes);
        let mut field = name.to_vec();
        field.resize(56, 0);
        directory.extend_from_slice(&field);
        directory.extend_from_slice(&filepos.to_le_bytes());
        directory.extend_from_slice(&i32::try_from(bytes.len()).unwrap().to_le_bytes());
    }
    directory.extend_from_slice(extra);
    let mut out = b"PACK".to_vec();
    out.extend_from_slice(&i32::try_from(12 + data.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&i32::try_from(directory.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&data);
    out.extend_from_slice(&directory);
    out
}

/// A PAK archive of `entries` given as text.
pub(crate) fn pak(entries: &[(&str, &str)]) -> Vec<u8> {
    let entries: Vec<(&[u8], &[u8])> = entries
        .iter()
        .map(|(name, data)| (name.as_bytes(), data.as_bytes()))
        .collect();
    pak_with_extra(&entries, &[])
}

/// An archive that does not mark the game as modified (PAK-4.2): `entries` padded with empty
/// entries to 339, and two extra directory bytes chosen so the directory CRC is 32981.
pub(crate) fn unmodified_pak(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut all: Vec<(Vec<u8>, Vec<u8>)> = entries
        .iter()
        .map(|(name, data)| (name.as_bytes().to_vec(), data.as_bytes().to_vec()))
        .collect();
    while all.len() < PAK_PAK0_COUNT {
        all.push((format!("pad/{}", all.len()).into_bytes(), Vec::new()));
    }
    let refs: Vec<(&[u8], &[u8])> = all
        .iter()
        .map(|(n, d)| (n.as_slice(), d.as_slice()))
        .collect();
    let plain = pak_with_extra(&refs, &[]);
    let dir_start = plain.len() - PAK_PAK0_COUNT * 64;
    let crc = crc_block(&plain[dir_start..]);
    for pair in 0..=u16::MAX {
        let [a, b] = pair.to_be_bytes();
        if crc_process_byte(crc_process_byte(crc, a), b) == PAK_PAK0_CRC {
            return pak_with_extra(&refs, &[a, b]);
        }
    }
    panic!("no two bytes reach the pak0 CRC");
}

/// A `gfx/pop.lmp` holding id's `pop` table, then `trailing` bytes.
pub(crate) fn pop_lmp(trailing: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in FS_POP_TABLE {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes.extend_from_slice(trailing);
    bytes
}

/// A WAD2 file of one lump named `name`.
pub(crate) fn wad(name: &[u8]) -> Vec<u8> {
    let mut out = b"WAD2".to_vec();
    out.extend_from_slice(&1_i32.to_le_bytes());
    out.extend_from_slice(&13_i32.to_le_bytes());
    out.push(b'z');
    out.extend_from_slice(&12_i32.to_le_bytes());
    out.extend_from_slice(&1_i32.to_le_bytes());
    out.extend_from_slice(&1_i32.to_le_bytes());
    out.extend_from_slice(&[66, 0, 0, 0]);
    let mut field = name.to_vec();
    field.resize(16, 0);
    out.extend_from_slice(&field);
    out
}
