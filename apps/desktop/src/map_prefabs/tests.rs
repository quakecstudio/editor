// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::PathBuf;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use qs_workbench::map_edit::Prefab;

use super::PrefabBrowser;
use crate::map_editor::MapAction;
use crate::strings::{Text, prefab_count};
use crate::theme::{Mode, Theme};

/// The browser's header and body, after `act`; what they asked for and the harness.
fn shown(
    browser: PrefabBrowser,
    act: impl FnOnce(&mut Harness<'_, (PrefabBrowser, Vec<MapAction>)>),
) -> Vec<MapAction> {
    let theme = Theme::load(Mode::Dark).unwrap();
    let mut harness = Harness::new_ui_state(
        |ui, (browser, actions): &mut (PrefabBrowser, Vec<MapAction>)| {
            actions.extend(browser.header(ui, &theme));
            browser.body(ui, &theme);
        },
        (browser, Vec::new()),
    );
    harness.run();
    act(&mut harness);
    harness.run();
    harness.state().1.clone()
}

// No spec statement: P6-23: the browser asks for the prefabs until it has them, then lists them
// with their count; Refresh asks again
#[test]
fn the_browser_lists_and_refreshes() {
    let asked = shown(PrefabBrowser::default(), |_| {});
    assert!(asked.iter().all(|a| *a == MapAction::ListPrefabs) && !asked.is_empty());
    let mut browser = PrefabBrowser::default();
    browser.set_prefabs(vec![Prefab {
        name: "pillar".to_owned(),
        path: PathBuf::from("prefabs/pillar.map"),
    }]);
    let mut seen = false;
    let refreshed = shown(browser, |harness| {
        seen = harness.query_by_label("pillar").is_some()
            && harness.query_by_label(&prefab_count(1)).is_some();
        harness.state_mut().1.clear();
        harness.get_by_label(Text::Refresh.get()).click();
    });
    assert!(seen, "the prefab and its count are shown");
    assert_eq!(refreshed, vec![MapAction::ListPrefabs]);
    let mut empty = PrefabBrowser::default();
    empty.set_prefabs(Vec::new());
    let mut said = false;
    let none = shown(empty, |harness| {
        said = harness.query_by_label(Text::MapNoPrefabs.get()).is_some();
    });
    assert!(said);
    assert!(none.is_empty());
}
