// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use eframe::egui::accesskit::Role;
use eframe::egui::{self, Key, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use qs_workbench::WatchName;
use qs_workbench::debug::Location;

use super::{DebugAction, DebugInput};
use crate::strings::Text;
use crate::theme::{Mode, Theme};

/// The console's box drawn alone, and the lines it asked to run.
struct Drawn {
    input: DebugInput,
    ran: Vec<String>,
}

/// The console's box drawn alone, with the levels `start` and `e1m1`, the keyboard in it.
fn drawn() -> Harness<'static, Drawn> {
    let theme = Theme::load(Mode::Dark).unwrap();
    let mut input = DebugInput::default();
    input.set_levels(Some(vec!["e1m1".to_owned(), "start".to_owned()]));
    let mut harness = theme
        .harness()
        .with_size(Vec2::new(500.0, 300.0))
        .build_ui_state(
            move |ui, drawn: &mut Drawn| {
                let asked = ui
                    .horizontal(|ui| drawn.input.command_box(ui, &theme))
                    .inner;
                if let Some(DebugAction::Console(line)) = asked {
                    drawn.ran.push(line);
                }
            },
            Drawn {
                input,
                ran: Vec::new(),
            },
        );
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, Text::ConsoleCommand.get())
        .click();
    harness.run();
    harness
}

/// Types `text` in the box, then a frame.
fn type_in(harness: &mut Harness<'static, Drawn>, text: &str) {
    harness.event(egui::Event::Text(text.to_owned()));
    harness.run();
}

/// What the box holds.
fn line(harness: &Harness<'static, Drawn>) -> String {
    harness
        .get_by_role_and_label(Role::TextInput, Text::ConsoleCommand.get())
        .accesskit_node()
        .value()
        .unwrap_or_default()
}

// No spec statement: UI-24, CON-4.3: the console's box offers its commands and settings as
// they are typed, each said to be a command or a setting with its default; one taken is put
// in the box with a space after it and the keyboard kept, and Enter then runs the line
#[test]
fn a_console_command_is_taken_from_the_offers() {
    let mut harness = drawn();
    type_in(&mut harness, "noc");
    harness.get_by_label("noclip, command").click();
    harness.run();
    assert_eq!(line(&harness), "noclip ");
    assert!(
        harness.state().ran.is_empty(),
        "taking an offer runs nothing"
    );
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().ran, ["noclip"]);
    assert_eq!(line(&harness), "");
    // The keyboard stays in the box, so a setting is typed at once.
    type_in(&mut harness, "sv_grav");
    harness.get_by_label("sv_gravity, setting, 800 by default");
}

// No spec statement: UI-24: after `map` or `changelevel` the box offers the game's levels, and
// a line no offer holds is still run as typed
#[test]
fn a_level_is_offered_after_map() {
    let mut harness = drawn();
    type_in(&mut harness, "map ");
    harness.get_by_label("map start, level");
    harness.get_by_label("map e1m1, level").click();
    harness.run();
    assert_eq!(line(&harness), "map e1m1 ");
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().ran, ["map e1m1"]);
    type_in(&mut harness, "changelevel st");
    harness.get_by_label("changelevel start, level");
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().ran, ["map e1m1", "changelevel st"]);
    // Leaving the box runs nothing.
    type_in(&mut harness, "god");
    harness.key_press(Key::Escape);
    harness.run();
    assert_eq!(harness.state().ran.len(), 2);
}

// No spec statement: UI-24: before the levels are read, `map` offers none, and nothing else
#[test]
fn no_level_is_offered_before_they_are_read() {
    let mut input = DebugInput::default();
    assert!(input.levels_unread());
    assert!(input.console_offers("map ").is_empty());
    assert!(!input.console_offers("ma").is_empty());
    input.set_levels(Some(Vec::new()));
    assert!(!input.levels_unread());
    assert!(input.console_offers("  changelevel x").is_empty());
    // A word that only starts with `map` is a command still being typed.
    assert!(
        input
            .console_offers("mapx ")
            .iter()
            .any(|offer| offer.name == "map")
    );
}

/// The watch box drawn alone, offering `self.health` and `time` where the game stopped, and
/// the watches it asked to add.
fn watch_drawn() -> Harness<'static, (DebugInput, Vec<String>)> {
    let theme = Theme::load(Mode::Dark).unwrap();
    let mut input = DebugInput::default();
    let stopped = Location {
        function: "hurt".to_owned(),
        file: Some("world.qc".into()),
        line: Some(7),
        engine: false,
    };
    assert!(input.watch_names_stale(Some(&stopped)));
    let names = [("self.health", ".float"), ("time", "float")]
        .map(|(expression, detail)| WatchName {
            expression: expression.to_owned(),
            detail: detail.to_owned(),
        })
        .to_vec();
    input.set_watch_names(Some(stopped.clone()), names);
    assert!(!input.watch_names_stale(Some(&stopped)));
    assert!(input.watch_names_stale(None));
    let mut harness = theme
        .harness()
        .with_size(Vec2::new(500.0, 300.0))
        .build_ui_state(
            move |ui, (input, added): &mut (DebugInput, Vec<String>)| {
                if let Some(DebugAction::Watch(expression)) = input.watch_box(ui, &theme) {
                    added.push(expression);
                }
            },
            (input, Vec::new()),
        );
    harness.run();
    harness
        .get_by_role_and_label(Role::TextInput, Text::AddWatch.get())
        .click();
    harness.run();
    harness
}

// No spec statement: UI-24, WORLD-9.5: the watch box offers what a watch may read where the
// game stopped, each with its type; one taken is added at once, and an expression typed is
// added on Enter
#[test]
fn a_watch_is_taken_from_the_names_in_scope() {
    let mut harness = watch_drawn();
    harness.event(egui::Event::Text("heal".to_owned()));
    harness.run();
    harness.get_by_label("self.health, .float").click();
    harness.run();
    assert_eq!(harness.state().1, ["self.health"]);
    harness
        .get_by_role_and_label(Role::TextInput, Text::AddWatch.get())
        .click();
    harness.run();
    harness.event(egui::Event::Text("self.enemy.health".to_owned()));
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().1, ["self.health", "self.enemy.health"]);
}
