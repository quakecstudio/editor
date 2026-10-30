// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use eframe::egui::{self, FontDefinitions, FontFamily, RawInput};

use super::{SystemFonts, families};
use crate::icons::Icon;
use crate::theme::Theme;

/// The bundled JetBrains Mono alone: what a "system" holding only it can give.
const MONO: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
/// Lucide's icon font, which has characters no text font has.
const ICONS: &[u8] = include_bytes!("../../assets/fonts/lucide.ttf");

/// A context with no fonts and a fallback over `bundled` that looks in a "system" of `system`.
fn fallback(bundled: &[&'static [u8]], system: &[&'static [u8]]) -> (egui::Context, SystemFonts) {
    let ctx = egui::Context::default();
    ctx.set_fonts(FontDefinitions::empty());
    (ctx, SystemFonts::with_files(bundled, system))
}

/// The number of fonts egui has after the next frame.
fn font_count(ctx: &egui::Context) -> usize {
    let mut output = ctx.run_ui(RawInput::default(), |_| {});
    output.textures_delta.clear();
    ctx.fonts(|fonts| fonts.definitions().font_data.len())
}

#[test]
fn a_character_no_font_has_is_drawn_in_a_system_font_that_has_it() {
    let (ctx, mut fonts) = fallback(&[MONO], &[MONO, ICONS]);
    let icon = Icon::Play.char();
    assert_eq!(
        fonts.cover(&ctx, &families(), "abc"),
        0,
        "the bundled font has these"
    );
    assert_eq!(fonts.cover(&ctx, &families(), &format!("x{icon}")), 1);
    assert_eq!(font_count(&ctx), 1);
    let definitions = ctx.fonts(|f| f.definitions().clone());
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        assert_eq!(
            definitions.families.get(&family).map(Vec::len),
            Some(1),
            "{family:?}"
        );
    }
    assert_eq!(
        fonts.cover(&ctx, &families(), &icon.to_string()),
        0,
        "looked for once"
    );
}

#[test]
fn a_character_no_system_font_has_is_looked_for_once() {
    let (ctx, mut fonts) = fallback(&[MONO], &[MONO]);
    let icon = Icon::Play.char();
    assert_eq!(fonts.cover(&ctx, &families(), &icon.to_string()), 0);
    assert_eq!(fonts.cover(&ctx, &families(), &icon.to_string()), 0);
    assert_eq!(fonts.searches(), 1, "a character is searched for once");
    assert_eq!(
        fonts.cover(&ctx, &families(), "\n\t"),
        0,
        "control characters are skipped"
    );
}

#[test]
fn the_bundled_fonts_cover_chinese_and_japanese() {
    let fonts = SystemFonts::new(&Theme::bundled_fonts());
    for c in "中文日本語かなカナ".chars() {
        assert!(fonts.has(c), "{c}");
    }
}

#[test]
fn the_system_folders_are_searched_for_font_files_to_any_depth() {
    let dir = std::env::temp_dir().join(format!("quakec-studio-fonts-{}", std::process::id()));
    let _ignored = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("deep/er")).unwrap();
    std::fs::write(dir.join("deep/er/icons.TTF"), ICONS).unwrap();
    std::fs::write(dir.join("notes.txt"), "not a font").unwrap();
    std::fs::write(dir.join("broken.otf"), "not a font either").unwrap();
    let files = super::font_files(&[dir.clone(), dir.join("missing")]);
    assert_eq!(
        files,
        [dir.join("broken.otf"), dir.join("deep/er/icons.TTF")]
    );
    let ctx = egui::Context::default();
    ctx.set_fonts(FontDefinitions::empty());
    let mut fonts = SystemFonts::over(
        &[MONO],
        super::Source::Folders {
            dirs: vec![dir.clone()],
            files: None,
        },
    );
    let icon = Icon::Play.char().to_string();
    assert_eq!(
        fonts.cover(&ctx, &families(), &icon),
        1,
        "found past a broken file"
    );
    let _ignored = std::fs::remove_dir_all(&dir);
}
