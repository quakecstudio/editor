// SPDX-License-Identifier: GPL-3.0-or-later

//! A text field that offers a known set as it is typed (UI-24, docs/design/usability.md, rule
//! 1): the keys a class reads, the map's targetnames, the project's classes. Each offer shows
//! what it names beside it, as `gate  func_door`. Typing narrows the list, the arrow keys mark
//! a row and Enter or a click takes it; a value the list does not hold is still taken, so a
//! compiler's key or a new name is typed as before.

use std::borrow::Cow;

use eframe::egui::{self, Key, Modifiers, RichText, Ui};

use crate::shell::name_area;
use crate::strings::{Text, offer_detail, offer_name};
use crate::theme::{Theme, Token};

/// The most rows the list shows before it scrolls.
const ROWS: f32 = 8.0;
/// The widest the list is, in points.
const LIST_WIDTH: f32 = 320.0;

/// One value the field offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Offer {
    /// The value taken.
    pub(crate) name: String,
    /// What it names, shown muted after it.
    pub(crate) detail: String,
}

/// The values a field offers.
#[derive(Clone, Copy)]
pub(crate) enum Offers<'a> {
    /// A list made already.
    Given(&'a [Offer]),
    /// A list made from what is typed, only while the field is edited, after the frame's keys:
    /// one too costly to make every frame, or one that depends on the text.
    Made(&'a dyn Fn(&str) -> Cow<'a, [Offer]>),
}

impl<'a> Offers<'a> {
    /// The offers for `typed`.
    fn for_typed(self, typed: &str) -> Cow<'a, [Offer]> {
        match self {
            Self::Given(offers) => Cow::Borrowed(offers),
            Self::Made(make) => make(typed),
        }
    }
}

/// A field over a known set of values.
pub(crate) struct Suggest<'a> {
    /// What the field is named to a screen reader, and its hint while empty.
    pub(crate) name: &'a str,
    /// The values offered.
    pub(crate) offers: Offers<'a>,
    /// The field's width, in points.
    pub(crate) width: f32,
}

/// How the field's edit was done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Taken {
    /// An offer was taken, by Enter on its marked row or a click.
    Offer(String),
    /// The text typed was left, by Enter or the keyboard gone elsewhere.
    Typed(String),
}

impl Taken {
    /// The value taken.
    pub(crate) fn value(self) -> String {
        match self {
            Self::Offer(value) | Self::Typed(value) => value,
        }
    }
}

/// The list's state between frames.
#[derive(Clone, Copy, Debug, Default)]
struct Shown {
    /// The row the arrow keys marked, among those shown; cleared by typing, which changes the
    /// rows.
    marked: Option<usize>,
    /// Where the list was drawn last frame.
    at: Option<egui::Rect>,
}

impl Offer {
    /// Those of `offers` that match `typed`: every one while the field holds one exactly or
    /// nothing, so a value set already shows what else it may be; otherwise those containing
    /// it, in any case.
    pub(crate) fn matching<'o>(offers: &'o [Self], typed: &str) -> Vec<&'o Self> {
        let typed = typed.trim();
        if typed.is_empty() || offers.iter().any(|offer| offer.name == typed) {
            return offers.iter().collect();
        }
        let lower = typed.to_lowercase();
        offers
            .iter()
            .filter(|offer| offer.name.to_lowercase().contains(&lower))
            .collect()
    }
}

impl Suggest<'_> {
    /// Draws the field over `typed`, and its list below it while it has the keyboard; the
    /// value once the edit is done: an offer taken, Enter, or the keyboard gone elsewhere.
    pub(crate) fn show(self, ui: &mut Ui, theme: &Theme, typed: &mut String) -> Option<String> {
        self.take(ui, theme, typed).1.map(Taken::value)
    }

    /// As [`Suggest::show`], with the field, and saying whether an offer was taken or the text
    /// typed left.
    pub(crate) fn take(
        self,
        ui: &mut Ui,
        theme: &Theme,
        typed: &mut String,
    ) -> (egui::Response, Option<Taken>) {
        // Sized exactly, as the key editor's other fields are, so a grid's column takes its
        // width in the first frame rather than growing to it over several.
        let edit = egui::TextEdit::singleline(typed).hint_text(self.name);
        let size = [self.width, ui.spacing().interact_size.y];
        let response = ui.add_sized(size, edit).accessible_name(self.name);
        let id = response.id.with(Text::SuggestList);
        let mut shown: Shown = ui.data(|d| d.get_temp(id)).unwrap_or_default();
        // A press on the list takes the keyboard from the field a frame before its click is
        // read: the list stays for that press alone (as the command line's offers do).
        let pressing = ui.input(|i| i.pointer.primary_down() || i.pointer.primary_released());
        let on_list = pressing
            && shown
                .at
                .zip(ui.ctx().pointer_interact_pos())
                .is_some_and(|(rect, pointer)| rect.contains(pointer));
        if response.changed() || response.gained_focus() {
            shown.marked = None;
        }
        // Made only while edited, from the text as this frame's keys left it.
        let active = response.has_focus() || response.lost_focus() || on_list;
        let offers = if active {
            self.offers.for_typed(typed)
        } else {
            Cow::Borrowed(&[][..])
        };
        let matching = Offer::matching(&offers, typed);
        let mut done = None;
        let mut moved = false;
        // Enter takes the keyboard from a one-line field in the frame it is pressed, so the
        // keys are read while the field has it or has just lost it.
        if (response.has_focus() || response.lost_focus()) && !matching.is_empty() {
            let (down, up, enter) = ui.input_mut(|i| {
                (
                    i.consume_key(Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::ArrowUp),
                    i.key_pressed(Key::Enter),
                )
            });
            let last = matching.len().saturating_sub(1);
            if down {
                shown.marked = Some(shown.marked.map_or(0, |m| m.saturating_add(1).min(last)));
            }
            if up {
                shown.marked = shown.marked.and_then(|m| m.checked_sub(1));
            }
            moved = down || up;
            if let Some(offer) = shown.marked.and_then(|m| matching.get(m))
                && enter
            {
                done = Some(Taken::Offer(offer.name.clone()));
            }
        }
        if (response.has_focus() || on_list) && !matching.is_empty() && done.is_none() {
            let muted = theme.color(Token::TextMuted);
            let area = egui::Area::new(id)
                .order(egui::Order::Foreground)
                .fixed_pos(response.rect.left_bottom())
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        name_area(ui, egui::accesskit::Role::ListBox, Text::SuggestList);
                        ui.set_max_width(LIST_WIDTH);
                        let height = ui.spacing().interact_size.y * ROWS;
                        egui::ScrollArea::vertical()
                            .max_height(height)
                            .show(ui, |ui| {
                                for (index, offer) in matching.iter().enumerate() {
                                    let marked = shown.marked == Some(index);
                                    let mut job = egui::text::LayoutJob::default();
                                    RichText::new(&offer.name).append_to(
                                        &mut job,
                                        ui.style(),
                                        egui::FontSelection::Default,
                                        egui::Align::Center,
                                    );
                                    RichText::new(offer_detail(&offer.detail))
                                        .color(muted)
                                        .append_to(
                                            &mut job,
                                            ui.style(),
                                            egui::FontSelection::Default,
                                            egui::Align::Center,
                                        );
                                    let row = ui.add(egui::Button::selectable(marked, job));
                                    let row =
                                        row.accessible_name(offer_name(&offer.name, &offer.detail));
                                    // Only as the mark moves, so the wheel still scrolls.
                                    if marked && moved {
                                        row.scroll_to_me(None);
                                    }
                                    if row.clicked() {
                                        done = Some(Taken::Offer(offer.name.clone()));
                                    }
                                }
                            });
                    });
                });
            shown.at = Some(area.response.rect);
        } else {
            shown.at = None;
        }
        if done.is_none() && response.lost_focus() && !on_list {
            done = Some(Taken::Typed(typed.trim().to_owned()));
        }
        if let Some(Taken::Offer(value) | Taken::Typed(value)) = &done {
            typed.clone_from(value);
            shown = Shown::default();
            if response.has_focus() {
                response.surrender_focus();
            }
        }
        ui.data_mut(|d| d.insert_temp(id, shown));
        (response, done)
    }
}

#[cfg(test)]
mod tests;
