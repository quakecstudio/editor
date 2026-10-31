// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2016      Spike
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from Novum/vkQuake@27789d7333a46674474d871137bb71e056719628: Quake/pr_ext.c, PF_strconv, chrconv_number, chrconv_punct, chrchar_alpha

//! vkQuake's `strconv`: a string's case and Quake's coloured-character offsets changed byte by
//! byte (BUILTIN-18.17).

/// `chrconv_number`: digit `byte` of the digit row starting at `base`, moved to the row `conv`
/// names (1 white, 2 red, 3 and 4 the special rows; any other keeps it).
fn number(byte: u8, base: u8, conv: i32) -> u8 {
    let to = match conv {
        1 => b'0',
        2 => b'0'.wrapping_add(128),
        3 => b'0'.wrapping_sub(30),
        4 => b'0'.wrapping_add(98),
        _ => base,
    };
    byte.wrapping_sub(base).wrapping_add(to)
}

/// `chrconv_punct`: punctuation `byte` of the half starting at `base`, moved to the half
/// `conv` names (1 white, 2 red; any other keeps it).
fn punct(byte: u8, base: u8, conv: i32) -> u8 {
    let to = match conv {
        1 => 0,
        2 => 128,
        _ => base,
    };
    byte.wrapping_sub(base).wrapping_add(to)
}

/// `chrchar_alpha`: letter `byte` of the case row `case_base` in the half `half`, given the
/// case `convc` names (1 lower, 2 upper) and the half `convt` names (1 white, 2 red, 5 and 6
/// alternating by the byte's position `at`).
fn alpha(byte: u8, case_base: u8, half: u8, convc: i32, convt: i32, at: usize) -> u8 {
    let letter = byte.wrapping_sub(half.wrapping_add(case_base));
    let half = match convt {
        1 => 0,
        2 => 128,
        5 | 6 => {
            let odd = i32::from(at & 1 == 1);
            if odd == convt.wrapping_sub(5) { 128 } else { 0 }
        }
        _ => half,
    };
    let case_base = match convc {
        1 => b'a',
        2 => b'A',
        _ => case_base,
    };
    letter.wrapping_add(case_base).wrapping_add(half)
}

/// `strconv(ccase, redalpha, rednum, s)`'s text of `text`, its first 1023 bytes (BUILTIN-18.17).
pub(crate) fn strconv(ccase: i32, redalpha: i32, rednum: i32, text: &[u8]) -> Vec<u8> {
    let text = text.get(..text.len().min(1023)).unwrap_or(text);
    let mut out = Vec::with_capacity(text.len());
    for (at, &byte) in text.iter().enumerate() {
        let red = byte.wrapping_sub(128);
        let converted = if byte.is_ascii_digit() {
            number(byte, b'0', rednum)
        } else if red.is_ascii_digit() && byte >= 128 {
            number(byte, b'0'.wrapping_add(128), rednum)
        } else if (b'0'.wrapping_add(98)..=b'9'.wrapping_add(98)).contains(&byte) {
            number(byte, b'0'.wrapping_add(98), rednum)
        } else if (b'0'.wrapping_sub(30)..=b'9'.wrapping_sub(30)).contains(&byte) {
            number(byte, b'0'.wrapping_sub(30), rednum)
        } else if byte.is_ascii_lowercase() {
            alpha(byte, b'a', 0, ccase, redalpha, at)
        } else if byte.is_ascii_uppercase() {
            alpha(byte, b'A', 0, ccase, redalpha, at)
        } else if byte >= 128 && red.is_ascii_lowercase() {
            alpha(byte, b'a', 128, ccase, redalpha, at)
        } else if byte >= 128 && red.is_ascii_uppercase() {
            alpha(byte, b'A', 128, ccase, redalpha, at)
        } else if byte & 127 < 16 || redalpha == 0 {
            byte
        } else if byte < 128 {
            punct(byte, 0, redalpha)
        } else {
            punct(byte, 128, redalpha)
        };
        out.push(converted);
    }
    out
}
