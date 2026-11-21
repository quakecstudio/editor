// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::{Part, show};
use crate::theme::{Mode, Theme};

/// `text` drawn as Markdown in a panel's width, once the theme's fonts are the window's: the
/// state says whether they are.
fn drawn(text: &'static str) -> Harness<'static, bool> {
    let theme = Theme::load(Mode::Dark).unwrap();
    let fonts = theme.clone();
    let mut harness = Harness::builder().with_size([320.0, 600.0]).build_ui_state(
        move |ui, ready: &mut bool| {
            if *ready {
                show(ui, &theme, Part::Message(0), text);
            }
        },
        false,
    );
    fonts.apply(&harness.ctx);
    harness.run();
    *harness.state_mut() = true;
    harness.run();
    harness
}

/// Whether a label with exactly `text` is drawn, once or more.
fn has<S>(harness: &Harness<'_, S>, text: &str) -> bool {
    harness.query_all_by_label(text).next().is_some()
}

// No spec statement: L-43, the acceptance criterion: a reply with a heading, bold text, a
// list, inline code and a code block is drawn with none of its marks showing
#[test]
fn a_reply_is_drawn_without_its_marks() {
    let harness = drawn(
        "## What I built\n\n**One fix along the way:** both lights went to `600`.\n\n\
         - a *soldier*\n- ~~a dog~~ an ogre\n\n1. compile\n2. play\n\n\
         ```\nmap.save\n```\n\n---\n\nSee [the guide](https://example.com/guide).\n",
    );
    for text in [
        "What I built",
        "One fix along the way: both lights went to 600.",
        "a soldier",
        "a dog an ogre",
        "compile",
        "play",
        "map.save",
        "See the guide (https://example.com/guide).",
        "\u{2022}",
        "1.",
        "2.",
    ] {
        assert!(has(&harness, text), "{text:?} is drawn");
    }
    for mark in ["##", "**", "```", "---", "~~"] {
        assert!(
            harness.query_all_by_label_contains(mark).next().is_none(),
            "{mark:?} is not drawn"
        );
    }
}

// No spec statement: L-43: a table's cells are drawn each by itself, its rule of dashes not
// at all; a task list's boxes are drawn as they are typed, which every bundled font can
// draw, and a link whose text is its address is drawn once
#[test]
fn tables_tasks_and_bare_links_are_drawn() {
    let harness = drawn(
        "| Mod | Builds |\n| --- | --- |\n| id 1.01 | yes |\n\n\
         - [x] sealed\n- [ ] lit\n\n<https://example.com>\n",
    );
    for text in [
        "Mod",
        "Builds",
        "id 1.01",
        "yes",
        "[x] sealed",
        "[ ] lit",
        "https://example.com",
    ] {
        assert!(has(&harness, text), "{text:?} is drawn");
    }
    assert!(harness.query_by_label_contains("---").is_none());
}

// No spec statement: L-43: a reply still arriving is drawn as far as it reads: a code block
// not yet closed shows its lines, bold not yet closed its text with the marks it has, and
// nothing at all draws nothing
#[test]
fn a_reply_still_arriving_is_drawn_as_far_as_it_reads() {
    let harness = drawn("Building the room\n\n```\nmap.brush_create");
    assert!(has(&harness, "Building the room"));
    assert!(has(&harness, "map.brush_create"));
    let harness = drawn("The **first");
    assert!(has(&harness, "The **first"));
    let harness = drawn("");
    assert!(harness.query_by_label_contains("a").is_none());
}

// No spec statement: L-43: a list inside a list and a quote are each drawn, the outer item
// before the inner ones
#[test]
fn nested_lists_and_quotes_are_drawn() {
    let harness = drawn("- rooms\n  - west\n  - east\n\n> as the map says\n");
    for text in ["rooms", "west", "east", "as the map says"] {
        assert!(has(&harness, text), "{text:?} is drawn");
    }
}

// No spec statement: L-43 (the review's findings): an item's text ends where a heading in it
// begins; a link around an image shows both addresses; and a code block of a list item is
// drawn, set in under the item
#[test]
fn blocks_in_items_and_links_in_links_are_drawn() {
    let harness = drawn(
        "- foo\n  # bar\n\n1. Run this:\n\n   ```\n   make ci\n   ```\n2. Push\n\n\
         [![build](badge.svg)](https://ci.example/run)\n",
    );
    for text in [
        "foo",
        "bar",
        "Run this:",
        "make ci",
        "Push",
        "build (badge.svg) (https://ci.example/run)",
    ] {
        assert!(has(&harness, text), "{text:?} is drawn");
    }
    assert!(!has(&harness, "foobar"));
    let left = |text: &str| harness.get_by_label(text).rect().left();
    assert!(
        left("make ci") > left("1."),
        "the code is set in past the number"
    );
}

// No spec statement: L-43 (the review's findings): two replies drawn one after the other keep
// their code blocks apart: each is drawn, under its own id
#[test]
fn two_replies_keep_their_blocks_apart() {
    let theme = Theme::load(Mode::Dark).unwrap();
    let fonts = theme.clone();
    let mut harness = Harness::builder().with_size([320.0, 600.0]).build_ui_state(
        move |ui, ready: &mut bool| {
            if *ready {
                for (place, text) in ["one\n\n```\nfirst\n```\n", "two\n\n```\nsecond\n```\n"]
                    .into_iter()
                    .enumerate()
                {
                    show(ui, &theme, Part::Message(place), text);
                }
            }
        },
        false,
    );
    fonts.apply(&harness.ctx);
    harness.run();
    *harness.state_mut() = true;
    harness.run();
    assert!(has(&harness, "first") && has(&harness, "second"));
    let top = |text: &str| harness.get_by_label(text).rect().top();
    assert!(top("second") > top("first"));
}

// No spec statement: L-43 (the review's findings): a text scrolled out of sight is not laid
// out again: once it has been drawn, its room is kept and nothing of it is drawn, so a long
// thread costs a frame what shows of it; a text that changed, as one still arriving does, is
// drawn again
#[test]
fn a_text_out_of_sight_keeps_its_room_and_is_not_drawn() {
    let theme = Theme::load(Mode::Dark).unwrap();
    let fonts = theme.clone();
    // What is drawn, and how far the thread is scrolled.
    let mut harness = Harness::builder().with_size([320.0, 200.0]).build_ui_state(
        move |ui, (text, offset): &mut (&'static str, f32)| {
            if text.is_empty() {
                return;
            }
            eframe::egui::ScrollArea::vertical()
                .vertical_scroll_offset(*offset)
                .show(ui, |ui| {
                    show(ui, &theme, Part::Message(0), text);
                    for place in 1..40 {
                        show(ui, &theme, Part::Message(place), "filler");
                    }
                });
        },
        ("", 0.0),
    );
    fonts.apply(&harness.ctx);
    harness.run();
    harness.state_mut().0 = "the first reply";
    harness.run();
    assert!(has(&harness, "the first reply"));
    // Scrolled past: its room is kept, so the rest stays where it was, and it is not drawn.
    harness.state_mut().1 = 400.0;
    harness.run();
    assert!(!has(&harness, "the first reply"));
    assert!(has(&harness, "filler"));
    // Back in sight, and changed: drawn again as it now reads.
    *harness.state_mut() = ("the first reply, longer", 0.0);
    harness.run();
    assert!(has(&harness, "the first reply, longer"));
}
