// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    reason = "test code: a failed index is a failed test"
)]

use std::path::{Path, PathBuf};

use super::{Excerpt, Multibuffer, Source};
use crate::strings::Truncation;

const FILE: &[u8] = b"one\ntwo\nthree health\nfour\nfive\nsix health\nseven\n";

// No spec statement: P4-33: an excerpt is its place's whole lines with a line of context
// above and below, and Open goes to the place until the caret has been in it
#[test]
fn an_excerpt_is_whole_lines_with_context() {
    let at = FILE
        .windows(6)
        .position(|w| w == b"health")
        .unwrap_or_default();
    let excerpt = Excerpt::around(Path::new("a.qc"), FILE, (at, at + 6));
    assert_eq!(excerpt.text, "two\nthree health\nfour");
    assert_eq!(excerpt.line, 2);
    assert_eq!(excerpt.open_at(), at);
    let first = Excerpt::around(Path::new("a.qc"), FILE, (0, 3));
    assert_eq!((first.text.as_str(), first.line), ("one\ntwo", 1));
}

// No spec statement: P4-33: the excerpts of one file that touch are joined, and an edit in
// one moves the excerpts after it in its file by the difference
#[test]
fn excerpts_join_and_move_with_edits() {
    let a = PathBuf::from("a.qc");
    let b = PathBuf::from("b.qc");
    let mut buffer = Multibuffer::default();
    buffer.show(
        Source::Search,
        &[
            (a.clone(), 0, 3),
            (a.clone(), 4, 7),
            (a.clone(), 35, 41),
            (b.clone(), 0, 3),
        ],
        Truncation::All,
        |_| Some(FILE.to_vec()),
    );
    assert_eq!(buffer.excerpts.len(), 3, "{:?}", buffer.excerpts);
    assert_eq!(buffer.excerpts[0].text, "one\ntwo\nthree health");
    assert_eq!(buffer.excerpts[1].text, "five\nsix health\nseven");
    assert_eq!(buffer.files(), [a.clone(), b]);
    let second = buffer.excerpts[1].start;
    buffer.edited(super::ExcerptIndex::new(0), "1\n2\n3".to_owned());
    assert_eq!(buffer.excerpts[0].end, 5);
    assert_eq!(buffer.excerpts[1].start, second - 15);
    assert_eq!(buffer.excerpts[2].start, 0, "another file's excerpt stays");
}
