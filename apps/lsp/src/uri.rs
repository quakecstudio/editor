// SPDX-License-Identifier: GPL-3.0-or-later

//! `file` URIs to paths and back.
//!
//! LSP 3.18, "URI": "URI's are transferred as strings. The URI's format is defined in
//! https://tools.ietf.org/html/rfc3986", and "some clients (such as VS Code) may encode colons in
//! drive letters while others do not ... Clients and servers should not assume that each other
//! are encoding the same way" (`specs/lsp/3.18/includes/types/uri.md`). So a URI is decoded
//! whatever it encodes, and the server writes its own the one way: `file://`, then the path
//! with `/` separators, every byte outside RFC 3986's unreserved set, `/` and a drive letter's
//! `:` percent-encoded.

use std::path::{Path, PathBuf};

/// The path a `file` URI names; `None` for another scheme or a URI with a host other than
/// `localhost`.
#[must_use]
pub fn to_path(uri: &str) -> Option<PathBuf> {
    let rest = strip_scheme(uri)?;
    let (authority, path) = rest.split_at(rest.find('/')?);
    if !(authority.is_empty() || authority.eq_ignore_ascii_case("localhost")) {
        return None;
    }
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let decoded = String::from_utf8(percent_decode(path)).ok()?;
    // `/c:/x` names the Windows path `c:/x`.
    let bytes = decoded.as_bytes();
    let drive = bytes.len() >= 3
        && bytes.get(1).is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(2) == Some(&b':');
    if drive {
        Some(PathBuf::from(decoded.get(1..)?))
    } else {
        Some(PathBuf::from(decoded))
    }
}

/// The `file` URI of `path`.
#[must_use]
pub fn from_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    let bytes = text.as_bytes();
    let drive = bytes.first().is_some_and(u8::is_ascii_alphabetic) && bytes.get(1) == Some(&b':');
    if drive {
        out.push('/');
    }
    for (index, &b) in bytes.iter().enumerate() {
        let keep = b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/')
            || (drive && index == 1);
        if keep {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The part of a URI after `file://`, the scheme matched without regard to case.
fn strip_scheme(uri: &str) -> Option<&str> {
    let (scheme, rest) = uri.split_once("://")?;
    scheme.eq_ignore_ascii_case("file").then_some(rest)
}

/// The bytes `text` percent-decodes to; a `%` not followed by two hex digits is itself.
fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0_usize;
    while let Some(&b) = bytes.get(at) {
        let hex = |i: usize| bytes.get(i).and_then(|&h| char::from(h).to_digit(16));
        if b == b'%'
            && let (Some(high), Some(low)) = (hex(at.saturating_add(1)), hex(at.saturating_add(2)))
            && let Ok(byte) = u8::try_from(high.saturating_mul(16).saturating_add(low))
        {
            out.push(byte);
            at = at.saturating_add(3);
        } else {
            out.push(b);
            at = at.saturating_add(1);
        }
    }
    out
}
