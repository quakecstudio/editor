// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or a wrong popup is a failed test; offsets are small"
)]

use std::path::{Path, PathBuf};

use eframe::egui::accesskit::Role;
use eframe::egui::{self, Event, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable};
use qs_editor_view::find::Case;
use qs_editor_view::{Position, Selection, Styler};
use serde_json::json;

use super::{Editor, Popup};
use crate::App;
use crate::agent::AgentPanel;
use crate::fallback::SystemFonts;
use crate::icons::Icon;
use crate::multibuffer::Source;
use crate::shell::CentreTab;
use crate::shell::{EDITOR_AREA, view_style};
use crate::strings::{Text, file_count, match_count};
use crate::theme::{Mode, Theme, Token};

/// The test project: `defs.qc` declares `bounces` and the rest; `src/weapons.qc` uses them.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ember")
}

type H = Harness<'static, Option<App>>;

/// A headless window with `project` open and `file` in the editor, which has the keyboard.
fn harness_in(project: &Path, file: &str) -> H {
    harness_stepping(project, file, 0.25)
}

/// [`harness_in`] whose steps are `dt` seconds apart.
fn harness_stepping(project: &Path, file: &str, dt: f32) -> H {
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_step_dt(dt)
        .build_ui_state(
            |ui, app: &mut Option<App>| {
                if let Some(app) = app {
                    app.frame(ui);
                }
            },
            None,
        );
    let mut app = App::checked_on_the_frame(&harness.ctx, None).unwrap();
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

// No spec statement: P4-33, UI-17: Shift+F12 shows every declaration and use of the name as
// excerpts of their files in the multibuffer, and Open goes to one
#[test]
fn shift_f12_shows_the_references_as_excerpts_and_open_goes_to_one() {
    let mut harness = harness("src/weapons.qc");
    caret_at(&mut harness, "bounces", 0, 0);
    press(&mut harness, Modifiers::SHIFT, Key::F12);
    harness.run();
    let shell = app(&harness).shell();
    assert_eq!(shell.centre_tab(), CentreTab::Search);
    let buffer = shell.multibuffer();
    assert_eq!(buffer.source, Source::References);
    assert_eq!(buffer.found, (5, false), "the declaration and four uses");
    let defs = fixture().join("defs.qc");
    let declaration = buffer
        .excerpts
        .iter()
        .find(|e| e.path == defs)
        .unwrap()
        .clone();
    assert!(
        declaration.text.contains(".float bounces;"),
        "{declaration:?}"
    );
    harness.get_by_label(Text::References.get());
    harness
        .get_all_by_label(Text::OpenExcerpt.get())
        .next()
        .unwrap();
    let index = buffer.excerpts.iter().position(|e| e.path == defs).unwrap();
    harness
        .get_all_by_label(Text::OpenExcerpt.get())
        .nth(index)
        .unwrap()
        .click();
    harness.run();
    let (path, caret) = app(&harness).shell().caret().unwrap();
    assert_eq!(path, defs);
    assert_eq!(
        caret,
        Position::new(6, 7),
        "at `bounces` in `.float bounces;`"
    );
    caret_at(&mut harness, ";", 0, 1);
    press(&mut harness, Modifiers::SHIFT, Key::F12);
    assert_eq!(popup(&harness), Some(Popup::Note(Text::NoReferences)));
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
    // The popup's label and its field are both named for the rename.
    harness.get_by_role_and_label(Role::TextInput, Text::RenameTo.get());
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
    harness
        .get_by_role_and_label(Role::Button, Text::Replace.get())
        .click();
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
    // The tab of the unsaved file, not its row in the Project tree, which is marked M.
    let tab = format!("world.qc {}", Text::UnsavedMark.get());
    let row = format!("world.qc {}", Text::ModifiedMark.get());
    assert!(
        harness.query_by_label(&row).is_some(),
        "the tree marks it unsaved"
    );
    harness
        .get_by_role_and_label(egui::accesskit::Role::Tab, &tab)
        .click();
    harness.run();
    assert!(text(&harness).starts_with("// kept "));
    // The tab's own editor came back: the edit is still undoable.
    assert!(editor_mut(&mut harness).view.state_mut().undo());
}

#[test]
fn the_colours_follow_each_check_of_the_text() {
    let mut harness = harness("src/world.qc");
    let before = Styler::revision(&editor(&harness).styler);
    caret_at(&mut harness, "self.health", 0, 0);
    harness.event(Event::Text("return; ".to_owned()));
    harness.run();
    let open = app(&harness).shell().workbench().open().unwrap();
    let styler = &editor(&harness).styler;
    assert_ne!(Styler::revision(styler), before);
    assert_eq!(Styler::revision(styler), open.revision());
    let line = text(&harness)
        .lines()
        .position(|l| l.contains("return;"))
        .unwrap();
    let spans = Styler::spans(styler, line, "");
    let keyword = Theme::load(Mode::Dark).unwrap().color(Token::CodeKeyword);
    assert!(spans.iter().any(|s| s.color == keyword), "{spans:?}");
}

#[test]
fn a_hover_waits_for_the_pointer_to_rest() {
    let mut harness = harness("src/weapons.qc");
    caret_at(&mut harness, "bounces", 0, 2);
    harness.run();
    let on_bounces = editor(&harness).view.caret_rect().unwrap().center();
    caret_at(&mut harness, "velocity", 0, 2);
    harness.run();
    let on_velocity = editor(&harness).view.caret_rect().unwrap().center();
    // Each step is a quarter of a second; the first sees the pointer arrive.
    harness.event(Event::PointerMoved(on_bounces));
    harness.step();
    harness.step();
    assert_eq!(
        popup(&harness),
        None,
        "not after a quarter of a second at rest"
    );
    // Moving to another name starts the wait again.
    harness.event(Event::PointerMoved(on_velocity));
    harness.step();
    harness.step();
    assert_eq!(popup(&harness), None, "the wait restarted at the new name");
    harness.step();
    harness.step();
    let Some(Popup::Hover { at, text }) = popup(&harness) else {
        panic!("{:?}", popup(&harness));
    };
    assert!(text.contains(".vector velocity"), "{text}");
    harness.run_steps(4);
    let shown = harness.get_by_label_contains(".vector velocity").rect();
    assert!(
        shown.top() > at.y && shown.top() < at.y + 40.0,
        "just below the pointer: {shown:?} for {at:?}"
    );
}

#[test]
fn the_picked_candidate_is_drawn_selected() {
    let mut harness = harness("src/weapons.qc");
    caret_at(&mut harness, "ember.speed", 0, 0);
    harness.event(Event::Text("bo".to_owned()));
    harness.run();
    press(&mut harness, Modifiers::CTRL, Key::Space);
    let toggled = |harness: &H, label: &str| {
        harness
            .get_by_label_contains(label)
            .accesskit_node()
            .toggled()
            == Some(egui::accesskit::Toggled::True)
    };
    // `bo` matches `bounces` and `BOUNCE_SPEED`, case aside, in that order.
    let Some(Popup::Complete { candidates, .. }) = popup(&harness) else {
        panic!("{:?}", popup(&harness));
    };
    let shown: Vec<String> = editor(&harness)
        .matching(&candidates)
        .iter()
        .map(|c| c.label.clone())
        .collect();
    assert_eq!(shown, ["bounces", "BOUNCE_SPEED"]);
    assert!(toggled(&harness, "bounces"));
    assert!(!toggled(&harness, "BOUNCE_SPEED"));
    press(&mut harness, Modifiers::NONE, Key::ArrowDown);
    assert!(!toggled(&harness, "bounces"));
    assert!(toggled(&harness, "BOUNCE_SPEED"));
}

#[test]
fn the_find_field_finds_on_enter_only() {
    let mut harness = harness("src/weapons.qc");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::F);
    harness.run_steps(3);
    harness.get_by_label(Text::Find.get()).click();
    harness.run_steps(2);
    harness.event(Event::Text("bounces".to_owned()));
    harness.run_steps(2);
    let start = editor(&harness).view.state().primary();
    // Leaving the field without Enter finds nothing.
    harness.get_by_label(Text::MatchCase.get()).click();
    harness.run_steps(2);
    assert_eq!(editor(&harness).view.state().primary(), start);
    harness.get_by_label(Text::Find.get()).click();
    harness.run_steps(2);
    harness.key_press(Key::Enter);
    harness.run_steps(3);
    assert_ne!(editor(&harness).view.state().primary(), start);
}

#[test]
fn switching_files_sends_the_edits_at_once() {
    // Steps of 50 ms, so the switch comes well before typing could settle.
    let mut harness = harness_stepping(&fixture(), "src/world.qc", 0.05);
    let world = fixture().join("src/world.qc");
    let in_world = |harness: &H| {
        let problems = app(harness).shell().workbench().problems();
        problems.iter().filter(|p| p.path == world).count()
    };
    let before = in_world(&harness);
    harness.event(Event::Text("float = 1; ".to_owned()));
    harness.step();
    harness.step();
    assert_eq!(
        in_world(&harness),
        before,
        "not sent while the editor stays"
    );
    harness.get_by_label("defs.qc").click();
    for _ in 0..3 {
        harness.step();
    }
    assert_eq!(
        app(&harness).shell().workbench().open().unwrap().path(),
        fixture().join("defs.qc")
    );
    assert!(in_world(&harness) > before, "sent when the editor moved on");
}

#[test]
fn focus_editor_gives_the_editor_the_keyboard() {
    let mut harness = harness("src/world.qc");
    let id = egui::Id::new(EDITOR_AREA);
    harness.ctx.memory_mut(|m| m.surrender_focus(id));
    harness.run();
    assert!(!harness.ctx.memory(|m| m.has_focus(id)));
    let ctx = harness.ctx.clone();
    app(&harness).shell().focus_editor(&ctx);
    harness.run();
    assert!(harness.ctx.memory(|m| m.has_focus(id)));
}

/// Plex Sans as the only bundled font, and Lucide's icon font as the "system"'s only one.
fn icons_from_the_system() -> SystemFonts {
    const PLEX: &[u8] = include_bytes!("../../assets/fonts/IBMPlexSans-Regular.ttf");
    const LUCIDE: &[u8] = include_bytes!("../../assets/fonts/lucide.ttf");
    SystemFonts::with_files(&[PLEX], &[LUCIDE])
}

#[test]
fn a_character_typed_that_no_bundled_font_has_gets_a_system_font() {
    let mut harness = harness("src/world.qc");
    let shell = harness.state_mut().as_mut().unwrap().shell_mut();
    shell.set_system_fonts(icons_from_the_system());
    harness.run();
    assert_eq!(
        app(&harness).shell().system_fonts_added(),
        0,
        "the file has none"
    );
    harness.event(Event::Text(Icon::Play.char().to_string()));
    harness.run();
    assert_eq!(app(&harness).shell().system_fonts_added(), 1);
}

#[test]
fn a_file_opened_with_such_a_character_gets_a_system_font() {
    let dir = std::env::temp_dir().join(format!("quakec-studio-glyph-{}", std::process::id()));
    let _ignored = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let icon = Icon::Play.char();
    std::fs::write(dir.join("progs.src"), "progs.dat\na.qc\nb.qc\n").unwrap();
    std::fs::write(dir.join("a.qc"), format!("float a;\n// {icon}\n")).unwrap();
    std::fs::write(dir.join("b.qc"), format!("float b;\n// {icon}\n")).unwrap();
    let mut harness = harness_in(&dir, "a.qc");
    let shell = harness.state_mut().as_mut().unwrap().shell_mut();
    shell.set_system_fonts(icons_from_the_system());
    // An edit on a.qc's first line looks at that line alone, which has no icon.
    caret_at(&mut harness, "float", 0, 0);
    harness.event(Event::Text("x".to_owned()));
    harness.run();
    assert_eq!(app(&harness).shell().system_fonts_added(), 0);
    // Another file is looked at whole: its icon is on its second line.
    harness.get_by_label("b.qc").click();
    harness.run();
    let _ignored = std::fs::remove_dir_all(&dir);
    assert_eq!(app(&harness).shell().system_fonts_added(), 1);
}

#[test]
fn the_editors_colours_are_the_themes() {
    let light = Theme::load(Mode::Light).unwrap();
    let style = view_style(&light);
    assert_eq!(style.background, light.color(Token::SurfaceEditor));
    assert_eq!(style.text, light.color(Token::TextCode));
    assert_eq!(style.removed_text, light.color(Token::TextOnError));
    assert_ne!(
        style,
        qs_editor_view::ViewStyle::default(),
        "not the dark defaults"
    );
}

// No spec statement: the breadcrumb names the file's folders, the file and the definition the
// caret is in, which the Symbols list names too
#[test]
fn the_breadcrumb_follows_the_caret() {
    let mut harness = harness("src/weapons.qc");
    let count = |harness: &H, name: &str| harness.query_all_by_label(name).count();
    caret_at(&mut harness, "ember.bounces", 0, 0);
    harness.run();
    assert_eq!(
        count(&harness, "W_FireEmber"),
        2,
        "the list and the breadcrumb"
    );
    assert_eq!(count(&harness, "Ember_Touch"), 1, "the list only");
    assert_eq!(count(&harness, "src"), 2, "the tree and the breadcrumb");
    caret_at(&mut harness, "Ember_Touch", 0, 0);
    harness.run();
    assert_eq!(count(&harness, "W_FireEmber"), 1);
    assert_eq!(count(&harness, "Ember_Touch"), 2);
}

// UI-14: Cmd+P opens the file finder over the project's files; typing narrows it to the files
// whose name or path holds the letters, Enter opens the first, and Escape closes it
#[test]
fn the_file_finder_opens_a_file_by_name() {
    let mut harness = harness("src/weapons.qc");
    press(&mut harness, Modifiers::COMMAND, Key::P);
    harness.run();
    assert!(app(&harness).shell().finder.is_open());
    assert!(harness.query_by_label(&file_count(3, 3)).is_some());
    harness.event(Event::Text("wrld".to_owned()));
    harness.run();
    assert!(harness.query_by_label(&file_count(1, 3)).is_some());
    press(&mut harness, Modifiers::NONE, Key::Enter);
    harness.run();
    assert!(!app(&harness).shell().finder.is_open());
    let open = app(&harness)
        .shell()
        .workbench()
        .open()
        .unwrap()
        .path()
        .to_path_buf();
    assert!(open.ends_with("src/world.qc"), "{}", open.display());
    press(&mut harness, Modifiers::COMMAND, Key::P);
    harness.run();
    harness.event(Event::Text("zzz".to_owned()));
    harness.run();
    assert!(harness.query_by_label(Text::NoFileMatch.get()).is_some());
    press(&mut harness, Modifiers::NONE, Key::Escape);
    harness.run();
    assert!(!app(&harness).shell().finder.is_open());
}

// UI-14: before anything is typed the finder lists the files opened most recently first and,
// as Zed's does, selects the one before the file being edited; `name:line` opens the file at
// that line
#[test]
fn the_finder_starts_from_recent_files_and_reads_a_line() {
    let mut harness = harness("src/weapons.qc");
    harness.get_by_label("world.qc").click();
    harness.run();
    press(&mut harness, Modifiers::COMMAND, Key::P);
    harness.run();
    // world.qc is being edited and listed first; Enter opens weapons.qc, the one before it.
    press(&mut harness, Modifiers::NONE, Key::Enter);
    harness.run();
    let open = app(&harness)
        .shell()
        .workbench()
        .open()
        .unwrap()
        .path()
        .to_path_buf();
    assert!(open.ends_with("src/weapons.qc"), "{}", open.display());
    press(&mut harness, Modifiers::COMMAND, Key::P);
    harness.run();
    harness.event(Event::Text("world:3".to_owned()));
    harness.run();
    press(&mut harness, Modifiers::NONE, Key::Enter);
    harness.run();
    harness.run();
    let shell = app(&harness).shell();
    assert!(
        shell
            .workbench()
            .open()
            .unwrap()
            .path()
            .ends_with("src/world.qc")
    );
    assert_eq!(shell.caret().unwrap().1.line, 2, "line 3, from 0");
}

// No spec statement: a query's last colon and a line number after it ask for that line; any
// other colon is part of the name
#[test]
fn a_finder_query_may_end_in_a_line() {
    use crate::finder::split_line;
    assert_eq!(split_line("world.qc:12"), ("world.qc", Some(12)));
    assert_eq!(split_line("world.qc: 7"), ("world.qc", Some(7)));
    assert_eq!(split_line("world.qc:0"), ("world.qc:0", None));
    assert_eq!(split_line("a:b"), ("a:b", None));
    assert_eq!(split_line("world"), ("world", None));
}

/// A model that answers every request with the same text.
struct Fixed(&'static str);

impl qs_agent::Model for Fixed {
    fn reply(
        &mut self,
        _: &str,
        _: &[qs_agent::ToolSpec],
        _: &[qs_agent::Message],
    ) -> Result<qs_agent::Reply, String> {
        Ok(qs_agent::Reply::Finish {
            summary: self.0.to_owned(),
        })
    }
}

/// The harness of `file` whose agent answers every inline edit with `answer`.
fn inline_harness(file: &str, answer: &'static str) -> H {
    let mut harness = harness(file);
    let shell = harness.state_mut().as_mut().unwrap().shell_mut();
    *shell.agent_mut() = AgentPanel::new(Box::new(move |_| {
        let model: Box<dyn qs_agent::Model + Send> = Box::new(Fixed(answer));
        Ok(model)
    }));
    shell.agent_mut().set_config(qs_models::ProviderConfig::new(
        qs_models::ProviderKind::Ollama,
        "local",
    ));
    harness
}

/// Selects line `line`, from 0, after its leading tab, and asks for an inline edit of it.
fn ask_inline(harness: &mut H, line: usize, request: &str) {
    let text = editor(harness).view.state().buffer().text();
    let length = text.lines().nth(line).unwrap().len();
    editor_mut(harness)
        .view
        .state_mut()
        .set_selections(&[Selection {
            anchor: Position::new(line, 1),
            head: Position::new(line, length),
        }]);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::K);
    harness.run_steps(2);
    assert!(
        matches!(popup(harness), Some(Popup::InlinePrompt(_))),
        "{:?}",
        popup(harness)
    );
    harness.event(Event::Text(request.to_owned()));
    harness.run_steps(2);
    harness.key_press(Key::Enter);
    harness.run_steps(2);
    // The fake model may have answered already; otherwise the editor waits for it.
    assert!(matches!(
        popup(harness),
        Some(Popup::InlineWaiting | Popup::InlineProposed)
    ));
    harness
        .state_mut()
        .as_mut()
        .unwrap()
        .shell_mut()
        .wait_inline();
    harness.run_steps(2);
}

// No spec statement: P4-10, the acceptance criterion with a fake model: Cmd+K on a selection
// asks for an edit, its answer replaces the selection drawn as a diff in place, Accept keeps
// it, and one undo then restores the original
#[test]
fn an_inline_edit_is_a_diff_in_place_undone_in_one_step() {
    let mut harness = inline_harness("src/weapons.qc", "self.bounces = 0;");
    let original = editor(&harness).view.state().buffer().text();
    ask_inline(&mut harness, 4, "reset the bounces");
    assert_eq!(popup(&harness), Some(Popup::InlineProposed));
    let proposed = editor(&harness).view.state().buffer().text();
    assert_eq!(proposed.lines().nth(4), Some("\tself.bounces = 0;"));
    assert!(
        editor_mut(&mut harness)
            .view
            .diff()
            .is_some_and(|d| !d.is_empty())
    );
    harness.get_by_label(Text::AgentAccept.get()).click();
    harness.run_steps(2);
    assert_eq!(popup(&harness), None);
    assert!(
        editor_mut(&mut harness).view.diff().is_none(),
        "the diff is gone"
    );
    assert_eq!(editor(&harness).view.state().buffer().text(), proposed);
    assert!(editor_mut(&mut harness).view.state_mut().undo());
    assert_eq!(editor(&harness).view.state().buffer().text(), original);
}

// No spec statement: P4-10, Reject takes an inline edit's answer back, and an edit asked with
// no provider set up says why
#[test]
fn an_inline_edit_is_rejected_or_refused() {
    let mut harness = inline_harness("src/weapons.qc", "self.bounces = 0;");
    let original = editor(&harness).view.state().buffer().text();
    ask_inline(&mut harness, 4, "reset the bounces");
    harness.get_by_label(Text::AgentReject.get()).click();
    harness.run_steps(2);
    assert_eq!(editor(&harness).view.state().buffer().text(), original);
    assert!(editor_mut(&mut harness).view.diff().is_none());
    let mut unset = harness_in(&fixture(), "src/weapons.qc");
    editor_mut(&mut unset)
        .view
        .state_mut()
        .set_selections(&[Selection {
            anchor: Position::new(4, 1),
            head: Position::new(4, 5),
        }]);
    unset.key_press_modifiers(Modifiers::COMMAND, Key::K);
    unset.run_steps(2);
    unset.event(Event::Text("x".to_owned()));
    unset.run_steps(2);
    unset.key_press(Key::Enter);
    unset.run_steps(3);
    assert!(
        matches!(popup(&unset), Some(Popup::InlineFailed(_))),
        "{:?}",
        popup(&unset)
    );
}
