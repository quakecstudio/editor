// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use eframe::egui::accesskit::Role;
use eframe::egui::{Key, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use qs_workbench::entities::Entities;
use serde_json::json;

use super::{Edict, EntityAction, PlayState, Properties, Shown, properties};
use crate::theme::{Mode, Theme};

// No spec statement: L-53: a paused field's box holds the game's value now: what a box
// held in an earlier pause is not kept through the game's running, nor for another
// edict, and a value the game changes while paused, by a step, is followed; what is
// typed stays while the value under it does
#[test]
fn a_paused_fields_box_shows_the_games_value() {
    let grunt = Some(Edict::new(7));
    let mut panel = Properties::default();
    panel.follow(grunt, PlayState::Paused);
    assert_eq!(panel.text("health", "30"), "30");
    // Typed, and kept while the game's value is the same.
    panel.text("health", "30").push('0');
    panel.follow(grunt, PlayState::Paused);
    assert_eq!(panel.text("health", "30"), "300");
    // A step changes the value under it: the box follows.
    assert_eq!(panel.text("health", "25"), "25");
    // Typed over a value the game then leaves as it is while it runs: gone at the next pause.
    panel.text("health", "25").push('9');
    panel.follow(grunt, PlayState::Playing);
    panel.follow(grunt, PlayState::Paused);
    assert_eq!(panel.text("health", "25"), "25");
    // Typed again, then another edict is selected, and nothing.
    panel.text("health", "25").push('1');
    panel.follow(Some(Edict::new(8)), PlayState::Paused);
    assert_eq!(panel.text("health", "25"), "25");
    panel.text("health", "25").push('1');
    panel.follow(None, PlayState::Paused);
    panel.follow(Some(Edict::new(8)), PlayState::Paused);
    assert_eq!(panel.text("health", "25"), "25");
}

/// What the drawn panel is given and keeps.
struct Drawn {
    entities: Entities,
    game: PlayState,
    panel: Properties,
    asked: Option<EntityAction>,
}

/// The game's entities with a grunt selected whose health is `health` and armour 0.
fn grunt(health: f64) -> Entities {
    let mut entities = Entities::new();
    entities.select(&json!({
        "edict": 7,
        "classname": "monster_army",
        "fields": [
            { "name": "health", "value": { "type": "float", "value": health } },
            { "name": "armorvalue", "value": { "type": "float", "value": 0.0 } },
        ],
    }));
    entities
}

/// The Properties panel drawn over a paused game whose grunt has 30 health.
fn drawn() -> Harness<'static, Drawn> {
    let theme = Theme::load(Mode::Dark).unwrap();
    let state = Drawn {
        entities: grunt(30.0),
        game: PlayState::Paused,
        panel: Properties::default(),
        asked: None,
    };
    let mut harness = theme
        .harness()
        .with_size(Vec2::new(400.0, 300.0))
        .build_ui_state(
            move |ui, drawn: &mut Drawn| {
                let shown = Shown {
                    entities: &drawn.entities,
                    game: drawn.game,
                    pending: &|_, _| None,
                };
                if let Some(asked) = properties(ui, &theme, &shown, &mut drawn.panel) {
                    drawn.asked = Some(asked);
                }
            },
            state,
        );
    harness.run();
    harness
}

/// The field's value as the panel writes it.
fn written(harness: &Harness<'_, Drawn>, field: &str) -> String {
    let entities = &harness.state().entities;
    entities.selected().unwrap().shown(field, entities).unwrap()
}

/// The text in each of the drawn panel's boxes, top to bottom.
fn boxes(harness: &Harness<'_, Drawn>) -> Vec<String> {
    harness
        .query_all_by_role(Role::TextInput)
        .map(|node| node.accesskit_node().value().unwrap_or_default())
        .collect()
}

/// Types `text` into the drawn panel's box `place`, from 0.
fn type_in(harness: &mut Harness<'_, Drawn>, place: usize, text: &str) {
    let node = harness.get_all_by_role(Role::TextInput).nth(place).unwrap();
    node.focus();
    node.type_text(text);
    harness.run();
}

// No spec statement: L-53, UI-13: drawn, the panel's boxes show the grunt's fields while
// the game is paused; Enter in one box asks for that field's change and leaves what was
// typed in another; and after the game ran and the grunt was shot, the next pause's boxes
// show its fields as they are, with nothing of the first pause's
#[test]
fn the_drawn_panel_shows_each_pauses_values() {
    let mut harness = drawn();
    let (health, armour) = (written(&harness, "health"), written(&harness, "armorvalue"));
    assert_eq!(boxes(&harness), [health.clone(), armour.clone()]);
    type_in(&mut harness, 1, "5");
    type_in(&mut harness, 0, "9");
    let typed = boxes(&harness);
    assert_ne!(typed, [health.clone(), armour.clone()]);
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(
        harness.state().asked,
        Some(EntityAction::SetField {
            field: "health".to_owned(),
            value: typed.first().unwrap().clone(),
        })
    );
    // What was typed in the other box stays.
    assert_eq!(boxes(&harness).get(1), typed.get(1));

    // The game runs and the grunt is shot: no box, and its health as it is.
    harness.state_mut().game = PlayState::Playing;
    harness.state_mut().entities = grunt(6.0);
    harness.run();
    let shot = written(&harness, "health");
    assert_ne!(shot, health);
    assert!(boxes(&harness).is_empty());
    assert!(harness.query_by_label(&shot).is_some(), "{shot}");
    // Paused again: the boxes hold the values of now, with nothing typed.
    harness.state_mut().game = PlayState::Paused;
    harness.run();
    assert_eq!(boxes(&harness), [shot, armour]);
}
