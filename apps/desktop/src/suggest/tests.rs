// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use eframe::egui::accesskit::Role;
use eframe::egui::{self, Key, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::{Offer, Suggest};
use crate::theme::{Mode, Theme};

/// The offers `gate`, `Gate2` and `light1`, each naming its class.
fn offers() -> Vec<Offer> {
    [
        ("gate", "func_door"),
        ("Gate2", "func_door"),
        ("light1", "light"),
    ]
    .into_iter()
    .map(|(name, detail)| Offer {
        name: name.to_owned(),
        detail: detail.to_owned(),
    })
    .collect()
}

/// The names `typed` matches among [`offers`].
fn matching(typed: &str) -> Vec<String> {
    let offers = offers();
    let field = Suggest {
        name: "target",
        offers: &offers,
        width: 0.0,
    };
    field
        .matching(typed)
        .into_iter()
        .map(|offer| offer.name.clone())
        .collect()
}

// No spec statement: UI-24: an empty field, or one holding an offer exactly, offers every
// value, so a value set already shows what else it may be; text typed narrows the list to
// the offers containing it in any case, and text no offer contains offers none
#[test]
fn typing_narrows_the_offers() {
    assert_eq!(matching(""), ["gate", "Gate2", "light1"]);
    assert_eq!(matching(" gate "), ["gate", "Gate2", "light1"]);
    assert_eq!(matching("GAT"), ["gate", "Gate2"]);
    assert_eq!(matching("t1"), ["light1"]);
    assert!(matching("door").is_empty());
}

/// The field drawn alone over [`offers`]: what is typed in it, and what it last took.
struct Drawn {
    typed: String,
    done: Option<String>,
}

/// A harness drawing the field over `offers`, named `target`, with the keyboard in it.
fn drawn_over(offers: Vec<Offer>) -> Harness<'static, Drawn> {
    let theme = Theme::load(Mode::Dark).unwrap();
    let mut harness = theme
        .harness()
        .with_size(Vec2::new(400.0, 300.0))
        .build_ui_state(
            move |ui, drawn: &mut Drawn| {
                let field = Suggest {
                    name: "target",
                    offers: &offers,
                    width: 120.0,
                };
                if let Some(done) = field.show(ui, &theme, &mut drawn.typed) {
                    drawn.done = Some(done);
                }
            },
            Drawn {
                typed: String::new(),
                done: None,
            },
        );
    harness.run();
    harness.get_by_role(Role::TextInput).click();
    harness.run();
    harness
}

/// A harness drawing the field over [`offers`].
fn drawn() -> Harness<'static, Drawn> {
    drawn_over(offers())
}

/// Types `text` in the field, then a frame.
fn type_in(harness: &mut Harness<'static, Drawn>, text: &str) {
    harness.event(egui::Event::Text(text.to_owned()));
    harness.run();
}

// No spec statement: UI-24: a row marked with the arrow keys is forgotten once more is typed,
// so Enter takes what the field holds and not the row the old mark now points at
#[test]
fn typing_clears_the_mark() {
    let mut harness = drawn();
    type_in(&mut harness, "ga");
    harness.key_press(Key::ArrowDown);
    harness.run();
    harness.key_press(Key::ArrowDown);
    harness.run();
    type_in(&mut harness, "te");
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().done.as_deref(), Some("gate"));
    // With the mark kept, Enter takes the marked row.
    harness.get_by_role(Role::TextInput).click();
    harness.run();
    harness.key_press(Key::ArrowDown);
    harness.run();
    harness.key_press(Key::ArrowDown);
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().done.as_deref(), Some("Gate2"));
}

// No spec statement: UI-24: a drag on the list's scroll bar is no row's click and leaves the
// keyboard in the field, its edit open, so Enter then takes what was typed
#[test]
fn a_drag_on_the_lists_scroll_bar_leaves_the_edit_open() {
    let many = (0..20)
        .map(|n| Offer {
            name: format!("t{n}"),
            detail: String::new(),
        })
        .collect();
    let mut harness = drawn_over(many);
    type_in(&mut harness, "t");
    let first = harness.get_by_label("t0").rect();
    // The scroll bar, at the list's right edge beside its first row.
    let bar = egui::pos2(first.left() + super::LIST_WIDTH - 4.0, first.center().y);
    let below = egui::pos2(bar.x, bar.y + 40.0);
    harness.hover_at(bar);
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: bar,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    harness.hover_at(below);
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: below,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    assert_eq!(harness.state().done, None);
    assert!(harness.get_by_role(Role::TextInput).is_focused());
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().done.as_deref(), Some("t"));
}

// No spec statement: UI-24: what is taken is trimmed, so a stray space is not part of a name
#[test]
fn what_is_taken_is_trimmed() {
    let mut harness = drawn();
    type_in(&mut harness, "gate ");
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().done.as_deref(), Some("gate"));
    assert_eq!(harness.state().typed, "gate");
}

// No spec statement: UI-14: a row marked with the arrow keys is scrolled to as the mark moves,
// and the list still scrolls with the wheel after it
#[test]
fn the_wheel_scrolls_the_list_after_a_mark() {
    let many = (0..20)
        .map(|n| Offer {
            name: format!("t{n}"),
            detail: String::new(),
        })
        .collect();
    let mut harness = drawn_over(many);
    type_in(&mut harness, "t");
    harness.key_press(Key::ArrowDown);
    harness.run();
    let before = harness.get_by_label("t0").rect().top();
    harness.hover_at(harness.get_by_label("t1").rect().center());
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        source: egui::MouseWheelSource::Unknown,
        delta: egui::vec2(0.0, -60.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    harness.run();
    let after = harness.get_by_label("t0").rect().top();
    assert!(after < before, "{before} then {after}");
}
