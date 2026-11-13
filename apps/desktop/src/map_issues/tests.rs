// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use qs_workbench::map_edit::{
    BrushIndex, EntityIndex, MapFound, MapIssue, MapSearchParams, QuickFix,
};

use super::{IssueBrowser, SearchFields};
use crate::map_editor::MapAction;
use crate::strings::{Text, entity_place, fix_label, issue_line};
use crate::theme::{Mode, Theme};

fn rename() -> QuickFix {
    QuickFix::RenameKey {
        entity: EntityIndex::new(2),
        from: "sped".to_owned(),
        to: "speed".to_owned(),
    }
}

fn issues() -> Vec<MapIssue> {
    vec![
        MapIssue {
            line: 12,
            message: "sped is no field".to_owned(),
            entity: Some(EntityIndex::new(2)),
            brush: None,
            fix: Some(rename()),
        },
        MapIssue {
            line: 0,
            message: "no WAD holds wall".to_owned(),
            entity: None,
            brush: None,
            fix: None,
        },
    ]
}

/// The browser's body with `browser`, after `act` on its harness; what it asked for.
fn after(
    browser: IssueBrowser,
    act: impl FnOnce(&mut Harness<'_, (IssueBrowser, Vec<MapAction>)>),
) -> Vec<MapAction> {
    let theme = Theme::load(Mode::Dark).unwrap();
    let mut harness = Harness::new_ui_state(
        |ui, (browser, actions): &mut (IssueBrowser, Vec<MapAction>)| {
            actions.extend(browser.header(ui, &theme));
            actions.extend(browser.body(ui, &theme));
        },
        (browser, Vec::new()),
    );
    harness.run();
    harness.state_mut().1.clear();
    act(&mut harness);
    harness.run();
    harness.state().1.clone()
}

// No spec statement: P6-19: an issue's Fix button asks for its fix, its text selects the
// entity it is about, and Check asks for the issues
#[test]
fn issues_fix_and_select() {
    let mut browser = IssueBrowser::default();
    browser.set_issues(issues());
    let fixed = after(browser, |harness| {
        harness.get_by_label(&fix_label(&rename())).click();
    });
    assert_eq!(fixed, vec![MapAction::Fix(rename())]);
    let mut browser = IssueBrowser::default();
    browser.set_issues(issues());
    let chosen = after(browser, |harness| {
        harness
            .get_by_label(&issue_line(12, "sped is no field"))
            .click();
    });
    assert_eq!(chosen, vec![MapAction::Select(Some(EntityIndex::new(2)))]);
    let checked = after(IssueBrowser::default(), |harness| {
        harness.get_by_label(Text::MapCheckIssues.get()).click();
    });
    assert_eq!(checked, vec![MapAction::CheckIssues]);
}

// No spec statement: P6-19: Find asks for the search the fields give, each part trimmed and an
// empty one left out, and a match selects its entity
#[test]
fn the_search_finds_and_selects() {
    let fields = SearchFields {
        classname: " ogre ".to_owned(),
        key: String::new(),
        value: "gate".to_owned(),
        texture: "  ".to_owned(),
    };
    assert_eq!(
        fields.params(),
        MapSearchParams {
            classname: Some("ogre".to_owned()),
            value: Some("gate".to_owned()),
            ..MapSearchParams::default()
        }
    );
    let browser = IssueBrowser {
        fields: fields.clone(),
        ..IssueBrowser::default()
    };
    let found = after(browser, |harness| {
        harness.get_by_label(Text::Find.get()).click();
    });
    assert_eq!(found, vec![MapAction::Find(fields.params())]);
    let mut browser = IssueBrowser::default();
    browser.set_found(MapFound {
        entities: vec![EntityIndex::new(3)],
        brushes: vec![(EntityIndex::new(3), BrushIndex::new(0))],
    });
    let chosen = after(browser, |harness| {
        harness.get_by_label(&entity_place(3)).click();
    });
    assert_eq!(chosen, vec![MapAction::Select(Some(EntityIndex::new(3)))]);
}

// No spec statement: P6-19: another map's issues and matches are forgotten
#[test]
fn clearing_forgets_both() {
    let mut browser = IssueBrowser::default();
    browser.set_issues(issues());
    browser.set_found(MapFound::default());
    browser.clear();
    assert!(browser.issues.is_none() && browser.found.is_none());
    assert!(after(browser, |_| {}).is_empty());
}
