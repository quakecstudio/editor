// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2016      Spike
// Copyright (C) 1996-2001 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2007-2008 Kristian Duske
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from Novum/vkQuake@27789d7333a46674474d871137bb71e056719628: Quake/pr_ext.c, PF_clientcommand
// Ported from Novum/vkQuake@27789d7333a46674474d871137bb71e056719628: Quake/cmd.c, Cmd_ExecuteString, Cmd_TokenizeString, Cmd_AddArg
// Ported from Novum/vkQuake@27789d7333a46674474d871137bb71e056719628: Quake/host_cmd.c, Host_Setinfo_f
// Ported from Novum/vkQuake@27789d7333a46674474d871137bb71e056719628: Quake/cl_main.c, SV_UpdateInfo, SV_DecodeUserInfo
// Ported from Novum/vkQuake@27789d7333a46674474d871137bb71e056719628: Quake/common.c, Info_RemoveKey, Info_SetKey, Info_GetKey

//! A client's userinfo under vkQuake's profile (BUILTIN-18.27 to BUILTIN-18.29): the info string
//! `setinfo` and `forceinfokey` change, the name and colours vkQuake decodes from it, and the
//! client commands `clientcommand` runs.

use std::collections::BTreeMap;

use crate::tokenize::{SPACE, com_parse, signed};

/// `SERVER_INFO_STRING_SIZE`, the bytes of a client's `userinfo`, its zero among them.
pub const SERVER_INFO_STRING_SIZE: usize = 8192;
/// The bytes of `client_t`'s `name`, its zero among them.
const CLIENT_NAME_SIZE: usize = 32;
/// The bytes of `SV_DecodeUserInfo`'s `tmp`, its zero among them.
const DECODE_SIZE: usize = 64;
/// `SV_UpdateInfo`'s `oldvalue`.
const OLD_VALUE_SIZE: usize = 1024;
/// `MAX_ARGS`, the most words `Cmd_TokenizeString` keeps.
const MAX_ARGS: usize = 80;
/// The bytes of one of `cmd_argv`'s words, its zero among them.
const ARG_SIZE: usize = 1024;
/// A colour's bits, `& 15`, the highest colour, 13, and the top colour's shift (BUILTIN-18.27).
const COLOR_MASK: i32 = 15;
const MAX_COLOR: i32 = 13;
const TOP_SHIFT: u32 = 4;
/// `setinfo <key> <value>`'s words (BUILTIN-18.28).
const SETINFO_WORDS: usize = 3;
/// `atoi`'s base.
const DECIMAL: i64 = 10;

/// One client's info string and what vkQuake decodes from it (BUILTIN-18.27).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientInfo {
    userinfo: Vec<u8>,
    name: Vec<u8>,
    colors: u8,
}

impl Default for ClientInfo {
    /// The mock's client as it enters: named `player` with colours 0 (DIFF-4.1), its userinfo
    /// the three keys a client sends.
    fn default() -> Self {
        Self {
            userinfo: b"\\name\\player\\topcolor\\0\\bottomcolor\\0".to_vec(),
            name: b"player".to_vec(),
            colors: 0,
        }
    }
}

/// What decoding a client's userinfo writes to its edict (BUILTIN-18.27).
#[derive(Clone, Debug, PartialEq)]
pub struct Decoded {
    /// `team`: the bottom colour plus 1.
    pub team: f32,
    /// The new name for `netname`, when the name changed.
    pub netname: Option<Vec<u8>>,
}

impl ClientInfo {
    /// The client's name, `client->name`.
    #[must_use]
    pub fn name(&self) -> &[u8] {
        &self.name
    }

    /// The client's colours, `client->colors`: the top colour times 16 plus the bottom.
    #[must_use]
    pub const fn colors(&self) -> u8 {
        self.colors
    }

    /// `SV_DropClient`'s "host_client->name[0] = 0;": the name emptied, the info kept.
    pub fn clear_name(&mut self) {
        self.name.clear();
    }

    /// The info string.
    #[must_use]
    pub fn userinfo(&self) -> &[u8] {
        &self.userinfo
    }

    /// `SV_UpdateInfo` on a player, without the network: when `value` differs from the key's
    /// value, the key is set and the info decoded; `None` when nothing changed.
    pub fn update(&mut self, key: &[u8], value: &[u8]) -> Option<Decoded> {
        let old = info_get_key(&self.userinfo, key, OLD_VALUE_SIZE);
        if value == old.as_slice() {
            return None;
        }
        info_set_key(&mut self.userinfo, SERVER_INFO_STRING_SIZE, key, value);
        Some(self.decode())
    }

    /// `SV_DecodeUserInfo`: the colours from `topcolor` and `bottomcolor`, each `atoi` of its
    /// text masked to 15 and at most 13, and the name from `name`, `unnamed` when it is empty.
    fn decode(&mut self) -> Decoded {
        let colour = |info: &[u8], key: &[u8]| -> u8 {
            let low = atoi(&info_get_key(info, key, DECODE_SIZE)) & COLOR_MASK;
            u8::try_from(low.min(MAX_COLOR)).unwrap_or_default()
        };
        let top = colour(&self.userinfo, b"topcolor");
        let bottom = colour(&self.userinfo, b"bottomcolor");
        self.colors = top.wrapping_shl(TOP_SHIFT) | bottom;
        let mut name = info_get_key(&self.userinfo, b"name", DECODE_SIZE);
        if name.is_empty() {
            name = b"unnamed".to_vec();
        }
        let netname = (self.name != name).then(|| {
            // q_strlcpy into `name[32]`.
            name.truncate(CLIENT_NAME_SIZE.saturating_sub(1));
            self.name.clone_from(&name);
            name
        });
        Decoded {
            team: f32::from(bottom) + 1.0,
            netname,
        }
    }
}

/// A client's slot, from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientSlot(usize);

impl ClientSlot {
    /// Slot `slot`.
    #[must_use]
    pub const fn new(slot: usize) -> Self {
        Self(slot)
    }

    /// The slot.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// Every client's info, by slot from 0; a slot not here has [`ClientInfo::default`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Clients {
    slots: BTreeMap<usize, ClientInfo>,
}

impl Clients {
    /// The info of client slot `slot`, from 0.
    #[must_use]
    pub fn get(&self, slot: ClientSlot) -> ClientInfo {
        self.slots.get(&slot.get()).cloned().unwrap_or_default()
    }

    /// The info of client slot `slot`, for changing.
    pub fn get_mut(&mut self, slot: ClientSlot) -> &mut ClientInfo {
        self.slots.entry(slot.get()).or_default()
    }
}

/// A client command `Cmd_ExecuteString` runs from a client (`src_client`): only the commands
/// vkQuake registers with `Cmd_AddCommand_ClientCommand` run; any other, `color` and `say`
/// among them, is refused with a developer message (BUILTIN-18.28).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientCommand {
    /// `setinfo <key> <value>`: `SV_UpdateInfo` of the client's own info, unless the key
    /// starts with `*`.
    Setinfo {
        /// The key.
        key: Vec<u8>,
        /// The value.
        value: Vec<u8>,
    },
    /// Nothing QuakeC sees: no words, `serverinfo`, `setinfo` without two arguments or with a
    /// `*` key, which only print, and every command a client may not run.
    Nothing,
}

impl ClientCommand {
    /// `Cmd_ExecuteString (text, src_client)`: the text's words by `Cmd_TokenizeString`, the
    /// first matched without case.
    #[must_use]
    pub fn parse(text: &[u8]) -> Self {
        let words = tokenize_command(text);
        let Some(name) = words.first() else {
            return Self::Nothing;
        };
        if !name.eq_ignore_ascii_case(b"setinfo") || words.len() != SETINFO_WORDS {
            return Self::Nothing;
        }
        let (Some(key), Some(value)) = (words.get(1), words.get(2)) else {
            return Self::Nothing;
        };
        if key.first() == Some(&b'*') {
            return Self::Nothing;
        }
        Self::Setinfo {
            key: key.clone(),
            value: value.clone(),
        }
    }
}

/// `Cmd_TokenizeString`: the words `COM_Parse` reads up to the text's first line feed, each
/// cut to 1023 bytes, at most [`MAX_ARGS`].
fn tokenize_command(text: &[u8]) -> Vec<Vec<u8>> {
    let byte = |at: usize| text.get(at).copied().unwrap_or(0);
    let mut words = Vec::new();
    let mut at = 0;
    loop {
        // `while (*text && *text <= ' ' && *text != '\n')`, `char` signed.
        while byte(at) != 0 && signed(byte(at)) <= SPACE && byte(at) != b'\n' {
            at = at.saturating_add(1);
        }
        if byte(at) == b'\n' || byte(at) == 0 {
            return words;
        }
        let Some((mut word, next)) = com_parse(text, at) else {
            return words;
        };
        if words.len() < MAX_ARGS {
            word.truncate(ARG_SIZE.saturating_sub(1));
            words.push(word);
        }
        at = next;
    }
}

/// The byte at `at` of a C string held without its zero: 0 past its end.
fn at(text: &[u8], at: usize) -> u8 {
    text.get(at).copied().unwrap_or(0)
}

/// Whether the pair at `i` (just after its `\`) has the key `key`: `!strncmp (info, key,
/// keylen) && info[keylen] == '\\'`.
fn key_at(info: &[u8], i: usize, key: &[u8]) -> bool {
    let end = i.saturating_add(key.len());
    info.get(i..end) == Some(key) && at(info, end) == b'\\'
}

/// The index past the bytes from `i` up to the next `\` or the end.
fn skip_part(info: &[u8], mut i: usize) -> usize {
    while at(info, i) != 0 && at(info, i) != b'\\' {
        i = i.saturating_add(1);
    }
    i
}

/// `Info_RemoveKey`: the first pair with the key `key` taken out, its leading `\` with it.
pub fn info_remove_key(info: &mut Vec<u8>, key: &[u8]) {
    let mut i = 0;
    while at(info, i) != 0 {
        let start = i;
        if at(info, i) != b'\\' {
            return;
        }
        i = i.saturating_add(1);
        if key_at(info, i, key) {
            let end = skip_part(info, i.saturating_add(key.len()).saturating_add(1));
            info.drain(start..end.min(info.len()));
            return;
        }
        i = skip_part(info, i);
        if at(info, i) != b'\\' {
            return;
        }
        i = skip_part(info, i.saturating_add(1));
    }
}

/// `Info_SetKey`: the key removed, then, for a value that is not empty, `\key\value` added at
/// the end unless the key is empty or either holds a `\`, or the string would no longer fit in
/// `size` bytes with one to spare.
pub fn info_set_key(info: &mut Vec<u8>, size: usize, key: &[u8], value: &[u8]) {
    info_remove_key(info, key);
    if value.is_empty() || key.is_empty() || key.contains(&b'\\') || value.contains(&b'\\') {
        return;
    }
    // `o + 2 + keylen + vallen >= e`, `e` being `info + infosize - 1`.
    let needed = info
        .len()
        .saturating_add(2)
        .saturating_add(key.len())
        .saturating_add(value.len());
    if needed >= size.saturating_sub(1) {
        return;
    }
    info.push(b'\\');
    info.extend_from_slice(key);
    info.push(b'\\');
    info.extend_from_slice(value);
}

/// `Info_GetKey` into a buffer of `size` bytes: the value of the first pair with the key `key`,
/// at most `size - 1` bytes of it, or empty.
#[must_use]
pub fn info_get_key(info: &[u8], key: &[u8], size: usize) -> Vec<u8> {
    let room = size.saturating_sub(1);
    let mut i = 0;
    while at(info, i) != 0 {
        if at(info, i) != b'\\' {
            break;
        }
        i = i.saturating_add(1);
        if key_at(info, i, key) {
            let start = i.saturating_add(key.len()).saturating_add(1);
            let end = skip_part(info, start).min(start.saturating_add(room));
            return info.get(start..end).unwrap_or_default().to_vec();
        }
        i = skip_part(info, i);
        if at(info, i) != b'\\' {
            break;
        }
        i = skip_part(info, i.saturating_add(1));
    }
    Vec::new()
}

/// C's `atoi` as glibc's `strtol` to an `int`: white space, an optional sign, the digits; a
/// value past a `long` is `LONG_MAX` or `LONG_MIN`, then cut to `int`'s low 32 bits.
fn atoi(text: &[u8]) -> i32 {
    let mut i = 0;
    while matches!(
        at(text, i),
        b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'
    ) {
        i = i.saturating_add(1);
    }
    let negative = at(text, i) == b'-';
    if matches!(at(text, i), b'-' | b'+') {
        i = i.saturating_add(1);
    }
    let mut value: i64 = 0;
    while at(text, i).is_ascii_digit() {
        let digit = i64::from(at(text, i).wrapping_sub(b'0'));
        value = if negative {
            value.saturating_mul(DECIMAL).saturating_sub(digit)
        } else {
            value.saturating_mul(DECIMAL).saturating_add(digit)
        };
        i = i.saturating_add(1);
    }
    // `(int)` of a `long`: the low 32 bits, as x86-64 truncates.
    let [a, b, c, d, ..] = value.to_le_bytes();
    i32::from_le_bytes([a, b, c, d])
}

#[cfg(test)]
mod tests;
