// SPDX-License-Identifier: GPL-3.0-or-later

//! What the view paints, read from egui's shapes: the gutter, the lines, the current line, the
//! selections and the carets, in units of the code font's character width and the row height.

use egui::epaint::{RectShape, Shape, TextShape};
use egui::{Color32, Event, FullOutput, Key, Modifiers, Rect};

use super::{Harness, numbered, pos};
use crate::Selection;
use crate::layout::Plain;

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
