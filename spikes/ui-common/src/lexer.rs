// SPDX-License-Identifier: GPL-3.0-or-later
//! A line-at-a-time QuakeC tokenizer for highlighting.
//!
//! This is not the compiler's lexer (that is `qc-syntax`, later). It only has to colour code the
//! way the mockups do, and it has to be cheap enough to run on the visible lines every frame.
//! Block comments are the only state carried between lines.

use std::collections::HashMap;
use std::ops::Range;

/// What a span of text is, for colouring. The semantic kinds (`Function`, `Builtin`,
/// `Constant`, `Field`) come from [`Symbols`], which stands in for the semantic model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Keyword,
    Type,
    Function,
    Builtin,
    Constant,
    Field,
    Number,
    String,
    Vector,
    Comment,
    Punctuation,
    Identifier,
    Whitespace,
}

/// One coloured span of a line, as a byte range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub kind: TokenKind,
}

/// Whether a line starts inside a `/* */` comment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineState {
    pub in_block_comment: bool,
}

const KEYWORDS: &[&str] = &[
    "if",
    "else",
    "while",
    "do",
    "for",
    "return",
    "local",
    "break",
    "continue",
    "switch",
    "case",
    "default",
    "const",
    "var",
    "nosave",
    "shared",
    "static",
    "nonstatic",
    "goto",
    "typedef",
    "enum",
    "enumflags",
    "class",
    "struct",
    "union",
    "inline",
    "virtual",
    "noref",
    "optional",
];
const TYPES: &[&str] = &[
    "void",
    "float",
    "vector",
    "string",
    "entity",
    "int",
    "integer",
    "bool",
    "__variant",
];

/// Names the semantic model would resolve, simulated by scanning declarations.
#[derive(Debug, Default)]
pub struct Symbols {
    kinds: HashMap<String, TokenKind>,
}

impl Symbols {
    /// Scans every line for declarations: `type(...) name` is a function (a builtin when its
    /// body is `#n`), `.type name` is a field, and an all-capitals name is a constant.
    #[must_use]
    pub fn scan<'a>(lines: impl Iterator<Item = &'a str>) -> Self {
        let mut kinds = HashMap::new();
        for line in lines {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix('.') {
                if let Some(name) = declared_name(rest) {
                    kinds.insert(name.to_owned(), TokenKind::Field);
                }
                continue;
            }
            let Some(close) = trimmed.find(')') else {
                continue;
            };
            let starts_with_type = TYPES.iter().any(|ty| trimmed.starts_with(ty));
            if !starts_with_type {
                continue;
            }
            let after = trimmed.get(close.saturating_add(1)..).unwrap_or("");
            if let Some(name) = declared_name(after) {
                let kind = if after.contains('#') {
                    TokenKind::Builtin
                } else {
                    TokenKind::Function
                };
                kinds.insert(name.to_owned(), kind);
            }
        }
        Self { kinds }
    }

    fn kind(&self, word: &str) -> TokenKind {
        if let Some(kind) = self.kinds.get(word) {
            return *kind;
        }
        let has_upper = word.bytes().any(|b| b.is_ascii_uppercase());
        let all_caps = word
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
        if has_upper && all_caps && word.len() > 1 {
            TokenKind::Constant
        } else {
            TokenKind::Identifier
        }
    }
}

fn declared_name(text: &str) -> Option<&str> {
    let text = text.trim_start();
    let text = text
        .split_once(|c: char| c.is_whitespace())
        .map_or(text, |(first, rest)| {
            if TYPES.contains(&first) {
                rest.trim_start()
            } else {
                text
            }
        });
    let end = text
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    let name = text.get(..end)?;
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        None
    } else {
        Some(name)
    }
}

/// Tokenizes one line. Returns the spans, which cover the line without gaps, and the state the
/// next line starts in.
#[must_use]
pub fn lex_line(line: &str, state: LineState, symbols: &Symbols) -> (Vec<Span>, LineState) {
    let bytes = line.as_bytes();
    let mut spans = Vec::new();
    let mut pos = 0;
    let mut in_block = state.in_block_comment;
    while pos < bytes.len() {
        let start = pos;
        let rest = bytes.get(pos..).unwrap_or(&[]);
        let kind = if in_block {
            match find(rest, b"*/") {
                Some(end) => {
                    pos = pos.saturating_add(end).saturating_add(2);
                    in_block = false;
                }
                None => pos = bytes.len(),
            }
            TokenKind::Comment
        } else if rest.starts_with(b"//") {
            pos = bytes.len();
            TokenKind::Comment
        } else if rest.starts_with(b"/*") {
            in_block = true;
            pos = pos.saturating_add(2);
            match find(bytes.get(pos..).unwrap_or(&[]), b"*/") {
                Some(end) => {
                    pos = pos.saturating_add(end).saturating_add(2);
                    in_block = false;
                }
                None => pos = bytes.len(),
            }
            TokenKind::Comment
        } else {
            let Some(&b) = rest.first() else { break };
            match b {
                b'"' | b'\'' => {
                    pos = pos.saturating_add(1);
                    while let Some(&c) = bytes.get(pos) {
                        pos = pos.saturating_add(1);
                        if c == b'\\' {
                            pos = pos.saturating_add(1);
                        } else if c == b {
                            break;
                        }
                    }
                    pos = pos.min(bytes.len());
                    if b == b'"' {
                        TokenKind::String
                    } else {
                        TokenKind::Vector
                    }
                }
                b' ' | b'\t' => {
                    while matches!(bytes.get(pos), Some(b' ' | b'\t')) {
                        pos = pos.saturating_add(1);
                    }
                    TokenKind::Whitespace
                }
                b'0'..=b'9' => {
                    while matches!(bytes.get(pos), Some(c) if c.is_ascii_alphanumeric() || *c == b'.')
                    {
                        pos = pos.saturating_add(1);
                    }
                    TokenKind::Number
                }
                c if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 => {
                    while matches!(bytes.get(pos), Some(c) if c.is_ascii_alphanumeric() || *c == b'_' || *c >= 0x80)
                    {
                        pos = pos.saturating_add(1);
                    }
                    let word = line.get(start..pos).unwrap_or("");
                    if KEYWORDS.contains(&word) {
                        TokenKind::Keyword
                    } else if TYPES.contains(&word) {
                        TokenKind::Type
                    } else {
                        symbols.kind(word)
                    }
                }
                _ => {
                    pos = pos.saturating_add(1);
                    TokenKind::Punctuation
                }
            }
        };
        let range = start..pos;
        match spans.last_mut() {
            Some(Span {
                range: last,
                kind: last_kind,
            }) if *last_kind == kind => last.end = pos,
            _ => spans.push(Span { range, kind }),
        }
    }
    (
        spans,
        LineState {
            in_block_comment: in_block,
        },
    )
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str, symbols: &Symbols) -> Vec<(String, TokenKind)> {
        let (spans, _) = lex_line(line, LineState::default(), symbols);
        spans
            .into_iter()
            .filter(|s| s.kind != TokenKind::Whitespace)
            .map(|s| (line.get(s.range).unwrap_or("").to_owned(), s.kind))
            .collect()
    }

    #[test]
    fn colours_a_statement_like_the_mockup() {
        let symbols = Symbols::scan(
            [
                "void() W_FireEmber =",
                "entity() spawn = #14;",
                ".float bounces;",
            ]
            .into_iter(),
        );
        let got = kinds("local entity e = spawn(); // x", &symbols);
        assert_eq!(
            got,
            vec![
                ("local".into(), TokenKind::Keyword),
                ("entity".into(), TokenKind::Type),
                ("e".into(), TokenKind::Identifier),
                ("=".into(), TokenKind::Punctuation),
                ("spawn".into(), TokenKind::Builtin),
                ("();".into(), TokenKind::Punctuation),
                ("// x".into(), TokenKind::Comment),
            ]
        );
        let got = kinds("self.bounces = MOVETYPE_BOUNCE + '0 0 16' + 0.7;", &symbols);
        assert!(got.contains(&("bounces".into(), TokenKind::Field)));
        assert!(got.contains(&("MOVETYPE_BOUNCE".into(), TokenKind::Constant)));
        assert!(got.contains(&("'0 0 16'".into(), TokenKind::Vector)));
        assert!(got.contains(&("0.7".into(), TokenKind::Number)));
        assert_eq!(
            kinds("W_FireEmber();", &symbols).first(),
            Some(&("W_FireEmber".into(), TokenKind::Function))
        );
    }

    #[test]
    fn carries_block_comments_across_lines() {
        let symbols = Symbols::default();
        let (_, state) = lex_line("a /* open", LineState::default(), &symbols);
        assert!(state.in_block_comment);
        let (spans, state) = lex_line("still */ b", state, &symbols);
        assert!(!state.in_block_comment);
        assert_eq!(spans.first().map(|s| s.kind), Some(TokenKind::Comment));
        assert_eq!(spans.last().map(|s| s.kind), Some(TokenKind::Identifier));
    }

    #[test]
    fn spans_cover_the_line_without_gaps() {
        let line = "if (x) { y = \"unterminated";
        let (spans, _) = lex_line(line, LineState::default(), &Symbols::default());
        let mut end = 0;
        for span in &spans {
            assert_eq!(span.range.start, end);
            end = span.range.end;
        }
        assert_eq!(end, line.len());
    }
}
