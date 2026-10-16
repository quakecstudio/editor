// SPDX-License-Identifier: GPL-3.0-or-later
//! What the editor exposes to assistive technology: one AccessKit text run per visible row and
//! the caret as a (row, character) position. Each prototype turns this into AccessKit nodes
//! through its framework's API.

use crate::document::{Cursor, Row};

/// One visible row as an AccessKit `TextRun`: its text and each character's UTF-8 length.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRun {
    pub value: String,
    pub character_lengths: Vec<u8>,
}

/// The text runs for `rows`, and the caret as (run index, character index) when the primary
/// cursor is on a visible row.
#[must_use]
pub fn text_runs(rows: &[Row], primary: Cursor) -> (Vec<TextRun>, Option<(usize, usize)>) {
    let mut runs = Vec::with_capacity(rows.len());
    let mut caret = None;
    for (index, row) in rows.iter().enumerate() {
        let mut value = row.text.clone();
        value.push('\n');
        let character_lengths = value
            .chars()
            .map(|c| u8::try_from(c.len_utf8()).unwrap_or(4))
            .collect();
        if row.line == Some(primary.line) {
            let chars = row.text.get(..primary.col).map_or(0, |s| s.chars().count());
            caret = Some((index, chars));
        }
        runs.push(TextRun {
            value,
            character_lengths,
        });
    }
    (runs, caret)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    #[test]
    fn runs_end_in_newlines_and_the_caret_counts_characters() {
        let mut doc = Document::new("a\nnihon 日本 x");
        doc.cursors = vec![Cursor { line: 1, col: 12 }];
        let rows = doc.rows(0, 2);
        let (runs, caret) = text_runs(&rows, doc.primary());
        assert_eq!(runs[0].value, "a\n");
        assert_eq!(
            runs[1]
                .character_lengths
                .iter()
                .map(|l| usize::from(*l))
                .sum::<usize>(),
            runs[1].value.len()
        );
        assert_eq!(caret, Some((1, 8)));
    }
}
