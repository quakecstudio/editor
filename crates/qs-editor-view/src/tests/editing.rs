// SPDX-License-Identifier: GPL-3.0-or-later

//! Undo and redo, the text's revision, byte offsets, find and replace, and the hovered
//! position.

use egui::{Event, Key, Modifiers, Pos2};

use super::{Harness, pos};
use crate::find::{Case, Query};
use crate::{Buffer, EditState, Plain, Selection};

fn query(text: &str, case: Case) -> Query {
    Query {
        text: text.to_owned(),
        case,
    }
}

#[test]
fn typing_is_one_undo_step_until_the_caret_moves_or_a_line_breaks() {
    let mut state = EditState::new("ab");
    state.set_selections(&[Selection::caret(pos(0, 2))]);
    let start = state.revision();
    state.insert("c");
    state.insert("d");
    assert_eq!(state.buffer().text(), "abcd");
    assert!(state.revision() > start);
    state.newline();
    state.insert("e");
    assert_eq!(state.buffer().text(), "abcd\ne");
    assert!(state.undo());
    assert_eq!(
        state.buffer().text(),
        "abcd\n",
        "the typing after the break"
    );
    assert!(state.undo());
    assert_eq!(state.buffer().text(), "abcd", "the break");
    assert!(state.undo());
    assert_eq!(
        state.buffer().text(),
        "ab",
        "the run of typing, in one step"
    );
    assert_eq!(state.primary().head, pos(0, 2), "the caret goes back too");
    assert!(!state.undo(), "nothing more");
    assert!(state.redo());
    assert_eq!(state.buffer().text(), "abcd");
    assert_eq!(state.primary().head, pos(0, 4));
    assert!(state.redo());
    assert!(state.redo());
    assert_eq!(state.buffer().text(), "abcd\ne");
    assert!(!state.redo());
}

#[test]
fn text_with_a_line_break_is_a_step_of_its_own() {
    let mut state = EditState::new("");
    state.insert("a");
    state.insert("b\nc");
    assert!(state.undo());
    assert_eq!(
        state.buffer().text(),
        "a",
        "the pasted lines, not the typing before them"
    );
    // Typing over a selection is a step of its own too.
    state.insert("xy");
    state.set_selections(&[Selection {
        anchor: pos(0, 1),
        head: pos(0, 3),
    }]);
    state.insert("z");
    assert!(state.undo());
    assert_eq!(state.buffer().text(), "axy");
}

#[test]
fn a_moved_caret_or_a_new_edit_starts_a_new_step_and_drops_the_redo() {
    let mut state = EditState::new("");
    state.insert("ab");
    state.set_selections(&[Selection::caret(pos(0, 0))]);
    state.insert("x");
    assert_eq!(state.buffer().text(), "xab");
    assert!(state.undo());
    assert_eq!(
        state.buffer().text(),
        "ab",
        "only the typing at the new place"
    );
    state.insert("y");
    assert!(!state.redo(), "an edit drops what was undone");
    // A redone step is not joined by the typing after it.
    let mut state = EditState::new("");
    state.insert("a");
    assert!(state.undo());
    assert!(state.redo());
    state.insert("b");
    assert!(state.undo());
    assert_eq!(state.buffer().text(), "a");
}

#[test]
fn every_kind_of_edit_undoes_across_carets() {
    let mut state = EditState::new("one\ntwo\nthree");
    state.set_selections(&[
        Selection::caret(pos(0, 3)),
        Selection::caret(pos(1, 3)),
        Selection::caret(pos(2, 5)),
    ]);
    state.backspace();
    assert_eq!(state.buffer().text(), "on\ntw\nthre");
    state.insert("!\n");
    assert_eq!(state.buffer().text(), "on!\n\ntw!\n\nthre!\n");
    assert!(state.undo());
    assert!(state.undo());
    assert_eq!(state.buffer().text(), "one\ntwo\nthree");
    assert_eq!(state.selections().len(), 3);
    // A delete and a backspace that change nothing are not steps.
    let mut state = EditState::new("a");
    let revision = state.revision();
    state.backspace();
    assert_eq!(state.revision(), revision);
    assert!(!state.undo());
    state.set_selections(&[Selection::caret(pos(0, 1))]);
    state.delete();
    assert!(!state.undo());
}

#[test]
fn replacing_the_text_keeps_the_carets_that_fit_and_forgets_the_history() {
    let mut state = EditState::new("abc\ndef");
    state.insert("x");
    state.set_selections(&[Selection::caret(pos(1, 3))]);
    let revision = state.revision();
    state.replace_text("q");
    assert_eq!(state.buffer().text(), "q");
    assert_eq!(state.primary().head, pos(0, 1));
    assert!(state.revision() > revision);
    assert!(!state.undo());
}

#[test]
fn offsets_and_positions_convert_both_ways() {
    let buffer = Buffer::new("ab\r\n\u{e9}c\n");
    assert_eq!(buffer.text(), "ab\n\u{e9}c\n");
    assert_eq!(buffer.offset(pos(0, 0)), 0);
    assert_eq!(buffer.offset(pos(1, 0)), 3);
    assert_eq!(buffer.offset(pos(1, 2)), 5);
    assert_eq!(buffer.offset(pos(2, 0)), 7);
    assert_eq!(buffer.offset(pos(9, 9)), 7, "clamped to the end");
    assert_eq!(buffer.position(0), pos(0, 0));
    assert_eq!(buffer.position(2), pos(0, 2));
    assert_eq!(buffer.position(3), pos(1, 0));
    assert_eq!(
        buffer.position(4),
        pos(1, 0),
        "inside a character, down to its start"
    );
    assert_eq!(buffer.position(6), pos(1, 3));
    assert_eq!(buffer.position(7), pos(2, 0));
    assert_eq!(buffer.position(99), pos(2, 0), "past the end is the end");
}

#[test]
fn a_query_matches_whole_occurrences_on_each_line() {
    let buffer = Buffer::new("Self self\nselfself\nx");
    let ignore = query("self", Case::Ignore).matches(&buffer);
    let starts: Vec<_> = ignore.iter().map(Selection::start).collect();
    assert_eq!(starts, [pos(0, 0), pos(0, 5), pos(1, 0), pos(1, 4)]);
    assert_eq!(ignore[0].end(), pos(0, 4));
    let exact = query("Self", Case::Match).matches(&buffer);
    assert_eq!(exact.len(), 1);
    assert!(query("", Case::Ignore).matches(&buffer).is_empty());
    assert!(query("f\ns", Case::Ignore).matches(&buffer).is_empty());
    assert!(query("zz", Case::Ignore).matches(&buffer).is_empty());
    // A line whose lowercase is longer than itself is skipped rather than mismatched.
    let dotted = Buffer::new("\u{130}x self");
    assert!(query("self", Case::Ignore).matches(&dotted).is_empty());
    assert_eq!(query("self", Case::Match).matches(&dotted).len(), 1);
}

#[test]
fn find_next_and_previous_wrap_around() {
    let mut state = EditState::new("a b a b a");
    let a = query("a", Case::Match);
    assert!(state.find_next(&a));
    assert_eq!(state.primary().start(), pos(0, 0));
    assert!(state.find_next(&a));
    assert_eq!(state.primary().start(), pos(0, 4));
    assert!(state.find_next(&a));
    assert!(state.find_next(&a));
    assert_eq!(state.primary().start(), pos(0, 0), "wraps to the start");
    assert!(state.find_previous(&a));
    assert_eq!(state.primary().start(), pos(0, 8), "wraps to the end");
    assert!(state.find_previous(&a));
    assert_eq!(state.primary().start(), pos(0, 4));
    assert!(!state.find_next(&query("z", Case::Match)));
    assert!(!state.find_previous(&query("z", Case::Match)));
}

#[test]
fn replace_next_replaces_the_selected_match_and_moves_on() {
    let mut state = EditState::new("a b a");
    let a = query("a", Case::Match);
    assert!(state.replace_next(&a, "x"), "the first press only finds");
    assert_eq!(state.buffer().text(), "a b a");
    assert!(state.replace_next(&a, "x"));
    assert_eq!(state.buffer().text(), "x b a");
    assert_eq!(state.primary().start(), pos(0, 4));
    assert!(!state.replace_next(&a, "x"));
    assert_eq!(state.buffer().text(), "x b x");
    assert!(state.undo());
    assert_eq!(
        state.buffer().text(),
        "x b a",
        "each replace is its own step"
    );
}

#[test]
fn replace_all_is_one_step() {
    let mut state = EditState::new("aa\na");
    let a = query("a", Case::Match);
    assert_eq!(state.replace_all(&a, "bc"), 3);
    assert_eq!(state.buffer().text(), "bcbc\nbc");
    assert_eq!(state.replace_all(&query("z", Case::Match), "q"), 0);
    assert!(state.undo());
    assert_eq!(state.buffer().text(), "aa\na");
}

#[test]
fn the_undo_keys_undo_and_redo() {
    let mut harness = Harness::new("ab");
    harness.frame(vec![Event::Text("c".to_owned())], &Plain);
    assert_eq!(harness.view.state().buffer().text(), "cab");
    harness.key(Key::Z, Modifiers::COMMAND);
    assert_eq!(harness.view.state().buffer().text(), "ab");
    harness.key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(harness.view.state().buffer().text(), "cab");
    harness.key(Key::Z, Modifiers::NONE);
    assert_eq!(
        harness.view.state().buffer().text(),
        "cab",
        "Z alone is a letter, typed as text, not undo"
    );
}

#[test]
fn the_hovered_position_follows_the_pointer_over_the_text() {
    let mut harness = Harness::new("hello\nworld");
    assert_eq!(harness.view.hovered(), None);
    harness.frame(vec![Event::PointerMoved(Pos2::new(400.0, 30.0))], &Plain);
    let over_text = harness.view.hovered();
    assert!(over_text.is_some_and(|p| p.line <= 1), "{over_text:?}");
    // The panel starts at x = 8; the numbers fill the gutter after it.
    harness.frame(vec![Event::PointerMoved(Pos2::new(12.0, 10.0))], &Plain);
    assert_eq!(harness.view.hovered(), None, "over the line numbers");
}
