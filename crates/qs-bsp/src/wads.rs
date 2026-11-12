// SPDX-License-Identifier: GPL-3.0-or-later

//! The WADs a BSP's external textures come from, as QuakeSpasm finds them (WAD3-4,
//! `docs/spec/wad3.md`): `Mod_LoadWadFiles` reads the world's `wad` key, `W_LoadWadList` turns
//! each name in it into the files it opens. Described from QuakeSpasm, not ported.

/// `MAX_QPATH`, the size of `W_LoadWadList`'s file name buffer, its NUL included.
const MAX_QPATH: usize = 64;
/// QuakeSpasm's `com_token`, the longest token `COM_Parse` gives, its NUL included.
const TOKEN_SIZE: usize = 1024;
/// The size of `Mod_LoadWadFiles`' `key` buffer, its NUL included.
const KEY_SIZE: usize = 128;
/// The first byte a signed `char` holds as negative, which `COM_ParseEx` skips as whitespace.
const SIGNED_CHAR_NEGATIVE: u8 = 0x80;
/// What `COM_FileBase` gives for a name with too short a base.
const NO_BASE: &[u8] = b"?model?";

/// The `wad` key of the first entity of `entities`, as `Mod_LoadWadFiles` reads it (WAD3-4.1):
/// the first key that is `wad` once a leading `_` and any trailing spaces are removed; `None`
/// when the text does not parse that far or has no such key.
/// What `COM_ParseEx` does with a token longer than `com_token`, its `cpe_mode_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Overflow {
    /// `CPE_NOTRUNC`: `// return parse error in case of overflow`.
    NoTrunc,
    /// `CPE_ALLOWTRUNC`: `// truncate com_token in case of overflow`.
    AllowTrunc,
}

#[must_use]
pub fn wad_key(entities: &[u8]) -> Option<Vec<u8>> {
    let mut at = 0;
    let open = parse(entities, &mut at, Overflow::NoTrunc)?;
    if open.first() != Some(&b'{') {
        return None;
    }
    loop {
        let token = parse(entities, &mut at, Overflow::NoTrunc)?;
        if token.first() == Some(&b'}') {
            return None;
        }
        let key = token.strip_prefix(b"_").unwrap_or(&token);
        let mut key = key
            .get(..key.len().min(KEY_SIZE - 1))
            .unwrap_or(key)
            .to_vec();
        while key.last() == Some(&b' ') {
            key.pop();
        }
        let value = parse(entities, &mut at, Overflow::AllowTrunc)?;
        if key == b"wad" {
            return Some(value);
        }
    }
}

/// The files `W_LoadWadList` tries for the `wad` key's `value`, in the key's order (WAD3-4.2):
/// for each name between `;`s, its base with `.wad` added, then the same under `gfx/`. An
/// empty name ends the list.
#[must_use]
pub fn wad_files(value: &[u8]) -> Vec<[Vec<u8>; 2]> {
    let mut out = Vec::new();
    for name in value.split(|&byte| byte == b';') {
        if name.is_empty() {
            break;
        }
        let mut file = file_base(name);
        file.extend_from_slice(b".wad");
        file.truncate(MAX_QPATH - 1);
        let mut gfx = b"gfx/".to_vec();
        gfx.extend_from_slice(&file);
        gfx.truncate(MAX_QPATH - 1);
        out.push([file, gfx]);
    }
    out
}

/// QuakeSpasm's `COM_FileBase`: the bytes between the last `/` or `\` and the last `.`, or
/// `?model?` when that is shorter than 2 bytes; at most `MAX_QPATH - 1`.
fn file_base(name: &[u8]) -> Vec<u8> {
    let slash = name
        .iter()
        .rposition(|&byte| byte == b'/' || byte == b'\\')
        .map_or(0, |at| at.saturating_add(1));
    let dot = name
        .iter()
        .rposition(|&byte| byte == b'.')
        .unwrap_or(name.len());
    match dot.checked_sub(slash) {
        Some(len) if len >= 2 => name
            .get(slash..slash.saturating_add(len.min(MAX_QPATH - 1)))
            .unwrap_or_default()
            .to_vec(),
        _ => NO_BASE.to_vec(),
    }
}

/// One token as QuakeSpasm's `COM_ParseEx` reads it from `text` at `*at`, moving `*at` past
/// it; `None` at the end of the text, or with [`Overflow::NoTrunc`] for a token longer than
/// `com_token`.
fn parse(text: &[u8], at: &mut usize, overflow: Overflow) -> Option<Vec<u8>> {
    let byte = |i: usize| text.get(i).copied().unwrap_or(0);
    // `char` is signed: a byte of 0x80 or more is below ' ' and skipped as whitespace.
    let white = |c: u8| c <= b' ' || c >= SIGNED_CHAR_NEGATIVE;
    loop {
        while white(byte(*at)) {
            if byte(*at) == 0 {
                return None;
            }
            *at = at.saturating_add(1);
        }
        let (c, next) = (byte(*at), byte(at.saturating_add(1)));
        if c == b'/' && next == b'/' {
            while byte(*at) != 0 && byte(*at) != b'\n' {
                *at = at.saturating_add(1);
            }
            continue;
        }
        if c == b'/' && next == b'*' {
            *at = at.saturating_add(2);
            while byte(*at) != 0 && !(byte(*at) == b'*' && byte(at.saturating_add(1)) == b'/') {
                *at = at.saturating_add(1);
            }
            if byte(*at) != 0 {
                *at = at.saturating_add(2);
            }
            continue;
        }
        break;
    }
    let mut token = Vec::new();
    let push = |token: &mut Vec<u8>, c: u8| -> Option<()> {
        if token.len() < TOKEN_SIZE - 1 {
            token.push(c);
            return Some(());
        }
        match overflow {
            Overflow::NoTrunc => None,
            Overflow::AllowTrunc => Some(()),
        }
    };
    let c = byte(*at);
    if c == b'"' {
        *at = at.saturating_add(1);
        loop {
            let c = byte(*at);
            if c != 0 {
                *at = at.saturating_add(1);
            }
            if c == b'"' || c == 0 {
                return Some(token);
            }
            push(&mut token, c)?;
        }
    }
    if matches!(c, b'{' | b'}' | b'(' | b')' | b'\'' | b':') {
        push(&mut token, c)?;
        *at = at.saturating_add(1);
        return Some(token);
    }
    let mut c = c;
    loop {
        push(&mut token, c)?;
        *at = at.saturating_add(1);
        c = byte(*at);
        if matches!(c, b'{' | b'}' | b'(' | b')' | b'\'') || white(c) {
            return Some(token);
        }
    }
}

#[cfg(test)]
mod tests;
