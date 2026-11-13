// SPDX-License-Identifier: GPL-3.0-or-later

//! The entity lump as light reads and rewrites it (LIGHT-2, LIGHT-1.6): each entity's keys, its
//! light value, style, cone and colour, switchable styles for named lights and the targets
//! spotlights aim at.

use qs_map::cnum::{c_atof, c_strtod};
use qs_map_bsp::Dialect;

use crate::{AXES, LightError, LightWarning, Vec3};

/// `DEFAULTLIGHTLEVEL`: the light of a `light*` entity with none (LIGHT-2.2).
pub const LIGHT_DEFAULT_LEVEL: i32 = 300;
/// The first style a named light is given (LIGHT-2.4).
pub const LIGHT_FIRST_SWITCHED_STYLE: i32 = 32;
/// `lighttargets[32]`: how many names may be given styles (LIGHT-2.4).
pub const LIGHT_MAX_SWITCHED: usize = 32;
/// `MAX_MAP_ENTITIES` (LIGHT-1.6).
pub const LIGHT_MAX_MAP_ENTITIES: usize = 1024;
/// `MAX_MAP_ENTSTRING` (LIGHT-1.6).
pub const LIGHT_MAX_MAP_ENTSTRING: usize = 65_536;
/// The highest style a light may have (LIGHT-2.3).
const MAX_STYLE: u32 = 254;
/// The bytes of `light` a key must start with to set the light value (LIGHT-2.1).
const LIGHT_PREFIX: &[u8] = b"light";
/// A colour component at most this is a fraction of full (LIGHT-6.1).
const FRACTION_MAX: f32 = 1.0;
/// `char line[128]`: the longest `"key" "value"` line and its LF and NUL `light` writes
/// (LIGHT-2.8).
const LINE_BUFFER: usize = 128;
/// The bytes of a line besides the key and value: four quotes, a space, the LF and the NUL.
const LINE_EXTRA: usize = 7;
/// Full colour on a channel (LIGHT-6.1).
pub(crate) const FULL: f32 = 255.0;

/// One entity: its keys in the order read, and what light makes of them.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entity {
    /// The keys and values, in the order read.
    pairs: Vec<(Vec<u8>, Vec<u8>)>,
    classname: Vec<u8>,
    target: Vec<u8>,
    targetname: Vec<u8>,
    pub(crate) origin: Vec3,
    pub(crate) angle: f32,
    pub(crate) light: i32,
    pub(crate) style: i32,
    /// `_color`, scaled to 0 to 255 (LIGHT-6.1); none for white.
    pub(crate) colour: Option<Vec3>,
    /// The entity a spotlight aims at (LIGHT-2.6).
    pub(crate) target_entity: Option<usize>,
}

/// `COM_Parse`: the next token of `text` from `*at`, a quoted string's bytes without its quotes,
/// a brace, parenthesis, quote mark or colon alone, or a word; none at the end. A `//` comment
/// runs to the end of its line (LIGHT-2.7).
fn token(text: &[u8], at: &mut usize) -> Option<Vec<u8>> {
    const SINGLE: &[u8] = b"{}()':";
    loop {
        let rest = text.get(*at..)?;
        let skip = rest.iter().take_while(|&&c| blank(c)).count();
        *at = at.saturating_add(skip);
        let rest = text.get(*at..)?;
        match rest {
            [] => return None,
            [b'/', b'/', ..] => {
                let line = rest.iter().take_while(|&&c| c != b'\n').count();
                *at = at.saturating_add(line);
            }
            [b'"', quoted @ ..] => {
                let len = quoted.iter().take_while(|&&c| c != b'"').count();
                let word = quoted.get(..len).unwrap_or_default().to_vec();
                // Past the closing quote; an unclosed one runs to the end (LIGHT-2.7).
                *at = at.saturating_add(len).saturating_add(2).min(text.len());
                return Some(word);
            }
            [first, ..] if SINGLE.contains(first) => {
                *at = at.saturating_add(1);
                return Some(vec![*first]);
            }
            _ => {
                let len = rest
                    .iter()
                    .take_while(|&&c| !blank(c))
                    .enumerate()
                    .take_while(|&(i, c)| i == 0 || !SINGLE.contains(c))
                    .count();
                *at = at.saturating_add(len);
                return Some(rest.get(..len).unwrap_or_default().to_vec());
            }
        }
    }
}

/// Whether `COM_Parse` takes `byte` as white space: at most a space, or a byte past 127, which a
/// signed `char` holds as negative (LIGHT-2.7).
const fn blank(byte: u8) -> bool {
    byte <= b' ' || byte > SIGNED_MAX
}

/// The largest byte a signed `char` holds as itself.
const SIGNED_MAX: u8 = 0x7f;

/// `atof` into an `int`, as C converts: truncated, a value past `int` held at its end
/// (LIGHT-2.1, LIGHT-2.3).
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec LIGHT-2.1: light stores atof's value in an int, truncating"
)]
fn atoi_of_atof(value: &[u8]) -> i32 {
    c_atof(value) as i32
}

/// `atof` into a `float`.
fn float_of(value: &[u8]) -> f32 {
    crate::to_f32(c_atof(value))
}

/// `sscanf(value, "%lf %lf %lf")`: the three numbers, or none when fewer convert.
fn three(value: &[u8]) -> Option<Vec3> {
    let mut out = [0.0; AXES];
    let mut rest = value;
    for slot in &mut out {
        let (number, used) = c_strtod(rest);
        if used == 0 {
            return None;
        }
        *slot = crate::to_f32(number);
        rest = rest.get(used..).unwrap_or_default();
    }
    Some(out)
}

impl Entity {
    fn new() -> Self {
        Self {
            pairs: Vec::new(),
            classname: Vec::new(),
            target: Vec::new(),
            targetname: Vec::new(),
            origin: [0.0; AXES],
            angle: 0.0,
            light: 0,
            style: 0,
            colour: None,
            target_entity: None,
        }
    }

    /// One key as `LoadEntities` reads it (LIGHT-2.1, LIGHT-2.3, LIGHT-6.1).
    fn read(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<(), LightError> {
        match key.as_slice() {
            b"classname" => self.classname.clone_from(&value),
            b"target" => self.target.clone_from(&value),
            b"targetname" => self.targetname.clone_from(&value),
            b"origin" => self.origin = three(&value).ok_or(LightError::Origin)?,
            b"style" => {
                self.style = atoi_of_atof(&value);
                if self.style.cast_unsigned() > MAX_STYLE {
                    return Err(LightError::BadStyle { style: self.style });
                }
            }
            b"angle" => self.angle = float_of(&value),
            b"_color" => self.colour = three(&value).map(colour_of),
            _ if key.starts_with(LIGHT_PREFIX) || key == b"_light" => {
                self.light = atoi_of_atof(&value);
            }
            _ => {}
        }
        self.pairs.push((key, value));
        Ok(())
    }

    /// `SetKeyValue`: the value of the key last read replaced, or the key added (LIGHT-1.6).
    fn set(&mut self, key: &[u8], value: Vec<u8>) {
        if let Some(pair) = self.pairs.iter_mut().rev().find(|(k, _)| k == key) {
            pair.1 = value;
        } else {
            self.pairs.push((key.to_vec(), value));
        }
    }

    /// Whether the entity is a light (LIGHT-2.1).
    pub(crate) const fn is_light(&self) -> bool {
        self.light != 0
    }

    /// Whether its colour is other than white (LIGHT-6.5).
    #[expect(
        clippy::float_cmp_const,
        reason = "spec LIGHT-6.5: white is exactly 255 on every channel"
    )]
    pub(crate) fn is_coloured(&self) -> bool {
        self.colour.is_some_and(|c| c != [FULL; AXES])
    }
}

/// A `_color` as light uses it: scaled by 255 when every component is 0 to 1; white is none
/// (LIGHT-6.1).
fn colour_of(colour: Vec3) -> Vec3 {
    if colour.iter().all(|&c| (0.0..=FRACTION_MAX).contains(&c)) {
        colour.map(|c| c * FULL)
    } else {
        colour
    }
}

/// The entities of an entity lump, as light reads them (LIGHT-2).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entities {
    pub(crate) all: Vec<Entity>,
    /// How many names were given styles (LIGHT-2.4).
    switched: usize,
}

impl Entities {
    /// `LoadEntities` and `MatchTargets` (LIGHT-2), with the warnings they print.
    pub(crate) fn read(
        lump: &[u8],
        dialect: Dialect,
        warnings: &mut Vec<LightWarning>,
    ) -> Result<Self, LightError> {
        let text = lump.split(|&b| b == 0).next().unwrap_or_default();
        let mut at = 0;
        let mut all: Vec<Entity> = Vec::new();
        let mut names: Vec<Vec<u8>> = Vec::new();
        while let Some(open) = token(text, &mut at) {
            if open != b"{" {
                return Err(LightError::ExpectedBrace { found: open });
            }
            if dialect == Dialect::Id && all.len() == LIGHT_MAX_MAP_ENTITIES {
                return Err(LightError::TooManyEntities);
            }
            let mut entity = Entity::new();
            loop {
                let key = token(text, &mut at).ok_or(LightError::Unclosed)?;
                if key == b"}" {
                    break;
                }
                let value = token(text, &mut at).ok_or(LightError::Unclosed)?;
                if value.first() == Some(&b'}') {
                    return Err(LightError::NoValue);
                }
                entity.read(key, value)?;
            }
            if entity.classname.starts_with(LIGHT_PREFIX) && entity.light == 0 {
                entity.light = LIGHT_DEFAULT_LEVEL;
            }
            if entity.classname == b"light" && !entity.targetname.is_empty() && entity.style == 0 {
                entity.style = switched_style(&mut names, &entity.targetname)?;
                entity.set(b"style", entity.style.to_string().into_bytes());
            }
            all.push(entity);
        }
        let mut entities = Self {
            all,
            switched: names.len(),
        };
        entities.match_targets(warnings);
        Ok(entities)
    }

    /// `MatchTargets`: each entity's target is the first entity of that name; a target with a
    /// style gives its style to the entity (LIGHT-2.5).
    fn match_targets(&mut self, warnings: &mut Vec<LightWarning>) {
        for i in 0..self.all.len() {
            let Some(entity) = self.all.get(i) else {
                continue;
            };
            if entity.target.is_empty() {
                continue;
            }
            let found = self
                .all
                .iter()
                .position(|other| other.targetname == entity.target);
            let Some(j) = found else {
                warnings.push(LightWarning::UnmatchedTarget {
                    origin: entity.origin.map(truncated),
                    classname: String::from_utf8_lossy(&entity.classname).into_owned(),
                });
                continue;
            };
            let style = self.all.get(j).map_or(0, |target| target.style);
            if let Some(entity) = self.all.get_mut(i) {
                entity.target_entity = Some(j);
                if style != 0 {
                    entity.style = style;
                    entity.set(b"style", style.to_string().into_bytes());
                }
            }
        }
    }

    /// How many names were given switchable styles.
    pub(crate) const fn switched(&self) -> usize {
        self.switched
    }

    /// `WriteEntitiesToString`: each entity with keys, its keys last read first, and the NUL
    /// that ends the text (LIGHT-1.6); only the id dialect has its length limit (LIGHT-1.7).
    pub(crate) fn written(&self, dialect: Dialect) -> Result<Vec<u8>, LightError> {
        let mut out = Vec::new();
        for entity in &self.all {
            if entity.pairs.is_empty() {
                continue;
            }
            out.extend_from_slice(b"{\n");
            for (key, value) in entity.pairs.iter().rev() {
                if dialect == Dialect::Id
                    && key
                        .len()
                        .saturating_add(value.len())
                        .saturating_add(LINE_EXTRA)
                        > LINE_BUFFER
                {
                    return Err(LightError::PairTooLong {
                        key: String::from_utf8_lossy(key).into_owned(),
                    });
                }
                out.push(b'"');
                out.extend_from_slice(key);
                out.extend_from_slice(b"\" \"");
                out.extend_from_slice(value);
                out.extend_from_slice(b"\"\n");
            }
            out.extend_from_slice(b"}\n");
            if dialect == Dialect::Id && out.len() > LIGHT_MAX_MAP_ENTSTRING {
                return Err(LightError::EntityTextTooLong);
            }
        }
        out.push(0);
        Ok(out)
    }
}

/// `LightStyleForTargetname`: the style of a name, 32 on for each new one; the 33rd is refused
/// (LIGHT-2.4).
fn switched_style(names: &mut Vec<Vec<u8>>, name: &[u8]) -> Result<i32, LightError> {
    let index = if let Some(index) = names.iter().position(|n| n == name) {
        index
    } else {
        if names.len() == LIGHT_MAX_SWITCHED {
            return Err(LightError::TooManySwitched);
        }
        names.push(name.to_vec());
        names.len().saturating_sub(1)
    };
    let index = i32::try_from(index).map_err(|_| LightError::TooManySwitched)?;
    Ok(LIGHT_FIRST_SWITCHED_STYLE.saturating_add(index))
}

/// A coordinate as `(int)` prints it in a warning.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec LIGHT-2.5: the warning prints each coordinate cast to int"
)]
const fn truncated(value: f32) -> i32 {
    value as i32
}
