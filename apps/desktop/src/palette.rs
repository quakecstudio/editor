// SPDX-License-Identifier: GPL-3.0-or-later

//! The command palette (UI-10), as the approved mockup draws it
//! (`docs/design/mockups/04-command-palette.png`): a search over every command in the
//! registry, best match first, each row its summary with the matched characters in the
//! accent, its id and its shortcut; Enter runs the selected command, or first asks for its
//! parameters in a form made from its schema.
//!
//! Every string comes from [`crate::strings`]; a test fails on a string literal here.

use std::ops::Range;

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, Align, Align2, Color32, FontFamily, FontId, Id, Key, Layout, Modifiers, Order, RichText,
    Sense, TextEdit, Ui, Vec2,
};
use qs_commands::palette::{PaletteMatch, search};
use qs_commands::params::{self, Field, FieldKind, Input};
use qs_commands::{CommandId, CommandInfo, Registry};
use serde_json::Value;

use crate::icons::Icon;
use crate::keymap;
use crate::strings::{Text, command_count};
use crate::theme::{Theme, Token};

/// The palette's width, in points, as in the mockup.
const WIDTH: f32 = 640.0;
/// How far below the window's top the palette opens, in points.
const TOP: f32 = 96.0;
/// The most rows shown before the list scrolls.
const ROWS: f32 = 8.0;

/// A command chosen whose parameters are being entered.
#[derive(Clone, Debug)]
pub struct Prompt {
    /// The command.
    pub id: CommandId,
    summary: &'static str,
    /// Its parameters' fields.
    pub fields: Vec<Field>,
    /// What has been entered in each, in the same order.
    pub inputs: Vec<Input>,
}

/// The palette's state.
#[derive(Clone, Debug, Default)]
pub struct Palette {
    open: bool,
    query: String,
    selected: usize,
    prompt: Option<Prompt>,
    error: Option<String>,
    /// Whether the query field takes focus in the next frame.
    focus: bool,
}

impl Palette {
    /// Whether it is showing.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Opens it, empty, on the first command.
    pub fn open(&mut self) {
        *self = Self {
            open: true,
            focus: true,
            ..Self::default()
        };
    }

    /// Closes it.
    pub fn close(&mut self) {
        *self = Self::default();
    }

    /// The parameter prompt, while one is showing.
    pub fn prompt_mut(&mut self) -> Option<&mut Prompt> {
        self.prompt.as_mut()
    }

    /// The error of the last command run from it, if it failed.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Shows the error a command run from the palette ended with, keeping it open.
    pub fn failed(&mut self, error: String) {
        self.error = Some(error);
    }

    /// Draws the palette over the window, if it is open, and returns the command to run with
    /// its parameters when one was chosen.
    pub fn show<C: 'static>(
        &mut self,
        ui: &Ui,
        id: Id,
        theme: &Theme,
        registry: &Registry<C>,
    ) -> Option<(CommandId, Value)> {
        if !self.open {
            return None;
        }
        let mut chosen = None;
        egui::Area::new(id)
            .order(Order::Foreground)
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, TOP))
            .show(ui.ctx(), |ui| {
                egui::Frame::NONE
                    .fill(theme.color(Token::SurfaceRaised))
                    .stroke(egui::Stroke::new(1.0, theme.color(Token::BorderStrong)))
                    .corner_radius(theme.radius.card)
                    .inner_margin(theme.space(2))
                    .show(ui, |ui| {
                        ui.set_width(WIDTH);
                        chosen = match self.prompt.take() {
                            Some(prompt) => self.prompt_ui(ui, theme, prompt),
                            None => self.list_ui(ui, theme, registry),
                        };
                        if let Some(error) = &self.error {
                            ui.label(RichText::new(error).color(theme.color(Token::StatusError)));
                        }
                    });
            });
        chosen
    }

    /// The search field and the matching commands.
    fn list_ui<C: 'static>(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        registry: &Registry<C>,
    ) -> Option<(CommandId, Value)> {
        let total = registry.commands().count();
        let found = search(registry.commands(), &self.query);
        let (up, down, enter, escape) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if escape {
            self.close();
            return None;
        }
        let last = found.len().saturating_sub(1);
        if down {
            self.selected = self.selected.saturating_add(1).min(last);
        }
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(Icon::Search.char().to_string()));
            let field = TextEdit::singleline(&mut self.query)
                .hint_text(Text::SearchCommands.get())
                .frame(egui::Frame::NONE)
                .desired_width(WIDTH - 200.0);
            let response = ui.add(field);
            if self.focus {
                response.request_focus();
                self.focus = false;
            }
            if response.changed() {
                self.selected = 0;
                self.error = None;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(command_count(found.len(), total))
                        .color(theme.color(Token::TextMuted)),
                );
            });
        });
        ui.separator();
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .max_height(theme.size.row * ROWS)
            .show(ui, |ui| {
                if found.is_empty() {
                    ui.label(
                        RichText::new(Text::PaletteNoMatch.get())
                            .color(theme.color(Token::TextMuted)),
                    );
                }
                for (index, item) in found.iter().enumerate() {
                    let selected = index == self.selected;
                    let response = row(ui, theme, item, selected);
                    if selected && (up || down) {
                        response.scroll_to_me(None);
                    }
                    if response.clicked() {
                        clicked = Some(index);
                    }
                }
            });
        ui.separator();
        hints(ui, theme);
        let pick = clicked.or_else(|| enter.then_some(self.selected))?;
        let info = found.get(pick)?.info;
        self.choose(info)
    }

    /// Runs `info` when it takes no parameters, else opens its prompt.
    fn choose(&mut self, info: &CommandInfo) -> Option<(CommandId, Value)> {
        let fields = params::fields(info.params());
        if fields.is_empty() {
            return Some((info.id().clone(), Value::Object(serde_json::Map::new())));
        }
        let inputs = fields.iter().map(|f| Input::empty(&f.kind)).collect();
        self.prompt = Some(Prompt {
            id: info.id().clone(),
            summary: info.summary(),
            fields,
            inputs,
        });
        self.error = None;
        self.focus = true;
        None
    }

    /// The parameter form of `prompt`; Enter or Run makes the parameters, Escape or Cancel goes
    /// back to the list.
    fn prompt_ui(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        mut prompt: Prompt,
    ) -> Option<(CommandId, Value)> {
        let (enter, escape) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new(prompt.summary).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(mono(prompt.id.as_str(), theme.color(Token::TextMuted)));
            });
        });
        ui.separator();
        let first_text = prompt
            .inputs
            .iter()
            .position(|input| matches!(input, Input::Text(_)));
        let focus = std::mem::take(&mut self.focus);
        for (index, (field, input)) in prompt.fields.iter().zip(&mut prompt.inputs).enumerate() {
            field_ui(ui, theme, focus && first_text == Some(index), field, input);
        }
        ui.separator();
        let mut run = enter;
        let mut cancel = escape;
        ui.horizontal(|ui| {
            run |= ui.button(Text::PaletteRun.get()).clicked();
            cancel |= ui.button(Text::PaletteCancel.get()).clicked();
        });
        if cancel {
            self.error = None;
            self.focus = true;
            return None;
        }
        if run {
            match params::object(&prompt.fields, &prompt.inputs) {
                Ok(value) => {
                    let id = prompt.id.clone();
                    self.prompt = Some(prompt);
                    return Some((id, value));
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        self.prompt = Some(prompt);
        None
    }
}

/// One command's row: its summary with the matched characters in the accent, then its id and
/// its shortcut on the right.
fn row(ui: &mut Ui, theme: &Theme, item: &PaletteMatch<'_>, selected: bool) -> egui::Response {
    let height = theme.size.row;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    if selected || response.hovered() {
        let fill = if selected {
            theme.color(Token::SelectFocus)
        } else {
            theme.color(Token::SurfacePanel)
        };
        ui.painter().rect_filled(rect, theme.radius.control, fill);
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(theme.space(1), 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let text = theme.color(Token::Text);
    let accent = theme.color(Token::Accent);
    child.label(highlighted(
        item.info.summary(),
        &item.summary_hits,
        text,
        accent,
        &FontId::proportional(theme.type_scale.body.size),
    ));
    child.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let shortcut =
            keymap::shortcut(item.info.id().as_str()).map(|s| ui.ctx().format_shortcut(&s));
        ui.add_sized(
            Vec2::new(48.0, height),
            egui::Label::new(
                RichText::new(shortcut.unwrap_or_default()).color(theme.color(Token::TextMuted)),
            ),
        );
        ui.label(highlighted(
            item.info.id().as_str(),
            &item.id_hits,
            theme.color(Token::TextMuted),
            accent,
            &FontId::monospace(theme.type_scale.body.size),
        ));
    });
    response
}

/// `text` with the bytes of `hits` in `accent` and the rest in `color`.
pub(crate) fn highlighted(
    text: &str,
    hits: &[Range<usize>],
    color: Color32,
    accent: Color32,
    font: &FontId,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    let format = |color| TextFormat {
        font_id: font.clone(),
        color,
        ..TextFormat::default()
    };
    let mut at = 0;
    for hit in hits {
        let (start, end) = (hit.start.clamp(at, text.len()), hit.end.min(text.len()));
        job.append(text.get(at..start).unwrap_or_default(), 0.0, format(color));
        job.append(
            text.get(start..end).unwrap_or_default(),
            0.0,
            format(accent),
        );
        at = end.max(start);
    }
    job.append(text.get(at..).unwrap_or_default(), 0.0, format(color));
    job
}

/// `text` in the code font.
fn mono(text: &str, color: Color32) -> RichText {
    RichText::new(text)
        .family(FontFamily::Monospace)
        .color(color)
}

/// The keys the palette takes, as in the mockup's footer.
fn hints(ui: &mut Ui, theme: &Theme) {
    let muted = theme.color(Token::TextMuted);
    ui.horizontal(|ui| {
        for (keys, text) in [
            (Text::KeyArrows, Text::PaletteMove),
            (Text::KeyEnter, Text::PaletteRun),
            (Text::KeyEscape, Text::PaletteClose),
        ] {
            ui.label(RichText::new(keys.get()).color(theme.color(Token::Text)));
            ui.label(RichText::new(text.get()).color(muted));
        }
    });
}

/// One parameter's label and input; with `focus` it takes the focus, as the first text field
/// does when the form opens, so the parameter can be typed at once.
fn field_ui(ui: &mut Ui, theme: &Theme, focus: bool, field: &Field, input: &mut Input) {
    ui.horizontal(|ui| {
        ui.label(mono(&field.name, theme.color(Token::Text)));
        if field.required {
            ui.label(
                RichText::new(Text::PaletteRequired.get()).color(theme.color(Token::TextMuted)),
            );
        }
    });
    if let Some(description) = &field.description {
        ui.label(
            RichText::new(description)
                .small()
                .color(theme.color(Token::TextMuted)),
        );
    }
    match (&field.kind, input) {
        (FieldKind::Choice(choices), Input::Choice(picked)) => {
            ui.horizontal_wrapped(|ui| {
                for (i, choice) in choices.iter().enumerate() {
                    let label = match &choice.value {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    let response = ui.selectable_label(*picked == Some(i), label);
                    let response = match &choice.description {
                        Some(description) => response.on_hover_text(description),
                        None => response,
                    };
                    if response.clicked() {
                        *picked = Some(i);
                    }
                }
                if picked.is_none() {
                    ui.label(
                        RichText::new(Text::PaletteChoose.get())
                            .color(theme.color(Token::TextMuted)),
                    );
                }
            });
        }
        (FieldKind::Boolean, Input::Bool(value)) => {
            ui.checkbox(value, "");
        }
        (_, Input::Text(text)) => {
            let response = ui.add(TextEdit::singleline(text).desired_width(WIDTH - 32.0));
            if focus {
                response.request_focus();
            }
        }
        (_, Input::Choice(_) | Input::Bool(_)) => {}
    }
}
