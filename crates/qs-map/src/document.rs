// SPDX-License-Identifier: GPL-3.0-or-later

//! The lossless `.map` document: every byte of the file is kept, so a file read and written
//! unchanged comes back byte for byte, and an edit changes only the text it touches.
//!
//! It reads what either compiler reads (MAP-1 to MAP-5): the standard and Valve 220 forms,
//! decided face by face (MAP-5.2), numbers as doubles (MAP-3.3), tokens of any length.
//! [`MapFile::check`] reports what id's qbsp or the game would make of it otherwise.

#[cfg(test)]
mod tests;

use core::fmt;

use crate::cnum::{c_atof, c_atoi, c_strtod, scan_vector};
use crate::geometry::{Plane, TexVecs, plane, standard_texvecs, valve_texvecs};
use crate::lexer::{Gap, LexError, Lexer, Token, is_map_space};

/// Why a map cannot be read, or an edit cannot be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapError {
    /// A token cannot be read: the text ends inside a quoted token (MAP-1.4).
    Lex(LexError),
    /// A token that must be on the current line is not (MAP-1.7).
    LineIncomplete {
        /// The line that ends too early.
        line: u32,
    },
    /// An entity does not start with `{` (MAP-2.1).
    NoOpenBrace {
        /// The line of the token found instead.
        line: u32,
    },
    /// The file ends inside an entity (MAP-2.2).
    EofWithoutBrace {
        /// The line of the entity's `{`.
        line: u32,
    },
    /// A face does not start with three points in parentheses (MAP-3.1, MAP-3.2).
    ParsingBrush {
        /// The line of the face.
        line: u32,
    },
    /// A face's number is not a number.
    BadNumber {
        /// The line of the face.
        line: u32,
        /// The token.
        text: Vec<u8>,
    },
    /// A Valve 220 face lacks a bracket where one belongs (MAP-5.1).
    Valve220 {
        /// The line of the face.
        line: u32,
    },
    /// A Valve 220 face to write has a scale of 0 (MAP-5.4). A file read with one keeps it,
    /// and [`MapFile::check`] reports it.
    ValveScaleZero {
        /// The line of the face.
        line: u32,
    },
    /// A key, value or texture name holds a byte its token cannot: `"` or NUL, or for a
    /// texture name whitespace; or is empty where a word is needed.
    Unwritable {
        /// The text.
        text: Vec<u8>,
    },
    /// A number to write is infinite or NaN.
    NotFinite,
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lex(error) => error.fmt(f),
            Self::LineIncomplete { line } => write!(f, "Line {line} is incomplete"),
            Self::NoOpenBrace { line } => write!(f, "line {line}: ParseEntity: {{ not found"),
            Self::EofWithoutBrace { line } => {
                write!(f, "line {line}: ParseEntity: EOF without closing brace")
            }
            Self::ParsingBrush { line } => write!(f, "line {line}: parsing brush"),
            Self::BadNumber { line, text } => {
                write!(f, "line {line}: {} is not a number", text.escape_ascii())
            }
            Self::Valve220 { line } => {
                write!(f, "line {line}: couldn't parse Valve220 texture info")
            }
            Self::ValveScaleZero { line } => {
                write!(f, "line {line}: a Valve 220 face has a scale of 0")
            }
            Self::Unwritable { text } => {
                write!(f, "{} cannot be written as a token", text.escape_ascii())
            }
            Self::NotFinite => f.write_str("a number to write is infinite or NaN"),
        }
    }
}

impl core::error::Error for MapError {}

impl From<LexError> for MapError {
    fn from(error: LexError) -> Self {
        Self::Lex(error)
    }
}

/// A face's texture definition (MAP-4, MAP-5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TexDef {
    /// The standard form: shift, rotation in degrees and scale (MAP-4.1).
    Standard {
        /// The s and t shift.
        shift: [f64; 2],
        /// The rotation in degrees.
        rotate: f64,
        /// The s and t scale; 0 is taken as 1 (MAP-4.4).
        scale: [f64; 2],
    },
    /// The Valve 220 form: the axes themselves (MAP-5.1).
    Valve {
        /// The s axis and shift.
        s: [f64; 4],
        /// The t axis and shift.
        t: [f64; 4],
        /// The rotation, kept for editors (MAP-5.3).
        rotate: f64,
        /// The s and t scale; a face with a scale of 0 has no texinfo (MAP-5.4).
        scale: [f64; 2],
    },
}

/// One face of a brush: three points and a texture (MAP-3.2).
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    leading: Vec<u8>,
    raw: Vec<u8>,
    line: u32,
    points: [[f64; 3]; 3],
    texture: Vec<u8>,
    texdef: TexDef,
    qbsp_differs: bool,
}

impl Face {
    /// A new face, written in the form MAP-7 describes.
    ///
    /// # Errors
    ///
    /// [`MapError::Unwritable`] for a texture name that is empty or holds whitespace, `"`
    /// or NUL; [`MapError::NotFinite`]; [`MapError::ValveScaleZero`] for a Valve 220 scale
    /// of 0.
    pub fn new(points: [[f64; 3]; 3], texture: &[u8], texdef: TexDef) -> Result<Self, MapError> {
        let mut face = Self {
            leading: b"\n".to_vec(),
            raw: Vec::new(),
            line: 0,
            points,
            texture: Vec::new(),
            texdef,
            qbsp_differs: false,
        };
        face.set(points, texture, texdef)?;
        Ok(face)
    }

    /// Replaces the face's points, texture and definition, rewriting its text.
    ///
    /// # Errors
    ///
    /// As [`Face::new`].
    pub fn set(
        &mut self,
        points: [[f64; 3]; 3],
        texture: &[u8],
        texdef: TexDef,
    ) -> Result<(), MapError> {
        if texture.is_empty()
            || texture
                .iter()
                .any(|&byte| byte == b'"' || is_map_space(byte))
        {
            return Err(MapError::Unwritable {
                text: texture.to_vec(),
            });
        }
        let mut integers = Vec::new();
        for value in points.iter().flatten() {
            integers.push(number(*value)?);
        }
        let mut words = Vec::new();
        let mut scales = Vec::new();
        match texdef {
            TexDef::Standard {
                shift,
                rotate,
                scale,
            } => {
                for value in [shift[0], shift[1], rotate] {
                    integers.push(number(value)?);
                }
                for value in scale {
                    scales.push(number(value)?);
                }
            }
            TexDef::Valve {
                s,
                t,
                rotate,
                scale,
            } => {
                if scale.contains(&0.0) {
                    return Err(MapError::ValveScaleZero { line: self.line });
                }
                for axis in [s, t] {
                    words.push("[".to_owned());
                    for value in axis {
                        words.push(number(value)?);
                    }
                    words.push("]".to_owned());
                }
                for value in [rotate, scale[0], scale[1]] {
                    words.push(number(value)?);
                }
            }
        }
        let mut raw = Vec::new();
        for (index, point) in integers.chunks(3).take(3).enumerate() {
            if index > 0 {
                raw.push(b' ');
            }
            raw.extend_from_slice(b"( ");
            for value in point {
                raw.extend_from_slice(value.as_bytes());
                raw.push(b' ');
            }
            raw.push(b')');
        }
        raw.push(b' ');
        raw.extend_from_slice(texture);
        for word in integers.iter().skip(9).chain(&scales).chain(&words) {
            raw.push(b' ');
            raw.extend_from_slice(word.as_bytes());
        }
        self.qbsp_differs = !words.is_empty()
            || integers
                .iter()
                .any(|text| !qbsp_integer_reads(text.as_bytes()))
            || scales
                .iter()
                .any(|text| !same_bits(c_atof(text.as_bytes()), c_strtod(text.as_bytes()).0));
        self.raw = raw;
        self.points = points;
        self.texture = texture.to_vec();
        self.texdef = texdef;
        Ok(())
    }

    /// The three points (MAP-3.2).
    #[must_use]
    pub const fn points(&self) -> &[[f64; 3]; 3] {
        &self.points
    }

    /// The texture name (MAP-4.1).
    #[must_use]
    pub fn texture(&self) -> &[u8] {
        &self.texture
    }

    /// The texture definition.
    #[must_use]
    pub const fn texdef(&self) -> &TexDef {
        &self.texdef
    }

    /// The 1-based line the face starts on, or 0 for a face not read from a file.
    #[must_use]
    pub const fn line(&self) -> u32 {
        self.line
    }

    /// The face's plane, or `None` when its points are collinear (MAP-3.4, MAP-3.6).
    #[must_use]
    pub fn plane(&self) -> Option<Plane<f64>> {
        plane(self.points)
    }

    /// The texinfo axes and offsets the face gives, or `None` when it has no plane or is a
    /// Valve 220 face with a scale of 0 (MAP-4.2 to MAP-4.4, MAP-5.3, MAP-5.4).
    #[must_use]
    pub fn texvecs(&self) -> Option<TexVecs<f64>> {
        match self.texdef {
            TexDef::Standard {
                shift,
                rotate,
                scale,
            } => Some(standard_texvecs(self.plane()?.normal, shift, rotate, scale)),
            TexDef::Valve { s, t, scale, .. } => valve_texvecs(s, t, scale),
        }
    }

    /// Whether id's qbsp reads the face differently: a Valve 220 face, which it cannot read
    /// (MAP-5.5), or a number that `atoi` or `atof` reads otherwise (MAP-3.3, MAP-4.1).
    #[must_use]
    pub const fn qbsp_differs(&self) -> bool {
        self.qbsp_differs
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.leading);
        out.extend_from_slice(&self.raw);
    }
}

/// A brush: its faces (MAP-3.1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Brush {
    leading: Vec<u8>,
    /// The faces, in file order.
    pub faces: Vec<Face>,
    closing: Vec<u8>,
    line: u32,
}

impl Brush {
    /// An empty brush; faces are added to [`Brush::faces`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            leading: b"\n{".to_vec(),
            faces: Vec::new(),
            closing: b"\n}".to_vec(),
            line: 0,
        }
    }

    /// The 1-based line of its `{`, or 0 for a brush not read from a file.
    #[must_use]
    pub const fn line(&self) -> u32 {
        self.line
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.leading);
        for face in &self.faces {
            face.write(out);
        }
        out.extend_from_slice(&self.closing);
    }
}

/// A key and its value (MAP-2.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair {
    leading: Vec<u8>,
    key_raw: Vec<u8>,
    between: Vec<u8>,
    value_raw: Vec<u8>,
    key: Vec<u8>,
    value: Vec<u8>,
    line: u32,
    ericw_escape: bool,
}

/// `text` as a quoted token, if it can be one: no `"` and no NUL (MAP-1.4).
fn quoted(text: &[u8]) -> Result<Vec<u8>, MapError> {
    if text.iter().any(|&byte| byte == b'"' || byte == 0) {
        return Err(MapError::Unwritable {
            text: text.to_vec(),
        });
    }
    let mut raw = Vec::with_capacity(text.len().saturating_add(2));
    raw.push(b'"');
    raw.extend_from_slice(text);
    raw.push(b'"');
    Ok(raw)
}

impl Pair {
    /// A new pair, both quoted and separated by one space (MAP-7.1, MAP-7.2).
    ///
    /// # Errors
    ///
    /// [`MapError::Unwritable`] when either holds `"` or NUL.
    pub fn new(key: &[u8], value: &[u8]) -> Result<Self, MapError> {
        Ok(Self {
            leading: b"\n".to_vec(),
            key_raw: quoted(key)?,
            between: b" ".to_vec(),
            value_raw: quoted(value)?,
            key: key.to_vec(),
            value: value.to_vec(),
            line: 0,
            ericw_escape: key.last() == Some(&b'\\'),
        })
    }

    /// The key.
    #[must_use]
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    /// The value.
    #[must_use]
    pub fn value(&self) -> &[u8] {
        &self.value
    }

    /// The 1-based line of the key, or 0 for a pair not read from a file.
    #[must_use]
    pub const fn line(&self) -> u32 {
        self.line
    }

    /// Whether the key or value is quoted and ends in a backslash with no CR or LF after its
    /// closing quote, which ericw-tools reads as an escaped quote (MAP-1.8).
    #[must_use]
    pub const fn ericw_escape(&self) -> bool {
        self.ericw_escape
    }

    /// The key as written, with its quotes if it has them.
    #[must_use]
    pub fn key_raw(&self) -> &[u8] {
        &self.key_raw
    }

    /// The value as written, with its quotes if it has them.
    #[must_use]
    pub fn value_raw(&self) -> &[u8] {
        &self.value_raw
    }

    /// Replaces the value, keeping the key and the text around it.
    ///
    /// # Errors
    ///
    /// [`MapError::Unwritable`] when the value holds `"` or NUL.
    pub fn set_value(&mut self, value: &[u8]) -> Result<(), MapError> {
        self.value_raw = quoted(value)?;
        self.value = value.to_vec();
        // What follows the value is not known here, so a trailing backslash is reported.
        self.ericw_escape = self.key_raw.ends_with(b"\\\"") || value.last() == Some(&b'\\');
        Ok(())
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.leading);
        out.extend_from_slice(&self.key_raw);
        out.extend_from_slice(&self.between);
        out.extend_from_slice(&self.value_raw);
    }
}

/// What an entity holds, in file order (MAP-2.2).
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A key and value.
    Pair(Pair),
    /// A brush.
    Brush(Brush),
}

/// An entity: its pairs and brushes (MAP-2).
#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    leading: Vec<u8>,
    /// The pairs and brushes, in file order.
    pub items: Vec<Item>,
    closing: Vec<u8>,
    line: u32,
}

impl Default for Entity {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity {
    /// An empty entity.
    #[must_use]
    pub fn new() -> Self {
        Self {
            leading: b"\n{".to_vec(),
            items: Vec::new(),
            closing: b"\n}".to_vec(),
            line: 0,
        }
    }

    /// The 1-based line of its `{`, or 0 for an entity not read from a file.
    #[must_use]
    pub const fn line(&self) -> u32 {
        self.line
    }

    /// The pairs, in file order, duplicates included (MAP-2.5).
    pub fn pairs(&self) -> impl Iterator<Item = &Pair> {
        self.items.iter().filter_map(|item| match item {
            Item::Pair(pair) => Some(pair),
            Item::Brush(_) => None,
        })
    }

    /// The brushes, in file order.
    pub fn brushes(&self) -> impl Iterator<Item = &Brush> {
        self.items.iter().filter_map(|item| match item {
            Item::Brush(brush) => Some(brush),
            Item::Pair(_) => None,
        })
    }

    /// The value of `key`: its last occurrence, the one both compilers look up (MAP-2.5).
    #[must_use]
    pub fn value(&self, key: &[u8]) -> Option<&[u8]> {
        self.pairs()
            .filter(|pair| pair.key == key)
            .last()
            .map(Pair::value)
    }

    /// The `classname`.
    #[must_use]
    pub fn classname(&self) -> Option<&[u8]> {
        self.value(b"classname")
    }

    /// The `origin`, read as three `%lf` numbers that default to 0 (MAP-2.7).
    #[must_use]
    pub fn origin(&self) -> [f64; 3] {
        scan_vector(self.value(b"origin").unwrap_or_default())
    }

    /// Whether the entity has a brush (MAP-6.2).
    #[must_use]
    pub fn has_brushes(&self) -> bool {
        self.brushes().next().is_some()
    }

    /// Sets `key`: its last occurrence takes the new value with its formatting kept, or a
    /// new pair goes after the last pair (first when there is none).
    ///
    /// # Errors
    ///
    /// [`MapError::Unwritable`] when either holds `"` or NUL.
    pub fn set(&mut self, key: &[u8], value: &[u8]) -> Result<(), MapError> {
        let mut last_pair = None;
        let mut found = None;
        for (index, item) in self.items.iter().enumerate() {
            if let Item::Pair(pair) = item {
                last_pair = Some(index);
                if pair.key == key {
                    found = Some(index);
                }
            }
        }
        if let Some(Item::Pair(pair)) = found.and_then(|index| self.items.get_mut(index)) {
            return pair.set_value(value);
        }
        let pair = Pair::new(key, value)?;
        let at = last_pair.map_or(0, |index| index.saturating_add(1));
        self.items.insert(at, Item::Pair(pair));
        Ok(())
    }

    /// Removes every pair with `key`, returning how many there were.
    pub fn remove(&mut self, key: &[u8]) -> usize {
        let before = self.items.len();
        self.items
            .retain(|item| !matches!(item, Item::Pair(pair) if pair.key == key));
        before.saturating_sub(self.items.len())
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.leading);
        for item in &self.items {
            match item {
                Item::Pair(pair) => pair.write(out),
                Item::Brush(brush) => brush.write(out),
            }
        }
        out.extend_from_slice(&self.closing);
    }
}

/// Which compiler's reading of the world's WAD keys to follow (MAP-6.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compiler {
    /// id's qbsp: one path.
    Qbsp,
    /// ericw-tools: paths separated by `;`.
    EricwTools,
}

/// A `.map` file, every byte kept.
#[derive(Clone, Debug, PartialEq)]
pub struct MapFile {
    /// The entities, in file order; the first is the world (MAP-6.1).
    pub entities: Vec<Entity>,
    trailing: Vec<u8>,
}

impl Default for MapFile {
    fn default() -> Self {
        Self::new()
    }
}

/// The parser: a lexer and the offset up to which the text has been taken into nodes.
struct Parser<'a> {
    lexer: Lexer<'a>,
    text: &'a [u8],
    cursor: usize,
}

impl<'a> Parser<'a> {
    /// The next token, or `None` at the end of the text.
    fn next(&mut self) -> Result<Option<Token<'a>>, MapError> {
        match self.lexer.skip() {
            Gap::End(_) => Ok(None),
            Gap::Token(gap_break) => Ok(Some(self.lexer.token(gap_break)?)),
        }
    }

    /// The next token, which must be on the current line (MAP-1.7).
    fn same_line(&mut self) -> Result<Token<'a>, MapError> {
        match self.lexer.skip() {
            Gap::End(line_break) | Gap::Token(Some(line_break)) => Err(MapError::LineIncomplete {
                line: line_break.line,
            }),
            Gap::Token(None) => Ok(self.lexer.token(None)?),
        }
    }

    /// The next token without consuming it, if it is on the current line.
    fn peek_same_line(&self) -> Option<Token<'a>> {
        let mut lexer = self.lexer.clone();
        match lexer.skip() {
            Gap::Token(None) => lexer.token(None).ok(),
            Gap::Token(Some(_)) | Gap::End(_) => None,
        }
    }

    /// The text from the cursor up to `offset`, moving the cursor there.
    fn take(&mut self, offset: usize) -> Vec<u8> {
        let piece = self
            .text
            .get(self.cursor..offset)
            .unwrap_or_default()
            .to_vec();
        self.cursor = offset.max(self.cursor);
        piece
    }

    fn number(&mut self, line: u32, numbers: &mut Vec<Vec<u8>>) -> Result<f64, MapError> {
        let token = self.same_line()?;
        let (value, used) = c_strtod(token.text);
        if used == 0 || used != token.text.len() {
            return Err(MapError::BadNumber {
                line,
                text: token.text.to_vec(),
            });
        }
        numbers.push(token.text.to_vec());
        Ok(value)
    }

    fn word(&mut self, word: &[u8], error: MapError) -> Result<(), MapError> {
        if self.same_line()?.is(word) {
            Ok(())
        } else {
            Err(error)
        }
    }

    fn entity(&mut self, open: Token<'a>) -> Result<Entity, MapError> {
        // The `{` and `}` are kept as written: qbsp takes `"{"` as `{` (MAP-2.1).
        let leading = self.take(open.end);
        let mut items = Vec::new();
        loop {
            let token = self
                .next()?
                .ok_or(MapError::EofWithoutBrace { line: open.line })?;
            if token.is(b"}") {
                let closing = self.take(token.end);
                return Ok(Entity {
                    leading,
                    items,
                    closing,
                    line: open.line,
                });
            }
            if token.is(b"{") {
                items.push(Item::Brush(self.brush(token)?));
                continue;
            }
            let leading = self.take(token.start);
            let value = self.same_line()?;
            items.push(Item::Pair(Pair {
                leading,
                key_raw: self
                    .text
                    .get(token.start..token.end)
                    .unwrap_or_default()
                    .to_vec(),
                between: self
                    .text
                    .get(token.end..value.start)
                    .unwrap_or_default()
                    .to_vec(),
                value_raw: self
                    .text
                    .get(value.start..value.end)
                    .unwrap_or_default()
                    .to_vec(),
                key: token.text.to_vec(),
                value: value.text.to_vec(),
                line: token.line,
                ericw_escape: escapes(self.text, token) || escapes(self.text, value),
            }));
            self.cursor = value.end;
        }
    }

    fn brush(&mut self, open: Token<'a>) -> Result<Brush, MapError> {
        let leading = self.take(open.end);
        let mut faces = Vec::new();
        loop {
            let Some(token) = self.next()? else {
                // MAP-3.1: the end of the file ends the brush, and then the entity fails.
                return Ok(Brush {
                    leading,
                    faces,
                    closing: Vec::new(),
                    line: open.line,
                });
            };
            if token.is(b"}") {
                let closing = self.take(token.end);
                return Ok(Brush {
                    leading,
                    faces,
                    closing,
                    line: open.line,
                });
            }
            faces.push(self.face(token)?);
        }
    }

    fn face(&mut self, first: Token<'a>) -> Result<Face, MapError> {
        let line = first.line;
        let leading = self.take(first.start);
        let parsing = MapError::ParsingBrush { line };
        let mut numbers = Vec::new();
        let mut points = [[0.0; 3]; 3];
        for (index, point) in points.iter_mut().enumerate() {
            let open = if index == 0 {
                Some(first)
            } else {
                self.next()?
            };
            if !open.is_some_and(|token| token.is(b"(")) {
                return Err(parsing);
            }
            for value in point.iter_mut() {
                *value = self.number(line, &mut numbers)?;
            }
            self.word(b")", parsing.clone())?;
        }
        let texture = self.same_line()?.text.to_vec();
        let mut qbsp_differs;
        let texdef = if self.peek_same_line().is_some_and(|token| token.is(b"[")) {
            qbsp_differs = true;
            let valve = MapError::Valve220 { line };
            let mut axes = [[0.0; 4]; 2];
            for axis in &mut axes {
                self.word(b"[", valve.clone())?;
                for value in axis.iter_mut() {
                    *value = self.number(line, &mut Vec::new())?;
                }
                self.word(b"]", valve.clone())?;
            }
            let rotate = self.number(line, &mut Vec::new())?;
            let scale = [
                self.number(line, &mut Vec::new())?,
                self.number(line, &mut Vec::new())?,
            ];
            let [s, t] = axes;
            TexDef::Valve {
                s,
                t,
                rotate,
                scale,
            }
        } else {
            let shift = [
                self.number(line, &mut numbers)?,
                self.number(line, &mut numbers)?,
            ];
            let rotate = self.number(line, &mut numbers)?;
            let mut scales = Vec::new();
            let scale = [
                self.number(line, &mut scales)?,
                self.number(line, &mut scales)?,
            ];
            qbsp_differs = scales
                .iter()
                .zip(scale)
                .any(|(text, value)| !same_bits(c_atof(text), value));
            TexDef::Standard {
                shift,
                rotate,
                scale,
            }
        };
        qbsp_differs |= numbers.iter().any(|text| !qbsp_integer_reads(text));
        let raw = self.take(self.lexer.offset());
        Ok(Face {
            leading,
            raw,
            line,
            points,
            texture,
            texdef,
            qbsp_differs,
        })
    }
}

/// Whether `token` is quoted and ends in a backslash with no CR or LF after its closing
/// quote (MAP-1.8).
fn escapes(text: &[u8], token: Token<'_>) -> bool {
    token.quoted
        && token.text.last() == Some(&b'\\')
        && !matches!(text.get(token.end), Some(b'\r' | b'\n'))
}

/// Whether qbsp's `atoi` reads `text` as the same number a double does (MAP-3.3).
fn qbsp_integer_reads(text: &[u8]) -> bool {
    same_bits(f64::from(c_atoi(text)), c_strtod(text).0)
}

#[expect(
    clippy::float_cmp,
    reason = "spec MAP-3.3: a number qbsp reads differently is one whose value differs at all"
)]
fn same_bits(a: f64, b: f64) -> bool {
    a == b
}

/// A number as MAP-7.3 writes it: an integer without a fraction, otherwise the shortest
/// decimal that reads back as the same double.
fn number(value: f64) -> Result<String, MapError> {
    if !value.is_finite() {
        return Err(MapError::NotFinite);
    }
    let value = value + 0.0;
    if value.fract() == 0.0 && value.abs() < 1e15 {
        Ok(format!("{value:.0}"))
    } else {
        Ok(format!("{value}"))
    }
}

impl MapFile {
    /// An empty map.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
            trailing: b"\n".to_vec(),
        }
    }

    /// Reads a map, keeping every byte.
    ///
    /// Implements MAP-1 to MAP-5 as both compilers read them: the text ends at its first
    /// NUL (MAP-1.1), whose bytes and what follows are kept; tokens have no length limit;
    /// numbers are doubles; each face is standard or Valve 220 by its own tokens.
    ///
    /// # Errors
    ///
    /// The first [`MapError`] in the file.
    pub fn parse(bytes: &[u8]) -> Result<Self, MapError> {
        let lexer = Lexer::new(bytes, false);
        let text = lexer.text();
        let mut parser = Parser {
            lexer,
            text,
            cursor: 0,
        };
        let mut entities = Vec::new();
        while let Some(open) = parser.next()? {
            if !open.is(b"{") {
                return Err(MapError::NoOpenBrace { line: open.line });
            }
            entities.push(parser.entity(open)?);
        }
        let mut trailing = parser.take(text.len());
        trailing.extend_from_slice(bytes.get(text.len()..).unwrap_or_default());
        Ok(Self { entities, trailing })
    }

    /// The file's bytes: exactly those read, with only the edits made since.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for entity in &self.entities {
            entity.write(&mut out);
        }
        out.extend_from_slice(&self.trailing);
        out
    }

    /// Adds an entity at the end, on a line of its own.
    pub fn push_entity(&mut self, mut entity: Entity) {
        if self.entities.is_empty() && self.trailing.iter().all(u8::is_ascii_whitespace) {
            let start = entity
                .leading
                .iter()
                .position(|byte| !byte.is_ascii_whitespace())
                .unwrap_or(entity.leading.len());
            entity.leading.drain(..start);
        }
        self.entities.push(entity);
    }

    /// The world: the first entity (MAP-6.1).
    #[must_use]
    pub fn world(&self) -> Option<&Entity> {
        self.entities.first()
    }

    /// Each brush entity's index and the model number qbsp gives it: the entities after the
    /// world with a brush, numbered from 1 in file order (MAP-6.2).
    #[must_use]
    pub fn brush_models(&self) -> Vec<(usize, usize)> {
        let mut models = Vec::new();
        for (index, entity) in self.entities.iter().enumerate().skip(1) {
            if entity.has_brushes() {
                models.push((index, models.len().saturating_add(1)));
            }
        }
        models
    }

    /// The WAD paths the world names (MAP-6.3): `_wad`, or `wad` when that is missing or
    /// empty; one path for qbsp, a list split at `;` for ericw-tools.
    #[must_use]
    pub fn wad_paths(&self, compiler: Compiler) -> Vec<Vec<u8>> {
        let Some(world) = self.world() else {
            return Vec::new();
        };
        let Some(value) = [b"_wad".as_slice(), b"wad"]
            .into_iter()
            .filter_map(|key| world.value(key))
            .find(|value| !value.is_empty())
        else {
            return Vec::new();
        };
        match compiler {
            Compiler::Qbsp => vec![value.to_vec()],
            Compiler::EricwTools => value
                .split(|&byte| byte == b';')
                .filter(|path| !path.is_empty())
                .map(<[u8]>::to_vec)
                .collect(),
        }
    }
}

/// Whether `key` is a tool key, which the engine ignores (MAP-6.7).
#[must_use]
pub fn is_tool_key(key: &[u8]) -> bool {
    key.first() == Some(&b'_')
}
