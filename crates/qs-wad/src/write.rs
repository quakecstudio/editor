// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing a WAD2 file (WAD-5): the lumps after the header, the directory last, every name
//! written as given and checked first.

use std::fmt;

use crate::{HEADER_INFOTABLEOFS, WAD_CMP_NONE, WAD_HEADER_SIZE, WAD_MAGIC, WAD_NAME_SIZE};

/// The longest name written, leaving room for its NUL (WAD-5.3).
pub const WAD_LUMP_NAME_MAX: usize = 15;

/// Why a lump could not be added or a file written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WadWriteError {
    /// The name is empty or longer than [`WAD_LUMP_NAME_MAX`] bytes (WAD-5.3).
    BadName {
        /// The name as given.
        name: Vec<u8>,
    },
    /// An earlier lump has the same name, ASCII case ignored (WAD-5.3).
    DuplicateName {
        /// The name as given.
        name: Vec<u8>,
    },
    /// The file would be larger than a signed 32-bit offset reaches (WAD-5.4).
    TooLarge,
}

impl fmt::Display for WadWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadName { name } => write!(
                f,
                "lump name \"{}\" must be 1 to {WAD_LUMP_NAME_MAX} bytes",
                String::from_utf8_lossy(name)
            ),
            Self::DuplicateName { name } => write!(
                f,
                "a lump named \"{}\" is already in the WAD",
                String::from_utf8_lossy(name)
            ),
            Self::TooLarge => f.write_str("the WAD would be larger than 2 GiB"),
        }
    }
}

impl core::error::Error for WadWriteError {}

/// One lump to write.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NewLump {
    name: Vec<u8>,
    lump_type: u8,
    data: Vec<u8>,
}

/// A WAD2 file being put together: lumps added in order, then written (WAD-5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WadBuilder {
    lumps: Vec<NewLump>,
}

impl WadBuilder {
    /// An empty WAD.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a lump of type `lump_type`, one of the `WAD_TYP_` constants, named `name`.
    ///
    /// Implements WAD-5.3.
    ///
    /// # Errors
    ///
    /// [`WadWriteError::BadName`] for an empty or too long name, and
    /// [`WadWriteError::DuplicateName`] for a name already added.
    pub fn add(&mut self, name: &[u8], lump_type: u8, data: Vec<u8>) -> Result<(), WadWriteError> {
        if name.is_empty() || name.len() > WAD_LUMP_NAME_MAX || name.contains(&0) {
            return Err(WadWriteError::BadName {
                name: name.to_vec(),
            });
        }
        if self
            .lumps
            .iter()
            .any(|lump| lump.name.eq_ignore_ascii_case(name))
        {
            return Err(WadWriteError::DuplicateName {
                name: name.to_vec(),
            });
        }
        self.lumps.push(NewLump {
            name: name.to_vec(),
            lump_type,
            data,
        });
        Ok(())
    }

    /// The file's bytes.
    ///
    /// Implements WAD-5.1, WAD-5.2 and WAD-5.4.
    ///
    /// # Errors
    ///
    /// [`WadWriteError::TooLarge`] when an offset or length would not fit 32 signed bits.
    pub fn to_bytes(&self) -> Result<Vec<u8>, WadWriteError> {
        let mut out = Vec::new();
        out.extend_from_slice(&WAD_MAGIC);
        out.extend_from_slice(&field(self.lumps.len())?.to_le_bytes());
        // The directory's offset, filled in once the lumps are written.
        out.extend_from_slice(&0_i32.to_le_bytes());
        let mut positions = Vec::with_capacity(self.lumps.len());
        for lump in &self.lumps {
            positions.push(field(out.len())?);
            out.extend_from_slice(&lump.data);
        }
        let directory = field(out.len())?;
        for (lump, position) in self.lumps.iter().zip(positions) {
            let size = field(lump.data.len())?;
            out.extend_from_slice(&position.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(&[lump.lump_type, WAD_CMP_NONE, 0, 0]);
            let mut name = [0_u8; WAD_NAME_SIZE];
            for (slot, &byte) in name.iter_mut().zip(&lump.name) {
                *slot = byte;
            }
            out.extend_from_slice(&name);
        }
        field(out.len())?;
        if let Some(slot) = out.get_mut(HEADER_INFOTABLEOFS..WAD_HEADER_SIZE) {
            slot.copy_from_slice(&directory.to_le_bytes());
        }
        Ok(out)
    }
}

/// A size or offset as the file's signed 32-bit field (WAD-5.4).
fn field(value: usize) -> Result<i32, WadWriteError> {
    i32::try_from(value).map_err(|_| WadWriteError::TooLarge)
}

#[cfg(test)]
mod tests;
