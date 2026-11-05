// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/host_cmd.c, Host_SavegameComment, Host_Savegame_f, Host_Loadgame_f
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, PR_UglyValueString, ED_Write, ED_WriteGlobals, ED_ParseGlobals

//! Saved games, as `Host_Savegame_f` writes them and `Host_Loadgame_f` reads them back
//! (PHYS-10.5): the header, the light styles, the globals the progs marks to save and every
//! edict's fields that are not zero.

use qc_edict::EdictNumber;
use qc_edict::system::global_by_name;
use qc_progs::{PROGS_DEF_SAVEGLOBAL, Progs};
use qc_types::{DefType, EntityRef, Etype, Slot};

use qc_vm::ServerState;

use super::{CLIENT, Spawner, World, field_slot};
use crate::builtins::SimCall;
use crate::spawn::{FieldDef, Parser, SpawnError, Token, c_atof, parse_saved_edict, parse_value};

/// `SAVEGAME_VERSION` (`host_cmd.c`).
pub const SAVEGAME_VERSION: i32 = 5;
/// `SAVEGAME_COMMENT_LENGTH` (`quakedef.h`): the comment's characters.
const SAVEGAME_COMMENT_LENGTH: usize = 39;
/// Where the comment's `kills:` count starts, `text+22`.
const SAVEGAME_KILLS_AT: usize = 22;
/// `NUM_SPAWN_PARMS` (`server.h`).
const NUM_SPAWN_PARMS: usize = 16;
/// `MAX_LIGHTSTYLES` (`quakedef.h`).
const MAX_LIGHTSTYLES: usize = 64;
/// `char str[32768]`: the most bytes `Host_Loadgame_f` reads for one `{ ... }` block, its
/// terminator included.
const SAVEGAME_BLOCK: usize = 32768;

/// A saved game's text as `Host_Loadgame_f` reads its header (PHYS-10.5).
#[derive(Clone, Debug, PartialEq)]
pub struct SaveGame {
    /// The comment, the level's name and kills, spaces written as `_`.
    pub comment: Vec<u8>,
    /// The client's spawn parms.
    pub parms: [f32; NUM_SPAWN_PARMS],
    /// `current_skill`, `(int)(tfloat + 0.1)` of the stored float.
    pub skill: i32,
    /// The map to spawn before the edicts are read back.
    pub map: String,
    /// `sv.time`.
    pub time: f32,
    lightstyles: Vec<Vec<u8>>,
    /// The globals' and the edicts' blocks.
    blocks: Vec<u8>,
}

/// Why a saved game could not be written or read (PHYS-10.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveError {
    /// "Not playing a local game.": no player has entered the game.
    NotPlaying,
    /// "Can't savegame with a dead player".
    DeadPlayer,
    /// "Savegame is version <n>, not 5".
    Version(i32),
    /// The file ends before its header does.
    Truncated,
    /// "Loadgame buffer overflow": a block of 32767 bytes or more before its `}`.
    BlockOverflow,
    /// "First token isn't a brace".
    NoBrace,
    /// More edicts than the world holds (`EDICT_NUM: bad number`).
    TooManyEdicts,
    /// A block's pairs do not parse (`ED_ParseEntity`, `ED_ParseEdict: parse error`).
    Parse(SpawnError),
    /// A value that names nothing, which id's `PR_UglyValueString` would read past an array
    /// for: an entity, a string, a function or a field.
    BadValue {
        /// The field or global.
        name: String,
    },
    /// Linking a read edict into the world failed (`SV_LinkEdict`).
    Link(String),
}

impl core::fmt::Display for SaveError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotPlaying => f.write_str("Not playing a local game."),
            Self::DeadPlayer => f.write_str("Can't savegame with a dead player"),
            Self::Version(version) => {
                write!(f, "Savegame is version {version}, not {SAVEGAME_VERSION}")
            }
            Self::Truncated => f.write_str("the saved game ends in its header"),
            Self::BlockOverflow => f.write_str("Loadgame buffer overflow"),
            Self::NoBrace => f.write_str("First token isn't a brace"),
            Self::TooManyEdicts => f.write_str("the saved game has more edicts than the world"),
            Self::Parse(error) => write!(f, "{error}"),
            Self::BadValue { name } => write!(f, "{name} names nothing"),
            Self::Link(error) => write!(f, "linking an edict: {error}"),
        }
    }
}

impl core::error::Error for SaveError {}

impl SaveGame {
    /// `Host_Loadgame_f`'s `fscanf`s: the version, the comment, the 16 spawn parms, the skill,
    /// the map, the time and the 64 light styles, each a word after any white space; the rest
    /// is the blocks (PHYS-10.5).
    ///
    /// # Errors
    ///
    /// [`SaveError::Version`] for a version other than 5, [`SaveError::Truncated`] for a file
    /// that ends first.
    pub fn read(text: &[u8]) -> Result<Self, SaveError> {
        let mut words = Words { text, pos: 0 };
        let version = words.next().map(c_scan_int).ok_or(SaveError::Truncated)?;
        if version != SAVEGAME_VERSION {
            return Err(SaveError::Version(version));
        }
        let comment = words.next().ok_or(SaveError::Truncated)?.to_vec();
        let mut parms = [0.0; NUM_SPAWN_PARMS];
        for parm in &mut parms {
            *parm = words.next().map(c_atof).ok_or(SaveError::Truncated)?;
        }
        // "this silliness is so we can load 1.06 save files, which have float skill values"
        let skill = words.next().map(c_atof).ok_or(SaveError::Truncated)?;
        let skill = truncate(skill + 0.1);
        let map = words.next().ok_or(SaveError::Truncated)?;
        let map = String::from_utf8_lossy(map).into_owned();
        let time = words.next().map(c_atof).ok_or(SaveError::Truncated)?;
        let lightstyles = (0..MAX_LIGHTSTYLES)
            .map(|_| words.next().map(<[u8]>::to_vec).ok_or(SaveError::Truncated))
            .collect::<Result<_, _>>()?;
        // The `%s\n` before the blocks skips the white space after the last style.
        words.skip_space();
        let blocks = text.get(words.pos..).unwrap_or_default().to_vec();
        Ok(Self {
            comment,
            parms,
            skill,
            map,
            time,
            lightstyles,
            blocks,
        })
    }
}

impl World {
    /// `Host_Savegame_f`'s file for this world (PHYS-10.5): version 5, the comment, the
    /// client's spawn parms, skill 1, the map, the time, the light styles (`m` for one not
    /// set), the globals `ED_WriteGlobals` writes, then `ED_Write` of every edict below
    /// `num_edicts`.
    ///
    /// # Errors
    ///
    /// [`SaveError::DeadPlayer`] while the player's health is 0 or less, and
    /// [`SaveError::BadValue`] for a value that names nothing.
    pub fn save_game(&self, progs: &Progs<'_>) -> Result<Vec<u8>, SaveError> {
        if self.player.is_none() {
            return Err(SaveError::NotPlaying);
        }
        if self.float_field(CLIENT, "health") <= 0.0 {
            return Err(SaveError::DeadPlayer);
        }
        let mut bytes = Vec::new();
        let line = |out: &mut Vec<u8>, text: &[u8]| {
            out.extend_from_slice(text);
            out.push(b'\n');
        };
        line(&mut bytes, SAVEGAME_VERSION.to_string().as_bytes());
        line(&mut bytes, &self.savegame_comment(progs));
        for parm in self.spawn_parms {
            line(&mut bytes, c_float(parm.as_f32()).as_bytes());
        }
        // `current_skill`: the world plays at skill 1.
        line(&mut bytes, b"1");
        line(&mut bytes, self.map.as_bytes());
        line(&mut bytes, c_double(self.time()).as_bytes());
        for style in 0..MAX_LIGHTSTYLES {
            line(
                &mut bytes,
                self.light_style(style).unwrap_or("m").as_bytes(),
            );
        }
        self.write_globals(progs, &mut bytes)?;
        for n in 0..self.machine.edicts().num_edicts() {
            self.write_edict(progs, n, &mut bytes)?;
        }
        Ok(bytes)
    }

    /// `Host_Loadgame_f` after `SV_SpawnServer` has spawned `save.map` into this world: the
    /// light styles, the globals, each edict cleared and read back, the free ones left free and
    /// the others linked, `num_edicts`, the time and the client's spawn parms; the client then
    /// connects with no `SetNewParms`, `ClientConnect` or `PutClientInServer` (PHYS-10.5).
    ///
    /// # Errors
    ///
    /// [`SaveError`] for a block that does not read or an edict that does not link.
    pub fn restore(&mut self, progs: &Progs<'_>, save: &SaveGame) -> Result<(), SaveError> {
        let engine = self.builtins.mock.engine_mut();
        for (slot, style) in engine.lightstyles.iter_mut().zip(&save.lightstyles) {
            *slot = Some(String::from_utf8_lossy(style).into_owned());
        }
        let mut entnum: Option<usize> = None;
        let mut rest = save.blocks.as_slice();
        let mut linked = Vec::new();
        loop {
            let (block, after) = next_block(rest)?;
            rest = after;
            let mut parser = Parser::new(block);
            match parser.next_token() {
                Token::End => break,
                Token::Word(word) if word == b"{" => {}
                Token::Word(_) => return Err(SaveError::NoBrace),
            }
            let Some(n) = entnum else {
                self.parse_globals(progs, &mut parser)?;
                entnum = Some(0);
                continue;
            };
            if self.machine.edicts().is_free(EdictNumber::new(n)).is_none() {
                return Err(SaveError::TooManyEdicts);
            }
            let number = EdictNumber::new(n);
            self.machine.edicts_mut().clear_edict(number);
            let mut spawner = Spawner {
                world: self,
                progs,
                state: ServerState::Loading,
            };
            let init = parse_saved_edict(&mut spawner, &mut parser, n).map_err(SaveError::Parse)?;
            if init {
                linked.push(n);
            } else {
                // `ED_ParseEdict`: `if (!init) ent->free = true;`
                self.machine.edicts_mut().free(number, 0.0);
            }
            entnum = Some(n.saturating_add(1));
        }
        self.machine
            .edicts_mut()
            .set_num_edicts(entnum.unwrap_or(0));
        self.builtins.mock.engine_mut().clock = f64::from(save.time);
        let mut call = SimCall::frame(&mut self.machine, progs, &mut self.builtins);
        for n in linked {
            call.link(n, false)
                .map_err(|e| SaveError::Link(e.to_string()))?;
        }
        self.spawn_parms = save.parms.map(Slot::from_f32);
        self.loaded = true;
        Ok(())
    }

    /// `ED_ParseGlobals`: each key a global by name, `ED_FindGlobal`, its value through
    /// `ED_ParseEpair` into the globals; a key naming no global is passed over.
    fn parse_globals(
        &mut self,
        progs: &Progs<'_>,
        parser: &mut Parser<'_>,
    ) -> Result<(), SaveError> {
        loop {
            let key = match parser.next_token() {
                Token::Word(word) if word == b"}" => break,
                Token::Word(word) => word,
                Token::End => return Err(SaveError::Parse(SpawnError::EofInEntity)),
            };
            let value = match parser.next_token() {
                Token::Word(word) if word == b"}" => {
                    return Err(SaveError::Parse(SpawnError::BraceWithoutData));
                }
                Token::Word(word) => word,
                Token::End => return Err(SaveError::Parse(SpawnError::EofInEntity)),
            };
            let Some(def) = progs.ed_find_global(&key) else {
                continue;
            };
            let field = FieldDef {
                slot: usize::try_from(def.ofs).unwrap_or(usize::MAX),
                etype: known(def.def_type()),
            };
            let mut spawner = Spawner {
                world: self,
                progs,
                state: ServerState::Loading,
            };
            parse_value(&mut spawner, field, &value, &mut |host, slot, value| {
                if let Ok(slot) = i32::try_from(slot) {
                    // A slot past the globals is not written.
                    let _written = host.world.machine.set_global(slot, value);
                }
            })
            .map_err(SaveError::Parse)?;
        }
        Ok(())
    }

    /// `Host_SavegameComment`: the level's name, the world's `message`, from the start and
    /// `kills:%3i/%3i` from 22, in 39 characters, every space an `_`.
    fn savegame_comment(&self, progs: &Progs<'_>) -> Vec<u8> {
        let mut text = vec![b' '; SAVEGAME_COMMENT_LENGTH];
        let message = self
            .string_value(progs, self.field(0, "message"))
            .unwrap_or_default();
        for (at, byte) in text.iter_mut().zip(message) {
            *at = byte;
        }
        let kills = format!(
            "kills:{:3}/{:3}",
            truncate(self.global_f32("killed_monsters")),
            truncate(self.global_f32("total_monsters"))
        );
        for (at, byte) in text.iter_mut().skip(SAVEGAME_KILLS_AT).zip(kills.bytes()) {
            *at = byte;
        }
        text.into_iter()
            .map(|b| if b == b' ' { b'_' } else { b })
            .collect()
    }

    /// `ED_WriteGlobals`: `{`, each global marked `DEF_SAVEGLOBAL` of type string, float or
    /// entity as `"name" "value"`, `}`.
    fn write_globals(&self, progs: &Progs<'_>, out: &mut Vec<u8>) -> Result<(), SaveError> {
        out.extend_from_slice(b"{\n");
        for def in progs.global_defs().iter() {
            if !def.is_save_global() {
                continue;
            }
            // Only a type that is ev_string, ev_float or ev_entity is written.
            let etype = match def.def_type() {
                DefType::Known(etype @ (Etype::String | Etype::Float | Etype::Entity)) => etype,
                DefType::Known(_) | DefType::Unknown(_) => continue,
            };
            let name = progs.string(def.s_name).unwrap_or_default();
            let slot = i32::try_from(def.ofs).unwrap_or(i32::MAX);
            let value = self.machine.global(slot).unwrap_or(Slot::ZERO);
            let text = self.ugly_value(progs, etype, &[value], name)?;
            write_pair(out, name, &text);
        }
        out.extend_from_slice(b"}\n");
        Ok(())
    }

    /// `ED_Write`: `{`, then for a used edict each field but the first, but one whose name's
    /// last but one character is `_` (`_x`, `_y`, `_z`) and but one whose slots are all 0, as
    /// `"name" "value"`; `}`.
    fn write_edict(&self, progs: &Progs<'_>, n: usize, out: &mut Vec<u8>) -> Result<(), SaveError> {
        out.extend_from_slice(b"{\n");
        let number = EdictNumber::new(n);
        if self.machine.edicts().is_free(number) != Some(true) {
            for def in progs.field_defs().iter().skip(1) {
                let name = progs.string(def.s_name).unwrap_or_default();
                if name.len() >= 2 && name.get(name.len().saturating_sub(2)) == Some(&b'_') {
                    continue;
                }
                let first = usize::try_from(def.ofs).unwrap_or(usize::MAX);
                // A type id's tables do not have: one slot, written `bad type <n>`.
                let (etype, slots) = match def.def_type() {
                    DefType::Known(etype) => (Some(etype), etype.slot_count()),
                    DefType::Unknown(_) => (None, 1),
                };
                let values: Vec<Slot> = (0..slots)
                    .map(|i| self.slot_of(n, first.saturating_add(i)))
                    .collect();
                if values.iter().all(|v| v.bits() == 0) {
                    continue;
                }
                let text = match etype {
                    Some(etype) => self.ugly_value(progs, etype, &values, name)?,
                    None => {
                        format!("bad type {}", def.type_bits & !PROGS_DEF_SAVEGLOBAL).into_bytes()
                    }
                };
                write_pair(out, name, &text);
            }
        }
        out.extend_from_slice(b"}\n");
        Ok(())
    }
}

impl World {
    /// Field slot `slot` of edict `n`, 0 past the fields.
    pub(super) fn slot_of(&self, n: usize, slot: usize) -> Slot {
        self.machine
            .edicts()
            .field(EdictNumber::new(n), slot)
            .unwrap_or(Slot::ZERO)
    }

    /// The system field `name` of edict `n`.
    pub(super) fn field(&self, n: usize, name: &str) -> Slot {
        self.slot_of(n, field_slot(name))
    }

    /// The system float field `name` of edict `n`.
    fn float_field(&self, n: usize, name: &str) -> f32 {
        self.field(n, name).as_f32()
    }

    /// The system global `name` as a float, 0 for one the progs does not have.
    fn global_f32(&self, name: &str) -> f32 {
        global_by_name(name)
            .and_then(|var| i32::try_from(var.slot).ok())
            .and_then(|slot| self.machine.global(slot).ok())
            .map_or(0.0, Slot::as_f32)
    }

    /// The text of the string value `value`: a string-table offset, or an engine string.
    pub(super) fn string_value(&self, progs: &Progs<'_>, value: Slot) -> Option<Vec<u8>> {
        let value = value.as_i32();
        if value >= 0 {
            return progs.string(value).map(<[u8]>::to_vec);
        }
        self.builtins
            .mock
            .engine()
            .strings
            .text(value)
            .map(<[u8]>::to_vec)
    }

    /// `PR_UglyValueString`: a string's text, an entity's edict number, a function's or a
    /// field's name, `void`, a float `%f`, a vector three `%f`s, `bad type <n>` otherwise.
    fn ugly_value(
        &self,
        progs: &Progs<'_>,
        etype: Etype,
        values: &[Slot],
        name: &[u8],
    ) -> Result<Vec<u8>, SaveError> {
        let first = values.first().copied().unwrap_or(Slot::ZERO);
        let bad = || SaveError::BadValue {
            name: String::from_utf8_lossy(name).into_owned(),
        };
        Ok(match etype {
            Etype::String => self.string_value(progs, first).ok_or_else(bad)?,
            Etype::Entity => {
                let number = self
                    .machine
                    .edicts()
                    .num_for_edict(EntityRef::from(first))
                    .map_err(|_| bad())?;
                number.index().to_string().into_bytes()
            }
            Etype::Function => {
                let index = usize::try_from(first.as_i32()).map_err(|_| bad())?;
                let function = progs.functions().get(index).ok_or_else(bad)?;
                progs.string(function.s_name).ok_or_else(bad)?.to_vec()
            }
            Etype::Field => {
                let ofs = u32::try_from(first.as_i32()).map_err(|_| bad())?;
                let def = progs.ed_field_at_ofs(ofs).ok_or_else(bad)?;
                progs.string(def.s_name).ok_or_else(bad)?.to_vec()
            }
            Etype::Void => b"void".to_vec(),
            Etype::Float => c_float(first.as_f32()).into_bytes(),
            Etype::Vector => {
                let mut text = String::new();
                for (i, value) in values.iter().enumerate() {
                    if i > 0 {
                        text.push(' ');
                    }
                    text.push_str(&c_float(value.as_f32()));
                }
                text.into_bytes()
            }
            Etype::Pointer => b"bad type 7".to_vec(),
        })
    }
}

/// A definition's type, `ev_float` for an unknown one as the spawner reads it.
pub(super) fn known(def: DefType) -> Etype {
    match def {
        DefType::Known(etype) => etype,
        DefType::Unknown(_) => Etype::Float,
    }
}

/// `fprintf (f,"\"%s\" ",name); fprintf (f,"\"%s\"\n", value);`
fn write_pair(out: &mut Vec<u8>, name: &[u8], value: &[u8]) {
    out.push(b'"');
    out.extend_from_slice(name);
    out.extend_from_slice(b"\" \"");
    out.extend_from_slice(value);
    out.extend_from_slice(b"\"\n");
}

/// C's `%f` of a float, promoted to double: six decimals, `inf` and `nan` as glibc and the
/// Microsoft C runtime of id's build spell them.
fn c_float(value: f32) -> String {
    c_double(f64::from(value))
}

/// C's `%f` of a double.
fn c_double(value: f64) -> String {
    if value.is_nan() {
        return if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        }
        .to_owned();
    }
    format!("{value:.6}")
}

/// C's `(int)` of a float, toward zero; 0 for NaN, saturated past the range.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec PHYS-10.5: C's (int) cast toward zero of a saved count or skill"
)]
fn truncate(value: f32) -> i32 {
    value as i32
}

/// `fscanf`'s `%i` of a word: an optional sign and decimal digits; 0 for none.
fn c_scan_int(word: &[u8]) -> i32 {
    let text = String::from_utf8_lossy(word);
    let end = text
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+'))))
        .map_or(text.len(), |(i, _)| i);
    text.get(..end).and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// The words of a saved game's header, as `fscanf`'s `%s` and `%f` read them: each run of
/// bytes that are not white space.
struct Words<'a> {
    text: &'a [u8],
    pos: usize,
}

impl<'a> Words<'a> {
    fn skip_space(&mut self) {
        while self.text.get(self.pos).is_some_and(u8::is_ascii_whitespace) {
            self.pos = self.pos.saturating_add(1);
        }
    }

    fn next(&mut self) -> Option<&'a [u8]> {
        self.skip_space();
        let start = self.pos;
        while self
            .text
            .get(self.pos)
            .is_some_and(|b| !b.is_ascii_whitespace())
        {
            self.pos = self.pos.saturating_add(1);
        }
        (self.pos > start)
            .then(|| self.text.get(start..self.pos))
            .flatten()
    }
}

/// `Host_Loadgame_f`'s read of one block: bytes up to and including the first `}`, or to a
/// zero byte, which ends the block and is passed over, or to the end; the block, and what
/// follows it.
///
/// # Errors
///
/// [`SaveError::BlockOverflow`] for a block of 32767 bytes or more.
fn next_block(rest: &[u8]) -> Result<(&[u8], &[u8]), SaveError> {
    let end = rest
        .iter()
        .position(|&b| b == b'}' || b == 0)
        .map_or(rest.len(), |i| {
            if rest.get(i) == Some(&b'}') {
                i.saturating_add(1)
            } else {
                i
            }
        });
    if end >= SAVEGAME_BLOCK.saturating_sub(1) {
        return Err(SaveError::BlockOverflow);
    }
    let (block, after) = rest.split_at(end);
    // `if (r == EOF || !r) break;` ends the block; `while (!feof(f))` reads on after it.
    let after = after.strip_prefix(&[0]).unwrap_or(after);
    Ok((block, after))
}

#[cfg(test)]
mod tests;
