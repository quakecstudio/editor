// SPDX-License-Identifier: GPL-3.0-or-later

//! The acceptance criterion's table (P4-37): each case a text, a caret, the keys typed and
//! the text and caret Vim leaves, taken from Vim's help at
//! vim/vim@81ebe329b38e0265f11b62b3fe073a94b5d2eafa (`motion.txt`, `change.txt`).

use super::{Effect, Mode, Vim, VimKey};
use crate::{Buffer, Position};

/// The keys of `typed`: each character, with `<Esc>`, `<CR>` and `<BS>` for those keys.
fn keys(typed: &str) -> Vec<VimKey> {
    let mut out = Vec::new();
    let mut rest = typed;
    while let Some(c) = rest.chars().next() {
        let (key, len) = if rest.starts_with("<Esc>") {
            (VimKey::Escape, 5)
        } else if rest.starts_with("<CR>") {
            (VimKey::Enter, 4)
        } else if rest.starts_with("<BS>") {
            (VimKey::Backspace, 4)
        } else {
            (VimKey::Char(c), c.len_utf8())
        };
        out.push(key);
        rest = rest.get(len..).unwrap_or_default();
    }
    out
}

/// `typed` at `caret` in `text`: the text and caret after, and the last effect.
fn run(text: &str, caret: (usize, usize), typed: &str) -> (String, Position, Effect) {
    let mut buffer = Buffer::new(text);
    let mut at = Position::new(caret.0, caret.1);
    let mut vim = Vim::new();
    let mut effect = Effect::Done;
    for key in keys(typed) {
        effect = vim.key(&mut buffer, &mut at, key);
    }
    (buffer.text(), at, effect)
}

/// What the help says, the text, the caret, the keys, the text after and the caret after.
type Case = (
    &'static str,
    &'static str,
    (usize, usize),
    &'static str,
    &'static str,
    (usize, usize),
);

/// The table.
const CASES: &[Case] = &[
    // motion.txt `w`: "[count] words forward."
    ("w", "hello world", (0, 0), "w", "hello world", (0, 6)),
    ("3w", "a b c d", (0, 0), "3w", "a b c d", (0, 6)),
    // motion.txt `*word*`: letters, digits and underscores, or other non-blank
    // characters, are separate words.
    (
        "w over punctuation",
        "foo.bar baz",
        (0, 0),
        "w",
        "foo.bar baz",
        (0, 3),
    ),
    // "An empty line is also considered to be a word."
    (
        "w to an empty line",
        "one\n\ntwo",
        (0, 0),
        "w",
        "one\n\ntwo",
        (1, 0),
    ),
    (
        "w from an empty line",
        "one\n\ntwo",
        (1, 0),
        "w",
        "one\n\ntwo",
        (2, 0),
    ),
    // motion.txt `b`: "[count] words backward."
    ("b", "hello world", (0, 6), "b", "hello world", (0, 0)),
    (
        "b mid-word",
        "hello world",
        (0, 8),
        "b",
        "hello world",
        (0, 6),
    ),
    // motion.txt `e`: "Forward to the end of word [count]".
    ("e", "hello world", (0, 0), "e", "hello world", (0, 4)),
    (
        "e at an end",
        "hello world",
        (0, 4),
        "e",
        "hello world",
        (0, 10),
    ),
    // motion.txt `0`: "To the first character of the line."
    ("0", "  indented", (0, 9), "0", "  indented", (0, 0)),
    // motion.txt `^`: to the first non-blank character of the line.
    ("^", "  indented", (0, 9), "^", "  indented", (0, 2)),
    // motion.txt `$`: "To the end of the line."
    ("$", "  indented", (0, 0), "$", "  indented", (0, 9)),
    // motion.txt `h` and `l`: [count] characters left and right, within the line.
    ("3l", "abcdef", (0, 0), "3l", "abcdef", (0, 3)),
    ("h at the start", "abc", (0, 0), "h", "abc", (0, 0)),
    ("l at the end", "abc", (0, 2), "l", "abc", (0, 2)),
    // `h` and `l` with a count, within the line.
    ("2h", "abcd", (0, 3), "2h", "abcd", (0, 1)),
    ("3l", "abcd", (0, 0), "3l", "abcd", (0, 3)),
    // `w`, `b` and `e` past the end of a line, an empty line a word of its own.
    ("w across a line", "ab\ncd", (0, 0), "w", "ab\ncd", (1, 0)),
    ("w from blanks", "a  b", (0, 1), "w", "a  b", (0, 3)),
    (
        "w onto an empty line",
        "ab\n\ncd",
        (0, 0),
        "w",
        "ab\n\ncd",
        (1, 0),
    ),
    ("b across a line", "ab\ncd", (1, 0), "b", "ab\ncd", (0, 0)),
    (
        "b onto an empty line",
        "ab\n\ncd",
        (2, 0),
        "b",
        "ab\n\ncd",
        (1, 0),
    ),
    ("e across a line", "a\nbc", (0, 0), "e", "a\nbc", (1, 1)),
    // motion.txt `j`: [count] lines downward, the column kept.
    ("j", "abcd\nabcd", (0, 2), "j", "abcd\nabcd", (1, 2)),
    ("k clamped", "ab\nabcd", (1, 3), "k", "ab\nabcd", (0, 1)),
    // motion.txt `G`: "Goto line [count], default last line, on the first non-blank".
    (
        "G",
        "one\ntwo\n  three",
        (0, 0),
        "G",
        "one\ntwo\n  three",
        (2, 2),
    ),
    (
        "2G",
        "one\ntwo\nthree",
        (0, 0),
        "2G",
        "one\ntwo\nthree",
        (1, 0),
    ),
    // motion.txt `gg`: "Goto line [count], default first line".
    (
        "gg",
        "one\ntwo\nthree",
        (2, 3),
        "gg",
        "one\ntwo\nthree",
        (0, 0),
    ),
    // change.txt `d{motion}`: delete the text the motion moves over.
    ("dw", "hello world", (0, 0), "dw", "world", (0, 0)),
    ("d$", "hello world", (0, 6), "d$", "hello ", (0, 5)),
    ("de", "hello world", (0, 0), "de", " world", (0, 0)),
    ("db", "hello world", (0, 6), "db", "world", (0, 0)),
    (
        "dw at the last word",
        "one two",
        (0, 4),
        "dw",
        "one ",
        (0, 3),
    ),
    // A count before the operator and one before the motion multiply.
    ("2d2w", "a b c d e f g", (0, 0), "2d2w", "e f g", (0, 0)),
    // change.txt `dd`: delete [count] lines, linewise.
    ("dd", "one\ntwo\nthree", (1, 0), "dd", "one\nthree", (1, 0)),
    ("2dd", "one\ntwo\nthree", (0, 0), "2dd", "three", (0, 0)),
    (
        "dd on the last line",
        "one\ntwo",
        (1, 0),
        "dd",
        "one",
        (0, 0),
    ),
    ("dj", "one\ntwo\nthree", (0, 0), "dj", "three", (0, 0)),
    // change.txt `x`: "Delete [count] characters under and after the cursor".
    ("x", "abc", (0, 1), "x", "ac", (0, 1)),
    ("3x", "abcdef", (0, 0), "3x", "def", (0, 0)),
    ("x at the end", "abc", (0, 2), "x", "ab", (0, 1)),
    // motion.txt, special case: "cw" changes only to the end of the word.
    (
        "cw",
        "hello world",
        (0, 0),
        "cwbye<Esc>",
        "bye world",
        (0, 2),
    ),
    // change.txt `cc`: change [count] lines into one, emptied.
    (
        "cc",
        "one\ntwo\nthree",
        (1, 0),
        "ccnew<Esc>",
        "one\nnew\nthree",
        (1, 2),
    ),
    // change.txt `yy` and `p`: yank lines, put them below.
    ("yyp", "one\ntwo", (0, 0), "yyp", "one\none\ntwo", (1, 0)),
    ("yyP", "one\ntwo", (1, 0), "yyP", "one\ntwo\ntwo", (1, 0)),
    (
        "yy on the last line, p",
        "one\ntwo",
        (1, 0),
        "yyp",
        "one\ntwo\ntwo",
        (2, 0),
    ),
    // `yw` then `p`: characters put after the cursor.
    ("ywp", "ab cd", (0, 0), "yw$p", "ab cdab ", (0, 7)),
    ("3p", "x", (0, 0), "yl3p", "xxxx", (0, 3)),
    // change.txt `"{register}`: a named register keeps what is yanked into it.
    (
        "\"ayy\"ap",
        "one\ntwo",
        (0, 0),
        "\"ayyj\"ap",
        "one\ntwo\none",
        (2, 0),
    ),
    (
        "unnamed after named",
        "one\ntwo",
        (0, 0),
        "\"ayyjp",
        "one\ntwo\none",
        (2, 0),
    ),
    // change.txt, Insert mode: `i` before the cursor, `a` after it, `A` at the end,
    // `I` before the first non-blank, `o` and `O` a new line below and above; <Esc>
    // leaves Insert mode with the cursor one character back.
    ("i", "hello", (0, 0), "ihi <Esc>", "hi hello", (0, 2)),
    ("a", "ac", (0, 0), "ab<Esc>", "abc", (0, 1)),
    ("A", "hello", (0, 0), "A!<Esc>", "hello!", (0, 5)),
    ("I", "  x", (0, 2), "I-<Esc>", "  -x", (0, 2)),
    ("o", "hello", (0, 2), "oline<Esc>", "hello\nline", (1, 3)),
    ("O", "hello", (0, 2), "Oline<Esc>", "line\nhello", (0, 3)),
    (
        "backspace",
        "hello",
        (0, 0),
        "Axy<BS><Esc>",
        "hellox",
        (0, 5),
    ),
    ("enter", "ab", (0, 1), "i<CR><Esc>", "a\nb", (1, 0)),
    // repeat.txt `.`: "Repeat last change, with count replaced with [count]."
    ("dw..", "a b c d", (0, 0), "dw..", "d", (0, 0)),
    ("2x.", "abcdef", (0, 0), "2x.", "ef", (0, 0)),
    ("A.", "ab\ncd", (0, 0), "A!<Esc>j.", "ab!\ncd!", (1, 2)),
    // visual.txt `v`: a characterwise selection an operator then takes.
    ("vlld", "hello world", (0, 0), "vlld", "lo world", (0, 0)),
    (
        "vey",
        "hello world",
        (0, 0),
        "veyP",
        "hellohello world",
        (0, 4),
    ),
    ("v<Esc>", "abc", (0, 0), "vl<Esc>x", "ac", (0, 1)),
];

// No spec statement: P4-37, UI-21, the acceptance criterion: Vim's motions and operators as
// its help gives them
#[test]
fn motions_and_operators_do_what_vims_help_says() {
    for (help, text, caret, typed, want_text, want_caret) in CASES {
        let (got_text, got_caret, _) = run(text, *caret, typed);
        assert_eq!(
            (got_text.as_str(), (got_caret.line, got_caret.col)),
            (*want_text, *want_caret),
            "{help}: {typed:?} on {text:?}"
        );
    }
}

// No spec statement: P4-37: `u` asks the editor to undo; the mode follows i, v and Escape
#[test]
fn undo_and_modes() {
    assert_eq!(run("abc", (0, 0), "u").2, Effect::Undo);
    let mut buffer = Buffer::new("abc");
    let mut at = Position::new(0, 0);
    let mut vim = Vim::new();
    assert_eq!(vim.mode(), Mode::Normal);
    vim.key(&mut buffer, &mut at, VimKey::Char('v'));
    assert_eq!((vim.mode(), vim.anchor()), (Mode::Visual, Some(at)));
    vim.key(&mut buffer, &mut at, VimKey::Escape);
    vim.key(&mut buffer, &mut at, VimKey::Char('i'));
    assert_eq!((vim.mode(), vim.anchor()), (Mode::Insert, None));
    vim.key(&mut buffer, &mut at, VimKey::Escape);
    assert_eq!(vim.mode(), Mode::Normal);
}
