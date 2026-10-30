// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or a wrong popup is a failed test; offsets are small"
)]

use std::path::{Path, PathBuf};

use eframe::egui::{self, Event, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use qs_editor_view::find::Case;
use qs_editor_view::{Position, Selection};
use serde_json::json;

use super::{Editor, Popup, place_text};
use crate::App;
use crate::shell::EDITOR_AREA;
use crate::strings::{Text, match_count};

/// The test project: `defs.qc` declares `bounces` and the rest; `src/weapons.qc` uses them.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ember")
}

type H = Harness<'static, Option<App>>;

/// A headless window with `project` open and `file` in the editor, which has the keyboard.
fn harness_in(project: &Path, file: &str) -> H {
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .build_ui_state(
            |ui, app: &mut Option<App>| {
                if let Some(app) = app {
                    app.frame(ui);
                }
            },
            None,
        );
    let mut app = App::new(&harness.ctx, None).unwrap();
    app.shell_mut().open_project(project).unwrap();
    app.shell_mut()
        .execute("file.open", json!({ "path": project.join(file) }))
        .unwrap();
    *harness.state_mut() = Some(app);
    harness.run();
    let id = egui::Id::new(EDITOR_AREA);
    harness.ctx.memory_mut(|m| m.request_focus(id));
    harness.run();
    harness
}

fn harness(file: &str) -> H {
    harness_in(&fixture(), file)
}

fn app(harness: &H) -> &App {
    harness.state().as_ref().unwrap()
}

fn editor(harness: &H) -> &Editor {
    app(harness).shell().editor().unwrap()
}

fn editor_mut(harness: &mut H) -> &mut Editor {
    harness
        .state_mut()
        .as_mut()
        .unwrap()
        .shell_mut()
        .editor_mut()
        .unwrap()
}

fn popup(harness: &H) -> Option<Popup> {
    editor(harness).popup.clone()
}

fn text(harness: &H) -> String {
    editor(harness).view.state().buffer().text()
}

/// Puts the caret `plus` bytes into the `n`th `needle` of the editor's text.
fn caret_at(harness: &mut H, needle: &str, n: usize, plus: usize) {
    let editor = editor_mut(harness);
    let text = editor.view.state().buffer().text();
    let offset = text.match_indices(needle).nth(n).unwrap().0 + plus;
    let at = editor.view.state().buffer().position(offset);
    editor
        .view
        .state_mut()
        .set_selections(&[Selection::caret(at)]);
}

fn press(harness: &mut H, modifiers: Modifiers, key: Key) {
    harness.key_press_modifiers(modifiers, key);
    harness.run();
}

#[test]
fn typing_reaches_the_project_once_it_settles() {
    let mut harness = harness("src/weapons.qc");
    let before = app(&harness).shell().workbench().problems().len();
    caret_at(&mut harness, "ember.speed", 0, 0);
    harness.event(Event::Text("x".to_owned()));
    harness.step();
    let sent = |harness: &H| {
        let open = app(harness).shell().workbench().open().unwrap();
        String::from_utf8_lossy(open.text()).contains("xember.speed")
    };
    assert!(!sent(&harness), "not while typing");
    harness.run();
    assert!(sent(&harness), "once typing has paused");
    assert!(app(&harness).shell().workbench().problems().len() > before);
}

#[test]
fn the_editors_own_keys_undo_its_edits() {
    let mut harness = harness("src/weapons.qc");
    let original = text(&harness);
    caret_at(&mut harness, "ember.speed", 0, 0);
    harness.event(Event::Text("q".to_owned()));
    harness.run();
    press(&mut harness, Modifiers::COMMAND, Key::Z);
    assert_eq!(text(&harness), original);
    assert_eq!(app(&harness).shell().session().undo_depth(), 0);
}

#[test]
fn f12_goes_to_the_definition_and_says_when_there_is_none() {
    let mut harness = harness("src/weapons.qc");
    caret_at(&mut harness, "bounces", 0, 2);
    press(&mut harness, Modifiers::NONE, Key::F12);
    let (path, caret) = app(&harness).shell().caret().unwrap();
    assert_eq!(path, fixture().join("defs.qc"));
    assert_eq!(
        caret,
        Position::new(6, 7),
        "at `bounces` in `.float bounces;`"
    );
    caret_at(&mut harness, ";", 0, 1);
    press(&mut harness, Modifiers::NONE, Key::F12);
    assert_eq!(popup(&harness), Some(Popup::Note(Text::NoDefinition)));
    harness.get_by_label(Text::NoDefinition.get());
    press(&mut harness, Modifiers::NONE, Key::Escape);
    assert_eq!(popup(&harness), None);
}

#[test]
fn shift_f12_lists_the_references_and_a_click_opens_one() {
    let mut harness = harness("src/weapons.qc");
    caret_at(&mut harness, "bounces", 0, 0);
    press(&mut harness, Modifiers::SHIFT, Key::F12);
    let Some(Popup::References(places)) = popup(&harness) else {
        panic!("{:?}", popup(&harness));
    };
    assert_eq!(places.len(), 5, "the declaration and four uses: {places:?}");
    harness.get_by_label(Text::References.get());
    let declaration = places
        .iter()
        .find(|p| p.path == fixture().join("defs.qc"))
        .unwrap();
    let label = place_text(declaration, &fixture().join("src/weapons.qc"));
    assert_eq!(label, "defs.qc:7:8");
    harness.get_by_label(&label).click();
    harness.run();
    let (path, caret) = app(&harness).shell().caret().unwrap();
    assert_eq!(path, fixture().join("defs.qc"));
    assert_eq!(caret, Position::new(6, 7));
    assert_eq!(popup(&harness), None);
    caret_at(&mut harness, ";", 0, 1);
    press(&mut harness, Modifiers::SHIFT, Key::F12);
    assert_eq!(popup(&harness), Some(Popup::Note(Text::NoReferences)));
}

#[test]
fn a_reference_in_the_same_file_is_its_line_and_column() {
    let place = qs_workbench::Place {
        path: PathBuf::from("/p/a.qc"),
        start: 0,
        end: 1,
        location: serde_json::from_value(json!({ "line": 3, "column": 4 })).unwrap(),
    };
    assert_eq!(place_text(&place, Path::new("/p/a.qc")), ":3:4");
    assert_eq!(place_text(&place, Path::new("/p/b.qc")), "a.qc:3:4");
}

#[test]
fn ctrl_space_completes_the_name_being_typed() {
    let mut harness = harness("src/weapons.qc");
    caret_at(&mut harness, "ember.speed", 0, 0);
    harness.event(Event::Text("BOUNCE_S".to_owned()));
    harness.run();
    press(&mut harness, Modifiers::CTRL, Key::Space);
    let Some(Popup::Complete { candidates, .. }) = popup(&harness) else {
        panic!("{:?}", popup(&harness));
    };
    assert!(candidates.iter().any(|c| c.label == "BOUNCE_SPEED"));
    harness.get_by_label_contains("BOUNCE_SPEED");
    let shown = editor(&harness).matching(&candidates);
    assert!(
        !shown.is_empty() && shown.iter().all(|c| c.label.starts_with("BOUNCE_S")),
        "only the names the prefix matches: {shown:?}"
    );
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert!(text(&harness).contains("BOUNCE_SPEEDember.speed"));
    assert_eq!(popup(&harness), None);
}

#[test]
fn the_completion_popup_moves_with_the_arrows_and_closes_when_nothing_matches() {
    let mut harness = harness("src/weapons.qc");
    caret_at(&mut harness, "ember.speed", 0, 0);
    press(&mut harness, Modifiers::CTRL, Key::Space);
    press(&mut harness, Modifiers::NONE, Key::ArrowDown);
    press(&mut harness, Modifiers::NONE, Key::ArrowDown);
    press(&mut harness, Modifiers::NONE, Key::ArrowUp);
    let Some(Popup::Complete { selected, .. }) = popup(&harness) else {
        panic!("{:?}", popup(&harness));
    };
    assert_eq!(selected, 1);
    harness.event(Event::Text("zzzz".to_owned()));
    harness.run();
    assert_eq!(popup(&harness), None, "no candidate starts with zzzz");
}

#[test]
fn f2_renames_the_name_everywhere() {
    let mut harness = harness("src/weapons.qc");
    caret_at(&mut harness, "bounces", 0, 7);
    press(&mut harness, Modifiers::NONE, Key::F2);
    let Some(Popup::Rename { name, .. }) = popup(&harness) else {
        panic!("{:?}", popup(&harness));
    };
    assert_eq!(name, "bounces", "the name before the caret");
    harness.get_by_label(Text::RenameTo.get());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.event(Event::Text("hops".to_owned()));
    harness.run();
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert!(text(&harness).contains("self.hops = self.hops - 1;"));
    assert_eq!(
        app(&harness).shell().session().undo_depth(),
        1,
        "a rename is a command"
    );
}

#[test]
fn a_refused_rename_says_so() {
    let mut harness = harness("src/weapons.qc");
    caret_at(&mut harness, "bounces", 0, 7);
    press(&mut harness, Modifiers::NONE, Key::F2);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.event(Event::Text("9x".to_owned()));
    harness.run();
    press(&mut harness, Modifiers::NONE, Key::Enter);
    assert_eq!(popup(&harness), Some(Popup::Note(Text::RenameFailed)));
}

#[test]
fn resting_on_a_name_shows_its_declaration() {
    let mut harness = harness("src/weapons.qc");
    caret_at(&mut harness, "bounces", 0, 2);
    harness.run();
    let caret = editor(&harness).view.caret_rect().unwrap();
    harness.event(Event::PointerMoved(caret.center()));
    // The popup fades in, repainting as it does.
    harness.run_steps(6);
    let Some(Popup::Hover { text, .. }) = popup(&harness) else {
        panic!("{:?}", popup(&harness));
    };
    assert!(text.contains(".float bounces"), "{text}");
    // Moving off the name closes it.
    harness.event(Event::PointerMoved(egui::pos2(1.0, 1.0)));
    harness.run_steps(2);
    harness.event(Event::PointerMoved(caret.center()));
    harness.step();
    assert_eq!(popup(&harness), None);
}

#[test]
fn cmd_f_finds_and_replaces() {
    let mut harness = harness("src/weapons.qc");
    // The find bar's fields animate their scrolling, so each step runs a few frames.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    harness.run_steps(3);
    editor_mut(&mut harness).find.as_mut().unwrap().query = "bounces".to_owned();
    harness.run_steps(3);
    harness.get_by_label(&match_count(4));
    harness.get_by_label(Text::FindNext.get()).click();
    harness.run_steps(3);
    assert_eq!(
        editor(&harness).view.state().primary().start(),
        Position::new(4, 6)
    );
    harness.get_by_label(Text::FindPrevious.get()).click();
    harness.run_steps(3);
    editor_mut(&mut harness).find.as_mut().unwrap().with = "hops".to_owned();
    harness.get_by_label(Text::ReplaceAll.get()).click();
    harness.run_steps(3);
    assert_eq!(text(&harness).matches("hops").count(), 4);
    editor_mut(&mut harness).find.as_mut().unwrap().query = "HOPS".to_owned();
    harness.get_by_label(Text::MatchCase.get()).click();
    harness.run_steps(3);
    assert_eq!(editor(&harness).find.as_ref().unwrap().case, Case::Match);
    harness.get_by_label(&match_count(0));
    editor_mut(&mut harness).find.as_mut().unwrap().with = "x".to_owned();
    harness.get_by_label(Text::Replace.get()).click();
    harness.run_steps(3);
    assert_eq!(text(&harness).matches("hops").count(), 4, "nothing matches");
    let id = egui::Id::new(EDITOR_AREA);
    harness.ctx.memory_mut(|m| m.request_focus(id));
    harness.run_steps(1);
    harness.key_press(Key::Escape);
    harness.run_steps(3);
    assert!(editor(&harness).find.is_none());
}

#[test]
fn cmd_s_writes_the_file() {
    let dir = std::env::temp_dir().join(format!("quakec-studio-save-{}", std::process::id()));
    let _ignored = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    for file in ["progs.src", "defs.qc", "src/world.qc", "src/weapons.qc"] {
        std::fs::copy(fixture().join(file), dir.join(file)).unwrap();
    }
    let mut harness = harness_in(&dir, "src/world.qc");
    harness.event(Event::Text("// saved ".to_owned()));
    harness.step();
    // Saved in the frame typing settles in: the text is sent before it is saved.
    harness.step();
    press(&mut harness, Modifiers::COMMAND, Key::S);
    let saved = std::fs::read_to_string(dir.join("src/world.qc")).unwrap();
    let _ignored = std::fs::remove_dir_all(&dir);
    assert!(saved.starts_with("// saved "), "{saved}");
}

#[test]
fn a_rename_elsewhere_gives_the_editor_its_new_text() {
    let mut harness = harness("defs.qc");
    let offset = text(&harness).find("bounces").unwrap();
    harness
        .state_mut()
        .as_mut()
        .unwrap()
        .shell_mut()
        .execute(
            "symbol.rename",
            json!({ "path": fixture().join("defs.qc"), "offset": offset, "name": "hops" }),
        )
        .unwrap();
    harness.run();
    assert!(text(&harness).contains(".float hops;"));
}

#[test]
fn another_files_edits_are_kept_when_the_editor_moves_on() {
    let mut harness = harness("src/world.qc");
    harness.event(Event::Text("// kept ".to_owned()));
    harness.step();
    harness.get_by_label("defs.qc").click();
    harness.run();
    harness.get_by_label("world.qc").click();
    harness.run();
    assert!(text(&harness).starts_with("// kept "));
}
