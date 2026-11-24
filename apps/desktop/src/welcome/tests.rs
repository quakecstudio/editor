// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::PathBuf;

use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use qs_workbench::game_files::{GameFiles, GameState};

use super::{Page, WelcomeAction, page};
use crate::features::LeftAction;
use crate::strings::Text;
use crate::theme::{Mode, Theme};

/// The room the page is drawn in, in points.
const PAGE: egui::Vec2 = egui::Vec2::new(800.0, 800.0);

/// What a test's page is drawn from.
#[derive(Default)]
struct Spec {
    files: Option<GameFiles>,
    recent: Vec<PathBuf>,
    error: Option<String>,
    play_key: Option<String>,
}

/// The page drawn for `spec`, with the last click's action kept.
fn drawn(spec: Spec) -> Harness<'static, (Spec, Option<WelcomeAction>)> {
    let theme = Theme::load(Mode::Dark).unwrap();
    // Room for the whole page: a click on a row scrolled out of sight reaches nothing.
    let mut harness = theme.harness().with_size(PAGE).build_ui_state(
        move |ui, (spec, action): &mut (Spec, Option<WelcomeAction>)| {
            let shown = Page {
                icon: None,
                files: spec.files.as_ref(),
                recent: &spec.recent,
                error: spec.error.as_deref(),
                play_key: spec.play_key.clone(),
                palette_key: "Cmd+Shift+P".to_owned(),
            };
            if let Some(clicked) = page(ui, &theme, &shown) {
                *action = Some(clicked);
            }
        },
        (spec, None),
    );
    harness.run();
    harness
}

/// The action of a click on the node labelled `label`.
fn clicked(spec: Spec, label: &str) -> Option<WelcomeAction> {
    let mut harness = drawn(spec);
    harness.get_by_label(label).click();
    harness.run();
    harness.state_mut().1.take()
}

fn files(state: GameState) -> GameFiles {
    GameFiles {
        state,
        folder: Some(PathBuf::from("/games/quake")),
        missing: Vec::new(),
    }
}

fn shows<S>(harness: &Harness<'_, S>, text: Text) -> bool {
    harness.query_by_label(text.get()).is_some()
}

// No spec statement: the page names its three steps, and before the game was looked for the
// first says why Play needs it
#[test]
fn the_three_steps_are_named() {
    let harness = drawn(Spec::default());
    for text in [
        Text::WelcomeTitle,
        Text::WelcomeTagline,
        Text::WelcomeGame,
        Text::WelcomeGameWhy,
        Text::WelcomeProject,
        Text::WelcomeProjectHow,
        Text::WelcomePlay,
        Text::WelcomePlayHow,
    ] {
        assert!(shows(&harness, text), "{}", text.get());
    }
    assert!(!shows(&harness, Text::RecentProjects), "none to list");
}

// No spec statement: the first step says what was found of the game and where
#[test]
fn the_first_step_says_what_was_found() {
    let harness = drawn(Spec {
        files: Some(files(GameState::Found)),
        ..Spec::default()
    });
    assert!(shows(&harness, Text::GameFound));
    assert!(harness.query_by_label("/games/quake").is_some());
    assert!(shows(&harness, Text::GameInPlace));
    assert!(!shows(&harness, Text::WelcomeGameWhy));
    let harness = drawn(Spec {
        files: Some(GameFiles {
            folder: None,
            ..files(GameState::Missing)
        }),
        ..Spec::default()
    });
    assert!(shows(&harness, Text::GameMissing));
}

// No spec statement: Play's key is written beside the third step when it has one
#[test]
fn plays_key_is_shown() {
    let harness = drawn(Spec {
        play_key: Some("F5".to_owned()),
        ..Spec::default()
    });
    assert!(harness.query_by_label("F5").is_some());
}

// No spec statement: each button and link asks for its own thing
#[test]
fn each_click_asks_for_its_own_thing() {
    assert!(matches!(
        clicked(Spec::default(), Text::WelcomeGameButton.get()),
        Some(WelcomeAction::GameFiles)
    ));
    assert!(matches!(
        clicked(Spec::default(), Text::NewProjectMenu.get()),
        Some(WelcomeAction::Project(LeftAction::New))
    ));
    assert!(matches!(
        clicked(Spec::default(), Text::OpenFolderButton.get()),
        Some(WelcomeAction::Project(LeftAction::OpenFolder))
    ));
    assert!(matches!(
        clicked(Spec::default(), Text::WelcomePalette.get()),
        Some(WelcomeAction::Palette)
    ));
    assert!(matches!(
        clicked(Spec::default(), Text::WelcomeGuide.get()),
        Some(WelcomeAction::UserGuide)
    ));
}

// No spec statement: a recent project is a row named for its folder, and a click opens it
#[test]
fn a_recent_project_opens_on_a_click() {
    let recent = vec![PathBuf::from("/mods/ember"), PathBuf::from("/mods/frost")];
    let spec = Spec {
        recent: recent.clone(),
        ..Spec::default()
    };
    let asked = clicked(spec, "frost");
    let folder = match asked {
        Some(WelcomeAction::Project(LeftAction::Open(folder))) => Some(folder),
        _ => None,
    };
    assert_eq!(folder.as_ref(), recent.last());
}

// No spec statement: why a project did not open is said under the second step, so it is seen
// with the Project panel hidden
#[test]
fn a_project_that_did_not_open_says_why() {
    let harness = drawn(Spec {
        error: Some("The folder is no longer there: /mods/gone".to_owned()),
        ..Spec::default()
    });
    assert!(
        harness
            .query_by_label("The folder is no longer there: /mods/gone")
            .is_some()
    );
}

// No spec statement: the page's title and each step's title are headings to a screen reader,
// as the panels' are
#[test]
fn the_titles_are_headings() {
    let harness = drawn(Spec::default());
    for text in [
        Text::WelcomeTitle,
        Text::WelcomeGame,
        Text::WelcomeProject,
        Text::WelcomePlay,
    ] {
        harness.get_by_role_and_label(Role::Heading, text.get());
    }
}
