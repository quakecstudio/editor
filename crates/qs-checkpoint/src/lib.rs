// SPDX-License-Identifier: GPL-3.0-or-later

//! Checkpoints of a project's files (P4-05, AGT-4): each a git commit whose tree holds every
//! file of the project, written as git's loose objects in a bare store of its own, on a private
//! ref `refs/checkpoints/<n>`. In a git working tree the store is `.git/quakec-studio/checkpoints`,
//! so the project's own history and its garbage collection never see it; elsewhere it is the
//! hidden `.quakec-studio/checkpoints`. Either is an ordinary bare repository that git itself
//! reads (`git --git-dir <store> log refs/checkpoints/1`).
//!
//! A checkpoint holds each regular file under the project root, by its path, except those under
//! [`SKIPPED`]: the git directory, the store, and `build/`, which the build writes. Restoring
//! one writes back every file it holds, byte for byte, and removes every file it does not hold,
//! so the project is exactly as it was, deletions and additions undone alike.
//!
//! The objects are git's: a blob is `blob <len>\0<bytes>`, a tree lists `<mode> <name>\0<id>`
//! in git's order, a commit names its tree, its parent and its time, each zlib-compressed under
//! `objects/<id[..2]>/<id[2..]>`, the id the SHA-1 of the uncompressed object.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The directories under the project root a checkpoint leaves out: git's, the hidden store's
/// and the build's.
pub const SKIPPED: [&str; 3] = [".git", ".quakec-studio", "build"];

/// The bytes of an object's id, a SHA-1 digest.
const ID_BYTES: usize = 20;
/// The hex digits that spell an object's id.
const ID_DIGITS: usize = ID_BYTES * 2;
/// The base of an id's digits.
const HEX_RADIX: u32 = 16;
/// The zlib level git writes loose objects at, its default.
const ZLIB_LEVEL: u8 = 6;

/// An object's id, the SHA-1 of the uncompressed object.
type ObjectId = [u8; ID_BYTES];

/// A checkpoint's number, from 1, the ref `refs/checkpoints/<n>`; written to JSON as the bare
/// number.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct CheckpointNumber(u32);

impl CheckpointNumber {
    /// The first checkpoint's.
    pub const FIRST: Self = Self(1);

    /// The one numbered `value`.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Its number.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// The number after it.
    #[must_use]
    const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl fmt::Display for CheckpointNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A file's mode in a tree: a regular file.
const FILE_MODE: &str = "100644";
/// A directory's mode in a tree.
const TREE_MODE: &str = "40000";
/// Who writes a checkpoint, in the commit's author and committer lines.
const SIGNATURE: &str = "QuakeC Studio <checkpoints@quakec-studio.invalid>";

/// Why a checkpoint could not be made, listed or restored.
#[derive(Debug)]
pub enum CheckpointError {
    /// A file or a directory could not be read or written.
    Io(PathBuf, io::Error),
    /// No checkpoint has this number.
    NotFound(CheckpointNumber),
    /// An object of the store is missing or is not what git writes.
    Corrupt(String),
}

impl fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(path, error) => write!(f, "{}: {error}", path.display()),
            Self::NotFound(n) => write!(f, "no checkpoint {n}"),
            Self::Corrupt(why) => write!(f, "the checkpoint store is damaged: {why}"),
        }
    }
}

impl std::error::Error for CheckpointError {}

/// A checkpoint made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    /// Its number.
    pub number: CheckpointNumber,
    /// Its commit's id, 40 hex digits.
    pub commit: String,
    /// What it was made for, the commit's message.
    pub label: String,
    /// How many files it holds.
    pub files: usize,
}

/// What a restore changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Restored {
    /// The files written back, their content having changed or they having been deleted, by
    /// path under the root.
    pub written: Vec<PathBuf>,
    /// The files removed, made after the checkpoint, by path under the root.
    pub removed: Vec<PathBuf>,
}

/// The checkpoints of the project at a root.
#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    git: PathBuf,
}

impl Store {
    /// The store of the project at `root`, made when it is not there: in `.git` when the root
    /// is a git working tree, else in the hidden `.quakec-studio`.
    ///
    /// # Errors
    ///
    /// [`CheckpointError::Io`] when the store cannot be made.
    pub fn open(root: &Path) -> Result<Self, CheckpointError> {
        let git = if root.join(".git").is_dir() {
            root.join(".git").join("quakec-studio").join("checkpoints")
        } else {
            root.join(".quakec-studio").join("checkpoints")
        };
        let store = Self {
            root: root.to_path_buf(),
            git,
        };
        store.init()?;
        Ok(store)
    }

    /// The store's git directory.
    #[must_use]
    pub fn git_dir(&self) -> &Path {
        &self.git
    }

    /// Makes the bare repository's layout when it is not there.
    fn init(&self) -> Result<(), CheckpointError> {
        for dir in ["objects", "refs/checkpoints"] {
            let path = self.git.join(dir);
            fs::create_dir_all(&path).map_err(|e| CheckpointError::Io(path, e))?;
        }
        for (name, text) in [
            ("HEAD", "ref: refs/heads/main\n"),
            (
                "config",
                "[core]\n\trepositoryformatversion = 0\n\tbare = true\n",
            ),
        ] {
            let path = self.git.join(name);
            if !path.exists() {
                fs::write(&path, text).map_err(|e| CheckpointError::Io(path, e))?;
            }
        }
        Ok(())
    }

    /// Records every file of the project as checkpoint `n`, the next number, with `label` as
    /// its message and `time` (seconds since 1970, UTC) as its time; its parent is the
    /// checkpoint before it.
    ///
    /// # Errors
    ///
    /// [`CheckpointError::Io`] when a file cannot be read or the store written.
    pub fn create(&self, label: &str, time: i64) -> Result<Checkpoint, CheckpointError> {
        let files = project_files(&self.root)?;
        let mut tree = Dir::default();
        for path in &files {
            let full = self.root.join(path);
            let bytes = fs::read(&full).map_err(|e| CheckpointError::Io(full.clone(), e))?;
            let id = self.write_object("blob", &bytes)?;
            tree.insert(path, id);
        }
        let tree_id = self.write_tree(&tree)?;
        let list = self.list()?;
        let parent = list.last();
        let number = parent.map_or(CheckpointNumber::FIRST, |p| p.number.next());
        let mut commit = format!("tree {}\n", hex(&tree_id));
        if let Some(parent) = parent {
            commit.push_str(&format!("parent {}\n", parent.commit));
        }
        for role in ["author", "committer"] {
            commit.push_str(&format!("{role} {SIGNATURE} {time} +0000\n"));
        }
        commit.push('\n');
        commit.push_str(label);
        commit.push('\n');
        let commit_id = hex(&self.write_object("commit", commit.as_bytes())?);
        let reference = self.git.join("refs/checkpoints").join(number.to_string());
        fs::write(&reference, format!("{commit_id}\n"))
            .map_err(|e| CheckpointError::Io(reference, e))?;
        Ok(Checkpoint {
            number,
            commit: commit_id,
            label: label.to_owned(),
            files: files.len(),
        })
    }

    /// Every checkpoint, oldest first.
    ///
    /// # Errors
    ///
    /// [`CheckpointError`] when a ref or its commit cannot be read.
    pub fn list(&self) -> Result<Vec<Checkpoint>, CheckpointError> {
        let refs = self.git.join("refs/checkpoints");
        let entries = fs::read_dir(&refs).map_err(|e| CheckpointError::Io(refs.clone(), e))?;
        let mut numbers: Vec<CheckpointNumber> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                entry
                    .file_name()
                    .to_str()?
                    .parse()
                    .ok()
                    .map(CheckpointNumber)
            })
            .collect();
        numbers.sort_unstable();
        numbers
            .into_iter()
            .map(|number| {
                let (commit, tree) = self.commit_of(number)?;
                let files = self.read_tree(&tree, Path::new(""))?.len();
                let label = self.label_of(&commit)?;
                Ok(Checkpoint {
                    number,
                    commit,
                    label,
                    files,
                })
            })
            .collect()
    }

    /// Puts every file of the project back as checkpoint `number` holds it, and removes every
    /// file it does not hold, leaving [`SKIPPED`] alone.
    ///
    /// # Errors
    ///
    /// [`CheckpointError::NotFound`] when there is no such checkpoint, and [`CheckpointError`]
    /// when the store or a file cannot be read or written.
    pub fn restore(&self, number: CheckpointNumber) -> Result<Restored, CheckpointError> {
        let (_, tree) = self.commit_of(number)?;
        let held = self.read_tree(&tree, Path::new(""))?;
        let mut restored = Restored::default();
        for path in project_files(&self.root)? {
            if !held.contains_key(&path) {
                let full = self.root.join(&path);
                fs::remove_file(&full).map_err(|e| CheckpointError::Io(full, e))?;
                restored.removed.push(path);
            }
        }
        for (path, id) in &held {
            let bytes = self.read_object(id, "blob")?;
            let full = self.root.join(path);
            if fs::read(&full).ok().as_deref() == Some(bytes.as_slice()) {
                continue;
            }
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| CheckpointError::Io(parent.to_path_buf(), e))?;
            }
            fs::write(&full, bytes).map_err(|e| CheckpointError::Io(full, e))?;
            restored.written.push(path.clone());
        }
        for path in &restored.removed {
            remove_emptied(&self.root, path)?;
        }
        Ok(restored)
    }

    /// Checkpoint `number`'s commit id and its tree's.
    fn commit_of(&self, number: CheckpointNumber) -> Result<(String, ObjectId), CheckpointError> {
        let reference = self.git.join("refs/checkpoints").join(number.to_string());
        let text = fs::read_to_string(&reference).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => CheckpointError::NotFound(number),
            _ => CheckpointError::Io(reference.clone(), e),
        })?;
        let commit = text.trim().to_owned();
        let id = parse_hex(&commit)
            .ok_or_else(|| CheckpointError::Corrupt(format!("refs/checkpoints/{number}")))?;
        let body = self.read_object(&id, "commit")?;
        let tree = String::from_utf8_lossy(&body)
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("tree "))
            .and_then(parse_hex)
            .ok_or_else(|| CheckpointError::Corrupt(format!("commit {commit} has no tree")))?;
        Ok((commit, tree))
    }

    /// The message of the commit `commit`.
    fn label_of(&self, commit: &str) -> Result<String, CheckpointError> {
        let id = parse_hex(commit).ok_or_else(|| CheckpointError::Corrupt(commit.to_owned()))?;
        let body = self.read_object(&id, "commit")?;
        let text = String::from_utf8_lossy(&body);
        Ok(text
            .split_once("\n\n")
            .map_or("", |(_, message)| message)
            .trim_end()
            .to_owned())
    }

    /// Writes `body` as an object of `kind` when it is not there; its id.
    fn write_object(&self, kind: &str, body: &[u8]) -> Result<ObjectId, CheckpointError> {
        let mut object = format!("{kind} {}\0", body.len()).into_bytes();
        object.extend_from_slice(body);
        let id = sha1_smol::Sha1::from(&object).digest().bytes();
        let name = hex(&id);
        let (folder, file) = name.split_at(2);
        let dir = self.git.join("objects").join(folder);
        let path = dir.join(file);
        if !path.exists() {
            fs::create_dir_all(&dir).map_err(|e| CheckpointError::Io(dir, e))?;
            let packed = miniz_oxide::deflate::compress_to_vec_zlib(&object, ZLIB_LEVEL);
            fs::write(&path, packed).map_err(|e| CheckpointError::Io(path, e))?;
        }
        Ok(id)
    }

    /// The body of object `id`, which must be of `kind`.
    fn read_object(&self, id: &ObjectId, kind: &str) -> Result<Vec<u8>, CheckpointError> {
        let name = hex(id);
        let (folder, file) = name.split_at(2);
        let path = self.git.join("objects").join(folder).join(file);
        let packed = fs::read(&path).map_err(|e| CheckpointError::Io(path, e))?;
        let object = miniz_oxide::inflate::decompress_to_vec_zlib(&packed)
            .map_err(|_| CheckpointError::Corrupt(format!("object {name} does not inflate")))?;
        let header_end = object
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| CheckpointError::Corrupt(format!("object {name} has no header")))?;
        let (header, rest) = object.split_at(header_end);
        let header = String::from_utf8_lossy(header);
        let body = rest.get(1..).unwrap_or_default();
        let wanted = format!("{kind} {}", body.len());
        if header != wanted {
            return Err(CheckpointError::Corrupt(format!(
                "object {name} is {header}, not {wanted}"
            )));
        }
        Ok(body.to_vec())
    }

    /// Writes `dir` as a tree, its folders first written as trees of their own; its id.
    fn write_tree(&self, dir: &Dir) -> Result<ObjectId, CheckpointError> {
        let mut entries: Vec<(Vec<u8>, &str, ObjectId)> = Vec::new();
        for (name, id) in &dir.files {
            entries.push((name.as_bytes().to_vec(), FILE_MODE, *id));
        }
        for (name, sub) in &dir.dirs {
            // Git orders a folder as though its name ended in `/`.
            let mut key = name.as_bytes().to_vec();
            key.push(b'/');
            entries.push((key, TREE_MODE, self.write_tree(sub)?));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let mut body = Vec::new();
        for (key, mode, id) in entries {
            let name = key.strip_suffix(b"/").unwrap_or(&key);
            body.extend_from_slice(mode.as_bytes());
            body.push(b' ');
            body.extend_from_slice(name);
            body.push(0);
            body.extend_from_slice(&id);
        }
        self.write_object("tree", &body)
    }

    /// Every file under the tree `id`, by its path under `prefix`, with its blob's id.
    fn read_tree(
        &self,
        id: &ObjectId,
        prefix: &Path,
    ) -> Result<BTreeMap<PathBuf, ObjectId>, CheckpointError> {
        let body = self.read_object(id, "tree")?;
        let mut files = BTreeMap::new();
        let mut rest = body.as_slice();
        let corrupt = || CheckpointError::Corrupt(format!("tree {} is cut short", hex(id)));
        while !rest.is_empty() {
            let space = rest.iter().position(|b| *b == b' ').ok_or_else(corrupt)?;
            let (mode, after) = rest.split_at(space);
            let after = after.get(1..).ok_or_else(corrupt)?;
            let nul = after.iter().position(|b| *b == 0).ok_or_else(corrupt)?;
            let (name, after) = after.split_at(nul);
            let (entry, after) = after
                .get(1..)
                .and_then(|after| after.split_first_chunk::<ID_BYTES>())
                .ok_or_else(corrupt)?;
            let child = *entry;
            rest = after;
            let name = String::from_utf8_lossy(name).into_owned();
            let path = prefix.join(&name);
            if mode == TREE_MODE.as_bytes() {
                files.extend(self.read_tree(&child, &path)?);
            } else {
                files.insert(path, child);
            }
        }
        Ok(files)
    }
}

/// A folder of a tree being written: its files' blob ids and its folders, by name.
#[derive(Debug, Default)]
struct Dir {
    files: BTreeMap<String, ObjectId>,
    dirs: BTreeMap<String, Dir>,
}

impl Dir {
    /// Puts the blob `id` at `path`, making the folders on the way.
    fn insert(&mut self, path: &Path, id: ObjectId) {
        let parts: Vec<String> = path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let Some((file, folders)) = parts.split_last() else {
            return;
        };
        let mut dir = self;
        for folder in folders {
            dir = dir.dirs.entry(folder.clone()).or_default();
        }
        dir.files.insert(file.clone(), id);
    }
}

/// Every regular file under `root`, by its path under it, in order, leaving out [`SKIPPED`] at
/// the root; symbolic links are not followed or held.
fn project_files(root: &Path) -> Result<Vec<PathBuf>, CheckpointError> {
    let mut found = Vec::new();
    let mut dirs = vec![PathBuf::new()];
    while let Some(dir) = dirs.pop() {
        let full = root.join(&dir);
        let entries = fs::read_dir(&full).map_err(|e| CheckpointError::Io(full.clone(), e))?;
        for entry in entries {
            let entry = entry.map_err(|e| CheckpointError::Io(full.clone(), e))?;
            let name = entry.file_name();
            if dir.as_os_str().is_empty()
                && SKIPPED
                    .iter()
                    .any(|skipped| name == std::ffi::OsStr::new(skipped))
            {
                continue;
            }
            let kind = entry
                .file_type()
                .map_err(|e| CheckpointError::Io(entry.path(), e))?;
            let path = dir.join(&name);
            if kind.is_dir() {
                dirs.push(path);
            } else if kind.is_file() {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Removes the folders of `path` under `root` that its removal left empty, innermost first,
/// up to the first that is not empty; `root` itself stays.
fn remove_emptied(root: &Path, path: &Path) -> Result<(), CheckpointError> {
    let mut folder = path.parent();
    while let Some(dir) = folder.filter(|d| !d.as_os_str().is_empty()) {
        let full = root.join(dir);
        let empty = fs::read_dir(&full).is_ok_and(|mut inside| inside.next().is_none());
        if !empty {
            break;
        }
        fs::remove_dir(&full).map_err(|e| CheckpointError::Io(full, e))?;
        folder = dir.parent();
    }
    Ok(())
}

/// `id` in lowercase hex.
fn hex(id: &ObjectId) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

/// The 20 bytes 40 hex digits spell.
fn parse_hex(text: &str) -> Option<ObjectId> {
    let mut id = [0_u8; ID_BYTES];
    if text.len() != ID_DIGITS {
        return None;
    }
    for (slot, pair) in id.iter_mut().zip(text.as_bytes().chunks(2)) {
        let pair = std::str::from_utf8(pair).ok()?;
        *slot = u8::from_str_radix(pair, HEX_RADIX).ok()?;
    }
    Some(id)
}

#[cfg(test)]
mod tests;
