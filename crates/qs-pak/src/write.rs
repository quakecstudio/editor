// SPDX-License-Identifier: GPL-3.0-or-later

//! Writes PAK archives (PAK-6): the files of a mod packed for players (P4-27), byte for byte
//! the same whatever order they are given in. Implements PAK-6.

use core::fmt;

use crate::{PAK_ENTRY_SIZE, PAK_HEADER_SIZE, PAK_MAGIC, PAK_MAX_FILES, PAK_NAME_SIZE};

/// Why an archive could not be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PakWriteError {
    /// A name is empty (PAK-6.3).
    EmptyName,
    /// A name has a NUL byte, which would end it early when read (PAK-6.3).
    NameHasNul {
        /// The name.
        name: Vec<u8>,
    },
    /// A name does not fit the 56-byte field with its NUL (PAK-6.3).
    NameTooLong {
        /// The name.
        name: Vec<u8>,
    },
    /// Two files have the same name (PAK-6.1).
    Duplicate {
        /// The name.
        name: Vec<u8>,
    },
    /// More files than an engine loads (PAK-6.4).
    TooManyFiles {
        /// How many were given.
        count: usize,
    },
    /// An offset or length past what the format's signed 32-bit fields hold (PAK-6.4).
    TooLarge,
}

impl fmt::Display for PakWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = |name: &[u8]| String::from_utf8_lossy(name).into_owned();
        match self {
            Self::EmptyName => f.write_str("a file has an empty name"),
            Self::NameHasNul { name } => write!(f, "{:?} has a NUL byte", text(name)),
            Self::NameTooLong { name } => write!(
                f,
                "{:?} is longer than {} bytes",
                text(name),
                PAK_NAME_SIZE.saturating_sub(1)
            ),
            Self::Duplicate { name } => write!(f, "{:?} is given twice", text(name)),
            Self::TooManyFiles { count } => {
                write!(f, "{count} files are more than {PAK_MAX_FILES}")
            }
            Self::TooLarge => f.write_str("the archive is larger than 2 GiB"),
        }
    }
}

impl std::error::Error for PakWriteError {}

/// The archive of `files`, each a name and its contents (PAK-6): the header, then each file's
/// contents in the order of their names' bytes, then the directory in the same order. Nothing
/// in it depends on the order the files are given in, or on when it is written.
///
/// # Errors
///
/// [`PakWriteError`] for a name the format cannot hold, a name given twice, more than
/// [`PAK_MAX_FILES`] files, or an archive past the 32-bit offsets.
pub fn write_pak<N: AsRef<[u8]>, D: AsRef<[u8]>>(
    files: &[(N, D)],
) -> Result<Vec<u8>, PakWriteError> {
    let mut sorted: Vec<(&[u8], &[u8])> = files
        .iter()
        .map(|(name, data)| (name.as_ref(), data.as_ref()))
        .collect();
    for (name, _) in &sorted {
        check_name(name)?;
    }
    if u32::try_from(sorted.len()).map_or(true, |count| count > PAK_MAX_FILES) {
        return Err(PakWriteError::TooManyFiles {
            count: sorted.len(),
        });
    }
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    if let Some(pair) = sorted.windows(2).find(|pair| pair_names_equal(pair)) {
        let name = pair
            .first()
            .map_or_else(Vec::new, |(name, _)| name.to_vec());
        return Err(PakWriteError::Duplicate { name });
    }
    let data_len = sorted
        .iter()
        .try_fold(0_usize, |sum, (_, data)| sum.checked_add(data.len()))
        .ok_or(PakWriteError::TooLarge)?;
    let dirofs = PAK_HEADER_SIZE
        .checked_add(data_len)
        .ok_or(PakWriteError::TooLarge)?;
    let dirlen = sorted
        .len()
        .checked_mul(PAK_ENTRY_SIZE)
        .ok_or(PakWriteError::TooLarge)?;
    let total = dirofs.checked_add(dirlen).ok_or(PakWriteError::TooLarge)?;
    let field = |n: usize| i32::try_from(n).map_err(|_| PakWriteError::TooLarge);
    // PAK-6.4: every offset the archive stores, and its end, fit a signed 32-bit field.
    field(total)?;
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&PAK_MAGIC);
    out.extend_from_slice(&field(dirofs)?.to_le_bytes());
    out.extend_from_slice(&field(dirlen)?.to_le_bytes());
    let mut directory = Vec::with_capacity(dirlen);
    for (name, data) in &sorted {
        let mut entry = [0_u8; PAK_ENTRY_SIZE];
        for (slot, byte) in entry.iter_mut().zip(name.iter()) {
            *slot = *byte;
        }
        let place = entry
            .get_mut(PAK_NAME_SIZE..)
            .ok_or(PakWriteError::TooLarge)?;
        let filepos = field(out.len())?.to_le_bytes();
        let filelen = field(data.len())?.to_le_bytes();
        for (slot, byte) in place.iter_mut().zip(filepos.iter().chain(filelen.iter())) {
            *slot = *byte;
        }
        directory.extend_from_slice(&entry);
        out.extend_from_slice(data);
    }
    out.extend_from_slice(&directory);
    Ok(out)
}

/// Whether the two names of a sorted pair are the same.
fn pair_names_equal(pair: &[(&[u8], &[u8])]) -> bool {
    matches!(pair, [a, b] if a.0 == b.0)
}

/// PAK-6.3: a name is 1 to 55 bytes, with no NUL, so that its field holds it and its NUL.
fn check_name(name: &[u8]) -> Result<(), PakWriteError> {
    if name.is_empty() {
        return Err(PakWriteError::EmptyName);
    }
    if name.contains(&0) {
        return Err(PakWriteError::NameHasNul {
            name: name.to_vec(),
        });
    }
    if name.len() >= PAK_NAME_SIZE {
        return Err(PakWriteError::NameTooLong {
            name: name.to_vec(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
