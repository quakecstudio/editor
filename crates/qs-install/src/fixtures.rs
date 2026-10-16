// SPDX-License-Identifier: GPL-3.0-or-later

//! Test fixtures: a filesystem and a registry held in memory, store layouts built on them,
//! and small PAK archives. Layouts hold directory structure and stand-in files only; no
//! game data is in the repository (LIC-4).

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Cursor};
use std::path::{Component, Path, PathBuf};

use qs_paths::{Platform, StoreDirs, store_dirs};
use qs_vfs::{DirEntry, EntryKind, HostFs};

use crate::md5::{Md5, hex};
use crate::{Finder, InstallFs, KnownFile, Registry, Release};

/// A case-sensitive filesystem in memory, as on Linux, with optional aliases that stand
/// for symbolic links when paths are made canonical.
#[derive(Debug, Default)]
pub(crate) struct MemFs {
    files: BTreeMap<Vec<String>, Vec<u8>>,
    dirs: BTreeSet<Vec<String>>,
    aliases: BTreeMap<Vec<String>, Vec<String>>,
}

fn key(path: &Path) -> Vec<String> {
    let mut key: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                key.pop();
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
    pub(crate) fn file(mut self, path: impl AsRef<Path>, bytes: &[u8]) -> Self {
        let path = key(path.as_ref());
        for len in 0..path.len() {
            self.dirs.insert(path[..len].to_vec());
        }
        self.files.insert(path, bytes.to_vec());
        self
    }

    /// Adds the directory `path` and every directory above it.
    pub(crate) fn dir(mut self, path: impl AsRef<Path>) -> Self {
        let path = key(path.as_ref());
        for len in 0..=path.len() {
            self.dirs.insert(path[..len].to_vec());
        }
        self
    }

    /// Makes `alias` resolve to `target`, as a symbolic link would: the alias is a
    /// directory of its own here, and the two meet only in [`InstallFs::canonical`].
    pub(crate) fn alias(mut self, alias: impl AsRef<Path>, target: impl AsRef<Path>) -> Self {
        self.aliases
            .insert(key(alias.as_ref()), key(target.as_ref()));
        self
    }

    /// Adds a stand-in game directory: `game_dir/pak0.pak` and any other files, each a small
    /// valid PAK archive.
    pub(crate) fn paks(mut self, base: impl AsRef<Path>, files: &[&str]) -> Self {
        for file in files {
            self = self.file(base.as_ref().join(file), &pak(file));
        }
        self
    }
}

impl HostFs for MemFs {
    type File = Cursor<Vec<u8>>;

    fn open(&self, path: &Path) -> io::Result<Self::File> {
        self.files
            .get(&key(path))
            .map(|bytes| Cursor::new(bytes.clone()))
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        let path = key(path);
        if !self.dirs.contains(&path) {
            return Err(io::ErrorKind::NotFound.into());
        }
        let depth = path.len();
        let mut entries = BTreeMap::new();
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

impl InstallFs for MemFs {
    fn canonical(&self, path: &Path) -> PathBuf {
        let path = key(path);
        let resolved = self.aliases.get(&path).cloned().unwrap_or(path);
        let mut out = PathBuf::new();
        for part in resolved {
            out.push(part);
        }
        out
    }
}

/// A registry held in memory: (key, value) to data.
#[derive(Debug, Default)]
pub(crate) struct MemRegistry(BTreeMap<(String, String), String>);

impl MemRegistry {
    pub(crate) fn with(mut self, key: &str, value: &str, data: &str) -> Self {
        self.0
            .insert((key.to_owned(), value.to_owned()), data.to_owned());
        self
    }
}

impl Registry for MemRegistry {
    fn string(&self, key: &str, value: &str) -> Option<String> {
        self.0.get(&(key.to_owned(), value.to_owned())).cloned()
    }
}

/// A small PAK archive holding one entry named after `label`, so each stand-in file has
/// its own bytes, size and checksum.
pub(crate) fn pak(label: &str) -> Vec<u8> {
    let data = label.as_bytes();
    let mut bytes = b"PACK".to_vec();
    let dirofs = 12 + data.len();
    bytes.extend_from_slice(&i32::try_from(dirofs).unwrap().to_le_bytes());
    bytes.extend_from_slice(&64_i32.to_le_bytes());
    bytes.extend_from_slice(data);
    let mut name = [0_u8; 56];
    name[..5].copy_from_slice(b"a.txt");
    bytes.extend_from_slice(&name);
    bytes.extend_from_slice(&12_i32.to_le_bytes());
    bytes.extend_from_slice(&i32::try_from(data.len()).unwrap().to_le_bytes());
    bytes
}

/// A table row that identifies `bytes` as the file `game_dir/name` of `release`, the way
/// FS-6.2 identifies the real files by their published size and checksum.
pub(crate) fn known(
    game_dir: &'static str,
    name: &'static str,
    release: Release,
    bytes: &[u8],
) -> KnownFile {
    let mut hasher = Md5::new();
    hasher.update(bytes);
    KnownFile {
        game_dir,
        name,
        release,
        size: u64::try_from(bytes.len()).unwrap(),
        md5: Box::leak(hex(&hasher.finish()).into_boxed_str()),
        description: "stand-in",
    }
}

/// The environment of one OS for a finder: its store folders under a home directory.
pub(crate) fn env(platform: Platform) -> StoreDirs {
    let vars: &[(&str, &str)] = match platform {
        Platform::MacOs => &[("HOME", "/Users/q")],
        Platform::Linux => &[("HOME", "/home/q")],
        Platform::Windows => &[
            ("ProgramFiles(x86)", "C:/Program Files (x86)"),
            ("SystemDrive", "C:"),
            ("APPDATA", "C:/Users/q/AppData/Roaming"),
        ],
    };
    store_dirs(platform, |name| {
        vars.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| (*value).into())
    })
}

/// Every install a finder for `platform` finds in `fs` and `registry`.
pub(crate) fn find(fs: &MemFs, registry: &MemRegistry, platform: Platform) -> Vec<crate::Install> {
    Finder::new(fs, registry, platform, env(platform)).find()
}
