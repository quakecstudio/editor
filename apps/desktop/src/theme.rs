// SPDX-License-Identifier: GPL-3.0-or-later

//! The theme (UI-15, ADR-0010): colours, type and spacing read from `docs/design/tokens.toml`,
//! the one place the UI layer takes them from, and applied to egui's style.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Vec2,
};
use serde::Deserialize;

/// The design tokens, built into the app.
const TOKENS: &str = include_str!("../../../docs/design/tokens.toml");

/// The fonts, built into the app (ADR-0010).
const PLEX_SANS: &[u8] = include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf");
const PLEX_SANS_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf");
const JETBRAINS_MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");
const LUCIDE: &[u8] = include_bytes!("../assets/fonts/lucide.ttf");
const NOTO_SANS_SC: &[u8] = include_bytes!("../assets/fonts/NotoSansSC-Regular.otf");

/// The font family names egui knows the fonts by.
const UI: &str = "ui";
const UI_SEMIBOLD: &str = "ui-semibold";
const CODE: &str = "code";
const ICONS: &str = "icons";
const CJK: &str = "cjk";

/// The semibold interface family, for titles and panel headings.
#[must_use]
pub fn semibold() -> FontFamily {
    FontFamily::Name(UI_SEMIBOLD.into())
}

/// Dark, the default, or light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// `[color.dark]`.
    #[default]
    Dark,
    /// `[color.light]`.
    Light,
}

/// A colour token the app draws with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Token {
    /// The title bar and the status bar.
    SurfaceChrome,
    /// The toolbar.
    SurfaceToolbar,
    /// The tab strip above the editor.
    SurfaceTabs,
    /// The editor, the console and text inputs.
    SurfaceEditor,
    /// The side and bottom panels.
    SurfacePanel,
    /// Popovers, chips and hovered rows.
    SurfaceRaised,
    /// The game's view.
    SurfaceViewport,
    /// The keyboard selection.
    SelectFocus,
    /// Text.
    Text,
    /// Secondary text.
    TextMuted,
    /// Counts and hints, on panel surfaces only.
    TextFaint,
    /// Text on the accent fill.
    TextOnAccent,
    /// The ember accent: Play and primary actions.
    Accent,
    /// Focus rings and links.
    Focus,
    /// Errors.
    StatusError,
    /// Warnings.
    StatusWarn,
    /// Dividers.
    Border,
    /// Buttons' outlines.
    BorderStrong,
    /// The line the caret is on.
    SurfaceCurrentLine,
    /// A line new in an inline diff.
    DiffAdded,
    /// A removed line in an inline diff, and an inline diagnostic.
    StatusErrorBg,
    /// Plain code.
    TextCode,
    /// Text on `status.error_bg`.
    TextOnError,
    /// Keywords.
    CodeKeyword,
    /// Types and fields.
    CodeType,
    /// Functions and builtins.
    CodeFunction,
    /// Constants and numbers.
    CodeConstant,
    /// Strings.
    CodeString,
    /// Comments.
    CodeComment,
}

impl Token {
    /// Every token the app draws with.
    pub const ALL: [Self; 29] = [
        Self::SurfaceChrome,
        Self::SurfaceToolbar,
        Self::SurfaceTabs,
        Self::SurfaceEditor,
        Self::SurfacePanel,
        Self::SurfaceRaised,
        Self::SurfaceViewport,
        Self::SelectFocus,
        Self::Text,
        Self::TextMuted,
        Self::TextFaint,
        Self::TextOnAccent,
        Self::Accent,
        Self::Focus,
        Self::StatusError,
        Self::StatusWarn,
        Self::Border,
        Self::BorderStrong,
        Self::SurfaceCurrentLine,
        Self::DiffAdded,
        Self::StatusErrorBg,
        Self::TextCode,
        Self::TextOnError,
        Self::CodeKeyword,
        Self::CodeType,
        Self::CodeFunction,
        Self::CodeConstant,
        Self::CodeString,
        Self::CodeComment,
    ];

    /// Its name in `tokens.toml`.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::SurfaceChrome => "surface.chrome",
            Self::SurfaceToolbar => "surface.toolbar",
            Self::SurfaceTabs => "surface.tabs",
            Self::SurfaceEditor => "surface.editor",
            Self::SurfacePanel => "surface.panel",
            Self::SurfaceRaised => "surface.raised",
            Self::SurfaceViewport => "surface.viewport",
            Self::SelectFocus => "select.focus",
            Self::Text => "text",
            Self::TextMuted => "text.muted",
            Self::TextFaint => "text.faint",
            Self::TextOnAccent => "text.on_accent",
            Self::Accent => "accent",
            Self::Focus => "focus",
            Self::StatusError => "status.error",
            Self::StatusWarn => "status.warn",
            Self::Border => "border",
            Self::BorderStrong => "border.strong",
            Self::SurfaceCurrentLine => "surface.current_line",
            Self::DiffAdded => "diff.added",
            Self::StatusErrorBg => "status.error_bg",
            Self::TextCode => "text.code",
            Self::TextOnError => "text.on_error",
            Self::CodeKeyword => "code.keyword",
            Self::CodeType => "code.type",
            Self::CodeFunction => "code.function",
            Self::CodeConstant => "code.constant",
            Self::CodeString => "code.string",
            Self::CodeComment => "code.comment",
        }
    }
}

/// `tokens.toml`, the parts the app reads.
#[derive(Deserialize)]
struct Tokens {
    color: Palettes,
    type_scale: TypeScale,
    space: Space,
    size: Sizes,
    radius: Radii,
}

#[derive(Deserialize)]
struct Palettes {
    dark: BTreeMap<String, String>,
    light: BTreeMap<String, String>,
}

/// One step of the type scale.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub struct TypeStep {
    /// The size in points.
    pub size: f32,
}

/// The type scale's steps (`[type_scale]`).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub struct TypeScale {
    /// Titles.
    pub title: TypeStep,
    /// The interface's text.
    pub body: TypeStep,
    /// Panel headings.
    pub panel_heading: TypeStep,
    /// Captions and counts.
    pub caption: TypeStep,
    /// Code.
    pub code: TypeStep,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
struct Space {
    scale: [f32; 5],
}

/// Fixed sizes (`[size]`), in points.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub struct Sizes {
    /// A list row.
    pub row: f32,
    /// A panel's header.
    pub panel_header: f32,
    /// A control: a button or an input.
    pub control: f32,
    /// The status bar.
    pub status_bar: f32,
    /// The tab strip.
    pub tabs: f32,
    /// The toolbar.
    pub toolbar: f32,
}

/// Corner radii (`[radius]`), in points.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub struct Radii {
    /// Inputs.
    pub input: f32,
    /// Buttons.
    pub control: f32,
    /// Cards and popovers.
    pub card: f32,
}

/// Why the theme could not be read: the built-in tokens do not have what the app needs.
#[derive(Debug)]
pub enum ThemeError {
    /// `tokens.toml` does not parse into the tables the app reads.
    Tokens(toml::de::Error),
    /// A colour token is missing from a theme.
    Missing {
        /// The theme.
        mode: Mode,
        /// The token.
        token: Token,
    },
    /// A colour token is not `#RRGGBB`.
    BadColor {
        /// The token.
        token: Token,
        /// Its value.
        value: String,
    },
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tokens(error) => write!(f, "docs/design/tokens.toml: {error}"),
            Self::Missing { mode, token } => {
                write!(f, "the {mode:?} theme has no {} colour", token.key())
            }
            Self::BadColor { token, value } => {
                write!(f, "colour {} is {value:?}, not #RRGGBB", token.key())
            }
        }
    }
}

impl std::error::Error for ThemeError {}

/// The theme: one palette and the shared type, spacing and shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    mode: Mode,
    colors: BTreeMap<Token, Color32>,
    /// The type scale.
    pub type_scale: TypeScale,
    space: [f32; 5],
    /// Fixed sizes.
    pub size: Sizes,
    /// Corner radii.
    pub radius: Radii,
}

impl Theme {
    /// The theme of `mode` from the built-in tokens.
    ///
    /// # Errors
    ///
    /// [`ThemeError`] when the tokens lack a value the app draws with.
    pub fn load(mode: Mode) -> Result<Self, ThemeError> {
        let tokens: Tokens = toml::from_str(TOKENS).map_err(ThemeError::Tokens)?;
        let palette = match mode {
            Mode::Dark => &tokens.color.dark,
            Mode::Light => &tokens.color.light,
        };
        let mut colors = BTreeMap::new();
        for token in Token::ALL {
            let value = palette
                .get(token.key())
                .ok_or(ThemeError::Missing { mode, token })?;
            colors.insert(token, parse_color(token, value)?);
        }
        Ok(Self {
            mode,
            colors,
            type_scale: tokens.type_scale,
            space: tokens.space.scale,
            size: tokens.size,
            radius: tokens.radius,
        })
    }

    /// The colour of `token`.
    #[must_use]
    pub fn color(&self, token: Token) -> Color32 {
        self.colors.get(&token).copied().unwrap_or(Color32::MAGENTA)
    }

    /// Step `step` of the spacing scale, 0 for 4 to 4 for 24; the last step past the end.
    #[must_use]
    pub fn space(&self, step: usize) -> f32 {
        self.space
            .get(step)
            .or(self.space.last())
            .copied()
            .unwrap_or_default()
    }

    /// The bundled fonts as egui's families: the interface in IBM Plex Sans with Lucide's
    /// icons after it, code in JetBrains Mono, and the semibold for headings; each falls back
    /// to Noto Sans SC for Chinese and Japanese text (ADR-0003), and then to the system's fonts
    /// (`crate::fallback`).
    #[must_use]
    pub fn fonts() -> FontDefinitions {
        let mut fonts = FontDefinitions::empty();
        for (name, bytes) in [
            (UI, PLEX_SANS),
            (UI_SEMIBOLD, PLEX_SANS_SEMIBOLD),
            (CODE, JETBRAINS_MONO),
            (ICONS, LUCIDE),
            (CJK, NOTO_SANS_SC),
        ] {
            fonts
                .font_data
                .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
        }
        let chain = |first: &str| vec![first.to_owned(), ICONS.to_owned(), CJK.to_owned()];
        fonts.families.insert(FontFamily::Proportional, chain(UI));
        fonts.families.insert(FontFamily::Monospace, chain(CODE));
        fonts.families.insert(semibold(), chain(UI_SEMIBOLD));
        fonts
    }

    /// The bundled font files, each as its bytes.
    #[must_use]
    pub const fn bundled_fonts() -> [&'static [u8]; 5] {
        [
            PLEX_SANS,
            PLEX_SANS_SEMIBOLD,
            JETBRAINS_MONO,
            LUCIDE,
            NOTO_SANS_SC,
        ]
    }

    /// Sets egui's fonts and style to the theme.
    pub fn apply(&self, ctx: &egui::Context) {
        ctx.set_fonts(Self::fonts());
        ctx.set_visuals(match self.mode {
            Mode::Dark => egui::Visuals::dark(),
            Mode::Light => egui::Visuals::light(),
        });
        let color = |token| self.color(token);
        let scale = self.type_scale;
        let control = CornerRadius::from(self.radius.control);
        let spacing = Vec2::new(self.space(1), self.space(0));
        let interact = Vec2::new(self.size.control, self.size.row);
        ctx.global_style_mut(|style| {
            style.text_styles = [
                (
                    TextStyle::Heading,
                    FontId::new(scale.title.size, semibold()),
                ),
                (TextStyle::Body, FontId::proportional(scale.body.size)),
                (TextStyle::Button, FontId::proportional(scale.body.size)),
                (TextStyle::Small, FontId::proportional(scale.caption.size)),
                (TextStyle::Monospace, FontId::monospace(scale.code.size)),
            ]
            .into();
            style.spacing.item_spacing = spacing;
            style.spacing.interact_size = interact;
            let visuals = &mut style.visuals;
            visuals.override_text_color = Some(color(Token::Text));
            visuals.panel_fill = color(Token::SurfacePanel);
            visuals.window_fill = color(Token::SurfaceRaised);
            visuals.extreme_bg_color = color(Token::SurfaceEditor);
            visuals.faint_bg_color = color(Token::SurfaceRaised);
            visuals.hyperlink_color = color(Token::Focus);
            visuals.selection.bg_fill = color(Token::SelectFocus);
            visuals.selection.stroke = Stroke::new(1.0, color(Token::Focus));
            visuals.window_stroke = Stroke::new(1.0, color(Token::Border));
            visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, color(Token::Border));
            for widget in [
                &mut visuals.widgets.inactive,
                &mut visuals.widgets.hovered,
                &mut visuals.widgets.active,
                &mut visuals.widgets.open,
            ] {
                widget.corner_radius = control;
            }
            visuals.widgets.inactive.weak_bg_fill = color(Token::SurfacePanel);
            visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, color(Token::BorderStrong));
            visuals.widgets.hovered.weak_bg_fill = color(Token::SurfaceRaised);
            visuals.widgets.active.weak_bg_fill = color(Token::SurfaceRaised);
            visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, color(Token::Focus));
        });
    }
}

/// `#RRGGBB` as a colour.
fn parse_color(token: Token, value: &str) -> Result<Color32, ThemeError> {
    let bad = || ThemeError::BadColor {
        token,
        value: value.to_owned(),
    };
    let hex = value
        .strip_prefix('#')
        .filter(|hex| hex.len() == 6)
        .ok_or_else(bad)?;
    let rgb = u32::from_str_radix(hex, 16).map_err(|_| bad())?;
    let [_, r, g, b] = rgb.to_be_bytes();
    Ok(Color32::from_rgb(r, g, b))
}
