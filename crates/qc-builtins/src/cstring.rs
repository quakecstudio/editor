// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-2001 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from Novum/vkQuake@27789d7333a46674474d871137bb71e056719628: Quake/common.c, q_strcasecmp, q_strncasecmp

//! The C string comparisons vkQuake's string builtins call (BUILTIN-18.22): glibc's `strcmp`
//! and `strncmp`, and QuakeSpasm's `q_strcasecmp` and `q_strncasecmp`.

/// The byte at `i` of a C string, 0 past its end.
fn at(text: &[u8], i: usize) -> u8 {
    text.get(i).copied().unwrap_or(0)
}

/// glibc's `strncmp (a, b, n)`, `strcmp` with no `n`: the difference of the first bytes that
/// differ, as unsigned chars, or 0.
pub(crate) fn strncmp(a: &[u8], b: &[u8], n: Option<usize>) -> i32 {
    let mut i = 0_usize;
    while n.is_none_or(|n| i < n) {
        let (ca, cb) = (at(a, i), at(b, i));
        if ca != cb {
            return i32::from(ca).wrapping_sub(i32::from(cb));
        }
        if ca == 0 {
            return 0;
        }
        i = i.saturating_add(1);
    }
    0
}

/// A byte through `q_tolower`, then stored in a `char`, which x86 signs.
fn lower(byte: u8) -> i32 {
    i32::from(i8::from_ne_bytes([byte.to_ascii_lowercase()]))
}

/// `q_strcasecmp (a, b)`, or with `n` `q_strncasecmp (a, b, n)`: `c1 - c2` of the first pair of
/// lowered `char`s that differ or where `a` ends.
pub(crate) fn strncasecmp(a: &[u8], b: &[u8], n: Option<usize>) -> i32 {
    if n == Some(0) {
        return 0;
    }
    let mut i = 0_usize;
    let (mut c1, mut c2);
    loop {
        c1 = lower(at(a, i));
        c2 = lower(at(b, i));
        i = i.saturating_add(1);
        if c1 == 0 || c1 != c2 {
            break;
        }
        if n.is_some_and(|n| i >= n) {
            break;
        }
    }
    c1.wrapping_sub(c2)
}

/// `strstr (text, needle)`'s offset in `text`, `None` when it is not there; an empty needle is
/// at 0.
pub(crate) fn strstr(text: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    text.windows(needle.len()).position(|w| w == needle)
}
