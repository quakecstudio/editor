// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's colours (UI-4): keywords, literals and comments from the file as written, and
//! names from the semantic model, in the `[syntax]` colours of `docs/design/tokens.toml`. A
//! builtin is a function drawn in italic, a constant is a global the model marks read-only.
//!
//! The spans are byte ranges of the open file's text as the workbench last checked it, split
//! into lines; between an edit and the next check they may lag the editor's text by a moment,
//! as a language server's semantic tokens do.

use eframe::egui::Color32;
use qs_editor_view::{StyledSpan, Styler};
use qs_workbench::{Highlight, HighlightKind, Modifier, OpenFile, SyntaxClass};

use crate::theme::{Theme, Token};

/// The colours of the open file's lines.
#[derive(Debug, Default)]
pub(crate) struct CodeStyler {
    /// The open file's revision the lines were made from.
    revision: u64,
    /// Each line's spans, in order and not overlapping.
    lines: Vec<Vec<StyledSpan>>,
}

impl CodeStyler {
    /// The colours of `open` in `theme`.
    pub(crate) fn new(open: &OpenFile, theme: &Theme) -> Self {
        let mut spans: Vec<(usize, usize, Color32, bool)> = Vec::new();
        for (span, class) in open.classes() {
            let (token, italic) = match class {
                SyntaxClass::Keyword => (Token::CodeKeyword, false),
                SyntaxClass::Type => (Token::CodeType, false),
                SyntaxClass::Number => (Token::CodeConstant, false),
                SyntaxClass::String => (Token::CodeString, false),
                SyntaxClass::Comment => (Token::CodeComment, true),
            };
            spans.push((span.start, span.end, theme.color(token), italic));
        }
        for highlight in open.highlights() {
            if let Some((token, italic)) = name_colour(highlight) {
                let span = highlight.span;
                spans.push((span.start, span.end, theme.color(token), italic));
            }
        }
        spans.sort_by_key(|&(start, ..)| start);
        let text = open.text();
        let starts = line_starts(text);
        let mut lines: Vec<Vec<StyledSpan>> = vec![Vec::new(); starts.len()];
        for (start, end, color, italic) in spans {
            let first = starts.partition_point(|&s| s <= start).saturating_sub(1);
            for (line, &line_start) in starts.iter().enumerate().skip(first) {
                if line_start >= end {
                    break;
                }
                let line_end = starts
                    .get(line.saturating_add(1))
                    .map_or(text.len(), |next| next.saturating_sub(1));
                let from = start.max(line_start).saturating_sub(line_start);
                let to = end.min(line_end).saturating_sub(line_start);
                if let Some(spans) = lines.get_mut(line)
                    && to > from
                    && spans.last().is_none_or(|last| last.range.end <= from)
                {
                    spans.push(StyledSpan {
                        range: from..to,
                        color,
                        italic,
                    });
                }
            }
        }
        Self {
            revision: open.revision(),
            lines,
        }
    }

    /// Whether these are the colours of `open` as it is now.
    pub(crate) const fn is_current(&self, open: &OpenFile) -> bool {
        self.revision == open.revision()
    }
}

impl Styler for CodeStyler {
    fn revision(&self) -> u64 {
        self.revision
    }

    fn spans(&self, line: usize, _text: &str) -> Vec<StyledSpan> {
        self.lines.get(line).cloned().unwrap_or_default()
    }
}

/// The colour token of a name, and whether it is italic; `None` for a name drawn as plain
/// code: a variable, a parameter or a local.
fn name_colour(highlight: &Highlight) -> Option<(Token, bool)> {
    let modifiers = highlight.modifiers;
    match highlight.kind {
        HighlightKind::Function => Some((
            Token::CodeFunction,
            modifiers.contains(Modifier::DefaultLibrary),
        )),
        HighlightKind::Field => Some((Token::CodeType, false)),
        HighlightKind::Global if modifiers.contains(Modifier::Readonly) => {
            Some((Token::CodeConstant, false))
        }
        HighlightKind::Global | HighlightKind::Parameter | HighlightKind::Local => None,
    }
}

/// The byte offset each line of `text` starts at; only LF ends a line (QC-1.2).
fn line_starts(text: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    for (offset, &b) in text.iter().enumerate() {
        if b == b'\n' {
            starts.push(offset.saturating_add(1));
        }
    }
    starts
}

#[cfg(test)]
mod tests;
