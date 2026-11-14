// SPDX-License-Identifier: GPL-3.0-or-later

//! What id's `qbsp` reads from a `.map`: a model of its `LoadMapFile` and the steps after it
//! that shape the BSP's entity and texture lumps, written from `docs/spec/map.md`.
//!
//! qbsp is in `qutils/` of id-Software/Quake-Tools, which is not a portable upstream
//! (`docs/porting.md`), so nothing here is ported: it is the behaviour the spec states, which
//! was recorded by running qbsp's parser on the vectors in `docs/spec/vectors/map/`. It
//! answers the question a mapper asks before compiling with id's tools: does qbsp read this
//! file, and what does it make of it? [`crate::MapFile`] is the reader to edit with.

#[cfg(test)]
mod tests;

use core::fmt;

use crate::cnum::{c_atof, c_atoi, scan_vector};
use crate::geometry::{Plane, TexVecs, on_plane, qbsp_plane, texvecs, to_f32};
use crate::lexer::{Gap, LexError, Lexer, Token, TokenLimit};

/// The most entities a map may hold, `MAX_MAP_ENTITIES` (MAP-2.8).
pub const MAP_MAX_ENTITIES: usize = 1024;

/// The size of qbsp's key buffer, `MAX_KEY`; a key holds at most 30 bytes (MAP-2.4).
pub const MAP_MAX_KEY: usize = 32;

/// The size of qbsp's value buffer, `MAX_VALUE`; a value holds at most 1022 bytes (MAP-2.4).
pub const MAP_MAX_VALUE: usize = 1024;

/// The most texture names, and the most texinfo, a map may use, `MAX_MAP_TEXINFO` (MAP-4.7).
pub const MAP_MAX_TEXINFO: usize = 4096;

/// The longest texture name, in bytes: qbsp's 16-byte slot less its NUL (MAP-4.6).
pub const MAP_MAX_TEXTURE_NAME: usize = 15;

/// The most brushes a map may hold, `MAX_MAP_BRUSHES`; qbsp does not check it (MAP-3).
pub const MAP_MAX_BRUSHES: usize = 4096;

/// The size of the line qbsp formats each pair into when it writes the entity lump,
/// `char line[128]` in `WriteEntitiesToString` (MAP-2.10).
pub const MAP_LUMP_LINE: usize = 128;

/// The size of the entity lump buffer, `MAX_MAP_ENTSTRING` (BSP-16).
pub const MAP_MAX_ENTSTRING: usize = 65536;

/// `TEX_SPECIAL`, the texinfo flag for sky and liquids (MAP-4.5, BSP-8.5).
pub const MAP_TEX_SPECIAL: i32 = 1;

/// Why qbsp stops reading a map. The messages are qbsp's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QbspError {
    /// A token could not be read (MAP-1.4, MAP-1.6).
    Lex(LexError),
    /// A token that must be on the current line is not (MAP-1.7).
    LineIncomplete {
        /// The line qbsp names (MAP-1.9).
        line: u32,
    },
    /// An entity does not start with `{` (MAP-2.1).
    NoOpenBrace,
    /// The file ends inside an entity (MAP-2.2).
    EofWithoutBrace,
    /// A key or value is too long (MAP-2.4); qbsp's message misspells `ParseEpair`.
    TokenTooLong,
    /// The map has more than [`MAP_MAX_ENTITIES`] entities (MAP-2.8).
    TooManyEntities,
    /// A face is not three points in parentheses (MAP-3.1, MAP-3.2, MAP-5.5).
    ParsingBrush,
    /// The map uses more than [`MAP_MAX_TEXINFO`] texture names (MAP-4.7).
    TooManyMiptex,
    /// The map uses more than [`MAP_MAX_TEXINFO`] texinfo (MAP-4.7).
    TooManyTexinfo,
    /// A texture name is longer than [`MAP_MAX_TEXTURE_NAME`] bytes, which overflows qbsp's
    /// slot (MAP-4.6).
    TextureNameTooLong {
        /// The name.
        name: Vec<u8>,
    },
    /// A pair overflows qbsp's line buffer when it writes the entity lump (MAP-2.10).
    PairTooLong {
        /// The key.
        key: Vec<u8>,
        /// The value.
        value: Vec<u8>,
    },
    /// The entity lump would exceed [`MAP_MAX_ENTSTRING`] bytes.
    EntityTextTooLong,
}

impl fmt::Display for QbspError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lex(error) => error.fmt(f),
            Self::LineIncomplete { line } => write!(f, "Line {line} is incomplete"),
            Self::NoOpenBrace => f.write_str("ParseEntity: { not found"),
            Self::EofWithoutBrace => f.write_str("ParseEntity: EOF without closing brace"),
            Self::TokenTooLong => f.write_str("ParseEpar: token too long"),
            Self::TooManyEntities => f.write_str("num_entities == MAX_MAP_ENTITIES"),
            Self::ParsingBrush => f.write_str("parsing brush"),
            Self::TooManyMiptex => f.write_str("nummiptex == MAX_MAP_TEXINFO"),
            Self::TooManyTexinfo => f.write_str("numtexinfo == MAX_MAP_TEXINFO"),
            Self::TextureNameTooLong { name } => write!(
                f,
                "texture name {} is longer than {MAP_MAX_TEXTURE_NAME} bytes",
                name.escape_ascii()
            ),
            Self::PairTooLong { key, .. } => write!(
                f,
                "the pair with key {} is longer than qbsp's entity lump line",
                key.escape_ascii()
            ),
            Self::EntityTextTooLong => f.write_str("Entity text too long"),
        }
    }
}

impl core::error::Error for QbspError {}

impl From<LexError> for QbspError {
    fn from(error: LexError) -> Self {
        Self::Lex(error)
    }
}

/// Something qbsp prints and carries on after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QbspWarning {
    /// A face lies on an earlier face's plane and is dropped (MAP-3.5).
    DuplicatePlane {
        /// The line of the face.
        line: u32,
    },
    /// A face's points are collinear; qbsp stops reading the brush there (MAP-3.6).
    NoNormal {
        /// The line of the face.
        line: u32,
    },
    /// The world names no WAD, so the textures lump is empty (MAP-6.3).
    NoWadFile,
    /// A texture is not in the WAD; its slot is empty (MAP-6.8).
    TextureNotFound {
        /// The texture's name.
        name: Vec<u8>,
    },
}

impl fmt::Display for QbspWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicatePlane { .. } => f.write_str("WARNING: brush with duplicate plane"),
            Self::NoNormal { .. } => f.write_str("WARNING: brush plane with no normal"),
            Self::NoWadFile => f.write_str("WARNING: no wadfile specified"),
            Self::TextureNotFound { name } => {
                write!(f, "WARNING: texture {} not found", name.escape_ascii())
            }
        }
    }
}

/// A face qbsp keeps (MAP-3, MAP-4).
#[derive(Clone, Debug, PartialEq)]
pub struct QbspFace {
    /// The three points, read with `atoi` (MAP-3.3).
    pub points: [[f32; 3]; 3],
    /// Its plane (MAP-3.4).
    pub plane: Plane<f32>,
    /// Its texinfo, an index into [`QbspMap::texinfo`] (MAP-4.7).
    pub texinfo: usize,
}

/// A texinfo (MAP-4.4, MAP-4.5, BSP-8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QbspTexinfo {
    /// The s and t axes and offsets.
    pub vecs: TexVecs<f32>,
    /// The texture, an index into [`QbspMap::miptex`].
    pub miptex: usize,
    /// [`MAP_TEX_SPECIAL`] or 0.
    pub flags: i32,
}

/// An entity as qbsp holds it (MAP-2).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QbspEntity {
    /// Every pair, in file order, duplicates included (MAP-2.5).
    pub pairs: Vec<(Vec<u8>, Vec<u8>)>,
    /// Each brush's kept faces, in file order.
    pub brushes: Vec<Vec<QbspFace>>,
    /// The `origin` key (MAP-2.7).
    pub origin: [f32; 3],
    /// The line of its `{`.
    pub line: u32,
}

impl QbspEntity {
    /// The value of `key`: its last occurrence, as qbsp's `ValueForKey` finds it (MAP-2.5).
    #[must_use]
    pub fn value_for_key(&self, key: &[u8]) -> Option<&[u8]> {
        self.pairs
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_slice())
    }

    /// Sets `key` as qbsp's `SetKeyValue` does: the last occurrence takes the value, or a
    /// new pair is added after the others.
    pub fn set_key_value(&mut self, key: &[u8], value: &[u8]) {
        match self.pairs.iter_mut().rev().find(|(name, _)| name == key) {
            Some((_, old)) => value.clone_into(old),
            None => self.pairs.push((key.to_vec(), value.to_vec())),
        }
    }
}

/// A texture slot of the BSP's textures lump (MAP-6.8, BSP-4.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureSlot {
    /// The name as the map gives it, or as qbsp makes it for an animation frame.
    pub name: Vec<u8>,
    /// Whether the WAD holds it; a missing texture gets the offset -1.
    pub found: bool,
}

/// A map as qbsp reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QbspMap {
    /// The entities, in file order; the first is the world (MAP-6.1).
    pub entities: Vec<QbspEntity>,
    /// The distinct texture names, in first-use order (MAP-4.7).
    pub miptex: Vec<Vec<u8>>,
    /// The distinct texinfo, in first-use order (MAP-4.7).
    pub texinfo: Vec<QbspTexinfo>,
    /// What qbsp printed while reading.
    pub warnings: Vec<QbspWarning>,
}

/// The world's keys a WAD path is read from, in order: `_wad`, then `wad` (MAP-6.3).
pub(crate) const MAP_WAD_KEYS: [&[u8]; 2] = [b"_wad", b"wad"];

/// Whether qbsp refuses a key of `len` bytes, which with its NUL overflows its `MAX_KEY`
/// buffer (MAP-2.4).
pub(crate) const fn key_too_long(len: usize) -> bool {
    len >= MAP_MAX_KEY.saturating_sub(1)
}

/// Whether qbsp refuses a value of `len` bytes, as [`key_too_long`] for `MAX_VALUE` (MAP-2.4).
pub(crate) const fn value_too_long(len: usize) -> bool {
    len >= MAP_MAX_VALUE.saturating_sub(1)
}

/// Whether a pair written as `"key" "value"` with its line break and NUL overflows qbsp's
/// 128-byte entity lump line (MAP-2.10).
pub(crate) const fn pair_too_long(key_len: usize, value_len: usize) -> bool {
    key_len.saturating_add(value_len).saturating_add(7) > MAP_LUMP_LINE
}

/// Whether a texture name of `len` bytes overflows qbsp's 16-byte miptex name (MAP-4.6).
pub(crate) const fn texture_name_too_long(len: usize) -> bool {
    len > MAP_MAX_TEXTURE_NAME
}

/// qbsp's tokenizer: `GetToken` with its `crossline` argument.
struct Reader<'a> {
    lexer: Lexer<'a>,
}

impl<'a> Reader<'a> {
    /// The next token, across line breaks, `GetToken (true)`; `None` at the end of the file.
    fn next(&mut self) -> Result<Option<Token<'a>>, QbspError> {
        match self.lexer.skip() {
            Gap::End(_) => Ok(None),
            Gap::Token(gap_break) => Ok(Some(self.lexer.token(gap_break)?)),
        }
    }

    /// The next token, which must be on the current line, `GetToken (false)`: a line break
    /// before it, or the end of the file, is an error (MAP-1.7).
    fn same_line(&mut self) -> Result<Token<'a>, QbspError> {
        self.lexer
            .same_line_token()?
            .map_err(|line_break| QbspError::LineIncomplete {
                line: line_break.qbsp_line,
            })
    }
}

/// Reads a map as qbsp's `LoadMapFile` does.
///
/// Implements MAP-1 to MAP-4 as qbsp behaves: the first error stops the read, as it stops
/// qbsp; warnings are kept in [`QbspMap::warnings`].
///
/// # Errors
///
/// The [`QbspError`] qbsp stops with.
pub fn load_map_file(bytes: &[u8]) -> Result<QbspMap, QbspError> {
    let mut reader = Reader {
        lexer: Lexer::new(bytes, TokenLimit::Qbsp),
    };
    let mut map = QbspMap::default();
    while let Some(open) = reader.next()? {
        if !open.is(b"{") {
            return Err(QbspError::NoOpenBrace);
        }
        if map.entities.len() == MAP_MAX_ENTITIES {
            return Err(QbspError::TooManyEntities);
        }
        let mut entity = QbspEntity {
            line: open.line,
            ..QbspEntity::default()
        };
        loop {
            let token = reader.next()?.ok_or(QbspError::EofWithoutBrace)?;
            if token.is(b"}") {
                break;
            }
            if token.is(b"{") {
                let brush = map.parse_brush(&mut reader)?;
                entity.brushes.push(brush);
            } else {
                if key_too_long(token.text.len()) {
                    return Err(QbspError::TokenTooLong);
                }
                let value = reader.same_line()?;
                if value_too_long(value.text.len()) {
                    return Err(QbspError::TokenTooLong);
                }
                entity
                    .pairs
                    .push((token.text.to_vec(), value.text.to_vec()));
            }
        }
        let origin = scan_vector(entity.value_for_key(b"origin").unwrap_or_default());
        entity.origin = origin.map(to_f32);
        map.entities.push(entity);
    }
    Ok(map)
}

/// An `int` stored in a `float`.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec MAP-3.3, MAP-4.1: qbsp stores atoi results in floats, rounding to nearest"
)]
const fn int_to_f32(value: i32) -> f32 {
    value as f32
}

impl QbspMap {
    /// qbsp's `ParseBrush`: faces up to `}` or the end of the file (MAP-3, MAP-4).
    fn parse_brush(&mut self, reader: &mut Reader<'_>) -> Result<Vec<QbspFace>, QbspError> {
        let mut faces: Vec<QbspFace> = Vec::new();
        while let Some(first) = reader.next()? {
            if first.is(b"}") {
                break;
            }
            let line = first.line;
            let mut points = [[0.0_f32; 3]; 3];
            for (index, point) in points.iter_mut().enumerate() {
                // At the end of the file qbsp compares the stale `)`, which fails too.
                let open = if index == 0 {
                    Some(first)
                } else {
                    reader.next()?
                };
                if !open.is_some_and(|token| token.is(b"(")) {
                    return Err(QbspError::ParsingBrush);
                }
                for value in point.iter_mut() {
                    *value = int_to_f32(c_atoi(reader.same_line()?.text));
                }
                if !reader.same_line()?.is(b")") {
                    return Err(QbspError::ParsingBrush);
                }
            }
            let miptex = self.find_miptex(reader.same_line()?.text)?;
            let shift_s = int_to_f32(c_atoi(reader.same_line()?.text));
            let shift_t = int_to_f32(c_atoi(reader.same_line()?.text));
            let rotate = int_to_f32(c_atoi(reader.same_line()?.text));
            let scale_s = to_f32(c_atof(reader.same_line()?.text));
            let scale_t = to_f32(c_atof(reader.same_line()?.text));
            if faces.iter().any(|face| on_plane(&points, &face.plane)) {
                self.warnings.push(QbspWarning::DuplicatePlane { line });
                continue;
            }
            let Some(plane) = qbsp_plane(points) else {
                self.warnings.push(QbspWarning::NoNormal { line });
                break;
            };
            let vecs = texvecs(plane.normal, [shift_s, shift_t], rotate, [scale_s, scale_t]);
            let texinfo = self.find_texinfo(vecs, miptex)?;
            faces.push(QbspFace {
                points,
                plane,
                texinfo,
            });
        }
        Ok(faces)
    }

    /// qbsp's `FindMiptex`: the slot of `name`, added in first-use order (MAP-4.6, MAP-4.7).
    fn find_miptex(&mut self, name: &[u8]) -> Result<usize, QbspError> {
        if let Some(index) = self.miptex.iter().position(|known| known == name) {
            return Ok(index);
        }
        if self.miptex.len() == MAP_MAX_TEXINFO {
            return Err(QbspError::TooManyMiptex);
        }
        if texture_name_too_long(name.len()) {
            return Err(QbspError::TextureNameTooLong {
                name: name.to_vec(),
            });
        }
        self.miptex.push(name.to_vec());
        Ok(self.miptex.len().saturating_sub(1))
    }

    /// qbsp's `FindTexinfo`: sets `TEX_SPECIAL` and finds or adds the texinfo, comparing
    /// every axis value exactly (MAP-4.5, MAP-4.7).
    fn find_texinfo(&mut self, vecs: TexVecs<f32>, miptex: usize) -> Result<usize, QbspError> {
        let name = self
            .miptex
            .get(miptex)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let flags = if is_special(name) { MAP_TEX_SPECIAL } else { 0 };
        let same = |known: &QbspTexinfo| {
            known.miptex == miptex
                && known.flags == flags
                && known
                    .vecs
                    .iter()
                    .flatten()
                    .zip(vecs.iter().flatten())
                    .all(|(a, b)| exactly_equal(*a, *b))
        };
        if let Some(index) = self.texinfo.iter().position(same) {
            return Ok(index);
        }
        if self.texinfo.len() == MAP_MAX_TEXINFO {
            return Err(QbspError::TooManyTexinfo);
        }
        self.texinfo.push(QbspTexinfo {
            vecs,
            miptex,
            flags,
        });
        Ok(self.texinfo.len().saturating_sub(1))
    }

    /// The world's WAD path: `_wad`, or `wad` when that is missing or empty; `None` when
    /// both are (MAP-6.3).
    #[must_use]
    pub fn wad_path(&self) -> Option<&[u8]> {
        let world = self.entities.first()?;
        MAP_WAD_KEYS
            .into_iter()
            .filter_map(|key| world.value_for_key(key))
            .find(|value| !value.is_empty())
    }

    /// Sets each brush entity's `model` key to `*<n>`, numbering the entities after the
    /// world that have a brush from 1 in file order, as qbsp's `UpdateEntLump` does
    /// (MAP-6.2).
    pub fn number_models(&mut self) {
        let mut number = 1_usize;
        for entity in self.entities.iter_mut().skip(1) {
            if entity.brushes.is_empty() {
                continue;
            }
            entity.set_key_value(b"model", format!("*{number}").as_bytes());
            number = number.saturating_add(1);
        }
    }

    /// The entity lump qbsp writes, its NUL included: each entity with pairs as `{`, its
    /// pairs in reverse file order, one `"key" "value"` per line, and `}` (MAP-2.6).
    ///
    /// # Errors
    ///
    /// [`QbspError::PairTooLong`] for a pair that overflows qbsp's line (MAP-2.10), and
    /// [`QbspError::EntityTextTooLong`] past [`MAP_MAX_ENTSTRING`].
    pub fn entity_lump(&self) -> Result<Vec<u8>, QbspError> {
        let mut out = Vec::new();
        for entity in &self.entities {
            if entity.pairs.is_empty() {
                continue;
            }
            out.extend_from_slice(b"{\n");
            for (key, value) in entity.pairs.iter().rev() {
                if pair_too_long(key.len(), value.len()) {
                    return Err(QbspError::PairTooLong {
                        key: key.clone(),
                        value: value.clone(),
                    });
                }
                out.push(b'"');
                out.extend_from_slice(key);
                out.extend_from_slice(b"\" \"");
                out.extend_from_slice(value);
                out.extend_from_slice(b"\"\n");
            }
            out.extend_from_slice(b"}\n");
            if out.len() > MAP_MAX_ENTSTRING {
                return Err(QbspError::EntityTextTooLong);
            }
        }
        out.push(0);
        Ok(out)
    }

    /// The textures lump's slots qbsp writes when its WAD holds `lump_names` (MAP-6.3,
    /// MAP-6.8): every texture name the map uses, then for each one starting with `+` the
    /// frames `+0` to `+9` and `+A` to `+J` of its animation that the WAD holds, compared
    /// with the WAD's names upper-cased; a slot whose texture the WAD lacks is not found.
    /// With no WAD path there are no slots and qbsp warns.
    ///
    /// # Errors
    ///
    /// [`QbspError::TooManyMiptex`] when the frames take the map past [`MAP_MAX_TEXINFO`].
    pub fn texture_slots(
        &self,
        lump_names: &[&[u8]],
    ) -> Result<(Vec<TextureSlot>, Vec<QbspWarning>), QbspError> {
        if self.wad_path().is_none() {
            return Ok((Vec::new(), vec![QbspWarning::NoWadFile]));
        }
        let lumps: Vec<Vec<u8>> = lump_names.iter().map(|name| cleanup_name(name)).collect();
        let mut names = self.miptex.clone();
        for base in &self.miptex {
            if base.first() != Some(&b'+') {
                continue;
            }
            for frame in (b'0'..=b'9').chain(b'A'..=b'J') {
                let mut name = base.clone();
                if let Some(second) = name.get_mut(1) {
                    *second = frame;
                }
                // AddAnimatingTextures compares the name as the map spells it.
                if lumps.contains(&name) && !names.contains(&name) {
                    if names.len() == MAP_MAX_TEXINFO {
                        return Err(QbspError::TooManyMiptex);
                    }
                    names.push(name);
                }
            }
        }
        let mut warnings = Vec::new();
        let slots = names
            .into_iter()
            .map(|name| {
                let found = lumps.contains(&cleanup_name(&name));
                if !found {
                    warnings.push(QbspWarning::TextureNotFound { name: name.clone() });
                }
                TextureSlot { name, found }
            })
            .collect();
        Ok((slots, warnings))
    }
}

/// qbsp's `CleanupName`: the name up to its NUL and 16 bytes, upper-cased (MAP-6.8).
fn cleanup_name(name: &[u8]) -> Vec<u8> {
    name.iter()
        .take(16)
        .take_while(|&&byte| byte != 0)
        .map(u8::to_ascii_uppercase)
        .collect()
}

/// Whether qbsp gives a texture `TEX_SPECIAL`: its name starts with `*` or with `sky` in
/// any case (MAP-4.5).
#[must_use]
pub fn is_special(name: &[u8]) -> bool {
    name.first() == Some(&b'*')
        || name
            .get(..3)
            .is_some_and(|start| start.eq_ignore_ascii_case(b"sky"))
}

#[expect(
    clippy::float_cmp,
    reason = "spec MAP-4.7: qbsp compares texinfo axis values exactly"
)]
fn exactly_equal(a: f32, b: f32) -> bool {
    a == b
}
