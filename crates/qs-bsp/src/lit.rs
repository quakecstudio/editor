// SPDX-License-Identifier: GPL-3.0-or-later

//! `.lit` files: coloured light for a BSP, beside it with the same name (BSP-18).
//!
//! id never defined the format; it is described from QuakeSpasm's `Mod_LoadLighting` and
//! ericw-tools' writer, cited in `docs/spec/bsp.md` for description only. Nothing is ported.

use core::fmt;

use crate::BspLoader;

/// The four bytes a `.lit` file starts with (BSP-18.1).
pub const BSP_LIT_MAGIC: [u8; 4] = *b"QLIT";

/// The only version QuakeSpasm reads (BSP-18.1, BSP-18.3).
pub const BSP_LIT_VERSION: i32 = 1;

/// Bytes before the samples (BSP-18.1).
pub const BSP_LIT_HEADER_SIZE: usize = 8;

/// Why QuakeSpasm ignores a `.lit` file (BSP-18.2, BSP-18.3). The messages are its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LitError {
    /// The file does not start with `QLIT`.
    Corrupt,
    /// The version is not 1.
    UnknownVersion {
        /// The stored version.
        version: i32,
    },
    /// The size is not `8 + 3 * filelen` of the lighting lump; a `QLIT` file too short to
    /// hold its version is reported this way.
    Outdated {
        /// The size the lighting lump calls for.
        expected: usize,
        /// The file's size.
        actual: usize,
    },
}

impl LitError {
    /// QuakeSpasm's message, with `name` where it names the file.
    #[must_use]
    pub fn message(&self, name: &str) -> String {
        match self {
            Self::Corrupt => "Corrupt .lit file (old version?), ignoring".to_owned(),
            Self::UnknownVersion { version } => format!("Unknown .lit file version ({version})"),
            Self::Outdated { expected, actual } => {
                format!("Outdated .lit file ({name} should be {expected} bytes, not {actual})")
            }
        }
    }
}

impl fmt::Display for LitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message("the .lit file"))
    }
}

impl core::error::Error for LitError {}

/// A `.lit` file's samples: red, green and blue for each byte of the lighting lump.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lit<'a> {
    /// `3 * filelen` bytes.
    pub rgb: &'a [u8],
}

impl<'a> Lit<'a> {
    /// Reads a `.lit` file for a BSP whose lighting lump is `lighting_len` bytes, checking
    /// it as QuakeSpasm's `Mod_LoadLighting` does: magic, then version, then size
    /// (BSP-18.1 to BSP-18.3).
    ///
    /// # Errors
    ///
    /// The [`LitError`] for which QuakeSpasm ignores the file.
    pub fn read(bytes: &'a [u8], lighting_len: usize) -> Result<Self, LitError> {
        if bytes.get(..4) != Some(BSP_LIT_MAGIC.as_slice()) {
            return Err(LitError::Corrupt);
        }
        let expected = lighting_len
            .checked_mul(3)
            .and_then(|size| size.checked_add(BSP_LIT_HEADER_SIZE))
            .unwrap_or(usize::MAX);
        let outdated = LitError::Outdated {
            expected,
            actual: bytes.len(),
        };
        let version = bytes
            .get(4..BSP_LIT_HEADER_SIZE)
            .and_then(|word| <[u8; 4]>::try_from(word).ok())
            .map(i32::from_le_bytes)
            .ok_or(outdated)?;
        if version != BSP_LIT_VERSION {
            return Err(LitError::UnknownVersion { version });
        }
        if bytes.len() != expected {
            return Err(outdated);
        }
        Ok(Self {
            rgb: bytes.get(BSP_LIT_HEADER_SIZE..).unwrap_or_default(),
        })
    }

    /// The colour of lighting byte `index`, the `.lit` file's triple at `3 * index`
    /// (BSP-18.1).
    #[must_use]
    pub fn sample(&self, index: usize) -> Option<[u8; 3]> {
        let start = index.checked_mul(3)?;
        let triple = self.rgb.get(start..start.checked_add(3)?)?;
        <[u8; 3]>::try_from(triple).ok()
    }
}

/// The colour of each lighting byte when there is no usable `.lit`: a grey triple of its
/// value (BSP-18.5).
#[must_use]
pub fn grey(lighting: &[u8]) -> Vec<[u8; 3]> {
    lighting.iter().map(|&value| [value; 3]).collect()
}

/// The name of a file beside a BSP: its name without its extension, then `extension`, as
/// QuakeSpasm builds the `.lit` and `.ent` names with `COM_StripExtension`: the last `.`
/// after the last `/` or `\` and not at the start ends the stem (BSP-18, BSP-19.1).
#[must_use]
pub fn companion_name(bsp_name: &str, extension: &str) -> String {
    let bytes = bsp_name.as_bytes();
    let mut stem = bsp_name;
    let mut at = bytes.len().saturating_sub(1);
    while at > 0 && bytes.get(at) != Some(&b'.') {
        at = at.saturating_sub(1);
        if matches!(bytes.get(at), Some(b'/' | b'\\')) {
            return format!("{bsp_name}{extension}");
        }
    }
    if at > 0 {
        stem = bsp_name.get(..at).unwrap_or(bsp_name);
    }
    format!("{stem}{extension}")
}

/// Whether QuakeSpasm uses a `.lit` or `.ent` file found in the search path element
/// `found_path_id` for a BSP found in `bsp_path_id`: not when the file's game directory has
/// lower priority, a smaller path id, than the BSP's (BSP-18.4, BSP-19.2).
#[must_use]
pub const fn companion_applies(bsp_path_id: u32, found_path_id: u32) -> bool {
    found_path_id >= bsp_path_id
}

/// A `.lit` file found on the search path for a map: its bytes and the path id of its game
/// directory, which is larger for a game directory of higher priority, as QuakeSpasm's
/// `path_id` is (BSP-18.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoundLit<'a> {
    /// The file's bytes.
    pub bytes: &'a [u8],
    /// Its game directory's path id.
    pub path_id: u32,
}

/// What a loader does with a map's `.lit` file (BSP-18.6 to BSP-18.8). None of these stops
/// the map loading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LitChoice<'a> {
    /// The loader reads no `.lit`: id's WinQuake and GLQuake (BSP-18.6).
    NotRead,
    /// No `.lit` was found.
    Missing,
    /// The `.lit` is from a game directory of lower priority than the map's (BSP-18.4).
    LowerPriority,
    /// The `.lit` is ignored for this reason (BSP-18.2, BSP-18.3).
    Ignored(LitError),
    /// The `.lit` gives the map's light.
    Used(Lit<'a>),
}

impl LitChoice<'_> {
    /// The colour of each lighting byte: the `.lit`'s samples when it is used, else the grey
    /// triples of the lighting lump (BSP-18.5, BSP-18.8).
    #[must_use]
    pub fn rgb(&self, lighting: &[u8]) -> Vec<[u8; 3]> {
        match self {
            Self::Used(lit) => lit.rgb.as_chunks::<3>().0.to_vec(),
            Self::NotRead | Self::Missing | Self::LowerPriority | Self::Ignored(_) => {
                grey(lighting)
            }
        }
    }

    /// The line QuakeSpasm prints with `Con_Printf` for the `.lit` named `name`: only for a
    /// `.lit` it ignores for what it holds (BSP-18.7). Its line for a lower-priority one is
    /// [`Self::developer_message`].
    #[must_use]
    pub fn message(&self, name: &str) -> Option<String> {
        match self {
            Self::Ignored(error) => Some(error.message(name)),
            Self::NotRead | Self::Missing | Self::LowerPriority | Self::Used(_) => None,
        }
    }

    /// The line QuakeSpasm prints with `Con_DPrintf`, only when its `developer` cvar is set,
    /// for a `.lit` from a game directory of lower priority (BSP-18.4, BSP-18.7).
    #[must_use]
    pub fn developer_message(&self, name: &str) -> Option<String> {
        match self {
            Self::LowerPriority => {
                Some(format!("ignored {name} from a gamedir with lower priority"))
            }
            Self::NotRead | Self::Missing | Self::Ignored(_) | Self::Used(_) => None,
        }
    }
}

/// The `.lit` choice QuakeSpasm's `Mod_LoadLighting` makes for a map loaded from the game
/// directory with path id `bsp_path_id`, whose lighting lump is `lighting_len` bytes, given
/// the `.lit` the search path found: the priority first, then the magic, the version and the
/// size, and the first check that fails ignores the file and leaves the map lit grey.
/// Loaders other than [`BspLoader::FitzQuake`] read no `.lit`.
///
/// Implements BSP-18.
#[must_use]
pub fn mod_load_lighting<'a>(
    loader: BspLoader,
    bsp_path_id: u32,
    lighting_len: usize,
    found: Option<FoundLit<'a>>,
) -> LitChoice<'a> {
    match loader {
        BspLoader::WinQuake | BspLoader::GlQuake => return LitChoice::NotRead,
        BspLoader::FitzQuake => {}
    }
    let Some(found) = found else {
        return LitChoice::Missing;
    };
    if !companion_applies(bsp_path_id, found.path_id) {
        return LitChoice::LowerPriority;
    }
    match Lit::read(found.bytes, lighting_len) {
        Ok(lit) => LitChoice::Used(lit),
        Err(error) => LitChoice::Ignored(error),
    }
}
