// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap is a failed test; line numbers and depths are small, \
              and sizes are compared exactly as set"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use eframe::egui::accesskit::{Role, Toggled};
use eframe::egui::{self, FontId, Pos2, RawInput, Rect, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use qs_commands::palette::search;
use qs_commands::params::{self, Input};
use qs_commands::remote::{Client, Host};
use qs_layout::{Focus, Layout, Panel};
use qs_wad::write::WadBuilder;
use qs_workbench::debug::Location;
use qs_workbench::map_edit::EntityIndex;
use qs_workbench::texture_import::project_wad;
use serde_json::json;

use crate::debugger::place;
use crate::editor::Request;
use crate::icons::Icon;
use crate::keymap;
use crate::map_tools::Readout;
use crate::palette::highlighted;
use crate::shell::{EDITOR_AREA, commanded_size, dragged_size};
use crate::strings::{Text, command_count, exit_status, match_count, missing_textures};
use crate::theme::{Mode, Theme, ThemeError, Token, semibold};
use crate::{App, AppError};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory under the system's temporary folder, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("quakec-studio-{}-{name}-{n}", std::process::id()));
        let _ignored = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

/// One headless frame of a 1440 by 900 window.
fn frame(ctx: &egui::Context, app: &mut App) {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(1440.0, 900.0),
        )),
        ..RawInput::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.frame(ui));
    // No renderer takes the textures, so they are dropped here.
    output.textures_delta.clear();
}

// UI-2: the window starts and draws its toolbar, panels and status bar without a display.
#[test]
fn the_window_draws_headless() {
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    for _ in 0..3 {
        frame(&ctx, &mut app);
    }
    assert_eq!(*app.shell().layout(), Layout::default());
}

// UI-2: a panel toggle is a command, and the layout written back is the one read at the next
// start.
#[test]
fn the_layout_persists_across_restarts() {
    let dir = TempDir::new("restart");
    let file = qs_layout::layout_path(dir.path());
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, Some(file.clone())).unwrap();
    frame(&ctx, &mut app);
    app.shell.toggle(Panel::Right).unwrap();
    app.shell.toggle(Panel::Bottom).unwrap();
    app.shell.resize(Panel::Left, 300.0).unwrap();
    assert!(app.shell().session().undo_depth() == 0);
    frame(&ctx, &mut app);
    let written = Layout::read(&file).unwrap();
    assert!(!written.right.visible && !written.bottom.visible && written.left.visible);
    let ctx = egui::Context::default();
    let mut again = App::checked_on_the_frame(&ctx, Some(file.clone())).unwrap();
    assert_eq!(again.shell().layout(), &written);
    frame(&ctx, &mut again);
    // A file that is not a layout starts the default layout.
    fs::write(&file, "not a layout").unwrap();
    let ctx = egui::Context::default();
    let app = App::checked_on_the_frame(&ctx, Some(file)).unwrap();
    assert_eq!(*app.shell().layout(), Layout::default());
}

// UI-2: every panel can be hidden and shown again.
#[test]
fn every_panel_toggles() {
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    for panel in Panel::ALL {
        app.shell.toggle(panel).unwrap();
        frame(&ctx, &mut app);
        assert!(!app.shell().layout().panel(panel).visible);
        app.shell.toggle(panel).unwrap();
        frame(&ctx, &mut app);
        assert!(app.shell().layout().panel(panel).visible);
    }
    assert!(app.shell.resize(Panel::Bottom, 10.0).is_err());
}

// No spec statement: AGT-7, a command another process sends over the app's socket runs in the
// window's session on the next frame, and its answer goes back
#[test]
fn a_command_sent_over_the_socket_runs_in_the_window() {
    let dir = TempDir::new("socket");
    let socket = if cfg!(windows) {
        PathBuf::from(format!(
            r"\\.\pipe\quakec-studio-test-{}",
            std::process::id()
        ))
    } else {
        dir.path().join("app.sock")
    };
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    let (woken, wake) = std::sync::mpsc::channel();
    let host = Host::listen(&socket, move || {
        let _sent = woken.send(());
    })
    .unwrap();
    app.shell_mut().set_host(host);
    let client = std::thread::spawn(move || {
        let mut client = Client::connect(&socket).unwrap();
        client
            .execute("view.toggle_panel", json!({ "panel": "left" }))
            .unwrap()
    });
    wake.recv().unwrap();
    frame(&ctx, &mut app);
    let shown = client.join().unwrap().unwrap();
    assert_eq!(shown.get("visible"), Some(&json!(false)), "{shown}");
    assert!(!app.shell().layout().panel(Panel::Left).visible);
}

// UI-15: both themes have every colour the app draws with, from docs/design/tokens.toml.
#[test]
fn both_themes_load_from_the_tokens() {
    let dark = Theme::load(Mode::Dark).unwrap();
    let light = Theme::load(Mode::Light).unwrap();
    assert_eq!(
        dark.color(Token::Accent),
        egui::Color32::from_rgb(0xEE, 0x8B, 0x45)
    );
    assert_eq!(
        light.color(Token::Accent),
        egui::Color32::from_rgb(0xA8, 0x4D, 0x16)
    );
    assert_eq!(
        dark.color(Token::SurfacePanel),
        egui::Color32::from_rgb(0x21, 0x1F, 0x1E)
    );
    for token in Token::ALL {
        assert_ne!(dark.color(token), egui::Color32::MAGENTA, "{token:?}");
        assert_ne!(dark.color(token), light.color(token), "{token:?}");
    }
    assert_eq!(dark.type_scale.body.size, 13.0);
    assert_eq!(dark.size.toolbar, 44.0);
    assert_eq!(
        (dark.space(0), dark.space(4), dark.space(9)),
        (4.0, 24.0, 24.0)
    );
}

// ADR-0010: the bundled fonts draw the interface, code, headings and every icon.
#[test]
fn the_bundled_fonts_have_every_glyph() {
    let ctx = egui::Context::default();
    let theme = Theme::load(Mode::Dark).unwrap();
    theme.apply(&ctx);
    // New fonts take effect at the start of the next frame.
    for _ in 0..2 {
        let mut output = ctx.run_ui(RawInput::default(), |_| {});
        output.textures_delta.clear();
    }
    // egui's `has_glyph` is false for a character the family's first face draws, which is
    // also its replacement face, so it shows only that an icon comes from the Lucide face
    // after it rather than falling back.
    ctx.fonts_mut(|fonts| {
        for family in [
            FontId::proportional(13.0),
            FontId::monospace(13.0),
            FontId::new(12.0, semibold()),
        ] {
            for icon in Icon::ALL {
                assert!(
                    fonts.has_glyph(&family, icon.char()),
                    "{icon:?} in {family:?}"
                );
            }
        }
    });
    let definitions = Theme::fonts();
    assert_eq!(definitions.font_data.len(), 5);
    assert_eq!(definitions.families.len(), 3);
    let icons: BTreeSet<char> = Icon::ALL.iter().map(|icon| icon.char()).collect();
    assert_eq!(icons.len(), Icon::ALL.len());
}

// CONV-16: every string the window shows is in the catalog, once.
#[test]
fn the_catalog_holds_each_string_once() {
    let texts: BTreeSet<&str> = Text::ALL.iter().map(|text| text.get()).collect();
    assert_eq!(texts.len(), Text::ALL.len());
    assert!(texts.iter().all(|text| !text.trim().is_empty()));
}

/// Whether the string literals on `code` hold words: a format string of placeholders and
/// punctuation, such as `"{}: {error}"`, holds none.
fn has_words(code: &str) -> bool {
    code.split('"').skip(1).step_by(2).any(|literal| {
        let mut depth = 0_u32;
        literal.chars().any(|c| match c {
            '{' => {
                depth += 1;
                false
            }
            '}' => {
                depth = depth.saturating_sub(1);
                false
            }
            c => depth == 0 && c.is_alphabetic(),
        })
    })
}

/// The lines of `source` that hold a string literal with words in it, other than comments,
/// attributes and log lines, which are for developers and are never shown.
fn literal_lines(source: &str) -> Vec<(usize, &str)> {
    let mut in_attribute = false;
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let code = line.split("//").next().unwrap_or_default().trim();
            if code.starts_with("#[") || in_attribute {
                in_attribute = !code.ends_with(")]") && !code.ends_with(']');
                return false;
            }
            !code.contains("tracing::") && has_words(code)
        })
        .map(|(index, line)| (index + 1, line))
        .collect()
}

// CONV-16: the UI modules hold no string literal; their text comes from the catalog.
#[test]
fn the_ui_code_has_no_string_literals() {
    for (name, source) in [
        ("shell.rs", include_str!("shell.rs")),
        ("entities.rs", include_str!("entities.rs")),
        ("debugger.rs", include_str!("debugger.rs")),
        ("finder.rs", include_str!("finder.rs")),
        ("game_guide.rs", include_str!("game_guide.rs")),
        ("palette.rs", include_str!("palette.rs")),
        ("terminal.rs", include_str!("terminal.rs")),
        ("picker.rs", include_str!("picker.rs")),
        ("git_panel.rs", include_str!("git_panel.rs")),
        ("multibuffer.rs", include_str!("multibuffer.rs")),
        ("project.rs", include_str!("project.rs")),
        ("editor.rs", include_str!("editor.rs")),
        ("code_style.rs", include_str!("code_style.rs")),
        ("menu.rs", include_str!("menu.rs")),
        ("map_editor.rs", include_str!("map_editor.rs")),
        ("map_browser.rs", include_str!("map_browser.rs")),
        ("map_fields.rs", include_str!("map_fields.rs")),
        ("map_groups.rs", include_str!("map_groups.rs")),
        ("map_outline.rs", include_str!("map_outline.rs")),
        ("lib.rs", include_str!("lib.rs")),
        ("main.rs", include_str!("main.rs")),
    ] {
        let found = literal_lines(source);
        assert!(found.is_empty(), "{name} has string literals: {found:?}");
    }
    // The check finds one.
    let found = literal_lines(
        "ui.label(\"Play\");\n// \"comment\"\ntracing::warn!(\"log\");\n#[expect(\n  reason = \"why\"\n)]\neprintln!(\"{}: {error}\", x);\n",
    );
    assert_eq!(found, [(1, "ui.label(\"Play\");")]);
}

/// The text of every string a frame painted.
fn painted(output: &egui::FullOutput) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.job.text.clone()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// One frame with `events`, returning what it painted.
fn frame_with(ctx: &egui::Context, app: &mut App, events: Vec<egui::Event>) -> Vec<String> {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(1440.0, 900.0),
        )),
        events,
        ..RawInput::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.frame(ui));
    output.textures_delta.clear();
    painted(&output)
}

fn labelled(icon: Icon, text: Text) -> String {
    format!("{} {}", icon.char(), text.get())
}

// UI-2: the window paints the toolbar, every panel's heading and placeholder, the centre and
// the status bar, from the catalog.
#[test]
fn the_window_paints_every_region() {
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    frame_with(&ctx, &mut app, Vec::new());
    let texts = frame_with(&ctx, &mut app, Vec::new());
    let mut expected: Vec<String> = [
        Text::PreviewHost,
        Text::PreviewDocked,
        Text::Project,
        Text::NoProject,
        Text::Entities,
        Text::NotPlaying,
        Text::Properties,
        Text::Agent,
        Text::NoAgent,
        Text::Problems,
        Text::Console,
        Text::Debugger,
        Text::NoProblems,
        Text::Editor,
        Text::Game,
        Text::NoFileOpen,
        Text::PreviewStopped,
    ]
    .iter()
    .map(|text| text.get().to_owned())
    .collect();
    expected.extend([
        labelled(Icon::Play, Text::Play),
        labelled(Icon::Pause, Text::Pause),
        labelled(Icon::Step, Text::Step),
        labelled(Icon::Restart, Text::RestartMap),
        labelled(Icon::Search, Text::SearchCommands),
        format!("{} 0", Icon::Error.char()),
        format!("{} 0", Icon::Warning.char()),
        Icon::PanelLeft.char().to_string(),
    ]);
    for text in &expected {
        assert!(texts.contains(text), "{text:?} not painted in {texts:?}");
    }
    // A hidden panel paints nothing.
    app.shell.toggle(Panel::Left).unwrap();
    for _ in 0..30 {
        frame_with(&ctx, &mut app, Vec::new());
    }
    let texts = frame_with(&ctx, &mut app, Vec::new());
    assert!(
        !texts.contains(&Text::NoProject.get().to_owned()),
        "{texts:?}"
    );
}

// UI-2: a size that moves while the pointer is held is a drag of the panel's edge; one that
// moves without it, by less than half a point, or on the first frame is not.
#[test]
fn only_a_drag_resizes_a_panel() {
    assert_eq!(dragged_size(Some(240.0), Some(260.0), true), Some(260.0));
    assert_eq!(dragged_size(Some(240.0), Some(220.0), true), Some(220.0));
    assert_eq!(dragged_size(Some(240.0), Some(240.5), true), Some(240.5));
    assert_eq!(dragged_size(Some(240.0), Some(240.4), true), None);
    assert_eq!(dragged_size(Some(240.0), Some(260.0), false), None);
    assert_eq!(dragged_size(None, Some(260.0), true), None);
    assert_eq!(dragged_size(Some(240.0), None, true), None);
}

// UI-2: the layout is written once a change is made and no drag is under way, and only once.
#[test]
fn the_layout_is_written_after_a_drag_not_during_it() {
    let dir = TempDir::new("drag");
    let file = qs_layout::layout_path(dir.path());
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, Some(file.clone())).unwrap();
    app.shell.resize(Panel::Left, 300.0).unwrap();
    app.shell.save_if_changed(true);
    assert!(!file.exists(), "written during the drag");
    app.shell.save_if_changed(false);
    assert_eq!(Layout::read(&file).unwrap().left.size, 300.0);
    fs::remove_file(&file).unwrap();
    app.shell.save_if_changed(false);
    assert!(!file.exists(), "written again with nothing changed");
}

// UI-2: a size within half a point of the panel's is not a resize.
#[test]
fn a_resize_under_half_a_point_changes_nothing() {
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    app.shell.resize(Panel::Right, 320.4).unwrap();
    assert_eq!(app.shell().layout().right.size, 320.0);
    app.shell.resize(Panel::Right, 319.6).unwrap();
    assert_eq!(app.shell().layout().right.size, 320.0);
    app.shell.resize(Panel::Right, 320.5).unwrap();
    assert_eq!(app.shell().layout().right.size, 320.5);
}

#[test]
fn errors_say_what_went_wrong() {
    let missing = ThemeError::Missing {
        mode: Mode::Light,
        token: Token::Accent,
    };
    assert_eq!(missing.to_string(), "the Light theme has no accent colour");
    let bad = ThemeError::BadColor {
        token: Token::Text,
        value: "red".to_owned(),
    };
    assert_eq!(bad.to_string(), "colour text is \"red\", not #RRGGBB");
    assert_eq!(
        AppError::Theme(missing).to_string(),
        "the Light theme has no accent colour"
    );
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    let error = app.shell.resize(Panel::Left, 1.0).unwrap_err();
    assert!(error.to_string().contains("at least 120 points"), "{error}");
    assert!(std::error::Error::source(&error).is_some());
}

// UI-2: a panel resized by its command, not by a drag, is drawn at the new size.
#[test]
fn a_panel_resized_by_command_is_drawn_at_its_size() {
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    frame(&ctx, &mut app);
    app.shell.resize(Panel::Left, 300.0).unwrap();
    app.shell.resize(Panel::Bottom, 260.0).unwrap();
    frame(&ctx, &mut app);
    frame(&ctx, &mut app);
    assert_eq!(app.shell.drawn[0], Some(300.0));
    assert_eq!(app.shell.drawn[2], Some(260.0));
    assert!((app.shell().layout().left.size - 300.0).abs() < f32::EPSILON);
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// Opens the palette and gives its search field the focus.
fn open_palette(ctx: &egui::Context, app: &mut App) {
    app.shell.open_palette();
    frame(ctx, app);
}

// UI-10: every registered command is the palette's first match for its own id, and Enter on
// it runs it or asks for its parameters.
#[test]
fn every_registered_command_is_reachable_from_the_palette() {
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    let ids: Vec<String> = app
        .shell()
        .session()
        .registry()
        .commands()
        .map(|c| c.id().as_str().to_owned())
        .collect();
    assert!(ids.len() >= 5, "the built-in and the layout commands");
    let all = search(app.shell().session().registry().commands(), "");
    assert_eq!(all.len(), ids.len(), "an empty query lists them all");
    for id in &ids {
        let found = search(app.shell().session().registry().commands(), id);
        assert_eq!(
            found.first().map(|m| m.info.id().as_str()),
            Some(id.as_str())
        );
        open_palette(&ctx, &mut app);
        frame_with(&ctx, &mut app, vec![egui::Event::Text(id.clone())]);
        frame_with(
            &ctx,
            &mut app,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        let palette = &mut app.shell.palette;
        let prompted = palette.prompt_mut().map(|p| p.id.as_str().to_owned());
        let ran = !palette.is_open() || palette.error().is_some();
        assert!(
            prompted.as_deref() == Some(id.as_str()) || (prompted.is_none() && ran),
            "{id}: prompted {prompted:?}, ran {ran}"
        );
        app.shell.palette.close();
    }
}

// UI-10: a command with parameters asks for them, and runs with what was entered.
#[test]
fn the_palette_asks_for_parameters_and_runs_the_command() {
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    open_palette(&ctx, &mut app);
    frame_with(
        &ctx,
        &mut app,
        vec![egui::Event::Text("toggle panel".to_owned())],
    );
    frame_with(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    let painted = frame_with(&ctx, &mut app, Vec::new());
    assert!(
        painted.iter().any(|t| t == "panel"),
        "the field is drawn: {painted:?}"
    );
    assert!(painted.iter().any(|t| t == "bottom"), "with its choices");
    frame_with(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.shell.palette.error(), Some("`panel` is required"));
    assert!(app.shell.palette.is_open());
    let prompt = app.shell.palette.prompt_mut().unwrap();
    assert_eq!(prompt.id.as_str(), "view.toggle_panel");
    prompt.inputs = vec![Input::Choice(Some(2))];
    frame_with(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(!app.shell.palette.is_open(), "it closes after running");
    assert!(
        !app.shell().layout().bottom.visible,
        "the bottom panel was toggled"
    );
}

// UI-10: Escape leaves the prompt for the list and closes the list; the arrow keys pick a row.
#[test]
fn the_palette_keys_move_go_back_and_close() {
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    frame_with(
        &ctx,
        &mut app,
        vec![key(
            egui::Key::P,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        )],
    );
    assert!(app.shell.palette.is_open(), "Cmd+Shift+P opens it");
    // The first command in id order that asks for parameters, and how far down it is.
    let mut infos: Vec<_> = app.shell().session().registry().commands().collect();
    infos.sort_by(|a, b| a.id().as_str().cmp(b.id().as_str()));
    let (downs, asking) = infos
        .iter()
        .enumerate()
        .find(|(_, info)| !params::fields(info.params()).is_empty())
        .map(|(index, info)| (index, info.id().as_str().to_owned()))
        .unwrap();
    let none = egui::Modifiers::NONE;
    frame_with(&ctx, &mut app, vec![key(egui::Key::ArrowUp, none)]);
    for _ in 0..downs {
        frame_with(&ctx, &mut app, vec![key(egui::Key::ArrowDown, none)]);
    }
    frame_with(&ctx, &mut app, vec![key(egui::Key::Enter, none)]);
    let prompt = app
        .shell
        .palette
        .prompt_mut()
        .map(|p| p.id.as_str().to_owned());
    assert_eq!(
        prompt.as_deref(),
        Some(asking.as_str()),
        "the first command in id order that asks for parameters"
    );
    frame_with(&ctx, &mut app, vec![key(egui::Key::Escape, none)]);
    assert!(app.shell.palette.prompt_mut().is_none(), "back to the list");
    assert!(app.shell.palette.is_open());
    frame_with(&ctx, &mut app, vec![key(egui::Key::Escape, none)]);
    assert!(!app.shell.palette.is_open(), "closed");
    open_palette(&ctx, &mut app);
    frame_with(&ctx, &mut app, vec![egui::Event::Text("zzzz".to_owned())]);
    let painted = frame_with(&ctx, &mut app, Vec::new());
    assert!(painted.iter().any(|t| t == Text::PaletteNoMatch.get()));
    frame_with(&ctx, &mut app, vec![key(egui::Key::Enter, none)]);
    assert!(
        app.shell.palette.is_open(),
        "Enter with no match does nothing"
    );
}

// UI-10: each row shows its command's id and shortcut, and the count of matches.
#[test]
fn a_row_shows_the_id_and_the_shortcut() {
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    open_palette(&ctx, &mut app);
    let painted = frame_with(&ctx, &mut app, vec![egui::Event::Text("undo".to_owned())]);
    let painted = [painted, frame_with(&ctx, &mut app, Vec::new())].concat();
    let undo = ctx.format_shortcut(&keymap::shortcut("edit.undo").unwrap());
    assert!(painted.contains(&undo), "{undo:?} in {painted:?}");
    assert!(painted.iter().any(|t| t == "edit.undo"));
    let registry = app.shell().session().registry();
    let total = registry.commands().count();
    let shown = search(registry.commands(), "undo").len();
    let count = command_count(shown, total);
    assert!(painted.contains(&count), "{count:?} in {painted:?}");
}

// PRIN-3: every key binding runs a registered command that needs no parameter (P4-32:
// `editor.go_to_line`'s line is optional, and without it the window asks for one).
#[test]
fn every_binding_names_a_registered_command_without_parameters() {
    let ctx = egui::Context::default();
    let app = App::checked_on_the_frame(&ctx, None).unwrap();
    let registry = app.shell().session().registry();
    for binding in keymap::BINDINGS {
        let info = registry
            .commands()
            .find(|c| c.id().as_str() == binding.command)
            .expect("every binding's command is registered");
        let required = info.params().get("required").and_then(|r| r.as_array());
        assert!(
            required.is_none_or(Vec::is_empty),
            "{} needs no parameter",
            binding.command
        );
        assert!(keymap::bindable(registry, binding.command));
        assert_eq!(keymap::shortcut(binding.command), Some(binding.shortcut));
    }
    assert_eq!(keymap::shortcut("view.toggle_panel"), None);
}

// PRIN-3: a binding's shortcut runs its command, and is left alone while the palette is open.
#[test]
fn a_shortcut_runs_its_command() {
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    let undo = keymap::shortcut("edit.undo").unwrap();
    let pressed = key(undo.logical_key, undo.modifiers);
    frame_with(&ctx, &mut app, vec![pressed.clone()]);
    assert!(!app.shell.palette.is_open());
    open_palette(&ctx, &mut app);
    frame_with(&ctx, &mut app, vec![pressed]);
    assert!(app.shell.palette.is_open());
}

// UI-10: the palette's count, and a row's matched characters in the accent in the row's font.
#[test]
fn the_count_and_the_highlight() {
    assert_eq!(command_count(1, 5), "1 of 5 commands");
    let font = FontId::monospace(13.0);
    let job = highlighted(
        "play.start",
        &[0..2, 5..6],
        egui::Color32::GRAY,
        egui::Color32::RED,
        &font,
    );
    let sections: Vec<(String, egui::Color32)> = job
        .sections
        .iter()
        .map(|s| {
            let text = job
                .text
                .get(s.byte_range.start.0..s.byte_range.end.0)
                .unwrap();
            (text.to_owned(), s.format.color)
        })
        .filter(|(text, _)| !text.is_empty())
        .collect();
    let gray = egui::Color32::GRAY;
    let red = egui::Color32::RED;
    assert_eq!(
        sections,
        [
            ("pl".to_owned(), red),
            ("ay.".to_owned(), gray),
            ("s".to_owned(), red),
            ("tart".to_owned(), gray),
        ]
    );
    assert!(job.sections.iter().all(|s| s.format.font_id == font));
}

// UI-10: the footer names the keys, and a prompt's first field takes what is typed.
#[test]
fn the_footer_and_the_first_field() {
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    open_palette(&ctx, &mut app);
    let painted = frame_with(&ctx, &mut app, Vec::new());
    for hint in [
        Text::PaletteMove,
        Text::PaletteRun,
        Text::PaletteClose,
        Text::KeyEscape,
    ] {
        assert!(painted.iter().any(|t| t == hint.get()), "{hint:?}");
    }
    frame_with(&ctx, &mut app, vec![egui::Event::Text("resize".to_owned())]);
    frame_with(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    let prompt = app.shell.palette.prompt_mut().unwrap();
    assert_eq!(prompt.id.as_str(), "view.resize_panel");
    let size = prompt.fields.iter().position(|f| f.name == "size").unwrap();
    let panel = prompt
        .fields
        .iter()
        .position(|f| f.name == "panel")
        .unwrap();
    assert!(
        panel < size,
        "required fields by name: the choice comes before the text"
    );
    prompt.inputs = prompt
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            if i == panel {
                Input::Choice(Some(0))
            } else {
                Input::empty(&f.kind)
            }
        })
        .collect();
    frame_with(&ctx, &mut app, Vec::new());
    frame_with(&ctx, &mut app, vec![egui::Event::Text("300".to_owned())]);
    frame_with(
        &ctx,
        &mut app,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.shell.palette.error(), None);
    assert!(!app.shell.palette.is_open());
    assert!((app.shell().layout().left.size - 300.0).abs() < f32::EPSILON);
}

// UI-2: a layout size that differs from the drawn one by half a point or more is drawn, unless
// a pointer button is held, as one is while the user drags the panel's edge.
#[test]
fn a_size_set_by_command_is_told_from_a_drag() {
    assert!(commanded_size(Some(240.0), 300.0, false));
    assert!(commanded_size(Some(300.0), 240.0, false), "smaller too");
    assert!(commanded_size(Some(240.0), 240.5, false));
    assert!(
        !commanded_size(Some(240.0), 240.4, false),
        "under half a point"
    );
    assert!(!commanded_size(Some(240.0), 240.0, false));
    assert!(
        !commanded_size(Some(240.0), 300.0, true),
        "a drag is under way"
    );
    assert!(
        !commanded_size(None, 300.0, false),
        "never drawn: default_size applies"
    );
}

// UI-4: the find bar counts its matches in words.
#[test]
fn the_match_count_reads_as_words() {
    assert_eq!(match_count(0), "0 matches");
    assert_eq!(match_count(1), "1 match");
    assert_eq!(match_count(12), "12 matches");
}

/// A kittest harness running the app with no file open, after its first frame.
fn accessible() -> Harness<'static, Option<App>> {
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
    let app = App::checked_on_the_frame(&harness.ctx, None).unwrap();
    *harness.state_mut() = Some(app);
    harness.run();
    harness
}

/// Runs the command `id` with no parameters in `harness`'s app, then a frame.
fn run_command(harness: &mut Harness<'static, Option<App>>, id: &str) {
    let app = harness.state_mut().as_mut().unwrap();
    app.shell.execute(id, json!({})).unwrap();
    harness.run();
}

// QA-9: every area of the window has a role and a name a screen reader can say, and every
// control has a name: the icon-only buttons are named for what they do.
#[test]
fn every_area_and_control_has_a_name() {
    let mut harness = accessible();
    harness.state_mut().as_mut().unwrap().shell.open_palette();
    harness.run();
    for (role, text) in [
        (Role::Toolbar, Text::Toolbar),
        (Role::Status, Text::StatusBar),
        (Role::Pane, Text::LeftPanel),
        (Role::Pane, Text::RightPanel),
        (Role::Pane, Text::BottomPanel),
        (Role::Pane, Text::Centre),
        (Role::Dialog, Text::CommandPalette),
        (Role::TextInput, Text::SearchCommands),
        (Role::Heading, Text::Project),
        (Role::Heading, Text::Symbols),
        (Role::Heading, Text::Entities),
        (Role::Heading, Text::Properties),
        (Role::Heading, Text::Agent),
        (Role::Button, Text::ToggleLeftPanel),
        (Role::Button, Text::ToggleRightPanel),
        (Role::Button, Text::ToggleBottomPanel),
        (Role::Button, Text::SearchCommands),
        (Role::Button, Text::Editor),
        (Role::Button, Text::Game),
        (Role::Button, Text::Map),
        (Role::Button, Text::Problems),
        (Role::Button, Text::Console),
        (Role::Button, Text::Debugger),
        (Role::Button, Text::Terminal),
    ] {
        harness.get_by_role_and_label(role, text.get());
    }
    // The palette lists the play loop's commands too, so the toolbar's are the buttons whose
    // names end with the control's text.
    for text in [Text::Play, Text::Pause, Text::Step, Text::RestartMap] {
        let found = harness.query_all_by_label_contains(text.get()).any(|node| {
            node.accesskit_node().role() == Role::Button
                && node
                    .accesskit_node()
                    .label()
                    .is_some_and(|label| label.ends_with(text.get()))
        });
        assert!(found, "{text:?}");
    }
    // The panel toggles say whether their panel is shown.
    let toggle = harness.get_by_role_and_label(Role::Button, Text::ToggleLeftPanel.get());
    assert_eq!(toggle.accesskit_node().toggled(), Some(Toggled::True));
    // The palette's rows are buttons named by their ids and summaries.
    harness.get_by_role_and_label(
        Role::Button,
        "view.toggle_panel: Shows or hides a panel: Project and Entities on the left, \
         Properties and the agent on the right, or Problems, Console and the debugger at the \
         bottom.",
    );
    // No control is nameless.
    let nameless: Vec<_> = harness
        .root()
        .children_recursive()
        .filter(|node| {
            matches!(
                node.accesskit_node().role(),
                Role::Button | Role::TextInput | Role::CheckBox
            )
        })
        .filter(|node| {
            node.accesskit_node()
                .label()
                .is_none_or(|label| label.trim().is_empty())
        })
        .collect();
    assert!(nameless.is_empty(), "{nameless:#?}");
}

// QA-9: a focus command gives its panel's first heading, or the bottom panel's shown tab, the
// keyboard, showing the panel first; the editor's command gives the editor the keyboard, or
// the Editor tab when no file is open; and the same panel can be focused again after the
// keyboard went elsewhere.
#[test]
fn focus_commands_reach_every_panel() {
    let mut harness = accessible();
    harness
        .state_mut()
        .as_mut()
        .unwrap()
        .shell
        .toggle(Panel::Right)
        .unwrap();
    harness.run();
    assert!(
        harness
            .query_by_role_and_label(Role::Heading, Text::Properties.get())
            .is_none()
    );
    for (id, role, text) in [
        (qs_layout::FOCUS_LEFT_PANEL, Role::Heading, Text::Project),
        (
            qs_layout::FOCUS_RIGHT_PANEL,
            Role::Heading,
            Text::Properties,
        ),
        (qs_layout::FOCUS_BOTTOM_PANEL, Role::Button, Text::Problems),
        (qs_layout::FOCUS_EDITOR, Role::Button, Text::Editor),
    ] {
        run_command(&mut harness, id);
        harness.run();
        let target = harness.get_by_role_and_label(role, text.get());
        assert!(target.is_focused(), "{id}: {target:?}");
    }
    assert!(
        harness
            .state()
            .as_ref()
            .unwrap()
            .shell()
            .layout()
            .right
            .visible
    );
    // The keyboard went to the Editor tab by the command above; Cmd+2 brings it to the left
    // panel.
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num2);
    harness.run();
    harness.run();
    let project = harness.get_by_role_and_label(Role::Heading, Text::Project.get());
    assert!(project.is_focused());
    // A Tab from the heading moves on into the panel.
    harness.key_press(egui::Key::Tab);
    harness.run();
    harness.run();
    let project = harness.get_by_role_and_label(Role::Heading, Text::Project.get());
    assert!(!project.is_focused());
    assert_eq!(
        harness.state().as_ref().unwrap().shell().layout().focus,
        Focus::Panel(Panel::Left)
    );
}

// QA-9: the focus shortcuts work while the editor has the keyboard, which the editor's own
// shortcuts do not, and a focus command on the panel the layout already names still moves
// the keyboard back from the editor.
#[test]
fn focus_shortcuts_reach_out_of_the_editor() {
    let mut harness = accessible();
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ember");
    let app = harness.state_mut().as_mut().unwrap();
    app.shell_mut().open_project(&project).unwrap();
    app.shell_mut()
        .execute(
            "file.open",
            json!({ "path": project.join("src/weapons.qc") }),
        )
        .unwrap();
    harness.run();
    let editor_has_keyboard = |harness: &Harness<'static, Option<App>>| {
        harness
            .ctx
            .memory(|m| m.has_focus(egui::Id::new(EDITOR_AREA)))
    };
    run_command(&mut harness, qs_layout::FOCUS_EDITOR);
    harness.run();
    assert!(editor_has_keyboard(&harness));
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num3);
    harness.run();
    harness.run();
    assert!(!editor_has_keyboard(&harness));
    let properties = harness.get_by_role_and_label(Role::Heading, Text::Properties.get());
    assert!(properties.is_focused());
    // Back to the editor by a click, then the same panel again by its key.
    harness
        .ctx
        .memory_mut(|m| m.request_focus(egui::Id::new(EDITOR_AREA)));
    harness.run();
    assert!(editor_has_keyboard(&harness));
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num3);
    harness.run();
    harness.run();
    assert!(!editor_has_keyboard(&harness));
    let properties = harness.get_by_role_and_label(Role::Heading, Text::Properties.get());
    assert!(properties.is_focused());
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num1);
    harness.run();
    harness.run();
    assert!(editor_has_keyboard(&harness));
    // With the Map tab shown, the editor's command gives the keyboard to that tab, as there
    // is no editor to give it to.
    harness
        .get_by_role_and_label(Role::Button, Text::Map.get())
        .click();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num1);
    harness.run();
    harness.run();
    assert!(!editor_has_keyboard(&harness));
    let map = harness.get_by_role_and_label(Role::Button, Text::Map.get());
    assert!(map.is_focused());
}

// PRIN-3: each binding's keys run its own command and no other, so Shift+Cmd+Z redoes rather
// than undoing again, outside the editor as in it.
#[test]
fn each_shortcut_runs_only_its_own_command() {
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    let ember = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ember");
    app.shell_mut().open_project(&ember).unwrap();
    frame(&ctx, &mut app);
    let defs = ember.join("defs.qc");
    let offset = fs::read_to_string(&defs).unwrap().find("bounces").unwrap();
    app.shell_mut()
        .execute(
            "symbol.rename",
            json!({ "path": defs, "offset": offset, "name": "hops" }),
        )
        .unwrap();
    let depths = |app: &App| {
        let session = app.shell().session();
        (session.undo_depth(), session.redo_depth())
    };
    assert_eq!(depths(&app), (1, 0));
    for (command, after) in [("edit.undo", (0, 1)), ("edit.redo", (1, 0))] {
        let shortcut = keymap::shortcut(command).unwrap();
        frame_with(
            &ctx,
            &mut app,
            vec![key(shortcut.logical_key, shortcut.modifiers)],
        );
        assert_eq!(depths(&app), after, "{command}");
    }
}

// PRIN-3: the bindings are matched most specific first, every binding once, so no shortcut
// that holds another's keys and more is taken by the other.
#[test]
fn bindings_are_matched_most_specific_first() {
    let order = keymap::in_match_order();
    assert_eq!(order.len(), keymap::BINDINGS.len());
    for binding in keymap::BINDINGS {
        assert!(order.contains(&binding));
    }
    let at = |command: &str| order.iter().position(|b| b.command == command).unwrap();
    assert!(at("edit.redo") < at("edit.undo"));
    for (i, earlier) in order.iter().enumerate() {
        for later in order.iter().skip(i + 1) {
            let (a, b) = (earlier.shortcut, later.shortcut);
            let same_key = a.logical_key == b.logical_key;
            assert!(
                !(same_key
                    && b.modifiers.contains(a.modifiers)
                    && !a.modifiers.contains(b.modifiers)),
                "{} is matched before {}, whose keys hold its own",
                earlier.command,
                later.command
            );
        }
    }
}

// PRIN-3 (P2-RA re-review): egui ignores an extra Alt as it does an extra Shift, so Alt+Cmd+Z
// still undoes and Alt+Shift+Cmd+Z still redoes; neither runs the other.
#[test]
fn an_extra_alt_runs_the_bindings_own_command() {
    let ctx = egui::Context::default();
    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    let ember = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ember");
    app.shell_mut().open_project(&ember).unwrap();
    frame(&ctx, &mut app);
    let defs = ember.join("defs.qc");
    let offset = fs::read_to_string(&defs).unwrap().find("bounces").unwrap();
    app.shell_mut()
        .execute(
            "symbol.rename",
            json!({ "path": defs, "offset": offset, "name": "hops" }),
        )
        .unwrap();
    let depths = |app: &App| {
        let session = app.shell().session();
        (session.undo_depth(), session.redo_depth())
    };
    let alt = egui::Modifiers::ALT;
    for (command, after) in [("edit.undo", (0, 1)), ("edit.redo", (1, 0))] {
        let shortcut = keymap::shortcut(command).unwrap();
        frame_with(
            &ctx,
            &mut app,
            vec![key(shortcut.logical_key, shortcut.modifiers.plus(alt))],
        );
        assert_eq!(depths(&app), after, "{command} with Alt");
    }
}

// No spec statement: the Debugger tab lists a breakpoint set with no game running, which waits
// for the next game; the step buttons wait for a stop, and unticking the breakpoint clears it
#[test]
fn the_debugger_tab_lists_breakpoints_and_steps_only_when_stopped() {
    let mut harness = accessible();
    harness
        .get_by_role_and_label(Role::Button, Text::Debugger.get())
        .click();
    harness.run();
    assert!(
        harness
            .query_by_label_contains(Text::NoBreakpoints.get())
            .is_some()
    );
    let app = harness.state_mut().as_mut().unwrap();
    app.shell
        .execute(
            qs_workbench::BREAKPOINT,
            json!({ "file": "src/a.qc", "line": 3 }),
        )
        .unwrap();
    harness.run();
    assert!(harness.query_by_label_contains("src/a.qc:3").is_some());
    assert!(
        harness
            .query_by_label_contains(Text::NotStopped.get())
            .is_some()
    );
    let step = harness.get_by_role_and_label(Role::Button, Text::StepOver.get());
    assert!(step.accesskit_node().is_disabled(), "nothing is stopped");
    harness.get_by_role(Role::CheckBox).click();
    harness.run();
    harness.run();
    assert!(harness.query_by_label_contains("src/a.qc:3").is_none());
}

// No spec statement: a call stack frame reads as its file's name and line, or `engine`
#[test]
fn a_frame_reads_as_its_place() {
    let frame = |file: Option<&str>, line, engine| Location {
        function: "f".to_owned(),
        file: file.map(PathBuf::from),
        line,
        engine,
    };
    assert_eq!(
        place(&frame(Some("src/weapons.qc"), Some(33), false)),
        "weapons.qc:33"
    );
    assert_eq!(
        place(&frame(Some("src/weapons.qc"), None, false)),
        "src/weapons.qc"
    );
    assert_eq!(place(&frame(None, Some(3), false)), "");
    assert_eq!(
        place(&frame(Some("a.qc"), Some(1), true)),
        Text::EngineFrame.get()
    );
}

// No spec statement: with no project open the Project section lists the recent projects; a
// click opens one, which then comes first in the list and its file; one that no longer opens
// says why and comes off the list
#[test]
fn recent_projects_open_from_the_project_section() {
    let dir = TempDir::new("recent");
    let file = qs_workbench::recent_path(&dir.0);
    let ember = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ember");
    let gone = dir.0.join("gone");
    let mut recent = qs_workbench::RecentProjects::default();
    recent.opened(&ember);
    recent.opened(&gone);
    recent.save(&file).unwrap();
    let mut harness = accessible();
    harness
        .state_mut()
        .as_mut()
        .unwrap()
        .shell
        .set_recent_file(file.clone());
    harness.run();
    assert!(harness.query_by_label(Text::RecentProjects.get()).is_some());
    harness.get_by_label("gone").click();
    harness.run();
    harness.run();
    let shell = &harness.state().as_ref().unwrap().shell;
    assert!(shell.workbench().root().is_none());
    assert_eq!(
        shell.recent_projects(),
        std::slice::from_ref(&ember),
        "gone came off"
    );
    assert!(
        harness
            .query_by_label_contains(Text::FolderGone.get())
            .is_some()
    );
    harness.get_by_label("ember").click();
    harness.run();
    harness.run();
    let shell = &harness.state().as_ref().unwrap().shell;
    assert!(
        shell
            .workbench()
            .root()
            .is_some_and(|root| root.ends_with("fixtures/ember"))
    );
    let saved = qs_workbench::RecentProjects::load(&file);
    assert_eq!(saved.projects().len(), 1);
    assert!(
        saved
            .projects()
            .first()
            .unwrap()
            .ends_with("fixtures/ember")
    );
}

/// A folder holding a game the preview can play: `id1/pak0.pak` and `id1/pak1.pak`.
fn game_folder(dir: &TempDir) -> PathBuf {
    let game = dir.0.join("quake");
    fs::create_dir_all(game.join("id1")).unwrap();
    fs::write(game.join("id1/pak0.pak"), b"PACK").unwrap();
    fs::write(game.join("id1/pak1.pak"), b"PACK").unwrap();
    game
}

// No spec statement: P3-44, a playable folder chosen is kept in the user's settings, so the
// next start plays from it with no guide; a saved folder that no longer holds a game is not
// used, and an unplayable folder chosen is not kept
#[test]
fn the_chosen_game_folder_is_kept_for_the_next_start() {
    let dir = TempDir::new("game-folder");
    let settings = dir.0.join("config/settings.toml");
    let game = game_folder(&dir);
    let ctx = egui::Context::default();

    let mut app = App::checked_on_the_frame(&ctx, None).unwrap();
    app.shell_mut().set_settings_file(settings.clone());
    let empty = dir.0.join("empty");
    fs::create_dir_all(&empty).unwrap();
    let refused = app.shell_mut().choose_game_folder(empty).unwrap();
    assert!(!refused.state.plays());
    assert!(!settings.exists(), "an unplayable folder is not kept");
    let chosen = app.shell_mut().choose_game_folder(game.clone()).unwrap();
    assert!(chosen.state.plays());

    let mut next = App::checked_on_the_frame(&ctx, None).unwrap();
    next.shell_mut().set_settings_file(settings.clone());
    assert_eq!(
        next.shell().session().context().preview.game_folder(),
        Some(game.as_path())
    );

    fs::remove_dir_all(&game).unwrap();
    let mut after = App::checked_on_the_frame(&ctx, None).unwrap();
    after.shell_mut().set_settings_file(settings);
    assert_eq!(
        after.shell().session().context().preview.game_folder(),
        None,
        "a vanished folder is not used"
    );
}

// No spec statement: P3-44, with the menu in the window (Windows and Linux) the toolbar's
// Quake Files button opens the guide; with the system's menu bar it is not on the toolbar
#[test]
fn the_toolbar_opens_the_quake_files_guide_where_there_is_no_menu_bar() {
    let mut harness = accessible();
    harness.run();
    assert!(
        harness.query_by_label(Text::GameFilesMenu.get()).is_none(),
        "the system's menu bar has it"
    );
    harness
        .state_mut()
        .as_mut()
        .unwrap()
        .shell
        .set_menu_in_window();
    harness.run();
    harness.get_by_label(Text::GameFilesMenu.get()).click();
    harness.run();
    harness.run();
    assert!(harness.query_by_label(Text::GameGuide.get()).is_some());
}

// No spec statement: P4-28, File > Export Mod: with the menu in the window the toolbar's
// Export Mod button, enabled once a project is open, writes the zip, and the status bar says
// where it is
#[test]
fn export_mod_writes_the_zip_and_says_where() {
    let dir = TempDir::new("export");
    let project = dir.path().join("exported");
    fs::create_dir_all(project.join("maps")).unwrap();
    fs::write(project.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    fs::write(project.join("world.qc"), "void() worldspawn = {};\n").unwrap();
    fs::write(project.join("maps/start.bsp"), b"BSP2").unwrap();
    let mut harness = accessible();
    {
        let shell = &mut harness.state_mut().as_mut().unwrap().shell;
        shell.set_menu_in_window();
        shell.open_project(&project).unwrap();
    }
    harness.run();
    harness.get_by_label(Text::ExportModMenu.get()).click();
    harness.run();
    harness.run();
    let zip = project.join("build").join("exported.zip");
    assert!(zip.is_file());
    let note = harness
        .state()
        .as_ref()
        .unwrap()
        .shell
        .export_note()
        .cloned();
    assert_eq!(note, Some(Ok(zip.clone())));
    let shown = format!("{} {}", Text::ExportedTo.get(), zip.display());
    assert!(
        harness.query_by_label(&shown).is_some(),
        "the status bar says {shown}"
    );
}

// No spec statement: P4-37, the acceptance criterion: a rebinding written to the keymap file
// takes effect on the next frame, with no restart, its keys taken from the default they had;
// the file's problems are reported
#[test]
fn a_rebinding_in_the_keymap_file_takes_effect_without_a_restart() {
    let dir = TempDir::new("keymap");
    let file = keymap::keymap_path(dir.path());
    let mut harness = accessible();
    harness
        .state_mut()
        .as_mut()
        .unwrap()
        .shell
        .set_keymap_file(file.clone());
    harness.run();
    let shell = |harness: &Harness<'static, Option<App>>| {
        harness.state().as_ref().unwrap().shell.keymap().clone()
    };
    let undo = keymap::parse_keys("secondary-z").unwrap();
    assert_eq!(shell(&harness).shortcut("edit.undo"), Some(undo));
    fs::write(
        &file,
        "[[binding]]\nkeys = \"secondary-z\"\ncommand = \"view.focus_left_panel\"\n\n\
         [[binding]]\nkeys = \"secondary-k\"\ncommand = \"no.such\"\n",
    )
    .unwrap();
    harness.run();
    let keymap = shell(&harness);
    assert_eq!(keymap.shortcut("view.focus_left_panel"), Some(undo));
    assert_eq!(keymap.shortcut("edit.undo"), None);
    assert_eq!(keymap.problems().len(), 1, "{:?}", keymap.problems());
    assert!(
        keymap
            .problems()
            .first()
            .is_some_and(|p| p.contains("no.such"))
    );
    fs::remove_file(&file).unwrap();
    harness.run();
    assert_eq!(
        shell(&harness).shortcut("edit.undo"),
        Some(undo),
        "back to the defaults"
    );
}

// No spec statement: P4-37: with `editor.key_mode = "vim"` in the user's settings, the code
// editor reads typed keys as Vim's
#[test]
fn the_vim_setting_makes_the_editor_read_vims_keys() {
    let dir = TempDir::new("vim-mode");
    let settings = dir.path().join("settings.toml");
    fs::write(&settings, "[editor]\nkey_mode = \"vim\"\n").unwrap();
    let mut harness = accessible();
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ember");
    let app = harness.state_mut().as_mut().unwrap();
    app.shell_mut().set_settings_file(settings);
    app.shell_mut().open_project(&project).unwrap();
    app.shell_mut()
        .execute(
            "file.open",
            json!({ "path": project.join("src/weapons.qc") }),
        )
        .unwrap();
    harness.run();
    run_command(&mut harness, qs_layout::FOCUS_EDITOR);
    harness.run();
    let text = |harness: &Harness<'static, Option<App>>| {
        harness
            .state()
            .as_ref()
            .unwrap()
            .shell()
            .editor_text()
            .unwrap()
    };
    let before = text(&harness);
    harness.event(egui::Event::Text("dd".to_owned()));
    harness.run();
    let after = text(&harness);
    let rest = before.split_once('\n').map(|(_, rest)| rest.to_owned());
    assert_eq!(Some(after), rest, "dd deletes the first line, not types dd");
}

// No spec statement: P4-36, the acceptance criterion: a task of quakec.toml is listed in the
// palette, runs from it in a new terminal tab, and its exit status is shown
#[test]
fn a_task_runs_from_the_palette_and_its_exit_status_is_shown() {
    let dir = TempDir::new("task");
    let project = dir.path().join("taskmod");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("quakec.toml"),
        "[[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n\n\
         [[task]]\nname = \"check\"\ncommand = \"echo checking&& exit 3\"\n",
    )
    .unwrap();
    fs::write(project.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    fs::write(project.join("world.qc"), "void() worldspawn = {};\n").unwrap();
    let mut harness = accessible();
    let app = harness.state_mut().as_mut().unwrap();
    app.shell_mut().open_project(&project).unwrap();
    app.shell.open_palette();
    harness.run();
    harness.event(egui::Event::Text("run task check".to_owned()));
    harness.run();
    let row = format!("{} check", Text::RunTask.get());
    harness.get_by_role_and_label(Role::Button, &row);
    harness.key_press(egui::Key::Enter);
    // The task now runs and asks for frames: one step, not a run to rest.
    harness.step();
    let exited = |harness: &Harness<'static, Option<App>>| {
        let shell = &harness.state().as_ref().unwrap().shell;
        shell
            .session()
            .context()
            .workbench
            .terminals()
            .tabs()
            .first()
            .unwrap()
            .exit()
    };
    // Each frame polls the tab without waiting; the loop ends when the process has. A running
    // tab asks for frames, so each is one step, not a run to rest.
    while exited(&harness).is_none() {
        harness.step();
        std::thread::yield_now();
    }
    harness.run();
    harness.get_by_label(&exit_status(3));
    harness.get_by_role_and_label(Role::Button, Text::Terminal.get());
    let shell = &harness.state().as_ref().unwrap().shell;
    let tab = shell
        .session()
        .context()
        .workbench
        .terminals()
        .tabs()
        .first()
        .unwrap();
    assert!(tab.text().contains("checking"), "{:?}", tab.text());
}

/// The app with the ember fixture open at `src/weapons.qc`, the editor holding the keyboard.
fn ember_open() -> Harness<'static, Option<App>> {
    let mut harness = accessible();
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ember");
    let app = harness.state_mut().as_mut().unwrap();
    app.shell_mut().open_project(&project).unwrap();
    app.shell_mut()
        .execute(
            "file.open",
            json!({ "path": project.join("src/weapons.qc") }),
        )
        .unwrap();
    harness.run();
    run_command(&mut harness, qs_layout::FOCUS_EDITOR);
    harness.run();
    harness
}

/// The primary caret's line, from 0, in the editor.
fn caret_line(harness: &Harness<'static, Option<App>>) -> usize {
    let shell = &harness.state().as_ref().unwrap().shell;
    shell.caret().unwrap().1.line
}

// No spec statement: P4-32, the acceptance criterion: every new layout and navigation action
// is a registry command the palette finds, and the keys of those Zed binds run them
#[test]
fn every_layout_and_navigation_action_is_a_command_in_the_palette() {
    let harness = accessible();
    let shell = &harness.state().as_ref().unwrap().shell;
    let registry = shell.session().registry();
    for id in [
        qs_layout::FOLD,
        qs_layout::UNFOLD,
        qs_layout::FOLD_ALL,
        qs_layout::UNFOLD_ALL,
        qs_layout::TOGGLE_SOFT_WRAP,
        qs_layout::GO_TO_LINE,
        qs_layout::GO_TO_SYMBOL,
        qs_layout::TOGGLE_OUTLINE,
        qs_layout::TAB_SWITCHER,
        qs_layout::SPLIT_RIGHT,
        qs_layout::SPLIT_DOWN,
        qs_layout::CLOSE_SPLIT,
    ] {
        assert!(registry.get(id).is_some(), "{id} is registered");
        let found = search(registry.commands(), id);
        assert!(
            found.iter().any(|m| m.info.id().as_str() == id),
            "the palette finds {id} by its id"
        );
    }
    for id in [
        qs_layout::GO_TO_LINE,
        qs_layout::GO_TO_SYMBOL,
        qs_layout::TOGGLE_OUTLINE,
        qs_layout::TAB_SWITCHER,
        qs_layout::FOLD,
        qs_layout::SPLIT_RIGHT,
    ] {
        assert!(shell.keymap().shortcut(id).is_some(), "{id} has a key");
    }
}

// No spec statement: P4-32: go to line moves the caret, and without a line asks for one in the
// palette's prompt; folding all folds the file's blocks and the caret stays shown
#[test]
fn go_to_line_and_folding_act_on_the_editor() {
    let mut harness = ember_open();
    harness
        .state_mut()
        .as_mut()
        .unwrap()
        .shell
        .execute(qs_layout::GO_TO_LINE, json!({ "line": 6 }))
        .unwrap();
    harness.run();
    assert_eq!(caret_line(&harness), 5);
    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::G);
    harness.run();
    harness.get_by_role_and_label(Role::Dialog, Text::CommandPalette.get());
    harness.key_press(egui::Key::Escape);
    harness.run();
    run_command(&mut harness, qs_layout::FOLD_ALL);
    harness.run();
    let shell = &harness.state().as_ref().unwrap().shell;
    let folds = shell.editor().unwrap().view.folds();
    assert!(folds.any_folded(), "{:?}", folds.regions());
    assert!(folds.is_hidden(4), "the first function's body");
    assert_eq!(
        caret_line(&harness),
        3,
        "the caret moved to its fold's first line"
    );
    run_command(&mut harness, qs_layout::UNFOLD_ALL);
    harness.run();
    let shell = &harness.state().as_ref().unwrap().shell;
    assert!(!shell.editor().unwrap().view.folds().any_folded());
}

// No spec statement: P4-32: go to symbol lists the file's symbols and Enter on one moves the
// caret there; the outline panel lists them too; soft wrap is the editor's
#[test]
fn go_to_symbol_the_outline_and_soft_wrap() {
    let mut harness = ember_open();
    run_command(&mut harness, qs_layout::GO_TO_SYMBOL);
    harness.run();
    harness.get_by_role_and_label(Role::Dialog, Text::GoToSymbol.get());
    harness.event(egui::Event::Text("W_Fire".to_owned()));
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    harness.run();
    assert_eq!(caret_line(&harness), 10, "W_FireEmber's line");
    run_command(&mut harness, qs_layout::TOGGLE_OUTLINE);
    harness.run();
    harness.get_by_role_and_label(Role::Pane, Text::Outline.get());
    assert_eq!(
        harness.query_all_by_label("Ember_Touch").count(),
        2,
        "in the Symbols section and the outline panel"
    );
    run_command(&mut harness, qs_layout::TOGGLE_SOFT_WRAP);
    harness.run();
    let shell = &harness.state().as_ref().unwrap().shell;
    assert_eq!(
        shell.editor().unwrap().view.wrap(),
        qs_editor_view::Wrap::Width
    );
}

// No spec statement: P4-32: the tab switcher lists the open files, the one before the shown
// file chosen; a split shows a second pane, which closing the split removes
#[test]
fn the_tab_switcher_and_the_split() {
    let mut harness = ember_open();
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ember");
    harness
        .state_mut()
        .as_mut()
        .unwrap()
        .shell_mut()
        .execute("file.open", json!({ "path": project.join("src/world.qc") }))
        .unwrap();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Tab);
    harness.run();
    harness.get_by_role_and_label(Role::Dialog, Text::TabSwitcher.get());
    harness.key_press(egui::Key::Enter);
    harness.run();
    harness.run();
    let path = |harness: &Harness<'static, Option<App>>| {
        let shell = &harness.state().as_ref().unwrap().shell;
        shell.caret().unwrap().0.to_path_buf()
    };
    assert!(
        path(&harness).ends_with("weapons.qc"),
        "{:?}",
        path(&harness)
    );
    run_command(&mut harness, qs_layout::SPLIT_RIGHT);
    harness.run();
    harness.get_by_role_and_label(Role::Pane, Text::SecondPane.get());
    let shell = &harness.state().as_ref().unwrap().shell;
    let second = shell.second_pane_editor().unwrap();
    assert!(second.path.ends_with("weapons.qc"));
    // The first pane goes to world.qc; a click in the second makes weapons.qc the one edited.
    harness
        .state_mut()
        .as_mut()
        .unwrap()
        .shell_mut()
        .execute("file.open", json!({ "path": project.join("src/world.qc") }))
        .unwrap();
    harness.run();
    let second_id = harness
        .state()
        .as_ref()
        .unwrap()
        .shell
        .second_pane_editor()
        .unwrap()
        .view
        .id();
    harness.ctx.memory_mut(|m| m.request_focus(second_id));
    harness.run();
    harness.run();
    assert!(
        path(&harness).ends_with("weapons.qc"),
        "{:?}",
        path(&harness)
    );
    let shell = &harness.state().as_ref().unwrap().shell;
    assert!(
        shell
            .second_pane_editor()
            .unwrap()
            .path
            .ends_with("world.qc")
    );
    assert!(
        harness
            .ctx
            .memory(|m| m.has_focus(egui::Id::new(EDITOR_AREA)))
    );
    run_command(&mut harness, qs_layout::CLOSE_SPLIT);
    harness.run();
    let shell = &harness.state().as_ref().unwrap().shell;
    assert!(shell.second_pane_editor().is_none());
}

/// Runs git in `dir` as a person would; its stdout.
fn git_cli(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

// No spec statement: P4-35, UI-19: the Git tab lists a changed file, stages it and commits
// with a message, and git status then says what the tab says; the editor marks the file's
// change against HEAD in its gutter
#[test]
fn the_git_tab_stages_and_commits_and_the_gutter_marks_changes() {
    let dir = TempDir::new("git-tab");
    let project = dir.path().join("gitmod");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    fs::write(project.join("world.qc"), "void() worldspawn = {};\n").unwrap();
    fs::write(project.join(".gitignore"), "build/\n").unwrap();
    git_cli(&project, &["init", "-q", "-b", "main"]);
    for (key, value) in [
        ("user.name", "Tester"),
        ("user.email", "tester@example.invalid"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
    ] {
        git_cli(&project, &["config", key, value]);
    }
    git_cli(&project, &["add", "-A"]);
    git_cli(&project, &["commit", "-q", "-m", "first"]);
    fs::write(
        project.join("world.qc"),
        "void() worldspawn = { local float a; };\n",
    )
    .unwrap();

    let mut harness = accessible();
    let app = harness.state_mut().as_mut().unwrap();
    app.shell_mut().open_project(&project).unwrap();
    app.shell_mut()
        .execute("file.open", json!({ "path": project.join("world.qc") }))
        .unwrap();
    harness.run();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    let marks = shell.editor_mut().unwrap().view.gutter_diff().cloned();
    assert!(
        marks.is_some_and(|diff| diff.is_added(0)),
        "line 1 changed since HEAD"
    );

    // Room for the whole tab, its commit box included, without scrolling.
    harness
        .state_mut()
        .as_mut()
        .unwrap()
        .shell
        .execute(
            qs_layout::RESIZE_PANEL,
            json!({ "panel": "bottom", "size": 560.0 }),
        )
        .unwrap();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, Text::Git.get())
        .click();
    harness.run();
    harness.run();
    assert!(
        harness.query_all_by_label("world.qc").count() >= 2,
        "the tab lists the file beside the editor's tab"
    );
    harness
        .get_by_role_and_label(Role::Button, Text::Stage.get())
        .click();
    harness.run();
    assert_eq!(
        git_cli(&project, &["status", "--porcelain"]).trim(),
        "M  world.qc"
    );
    harness
        .get_by_role_and_label(Role::MultilineTextInput, Text::CommitMessage.get())
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::MultilineTextInput, Text::CommitMessage.get())
        .type_text("Add a local");
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, Text::Commit.get())
        .click();
    harness.run();
    harness.run();
    assert_eq!(git_cli(&project, &["status", "--porcelain"]), "");
    assert_eq!(
        git_cli(&project, &["log", "-1", "--format=%s"]).trim(),
        "Add a local"
    );
    harness.get_by_label(Text::NoChanges.get());
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    let marks = shell.editor_mut().unwrap().view.gutter_diff().cloned();
    assert!(
        marks.is_some_and(|diff| diff.is_empty()),
        "nothing changed since the new HEAD"
    );
}

// No spec statement: P4-34, UI-18: the editor shows a warning at its line's end with a lightbulb;
// a click on it lists the quick fixes and choosing one fixes it; inlay hints name each
// argument's parameter; Cmd+Shift+I formats the file
#[test]
fn notes_quick_fixes_inlay_hints_and_format_in_the_editor() {
    let dir = TempDir::new("fixes");
    let project = dir.path().join("fixmod");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    let text =
        "void(float speed) g = {};\nvoid() worldspawn =\n{\n  local float unused;\n\tg (1);\n};\n";
    fs::write(project.join("world.qc"), text).unwrap();
    let mut harness = accessible();
    let app = harness.state_mut().as_mut().unwrap();
    app.shell_mut().open_project(&project).unwrap();
    app.shell_mut()
        .execute("file.open", json!({ "path": project.join("world.qc") }))
        .unwrap();
    harness.run();
    harness.run();
    let shown = |harness: &Harness<'static, Option<App>>| {
        let shell = &harness.state().as_ref().unwrap().shell;
        shell.editor_text().unwrap_or_default()
    };
    // The inlay hint and the note are painted in the editor.
    let ctx = harness.ctx.clone();
    let app = harness.state_mut().as_mut().unwrap();
    let painted = frame_with(&ctx, app, Vec::new());
    assert!(
        painted.iter().any(|t| t.contains("speed: ")),
        "the inlay hint: {painted:?}"
    );
    assert!(painted.iter().any(|t| t == "Local unused is never used"));
    // The quick fix, through the editor's request as a click on the note asks it.
    let offset = text.find("unused").unwrap();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell.handle_requests_for_test(vec![Request::Fixes(offset)]);
    harness.run();
    harness
        .get_by_label("Remove the unused local unused")
        .click();
    harness.run();
    harness.run();
    assert!(!shown(&harness).contains("unused"), "{}", shown(&harness));
    // Cmd+Shift+I formats: the body's lines are indented with tabs.
    run_command(&mut harness, qs_layout::FOCUS_EDITOR);
    harness.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::I,
    );
    harness.run();
    harness.run();
    assert!(
        shown(&harness).contains("{\n\tg (1);\n};"),
        "{}",
        shown(&harness)
    );
}

/// A copy of the map editor's fixture project in a fresh directory named `name`.
fn keep_copy(name: &str) -> PathBuf {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/keep");
    let dir = std::env::temp_dir().join(format!("qs-desktop-keep-{name}-{}", std::process::id()));
    let _gone = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("maps")).unwrap();
    for file in ["progs.src", "defs.qc", "world.qc", "maps/keep.map"] {
        fs::copy(fixture.join(file), dir.join(file)).unwrap();
    }
    dir
}

/// The open map's text in `harness`'s app.
fn map_text(harness: &Harness<'static, Option<App>>) -> String {
    let shell = harness.state().as_ref().unwrap().shell();
    let map = shell.session().context().workbench.map().unwrap();
    String::from_utf8(map.file().to_bytes()).unwrap()
}

// MAP-20, P5-06: the map view takes TrenchBroom's keys on the selection while it has the
// keyboard: the arrows and Page Up move by the 16-unit grid along the camera's nearest axis,
// Alt+Right turns a quarter, Cmd+D duplicates, Delete deletes and Cmd+S saves; each through a
// command, so each undoes
#[test]
fn the_map_view_takes_trenchbrooms_keys() {
    let dir = keep_copy("keys");
    let mut harness = accessible();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute("project.open", json!({ "path": dir }))
        .unwrap();
    shell
        .execute("map.open", json!({ "path": dir.join("maps/keep.map") }))
        .unwrap();
    harness.run();
    harness.get_by_label(Text::MapEditorView.get()).click();
    harness.run();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell.execute("map.select", json!({ "entity": 2 })).unwrap();
    harness.run();
    let before = map_text(&harness);
    assert!(before.contains("\"origin\" \"96 64 24\""), "{before}");
    // The camera starts facing the player start's angle 0, along +x.
    harness.key_press(egui::Key::ArrowUp);
    harness.run();
    assert!(map_text(&harness).contains("\"origin\" \"112 64 24\""));
    harness.key_press(egui::Key::PageUp);
    harness.run();
    assert!(map_text(&harness).contains("\"origin\" \"112 64 40\""));
    harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowRight);
    harness.run();
    assert!(map_text(&harness).contains("\"angle\" \"270\""));
    let entities = |harness: &Harness<'static, Option<App>>| {
        let shell = harness.state().as_ref().unwrap().shell();
        let map = shell.session().context().workbench.map().unwrap();
        (map.file().entities.len(), map.selected())
    };
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::D);
    harness.run();
    assert_eq!(entities(&harness), (6, Some(EntityIndex::new(5))));
    harness.key_press(egui::Key::Delete);
    harness.run();
    assert_eq!(entities(&harness), (5, None));
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.run();
    let saved = fs::read_to_string(dir.join("maps/keep.map")).unwrap();
    assert!(
        saved.contains("\"origin\" \"112 64 40\"\n\"angle\" \"270\""),
        "{saved}"
    );
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    for _ in 0..5 {
        shell.execute("edit.undo", json!({})).unwrap();
    }
    harness.run();
    assert_eq!(map_text(&harness), before);
}

/// Drags with the primary button from `from` to `to` in steps, holding `modifiers`.
fn drag(
    harness: &mut Harness<'static, Option<App>>,
    from: Pos2,
    to: Pos2,
    modifiers: egui::Modifiers,
) {
    harness.event(egui::Event::ModifiersChanged(modifiers));
    harness.hover_at(from);
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers,
    });
    harness.run();
    for step in 1..=4_u8 {
        let t = f32::from(step) / 4.0;
        harness.hover_at(from + (to - from) * t);
        harness.run();
    }
    harness.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers,
    });
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.run();
    harness.run();
}

/// Where the map editor drew `entity`'s centre in `harness`'s last frame.
fn drawn_at(harness: &Harness<'static, Option<App>>, entity: usize) -> Pos2 {
    let shell = harness.state().as_ref().unwrap().shell();
    shell
        .map_editor()
        .drawn_at(EntityIndex::new(entity))
        .unwrap()
}

/// The keys and values of the open map's entity `entity` in `harness`'s app.
fn map_value(harness: &Harness<'static, Option<App>>, entity: usize, key: &str) -> Option<String> {
    let shell = harness.state().as_ref()?.shell();
    let map = shell.session().context().workbench.map()?;
    map.file()
        .entities
        .get(entity)?
        .value(key.as_bytes())
        .map(|value| String::from_utf8_lossy(value).into_owned())
}

/// The middle of the entity browser's cell for `class`: of the buttons named so, the lowest,
/// below the view, since the map outline names its rows by classname too.
fn browser_cell(harness: &Harness<'static, Option<App>>, class: &str) -> Pos2 {
    harness
        .query_all_by_role_and_label(Role::Button, class)
        .map(|node| node.rect())
        .max_by(|a, b| a.center().y.total_cmp(&b.center().y))
        .unwrap()
        .center()
}

// No spec statement: P5-13, MAP-27: the entity browser lists every class the QuakeC defines and
// a point class dragged from it into the view is placed; the eyedropper beside a key the code
// fires by points it at the entity clicked, naming it; an Alt-drag from the selection links the
// entity dropped on; Shift adds an entity to the selection, and a spawnflag ticked, named by the
// code, sets every selected entity; each through a command, so each undoes
#[test]
fn entities_are_placed_and_linked_without_typing() {
    let dir = keep_copy("browser");
    let mut harness = accessible();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute("project.open", json!({ "path": dir }))
        .unwrap();
    shell
        .execute("map.open", json!({ "path": dir.join("maps/keep.map") }))
        .unwrap();
    harness.run();
    harness.run();
    lowest(&harness, Text::Entities.get()).click();
    harness.run();
    let before = map_text(&harness);
    for class in ["info_player_start", "monster_ogre", "light", "func_door"] {
        browser_cell(&harness, class);
    }
    assert!(
        harness
            .query_all_by_value("4 of 4 entities")
            .next()
            .is_some()
    );
    let view = harness.get_by_label(Text::MapEditorView.get()).rect();
    let light = browser_cell(&harness, "light");
    drag(&mut harness, light, view.center(), egui::Modifiers::NONE);
    assert_eq!(
        map_value(&harness, 5, "classname").as_deref(),
        Some("light")
    );
    let origin = map_value(&harness, 5, "origin").unwrap();
    assert!(
        origin
            .split(' ')
            .all(|c| c.parse::<i64>().unwrap().rem_euclid(16) == 0),
        "on the grid: {origin}"
    );
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell.execute("edit.undo", json!({})).unwrap();
    // The eyedropper: the ogre's target pointed at the player start, which takes a name.
    shell.execute("map.select", json!({ "entity": 2 })).unwrap();
    harness.run();
    harness.run();
    harness.get_by_label(Text::MapPick.get()).click();
    harness.run();
    let start = drawn_at(&harness, 1);
    harness.hover_at(start);
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.event(egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    harness.run();
    assert_eq!(
        map_value(&harness, 2, "target").as_deref(),
        Some("info_player_start1")
    );
    assert_eq!(
        map_value(&harness, 1, "targetname").as_deref(),
        Some("info_player_start1")
    );
    // An Alt-drag from the ogre onto the door sets the same key again.
    let (ogre, door) = (drawn_at(&harness, 2), drawn_at(&harness, 4));
    drag(&mut harness, ogre, door, egui::Modifiers::ALT);
    assert_eq!(map_value(&harness, 2, "target").as_deref(), Some("gate"));
    // Shift adds the ogre to the door's selection; the door's spawnflag is set on both, the
    // ogre keeping its own flags.
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute(
            "map.set_key",
            json!({ "entity": 2, "key": "spawnflags", "value": "256" }),
        )
        .unwrap();
    shell.execute("map.select", json!({ "entity": 4 })).unwrap();
    harness.run();
    harness.run();
    let ogre = drawn_at(&harness, 2);
    harness.hover_at(ogre);
    harness.run();
    for pressed in [true, false] {
        harness.event_modifiers(
            egui::Event::PointerButton {
                pos: ogre,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::SHIFT,
            },
            egui::Modifiers::SHIFT,
        );
    }
    harness.run();
    harness.run();
    assert!(
        harness
            .query_all_by_value("1 more selected: key edits set it too")
            .next()
            .is_some()
    );
    harness.get_by_label("DOOR_START_OPEN").click();
    harness.run();
    harness.run();
    assert_eq!(map_value(&harness, 4, "spawnflags").as_deref(), Some("1"));
    assert_eq!(map_value(&harness, 2, "spawnflags").as_deref(), Some("257"));
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    for _ in 0..4 {
        shell.execute("edit.undo", json!({})).unwrap();
    }
    harness.run();
    assert_eq!(map_text(&harness), before);
}

// No spec statement: P5-13, MAP-27: the filter by skill and game is found again once the map is
// edited, and one that could not be found shows every entity
#[test]
fn the_map_filter_is_found_again_after_an_edit() {
    use crate::map_editor::{MapEditor, ShowFilter};
    use qs_workbench::map_edit::{GameMode, MapRevision, Skill};

    let mut editor = MapEditor::default();
    let read = MapRevision::default();
    let edited = read.next();
    assert_eq!(
        editor.filter_stale(read),
        None,
        "every entity needs no filter"
    );
    editor.set_hidden(ShowFilter::Hard, Some(vec![EntityIndex::new(2)]), read);
    assert_eq!(editor.filter_stale(read), None);
    assert_eq!(editor.filter_stale(edited), Some(ShowFilter::Hard));
    editor.set_hidden(ShowFilter::Deathmatch, None, edited);
    assert_eq!(
        editor.filter_stale(edited.next()),
        None,
        "a failed filter shows every entity"
    );
    assert_eq!(ShowFilter::Every.game(), None);
    assert_eq!(
        ShowFilter::Coop.game(),
        Some((Skill::Normal, GameMode::Coop))
    );
    assert_eq!(
        ShowFilter::Nightmare.game(),
        Some((Skill::Nightmare, GameMode::Single))
    );
}

/// A TrenchBroom map with two linked groups and a locked layer holding a light (P5-14).
const LINKED_MAP: &str = "{\n\"classname\" \"worldspawn\"\n}\n\
{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\"_tb_name\" \"group\"\n\"_tb_id\" \"1\"\n\
\"_tb_linked_group_id\" \"{38b3b39d-a165-4999-985d-d40563ce51c1}\"\n\
\"_tb_transformation\" \"1 0 0 128 0 1 0 0 0 0 1 0 0 0 0 1\"\n}\n\
{\n\"classname\" \"monster_army\"\n\"origin\" \"128 0 0\"\n\"_tb_group\" \"1\"\n}\n\
{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\"_tb_name\" \"group\"\n\"_tb_id\" \"2\"\n\
\"_tb_linked_group_id\" \"{38b3b39d-a165-4999-985d-d40563ce51c1}\"\n}\n\
{\n\"classname\" \"monster_army\"\n\"origin\" \"0 0 0\"\n\"_tb_group\" \"2\"\n}\n\
{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_layer\"\n\"_tb_name\" \"Lights\"\n\"_tb_id\" \"3\"\n\
\"_tb_layer_locked\" \"1\"\n}\n\
{\n\"classname\" \"light\"\n\"origin\" \"0 0 64\"\n\"_tb_layer\" \"3\"\n}\n";

// No spec statement: P5-14, MAP-26: TrenchBroom's Isolate (Ctrl+I, Cmd on macOS) hides all but
// the selection and Show All (Ctrl+Shift+I) shows it again; the Hide button hides the
// selection; a layer's Locked checkbox writes the layer's setting, through commands with undo
#[test]
fn the_overview_keys_and_layer_settings_work() {
    let dir = std::env::temp_dir().join(format!("qs-desktop-linked-{}", std::process::id()));
    let _gone = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("linked.map");
    fs::write(&path, LINKED_MAP).unwrap();
    let mut harness = accessible();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell.execute("map.open", json!({ "path": path })).unwrap();
    harness.run();
    let hidden = |harness: &Harness<'static, Option<App>>| {
        let shell = harness.state().as_ref().unwrap().shell();
        let map = shell.session().context().workbench.map().unwrap();
        (1..7)
            .filter(|&e| map.is_hidden(EntityIndex::new(e)))
            .collect::<Vec<_>>()
    };
    harness.get_by_label(Text::MapEditorView.get()).click();
    harness.run();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell.execute("map.select", json!({ "entity": 4 })).unwrap();
    harness.run();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::I);
    harness.run();
    assert_eq!(hidden(&harness), [1, 2, 3, 5, 6]);
    harness.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::I,
    );
    harness.run();
    assert!(hidden(&harness).is_empty());
    harness.get_by_label(Text::MapHide.get()).click();
    harness.run();
    assert_eq!(hidden(&harness), [4]);
    harness.get_by_label(Text::MapLayerLocked.get()).click();
    harness.run();
    harness.run();
    assert!(!map_text(&harness).contains("_tb_layer_locked"));
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell.execute("edit.undo", json!({})).unwrap();
    harness.run();
    assert_eq!(map_text(&harness), LINKED_MAP);
}

// No spec statement: P5-16, MAP-31: the map outline lists the map; choosing a row selects the
// entity and frames it in the view with no camera movement by hand; dragging a row onto a
// layer moves the entity into it
#[test]
fn the_outline_selects_frames_and_moves() {
    let dir = std::env::temp_dir().join(format!("qs-desktop-outline-{}", std::process::id()));
    let _gone = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("outline.map");
    let map =
        format!("{LINKED_MAP}{{\n\"classname\" \"monster_ogre\"\n\"origin\" \"2000 2000 0\"\n}}\n");
    fs::write(&path, &map).unwrap();
    let mut harness = accessible();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell.execute("map.open", json!({ "path": path })).unwrap();
    harness.run();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "monster_ogre")
        .click();
    harness.run();
    harness.run();
    harness.run();
    let shell = harness.state().as_ref().unwrap().shell();
    let selected = shell
        .session()
        .context()
        .workbench
        .map()
        .unwrap()
        .selected();
    assert_eq!(selected, Some(EntityIndex::new(7)));
    let view = harness.get_by_label(Text::MapEditorView.get()).rect();
    let at = drawn_at(&harness, 7);
    assert!(
        (at - view.center()).length() < 4.0,
        "framed in the middle of the view: {at:?} in {view:?}"
    );
    let ogre = harness
        .get_by_role_and_label(Role::Button, "monster_ogre")
        .rect()
        .center();
    let lights = harness
        .get_by_role_and_label(Role::Button, "Lights")
        .rect()
        .center();
    drag(&mut harness, ogre, lights, egui::Modifiers::NONE);
    harness.run();
    assert!(
        map_text(&harness).ends_with("\"origin\" \"2000 2000 0\"\n\"_tb_layer\" \"3\"\n}\n"),
        "{}",
        map_text(&harness)
    );
}

/// The number of brushes of the open map's entity `entity` in `harness`'s app.
#[expect(
    clippy::indexing_slicing,
    reason = "test code: a missing entity is a failed test"
)]
fn brush_count(harness: &Harness<'static, Option<App>>, entity: usize) -> usize {
    let shell = harness.state().as_ref().unwrap().shell();
    let map = shell.session().context().workbench.map().unwrap();
    map.file().entities[entity].brushes().count()
}

/// Clicks with the primary button at `at` in `harness`'s app.
fn click_at(harness: &mut Harness<'static, Option<App>>, at: Pos2) {
    click_with(harness, at, egui::Modifiers::NONE);
}

/// Clicks with the primary button at `at` in `harness`'s app, `modifiers` held.
fn click_with(harness: &mut Harness<'static, Option<App>>, at: Pos2, modifiers: egui::Modifiers) {
    harness.hover_at(at);
    harness.event(egui::Event::ModifiersChanged(modifiers));
    harness.run();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        });
    }
    harness.run();
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.run();
}

// No spec statement: P6-22, MAP-13: with the Select tool an Alt-click picks the face in the way,
// the UV editor flips its texture through a command that undoes, and a plain click lets the
// face go
#[test]
fn alt_click_picks_a_face_for_the_uv_editor() {
    let dir = keep_copy("uv-faces");
    let mut harness = accessible();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute("project.open", json!({ "path": dir }))
        .unwrap();
    shell
        .execute("map.open", json!({ "path": dir.join("maps/keep.map") }))
        .unwrap();
    harness.run();
    harness.run();
    let before = map_text(&harness);
    assert!(harness.query_by_label(Text::MapFaces.get()).is_none());
    let at = drawn_at(&harness, 4);
    click_with(&mut harness, at, egui::Modifiers::ALT);
    assert!(
        harness.query_by_label(Text::MapFaces.get()).is_some(),
        "an Alt-click with the Select tool picks a face"
    );
    harness.get_by_label(Text::MapUvFlipS.get()).click();
    harness.run();
    harness.run();
    assert_ne!(map_text(&harness), before, "Flip S changed the face");
    click_at(&mut harness, at);
    assert!(harness.query_by_label(Text::MapFaces.get()).is_none());
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell.execute("edit.undo", json!({})).unwrap();
    harness.run();
    assert_eq!(map_text(&harness), before);
}

// No spec statement: P6-08, MAP-2: a click with a brush tool selects the first brush the ray
// enters, by its faces, and Delete brush in Properties deletes it; the Brush tool draws a box on
// the floor with a drag; each through a command, so each undoes
#[test]
fn brushes_are_selected_deleted_and_drawn_in_the_view() {
    let dir = keep_copy("brushes");
    let mut harness = accessible();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute("project.open", json!({ "path": dir }))
        .unwrap();
    shell
        .execute("map.open", json!({ "path": dir.join("maps/keep.map") }))
        .unwrap();
    harness.run();
    harness.run();
    let before = map_text(&harness);
    let (world, door) = (brush_count(&harness, 0), brush_count(&harness, 4));
    harness.get_by_label(Text::MapToolVertex.get()).click();
    harness.run();
    // The camera starts behind the player start, outside the west wall: a click on the door
    // takes the wall, the first brush in the way, and once it is gone, the door.
    let at = drawn_at(&harness, 4);
    click_at(&mut harness, at);
    harness.get_by_label(Text::MapDeleteBrush.get()).click();
    harness.run();
    harness.run();
    let after = map_text(&harness);
    assert_eq!(
        (brush_count(&harness, 0), brush_count(&harness, 4)),
        (world - 1, door)
    );
    assert!(!after.contains("( -272 -256 1 )"), "the west wall went");
    click_at(&mut harness, at);
    harness.get_by_label(Text::MapDeleteBrush.get()).click();
    harness.run();
    harness.run();
    assert_eq!(
        (brush_count(&harness, 0), brush_count(&harness, 4)),
        (world - 1, door - 1)
    );
    harness.get_by_label(Text::MapToolBrush.get()).click();
    harness.run();
    let view = harness.get_by_label(Text::MapEditorView.get()).rect();
    let below = view.center() + egui::vec2(0.0, view.height() / 3.0);
    drag(
        &mut harness,
        below - egui::vec2(60.0, 0.0),
        below + egui::vec2(60.0, 20.0),
        egui::Modifiers::NONE,
    );
    assert_eq!(brush_count(&harness, 0), world, "{}", map_text(&harness));
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    for _ in 0..3 {
        shell.execute("edit.undo", json!({})).unwrap();
    }
    harness.run();
    assert_eq!(map_text(&harness), before);
}

/// What the Move or Measure tool of `harness`'s app last showed.
fn shown(harness: &Harness<'static, Option<App>>) -> Option<Readout> {
    harness
        .state()
        .as_ref()?
        .shell()
        .map_editor()
        .tools()
        .shown()
}

// No spec statement: P6-09, MAP-11: in the top view the Move tool drags the selected brush by
// the move it shows, in Quake units, its textures locked, and the move undoes; the Measure tool
// shows the distance between the points dragged between
#[test]
#[expect(clippy::panic, reason = "test code: another readout is a failed test")]
fn the_move_and_measure_tools_show_the_edit_they_make() {
    let dir = keep_copy("move-measure");
    let mut harness = accessible();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute("project.open", json!({ "path": dir }))
        .unwrap();
    shell
        .execute("map.open", json!({ "path": dir.join("maps/keep.map") }))
        .unwrap();
    shell
        .execute("view.map_panes", json!({ "panes": "quad" }))
        .unwrap();
    harness.run();
    harness.run();
    let before = map_text(&harness);
    harness.get_by_label(Text::MapToolMove.get()).click();
    harness.run();
    // The first brush in the way of a click on the door is the west wall.
    let at = drawn_at(&harness, 4);
    click_at(&mut harness, at);
    let top = harness.get_by_label(Text::MapViewTop.get()).rect();
    // Two map units a point: 64 points right and 32 up is 128 east and 64 north.
    drag(
        &mut harness,
        top.center(),
        top.center() + egui::vec2(64.0, -32.0),
        egui::Modifiers::NONE,
    );
    harness.run();
    let Some(Readout::Move { by, .. }) = shown(&harness) else {
        panic!("a move readout: {:?}", shown(&harness));
    };
    assert_eq!(by, [128.0, 64.0, 0.0]);
    let after = map_text(&harness);
    assert!(!after.contains("( -272 -256 1 )"), "{after}");
    assert!(
        after.contains("( -144 -192 1 )"),
        "the wall moved by the move shown: {after}"
    );
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell.execute("edit.undo", json!({})).unwrap();
    harness.run();
    assert_eq!(map_text(&harness), before);
    harness.get_by_label(Text::MapToolMeasure.get()).click();
    harness.run();
    drag(
        &mut harness,
        top.center(),
        top.center() + egui::vec2(64.0, -48.0),
        egui::Modifiers::NONE,
    );
    harness.run();
    let Some(measured @ Readout::Measure { from, to }) = shown(&harness) else {
        panic!("a measure readout: {:?}", shown(&harness));
    };
    assert_eq!(
        [to[0] - from[0], to[1] - from[1], to[2] - from[2]],
        [128.0, 96.0, 0.0]
    );
    assert_eq!(measured.text(), "Distance 160.00 (128 96 0)");
    assert_eq!(map_text(&harness), before, "measuring changes nothing");
}

// No spec statement: P6-11, QBSP-9.8, MAP-6.8: with a wall gone, Check for leaks reports the leak and
// draws its trail; Fly along the leak takes the camera from the entity the void reaches along
// the trail to its end; an edit leaves the report out of date
#[test]
fn a_leak_is_found_and_flown_along() {
    let dir = keep_copy("leak");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/keep/gfx/keep.wad");
    fs::create_dir_all(dir.join("gfx")).unwrap();
    fs::copy(fixture, dir.join("gfx/keep.wad")).unwrap();
    let mut harness = accessible();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute("project.open", json!({ "path": dir }))
        .unwrap();
    shell
        .execute("map.open", json!({ "path": dir.join("maps/keep.map") }))
        .unwrap();
    harness.run();
    harness.get_by_label(Text::MapCheckLeaks.get()).click();
    harness.run();
    harness.run();
    let editor = |harness: &Harness<'static, Option<App>>| {
        let shell = harness.state().as_ref().unwrap().shell();
        shell.workbench().map().unwrap().report().cloned()
    };
    let sealed = editor(&harness).unwrap();
    assert!(sealed.leak.is_empty(), "{:?}", sealed.leak);
    assert!(harness.query_by_label(Text::MapSealed.get()).is_some());
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute("map.brush_delete", json!({ "entity": 0, "brush": 2 }))
        .unwrap();
    shell.execute("map.leaks", json!({})).unwrap();
    harness.run();
    let trail = editor(&harness).unwrap().leak;
    assert!(trail.len() >= 2, "{trail:?}");
    harness.get_by_label(Text::MapFlyLeak.get()).click();
    // A frame at a time: run() would go on while the flight asks for the next frame.
    harness.step();
    harness.step();
    let shell = harness.state().as_ref().unwrap().shell();
    assert!(shell.map_editor().leaks().flying());
    let first = shell.map_editor().camera().unwrap().origin;
    assert!(
        first
            .iter()
            .zip(trail.first().copied().unwrap_or_default())
            .any(|(a, b)| (a - b).abs() > 1e-3),
        "it moved"
    );
    for _ in 0..600 {
        harness.step();
        if !harness
            .state()
            .as_ref()
            .unwrap()
            .shell()
            .map_editor()
            .leaks()
            .flying()
        {
            break;
        }
    }
    let shell = harness.state().as_ref().unwrap().shell();
    assert!(!shell.map_editor().leaks().flying(), "the flight ends");
    let end = *trail.last().unwrap();
    let at = shell.map_editor().camera().unwrap().origin;
    assert!(
        at.iter().zip(end).all(|(a, b)| (a - b).abs() < 1e-3),
        "{at:?} at the trail's end {end:?}"
    );
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell.execute("edit.undo", json!({})).unwrap();
    harness.run();
    assert!(
        harness.query_by_label(Text::MapFlyLeak.get()).is_none(),
        "an edit leaves the report out of date"
    );
    // A texture the WADs lack is listed.
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute(
            "map.brush_texture",
            json!({ "entity": 4, "brushes": [0], "texture": "nowhere" }),
        )
        .unwrap();
    shell.execute("map.leaks", json!({})).unwrap();
    harness.run();
    assert_eq!(editor(&harness).unwrap().missing_textures, ["nowhere"]);
    assert!(
        harness
            .query_by_label(&missing_textures(&["nowhere".to_owned()]))
            .is_some()
    );
}

/// Of the nodes labelled `label`, the lowest on screen: below the view, where the browsers are.
fn lowest<'h>(
    harness: &'h Harness<'static, Option<App>>,
    label: &'h str,
) -> egui_kittest::Node<'h> {
    harness
        .query_all_by_label(label)
        .max_by(|a, b| a.rect().center().y.total_cmp(&b.rect().center().y))
        .unwrap()
}

/// A texture lump `name`, 16 by 16, every pixel `index`.
fn miptex(name: &str, index: u8) -> Vec<u8> {
    let mut lump = name.as_bytes().to_vec();
    lump.resize(16, 0);
    for value in [16_u32, 16, 40, 40 + 256, 40 + 256 + 64, 40 + 256 + 64 + 16] {
        lump.extend_from_slice(&value.to_le_bytes());
    }
    lump.resize(40 + 256 + 64 + 16 + 4, index);
    lump
}

/// Writes a WAD2 file of `count` textures, `tex00` onwards, at `path`.
fn write_wad(path: &Path, count: u8) {
    let mut builder = WadBuilder::new();
    for n in 0..count {
        let name = format!("tex{n:02}");
        builder
            .add(name.as_bytes(), qs_wad::WAD_TYP_MIPTEX, miptex(&name, n))
            .unwrap();
    }
    fs::write(path, builder.to_bytes().unwrap()).unwrap();
}

/// A copy of the map editor's fixture project whose map names `textures.wad`, of `count`
/// textures, with a palette of the project's own, never id's.
fn textured_copy(name: &str, count: u8) -> PathBuf {
    let dir = keep_copy(name);
    fs::create_dir_all(dir.join("gfx")).unwrap();
    let palette: Vec<u8> = (0..=255_u8).flat_map(|i| [i, 255 - i, 64]).collect();
    fs::write(dir.join("gfx/palette.lmp"), palette).unwrap();
    write_wad(&dir.join("textures.wad"), count);
    let map = dir.join("maps/keep.map");
    let text = fs::read_to_string(&map).unwrap();
    assert!(text.contains("\"wad\" \"gfx/keep.wad\""));
    let text = text.replacen("\"wad\" \"gfx/keep.wad\"", "\"wad\" \"textures.wad\"", 1);
    fs::write(&map, text).unwrap();
    dir
}

/// `harness` with `dir`'s project and its map open, drawn.
fn open_textured(harness: &mut Harness<'static, Option<App>>, dir: &Path) {
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute("project.open", json!({ "path": dir }))
        .unwrap();
    shell
        .execute("map.open", json!({ "path": dir.join("maps/keep.map") }))
        .unwrap();
    harness.run();
    harness.run();
}

// No spec statement: P6-10, MAP-5: the texture browser lists every texture of the WAD the
// world names and every name the map's faces use, each with its count of faces, in the tab
// below the view; it decodes exactly the textures whose cells are on screen
#[test]
fn the_texture_browser_shows_the_map_s_textures_decoding_only_those_drawn() {
    let dir = textured_copy("textures", 60);
    let mut harness = accessible();
    open_textured(&mut harness, &dir);
    let shell = harness.state().as_ref().unwrap().shell();
    let browser = shell.map_editor().texture_browser();
    let listing = browser.listing();
    let total = listing.textures.len();
    let names: BTreeSet<&str> = listing.textures.iter().map(|t| t.name.as_str()).collect();
    for n in 0..60 {
        assert!(names.contains(format!("tex{n:02}").as_str()), "tex{n:02}");
    }
    let map = shell.session().context().workbench.map().unwrap();
    for entity in &map.file().entities {
        for brush in entity.brushes() {
            for face in &brush.faces {
                let name = String::from_utf8_lossy(face.texture()).into_owned();
                assert!(names.contains(name.as_str()), "{name}");
            }
        }
    }
    let uses: usize = listing.textures.iter().map(|t| t.uses).sum();
    let faces: usize = map
        .file()
        .entities
        .iter()
        .flat_map(|e| e.brushes())
        .map(|b| b.faces.len())
        .sum();
    assert_eq!(uses, faces);
    // The cells drawn are those within the browser, as wide as the view above it; each
    // decoded once, the rest never.
    let view = harness.get_by_label(Text::MapEditorView.get()).rect();
    let on_screen = (0..60)
        .filter(|n| {
            harness
                .query_all_by_label(&format!("tex{n:02}"))
                .any(|node| node.rect().left() < view.right() && node.rect().right() > view.left())
        })
        .count();
    let decodes = harness
        .state()
        .as_ref()
        .unwrap()
        .shell()
        .map_editor()
        .texture_browser()
        .decodes();
    assert!(on_screen > 0 && on_screen < 60, "{on_screen} on screen");
    assert_eq!(decodes, on_screen);
    harness.run();
    let again = harness
        .state()
        .as_ref()
        .unwrap()
        .shell()
        .map_editor()
        .texture_browser()
        .decodes();
    assert_eq!(again, decodes, "a picture is decoded once");
    let summary = format!("textures.wad, {total} textures");
    assert!(
        harness.query_all_by_label(&summary).next().is_some(),
        "{summary}"
    );
}

// No spec statement: P6-10, MAP-32: a texture another program adds to a WAD the map uses shows
// in the browser with no restart
#[test]
fn a_texture_added_to_the_wad_shows_without_a_restart() {
    let dir = textured_copy("texture-added", 3);
    let mut harness = accessible();
    open_textured(&mut harness, &dir);
    let count = |harness: &Harness<'static, Option<App>>| {
        harness
            .state()
            .as_ref()
            .unwrap()
            .shell()
            .map_editor()
            .texture_browser()
            .listing()
            .textures
            .iter()
            .filter(|t| t.name.starts_with("tex"))
            .count()
    };
    assert_eq!(count(&harness), 3);
    let wad = dir.join("textures.wad");
    let watched = harness
        .state()
        .as_ref()
        .unwrap()
        .shell()
        .map_editor()
        .texture_watched();
    assert_eq!(watched, std::slice::from_ref(&wad));
    write_wad(&wad, 4);
    harness
        .state_mut()
        .as_mut()
        .unwrap()
        .shell
        .files_written(vec![wad]);
    harness.run();
    harness.run();
    assert_eq!(count(&harness), 4);
    assert!(harness.query_all_by_label("tex03").next().is_some());
}

// No spec statement: P6-10: naming another WAD in the world's wad key gives the browser that
// WAD's textures, with no reopening
#[test]
fn a_new_wad_key_gives_the_browser_its_textures() {
    let dir = textured_copy("texture-key", 3);
    let other = dir.join("other.wad");
    let mut builder = WadBuilder::new();
    builder
        .add(b"stone", qs_wad::WAD_TYP_MIPTEX, miptex("stone", 7))
        .unwrap();
    fs::write(&other, builder.to_bytes().unwrap()).unwrap();
    let mut harness = accessible();
    open_textured(&mut harness, &dir);
    harness
        .state_mut()
        .as_mut()
        .unwrap()
        .shell
        .execute(
            "map.set_key",
            json!({ "entity": 0, "key": "wad", "value": "other.wad" }),
        )
        .unwrap();
    harness.run();
    harness.run();
    let shell = harness.state().as_ref().unwrap().shell();
    let listing = shell.map_editor().texture_browser().listing();
    assert!(listing.textures.iter().any(|t| t.name == "stone"));
    assert!(!listing.textures.iter().any(|t| t.name == "tex00"));
    assert_eq!(shell.map_editor().texture_watched(), [other]);
}

// No spec statement: P6-18: the Shape tool fills a box dragged on the ground with the shape
// chosen in its toolbar, hollow with sides as set, in one step that undoes
#[test]
fn the_shape_tool_fills_a_dragged_box() {
    let dir = keep_copy("shapes");
    let mut harness = accessible();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute("project.open", json!({ "path": dir }))
        .unwrap();
    shell
        .execute("map.open", json!({ "path": dir.join("maps/keep.map") }))
        .unwrap();
    harness.run();
    harness.run();
    let before = map_text(&harness);
    let world = brush_count(&harness, 0);
    harness.get_by_label(Text::MapToolShape.get()).click();
    harness.run();
    harness.get_by_label(Text::MapShapeHollow.get()).click();
    harness.run();
    let view = harness.get_by_label(Text::MapEditorView.get()).rect();
    let below = view.center() + egui::vec2(0.0, view.height() / 3.0);
    drag(
        &mut harness,
        below - egui::vec2(160.0, 60.0),
        below + egui::vec2(160.0, 60.0),
        egui::Modifiers::NONE,
    );
    let refused = harness
        .state()
        .as_ref()
        .unwrap()
        .shell()
        .map_editor()
        .refused_text();
    assert_eq!(refused, None);
    // A hollow cylinder of the default 16 sides: a brush a side.
    assert_eq!(
        brush_count(&harness, 0),
        world + 16,
        "{}",
        map_text(&harness)
    );
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell.execute("edit.undo", json!({})).unwrap();
    harness.run();
    assert_eq!(map_text(&harness), before);
}

/// A file dropped on the window, by its path.
#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        fs::read(&self.0).map_err(|error| error.to_string())
    }
}

// No spec statement: P6-16, MAP-23: a PNG dropped on the texture browser opens its import
// preview, named after the file; Import writes it into the project's WAD, the map's world
// names the WAD, and the browser lists the texture
#[test]
fn a_dropped_png_is_imported_into_the_browser() {
    let dir = keep_copy("import");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/keep/gfx");
    fs::create_dir_all(dir.join("gfx")).unwrap();
    for file in ["keep.wad", "palette.lmp"] {
        fs::copy(fixture.join(file), dir.join("gfx").join(file)).unwrap();
    }
    let image = dir.join("Brick16.png");
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([200, 40, 30, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    fs::write(&image, bytes.into_inner()).unwrap();
    let mut harness = accessible();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute("project.open", json!({ "path": dir }))
        .unwrap();
    shell
        .execute("map.open", json!({ "path": dir.join("maps/keep.map") }))
        .unwrap();
    harness.run();
    harness.run();
    let over = lowest(&harness, "wall").rect().center();
    harness.hover_at(over);
    harness
        .input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(image.clone())));
    harness.run();
    let browser = |harness: &Harness<'static, Option<App>>| {
        let shell = harness.state().as_ref().unwrap().shell();
        shell
            .map_editor()
            .texture_browser()
            .importing()
            .map(|i| i.name().to_owned())
    };
    assert_eq!(browser(&harness).as_deref(), Some("brick16"));
    harness.get_by_label(Text::MapImportSave.get()).click();
    harness.run();
    harness.run();
    assert_eq!(browser(&harness), None, "the preview closed");
    let shell = harness.state().as_ref().unwrap().shell();
    let names: Vec<&str> = shell
        .map_editor()
        .texture_browser()
        .listing()
        .textures
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert!(names.contains(&"brick16"), "{names:?}");
    let wad = project_wad(&dir);
    assert!(wad.is_file());
    assert!(map_text(&harness).contains("\"wad\" \"gfx/keep.wad;textures/"));
    // A changed image imported again replaces the texture, and the open map shows the new one.
    let decoded = |harness: &Harness<'static, Option<App>>| {
        let shell = harness.state().as_ref().unwrap().shell();
        shell.map_editor().texture_browser().decoded("brick16")
    };
    let first = decoded(&harness).unwrap();
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([30, 40, 200, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    fs::write(&image, bytes.into_inner()).unwrap();
    harness.hover_at(over);
    harness
        .input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(image.clone())));
    harness.run();
    harness.get_by_label(Text::MapImportSave.get()).click();
    harness.run();
    harness.run();
    let second = decoded(&harness).unwrap();
    assert_ne!(second.rgba, first.rgba, "the browser shows the new picture");
    let shell = harness.state().as_ref().unwrap().shell();
    let count = shell
        .map_editor()
        .texture_browser()
        .listing()
        .textures
        .iter()
        .filter(|t| t.name == "brick16")
        .count();
    assert_eq!(count, 1, "replaced, not added");
}

/// A pillar map, a box from the origin to `top` high.
fn pillar(top: u32) -> String {
    format!(
        "{{\n\"classname\" \"worldspawn\"\n{{\n\
         ( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) rock 0 0 0 1 1\n\
         ( 16 0 0 ) ( 16 0 1 ) ( 16 1 0 ) rock 0 0 0 1 1\n\
         ( 0 0 0 ) ( 0 0 1 ) ( 1 0 0 ) rock 0 0 0 1 1\n\
         ( 0 16 0 ) ( 1 16 0 ) ( 0 16 1 ) rock 0 0 0 1 1\n\
         ( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) rock 0 0 0 1 1\n\
         ( 0 0 {top} ) ( 0 1 {top} ) ( 1 0 {top} ) rock 0 0 0 1 1\n}}\n}}\n"
    )
}

// spec MAP-14.6: an external map instance is drawn from the file it names, which is watched, and
// drawn again when another program writes it
#[test]
fn an_instance_follows_its_file() {
    let dir = std::env::temp_dir().join(format!("qs-desktop-follow-{}", std::process::id()));
    let _gone = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let prefab = dir.join("pillar.map");
    fs::write(&prefab, pillar(64)).unwrap();
    let map = dir.join("follow.map");
    fs::write(
        &map,
        "{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"misc_external_map\"\n\
         \"_external_map\" \"pillar.map\"\n\"_external_map_classname\" \"func_wall\"\n}\n",
    )
    .unwrap();
    let mut harness = accessible();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell.execute("map.open", json!({ "path": map })).unwrap();
    harness.run();
    harness.run();
    let top = |harness: &Harness<'static, Option<App>>| {
        let editor = harness.state().as_ref().unwrap().shell().map_editor();
        let instance = editor
            .scene()
            .entities
            .iter()
            .find(|shape| shape.index == EntityIndex::new(1))
            .unwrap();
        instance.bounds[1][2]
    };
    assert!((top(&harness) - 64.0).abs() < 1e-3);
    let watched = harness
        .state()
        .as_ref()
        .unwrap()
        .shell()
        .map_editor()
        .external_watched();
    assert_eq!(watched, std::slice::from_ref(&prefab));
    fs::write(&prefab, pillar(128)).unwrap();
    harness
        .state_mut()
        .as_mut()
        .unwrap()
        .shell
        .files_written(vec![prefab]);
    harness.run();
    harness.run();
    assert!((top(&harness) - 128.0).abs() < 1e-3, "{}", top(&harness));
}

// No spec statement: P6-23, MAP-33: a brush selected is saved as a prefab under the name typed,
// which the Prefabs tab lists; dragged into the view it is copied in, in one step that undoes
#[test]
fn a_prefab_is_saved_and_dragged_in() {
    let dir = keep_copy("prefabs");
    let mut harness = accessible();
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute("project.open", json!({ "path": dir }))
        .unwrap();
    shell
        .execute("map.open", json!({ "path": dir.join("maps/keep.map") }))
        .unwrap();
    harness.run();
    harness.run();
    let before = brush_count(&harness, 0);
    harness.get_by_label(Text::MapToolVertex.get()).click();
    harness.run();
    // The first brush in the way of a click on the door is the west wall.
    let at = drawn_at(&harness, 4);
    click_at(&mut harness, at);
    harness
        .get_by_role_and_label(egui::accesskit::Role::TextInput, Text::MapPrefabName.get())
        .click();
    harness.run();
    harness
        .get_by_role_and_label(egui::accesskit::Role::TextInput, Text::MapPrefabName.get())
        .type_text("wall");
    harness.run();
    harness.get_by_label(Text::MapSavePrefab.get()).click();
    harness.run();
    harness.run();
    assert!(dir.join("prefabs/wall.map").is_file());
    harness.get_by_label(Text::MapPrefabs.get()).click();
    harness.get_by_label(Text::MapToolSelect.get()).click();
    harness.run();
    harness.run();
    let source = harness.get_by_label("wall").rect().center();
    let view = harness
        .get_by_label(Text::MapEditorView.get())
        .rect()
        .center();
    drag(&mut harness, source, view, egui::Modifiers::NONE);
    assert_eq!(
        brush_count(&harness, 0),
        before + 1,
        "{}",
        map_text(&harness)
    );
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell.execute("edit.undo", json!({})).unwrap();
    harness.run();
    assert_eq!(brush_count(&harness, 0), before);
}
