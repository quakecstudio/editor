// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::show;
use crate::theme::{Mode, Theme};

/// `text` drawn as Markdown in a panel's width, once the theme's fonts are the window's: the
/// state says whether they are.
fn drawn(text: &'static str) -> Harness<'static, bool> {
    let theme = Theme::load(Mode::Dark).unwrap();
    let fonts = theme.clone();
    let mut harness = Harness::builder().with_size([320.0, 600.0]).build_ui_state(
        move |ui, ready: &mut bool| {
            if *ready {
                show(ui, &theme, text);
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
fn has(harness: &Harness<'_, bool>, text: &str) -> bool {
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
// at all; a task list's boxes are marks, and a link whose text is its address is drawn once
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
        "\u{2611} sealed",
        "\u{2610} lit",
        "https://example.com",
    ] {
        assert!(has(&harness, text), "{text:?} is drawn");
    }
    assert!(harness.query_by_label_contains("---").is_none());
    assert!(harness.query_by_label_contains("[x]").is_none());
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
