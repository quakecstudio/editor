// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, ED_LoadFromFile, ED_ParseEdict, ED_ParseEpair
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/common.c, COM_Parse

//! Spawning a map's entities, id's `ED_LoadFromFile` (EDICT-8).
//!
//! `ED_LoadFromFile` reads the entity lump a map carries, one `{ "key" "value" ... }` entity
//! at a time, applies each pair to a field (`ED_ParseEpair`), inhibits the entities a skill
//! or deathmatch setting excludes (EDICT-8.4), and calls each surviving entity's spawn
//! function (EDICT-8.6). The tokenizer is id's `COM_Parse` (EDICT-8.8). The caller supplies
//! the engine operations through a [`SpawnHost`]: allocating edicts, finding fields and
//! functions, reading and writing fields, making engine strings, reading a setting, and
//! running or freeing an entity.
//!
//! The port keeps id's behaviour, not its C: the globals are the explicit [`SpawnHost`], the
//! errors are [`SpawnError`] values rather than `Sys_Error`/`Host_Error`, and every
//! conversion is written out. Where id's C is undefined, a vector value with fewer than three
//! components, the spec fixes the answer and this follows it (EDICT-8.3).
//!
//! Implements `docs/spec/edicts.md` EDICT-8. The edicts are [`qc_edict`]; the field type
//! conversions are PROGS-17; the string escapes are `qs_porting_example::ed_new_string`
//! (EXAMPLE-1), as EDICT-8.2 requires.

use qc_types::{Etype, Slot};

/// `SPAWNFLAG_NOT_EASY` (EDICT-8.4), id's `WinQuake/server.h`.
pub const EDICT_SPAWNFLAG_NOT_EASY: i32 = 256;
/// `SPAWNFLAG_NOT_MEDIUM` (EDICT-8.4), id's `WinQuake/server.h`.
pub const EDICT_SPAWNFLAG_NOT_MEDIUM: i32 = 512;
/// `SPAWNFLAG_NOT_HARD` (EDICT-8.4), id's `WinQuake/server.h`.
pub const EDICT_SPAWNFLAG_NOT_HARD: i32 = 1024;
/// `SPAWNFLAG_NOT_DEATHMATCH` (EDICT-8.4), id's `WinQuake/server.h`.
pub const EDICT_SPAWNFLAG_NOT_DEATHMATCH: i32 = 2048;

/// The `spawnflags` field slot (EDICT-3.1), read by the inhibition test (EDICT-8.4).
pub const SPAWNFLAGS_SLOT: usize = 89;

/// One token id's `COM_Parse` yields (EDICT-8.8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    /// A token: the bytes of `com_token`, a word or a quoted string's content.
    Word(Vec<u8>),
    /// End of the text (`COM_Parse` returned `NULL`).
    End,
}

/// A field a progs declares, found by name (id's `ED_FindField`, PROGS-9.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldDef {
    /// The field slot the key names (its `ofs`, the first slot of the value).
    pub slot: usize,
    /// The field's declared type.
    pub etype: Etype,
}

/// Why spawning a map's entities failed (EDICT-8.8, EDICT-8.2), id's `Sys_Error` or
/// `Host_Error` messages.
///
/// The three tokenizer errors are EDICT-8.8; `ParseError` is id's `ED_ParseEdict: parse error`
/// for an unknown function or field name in `ED_ParseEpair` (EDICT-8.2); `NoFreeEdicts` is
/// `ED_Alloc` with the array full (EDICT-6.2); `BadEdictNumber` is `EDICT_NUM` given an entity
/// key's number out of range (EDICT-1.7, EDICT-8.2). `ParseError` is a host error and the rest
/// are fatal (`Sys_Error`), but all of them happen while the map loads, when no shutdown runs
/// (DIFF-8.3), so the host treats them alike: one error record with function 0 and statement
/// -1 (DIFF-8.1), then the scenario ends. The classname errors of EDICT-8.5 are not errors in
/// id (the entity is freed and a message printed), so the host frees the edict and goes on; an
/// error the spawn function raises is a [`qc_vm::RunError`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpawnError {
    /// id's `ED_LoadFromFile: found <token> when expecting {` (EDICT-8.8).
    ExpectedBrace {
        /// The token found where `{` was expected.
        token: Vec<u8>,
    },
    /// id's `ED_ParseEntity: EOF without closing brace` (EDICT-8.8).
    EofInEntity,
    /// id's `ED_ParseEntity: closing brace without data` (EDICT-8.8).
    BraceWithoutData,
    /// id's `ED_ParseEdict: parse error` (EDICT-8.2): an unknown function or field name.
    ParseError,
    /// id's `ED_Alloc: no free edicts` (EDICT-6.2): the map has more entities than the array
    /// holds.
    NoFreeEdicts,
    /// id's `EDICT_NUM: bad number <n>` (EDICT-1.7): an entity key's `atoi(value)` is negative
    /// or not less than `LIMIT_MAX_EDICTS` (EDICT-8.2).
    BadEdictNumber {
        /// The number `atoi` gave.
        number: i32,
    },
}

impl core::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ExpectedBrace { token } => write!(
                f,
                "ED_LoadFromFile: found {} when expecting {{",
                String::from_utf8_lossy(token)
            ),
            Self::EofInEntity => f.write_str("ED_ParseEntity: EOF without closing brace"),
            Self::BraceWithoutData => f.write_str("ED_ParseEntity: closing brace without data"),
            Self::ParseError => f.write_str("ED_ParseEdict: parse error"),
            Self::NoFreeEdicts => f.write_str("ED_Alloc: no free edicts"),
            Self::BadEdictNumber { number } => write!(f, "EDICT_NUM: bad number {number}"),
        }
    }
}

impl core::error::Error for SpawnError {}

/// A reader over the entity-lump bytes that yields id's `COM_Parse` tokens (EDICT-8.8).
///
/// Ported from `WinQuake/common.c`, `COM_Parse`: skip whitespace (every byte whose value as a
/// signed `char` is at most a space, so a byte 0x80 or above is whitespace too) and `//`
/// comments, read a quoted string up to the next `"` or the end, read a single-character
/// token for one of `{ } ) ( ' :`, or read a word up to the next byte at most a space or a
/// single-character delimiter.
#[derive(Clone, Debug)]
pub struct Parser<'a> {
    data: &'a [u8],
    pos: usize,
}

/// The single-character tokens of id's `COM_Parse`: `{ } ) ( ' :`.
fn is_single(byte: u8) -> bool {
    matches!(byte, b'{' | b'}' | b')' | b'(' | b'\'' | b':')
}

/// C's `*data <= ' '` for a signed `char data`: a byte is whitespace when, read as a signed
/// `char`, it is at most a space. On the build id ships `char` is signed, so 0x80 to 0xFF are
/// negative and count as whitespace.
fn is_space(byte: u8) -> bool {
    byte.cast_signed() <= b' '.cast_signed()
}

/// C's `c > 32` for a signed `char c`: a byte continues a word when, read signed, it is above
/// 32; a byte 0x80 or above is negative, so it ends the word.
fn above_space(byte: u8) -> bool {
    byte.cast_signed() > 32
}

impl<'a> Parser<'a> {
    /// A parser over `data`, the entity lump as `ED_LoadFromFile` receives it (EDICT-8.1).
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// The byte at the cursor, or `None` at the end.
    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    /// Advances the cursor by one byte.
    fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }

    /// The next token, id's `COM_Parse` (EDICT-8.8). `Token::End` is id's `NULL` return.
    ///
    /// id's `*data == 0` ends the text: the entity lump has a NUL after its last byte
    /// (DIFF-2.5), which `COM_Parse` sees as end of file. This parser treats both the real end
    /// of the slice and a NUL byte as the end, since the lump's one trailing NUL is the only
    /// NUL a map's entity text holds.
    pub fn next_token(&mut self) -> Token {
        // COM_Parse: skip whitespace; a 0 byte is end of file.
        loop {
            match self.peek() {
                None | Some(0) => return Token::End,
                // skip // comments
                Some(b'/') if self.data.get(self.pos.saturating_add(1)) == Some(&b'/') => {
                    while let Some(c) = self.peek() {
                        if c == b'\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                Some(c) if is_space(c) => self.bump(),
                Some(_) => break,
            }
        }

        let Some(c) = self.peek() else {
            return Token::End;
        };

        // handle quoted strings specially
        if c == b'"' {
            self.bump();
            let mut token = Vec::new();
            loop {
                match self.peek() {
                    // c == '"' || !c: the closing quote or a 0 byte ends the string; the quote
                    // is consumed, the 0 is not (the lump's only 0 is the trailing one).
                    None | Some(0) => return Token::Word(token),
                    Some(b'"') => {
                        self.bump();
                        return Token::Word(token);
                    }
                    Some(byte) => {
                        token.push(byte);
                        self.bump();
                    }
                }
            }
        }

        // parse single characters
        if is_single(c) {
            self.bump();
            return Token::Word(vec![c]);
        }

        // parse a regular word: read until a byte at most a space or a single-char delimiter.
        let mut token = Vec::new();
        while let Some(byte) = self.peek() {
            token.push(byte);
            self.bump();
            match self.peek() {
                Some(next) if is_single(next) => break,
                Some(next) if above_space(next) => {}
                _ => break,
            }
        }
        Token::Word(token)
    }
}

/// The engine operations `ED_LoadFromFile`, `ED_ParseEdict` and `ED_ParseEpair` need
/// (EDICT-8), supplied by the host so the port stays pure.
///
/// Each method is one piece of id's code: allocating an edict (`ED_Alloc`, EDICT-6, and the
/// world for the first entity, EDICT-4.2); finding a field or function by name
/// (`ED_FindField`, `ED_FindFunction`); reading a global (for the `field` key type, which
/// reads `G_INT(def->ofs)`); writing an edict field; making an engine string; reading the
/// `spawnflags` field for the inhibition test; and running the spawn function or freeing the
/// entity. The spawn function runs through `self`, which the host sets to the entity
/// (EDICT-8.6).
pub trait SpawnHost {
    /// Allocates the edict for the next entity (EDICT-8.2). `first` is true for the map's first
    /// entity, which goes into edict 0 without clearing it (EDICT-4.2); a later one comes from
    /// `ED_Alloc` (EDICT-6), which clears it. Returns the edict number.
    ///
    /// # Errors
    ///
    /// [`SpawnError::NoFreeEdicts`], id's fatal `ED_Alloc: no free edicts` (EDICT-6.2), when
    /// the array is full.
    fn allocate(&mut self, first: bool) -> Result<usize, SpawnError>;

    /// The entity value of edict `number`, id's `EDICT_TO_PROG(EDICT_NUM(number))`, for an
    /// entity key (EDICT-8.2, EDICT-1.5).
    ///
    /// # Errors
    ///
    /// [`SpawnError::BadEdictNumber`], id's fatal `EDICT_NUM: bad number <n>` (EDICT-1.7), when
    /// `number` is negative or not less than `LIMIT_MAX_EDICTS`.
    fn edict_value(&self, number: i32) -> Result<i32, SpawnError>;

    /// The field named `name`, id's `ED_FindField`. `None` when the progs does not declare it
    /// (EDICT-8.2: the key is then ignored).
    fn find_field(&self, name: &[u8]) -> Option<FieldDef>;

    /// The function index named `name`, id's `ED_FindFunction`. `None` when no function has the
    /// name (EDICT-8.2: `ED_ParseEdict: parse error`; EDICT-8.5: no spawn function).
    fn find_function(&self, name: &[u8]) -> Option<i32>;

    /// The global at slot `slot`, for the `field` key type (EDICT-8.2). `None` out of range.
    fn global(&self, slot: usize) -> Option<Slot>;

    /// Writes field slot `field` of edict `edict` (EDICT-8.2).
    fn set_field(&mut self, edict: usize, field: usize, value: Slot);

    /// Makes an engine string holding `text` and returns its value (EDICT-13.1).
    fn make_string(&mut self, text: Vec<u8>) -> i32;

    /// The `spawnflags` field of edict `edict`, read for the inhibition test (EDICT-8.4).
    fn spawnflags(&self, edict: usize) -> f32;

    /// Spawns edict `edict` by calling function `function` with `self` set to it (EDICT-8.6).
    ///
    /// # Errors
    ///
    /// Whatever the spawn function raises (DIFF-8.1).
    fn spawn(&mut self, edict: usize, function: i32) -> Result<(), qc_vm::RunError>;

    /// Frees edict `edict`, id's `ED_Free` (EDICT-7), for an inhibited or classless entity
    /// (EDICT-8.4, EDICT-8.5).
    fn free(&mut self, edict: usize);
}

/// Converts one `"key" "value"` pair's value by the field's type and writes it, id's
/// `ED_ParseEpair` (EDICT-8.2).
///
/// # Errors
///
/// [`SpawnError::ParseError`] for an unknown function or field name;
/// [`SpawnError::BadEdictNumber`] for an entity number out of range.
fn parse_epair<H: SpawnHost>(
    host: &mut H,
    edict: usize,
    field: FieldDef,
    value: &[u8],
) -> Result<(), SpawnError> {
    match field.etype {
        // ev_string: ED_NewString's escapes, a new engine string (EDICT-8.2, EXAMPLE-1).
        Etype::String => {
            let decoded = qs_porting_example::ed_new_string(value);
            let string = host.make_string(decoded);
            host.set_field(edict, field.slot, Slot::from_i32(string));
        }
        // ev_float: C atof (EDICT-8.2).
        Etype::Float => host.set_field(edict, field.slot, Slot::from_f32(c_atof(value))),
        // ev_vector: three atof values, a missing component 0 (EDICT-8.3).
        Etype::Vector => {
            let [x, y, z] = parse_vector(value);
            host.set_field(edict, field.slot, Slot::from_f32(x));
            host.set_field(edict, field.slot.saturating_add(1), Slot::from_f32(y));
            host.set_field(edict, field.slot.saturating_add(2), Slot::from_f32(z));
        }
        // ev_entity: id stores EDICT_TO_PROG(EDICT_NUM(atoi(s))), the entity value of edict
        // atoi(value) (EDICT-8.2, EDICT-1.5); EDICT_NUM fails fatally on a number out of range
        // (EDICT-1.7).
        Etype::Entity => {
            let entity = host.edict_value(c_atoi(value))?;
            host.set_field(edict, field.slot, Slot::from_i32(entity));
        }
        // ev_function: the function of that name (EDICT-8.2); unknown is a parse error.
        Etype::Function => {
            let name = c_string(value);
            let function = host.find_function(name).ok_or(SpawnError::ParseError)?;
            host.set_field(edict, field.slot, Slot::from_i32(function));
        }
        // ev_field: the value of the global whose slot is the named field's offset: id reads
        // G_INT(ED_FindField(s)->ofs) (EDICT-8.2); an unknown field name is a parse error.
        Etype::Field => {
            let name = c_string(value);
            let named = host.find_field(name).ok_or(SpawnError::ParseError)?;
            let global = host.global(named.slot).unwrap_or(Slot(0));
            host.set_field(edict, field.slot, global);
        }
        // ev_void and ev_pointer are never a field's declared type a map key names.
        Etype::Void | Etype::Pointer => {}
    }
    Ok(())
}

/// Applies one `"key" "value"` pair, with the key fix-ups of EDICT-8.2 before the type
/// conversion: the `angle`/`light` renames, the trailing-space trim, the `_` skip and the
/// unknown-field skip.
fn apply_key<H: SpawnHost>(
    host: &mut H,
    edict: usize,
    key: &[u8],
    value: &[u8],
) -> Result<(), SpawnError> {
    // ED_ParseEdict: remove trailing spaces from the key.
    let key = trim_trailing_spaces(key);

    // "angle" -> "angles", value v -> "0 v 0"; "light" -> "light_lev".
    let rewritten: Option<Vec<u8>>;
    let (key, value): (&[u8], &[u8]) = if key == b"angle" {
        let mut v = Vec::with_capacity(value.len().saturating_add(4));
        v.extend_from_slice(b"0 ");
        v.extend_from_slice(value);
        v.extend_from_slice(b" 0");
        rewritten = Some(v);
        (b"angles", rewritten.as_deref().unwrap_or(value))
    } else if key == b"light" {
        (b"light_lev", value)
    } else {
        (key, value)
    };

    // A key starting with `_` is ignored (EDICT-8.2).
    if key.first() == Some(&b'_') {
        return Ok(());
    }

    // A key that names no field is ignored, with the message `'<key>' is not a field`
    // (EDICT-8.2); the message is console output and not compared (DIFF-3.5).
    let Some(field) = host.find_field(key) else {
        return Ok(());
    };

    parse_epair(host, edict, field, value)
}

/// What [`parse_edict`] decided about one entity (EDICT-8.5, EDICT-8.6).
enum Parsed {
    /// Spawn the entity: call the function its `classname` names (EDICT-8.6).
    Spawn(i32),
    /// Free the entity instead (EDICT-8.5): no `classname`, or a `classname` naming no
    /// function.
    NoSpawn,
}

/// Reads one entity's `{ ... }` block, applies its keys and decides whether to spawn it, id's
/// `ED_ParseEdict` (EDICT-8.2, EDICT-8.5).
///
/// The opening `{` has been read by [`load_from_file`]. `classname` is read from the pairs and
/// its function looked up after the keys are applied, as id does.
fn parse_edict<H: SpawnHost>(
    host: &mut H,
    parser: &mut Parser<'_>,
    edict: usize,
) -> Result<Parsed, SpawnError> {
    let mut classname: Option<Vec<u8>> = None;
    loop {
        // parse the key
        let keyname = match parser.next_token() {
            Token::Word(word) if word == b"}" => break,
            Token::Word(word) => word,
            Token::End => return Err(SpawnError::EofInEntity),
        };

        // parse the value
        let value = match parser.next_token() {
            Token::Word(word) if word == b"}" => return Err(SpawnError::BraceWithoutData),
            Token::Word(word) => word,
            Token::End => return Err(SpawnError::EofInEntity),
        };

        // Remember the classname (after the trailing-space trim) for the spawn-function lookup.
        if trim_trailing_spaces(&keyname) == b"classname" {
            classname = Some(qs_porting_example::ed_new_string(&value));
        }

        apply_key(host, edict, &keyname, &value)?;
    }

    // EDICT-8.5: no classname, or a classname that names no function, frees the entity.
    let Some(classname) = classname else {
        return Ok(Parsed::NoSpawn);
    };
    match host.find_function(&classname) {
        Some(function) => Ok(Parsed::Spawn(function)),
        None => Ok(Parsed::NoSpawn),
    }
}

/// `key` with its trailing spaces removed (EDICT-8.2).
fn trim_trailing_spaces(key: &[u8]) -> &[u8] {
    let mut key = key;
    while let Some((&b' ', head)) = key.split_last() {
        key = head;
    }
    key
}

/// Whether an entity is inhibited by the skill or deathmatch settings (EDICT-8.4).
///
/// `deathmatch` is the value of the setting; `skill` is `(int)(skill + 0.5)` clamped to 0 to 3
/// (DIFF-3.2). `spawnflags` is truncated to an integer for the test.
#[must_use]
pub fn inhibited(spawnflags: f32, deathmatch: bool, skill: i32) -> bool {
    let flags = ftoi(spawnflags);
    if deathmatch {
        return flags & EDICT_SPAWNFLAG_NOT_DEATHMATCH != 0;
    }
    match skill {
        0 => flags & EDICT_SPAWNFLAG_NOT_EASY != 0,
        1 => flags & EDICT_SPAWNFLAG_NOT_MEDIUM != 0,
        // 2 or more: NOT_HARD. The clamp keeps skill at most 3 (DIFF-3.2).
        _ => flags & EDICT_SPAWNFLAG_NOT_HARD != 0,
    }
}

/// Reads the whole entity lump and spawns each entity, id's `ED_LoadFromFile` (EDICT-8.1,
/// EDICT-8.2, EDICT-8.4 to EDICT-8.6).
///
/// `deathmatch` and `skill` are the values the inhibition test uses (DIFF-3.5). The host
/// allocates each edict, applies the keys, and the entity is inhibited (EDICT-8.4) or its
/// spawn function is called (EDICT-8.6); a classless entity is freed (EDICT-8.5). The clock
/// stays at 1 throughout (EDICT-8.1), so an inhibited or classless entity is reused by the
/// next allocation (EDICT-8.7).
///
/// # Errors
///
/// [`LoadError::Spawn`] on a tokenizer or parse error (EDICT-8.8, EDICT-8.2), a full edict
/// array (EDICT-6.2) or an entity number out of range (EDICT-1.7);
/// [`LoadError::Run`] when a spawn function raises a runtime error (DIFF-8.1). Nothing after
/// the error runs.
pub fn load_from_file<H: SpawnHost>(
    host: &mut H,
    data: &[u8],
    deathmatch: bool,
    skill: i32,
) -> Result<(), LoadError> {
    let mut parser = Parser::new(data);
    let mut first = true;
    loop {
        // parse the opening brace
        match parser.next_token() {
            Token::End => break,
            Token::Word(word) if word == b"{" => {}
            Token::Word(word) => {
                return Err(LoadError::Spawn(SpawnError::ExpectedBrace { token: word }));
            }
        }

        // The first entity goes into edict 0 (EDICT-4.2); each later one from allocation.
        let edict = host.allocate(first).map_err(LoadError::Spawn)?;
        first = false;

        let parsed = parse_edict(host, &mut parser, edict).map_err(LoadError::Spawn)?;

        // EDICT-8.4: remove the things a setting excludes, after the keys are applied.
        match parsed {
            Parsed::Spawn(function) => {
                let flags = host.spawnflags(edict);
                if inhibited(flags, deathmatch, skill) {
                    host.free(edict);
                } else {
                    host.spawn(edict, function).map_err(LoadError::Run)?;
                }
            }
            Parsed::NoSpawn => host.free(edict),
        }
    }
    Ok(())
}

/// Either a spawn-time parse error (EDICT-8) or an error a spawn function raised (a
/// [`qc_vm::RunError`]); [`load_from_file`] threads both out of the loop.
#[derive(Debug)]
pub enum LoadError {
    /// A tokenizer or parse error (EDICT-8.8, EDICT-8.2), a full edict array (EDICT-6.2) or
    /// an entity number out of range (EDICT-1.7).
    Spawn(SpawnError),
    /// An error a spawn function raised (DIFF-8.1).
    Run(qc_vm::RunError),
}

/// C `atoi` of `text`: leading whitespace, an optional sign and a run of digits, stopping at
/// the first non-digit (EDICT-8.2). No digits is 0.
#[must_use]
pub fn c_atoi(text: &[u8]) -> i32 {
    let text = c_string(text);
    let mut rest = skip_c_whitespace(text);
    let negative = match rest.split_first() {
        Some((&b'-', tail)) => {
            rest = tail;
            true
        }
        Some((&b'+', tail)) => {
            rest = tail;
            false
        }
        _ => false,
    };
    let mut value: i64 = 0;
    let mut any = false;
    for &b in rest {
        if b.is_ascii_digit() {
            any = true;
            value = value
                .saturating_mul(10)
                .saturating_add(i64::from(b.wrapping_sub(b'0')));
        } else {
            break;
        }
    }
    if !any {
        return 0;
    }
    let value = if negative {
        0_i64.saturating_sub(value)
    } else {
        value
    };
    // atoi wraps to int; a map's entity numbers are small, so a saturating clamp keeps id's
    // answer for every value a map gives and never panics on a hand-built lump.
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

/// C `atof` of `text`, rounded to an `f32`, as id's `ED_ParseEpair` converts a float value
/// (EDICT-8.2): leading whitespace, an optional sign, digits with at most one `.`, and an
/// optional decimal exponent; conversion stops at anything else. No digits is 0.
#[must_use]
pub fn c_atof(text: &[u8]) -> f32 {
    let text = c_string(text);
    let rest = skip_c_whitespace(text);
    let mut decimal = String::new();
    let mut iter = rest.iter().copied().peekable();
    if let Some(&sign) = iter.peek()
        && (sign == b'+' || sign == b'-')
    {
        if sign == b'-' {
            decimal.push('-');
        }
        iter.next();
    }
    let mut digits = false;
    let mut seen_dot = false;
    while let Some(&b) = iter.peek() {
        if b.is_ascii_digit() {
            digits = true;
            decimal.push(char::from(b));
            iter.next();
        } else if b == b'.' && !seen_dot {
            seen_dot = true;
            decimal.push('.');
            iter.next();
        } else {
            break;
        }
    }
    if digits
        && let Some(&b) = iter.peek()
        && (b == b'e' || b == b'E')
    {
        let mut exponent = String::from("e");
        iter.next();
        if let Some(&sign) = iter.peek()
            && (sign == b'+' || sign == b'-')
        {
            exponent.push(char::from(sign));
            iter.next();
        }
        let mut exp_digits = false;
        while let Some(&b) = iter.peek() {
            if b.is_ascii_digit() {
                exp_digits = true;
                exponent.push(char::from(b));
                iter.next();
            } else {
                break;
            }
        }
        if exp_digits {
            decimal.push_str(&exponent);
        }
    }
    if !digits {
        return 0.0;
    }
    let value = decimal.parse::<f64>().unwrap_or(0.0);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec EDICT-8.2: C atof's double is rounded to the field's f32"
    )]
    let rounded = value as f32;
    rounded
}

/// Three `atof` values separated by single spaces; a missing component is 0 (EDICT-8.3).
fn parse_vector(value: &[u8]) -> [f32; 3] {
    let text = c_string(value);
    let mut out = [0.0_f32; 3];
    for (slot, part) in out.iter_mut().zip(text.split(|&b| b == b' ')) {
        *slot = c_atof(part);
    }
    out
}

/// `text` with leading C whitespace removed (the bytes `atoi`/`atof` skip).
fn skip_c_whitespace(text: &[u8]) -> &[u8] {
    let mut rest = text;
    while let Some((&b, tail)) = rest.split_first() {
        if matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
            rest = tail;
        } else {
            break;
        }
    }
    rest
}

/// `text` up to its first NUL, as a C string argument would be.
fn c_string(text: &[u8]) -> &[u8] {
    text.split(|&b| b == 0).next().unwrap_or_default()
}

/// C's float-to-integer conversion id's `(int)` gives on x86, as BUILTIN-2.2 fixes it:
/// truncation toward zero, and `i32::MIN` for a NaN or a value outside the `i32` range.
fn ftoi(value: f32) -> i32 {
    let d = f64::from(value);
    if d.is_nan() || d >= 2_147_483_648.0 || d <= -2_147_483_649.0 {
        return i32::MIN;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec BUILTIN-2.2: truncation toward zero, range checked above"
    )]
    let truncated = d as i32;
    truncated
}

#[cfg(test)]
mod tests;
