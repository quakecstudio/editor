// SPDX-License-Identifier: GPL-3.0-or-later

//! Map diagnostics: what id's qbsp, ericw-tools or the game make of a map that the
//! lossless reader reads (MAP-1 to MAP-7).

#[cfg(test)]
mod tests;

use core::fmt;
use std::collections::BTreeSet;

use crate::cnum::c_atof;
use crate::document::{Entity, MapFile};
use crate::geometry::on_plane;
use crate::lexer::MAP_MAX_TOKEN;
use crate::qbsp::{
    MAP_MAX_ENTITIES, MAP_MAX_TEXINFO, QbspError, key_too_long, load_map_file, pair_too_long,
    texture_name_too_long, value_too_long,
};

/// The largest CD track the world's `sounds` key can name: it is sent as one byte (MAP-6.6).
pub const MAP_MAX_SOUNDS: f64 = 255.0;

/// What a diagnostic is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// A key or value longer than 127 bytes, which qbsp cannot read (MAP-1.6).
    TokenTooLarge,
    /// A `;` outside quotes, which ericw-tools takes as a comment (MAP-1.8).
    EricwComment,
    /// A quoted token ending in a backslash, whose closing quote ericw-tools takes as
    /// escaped (MAP-1.8).
    EricwEscape,
    /// A key of 31 bytes or more, or a value of 1023 or more, which qbsp rejects (MAP-2.4).
    KeyTooLong,
    /// A key that appears more than once: qbsp looks it up by its last occurrence (MAP-2.5)
    /// but writes the pairs in reverse, so the game sees the first occurrence of a map qbsp
    /// compiled (MAP-2.6); ericw-tools keeps the last value (MAP-2.5).
    DuplicateKey {
        /// The key.
        key: Vec<u8>,
    },
    /// The 1025th entity, which qbsp rejects (MAP-2.8).
    TooManyEntities,
    /// A key with whitespace at either end, which ericw-tools removes (MAP-2.9).
    KeyWhitespace,
    /// A key and value longer together than qbsp's entity lump line holds (MAP-2.10).
    PairTooLong,
    /// A face whose numbers id's qbsp reads otherwise: a fraction, which `atoi` drops
    /// (MAP-3.3, MAP-4.1, MAP-7.4).
    QbspNumber,
    /// A face on the plane of an earlier face of its brush, which both compilers drop
    /// (MAP-3.5).
    DuplicatePlane,
    /// A face whose points are collinear (MAP-3.6).
    NoNormal,
    /// A texture name longer than 15 bytes (MAP-4.6).
    TextureNameTooLong,
    /// The map uses more than 4096 texture names or texinfo, which qbsp rejects (MAP-4.7).
    TooManyTextures,
    /// A Valve 220 face, which id's qbsp cannot read (MAP-5.5).
    Valve220,
    /// A Valve 220 face with a scale of 0, which has no texinfo (MAP-5.4).
    ValveScaleZero,
    /// The world names no WAD (MAP-6.3).
    NoWad,
    /// The world's `message` holds `%`, which id's server uses as a format (MAP-6.5).
    MessagePercent,
    /// The world's `sounds` is outside 0 to 255 (MAP-6.6).
    SoundsRange,
    /// A key or value holding a LF (MAP-7.1).
    LineBreakInToken,
}

impl fmt::Display for DiagnosticKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TokenTooLarge => "a token longer than 127 bytes, which id's qbsp cannot read",
            Self::EricwComment => "a ; outside quotes, which ericw-tools reads as a comment",
            Self::EricwEscape => "a quoted token ending in \\, which ericw-tools reads on",
            Self::KeyTooLong => "a key or value too long for id's qbsp",
            Self::DuplicateKey { .. } => {
                "a duplicated key: qbsp reads the last, the game the first, ericw-tools the last"
            }
            Self::TooManyEntities => "more than 1024 entities",
            Self::KeyWhitespace => "a key with whitespace at an end, which ericw-tools removes",
            Self::PairTooLong => "a key and value too long for id's qbsp's entity lump",
            Self::QbspNumber => "a number id's qbsp reads as another value",
            Self::DuplicatePlane => "a face on the plane of an earlier face, which is dropped",
            Self::NoNormal => "a face whose points are collinear",
            Self::TextureNameTooLong => "a texture name longer than 15 bytes",
            Self::TooManyTextures => "more than 4096 texture names or texinfo",
            Self::Valve220 => "a Valve 220 face, which id's qbsp cannot read",
            Self::ValveScaleZero => "a Valve 220 face with a scale of 0",
            Self::NoWad => "the world names no WAD",
            Self::MessagePercent => "the world's message holds %",
            Self::SoundsRange => "the world's sounds is outside 0 to 255",
            Self::LineBreakInToken => "a key or value holding a line break",
        })
    }
}

/// A diagnostic and the line it is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// The 1-based line, or 0 for an element not read from a file.
    pub line: u32,
    /// What it is.
    pub kind: DiagnosticKind,
}

impl MapFile {
    /// Every diagnostic, in file order of the elements, then those about the whole map.
    #[must_use]
    pub fn check(&self) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        let mut textures = BTreeSet::new();
        let mut faces = 0_usize;
        for (index, entity) in self.entities.iter().enumerate() {
            if index == MAP_MAX_ENTITIES {
                out.push(Diagnostic {
                    line: entity.line(),
                    kind: DiagnosticKind::TooManyEntities,
                });
            }
            entity.check_pairs(&mut out);
            for brush in entity.brushes() {
                let mut kept = Vec::new();
                for face in &brush.faces {
                    let mut push = |kind| {
                        out.push(Diagnostic {
                            line: face.line(),
                            kind,
                        });
                    };
                    faces = faces.saturating_add(1);
                    textures.insert(face.texture());
                    if texture_name_too_long(face.texture().len()) {
                        push(DiagnosticKind::TextureNameTooLong);
                    }
                    if let crate::TexDef::Valve { scale, .. } = face.texdef() {
                        push(DiagnosticKind::Valve220);
                        if scale.contains(&0.0) {
                            push(DiagnosticKind::ValveScaleZero);
                        }
                    } else if face.qbsp_differs() {
                        push(DiagnosticKind::QbspNumber);
                    }
                    if kept.iter().any(|plane| on_plane(face.points(), plane)) {
                        push(DiagnosticKind::DuplicatePlane);
                        continue;
                    }
                    match face.plane() {
                        Some(plane) => kept.push(plane),
                        None => push(DiagnosticKind::NoNormal),
                    }
                }
            }
        }
        self.check_world(&mut out);
        let too_many = textures.len() > MAP_MAX_TEXINFO
            || (faces > MAP_MAX_TEXINFO
                && matches!(
                    load_map_file(&self.to_bytes()),
                    Err(QbspError::TooManyTexinfo | QbspError::TooManyMiptex)
                ));
        if too_many {
            out.push(Diagnostic {
                line: 0,
                kind: DiagnosticKind::TooManyTextures,
            });
        }
        out
    }

    fn check_world(&self, out: &mut Vec<Diagnostic>) {
        let Some(world) = self.world() else {
            return;
        };
        let line = world.line();
        if self.wad_paths(crate::Compiler::Qbsp).is_empty() {
            out.push(Diagnostic {
                line,
                kind: DiagnosticKind::NoWad,
            });
        }
        if world
            .value(b"message")
            .is_some_and(|message| message.contains(&b'%'))
        {
            out.push(Diagnostic {
                line,
                kind: DiagnosticKind::MessagePercent,
            });
        }
        if let Some(sounds) = world.value(b"sounds") {
            // The engine converts the float to an int and sends its low byte (MAP-6.6).
            let track = c_atof(sounds).trunc();
            if !(0.0..=MAP_MAX_SOUNDS).contains(&track) {
                out.push(Diagnostic {
                    line,
                    kind: DiagnosticKind::SoundsRange,
                });
            }
        }
    }
}

impl Entity {
    /// The diagnostics of the entity's pairs.
    fn check_pairs(&self, out: &mut Vec<Diagnostic>) {
        let mut seen = BTreeSet::new();
        for pair in self.pairs() {
            let mut push = |kind| {
                out.push(Diagnostic {
                    line: pair.line(),
                    kind,
                });
            };
            let (key, value) = (pair.key(), pair.value());
            if key.len() >= MAP_MAX_TOKEN || value.len() >= MAP_MAX_TOKEN {
                push(DiagnosticKind::TokenTooLarge);
            }
            if key_too_long(key.len()) || value_too_long(value.len()) {
                push(DiagnosticKind::KeyTooLong);
            }
            if pair_too_long(key.len(), value.len()) {
                push(DiagnosticKind::PairTooLong);
            }
            if !seen.insert(key) {
                push(DiagnosticKind::DuplicateKey { key: key.to_vec() });
            }
            if key.first().is_some_and(u8::is_ascii_whitespace)
                || key.last().is_some_and(u8::is_ascii_whitespace)
            {
                push(DiagnosticKind::KeyWhitespace);
            }
            if key.contains(&b'\n') || value.contains(&b'\n') {
                push(DiagnosticKind::LineBreakInToken);
            }
            let unquoted =
                |raw: &[u8], text: &[u8]| raw.first() != Some(&b'"') && text.contains(&b';');
            if unquoted(pair.key_raw(), key) || unquoted(pair.value_raw(), value) {
                push(DiagnosticKind::EricwComment);
            }
            if pair.ericw_escape() {
                push(DiagnosticKind::EricwEscape);
            }
        }
    }
}
