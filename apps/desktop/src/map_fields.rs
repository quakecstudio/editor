// SPDX-License-Identifier: GPL-3.0-or-later

//! The key editor's fields (P5-13, MAP-27), each chosen from what the QuakeC says of the key:
//! spawnflags as checkboxes named by the code, an angle with up and down, a colour with a
//! picker, a float the map gives several values as a choice of them, a targeting key as the
//! map's targetnames and `classname` as the project's classes, a value the class's comment
//! numbers as those values with their names, and a file the code loads as the project's and
//! the game's files of its kind, each offered as it is typed with what it names (UI-24); any
//! other key as text. Each gives the value to set once an edit is done, so an edit is one undo
//! step.

use std::collections::BTreeSet;

use eframe::egui::{self, RichText, Ui};
use qs_map_edit::FlagChange;
use qs_map_edit::FlagInfo;

use crate::icons::Icon;
use crate::map_scene::{ANGLE, COLOR_SUFFIX, FLOAT_TYPE, SPAWNFLAGS};
use crate::strings::Text;
use crate::suggest::{Offer, Suggest};
use crate::theme::{Theme, Token};

/// The width of a value's text field, in points.
const VALUE_WIDTH: f32 = 140.0;
/// The width of a short value's text field, beside its other controls.
const SHORT_WIDTH: f32 = 64.0;
/// The width of a targeting key's field, beside the eyedropper.
const TARGET_WIDTH: f32 = 100.0;
/// The start of a brush model's name, `*1`, which the compiler writes (BUILTIN-10.3).
const BRUSH_MODEL: char = '*';
/// `angle`'s values for straight up and straight down (`SetMovedir`).
const UP: &str = "-1";
const DOWN: &str = "-2";
/// The largest component of a colour written from 0 to 1; one above it is from 0 to 255.
const UNIT: f32 = 1.0;
/// The largest component of a colour written from 0 to 255.
const BYTE: f32 = 255.0;

/// How a colour's components are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scale {
    /// From 0 to 1, as `light`'s `_color` in ericw-tools.
    Unit,
    /// From 0 to 255.
    Byte,
}

impl Scale {
    /// The largest component.
    const fn top(self) -> f32 {
        match self {
            Self::Unit => UNIT,
            Self::Byte => BYTE,
        }
    }
}
/// A colour's components are written to two places when from 0 to 1.
const PLACES: f32 = 100.0;
/// The colour's three components.
const COMPONENTS: usize = 3;

/// How a key's value is edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field<'a> {
    /// Typed, with controls beside it for the kind of value.
    Typed(Typed<'a>),
    /// The name of an entity to fire, from the map's targetnames, beside the eyedropper; a
    /// name no entity has is marked.
    Target(&'a [Offer]),
    /// The entity's class, from the project's classes.
    Class(&'a [Offer]),
    /// One of the values the class's comment numbers, each offered with what it is called,
    /// the name of the one set shown beside it.
    Numbered(&'a [Offer]),
    /// A file's name, from the project's and the game's files of the kind the code loads it
    /// as (MAP-8.11); a name no file has is marked.
    File(&'a [Offer]),
}

/// A value typed, and what is beside its field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Typed<'a> {
    /// As text.
    Text,
    /// An angle in degrees, or up or down.
    Angle,
    /// A colour of three numbers.
    Color,
    /// A float, from the values the map gives it.
    Choice(&'a BTreeSet<String>),
}

impl<'a> Typed<'a> {
    /// The field for `key`, of the type `ty` its class reads it as, which the map gives the
    /// values `in_use`.
    pub(crate) fn of(key: &str, ty: Option<&str>, in_use: Option<&'a BTreeSet<String>>) -> Self {
        if key == ANGLE {
            return Self::Angle;
        }
        if key.to_ascii_lowercase().ends_with(COLOR_SUFFIX) {
            return Self::Color;
        }
        match in_use {
            Some(values) if ty == Some(FLOAT_TYPE) && key != SPAWNFLAGS && values.len() > 1 => {
                Self::Choice(values)
            }
            Some(_) | None => Self::Text,
        }
    }
}

impl Field<'_> {
    /// Draws the field over `typed`, the value being typed, for the value `value`; the value
    /// to set once an edit is done.
    /// The field is named `key` to a screen reader.
    pub(crate) fn ui(
        self,
        ui: &mut Ui,
        theme: &Theme,
        key: &str,
        typed: &mut String,
        value: &str,
    ) -> Option<String> {
        ui.horizontal(|ui| match self {
            Self::Target(offers) => {
                let field = Suggest {
                    name: key,
                    offers,
                    width: TARGET_WIDTH,
                };
                let set = field.show(ui, theme, typed).filter(|done| done != value);
                let typed = typed.trim();
                if !typed.is_empty() && !offers.iter().any(|offer| offer.name == typed) {
                    let warn = theme.color(Token::StatusWarn);
                    Icon::Warning.sign(ui, warn, Text::MapNoSuchTarget);
                }
                set
            }
            Self::Class(offers) => {
                let field = Suggest {
                    name: key,
                    offers,
                    width: VALUE_WIDTH,
                };
                field.show(ui, theme, typed).filter(|done| done != value)
            }
            Self::Numbered(offers) => {
                let field = Suggest {
                    name: key,
                    offers,
                    width: SHORT_WIDTH,
                };
                let set = field.show(ui, theme, typed).filter(|done| done != value);
                let current = set.as_deref().unwrap_or(value);
                if let Some(offer) = offers.iter().find(|offer| offer.name == current) {
                    ui.label(RichText::new(&offer.detail).color(theme.color(Token::TextMuted)));
                }
                set
            }
            Self::File(offers) => {
                let field = Suggest {
                    name: key,
                    offers,
                    width: VALUE_WIDTH,
                };
                let set = field.show(ui, theme, typed).filter(|done| done != value);
                let typed = typed.trim();
                // A brush model's `*N` is the compiler's, not a file (BUILTIN-10.3).
                if !typed.is_empty()
                    && !typed.starts_with(BRUSH_MODEL)
                    && !offers.iter().any(|offer| offer.name == typed)
                {
                    let warn = theme.color(Token::StatusWarn);
                    Icon::Warning.sign(ui, warn, Text::MapNoSuchFile);
                }
                set
            }
            Self::Typed(field) => field.ui(ui, key, typed, value),
        })
        .inner
    }
}

impl Typed<'_> {
    /// A field typed as text, with the angle's, the colour's or the choice's controls beside
    /// it; the value to set once an edit is done.
    fn ui(self, ui: &mut Ui, key: &str, typed: &mut String, value: &str) -> Option<String> {
        let mut set = None;
        let width = match self {
            Self::Text => VALUE_WIDTH,
            Self::Angle | Self::Color | Self::Choice(_) => SHORT_WIDTH,
        };
        let edit = ui.add_sized(
            [width, ui.spacing().interact_size.y],
            egui::TextEdit::singleline(typed),
        );
        let edit = edit.accessible_name(key);
        if edit.lost_focus() && typed != value {
            set = Some(typed.clone());
        }
        match self {
            Self::Text => {}
            Self::Angle => {
                if Icon::Up.tool(ui, Text::MapAngleUp).clicked() {
                    set = Some(UP.to_owned());
                }
                if Icon::Down.tool(ui, Text::MapAngleDown).clicked() {
                    set = Some(DOWN.to_owned());
                }
            }
            Self::Color => {
                let (mut rgb, scale) = color(typed);
                if ui.color_edit_button_rgb(&mut rgb).changed() {
                    *typed = color_text(rgb, scale);
                }
                let done = ui.input(|i| i.pointer.any_released());
                if done && typed != value && !edit.has_focus() {
                    set = Some(typed.clone());
                }
            }
            Self::Choice(values) => {
                let picker = egui::ComboBox::from_id_salt(ui.scope_id().with(value))
                    .selected_text(Text::MapChoices.get())
                    .show_ui(ui, |ui| {
                        for choice in values {
                            if ui.selectable_label(choice == value, choice).clicked() {
                                set = Some(choice.clone());
                            }
                        }
                    });
                picker.response.accessible_name(Text::MapChoices.get());
            }
        }
        set
    }
}

/// A colour's text as the picker takes it, each component 0 to 1, and the scale it was written
/// in; black when it is not three numbers.
fn color(text: &str) -> ([f32; COMPONENTS], Scale) {
    let numbers: Vec<f32> = text
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    let Ok(rgb) = <[f32; COMPONENTS]>::try_from(numbers) else {
        return ([0.0; COMPONENTS], Scale::Unit);
    };
    let scale = if rgb.iter().any(|c| *c > UNIT) {
        Scale::Byte
    } else {
        Scale::Unit
    };
    (rgb.map(|c| (c / scale.top()).clamp(0.0, UNIT)), scale)
}

/// `rgb` written in `scale`: whole numbers from 0 to 255, or 0 to 1 to two places.
fn color_text(rgb: [f32; COMPONENTS], scale: Scale) -> String {
    rgb.map(|c| {
        let c = match scale {
            Scale::Byte => (c * BYTE).round(),
            Scale::Unit => (c * PLACES).round() / PLACES,
        };
        format!("{c}")
    })
    .join(" ")
}

/// The spawnflags `flags` as checkboxes over `value`, named as the code names them; the flag
/// ticked or cleared.
pub(crate) fn spawnflags(
    ui: &mut Ui,
    flags: &[FlagInfo],
    value: &str,
) -> Option<(u32, FlagChange)> {
    let whole = value.trim().split('.').next().unwrap_or_default();
    let bits: u32 = whole.parse().unwrap_or(0);
    let mut changed = None;
    ui.label(Text::MapSpawnflags.get());
    ui.horizontal_wrapped(|ui| {
        for flag in flags {
            let mut on = bits & flag.value != 0;
            let name = flag.name.clone().unwrap_or_else(|| flag.value.to_string());
            if ui.checkbox(&mut on, name).changed() {
                let change = if on {
                    FlagChange::Set
                } else {
                    FlagChange::Clear
                };
                changed = Some((flag.value, change));
            }
        }
    });
    changed
}

#[cfg(test)]
mod tests;
