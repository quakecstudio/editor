// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap is a failed test; line numbers and depths are small"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use eframe::egui::{self, FontId, RawInput, Rect, Vec2};
use qs_layout::{Layout, Panel};

use crate::App;
use crate::icons::Icon;
use crate::strings::Text;
use crate::theme::{Mode, Theme, Token, semibold};

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
