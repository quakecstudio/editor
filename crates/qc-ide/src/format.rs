// SPDX-License-Identifier: GPL-3.0-or-later

//! The QuakeC formatter (P4-34, UI-18): each line indented with tabs to its depth in the code's
//! blocks, trailing white space removed and the file ended with one line break.
//!
//! The model is id's own layout of `qcc`'s sources and gofmt's: tabs for indentation, a block's
//! body one level in from its braces, and a statement a `if`, `while`, `for`, `else` or `do`
//! governs without braces one level in from it. The formatter changes only white space at the
//! start and the end of lines: it never joins or splits a line or touches a token, so the
//! program compiles to the same `progs.dat` with the same line numbers, and formatting a
//! formatted file changes nothing. A line that starts inside a block comment or a token that
//! spans lines is left as it is but for its trailing white space outside a token, and a
//! preprocessor line keeps its own indentation, so comments are kept as written.

use std::path::Path;

use qc_lexer::{Dialect, Punct, Token, TokenKind, lex_with};
use qc_project::{Loader, normalize};

use crate::host::AnalysisHost;

impl<L: Loader> AnalysisHost<L> {
    /// `path`'s current text formatted ([`format`]) in the dialect of the first module that
    /// compiles it, vanilla for a file no module compiles; `None` when there is no text.
    pub fn formatted(&mut self, path: &Path) -> Option<Vec<u8>> {
        let text = self.text(path)?;
        let path = normalize(path);
        let dialect = self
            .modules
            .iter()
            .find(|module| module.files.contains(&path))
            .map_or(Dialect::Vanilla, |module| module.dialect);
        Some(format(&text, dialect))
    }
}

/// How a line is formatted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineKind {
    /// Code: indented to its depth.
    Code(usize),
    /// Inside a block comment or a token over several lines, or a preprocessor line: kept,
    /// only its trailing white space removed outside a token.
    Kept,
    /// A line with nothing on it but white space: emptied.
    Blank,
}

/// The ranges no line break may be reindented in: every token, and every comment between two.
fn protected(text: &[u8], tokens: &[Token]) -> Vec<(usize, usize)> {
    const OPEN: &[u8] = b"/*";
    const CLOSE: &[u8] = b"*/";
    let mut out: Vec<(usize, usize)> = tokens.iter().map(|t| (t.span.start, t.span.end)).collect();
    let mut gaps = Vec::with_capacity(tokens.len().saturating_add(1));
    let mut at = 0_usize;
    for token in tokens {
        gaps.push((at, token.span.start));
        at = token.span.end;
    }
    gaps.push((at, text.len()));
    for (start, end) in gaps {
        let gap = text.get(start..end).unwrap_or_default();
        let mut offset = 0_usize;
        while let Some(rest) = gap.get(offset..) {
            let block = find(rest, OPEN).map(|i| (i, true));
            let line = find(rest, b"//").map(|i| (i, false));
            let Some((index, is_block)) = block.into_iter().chain(line).min() else {
                break;
            };
            let begin = offset.saturating_add(index);
            let close = if is_block {
                rest.get(index.saturating_add(OPEN.len())..)
                    .and_then(|after| find(after, CLOSE))
                    .map_or(gap.len(), |i| {
                        begin
                            .saturating_add(i)
                            .saturating_add(OPEN.len())
                            .saturating_add(CLOSE.len())
                    })
            } else {
                rest.get(index..)
                    .and_then(|after| after.iter().position(|b| *b == b'\n'))
                    .map_or(gap.len(), |i| begin.saturating_add(i))
            };
            out.push((start.saturating_add(begin), start.saturating_add(close)));
            offset = close.max(begin.saturating_add(1));
        }
    }
    out
}

/// Where `needle` first starts in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Whether `token` is the punctuation `punct`.
fn is(token: &Token, punct: Punct) -> bool {
    token.kind == TokenKind::Punct(punct)
}

/// Whether `token` is the keyword `word`.
fn is_word(token: &Token, word: &str) -> bool {
    matches!(&token.kind, TokenKind::Name(name) if name == word)
}

/// `text` formatted: each code line indented with tabs to its depth, trailing white space
/// removed, one line break at the end. Line breaks stay as they are, `\r\n` included. The
/// text is bytes, as the compiler reads it, so bytes that are not UTF-8 are kept.
#[must_use]
pub fn format(text: &[u8], dialect: Dialect) -> Vec<u8> {
    let tokens = lex_with(text, dialect).tokens;
    let protected = protected(text, &tokens);
    let mut out: Vec<u8> = Vec::with_capacity(text.len());
    let mut next_token = 0_usize;
    // The open braces and parentheses before the line, and whether the line before governs
    // the next statement without braces.
    let mut braces = 0_usize;
    let mut parens = 0_usize;
    let mut governed = 0_usize;
    let mut line_start = 0_usize;
    for line in text.split_inclusive(|b| *b == b'\n') {
        let line_end = line_start.saturating_add(line.len());
        let inside = protected
            .iter()
            .any(|(start, end)| *start < line_start && line_start < *end);
        // The tokens that start on this line.
        let first = next_token;
        while tokens
            .get(next_token)
            .is_some_and(|t| t.span.start < line_end)
        {
            next_token = next_token.saturating_add(1);
        }
        let on_line = tokens.get(first..next_token).unwrap_or_default();
        let kind = if inside {
            LineKind::Kept
        } else if line.trim_ascii().is_empty() {
            LineKind::Blank
        } else if on_line
            .first()
            .is_some_and(|t| is(t, Punct::Hash) && t.line_start)
        {
            LineKind::Kept
        } else {
            let closes = on_line
                .first()
                .is_some_and(|t| is(t, Punct::RBrace) || is(t, Punct::RParen));
            let opens_block = on_line.first().is_some_and(|t| is(t, Punct::LBrace));
            let mut depth = braces.saturating_add(usize::from(parens > 0));
            if on_line.first().is_some_and(|t| is(t, Punct::RBrace)) {
                depth = depth.saturating_sub(1);
            }
            if !opens_block && !closes {
                depth = depth.saturating_add(governed);
            } else if opens_block {
                depth = depth.saturating_add(governed.saturating_sub(1));
            }
            LineKind::Code(depth)
        };
        // What the line's tokens leave open for the next.
        for token in on_line {
            if is(token, Punct::LBrace) {
                braces = braces.saturating_add(1);
            } else if is(token, Punct::RBrace) {
                braces = braces.saturating_sub(1);
            } else if is(token, Punct::LParen) {
                parens = parens.saturating_add(1);
            } else if is(token, Punct::RParen) {
                parens = parens.saturating_sub(1);
            }
        }
        if let LineKind::Code(_) = kind {
            governed = governs(on_line, governed);
        }
        let (body, ending) = split_ending(line);
        match kind {
            LineKind::Blank => {}
            LineKind::Kept => {
                // A line that starts inside a token keeps its trailing white space, which may
                // be the token's.
                let in_token = tokens
                    .iter()
                    .any(|t| t.span.start < line_start && line_start < t.span.end);
                out.extend_from_slice(if in_token {
                    body
                } else {
                    body.trim_ascii_end()
                });
            }
            LineKind::Code(depth) => {
                out.extend(std::iter::repeat_n(b'\t', depth));
                out.extend_from_slice(body.trim_ascii());
            }
        }
        out.extend_from_slice(ending);
        line_start = line_end;
    }
    // One line break at the end.
    let crlf = find(&out, b"\r\n").is_some();
    while out.last().is_some_and(|b| matches!(b, b'\n' | b'\r')) {
        out.pop();
    }
    if !out.is_empty() {
        out.extend_from_slice(if crlf { b"\r\n" } else { b"\n" });
    }
    out
}

/// How many levels the line after `line` is indented for statements governed without braces:
/// one more after a line that ends a `if (...)`, `while (...)` or `for (...)` header or is
/// `else` or `do`, none after a line that ends a statement or a block.
fn governs(line: &[Token], before: usize) -> usize {
    let (Some(first), Some(last)) = (line.first(), line.last()) else {
        return before;
    };
    let header = ["if", "while", "for"].iter().any(|w| is_word(first, w))
        || (is_word(first, "else") && line.get(1).is_some_and(|t| is_word(t, "if")))
        || (is(first, Punct::RBrace) && line.get(1).is_some_and(|t| is_word(t, "else")));
    let ends_header = is(last, Punct::RParen) && header;
    let bare = is_word(last, "else") || is_word(last, "do");
    if ends_header || bare {
        before.saturating_add(1)
    } else if is(last, Punct::Semicolon) || is(last, Punct::RBrace) || is(last, Punct::LBrace) {
        0
    } else {
        before
    }
}

/// `line` without its line break, and the line break.
fn split_ending(line: &[u8]) -> (&[u8], &[u8]) {
    if let Some(body) = line.strip_suffix(b"\r\n") {
        (body, b"\r\n")
    } else if let Some(body) = line.strip_suffix(b"\n") {
        (body, b"\n")
    } else {
        (line, b"")
    }
}

#[cfg(test)]
mod tests {
    use qc_lexer::Dialect;

    use super::format;

    fn vanilla(text: &str) -> String {
        String::from_utf8(format(text.as_bytes(), Dialect::Vanilla)).unwrap_or_default()
    }

    // No spec statement: P4-34, UI-18: blocks indent their bodies with tabs, a statement an
    // `if` or `else` governs without braces one more, and trailing white space goes
    #[test]
    fn blocks_and_governed_statements_are_indented_with_tabs() {
        let text = "void() f =\n{\n  local float a;   \nif (a)\nreturn;\nelse\n    {\na = 1;\n      }\n\
                    while (a)\n{\n a = a - 1;\n}\n};\n";
        let want = "void() f =\n{\n\tlocal float a;\n\tif (a)\n\t\treturn;\n\telse\n\t{\n\t\ta = 1;\n\
                    \t}\n\twhile (a)\n\t{\n\t\ta = a - 1;\n\t}\n};\n";
        assert_eq!(vanilla(text), want);
        assert_eq!(
            vanilla(want),
            want,
            "formatting a formatted file changes nothing"
        );
    }

    // No spec statement: P4-34: comments and lines inside a block comment are kept as written
    // but for their trailing white space,
    // a preprocessor line keeps its place, and a call over several lines is one level in
    #[test]
    fn comments_directives_and_continued_calls() {
        let text = "/* a block\n   comment kept   \n*/\n#define X 1\n  #ifdef X\nvoid() f =\n{\n\
                    // a line comment\nsprint (self,\n\"x\");\n};\n\n\n";
        let want = "/* a block\n   comment kept\n*/\n#define X 1\n  #ifdef X\nvoid() f =\n{\n\
                    \t// a line comment\n\tsprint (self,\n\t\t\"x\");\n};\n";
        assert_eq!(vanilla(text), want);
        assert_eq!(vanilla(want), want);
    }

    // No spec statement: P4-34: Latin-1 bytes are kept as they are
    #[test]
    fn bytes_that_are_not_utf8_are_kept() {
        let text = b"string s = \"caf\xe9\";  \n";
        assert_eq!(format(text, Dialect::Vanilla), b"string s = \"caf\xe9\";\n");
    }

    // No spec statement: P4-34: CRLF line breaks are kept, and an empty file stays empty
    #[test]
    fn line_breaks_are_kept_and_one_ends_the_file() {
        assert_eq!(
            vanilla("void() f =\r\n{\r\nx();  \r\n};"),
            "void() f =\r\n{\r\n\tx();\r\n};\r\n"
        );
        assert_eq!(vanilla(""), "");
        assert_eq!(vanilla("\n\n"), "");
        assert_eq!(vanilla("float x;"), "float x;\n");
    }
}
