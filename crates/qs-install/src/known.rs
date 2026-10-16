// SPDX-License-Identifier: GPL-3.0-or-later

//! The published sizes and MD5 checksums of Quake's game files, and identifying a file by
//! them. Implements FS-6.2 and FS-6.3.
//!
//! Only sizes and checksums are here; no game data is (LIC-4). Every row's source is in
//! FS-6's constants table in `docs/spec/filesystem.md`.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use qs_pak::{Pak, PakError};
use qs_vfs::HostFs;

use crate::md5::{Md5, hex};

/// Which release of the game a file belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Release {
    /// The 1996 game, its 1997 mission packs, and the 2016 `dopa` episode.
    Original,
    /// The 2021 rerelease (FS-5).
    Rerelease,
}

/// A game file whose size and MD5 checksum have been published.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KnownFile {
    /// The game directory it belongs in, such as `id1` or `hipnotic`.
    pub game_dir: &'static str,
    /// Its file name inside that directory, such as `pak1.pak`.
    pub name: &'static str,
    /// The release it belongs to.
    pub release: Release,
    /// Its size in bytes.
    pub size: u64,
    /// Its MD5 checksum in lowercase hexadecimal.
    pub md5: &'static str,
    /// What it is, for people.
    pub description: &'static str,
}

impl KnownFile {
    /// Whether this is the registered game's `id1/pak1.pak`.
    #[must_use]
    pub fn is_registered_pak1(&self) -> bool {
        self.game_dir == "id1" && self.name == "pak1.pak" && self.release == Release::Original
    }

    /// Whether this is an original `id1/pak0.pak`, the file shareware and registered share.
    #[must_use]
    pub fn is_original_pak0(&self) -> bool {
        self.game_dir == "id1" && self.name == "pak0.pak" && self.release == Release::Original
    }
}

const fn row(
    game_dir: &'static str,
    release: Release,
    size: u64,
    md5: &'static str,
    description: &'static str,
) -> KnownFile {
    KnownFile {
        game_dir,
        name: "pak0.pak",
        release,
        size,
        md5,
        description,
    }
}

/// `FS_KNOWN_FILES`: every file FS-6.2 identifies.
pub static FS_KNOWN_FILES: [KnownFile; 15] = [
    row(
        "id1",
        Release::Original,
        18_689_235,
        "5906e5998fc3d896ddaf5e6a62e03abb",
        "Quake 1.06 episode 1, shared by the shareware and registered game",
    ),
    row(
        "id1",
        Release::Original,
        18_276_119,
        "85fc9cee2035b66290da1e33be2ac86b",
        "Quake 1.01 episode 1",
    ),
    KnownFile {
        game_dir: "id1",
        name: "pak1.pak",
        release: Release::Original,
        size: 34_257_856,
        md5: "d76b3e5678f0b64ac74ce5e340e6a685",
        description: "Quake registered, episodes 2 to 4",
    },
    row(
        "hipnotic",
        Release::Original,
        35_527_205,
        "0ab83681aaf841c4320269e02941a14a",
        "Mission pack 1, Scourge of Armagon",
    ),
    row(
        "rogue",
        Release::Original,
        37_875_279,
        "c38a4e04219c317cd1b02f386bdfe11f",
        "Mission pack 2, Dissolution of Eternity (mission packs CD)",
    ),
    row(
        "rogue",
        Release::Original,
        37_875_279,
        "f8898a483b131de21581656c94f3c1a4",
        "Mission pack 2, Dissolution of Eternity (its own CD)",
    ),
    row(
        "rogue",
        Release::Original,
        37_875_279,
        "f71428763229bdb373f00a547296efe3",
        "Mission pack 2, Dissolution of Eternity (Steam and GOG)",
    ),
    row(
        "dopa",
        Release::Original,
        18_556_973,
        "607a5d9f0fc778cad37889a093dc593e",
        "Episode 5, Dimension of the Past (2016)",
    ),
    row(
        "id1",
        Release::Rerelease,
        179_618_935,
        "457d9221804b0346fd8f42923b2a7879",
        "Rerelease base game",
    ),
    row(
        "hipnotic",
        Release::Rerelease,
        79_912_030,
        "f8b8700e9eed54d521618574d9b690e9",
        "Rerelease mission pack 1, Scourge of Armagon",
    ),
    row(
        "rogue",
        Release::Rerelease,
        44_484_697,
        "54e3e4fbce74232618a632b7440c05de",
        "Rerelease mission pack 2, Dissolution of Eternity",
    ),
    row(
        "dopa",
        Release::Rerelease,
        40_557_898,
        "1e91e104a16cc0fdd53f6b7ce37005e0",
        "Rerelease episode 5, Dimension of the Past",
    ),
    row(
        "mg1",
        Release::Rerelease,
        496_862_633,
        "0aff296d1ec3b9b70e67ce84c60a0d13",
        "Rerelease episode 6, Dimension of the Machine (2021)",
    ),
    row(
        "mg1",
        Release::Rerelease,
        760_108_744,
        "21ff64bc11548c28f1ea03e48eb97a82",
        "Rerelease episode 6, Dimension of the Machine (later update)",
    ),
    row(
        "q64",
        Release::Rerelease,
        101_636_600,
        "20eb6f63a2ef3f8a763b03dbdb1a33b1",
        "Rerelease Quake 64",
    ),
];

/// What a game file turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileStatus {
    /// Its size and checksum match a published file (FS-6.2).
    Known(KnownFile),
    /// It loads as a PAK archive, but no published file matches it (FS-6.3).
    Unrecognised {
        /// Its entry count.
        entries: usize,
        /// Whether id's engine would mark the game as modified by it (PAK-4.2).
        modified: bool,
    },
    /// It cannot be opened or read, or it is not a PAK archive id's engine would load
    /// (FS-6.3).
    Unusable(PakError),
}

/// Identifies the file at `path`.
///
/// Implements FS-6.2 and FS-6.3: the size is compared first, and only a file whose size is
/// a known file's is hashed; a file that matches nothing is read as a PAK archive.
pub fn identify<F: HostFs + ?Sized>(fs: &F, path: &Path) -> FileStatus {
    identify_with(fs, path, &FS_KNOWN_FILES)
}

/// [`identify`] against the table `known` instead of [`FS_KNOWN_FILES`].
pub(crate) fn identify_with<F: HostFs + ?Sized>(
    fs: &F,
    path: &Path,
    known: &[KnownFile],
) -> FileStatus {
    match identify_in(fs, path, known) {
        Ok(status) => status,
        Err(error) => FileStatus::Unusable(error),
    }
}

fn identify_in<F: HostFs + ?Sized>(
    fs: &F,
    path: &Path,
    table: &[KnownFile],
) -> Result<FileStatus, PakError> {
    let mut file = fs.open(path)?;
    let size = file.seek(SeekFrom::End(0))?;
    if table.iter().any(|known| known.size == size) {
        file.seek(SeekFrom::Start(0))?;
        let digest = md5_of(&mut file)?;
        if let Some(known) = table
            .iter()
            .find(|known| known.size == size && known.md5 == digest)
        {
            return Ok(FileStatus::Known(*known));
        }
    }
    let pak = Pak::read_from(&mut file)?;
    Ok(FileStatus::Unrecognised {
        entries: pak.entries().len(),
        modified: pak.is_modified(),
    })
}

/// The size and MD5 checksum of the file at `path`, in the form of [`KnownFile`]'s `size`
/// and `md5`, so an unrecognised file can be added to [`FS_KNOWN_FILES`]. Reads the whole
/// file.
///
/// # Errors
///
/// [`PakError::Io`] when the file cannot be opened or read.
pub fn size_and_md5<F: HostFs + ?Sized>(fs: &F, path: &Path) -> Result<(u64, String), PakError> {
    let mut file = fs.open(path)?;
    let size = file.seek(SeekFrom::End(0))?;
    file.seek(SeekFrom::Start(0))?;
    Ok((size, md5_of(&mut file)?))
}

fn md5_of(reader: &mut impl Read) -> Result<String, PakError> {
    let mut hasher = Md5::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(hex(&hasher.finish()));
        }
        hasher.update(buffer.get(..read).unwrap_or_default());
    }
}
