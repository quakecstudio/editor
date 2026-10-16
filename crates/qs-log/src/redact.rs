// SPDX-License-Identifier: GPL-3.0-or-later

//! Redaction: how source text is kept out of the logs (QA-7).

use std::fmt;

/// Field names whose values are always redacted, whatever their type.
///
/// A field named here never reaches a log file, so `tracing::debug!(source = text)` is safe even
/// when `text` is a plain `&str`.
pub const SOURCE_FIELD_NAMES: &[&str] = &[
    "source",
    "src",
    "source_text",
    "text",
    "snippet",
    "content",
    "contents",
];

/// Whether a field with this name holds source text.
pub(crate) fn is_source_field(name: &str) -> bool {
    SOURCE_FIELD_NAMES.contains(&name)
}

/// The marker written in place of `len` bytes of redacted text.
pub(crate) fn marker(len: usize) -> String {
    format!("<redacted {len} bytes>")
}

/// Source text that must never be logged.
///
/// Wrap QuakeC source, map text or any other user content in `SourceText` before it can reach a
/// log, an error message or a panic: its [`Display`](fmt::Display) and [`Debug`] print only its
/// length, as `<redacted 42 bytes>`, so it is safe in a message (`info!("{}", SourceText(s))`), a
/// field (`info!(line = %SourceText(s))`) and a span alike.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SourceText<T: AsRef<str>>(pub T);

impl<T: AsRef<str>> fmt::Display for SourceText<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&marker(self.0.as_ref().len()))
    }
}

impl<T: AsRef<str>> fmt::Debug for SourceText<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_debug_show_only_the_length() {
        let text = SourceText("void() main = {};");
        assert_eq!(text.to_string(), "<redacted 17 bytes>");
        assert_eq!(format!("{text:?}"), "<redacted 17 bytes>");
        assert_eq!(
            format!("{:?}", SourceText(String::new())),
            "<redacted 0 bytes>"
        );
    }

    #[test]
    fn length_is_in_bytes_not_chars() {
        assert_eq!(SourceText("\u{e9}").to_string(), "<redacted 2 bytes>");
    }

    #[test]
    fn source_field_names_match_exactly() {
        for name in SOURCE_FIELD_NAMES {
            assert!(is_source_field(name), "{name}");
        }
        for name in ["message", "path", "sources", "Source", ""] {
            assert!(!is_source_field(name), "{name}");
        }
    }
}
