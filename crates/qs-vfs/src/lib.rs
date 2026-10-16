// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/common.c, COM_CheckParm, COM_FileExtension, COM_InitFilesystem, COM_AddGameDirectory, COM_FindFile, COM_LoadFile, COM_CheckRegistered
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/wad.c, W_LoadWadFile

//! The game filesystem: the search path of game directories and PAK archives id's engine
//! builds from a base directory and its command line, and file lookup on it.
//!
//! [`Vfs::new`] is id's `COM_InitFilesystem` followed by `COM_CheckRegistered`;
//! [`Vfs::find`] is `COM_FindFile` and [`Vfs::load`] is `COM_LoadFile`. [`Vfs::files`] lists
//! every file of every element so that an install can be walked. The host filesystem is
//! behind [`HostFs`]; [`StdFs`] is the real one.
//!
//! The behaviour is specified in `docs/spec/filesystem.md` (FS-1 to FS-5); archives are
//! `qs-pak` (`docs/spec/pak.md`) and `gfx.wad` is checked with `qs-wad` (WAD-1.4).

mod host;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;

use core::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use qs_pak::{Pak, PakError};
use qs_wad::{Wad, WadError};

pub use host::{DirEntry, EntryKind, HostFs, StdFs};
use host::{components, resolve};

/// The game directory every search path starts with, id's `GAMENAME` (FS-1.7).
pub const FS_GAMENAME: &str = "id1";

/// The file whose presence makes the game registered (FS-4.1).
pub const FS_POP_PATH: &str = "gfx/pop.lmp";

/// The bytes of `gfx/pop.lmp` that are checked, `sizeof(check)` in id's
/// `COM_CheckRegistered` (FS-4.2).
pub const FS_POP_SIZE: usize = 256;

/// id's `pop` table from `WinQuake/common.c`: what the first 256 bytes of `gfx/pop.lmp`
/// must hold, as big-endian 16-bit values (FS-4.2).
pub const FS_POP_TABLE: [u16; 128] = [
    0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, //
    0x0000, 0x0000, 0x6600, 0x0000, 0x0000, 0x0000, 0x6600, 0x0000, //
    0x0000, 0x0066, 0x0000, 0x0000, 0x0000, 0x0000, 0x0067, 0x0000, //
    0x0000, 0x6665, 0x0000, 0x0000, 0x0000, 0x0000, 0x0065, 0x6600, //
    0x0063, 0x6561, 0x0000, 0x0000, 0x0000, 0x0000, 0x0061, 0x6563, //
    0x0064, 0x6561, 0x0000, 0x0000, 0x0000, 0x0000, 0x0061, 0x6564, //
    0x0064, 0x6564, 0x0000, 0x6469, 0x6969, 0x6400, 0x0064, 0x6564, //
    0x0063, 0x6568, 0x6200, 0x0064, 0x6864, 0x0000, 0x6268, 0x6563, //
    0x0000, 0x6567, 0x6963, 0x0064, 0x6764, 0x0063, 0x6967, 0x6500, //
    0x0000, 0x6266, 0x6769, 0x6a68, 0x6768, 0x6a69, 0x6766, 0x6200, //
    0x0000, 0x0062, 0x6566, 0x6666, 0x6666, 0x6666, 0x6562, 0x0000, //
    0x0000, 0x0000, 0x0062, 0x6364, 0x6664, 0x6362, 0x0000, 0x0000, //
    0x0000, 0x0000, 0x0000, 0x0062, 0x6662, 0x0000, 0x0000, 0x0000, //
    0x0000, 0x0000, 0x0000, 0x0061, 0x6661, 0x0000, 0x0000, 0x0000, //
    0x0000, 0x0000, 0x0000, 0x0000, 0x6500, 0x0000, 0x0000, 0x0000, //
    0x0000, 0x0000, 0x0000, 0x0000, 0x6400, 0x0000, 0x0000, 0x0000, //
];

/// Why the search path could not be built or a file could not be read. The messages are
/// id's where id has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VfsError {
    /// An archive opened but failed to load, or one of its entries could not be read
    /// (FS-1.6, PAK-3.4).
    Archive {
        /// The archive's path as the engine names it.
        path: String,
        /// What was wrong.
        error: PakError,
    },
    /// A `-path` archive could not be opened (FS-2.2).
    CouldntLoadPackfile {
        /// The argument.
        path: String,
    },
    /// `gfx/pop.lmp` does not hold id's `pop` table (FS-4.2).
    CorruptedDataFile,
    /// A shareware game was marked as modified (FS-4.3).
    ModifiedShareware,
    /// A WAD2 file was not found on the search path (WAD-1.4).
    WadNotFound {
        /// The file's name.
        name: String,
    },
    /// A WAD2 file was found but did not load (WAD-1.2 to WAD-2.4).
    Wad {
        /// The file's name.
        name: String,
        /// What was wrong.
        error: WadError,
    },
    /// A loose file was found but could not be read.
    Io {
        /// The file's host path.
        path: PathBuf,
        /// What went wrong.
        kind: io::ErrorKind,
    },
}

impl fmt::Display for VfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Archive { path, error } => write!(f, "{path} {error}"),
            Self::CouldntLoadPackfile { path } => write!(f, "Couldn't load packfile: {path}"),
            Self::CorruptedDataFile => f.write_str("Corrupted data file."),
            Self::ModifiedShareware => {
                f.write_str("You must have the registered version to use modified games")
            }
            Self::WadNotFound { name } => write!(f, "W_LoadWadFile: couldn't load {name}"),
            Self::Wad { name, error } => write!(f, "Wad file {name} {error}"),
            Self::Io { path, kind } => write!(f, "{} could not be read: {kind}", path.display()),
        }
    }
}

impl core::error::Error for VfsError {}

/// A directory on the search path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Directory {
    name: String,
    root: PathBuf,
    components: Vec<String>,
}

impl Directory {
    /// The directory's path as the engine names it, such as `./id1`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The directory's host path, if it exists, with its game directory name matched as
    /// FS-3.7 says.
    pub fn host_path<H: HostFs + ?Sized>(&self, host: &H) -> Option<PathBuf> {
        let parts: Vec<&str> = self.components.iter().map(String::as_str).collect();
        resolve(host, &self.root, &parts, EntryKind::Dir)
    }
}

/// A PAK archive on the search path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Archive {
    name: String,
    path: PathBuf,
    pak: Pak,
}

impl Archive {
    /// The archive's path as the engine names it, such as `./id1/pak0.pak`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The archive's host path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The loaded archive.
    #[must_use]
    pub const fn pak(&self) -> &Pak {
        &self.pak
    }
}

/// One element of the search path, id's `searchpath_t`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Element {
    /// Loose files under a directory.
    Directory(Directory),
    /// The entries of an archive.
    Archive(Archive),
}

impl Element {
    /// The element's path as the engine names it.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Directory(directory) => directory.name(),
            Self::Archive(archive) => archive.name(),
        }
    }
}

/// Where a file is.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Location {
    /// Entry `entry` of the archive.
    Entry(usize),
    /// A loose file at this host path.
    Loose(PathBuf),
}

/// A file on the search path, from [`Vfs::find`] or [`Vfs::files`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FileRef {
    /// The index of its element in [`Vfs::elements`].
    pub element: usize,
    /// The file's name on the search path.
    pub name: Vec<u8>,
    /// Where it is.
    pub location: Location,
}

/// A file's contents as id's `COM_LoadFile` returns them: its bytes and one NUL after them
/// that is not part of its size (FS-3.6).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LoadedFile {
    with_nul: Vec<u8>,
}

impl LoadedFile {
    fn new(mut bytes: Vec<u8>) -> Self {
        bytes.push(0);
        Self { with_nul: bytes }
    }

    /// The file's bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.with_nul.split_last().map_or(&[], |(_, bytes)| bytes)
    }

    /// The file's bytes followed by one NUL, so text can be read as a C string.
    #[must_use]
    pub fn bytes_with_nul(&self) -> &[u8] {
        &self.with_nul
    }

    /// The file's size: the entry's `filelen` or the loose file's length.
    #[must_use]
    pub fn size(&self) -> usize {
        self.bytes().len()
    }

    /// The file's bytes, without the NUL.
    #[must_use]
    pub fn into_bytes(mut self) -> Vec<u8> {
        self.with_nul.pop();
        self.with_nul
    }
}

/// The game filesystem: a search path over a host filesystem, and whether the game is
/// registered and modified.
#[derive(Debug)]
pub struct Vfs<H> {
    host: H,
    elements: Vec<Element>,
    game_dir: String,
    modified: bool,
    proghack: bool,
    registered: bool,
}

impl<H: HostFs> Vfs<H> {
    /// Builds the search path for the command-line `args` (the arguments after the program
    /// name), then checks for the registered game: id's `COM_InitFilesystem` followed by
    /// `COM_CheckRegistered`. `default_basedir` is the host's default base directory, used
    /// when there is no `-basedir` (FS-1.2).
    ///
    /// Implements FS-1, FS-2 and FS-4.
    ///
    /// # Errors
    ///
    /// [`VfsError::Archive`] when an archive opens but does not load (FS-1.6),
    /// [`VfsError::CouldntLoadPackfile`] (FS-2.2), [`VfsError::CorruptedDataFile`] (FS-4.2)
    /// and [`VfsError::ModifiedShareware`] (FS-4.3).
    pub fn new<S: AsRef<str>>(
        host: H,
        args: &[S],
        default_basedir: &str,
    ) -> Result<Self, VfsError> {
        let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
        let mut vfs = Self {
            host,
            elements: Vec::new(),
            game_dir: String::new(),
            modified: false,
            proghack: false,
            registered: true,
        };
        vfs.com_init_filesystem(&args, default_basedir)?;
        vfs.com_check_registered()?;
        Ok(vfs)
    }

    /// id's `COM_InitFilesystem` without `-cachedir` (FS-2.5).
    fn com_init_filesystem(
        &mut self,
        args: &[&str],
        default_basedir: &str,
    ) -> Result<(), VfsError> {
        let mut basedir = match com_check_parm(args, "-basedir").and_then(|i| value(args, i)) {
            Some(basedir) => basedir.to_owned(),
            None => default_basedir.to_owned(),
        };
        if basedir.ends_with(['/', '\\']) {
            basedir.pop();
        }
        self.com_add_game_directory(&basedir, FS_GAMENAME)?;
        if com_check_parm(args, "-rogue").is_some() {
            self.com_add_game_directory(&basedir, "rogue")?;
        }
        if com_check_parm(args, "-hipnotic").is_some() {
            self.com_add_game_directory(&basedir, "hipnotic")?;
        }
        if let Some(game) = com_check_parm(args, "-game").and_then(|i| value(args, i)) {
            self.modified = true;
            self.com_add_game_directory(&basedir, game)?;
        }
        if let Some(index) = com_check_parm(args, "-path") {
            self.modified = true;
            self.elements.clear();
            for &arg in args.iter().skip(index.saturating_add(1)) {
                if arg.starts_with(['+', '-']) {
                    break;
                }
                let element = if com_file_extension(arg) == b"pak" {
                    let Some(archive) = self.com_load_pack_file(Path::new(arg), arg)? else {
                        return Err(VfsError::CouldntLoadPackfile {
                            path: arg.to_owned(),
                        });
                    };
                    Element::Archive(archive)
                } else {
                    Element::Directory(Directory {
                        name: arg.to_owned(),
                        root: PathBuf::from(arg),
                        components: Vec::new(),
                    })
                };
                self.elements.insert(0, element);
            }
        }
        self.proghack = com_check_parm(args, "-proghack").is_some();
        Ok(())
    }

    /// id's `COM_AddGameDirectory` for `<basedir>/<game>`.
    ///
    /// Implements FS-1.4 to FS-1.6 and FS-1.9; game directory and archive names are matched
    /// as FS-3.7 says.
    fn com_add_game_directory(&mut self, basedir: &str, game: &str) -> Result<(), VfsError> {
        let name = format!("{basedir}/{game}");
        let root = if basedir.is_empty() {
            PathBuf::from("/")
        } else {
            PathBuf::from(basedir)
        };
        let directory = Directory {
            name: name.clone(),
            root,
            components: components(game).map(str::to_owned).collect(),
        };
        let resolved = directory.host_path(&self.host);
        self.game_dir.clone_from(&name);
        self.elements.insert(0, Element::Directory(directory));
        let Some(resolved) = resolved else {
            return Ok(());
        };
        for number in 0_u32.. {
            let file = format!("pak{number}.pak");
            let Some(path) = resolve(&self.host, &resolved, &[&file], EntryKind::File) else {
                break;
            };
            let Some(archive) = self.com_load_pack_file(&path, &format!("{name}/{file}"))? else {
                break;
            };
            self.elements.insert(0, Element::Archive(archive));
        }
        Ok(())
    }

    /// id's `COM_LoadPackFile`: `None` when the file cannot be opened (PAK-1.4), an error
    /// when it opens but does not load (FS-1.6). Loading it applies PAK-4.2.
    fn com_load_pack_file(&mut self, path: &Path, name: &str) -> Result<Option<Archive>, VfsError> {
        let Ok(mut file) = self.host.open(path) else {
            return Ok(None);
        };
        let pak = Pak::read_from(&mut file).map_err(|error| VfsError::Archive {
            path: name.to_owned(),
            error,
        })?;
        self.modified |= pak.is_modified();
        Ok(Some(Archive {
            name: name.to_owned(),
            path: path.to_path_buf(),
            pak,
        }))
    }

    /// id's `COM_CheckRegistered`.
    ///
    /// Implements FS-4: the lookup of `gfx/pop.lmp` runs as registered (FS-4.1).
    fn com_check_registered(&mut self) -> Result<(), VfsError> {
        let pop = self.load(FS_POP_PATH.as_bytes())?;
        self.registered = false;
        let Some(pop) = pop else {
            if self.modified {
                return Err(VfsError::ModifiedShareware);
            }
            return Ok(());
        };
        let Some(check) = pop.bytes().get(..FS_POP_SIZE) else {
            return Err(VfsError::CorruptedDataFile);
        };
        for (pair, &want) in check.as_chunks::<2>().0.iter().zip(&FS_POP_TABLE) {
            if u16::from_be_bytes(*pair) != want {
                return Err(VfsError::CorruptedDataFile);
            }
        }
        self.registered = true;
        Ok(())
    }

    /// The host filesystem.
    #[must_use]
    pub const fn host(&self) -> &H {
        &self.host
    }

    /// The search path, head first: the order lookups search it in.
    #[must_use]
    pub fn elements(&self) -> &[Element] {
        &self.elements
    }

    /// The game directory the engine writes to, id's `com_gamedir`: the last game directory
    /// added (FS-1.9).
    #[must_use]
    pub fn game_dir(&self) -> &str {
        &self.game_dir
    }

    /// Whether the game is marked as modified, id's `com_modified` (FS-1.8, FS-2.1, PAK-4.2).
    #[must_use]
    pub const fn is_modified(&self) -> bool {
        self.modified
    }

    /// Whether the game is registered, id's `static_registered` (FS-4.2).
    #[must_use]
    pub const fn is_registered(&self) -> bool {
        self.registered
    }

    /// Finds `name` on the search path, id's `COM_FindFile`: the file from the first element,
    /// from the head, that has it, or `None`.
    ///
    /// Implements FS-2.4 and FS-3.1 to FS-3.5, FS-3.7 and FS-3.8.
    #[must_use]
    pub fn find(&self, name: &[u8]) -> Option<FileRef> {
        let start = usize::from(self.proghack && name == b"progs.dat");
        let loose = self.loose_components(name);
        for (index, element) in self.elements.iter().enumerate().skip(start) {
            let location = match element {
                Element::Archive(archive) => archive
                    .pak
                    .entries()
                    .iter()
                    .position(|entry| entry.name() == name)
                    .map(Location::Entry),
                Element::Directory(directory) => loose.as_ref().and_then(|parts| {
                    let mut all: Vec<&str> =
                        directory.components.iter().map(String::as_str).collect();
                    all.extend(parts.iter().copied());
                    resolve(&self.host, &directory.root, &all, EntryKind::File).map(Location::Loose)
                }),
            };
            if let Some(location) = location {
                return Some(FileRef {
                    element: index,
                    name: name.to_vec(),
                    location,
                });
            }
        }
        None
    }

    /// The components of `name` as a loose file, or `None` when directory elements cannot
    /// have it: in a shareware game when it has a separator (FS-3.4), when it is absolute or
    /// has a `..` component (FS-3.8), or when it is not UTF-8.
    fn loose_components<'a>(&self, name: &'a [u8]) -> Option<Vec<&'a str>> {
        let has_separator = name.iter().any(|&b| b == b'/' || b == b'\\');
        if !self.registered && has_separator {
            return None;
        }
        let name = core::str::from_utf8(name).ok()?;
        let bytes = name.as_bytes();
        let drive =
            bytes.get(1) == Some(&b':') && bytes.first().is_some_and(u8::is_ascii_alphabetic);
        if name.starts_with(['/', '\\']) || drive || name.split(['/', '\\']).any(|c| c == "..") {
            return None;
        }
        Some(components(name).collect())
    }

    /// Reads the file `file` refers to.
    ///
    /// Implements PAK-3.2 to PAK-3.4 for archive entries and FS-3.6 for sizes.
    ///
    /// # Errors
    ///
    /// [`VfsError::Archive`] when an entry's range is outside its archive or the archive
    /// cannot be read, and [`VfsError::Io`] when a loose file cannot be read.
    pub fn read(&self, file: &FileRef) -> Result<Vec<u8>, VfsError> {
        match (&file.location, self.elements.get(file.element)) {
            (Location::Entry(index), Some(Element::Archive(archive))) => {
                let error = |error| VfsError::Archive {
                    path: archive.name.clone(),
                    error,
                };
                let entry = archive
                    .pak
                    .entries()
                    .get(*index)
                    .ok_or_else(|| error(PakError::Io(io::ErrorKind::NotFound)))?;
                let mut handle = self
                    .host
                    .open(&archive.path)
                    .map_err(|e| error(PakError::from(e)))?;
                archive.pak.read_entry(&mut handle, entry).map_err(error)
            }
            (Location::Loose(path), _) => {
                let io_error = |e: io::Error| VfsError::Io {
                    path: path.clone(),
                    kind: e.kind(),
                };
                let mut handle = self.host.open(path).map_err(io_error)?;
                let mut bytes = Vec::new();
                handle.read_to_end(&mut bytes).map_err(io_error)?;
                Ok(bytes)
            }
            (Location::Entry(_), _) => Err(VfsError::Io {
                path: PathBuf::from(String::from_utf8_lossy(&file.name).into_owned()),
                kind: io::ErrorKind::NotFound,
            }),
        }
    }

    /// Finds and reads `name`, id's `COM_LoadFile`: `None` when it is not on the search path
    /// (FS-3.5).
    ///
    /// Implements FS-3.
    ///
    /// # Errors
    ///
    /// As [`Vfs::read`].
    pub fn load(&self, name: &[u8]) -> Result<Option<LoadedFile>, VfsError> {
        let Some(file) = self.find(name) else {
            return Ok(None);
        };
        self.read(&file).map(|bytes| Some(LoadedFile::new(bytes)))
    }

    /// Loads the WAD2 file `name` through the search path and checks that it loads, as id's
    /// `W_LoadWadFile` does for [`qs_wad::WAD_GFX_NAME`].
    ///
    /// Implements WAD-1.4.
    ///
    /// # Errors
    ///
    /// [`VfsError::WadNotFound`] when it is not on the search path, [`VfsError::Wad`] when it
    /// does not load, and the errors of [`Vfs::read`].
    pub fn load_wad(&self, name: &str) -> Result<Vec<u8>, VfsError> {
        let Some(file) = self.load(name.as_bytes())? else {
            return Err(VfsError::WadNotFound {
                name: name.to_owned(),
            });
        };
        let bytes = file.into_bytes();
        if let Err(error) = Wad::read(&bytes) {
            return Err(VfsError::Wad {
                name: name.to_owned(),
                error,
            });
        }
        Ok(bytes)
    }

    /// Every file of every element, in search order: each archive's entries in directory
    /// order, and each existing directory's loose files in name order, with `/` between
    /// directory names. Files a lookup would not reach (shadowed by an earlier element,
    /// or a later duplicate in one archive) are listed too.
    #[must_use]
    pub fn files(&self) -> Vec<FileRef> {
        let mut files = Vec::new();
        for (index, element) in self.elements.iter().enumerate() {
            match element {
                Element::Archive(archive) => {
                    for (entry_index, entry) in archive.pak.entries().iter().enumerate() {
                        files.push(FileRef {
                            element: index,
                            name: entry.name().to_vec(),
                            location: Location::Entry(entry_index),
                        });
                    }
                }
                Element::Directory(directory) => {
                    if let Some(root) = directory.host_path(&self.host) {
                        self.walk(index, &root, "", &mut files);
                    }
                }
            }
        }
        files
    }

    /// Adds the loose files under `path`, named with `prefix`, to `files`.
    fn walk(&self, element: usize, path: &Path, prefix: &str, files: &mut Vec<FileRef>) {
        let Ok(mut entries) = self.host.read_dir(path) else {
            return;
        };
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        for entry in entries {
            let name = format!("{prefix}{}", entry.name.to_string_lossy());
            let child = path.join(&entry.name);
            match entry.kind {
                EntryKind::File => files.push(FileRef {
                    element,
                    name: name.into_bytes(),
                    location: Location::Loose(child),
                }),
                EntryKind::Dir => self.walk(element, &child, &format!("{name}/"), files),
                EntryKind::Other => {}
            }
        }
    }
}

/// id's `COM_CheckParm`: the index of the first argument equal to `parm` (FS-1.1). `args`
/// excludes the program name, so the result is id's index minus one.
fn com_check_parm(args: &[&str], parm: &str) -> Option<usize> {
    args.iter().position(|&arg| arg == parm)
}

/// The argument after the option at `index`, if there is one (FS-1.1).
fn value<'a>(args: &[&'a str], index: usize) -> Option<&'a str> {
    args.get(index.checked_add(1)?).copied()
}

/// id's `COM_FileExtension`: the text after the first `.`, at most 7 bytes (FS-2.3).
fn com_file_extension(path: &str) -> &[u8] {
    let bytes = path.as_bytes();
    let Some(dot) = bytes.iter().position(|&b| b == b'.') else {
        return &[];
    };
    let rest = bytes.get(dot.saturating_add(1)..).unwrap_or_default();
    rest.get(..rest.len().min(7)).unwrap_or_default()
}
