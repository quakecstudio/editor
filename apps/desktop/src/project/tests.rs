// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test; lines and columns start at 1"
)]

use std::path::{Path, PathBuf};

use eframe::egui::{self, Key};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use qs_editor_view::Position;
use qs_workbench::Location;

use super::{counts, problem_text};
use crate::App;
use crate::icons::Icon;
use crate::shell::EDITOR_AREA;
use crate::strings::Text;

/// The test project: two problems in `src/weapons.qc`, an error each, and a warning in
/// `src/world.qc`.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ember")
}

/// A headless window with the test project open.
fn harness() -> Harness<'static, Option<App>> {
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
    app.shell_mut().open_project(&fixture()).unwrap();
    *harness.state_mut() = Some(app);
    harness.run();
    harness
}

fn app<'h>(harness: &'h Harness<'static, Option<App>>) -> &'h App {
    harness.state().as_ref().unwrap()
}

/// The caret `location` is at, in the editor's lines and byte columns, for ASCII text.
fn position(location: Location) -> Position {
    Position::new(
        usize::try_from(location.line() - 1).unwrap(),
        usize::try_from(location.column() - 1).unwrap(),
    )
}

// P2-19: every problem is listed, and choosing one opens its file with the caret at the start
// of its span.
#[test]
fn choosing_a_problem_opens_its_file_at_its_place() {
    let mut harness = harness();
    let problems = app(&harness).shell().workbench().problems().to_vec();
    assert_eq!(problems.len(), 3, "{problems:?}");
    let root = fixture();
    for problem in &problems {
        let label = problem_text(problem, Some(&root));
        harness.get_by_label(&label).click();
        harness.run();
        let shell = app(&harness).shell();
        let open = shell.workbench().open().unwrap();
        assert_eq!(open.path(), problem.path);
        assert_eq!(open.offset(), problem.diagnostic.span.start);
        let (path, caret) = shell.caret().unwrap();
        assert_eq!(path, problem.path);
        assert_eq!(caret, position(problem.location), "{label}");
    }
}

// P2-19: choosing the same problem again moves the caret back to it.
#[test]
fn choosing_a_problem_again_moves_the_caret_back() {
    let mut harness = harness();
    let problem = app(&harness).shell().workbench().problems()[0].clone();
    let label = problem_text(&problem, Some(&fixture()));
    harness.get_by_label(&label).click();
    harness.run();
    harness.key_press(Key::ArrowDown);
    harness.run();
    harness.get_by_label(&label).click();
    harness.run();
    let (_, caret) = app(&harness).shell().caret().unwrap();
    assert_eq!(caret, position(problem.location));
}

// P2-19: the tree lists the project's files under their directories, and choosing one opens it
// at its start.
#[test]
fn choosing_a_file_in_the_tree_opens_it() {
    let mut harness = harness();
    for name in ["ember", "src", "defs.qc", "world.qc", "weapons.qc"] {
        harness.get_by_label(name);
    }
    harness.get_by_label("world.qc").click();
    harness.run();
    let (path, caret) = app(&harness).shell().caret().unwrap();
    assert_eq!(path, fixture().join("src/world.qc"));
    assert_eq!(caret, Position::new(0, 0));
}

// P2-19: the Symbols section lists the open file's symbols, or the project's, and choosing one
// opens its definition.
#[test]
fn choosing_a_symbol_opens_its_definition() {
    let mut harness = harness();
    harness.get_by_label(Text::NoSymbols.get());
    harness.get_by_label("defs.qc").click();
    harness.run();
    for name in ["self", "time", "health", "BOUNCE_SPEED", "remove"] {
        harness.get_by_label(name);
    }
    assert!(harness.query_by_label("W_FireEmber").is_none());
    harness.get_by_label("health").click();
    harness.run();
    let (_, caret) = app(&harness).shell().caret().unwrap();
    assert_eq!(
        caret,
        Position::new(5, 7),
        "at `health` in `.float health;`"
    );

    harness.get_by_label(Text::SymbolsAll.get()).click();
    harness.run();
    harness.get_by_label("W_FireEmber").click();
    harness.run();
    let (path, caret) = app(&harness).shell().caret().unwrap();
    assert_eq!(path, fixture().join("src/weapons.qc"));
    assert_eq!(caret, Position::new(10, 7));
}

// P2-19: the Problems tab and the status bar count the problems.
#[test]
fn the_problems_are_counted() {
    let harness = harness();
    harness.get_by_label(&format!("{} 3", Text::Problems.get()));
    harness.get_by_label(&format!("{} 2", Icon::Error.char()));
    harness.get_by_label(&format!("{} 1", Icon::Warning.char()));
    let problems = app(&harness).shell().workbench().problems();
    assert_eq!(counts(problems), (2, 1));
    assert_eq!(counts(&[]), (0, 0));
}

// P2-19: a problem's row is its icon, code and message, then its place under the project.
#[test]
fn a_problems_row_says_what_and_where() {
    let harness = harness();
    let problems = app(&harness).shell().workbench().problems();
    let root = fixture();
    let error = problems
        .iter()
        .find(|p| p.diagnostic.message.contains("speed"))
        .unwrap();
    assert_eq!(
        problem_text(error, Some(&root)),
        format!(
            "{} E0301 Unknown value \"speed\"  {}:15:8",
            Icon::Error.char(),
            Path::new("src").join("weapons.qc").display()
        )
    );
    let warning = problems
        .iter()
        .find(|p| p.diagnostic.code.as_str() == "W0201")
        .unwrap();
    assert!(
        problem_text(warning, None).starts_with(&format!("{} W0201", Icon::Warning.char())),
        "{}",
        problem_text(warning, None)
    );
    assert!(
        problem_text(warning, None).ends_with(&format!("{}:5:14", warning.path.display())),
        "a file outside the root keeps its whole path"
    );
}

// P2-19: the open file's row in the tree is drawn selected, and no other.
#[test]
fn the_open_file_is_selected_in_the_tree() {
    let mut harness = harness();
    harness.get_by_label("world.qc").click();
    harness.run();
    let selected = |harness: &Harness<'static, Option<App>>, name: &str| {
        harness.get_by_label(name).accesskit_node().toggled()
            == Some(egui::accesskit::Toggled::True)
    };
    assert!(selected(&harness, "world.qc"));
    assert!(!selected(&harness, "weapons.qc"));
    assert!(!selected(&harness, "defs.qc"));
}

// P2-19: opening the file the editor already shows moves its caret and keeps its text, edits
// included.
#[test]
fn opening_the_shown_file_again_keeps_its_edits() {
    let mut harness = harness();
    harness.get_by_label("weapons.qc").click();
    harness.run();
    harness.get_by_label(Text::Editor.get()).click();
    harness.run();
    let id = egui::Id::new(EDITOR_AREA);
    harness.ctx.memory_mut(|m| m.request_focus(id));
    harness.run();
    harness.event(egui::Event::Text("zz".to_owned()));
    harness.run();
    let problem = app(&harness).shell().workbench().problems()[0].clone();
    harness
        .get_by_label(&problem_text(&problem, Some(&fixture())))
        .click();
    harness.run();
    let shell = app(&harness).shell();
    assert_eq!(shell.caret().unwrap().1, position(problem.location));
    assert!(shell.editor_text().unwrap().starts_with("zz"));
}

// P2-19: a long tree and a long symbol list each scroll in their share of the left panel, so
// the sections below them stay in the window.
#[test]
fn long_lists_leave_room_for_the_sections_below() {
    let dir = std::env::temp_dir().join(format!("quakec-studio-long-{}", std::process::id()));
    let _ignored = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut src = String::from("progs.dat\n");
    for n in 0..80 {
        let name = format!("f{n:02}.qc");
        std::fs::write(dir.join(&name), format!("float g{n}a;\nfloat g{n}b;\n")).unwrap();
        src.push_str(&name);
        src.push('\n');
    }
    std::fs::write(dir.join("progs.src"), src).unwrap();
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
    app.shell_mut().open_project(&dir).unwrap();
    *harness.state_mut() = Some(app);
    harness.run();
    harness.get_by_label(Text::SymbolsAll.get()).click();
    harness.run();
    let symbols = harness.get_by_label(Text::Symbols.get()).rect();
    let entities = harness.get_by_label(Text::Entities.get()).rect();
    let _ignored = std::fs::remove_dir_all(&dir);
    assert!(symbols.bottom() < 900.0 * 0.6, "{symbols:?}");
    // The Entities heading and its two lines of note, about 50 points, fit above the status
    // bar at 866.
    assert!(entities.bottom() + 50.0 < 866.0, "{entities:?}");
}
