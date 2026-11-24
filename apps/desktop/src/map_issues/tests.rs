// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use qs_map_edit::uv::{FaceIndex, FaceRef};
use qs_map_edit::{
    BrushIndex, EntityIndex, MapFound, MapIssue, MapRevision, MapSearchParams, QuickFix,
};

use super::IssueBrowser;
use crate::map_editor::MapAction;
use crate::strings::{Text, brush_place, entity_place, face_place, fix_label, issue_line};
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
    let mut harness = theme.harness().build_ui_state(
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
    browser.set_issues(issues(), MapRevision::default());
    let fixed = after(browser, |harness| {
        harness.get_by_label(&fix_label(&rename())).click();
    });
    assert_eq!(fixed, vec![MapAction::Fix(rename())]);
    let mut browser = IssueBrowser::default();
    browser.set_issues(issues(), MapRevision::default());
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

// No spec statement: P6-19, P7-08: Find, or Enter in the box, asks for the search typed; a
// match selects its entity, a face match its face
#[test]
fn the_search_finds_and_selects() {
    let browser = IssueBrowser {
        query: "classname is \"light\"".to_owned(),
        ..IssueBrowser::default()
    };
    let found = after(browser, |harness| {
        harness.get_by_label(Text::Find.get()).click();
    });
    let asked = MapSearchParams {
        query: "classname is \"light\"".to_owned(),
    };
    assert_eq!(found, vec![MapAction::Find(asked)]);
    let face = FaceRef {
        entity: EntityIndex::new(0),
        brush: BrushIndex::new(2),
        face: FaceIndex::new(5),
    };
    let mut browser = IssueBrowser::default();
    browser.set_found(
        MapFound {
            entities: vec![EntityIndex::new(3)],
            brushes: vec![(EntityIndex::new(3), BrushIndex::new(0))],
            faces: vec![face],
        },
        MapRevision::default(),
    );
    let chosen = after(browser, |harness| {
        harness.get_by_label(&entity_place(3)).click();
        harness.get_by_label(&face_place(face)).click();
    });
    assert_eq!(
        chosen,
        vec![
            MapAction::Select(Some(EntityIndex::new(3))),
            MapAction::SelectFound(MapFound {
                faces: vec![face],
                ..MapFound::default()
            }),
        ]
    );
}

// No spec statement: P6-19: another map's issues and matches are forgotten
#[test]
fn clearing_forgets_both() {
    let mut browser = IssueBrowser::default();
    browser.set_issues(issues(), MapRevision::default());
    browser.set_found(MapFound::default(), MapRevision::default());
    browser.clear();
    assert!(browser.issues.is_none() && browser.found.is_none());
    assert!(after(browser, |_| {}).is_empty());
}

// No spec statement: P6-19: a brush match selects its entity
#[test]
fn a_brush_match_selects_its_entity() {
    let mut browser = IssueBrowser::default();
    browser.set_found(
        MapFound {
            entities: Vec::new(),
            brushes: vec![(EntityIndex::new(4), BrushIndex::new(1))],
            faces: Vec::new(),
        },
        MapRevision::default(),
    );
    let chosen = after(browser, |harness| {
        harness
            .get_by_label(&brush_place(EntityIndex::new(4), BrushIndex::new(1)))
            .click();
    });
    assert_eq!(chosen, vec![MapAction::Select(Some(EntityIndex::new(4)))]);
}

// No spec statement: P6-19: issues and matches found at one edit of the map are dropped once
// the map is at another, as after an undo
#[test]
fn an_edit_drops_the_lists() {
    let mut browser = IssueBrowser::default();
    let at = MapRevision::default();
    browser.set_issues(issues(), at);
    browser.follow(at);
    assert!(browser.issues.is_some(), "the same edits keep them");
    browser.follow(at.next());
    assert!(browser.issues.is_none() && browser.found.is_none() && browser.seen.is_none());
    browser.set_found(MapFound::default(), at.next());
    browser.set_issues(issues(), at.next());
    assert!(browser.issues.is_some() && browser.found.is_some());
}
