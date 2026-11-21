// SPDX-License-Identifier: GPL-3.0-or-later

use super::{Edict, PlayState, Properties};

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
    // The game runs, the grunt is shot, the game is paused again.
    panel.follow(grunt, PlayState::Playing);
    panel.follow(grunt, PlayState::Paused);
    assert_eq!(panel.text("health", "6"), "6");
    // Typed again, then another edict is selected, and nothing.
    panel.text("health", "6").push('1');
    panel.follow(Some(Edict::new(8)), PlayState::Paused);
    assert_eq!(panel.text("health", "6"), "6");
    panel.text("health", "6").push('1');
    panel.follow(None, PlayState::Paused);
    panel.follow(Some(Edict::new(8)), PlayState::Paused);
    assert_eq!(panel.text("health", "6"), "6");
}
