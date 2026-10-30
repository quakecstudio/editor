// SPDX-License-Identifier: GPL-3.0-or-later

//! System font fallback (ADR-0003): a character no bundled font has is drawn in a font of the
//! system's that has it.
//!
//! egui draws only the fonts it is given, so the app gives it more when it needs them, as a
//! browser's or Zed's fallback does: the first time text holds a character the loaded fonts
//! lack, the system's fonts are listed (`fontdb`), the first face whose character map has it
//! (`ttf-parser`) is read, and egui gets it as the last fallback of every family. Each
//! character is looked for once.

use std::collections::BTreeSet;
use std::sync::Arc;

use eframe::egui::{
    Context, FontData, FontFamily,
    epaint::text::{FontInsert, FontPriority, InsertFontFamily},
};

/// The fonts egui has, and the system's, listed the first time a character is missing.
pub(crate) struct SystemFonts {
    /// Each font given to egui, as its file's bytes and its face's index in the file.
    loaded: Vec<(Arc<[u8]>, u32)>,
    db: Option<fontdb::Database>,
    /// The system faces given to egui already.
    added: BTreeSet<fontdb::ID>,
    /// The characters looked for already, found or not.
    tried: BTreeSet<char>,
}

impl SystemFonts {
    /// The fallback over the bundled fonts `bundled`, each a font file's bytes.
    pub(crate) fn new(bundled: &[&'static [u8]]) -> Self {
        Self {
            loaded: bundled.iter().map(|bytes| (Arc::from(*bytes), 0)).collect(),
            db: None,
            added: BTreeSet::new(),
            tried: BTreeSet::new(),
        }
    }

    /// The fallback over `bundled` that looks in `db` rather than the system's fonts.
    #[cfg(test)]
    pub(crate) fn with_database(bundled: &[&'static [u8]], db: fontdb::Database) -> Self {
        Self {
            db: Some(db),
            ..Self::new(bundled)
        }
    }

    /// The number of system faces given to egui.
    #[cfg(test)]
    pub(crate) fn added(&self) -> usize {
        self.added.len()
    }

    /// Whether a font egui has draws `c`. egui's own `has_glyph` cannot say: it is false for
    /// every character of the face that also draws its replacement character.
    fn has(&self, c: char) -> bool {
        self.loaded.iter().any(|(data, index)| {
            ttf_parser::Face::parse(data, *index).is_ok_and(|face| face.glyph_index(c).is_some())
        })
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
            let Some((id, data, index)) = self.face_with(c) else {
                tracing::debug!(character = %c, "no system font has it");
                continue;
            };
            let shared: Arc<[u8]> = Arc::from(data.as_slice());
            let mut font = FontData::from_owned(data);
            font.index = index;
            ctx.add_font(FontInsert::new(
                &format!("system-{id:?}"),
                font,
                families
                    .iter()
                    .map(|family| InsertFontFamily {
                        family: family.clone(),
                        priority: FontPriority::Lowest,
                    })
                    .collect(),
            ));
            self.loaded.push((shared, index));
            self.added.insert(id);
            added = added.saturating_add(1);
        }
        added
    }

    /// The first system face not given to egui yet whose character map has `c`, with its
    /// file's bytes and its index in the file.
    fn face_with(&mut self, c: char) -> Option<(fontdb::ID, Vec<u8>, u32)> {
        let db = self.db.get_or_insert_with(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            db
        });
        let ids: Vec<fontdb::ID> = db
            .faces()
            .map(|face| face.id)
            .filter(|id| !self.added.contains(id))
            .collect();
        ids.into_iter().find_map(|id| {
            db.with_face_data(id, |data, index| {
                let face = ttf_parser::Face::parse(data, index).ok()?;
                face.glyph_index(c)?;
                Some((id, data.to_vec(), index))
            })
            .flatten()
        })
    }
}

/// The families text is drawn in: the interface's and the code's.
pub(crate) fn families() -> [FontFamily; 2] {
    [FontFamily::Proportional, FontFamily::Monospace]
}

#[cfg(test)]
mod tests;
