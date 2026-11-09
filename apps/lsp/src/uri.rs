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
//!
//! On Windows a URI's host other than `localhost` names a UNC share, as VS Code writes one:
//! vscode-uri's `URI.file` says "check for authority as used in UNC shares" and takes the host
//! from `\\host\share\...`, and its `uriToFsPath` reads it back ("unc path:
//! file://shares/c$/far/boo"), fetched 2026-11-04 from
//! https://raw.githubusercontent.com/microsoft/vscode-uri/main/src/uri.ts. RFC 8089, Appendix
//! E.3.1 (non-normative), maps "\\host.example.com\Share\path\to\file.txt" to
//! "file://host.example.com/Share/path/to/file.txt" the same way.

use std::path::{Path, PathBuf};

/// Whose rules a path follows: a drive letter and `\` separators are Windows's alone, and on
/// any other host `/c:/x` is the absolute path it spells and `\` a byte of a file's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Host {
    /// Windows.
    Windows,
    /// Linux, macOS and the other hosts with one root.
    Unix,
}

/// The host this server runs on.
const HOST: Host = if cfg!(windows) {
    Host::Windows
} else {
    Host::Unix
};

/// The path a `file` URI names; `None` for another scheme, or, except on Windows, where it is
/// the UNC path `\\host\share\...`, a URI with a host other than `localhost`.
#[must_use]
pub fn to_path(uri: &str) -> Option<PathBuf> {
    to_path_on(uri, HOST)
}

/// The `file` URI of `path`; on Windows a UNC path `\\host\share\...` is written with its host
/// as the URI's.
#[must_use]
pub fn from_path(path: &Path) -> String {
    from_path_on(path, HOST)
}

/// [`to_path`] by the rules of `host`.
pub(crate) fn to_path_on(uri: &str, host: Host) -> Option<PathBuf> {
    let rest = strip_scheme(uri)?;
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (authority, path) = rest.split_at(rest.find('/')?);
    let decoded = String::from_utf8(percent_decode(path)).ok()?;
    if !(authority.is_empty() || authority.eq_ignore_ascii_case("localhost")) {
        return unc_path(authority, &decoded, host);
    }
    // `/c:/x` names the Windows path `c:/x`.
    let bytes = decoded.as_bytes();
    let drive = host == Host::Windows
        && bytes.len() >= 3
        && bytes.get(1).is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(2) == Some(&b':');
    if drive {
        Some(PathBuf::from(decoded.get(1..)?))
    } else {
        Some(PathBuf::from(decoded))
    }
}

/// The UNC path `\\server\share\...` that a `file` URI with the host `authority` and the
/// decoded path `path` names on Windows. `None` on a host with one root; for a host that decodes
/// to nothing, to a separator or to invalid UTF-8; and for the path `/` alone, which names no
/// share, as vscode-uri's `uriToFsPath` reads a host as a share only `if (uri.authority &&
/// uri.path.length > 1 && uri.scheme === 'file')`.
fn unc_path(authority: &str, path: &str, host: Host) -> Option<PathBuf> {
    if host == Host::Unix || path.len() <= 1 {
        return None;
    }
    let server = String::from_utf8(percent_decode(authority)).ok()?;
    if server.is_empty() || server.contains(['/', '\\']) {
        return None;
    }
    let mut unc = String::from("\\\\");
    unc.push_str(&server);
    unc.push_str(&path.replace('/', "\\"));
    Some(PathBuf::from(unc))
}

/// [`from_path`] by the rules of `host`.
pub(crate) fn from_path_on(path: &Path, host: Host) -> String {
    let lossy = path.to_string_lossy();
    let text = match host {
        Host::Windows => lossy.replace('\\', "/"),
        Host::Unix => lossy.into_owned(),
    };
    // A verbatim path: `//?/UNC/host/share/...` is the share, and `//?/c:/...` the drive path.
    let text = match text.strip_prefix("//?/") {
        Some(rest) if host == Host::Windows => match rest.strip_prefix("UNC/") {
            Some(share) => format!("//{share}"),
            None => rest.to_owned(),
        },
        Some(_) | None => text,
    };
    let mut out = String::from("file://");
    // A UNC path, `//host/share/...` once its separators are `/`: the host is the URI's, in
    // lower case as vscode-uri's `_asFormatted` writes it (`authority =
    // authority.toLowerCase();`). A device path, `//./...`, is no share.
    let unc = text
        .strip_prefix("//")
        .map(|rest| rest.split_at(rest.find('/').unwrap_or(rest.len())))
        .filter(|(server, _)| host == Host::Windows && !matches!(*server, "" | "." | "?"));
    if let Some((server, path)) = unc {
        push_encoded(&mut out, server.to_ascii_lowercase().as_bytes());
        out.push('/');
        push_encoded(&mut out, path.strip_prefix('/').unwrap_or(path).as_bytes());
        return out;
    }
    let bytes = text.as_bytes();
    let drive = host == Host::Windows
        && bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(1) == Some(&b':');
    let bytes = match bytes.split_at_checked(2) {
        Some((letter, rest)) if drive => {
            out.push('/');
            out.push_str(&String::from_utf8_lossy(letter));
            rest
        }
        Some(_) | None => bytes,
    };
    push_encoded(&mut out, bytes);
    out
}

/// Writes `bytes` to `out`, each byte outside RFC 3986's unreserved set and `/` percent-encoded.
fn push_encoded(out: &mut String, bytes: &[u8]) {
    for &b in bytes {
        let keep = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/');
        if keep {
            out.push(char::from(b));
        } else {
            out.push('%');
            for digit in [b >> 4, b & 0x0F] {
                let hex = char::from_digit(u32::from(digit), 16).unwrap_or('0');
                out.push(hex.to_ascii_uppercase());
            }
        }
    }
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
