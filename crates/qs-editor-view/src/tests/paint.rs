// SPDX-License-Identifier: GPL-3.0-or-later

//! What the view paints, read from egui's shapes: the gutter, the lines, the current line, the
//! selections and the carets, in units of the code font's character width and the row height.

use egui::epaint::{RectShape, Shape, TextShape};
use egui::{Color32, Event, FullOutput, Key, Modifiers, Rect};

use super::{Harness, numbered, pos};
use crate::Selection;
use crate::fold::FoldRegion;
use crate::layout::Plain;
use crate::view::Wrap;

/// Every rectangle and text egui was asked to paint, nested shapes flattened.
fn shapes(output: &FullOutput) -> (Vec<RectShape>, Vec<TextShape>) {
    fn walk(shape: &Shape, rects: &mut Vec<RectShape>, texts: &mut Vec<TextShape>) {
        match shape {
            Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, rects, texts);
                }
            }
            Shape::Rect(rect) => rects.push(rect.clone()),
            Shape::Text(text) => texts.push(text.clone()),
            _ => {}
        }
    }
    let (mut rects, mut texts) = (Vec::new(), Vec::new());
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut rects, &mut texts);
    }
    (rects, texts)
}

/// The rectangles filled with `color`, top to bottom, left to right.
fn filled(output: &FullOutput, color: Color32) -> Vec<Rect> {
    let mut rects: Vec<Rect> = shapes(output)
        .0
        .into_iter()
        .filter(|r| r.fill == color)
        .map(|r| r.rect)
        .collect();
    rects.sort_by(|a, b| {
        (a.top(), a.left())
            .partial_cmp(&(b.top(), b.left()))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rects
}

/// The painted text `text`'s shape.
fn text_shape(output: &FullOutput, text: &str) -> TextShape {
    shapes(output)
        .1
        .into_iter()
        .find(|t| t.galley.text() == text)
        .unwrap_or_else(|| panic!("no text {text:?}"))
}

/// The view's measures in a frame: its rectangle, where text starts, a character's width and a
/// row's height.
struct Measures {
    view: Rect,
    text_left: f32,
    char_width: f32,
    row: f32,
}

fn measures(harness: &Harness, output: &FullOutput) -> Measures {
    let view = filled(output, harness.style.background)[0];
    let current = filled(output, harness.style.current_line)[0];
    let char_width = text_shape(output, "1").galley.size().x;
    let first = harness
        .view
        .state()
        .buffer()
        .line(harness.view.first_line());
    let text_left = text_shape(output, first).pos.x;
    Measures {
        view,
        text_left,
        char_width,
        row: current.height(),
    }
}

/// Whether two positions are the same, within the point egui's rounding of painted text and
/// glyph positions to the pixel grid moves them; a wrong column is a whole character off.
fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0
}

fn at(m: &Measures, col: f32) -> f32 {
    col.mul_add(m.char_width, m.text_left)
}

#[test]
fn the_gutter_holds_right_aligned_numbers_before_the_text() {
    let mut harness = Harness::new(&numbered(5));
    let output = harness.frame(Vec::new(), &Plain);
    let m = measures(&harness, &output);
    assert!(
        near(m.text_left - m.view.left(), 24.0 + 2.0 * m.char_width),
        "two digits at least, between two 12-point pads"
    );
    let one = text_shape(&output, "1");
    assert!(
        near(one.pos.x + one.galley.size().x, m.text_left - 12.0),
        "right-aligned"
    );
    let five = text_shape(&output, "5");
    assert!(
        near(five.pos.y - one.pos.y, 4.0 * m.row),
        "one row per line"
    );
    assert!(near(text_shape(&output, "line 4").pos.y, five.pos.y));

    let mut wide = Harness::new(&numbered(120));
    let output = wide.frame(Vec::new(), &Plain);
    let m = measures(&wide, &output);
    assert!(
        near(m.text_left - m.view.left(), 24.0 + 3.0 * m.char_width),
        "three digits for 120 lines"
    );
    wide.view.scroll_to(7);
    let output = wide.frame(Vec::new(), &Plain);
    let eight = text_shape(&output, "8");
    let view = filled(&output, wide.style.background)[0];
    assert!(
        eight.pos.y - view.top() < m.row,
        "line 8 is the first row when scrolled to it"
    );
}

#[test]
fn the_current_line_and_the_carets_are_painted_where_the_carets_are() {
    let mut harness = Harness::new("abcdef\nabcdef\nabcdef");
    harness
        .view
        .state_mut()
        .set_selections(&[Selection::caret(pos(1, 3)), Selection::caret(pos(2, 5))]);
    let output = harness.frame(Vec::new(), &Plain);
    let m = measures(&harness, &output);
    let current = filled(&output, harness.style.current_line);
    assert_eq!(current.len(), 1, "only the primary caret's line");
    assert!(near(current[0].top(), m.row.mul_add(1.0, m.view.top())));
    assert!(near(current[0].width(), m.view.width()));
    let carets = filled(&output, harness.style.caret);
    assert_eq!(carets.len(), 2, "one per caret");
    assert!(near(carets[0].left(), at(&m, 3.0)));
    assert!(near(
        carets[0].top(),
        m.row.mul_add(1.0, m.view.top()) + 1.0
    ));
    assert!(near(carets[0].height(), m.row - 2.0));
    assert!(near(carets[0].width(), 2.0));
    assert!(near(carets[1].left(), at(&m, 5.0)));
    assert!(near(
        carets[1].top(),
        m.row.mul_add(2.0, m.view.top()) + 1.0
    ));

    let id = harness.view.id();
    harness.ctx.memory_mut(|mem| mem.surrender_focus(id));
    let output = harness.frame(Vec::new(), &Plain);
    assert!(
        filled(&output, harness.style.caret).is_empty(),
        "no caret without focus"
    );
}

#[test]
fn a_selection_is_a_band_on_each_line_it_covers() {
    let mut harness = Harness::new("abcdef\n\nabcdef");
    harness.view.state_mut().set_selections(&[Selection {
        anchor: pos(0, 2),
        head: pos(2, 3),
    }]);
    let output = harness.frame(Vec::new(), &Plain);
    let m = measures(&harness, &output);
    let bands = filled(&output, harness.style.selection);
    assert_eq!(bands.len(), 3);
    assert!(near(bands[0].left(), at(&m, 2.0)));
    assert!(
        near(bands[0].right(), at(&m, 7.0)),
        "to the line's end and one character for its line break"
    );
    assert!(near(bands[0].top(), m.view.top()));
    assert!(near(bands[0].height(), m.row));
    assert!(near(bands[1].left(), m.text_left));
    assert!(
        near(bands[1].right(), m.text_left + 2.0),
        "an empty line's break is a caret wide"
    );
    assert!(near(bands[2].left(), m.text_left));
    assert!(
        near(bands[2].right(), at(&m, 3.0)),
        "the last line to the head"
    );
    assert!(near(bands[2].top(), m.row.mul_add(2.0, m.view.top())));
}

#[test]
fn the_carets_move_past_the_composition_on_its_line() {
    let mut harness = Harness::new("abcdef\nabcdef");
    harness.view.state_mut().set_selections(&[
        Selection::caret(pos(0, 1)),
        Selection::caret(pos(0, 3)),
        Selection::caret(pos(1, 3)),
    ]);
    let preedit = Event::Ime(egui::ImeEvent::Preedit {
        text: "xy".to_owned(),
        active_range_chars: None,
    });
    let output = harness.frame(vec![preedit], &Plain);
    let first_line = harness.view.state().buffer().line(0).to_owned();
    let composed = format!(
        "{}xy{}",
        first_line.get(..1).unwrap_or(""),
        first_line.get(1..).unwrap_or("")
    );
    let m = Measures {
        text_left: text_shape(&output, &composed).pos.x,
        ..measures(&harness, &output)
    };
    let carets = filled(&output, harness.style.caret);
    assert_eq!(carets.len(), 3);
    assert!(
        near(carets[0].left(), at(&m, 3.0)),
        "the primary caret after the composition"
    );
    assert!(
        near(carets[1].left(), at(&m, 5.0)),
        "a caret after it on its line moves too"
    );
    assert!(
        near(carets[2].left(), at(&m, 3.0)),
        "a caret on another line does not"
    );
    let ime = output.platform_output.ime.unwrap();
    assert!(near(ime.cursor_rect.left(), carets[0].left()));
    harness.key(Key::Escape, Modifiers::NONE);
}

#[test]
fn a_line_outside_the_selection_has_no_band_and_text_sits_mid_row() {
    let mut harness = Harness::new("abc\nabc\nabc\nabc");
    harness.view.state_mut().set_selections(&[Selection {
        anchor: pos(1, 1),
        head: pos(1, 2),
    }]);
    let output = harness.frame(Vec::new(), &Plain);
    let m = measures(&harness, &output);
    let bands = filled(&output, harness.style.selection);
    assert_eq!(bands.len(), 1, "only the selected line");
    assert!(near(bands[0].top(), m.row.mul_add(1.0, m.view.top())));
    let line = text_shape(&output, "abc");
    let offset = line.pos.y - m.view.top();
    let centred = (m.row - line.galley.size().y) / 2.0;
    assert!(
        (offset - centred).abs() < 0.01,
        "{offset} is {centred} into the row"
    );
}

#[test]
fn a_click_focuses_the_view_and_places_the_caret_at_its_column() {
    let mut harness = Harness::new("abcdef\nabcdef");
    let output = harness.frame(Vec::new(), &Plain);
    let m = measures(&harness, &output);
    let id = harness.view.id();
    harness.ctx.memory_mut(|mem| mem.surrender_focus(id));
    harness.frame(Vec::new(), &Plain);
    let click = |x: f32, y: f32| {
        let p = egui::Pos2::new(x, y);
        vec![
            Event::PointerMoved(p),
            Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ]
    };
    let y = m.row.mul_add(1.5, m.view.top());
    harness.frame(click(at(&m, 2.3), y), &Plain);
    assert!(
        harness.ctx.memory(|mem| mem.has_focus(id)),
        "a click takes focus"
    );
    assert_eq!(harness.view.state().primary().head, pos(1, 2));
    harness.frame(click(at(&m, 4.7), m.view.top() + 1.0), &Plain);
    assert_eq!(harness.view.state().primary().head, pos(0, 5));
}

#[test]
fn the_inline_diff_tints_new_lines_and_shows_removed_ones_as_rows() {
    let mut harness = Harness::new("a\nB\nc");
    harness.view.set_diff_base(Some("a\nb\nc\nd".to_owned()));
    let output = harness.frame(Vec::new(), &Plain);
    let style = harness.style.clone();
    let added = filled(&output, style.added);
    let removed = filled(&output, style.removed);
    assert_eq!(added.len(), 1, "line B");
    assert_eq!(removed.len(), 2, "b above B, and d at the end");
    let b = text_shape(&output, "b");
    let new_b = text_shape(&output, "B");
    assert!(
        b.pos.y < new_b.pos.y,
        "a removed line is drawn above where it was"
    );
    assert_eq!(b.galley.job.sections[0].format.color, style.removed_text);
    text_shape(&output, "d");
    assert!(removed[0].top() < added[0].top() && added[0].top() < removed[1].top());
    // A removed line's text sits where a line's does in its row, its mark where a number is.
    assert_eq!(b.pos.x, new_b.pos.x);
    assert_eq!(b.pos.y - removed[0].top(), new_b.pos.y - added[0].top());
    let mark = text_shape(&output, "-");
    let number = text_shape(&output, "2");
    assert_eq!(mark.pos.y - removed[0].top(), number.pos.y - added[0].top());
    assert!(mark.pos.x + mark.galley.size().x <= b.pos.x);
    assert_eq!(
        mark.pos.x + mark.galley.size().x,
        number.pos.x + number.galley.size().x
    );
    assert_eq!(harness.view.diff().unwrap().removed_before(1), ["b"]);
    // An edit is diffed again on the next frame.
    harness
        .view
        .state_mut()
        .set_selections(&[Selection::caret(pos(1, 0))]);
    harness.view.state_mut().delete();
    harness.view.state_mut().insert("b");
    assert!(!harness.view.diff().unwrap().is_added(1));
    harness.view.set_diff_base(None);
    let output = harness.frame(Vec::new(), &Plain);
    assert!(filled(&output, style.removed).is_empty());
    assert!(harness.view.diff().is_none());
}

#[test]
fn a_click_on_a_removed_row_places_the_caret_at_the_line_after_it() {
    let mut harness = Harness::new("a\nc");
    harness.view.set_diff_base(Some("a\nb\nc".to_owned()));
    let output = harness.frame(Vec::new(), &Plain);
    let removed = filled(&output, harness.style.removed)[0];
    harness.frame(
        vec![
            Event::PointerMoved(removed.center()),
            Event::PointerButton {
                pos: removed.center(),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            Event::PointerButton {
                pos: removed.center(),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
        &Plain,
    );
    assert_eq!(harness.view.state().primary().head, pos(1, 0));
    assert_eq!(harness.view.hovered(), Some(pos(1, 0)));
    // A line removed after the last one places the caret at the end of the text.
    let mut harness = Harness::new("a\nbc");
    harness.view.set_diff_base(Some("a\nbc\nd".to_owned()));
    let output = harness.frame(Vec::new(), &Plain);
    let removed = filled(&output, harness.style.removed)[0];
    harness.frame(vec![Event::PointerMoved(removed.center())], &Plain);
    assert_eq!(harness.view.hovered(), Some(pos(1, 2)));
}

#[test]
fn the_caret_rect_is_where_the_primary_caret_was_drawn() {
    let mut harness = Harness::new(&numbered(200));
    harness
        .view
        .state_mut()
        .set_selections(&[Selection::caret(pos(1, 2))]);
    let output = harness.frame(Vec::new(), &Plain);
    let caret = harness.view.caret_rect().unwrap();
    let carets = filled(&output, harness.style.caret);
    assert_eq!(carets, [caret], "the focused caret's rectangle");
    harness.view.scroll_to(100);
    harness.frame(Vec::new(), &Plain);
    assert_eq!(harness.view.caret_rect(), None, "scrolled off screen");
}

/// A press and release of the primary button at (`x`, `y`).
fn click_at(x: f32, y: f32) -> Vec<Event> {
    let p = egui::Pos2::new(x, y);
    vec![
        Event::PointerMoved(p),
        Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
        Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    ]
}

// No spec statement: a click in the gutter reports its line once and moves no caret; a click
// on the text reports none
#[test]
fn a_gutter_click_reports_its_line_and_leaves_the_caret() {
    let mut harness = Harness::new("abc\nabc\nabc");
    let output = harness.frame(Vec::new(), &Plain);
    let m = measures(&harness, &output);
    let y = m.row.mul_add(2.5, m.view.top());
    harness.frame(click_at(m.view.left() + 2.0, y), &Plain);
    assert_eq!(harness.view.take_gutter_click(), Some(2));
    assert_eq!(harness.view.take_gutter_click(), None, "taken once");
    assert_eq!(harness.view.state().primary().head, pos(0, 0));
    harness.frame(click_at(at(&m, 1.2), y), &Plain);
    assert_eq!(harness.view.take_gutter_click(), None);
    assert_eq!(harness.view.state().primary().head, pos(2, 1));
}

// No spec statement: the paused line has its own background, over the current line's, and the
// breakpoints are the lines set, from 0
#[test]
fn the_paused_line_is_tinted_and_breakpoints_are_kept() {
    let mut harness = Harness::new("abc\nabc\nabc");
    harness.view.set_breakpoints([0, 2]);
    assert_eq!(
        harness
            .view
            .breakpoints()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [0, 2]
    );
    harness.view.set_paused(Some(0));
    let output = harness.frame(Vec::new(), &Plain);
    let paused = filled(&output, harness.style.paused_line);
    assert_eq!(paused.len(), 1);
    assert!(
        filled(&output, harness.style.current_line).is_empty(),
        "the caret's line is the paused one"
    );
    harness.view.set_paused(None);
    let output = harness.frame(Vec::new(), &Plain);
    assert!(filled(&output, harness.style.paused_line).is_empty());
    assert_eq!(filled(&output, harness.style.current_line).len(), 1);
}

/// The texts painted, top to bottom.
fn painted_lines(output: &FullOutput) -> Vec<String> {
    let mut texts: Vec<TextShape> = shapes(output).1;
    texts.sort_by(|a, b| {
        a.pos
            .y
            .partial_cmp(&b.pos.y)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    texts
        .into_iter()
        .map(|t| t.galley.text().to_owned())
        .collect()
}

// No spec statement: P4-32, UI-16: a folded region shows its first and last lines and hides
// those between; the arrows go past it and a caret moved into it unfolds it
#[test]
fn a_folded_region_hides_its_middle_lines_and_a_caret_in_it_unfolds_it() {
    let text = "void() f =\n{\n\tlocal float a;\n\ta = 1;\n};\nfloat b;";
    let mut harness = Harness::new(text);
    harness
        .view
        .folds_mut()
        .set_regions([FoldRegion { start: 1, end: 4 }]);
    assert!(harness.view.folds_mut().fold_at(1));
    let output = harness.frame(Vec::new(), &Plain);
    let lines = painted_lines(&output);
    assert!(lines.contains(&"{".to_owned()) && lines.contains(&"};".to_owned()));
    assert!(
        !lines.iter().any(|l| l.contains("local float a")),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"\u{2026}".to_owned()),
        "the folded mark: {lines:?}"
    );
    // From the line above, down goes to the fold's first line, then past the hidden ones.
    harness
        .view
        .state_mut()
        .set_selections(&[Selection::caret(pos(1, 0))]);
    harness.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(harness.view.state().primary().head.line, 4);
    harness.key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(harness.view.state().primary().head.line, 1);
    assert!(harness.view.folds().any_folded());
    // Placed inside from outside the view, as go to line does, the caret unfolds it.
    harness
        .view
        .state_mut()
        .set_selections(&[Selection::caret(pos(2, 0))]);
    let output = harness.frame(Vec::new(), &Plain);
    assert!(!harness.view.folds().any_folded());
    assert!(
        painted_lines(&output)
            .iter()
            .any(|l| l.contains("local float a"))
    );
}

// No spec statement: P4-32: a click on a fold's mark in the gutter folds and unfolds it, and
// is no breakpoint click
#[test]
fn a_click_on_the_fold_mark_toggles_the_fold() {
    let mut harness = Harness::new("{\n\ta;\n\tb;\n}\nc;");
    harness
        .view
        .folds_mut()
        .set_regions([FoldRegion { start: 0, end: 3 }]);
    let output = harness.frame(Vec::new(), &Plain);
    let m = measures(&harness, &output);
    let mark = egui::Pos2::new(m.text_left - 6.0, m.view.top() + m.row / 2.0);
    let click = |harness: &mut Harness| {
        harness.frame(
            vec![
                Event::PointerMoved(mark),
                Event::PointerButton {
                    pos: mark,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                Event::PointerButton {
                    pos: mark,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ],
            &Plain,
        );
    };
    click(&mut harness);
    assert!(harness.view.folds().any_folded());
    assert_eq!(harness.view.take_gutter_click(), None);
    click(&mut harness);
    assert!(!harness.view.folds().any_folded());
}

// No spec statement: P4-32, UI-16: with soft wrap a line longer than the view takes more rows
// and the line after it moves down; without, it is one row
#[test]
fn soft_wrap_puts_a_long_line_on_more_rows() {
    let long = "word ".repeat(60);
    let text = format!("{long}\nafter");
    let mut harness = Harness::new(&text);
    let top_of_after = |output: &FullOutput| text_shape(output, "after").pos.y;
    let output = harness.frame(Vec::new(), &Plain);
    let m = measures(&harness, &output);
    let unwrapped = top_of_after(&output);
    assert!(near(unwrapped - m.view.top(), m.row));
    harness.view.set_wrap(Wrap::Width);
    let output = harness.frame(Vec::new(), &Plain);
    let wrapped = top_of_after(&output);
    assert!(
        wrapped - m.view.top() >= 2.0 * m.row - 1.0,
        "{wrapped} {}",
        m.row
    );
    // End clicks to the end of the long line: the caret is on its last row, below the first.
    harness
        .view
        .state_mut()
        .set_selections(&[Selection::caret(pos(0, 0))]);
    harness.key(Key::End, Modifiers::NONE);
    let output = harness.frame(Vec::new(), &Plain);
    let caret = filled(&output, harness.style.caret)[0];
    assert!(caret.top() > m.view.top() + m.row, "{caret:?}");
    harness.view.set_wrap(Wrap::Off);
    let output = harness.frame(Vec::new(), &Plain);
    assert!(near(top_of_after(&output), unwrapped));
}

// No spec statement: P4-32, UI-16: an indent guide at each indent level after the first, a tab
// or four spaces, and none on a blank line
#[test]
fn indent_guides_mark_each_inner_level() {
    let mut harness = Harness::new("a\n\t\tb\n        c\n\t\t\n");
    let output = harness.frame(Vec::new(), &Plain);
    let m = measures(&harness, &output);
    let guides: Vec<(f32, f32)> = output
        .shapes
        .iter()
        .flat_map(|clipped| match &clipped.shape {
            Shape::LineSegment { points, stroke } if stroke.color == harness.style.indent_guide => {
                Some((points[0].x, points[0].y))
            }
            _ => None,
        })
        .collect();
    assert_eq!(guides.len(), 2, "{guides:?}");
    assert!(guides.iter().all(|(x, _)| *x > m.text_left));
    assert!(guides.iter().any(|(_, y)| near(*y, m.view.top() + m.row)));
    assert!(
        guides
            .iter()
            .any(|(_, y)| near(*y, m.view.top() + 2.0 * m.row))
    );
}

// No spec statement: P4-35, UI-19: the gutter marks an added line, a changed one and where
// lines were removed against the text at HEAD, and follows an edit
#[test]
fn the_gutter_marks_changes_against_head() {
    let mut harness = Harness::new("one\nTWO\nthree\nnew\nfive");
    harness
        .view
        .set_gutter_base(Some("one\ntwo\nthree\nfour\ngone\nfive".to_owned()));
    let output = harness.frame(Vec::new(), &Plain);
    let m = measures(&harness, &output);
    let row_of = |rect: &Rect| ((rect.top() - m.view.top()) / m.row).round();
    let marks = |output: &FullOutput, color| -> Vec<f32> {
        filled(output, color)
            .iter()
            .filter(|r| r.width() <= 3.5)
            .map(row_of)
            .collect()
    };
    assert_eq!(marks(&output, harness.style.modified_mark), [1.0, 3.0]);
    assert!(marks(&output, harness.style.added_mark).is_empty());
    harness
        .view
        .state_mut()
        .set_selections(&[Selection::caret(pos(4, 4))]);
    harness.view.state_mut().insert("\nsix");
    let output = harness.frame(Vec::new(), &Plain);
    assert_eq!(marks(&output, harness.style.added_mark), [5.0]);
    harness.view.set_gutter_base(None);
    let output = harness.frame(Vec::new(), &Plain);
    assert!(marks(&output, harness.style.modified_mark).is_empty());
}
