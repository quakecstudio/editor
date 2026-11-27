// SPDX-License-Identifier: GPL-3.0-or-later

//! The Balance tab below the map view (L-78, MAP-37): Count makes the map's balance sheets
//! (`map.balance`) and the tab shows them a column a skill for the kind of game chosen: the
//! monsters by class with their health, the weapons, the ammunition with its damage, the health,
//! armour and secrets, and the damage over the monsters' health, for the whole map, the
//! selection when Count was pressed, or one layer or group. A row's name selects its entities,
//! and a skill's heading shows the map as that skill spawns it where the view's filter offers
//! it. Each is a [`MapAction`] the shell runs as a command.

use std::collections::BTreeSet;

use eframe::egui::{self, RichText, Ui};
use qs_map_edit::{EntityIndex, MapFound, MapRevision};
use qs_workbench::map_balance::{
    AmountLine, BalanceSheet, Entry, MapBalance, RoomKindName, RoomTally, SKILLS, Tally,
};
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

/// Which entities the table counts (BALANCE-2.3, BALANCE-2.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Scope {
    /// The whole map.
    #[default]
    Map,
    /// The entities selected when Count was pressed.
    Selection,
    /// The layer or group whose own entity this is.
    Room(EntityIndex),
}

impl Scope {
    /// The tally of `sheet` it counts, `None` when the sheet has none.
    fn tally(self, sheet: &BalanceSheet) -> Option<&Tally> {
        match self {
            Self::Map => Some(&sheet.map),
            Self::Selection => sheet.selection.as_ref(),
            Self::Room(entity) => sheet
                .rooms
                .iter()
                .find(|room| room.entity == entity)
                .map(|room| &room.tally),
        }
    }

    /// The ratio of `sheet` it shows: the sheet's own, the player's start counted, for the whole
    /// map; the tally's own for the rest.
    fn ratio(self, sheet: &BalanceSheet) -> Option<f32> {
        match self {
            Self::Map => sheet.ratio,
            Self::Selection | Self::Room(_) => self.tally(sheet).and_then(|tally| tally.ratio),
        }
    }
}

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

/// The rows the tallies `scope` counts of the sheets of the four skills make, in order: the
/// monsters by class, their count and health, the weapons by class, the ammunition by kind,
/// health, armour, secrets, damage and the ratio.
pub(crate) fn rows(sheets: &[&BalanceSheet; 4], scope: Scope) -> Vec<Row> {
    let names = |each: &dyn Fn(&Tally) -> Vec<String>| -> BTreeSet<String> {
        sheets
            .iter()
            .filter_map(|sheet| scope.tally(sheet))
            .flat_map(each)
            .collect()
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
            let cells = sheets.map(|sheet| match scope.tally(sheet).and_then(&reading) {
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
        cells: sheets.map(|sheet| scope.ratio(sheet).map_or_else(|| "-".to_owned(), number)),
        entities: Vec::new(),
    });
    out
}

/// The view's filter that shows the map as `skill` spawns it in a game of `mode`: each skill's
/// in single player, and in coop and deathmatch only normal's, the one the view offers; `None`
/// where the view has no such filter.
const fn filter_of(skill: Skill, mode: GameMode) -> Option<ShowFilter> {
    match (mode, skill) {
        (GameMode::Single, Skill::Easy) => Some(ShowFilter::Easy),
        (GameMode::Single, Skill::Normal) => Some(ShowFilter::Normal),
        (GameMode::Single, Skill::Hard) => Some(ShowFilter::Hard),
        (GameMode::Single, Skill::Nightmare) => Some(ShowFilter::Nightmare),
        (GameMode::Coop, Skill::Normal) => Some(ShowFilter::Coop),
        (GameMode::Deathmatch, Skill::Normal) => Some(ShowFilter::Deathmatch),
        (GameMode::Coop | GameMode::Deathmatch, Skill::Easy | Skill::Hard | Skill::Nightmare) => {
            None
        }
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

/// The Balance tab's state: the sheets last made, the kind of game shown and what is counted.
#[derive(Debug, Default)]
pub(crate) struct BalancePane {
    found: Option<MapBalance>,
    mode: GameMode,
    scope: Scope,
    /// The map's edits the sheets were made at.
    seen: Option<MapRevision>,
}

impl BalancePane {
    /// Takes the sheets `map.balance` made of the map at `at`.
    pub(crate) fn set(&mut self, found: MapBalance, at: MapRevision) {
        // What was counted stays chosen while the new sheets still have it.
        if !found
            .sheets
            .iter()
            .any(|sheet| self.scope.tally(sheet).is_some())
        {
            self.scope = Scope::Map;
        }
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
        if let Some(sheet) = self.found.as_ref().and_then(|found| found.sheets.first()) {
            let mut scopes = vec![(Scope::Map, Text::MapBalanceWholeMap.get().to_owned())];
            if sheet.selection.is_some() {
                scopes.push((Scope::Selection, Text::MapBalanceSelection.get().to_owned()));
            }
            for room in &sheet.rooms {
                scopes.push((Scope::Room(room.entity), room_name(room)));
            }
            let shown = scopes
                .iter()
                .find(|(scope, _)| *scope == self.scope)
                .map_or_else(String::new, |(_, name)| name.clone());
            let picked = egui::ComboBox::from_id_salt(ui.scope_id().with(Text::MapBalanceScope))
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for (scope, name) in scopes {
                        ui.selectable_value(&mut self.scope, scope, name);
                    }
                });
            picked.response.accessible_name(Text::MapBalanceScope.get());
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
                        let name = skill_text(skill).get();
                        match filter_of(skill, self.mode) {
                            Some(filter) => {
                                if ui
                                    .button(name)
                                    .on_hover_text(Text::MapBalanceShowHint.get())
                                    .clicked()
                                {
                                    actions.push(MapAction::Filter(filter));
                                }
                            }
                            None => {
                                ui.label(name);
                            }
                        }
                    }
                    ui.end_row();
                    for row in rows(&sheets, self.scope) {
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

/// A layer's or group's name in the picker: its `_tb_name`, or its kind and place for none.
fn room_name(room: &RoomTally) -> String {
    if room.name.is_empty() {
        let kind = match room.kind {
            RoomKindName::Layer => Text::MapBalanceLayer,
            RoomKindName::Group => Text::MapGroup,
        };
        format!("{} {}", kind.get(), room.entity.get())
    } else {
        room.name.clone()
    }
}

#[cfg(test)]
mod tests;
