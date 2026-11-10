// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::PathBuf;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use qs_workbench::game_files::{GameFiles, GameState};

use super::{Guide, GuideAction, guide, how_to_get};
use crate::strings::Text;
use crate::theme::{Mode, Theme};

/// Game data in `state`, at `/games/quake`, lacking `missing`.
fn files(state: GameState, missing: &[&str]) -> GameFiles {
    GameFiles {
        state,
        folder: Some(PathBuf::from("/games/quake")),
        missing: missing.iter().map(|&m| m.to_owned()).collect(),
    }
}

/// The guide drawn for `files`, with the last click's action kept.
fn drawn(files: GameFiles) -> Harness<'static, (GameFiles, Option<GuideAction>)> {
    let theme = Theme::load(Mode::Dark).unwrap();
    let mut harness = Harness::new_ui_state(
        move |ui, (files, action): &mut (GameFiles, Option<GuideAction>)| {
            if let Some(clicked) = guide(ui, &theme, files) {
                *action = Some(clicked);
            }
        },
        (files, None),
    );
    harness.run();
    harness
}

fn shows<S>(harness: &Harness<'_, S>, text: Text) -> bool {
    harness.query_by_label(text.get()).is_some()
}

// spec FS-6.5: the registered game is found; the guide says so and gives no steps
#[test]
fn found_says_so_and_gives_no_steps() {
    let harness = drawn(files(GameState::Found, &[]));
    assert!(shows(&harness, Text::GameFound));
    assert!(harness.query_by_label("/games/quake").is_some());
    assert!(!shows(&harness, how_to_get()));
    assert!(!shows(&harness, Text::GameLacks));
    assert!(shows(&harness, Text::GameInPlace));
}

// spec FS-6.5: the shareware episode alone lacks id1/pak1.pak, and the guide says how to get it
#[test]
fn shareware_only_names_pak1() {
    let harness = drawn(files(GameState::SharewareOnly, &["id1/pak1.pak"]));
    assert!(shows(&harness, Text::GameSharewareOnly));
    assert!(shows(&harness, Text::GameLacks));
    assert!(harness.query_by_label("id1/pak1.pak").is_some());
    assert!(shows(&harness, how_to_get()));
    assert!(shows(&harness, Text::GameStores));
}

// spec FS-6.5: the rerelease alone lacks the original game's two paks
#[test]
fn rerelease_only_names_both_paks() {
    let harness = drawn(files(
        GameState::RereleaseOnly,
        &["id1/pak0.pak", "id1/pak1.pak"],
    ));
    assert!(shows(&harness, Text::GameRereleaseOnly));
    assert!(
        harness
            .query_by_label("id1/pak0.pak, id1/pak1.pak")
            .is_some()
    );
    assert!(shows(&harness, Text::GameStores));
}

// No spec statement: with nothing found the guide says so, says how to get the files on this
// OS, and its buttons ask for the folder picker and for closing
#[test]
fn missing_offers_the_folder_picker() {
    let mut harness = drawn(GameFiles {
        state: GameState::Missing,
        folder: None,
        missing: vec!["id1/pak0.pak".to_owned(), "id1/pak1.pak".to_owned()],
    });
    assert!(shows(&harness, Text::GameMissing));
    assert!(shows(&harness, how_to_get()));
    harness.get_by_label(Text::ChooseGameFolder.get()).click();
    harness.run();
    assert_eq!(harness.state().1, Some(GuideAction::ChooseFolder));
    harness.get_by_label(Text::NotNow.get()).click();
    harness.run();
    assert_eq!(harness.state().1, Some(GuideAction::Close));
}

// No spec statement: the guide opens unless the registered game is found
#[test]
fn the_guide_opens_unless_the_game_is_found() {
    assert_eq!(
        Guide::for_files(&files(GameState::Found, &[])),
        Guide::Hidden
    );
    for state in [
        GameState::SharewareOnly,
        GameState::RereleaseOnly,
        GameState::Missing,
    ] {
        assert_eq!(
            Guide::for_files(&files(state, &[])),
            Guide::Shown,
            "{state:?}"
        );
    }
}
