// SPDX-License-Identifier: GPL-3.0-or-later

//! System font fallback (ADR-0003): a character no bundled font has is drawn in a font of the
//! system's that has it.
//!
//! egui draws only the fonts it is given, so the app gives it more when it needs them, as a
//! browser's or Zed's fallback does: the first time text holds a character the loaded fonts
//! lack, the font files in the system's font folders (`qs_paths::current_font_dirs`) are read
//! one by one until a face whose character map has it is found (`skrifa`, the font reader egui
//! uses), and egui gets that face as the last fallback of every family. Each character is
//! looked for once.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{
    Context, FontData, FontFamily,
    epaint::text::{FontInsert, FontPriority, InsertFontFamily},
};
use skrifa::raw::FileRef;
use skrifa::{FontRef, MetadataProvider as _};

/// Where the system's font files come from: its font folders, or for a test a list of files.
enum Source {
    /// The font files under these folders, listed the first time a character is missing.
    Folders {
        dirs: Vec<PathBuf>,
        files: Option<Vec<PathBuf>>,
    },
    /// These font files' bytes.
    #[cfg(test)]
    Files(Vec<Arc<[u8]>>),
}

/// The fonts egui has, and where to find the system's.
pub(crate) struct SystemFonts {
    /// Each font given to egui, as its file's bytes and its face's index in the file.
    loaded: Vec<(Arc<[u8]>, u32)>,
    source: Source,
    /// The system faces given to egui already, by file and index.
    added: BTreeSet<(usize, u32)>,
    /// The characters looked for already, found or not.
    tried: BTreeSet<char>,
    /// How many times the system's fonts were searched for a character.
    searches: usize,
}

impl SystemFonts {
    /// The fallback over the bundled fonts `bundled`, each a font file's bytes, searching the
    /// system's font folders.
    pub(crate) fn new(bundled: &[&'static [u8]]) -> Self {
        Self::over(
            bundled,
            Source::Folders {
                dirs: qs_paths::current_font_dirs(),
                files: None,
            },
        )
    }

    /// The fallback over `bundled` whose "system" has only the font files `files`.
    #[cfg(test)]
    pub(crate) fn with_files(bundled: &[&'static [u8]], files: &[&'static [u8]]) -> Self {
        Self::over(
            bundled,
            Source::Files(files.iter().map(|bytes| Arc::from(*bytes)).collect()),
        )
    }

    fn over(bundled: &[&'static [u8]], source: Source) -> Self {
        Self {
            loaded: bundled.iter().map(|bytes| (Arc::from(*bytes), 0)).collect(),
            source,
            added: BTreeSet::new(),
            tried: BTreeSet::new(),
            searches: 0,
        }
    }

    /// How many times the system's fonts were searched for a character.
    #[cfg(test)]
    pub(crate) const fn searches(&self) -> usize {
        self.searches
    }

    /// The number of system faces given to egui.
    #[cfg(test)]
    pub(crate) fn added(&self) -> usize {
        self.added.len()
    }

    /// Whether a font egui has draws `c`. egui's own `has_glyph` cannot say: it is false for
    /// every character of the face that also draws its replacement character.
    fn has(&self, c: char) -> bool {
        self.loaded
            .iter()
            .any(|(data, index)| face_has(data, *index, c))
    }

    /// Gives egui, in `ctx`, a system face for each character of `text` that no font it has
    /// draws, as the last fallback of each of `families`; the number of faces added.
    pub(crate) fn cover(&mut self, ctx: &Context, families: &[FontFamily], text: &str) -> usize {
        let unique: BTreeSet<char> = text
            .chars()
            .filter(|c| !c.is_control() && !self.tried.contains(c))
            .collect();
        let mut added = 0_usize;
        for c in unique {
            self.tried.insert(c);
            if self.has(c) {
                continue;
            }
            let Some(Found { file, data, index }) = self.face_with(c) else {
                tracing::debug!(character = %c, "no system font has it");
                continue;
            };
            let mut font = FontData::from_owned(data.to_vec());
            font.index = index;
            ctx.add_font(FontInsert::new(
                &format!("system-{file}-{index}"),
                font,
                families
                    .iter()
                    .map(|family| InsertFontFamily {
                        family: family.clone(),
                        priority: FontPriority::Lowest,
                    })
                    .collect(),
            ));
            self.loaded.push((data, index));
            self.added.insert((file, index));
            added = added.saturating_add(1);
        }
        added
    }

    /// The first system face not given to egui yet whose character map has `c`.
    fn face_with(&mut self, c: char) -> Option<Found> {
        self.searches = self.searches.saturating_add(1);
        let count = match &mut self.source {
            Source::Folders { dirs, files } => files.get_or_insert_with(|| font_files(dirs)).len(),
            #[cfg(test)]
            Source::Files(files) => files.len(),
        };
        (0..count).find_map(|file| {
            let data: Arc<[u8]> = match &self.source {
                Source::Folders { files, .. } => {
                    let path = files.as_ref()?.get(file)?;
                    Arc::from(std::fs::read(path).ok()?)
                }
                #[cfg(test)]
                Source::Files(files) => Arc::clone(files.get(file)?),
            };
            let faces = u32::try_from(FileRef::new(&data).ok()?.fonts().count()).ok()?;
            let index = (0..faces)
                .find(|&index| !self.added.contains(&(file, index)) && face_has(&data, index, c))?;
            Some(Found { file, data, index })
        })
    }
}

/// A system face: the number of its file among the system's, the file's bytes, and the face's
/// index in the file.
struct Found {
    file: usize,
    data: Arc<[u8]>,
    index: u32,
}

/// Whether face `index` of the font file `data` has a glyph for `c`.
fn face_has(data: &[u8], index: u32, c: char) -> bool {
    FontRef::from_index(data, index).is_ok_and(|font| font.charmap().map(c).is_some())
}

/// The font files under `dirs`, by path: every `.ttf`, `.otf`, `.ttc` and `.otc`, folders
/// searched to any depth. A folder that cannot be read is skipped.
fn font_files(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut pending: Vec<PathBuf> = dirs.to_vec();
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                FONT_EXTENSIONS
                    .iter()
                    .any(|known| e.eq_ignore_ascii_case(known))
            }) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// The extensions of the font files the system fallback reads.
const FONT_EXTENSIONS: [&str; 4] = ["ttf", "otf", "ttc", "otc"];

/// The families text is drawn in: the interface's and the code's.
pub(crate) fn families() -> [FontFamily; 2] {
    [FontFamily::Proportional, FontFamily::Monospace]
}

#[cfg(test)]
mod tests;
