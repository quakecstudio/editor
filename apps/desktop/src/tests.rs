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

use eframe::egui::{self, FontId, RawInput, Rect, Vec2};
use qs_commands::palette::search;
use qs_commands::params::{self, Input};
use qs_layout::{Layout, Panel};

use crate::icons::Icon;
use crate::keymap;
use crate::shell::dragged_size;
use crate::strings::{Text, command_count};
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
    let mut app = App::new(&ctx, None).unwrap();
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
    let mut app = App::new(&ctx, Some(file.clone())).unwrap();
    frame(&ctx, &mut app);
    app.shell.toggle(Panel::Right).unwrap();
    app.shell.toggle(Panel::Bottom).unwrap();
    app.shell.resize(Panel::Left, 300.0).unwrap();
    assert!(app.shell().session().undo_depth() == 0);
    frame(&ctx, &mut app);
    let written = Layout::read(&file).unwrap();
    assert!(!written.right.visible && !written.bottom.visible && written.left.visible);
    let ctx = egui::Context::default();
    let mut again = App::new(&ctx, Some(file.clone())).unwrap();
    assert_eq!(again.shell().layout(), &written);
    frame(&ctx, &mut again);
    // A file that is not a layout starts the default layout.
    fs::write(&file, "not a layout").unwrap();
    let ctx = egui::Context::default();
    let app = App::new(&ctx, Some(file)).unwrap();
    assert_eq!(*app.shell().layout(), Layout::default());
}

// UI-2: every panel can be hidden and shown again.
#[test]
fn every_panel_toggles() {
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, None).unwrap();
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
    assert_eq!(definitions.font_data.len(), 4);
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
        ("palette.rs", include_str!("palette.rs")),
        ("menu.rs", include_str!("menu.rs")),
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
    let mut app = App::new(&ctx, None).unwrap();
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
    let mut app = App::new(&ctx, Some(file.clone())).unwrap();
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
    let mut app = App::new(&ctx, None).unwrap();
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
    let mut app = App::new(&ctx, None).unwrap();
    let error = app.shell.resize(Panel::Left, 1.0).unwrap_err();
    assert!(error.to_string().contains("at least 120 points"), "{error}");
    assert!(std::error::Error::source(&error).is_some());
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
    app.shell.palette.open();
    frame(ctx, app);
}

// UI-10: every registered command is the palette's first match for its own id, and Enter on
// it runs it or asks for its parameters.
#[test]
fn every_registered_command_is_reachable_from_the_palette() {
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, None).unwrap();
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
    let mut app = App::new(&ctx, None).unwrap();
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
    let mut app = App::new(&ctx, None).unwrap();
    frame_with(
        &ctx,
        &mut app,
        vec![key(
            egui::Key::P,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        )],
    );
    assert!(app.shell.palette.is_open(), "Cmd+Shift+P opens it");
    let none = egui::Modifiers::NONE;
    frame_with(&ctx, &mut app, vec![key(egui::Key::ArrowUp, none)]);
    for _ in 0..3 {
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
        Some("view.resize_panel"),
        "the fourth command in id order"
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
    let mut app = App::new(&ctx, None).unwrap();
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

// PRIN-3: every key binding runs a registered command that takes no parameters.
#[test]
fn every_binding_names_a_registered_command_without_parameters() {
    let ctx = egui::Context::default();
    let app = App::new(&ctx, None).unwrap();
    let registry = app.shell().session().registry();
    for binding in keymap::BINDINGS {
        let info = registry
            .commands()
            .find(|c| c.id().as_str() == binding.command)
            .expect("every binding's command is registered");
        assert!(params::fields(info.params()).is_empty());
        assert_eq!(keymap::shortcut(binding.command), Some(binding.shortcut));
    }
    assert_eq!(keymap::shortcut("view.toggle_panel"), None);
}

// PRIN-3: a binding's shortcut runs its command, and is left alone while the palette is open.
#[test]
fn a_shortcut_runs_its_command() {
    let ctx = egui::Context::default();
    let mut app = App::new(&ctx, None).unwrap();
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
    let job = crate::palette::highlighted(
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
    let mut app = App::new(&ctx, None).unwrap();
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
    let choice = prompt.inputs.get_mut(panel).unwrap();
    *choice = Input::Choice(Some(0));
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
