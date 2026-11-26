// SPDX-License-Identifier: GPL-3.0-or-later

//! The Balance tab below the map view (L-78, MAP-37): Count makes the map's balance sheets
//! (`map.balance`) and the tab shows them a column a skill for the kind of game chosen: the
//! monsters by class with their health, the weapons, the ammunition with its damage, the health,
//! armour and secrets, and the damage over the monsters' health. A row's name selects its
//! entities, and a skill's heading shows the map as that skill spawns it. Each is a
//! [`MapAction`] the shell runs as a command.

use std::collections::BTreeSet;

use eframe::egui::{self, RichText, Ui};
use qs_map_edit::{EntityIndex, MapFound, MapRevision};
use qs_workbench::map_balance::{AmountLine, BalanceSheet, Entry, MapBalance, SKILLS, Tally};
use qs_workbench::map_filter::{GameMode, Skill};

use crate::map_editor::{MapAction, ShowFilter};
use crate::strings::{Text, balance_unentered};
use crate::theme::{Theme, Token};

/// The kinds of game offered, in order, with their names.
const MODES: [(GameMode, Text); 3] = [
    (GameMode::Single, Text::MapSingle),
    (GameMode::Coop, Text::MapCoop),
    (GameMode::Deathmatch, Text::MapDeathmatch),
];

/// One row of the table: its name, a cell for each skill, and the entities it names on any.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub(crate) name: String,
    pub(crate) cells: [String; 4],
    pub(crate) entities: Vec<EntityIndex>,
}

/// A number as the table shows it: whole, or to two places.
fn number(value: f32) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

/// Which line of a tally a row of an amount reads.
type AmountOf = fn(&Tally) -> &AmountLine;

/// What a row reads of one tally: its cell, and the entities it names.
type Reading = Box<dyn Fn(&Tally) -> Option<(String, Vec<EntityIndex>)>>;

/// The rows the sheets of the four skills make, in order: the monsters by class, their count and
/// health, the weapons by class, the ammunition by kind, health, armour, secrets, damage and the
/// sheet's ratio.
pub(crate) fn rows(sheets: &[&BalanceSheet; 4]) -> Vec<Row> {
    let names = |each: &dyn Fn(&Tally) -> Vec<String>| -> BTreeSet<String> {
        sheets.iter().flat_map(|sheet| each(&sheet.map)).collect()
    };
    let mut readings: Vec<(String, Reading)> = Vec::new();
    for class in names(&|tally| tally.monsters.iter().map(|l| l.classname.clone()).collect()) {
        let name = class.clone();
        readings.push((
            class,
            Box::new(move |tally: &Tally| {
                tally
                    .monsters
                    .iter()
                    .find(|l| l.classname == name)
                    .map(|l| {
                        (
                            format!("{} ({} hp)", number(l.count), number(l.health)),
                            l.entities.clone(),
                        )
                    })
            }),
        ));
    }
    readings.push((
        Text::MapBalanceMonsters.get().to_owned(),
        Box::new(|tally: &Tally| {
            let entities = tally.monsters.iter().flat_map(|l| l.entities.clone());
            Some((number(tally.monster_count), entities.collect()))
        }),
    ));
    readings.push((
        Text::MapBalanceMonsterHealth.get().to_owned(),
        Box::new(|tally: &Tally| Some((number(tally.monster_health), Vec::new()))),
    ));
    for class in names(&|tally| tally.weapons.iter().map(|l| l.classname.clone()).collect()) {
        let name = class.clone();
        readings.push((
            class,
            Box::new(move |tally: &Tally| {
                tally
                    .weapons
                    .iter()
                    .find(|l| l.classname == name)
                    .map(|l| (l.entities.len().to_string(), l.entities.clone()))
            }),
        ));
    }
    for kind in names(&|tally| tally.ammo.iter().map(|l| l.ammo.clone()).collect()) {
        let name = kind.clone();
        readings.push((
            kind,
            Box::new(move |tally: &Tally| {
                tally.ammo.iter().find(|l| l.ammo == name).map(|l| {
                    let cell = l.damage.map_or_else(
                        || number(l.units),
                        |damage| format!("{} ({} dmg)", number(l.units), number(damage)),
                    );
                    (cell, l.entities.clone())
                })
            }),
        ));
    }
    let amounts: [(Text, AmountOf); 3] = [
        (Text::MapBalanceHealth, |tally| &tally.health),
        (Text::MapBalanceArmour, |tally| &tally.armour),
        (Text::MapBalanceSecrets, |tally| &tally.secrets),
    ];
    for (text, line_of) in amounts {
        readings.push((
            text.get().to_owned(),
            Box::new(move |tally: &Tally| {
                let line = line_of(tally);
                Some((number(line.amount), line.entities.clone()))
            }),
        ));
    }
    readings.push((
        Text::MapBalanceDamage.get().to_owned(),
        Box::new(|tally: &Tally| Some((number(tally.damage), Vec::new()))),
    ));
    let mut out: Vec<Row> = readings
        .into_iter()
        .map(|(name, reading)| {
            let mut entities = BTreeSet::new();
            let cells = sheets.map(|sheet| match reading(&sheet.map) {
                Some((cell, named)) => {
                    entities.extend(named);
                    cell
                }
                None => "-".to_owned(),
            });
            Row {
                name,
                cells,
                entities: entities.into_iter().collect(),
            }
        })
        .collect();
    out.push(Row {
        name: Text::MapBalanceRatio.get().to_owned(),
        cells: sheets.map(|sheet| sheet.ratio.map_or_else(|| "-".to_owned(), number)),
        entities: Vec::new(),
    });
    out
}

/// The filter that shows the map as `skill` spawns it in a game of `mode`: a skill's own in
/// single player, the kind of game's in coop and deathmatch, which the view offers on normal.
const fn filter_of(skill: Skill, mode: GameMode) -> ShowFilter {
    match (mode, skill) {
        (GameMode::Single, Skill::Easy) => ShowFilter::Easy,
        (GameMode::Single, Skill::Normal) => ShowFilter::Normal,
        (GameMode::Single, Skill::Hard) => ShowFilter::Hard,
        (GameMode::Single, Skill::Nightmare) => ShowFilter::Nightmare,
        (GameMode::Coop, _) => ShowFilter::Coop,
        (GameMode::Deathmatch, _) => ShowFilter::Deathmatch,
    }
}

/// The name of a skill's column.
const fn skill_text(skill: Skill) -> Text {
    match skill {
        Skill::Easy => Text::MapSkillEasy,
        Skill::Normal => Text::MapSkillNormal,
        Skill::Hard => Text::MapSkillHard,
        Skill::Nightmare => Text::MapSkillNightmare,
    }
}

/// The Balance tab's state: the sheets last made, and the kind of game shown.
#[derive(Debug, Default)]
pub(crate) struct BalancePane {
    found: Option<MapBalance>,
    mode: GameMode,
    /// The map's edits the sheets were made at.
    seen: Option<MapRevision>,
}

impl BalancePane {
    /// Takes the sheets `map.balance` made of the map at `at`.
    pub(crate) fn set(&mut self, found: MapBalance, at: MapRevision) {
        self.found = Some(found);
        self.seen = Some(at);
    }

    /// Forgets the sheets when the map is no longer at the edits they were made at: their
    /// places may name other entities now.
    pub(crate) fn follow(&mut self, edits: MapRevision) {
        if self.seen.is_some_and(|seen| seen != edits) {
            self.clear();
        }
    }

    /// Forgets the sheets.
    pub(crate) fn clear(&mut self) {
        self.found = None;
        self.seen = None;
    }

    /// The four sheets of the kind of game shown, skill by skill.
    fn shown(&self) -> Option<[&BalanceSheet; 4]> {
        let found = self.found.as_ref()?;
        let sheet = |skill: Skill| {
            found
                .sheets
                .iter()
                .find(|sheet| sheet.skill == skill && sheet.mode == self.mode)
        };
        Some([
            sheet(Skill::Easy)?,
            sheet(Skill::Normal)?,
            sheet(Skill::Hard)?,
            sheet(Skill::Nightmare)?,
        ])
    }

    /// The header's part: Count, which also tallies `selection`, and the kind of game shown.
    pub(crate) fn header(&mut self, ui: &mut Ui, selection: Vec<EntityIndex>) -> Vec<MapAction> {
        let mut actions = Vec::new();
        if ui
            .button(Text::MapCount.get())
            .on_hover_text(Text::MapCountHint.get())
            .clicked()
        {
            actions.push(MapAction::CheckBalance(selection));
        }
        for (mode, text) in MODES {
            ui.selectable_value(&mut self.mode, mode, text.get());
        }
        actions
    }

    /// The table, a column a skill.
    pub(crate) fn body(&self, ui: &mut Ui, theme: &Theme) -> Vec<MapAction> {
        let mut actions = Vec::new();
        let Some(sheets) = self.shown() else {
            ui.label(
                RichText::new(Text::MapBalanceNone.get()).color(theme.color(Token::TextMuted)),
            );
            return actions;
        };
        if let Some(error) = sheets.iter().find_map(|sheet| match &sheet.entry {
            Entry::NotEntered { error } => Some(error),
            Entry::Entered => None,
        }) {
            ui.label(RichText::new(balance_unentered(error)).color(theme.color(Token::StatusWarn)));
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("map-balance")
                .striped(true)
                .num_columns(SKILLS.len().saturating_add(1))
                .show(ui, |ui| {
                    ui.label("");
                    for skill in SKILLS {
                        if ui
                            .button(skill_text(skill).get())
                            .on_hover_text(Text::MapBalanceShowHint.get())
                            .clicked()
                        {
                            actions.push(MapAction::Filter(filter_of(skill, self.mode)));
                        }
                    }
                    ui.end_row();
                    for row in rows(&sheets) {
                        if row.entities.is_empty() {
                            ui.label(&row.name);
                        } else if ui
                            .link(&row.name)
                            .on_hover_text(Text::MapBalanceRowHint.get())
                            .clicked()
                        {
                            actions.push(MapAction::SelectFound(MapFound {
                                entities: row.entities.clone(),
                                brushes: Vec::new(),
                                faces: Vec::new(),
                            }));
                        }
                        for cell in &row.cells {
                            ui.label(cell);
                        }
                        ui.end_row();
                    }
                });
        });
        actions
    }
}

#[cfg(test)]
mod tests;
