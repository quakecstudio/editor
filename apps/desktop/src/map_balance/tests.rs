// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "test code: a failed index or unwrap is a failed test"
)]

use qs_map_edit::EntityIndex;
use qs_workbench::map_balance::{
    AmmoLine, AmountLine, BalanceSheet, Entry, Inventory, MonsterLine, RoomKindName, RoomTally,
    Tally,
};
use qs_workbench::map_filter::{GameMode, Skill};

use super::{Scope, filter_of, rows};
use crate::map_editor::ShowFilter;

/// A sheet of `skill` with `grunts` monsters of 30 health and 20 shells.
fn sheet(skill: Skill, grunts: f32) -> BalanceSheet {
    let entities: Vec<EntityIndex> = (1..=2).map(EntityIndex::new).collect();
    BalanceSheet {
        skill,
        mode: GameMode::Single,
        entry: Entry::Entered,
        start: Inventory::default(),
        map: Tally {
            monsters: vec![MonsterLine {
                classname: "monster_army".to_owned(),
                count: grunts,
                health: grunts * 30.0,
                entities,
            }],
            monster_count: grunts,
            monster_health: grunts * 30.0,
            weapons: Vec::new(),
            ammo: vec![AmmoLine {
                ammo: "ammo_shells".to_owned(),
                units: 20.0,
                damage: Some(560.0),
                entities: vec![EntityIndex::new(3)],
            }],
            health: AmountLine::default(),
            armour: AmountLine::default(),
            secrets: AmountLine::default(),
            damage: 560.0,
            ratio: Some(560.0 / (grunts * 30.0)),
        },
        rooms: Vec::new(),
        selection: None,
        total_monsters: grunts,
        total_secrets: 0.0,
        ratio: Some(560.0 / (grunts * 30.0)),
    }
}

// A column a skill: each monster class and ammo kind a row with its entities, then the totals.
#[test]
fn the_table_has_a_column_a_skill_and_a_row_a_class() {
    let sheets = [
        sheet(Skill::Easy, 1.0),
        sheet(Skill::Normal, 2.0),
        sheet(Skill::Hard, 2.0),
        sheet(Skill::Nightmare, 2.0),
    ];
    let table = rows(
        &[&sheets[0], &sheets[1], &sheets[2], &sheets[3]],
        Scope::Map,
    );
    let names: Vec<&str> = table.iter().map(|row| row.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "monster_army",
            "Monsters",
            "Monster health",
            "ammo_shells",
            "Health",
            "Armour",
            "Secrets",
            "Ammo damage",
            "Damage per monster health"
        ]
    );
    assert_eq!(
        table[0].cells,
        ["1 (30 hp)", "2 (60 hp)", "2 (60 hp)", "2 (60 hp)"]
    );
    assert_eq!(
        table[0].entities,
        [EntityIndex::new(1), EntityIndex::new(2)]
    );
    assert_eq!(table[3].cells[0], "20 (560 dmg)");
    assert_eq!(table[8].cells, ["18.67", "9.33", "9.33", "9.33"]);
}

// A skill's heading shows the map as that skill spawns it where the view's filter offers it:
// every skill in single player, only normal in coop and deathmatch.
#[test]
fn a_skills_heading_chooses_the_view_filter() {
    assert_eq!(
        filter_of(Skill::Hard, GameMode::Single),
        Some(ShowFilter::Hard)
    );
    assert_eq!(
        filter_of(Skill::Normal, GameMode::Coop),
        Some(ShowFilter::Coop)
    );
    assert_eq!(
        filter_of(Skill::Normal, GameMode::Deathmatch),
        Some(ShowFilter::Deathmatch)
    );
    assert_eq!(filter_of(Skill::Easy, GameMode::Coop), None);
    assert_eq!(filter_of(Skill::Nightmare, GameMode::Deathmatch), None);
}

// The table counts the whole map, the selection or one layer or group; a sheet without the
// chosen tally shows a dash, and the whole map's ratio is the sheet's own.
#[test]
fn the_table_counts_what_is_chosen() {
    let mut with = sheet(Skill::Normal, 2.0);
    let mut picked = with.map.clone();
    picked.monster_count = 1.0;
    picked.monster_health = 30.0;
    picked.ratio = Some(1.0);
    with.selection = Some(picked.clone());
    with.rooms = vec![RoomTally {
        entity: EntityIndex::new(7),
        kind: RoomKindName::Group,
        name: String::new(),
        tally: picked,
    }];
    with.ratio = Some(5.0);
    let without = sheet(Skill::Easy, 1.0);
    let sheets = [&without, &with, &with, &with];
    let monsters = |scope| {
        rows(&sheets, scope)
            .into_iter()
            .find(|row| row.name == "Monsters")
            .unwrap()
            .cells
    };
    assert_eq!(monsters(Scope::Map), ["1", "2", "2", "2"]);
    assert_eq!(monsters(Scope::Selection), ["-", "1", "1", "1"]);
    assert_eq!(
        monsters(Scope::Room(EntityIndex::new(7))),
        ["-", "1", "1", "1"]
    );
    let ratio = |scope| rows(&sheets, scope).last().unwrap().cells[1].clone();
    assert_eq!(ratio(Scope::Map), "5");
    assert_eq!(ratio(Scope::Selection), "1");
    assert_eq!(super::room_name(&with.rooms[0]), "Group 7");
}
