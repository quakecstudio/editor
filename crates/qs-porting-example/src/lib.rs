// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, ED_NewString

//! The worked example of the porting process in `docs/porting.md`.
//!
//! It ports one id function, `ED_NewString`, which decodes the escapes in a string value read
//! from a map's entity lump or a saved game. The behaviour is specified in
//! `docs/spec/example.md`, section EXAMPLE-1.

/// Decodes the escapes in an entity-lump string value, as id's `ED_NewString` does.
///
/// Implements EXAMPLE-1: the input ends at its first NUL byte (EXAMPLE-1.1), a backslash
/// followed by `n` becomes a newline (EXAMPLE-1.2), a backslash followed by any other byte
/// becomes a backslash and the byte is dropped (EXAMPLE-1.3), a trailing backslash is kept
/// (EXAMPLE-1.4), every other byte is copied unchanged (EXAMPLE-1.5) and the output is
/// never re-scanned (EXAMPLE-1.6).
///
/// The returned bytes do not include the C string's terminating NUL.
#[must_use]
pub fn ed_new_string(input: &[u8]) -> Vec<u8> {
    let string = input.split(|&byte| byte == 0).next().unwrap_or_default();
    let mut decoded = Vec::with_capacity(string.len());
    let mut bytes = string.iter().copied();
    while let Some(byte) = bytes.next() {
        if byte == b'\\' {
            match bytes.next() {
                Some(b'n') => decoded.push(b'\n'),
                Some(_) | None => decoded.push(b'\\'),
            }
        } else {
            decoded.push(byte);
        }
    }
    decoded
}

#[cfg(test)]
mod tests {
    use super::ed_new_string;

    // spec EXAMPLE-1.1: the input ends at its first NUL byte.
    #[test]
    fn input_ends_at_first_nul() {
        assert_eq!(ed_new_string(b"ab\0cd"), b"ab");
        assert_eq!(ed_new_string(b"\0"), b"");
        assert_eq!(ed_new_string(b"\\\0n"), b"\\");
    }

    // spec EXAMPLE-1.2: backslash then `n` becomes one newline byte.
    #[test]
    fn backslash_n_is_newline() {
        assert_eq!(ed_new_string(b"\\n"), b"\n");
        assert_eq!(ed_new_string(b"Hello\\nWorld"), b"Hello\nWorld");
        assert_eq!(ed_new_string(b"\\n\\n"), b"\n\n");
    }

    // spec EXAMPLE-1.3: backslash then any other byte becomes one backslash; the byte is dropped.
    #[test]
    fn backslash_other_is_backslash() {
        assert_eq!(ed_new_string(b"\\\\"), b"\\");
        assert_eq!(ed_new_string(b"a\\tb"), b"a\\b");
        assert_eq!(ed_new_string(b"\\\""), b"\\");
        assert_eq!(ed_new_string(b"\\N"), b"\\");
        assert_eq!(ed_new_string(b"\\\xff"), b"\\");
    }

    // spec EXAMPLE-1.4: a backslash as the last byte stays a backslash.
    #[test]
    fn trailing_backslash_is_kept() {
        assert_eq!(ed_new_string(b"\\"), b"\\");
        assert_eq!(ed_new_string(b"path\\"), b"path\\");
        assert_eq!(ed_new_string(b"\\\\\\"), b"\\\\");
    }

    // spec EXAMPLE-1.5: every other byte is copied unchanged.
    #[test]
    fn other_bytes_are_copied() {
        assert_eq!(ed_new_string(b""), b"");
        assert_eq!(ed_new_string(b"func_door"), b"func_door");
        assert_eq!(ed_new_string(b"\x01\x7f\x80\xff\n"), b"\x01\x7f\x80\xff\n");
    }

    // spec EXAMPLE-1.6: escapes do not chain; the output is never re-scanned.
    #[test]
    fn output_is_not_rescanned() {
        assert_eq!(ed_new_string(b"\\\\n"), b"\\n");
        assert_eq!(ed_new_string(b"\\\\\\n"), b"\\\n");
    }

    // spec EXAMPLE-1.7: the output is never longer than the input.
    #[test]
    fn output_is_never_longer() {
        for input in [&b""[..], b"\\", b"\\n", b"\\\\n", b"abc", b"a\\"] {
            assert!(ed_new_string(input).len() <= input.len());
        }
    }
}
