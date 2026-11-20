// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use qs_templates::Template;
use qs_workbench::game_files::GameState;

use super::{NewProject, NewProjectAction, body};
use crate::strings::Text;
use crate::theme::{Mode, Theme};

/// The room the window is drawn in, in points.
const ROOM: eframe::egui::Vec2 = eframe::egui::Vec2::new(800.0, 600.0);

/// The dialog's body drawn with `chosen` marked, with the last click's action kept.
fn drawn(chosen: Template) -> Harness<'static, (Template, Option<NewProjectAction>)> {
    let theme = Theme::load(Mode::Dark).unwrap();
    let fonts = theme.clone();
    let mut harness = Harness::new_ui_state(
        move |ui, (chosen, action): &mut (Template, Option<NewProjectAction>)| {
            if let Some(clicked) = body(ui, &theme, chosen) {
                *action = Some(clicked);
            }
        },
        (chosen, None),
    );
    fonts.apply(&harness.ctx);
    harness.run();
    harness
}

/// Clicks the node labelled by each of `labels` in turn.
fn click<S>(harness: &mut Harness<'_, S>, labels: &[Text]) {
    for label in labels {
        harness.get_by_label(label.get()).click();
        harness.run();
    }
}

// No spec statement: L-28, the dialog names both games in plain words and says what each
// project starts from; no text names a template's folder
#[test]
fn both_games_are_named() {
    let harness = drawn(Template::Id1);
    for text in [
        Text::NewProjectWhich,
        Text::GameOriginal,
        Text::GameOriginalFrom,
        Text::GameRerelease,
        Text::GameRereleaseFrom,
    ] {
        assert!(
            harness.query_by_label(text.get()).is_some(),
            "{}",
            text.get()
        );
    }
    for text in Text::ALL {
        assert!(!text.get().contains("id1 Project"), "{}", text.get());
    }
}

// No spec statement: L-28, Choose Folder asks for a project of the game marked, which a click
// on the other game changes
#[test]
fn choose_folder_makes_the_game_marked() {
    let mut harness = drawn(Template::Id1);
    click(&mut harness, &[Text::NewProjectChoose]);
    assert_eq!(
        harness.state().1,
        Some(NewProjectAction::Make(Template::Id1))
    );

    let mut harness = drawn(Template::Id1);
    click(&mut harness, &[Text::GameRerelease, Text::NewProjectChoose]);
    assert_eq!(
        harness.state().1,
        Some(NewProjectAction::Make(Template::Rerelease))
    );

    let mut harness = drawn(Template::Rerelease);
    click(&mut harness, &[Text::GameOriginal, Text::NewProjectChoose]);
    assert_eq!(
        harness.state().1,
        Some(NewProjectAction::Make(Template::Id1))
    );
}

// No spec statement: L-28, Cancel makes nothing
#[test]
fn cancel_closes() {
    let mut harness = drawn(Template::Rerelease);
    click(&mut harness, &[Text::PaletteCancel]);
    assert_eq!(harness.state().1, Some(NewProjectAction::Close));
}

// No spec statement: L-28, the rerelease is chosen to begin with only when the rerelease is
// all that was found; the original otherwise, and when the game was not looked for
#[test]
fn the_game_found_decides_the_one_marked() {
    for (found, template) in [
        (Some(GameState::RereleaseOnly), Template::Rerelease),
        (Some(GameState::Found), Template::Id1),
        (Some(GameState::SharewareOnly), Template::Id1),
        (Some(GameState::Missing), Template::Id1),
        (None, Template::Id1),
    ] {
        assert_eq!(NewProject::preselected(found), template, "{found:?}");
        let mut dialog = NewProject::default();
        dialog.open(found);
        assert_eq!(dialog, NewProject::Shown(template));
    }
}

// No spec statement: L-28, the window hides on Choose Folder, which returns the game marked,
// and on Escape, which returns nothing; hidden, it draws nothing
#[test]
fn the_window_hides_when_answered() {
    let theme = Theme::load(Mode::Dark).unwrap();
    let fonts = theme.clone();
    // Room for the window: a harness sized to its panel's contents, which are none, has none.
    let mut harness = Harness::builder().with_size(ROOM).build_ui_state(
        move |ui, (dialog, made): &mut (NewProject, Option<Template>)| {
            if let Some(template) = dialog.show(ui.ctx(), &theme) {
                *made = Some(template);
            }
        },
        (NewProject::Shown(Template::Rerelease), None),
    );
    fonts.apply(&harness.ctx);
    // Twice: the window takes its place a frame after the fonts change its size.
    harness.run();
    harness.run();
    click(&mut harness, &[Text::NewProjectChoose]);
    assert_eq!(
        *harness.state(),
        (NewProject::Hidden, Some(Template::Rerelease))
    );
    assert!(
        harness
            .query_by_label(Text::NewProjectWhich.get())
            .is_none()
    );

    harness.state_mut().0.open(None);
    harness.state_mut().1 = None;
    harness.run();
    harness.key_press(eframe::egui::Key::Escape);
    harness.run();
    assert_eq!(*harness.state(), (NewProject::Hidden, None));
}
