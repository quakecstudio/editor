// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::fmt::Write as _;
use std::path::PathBuf;

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::{Value, json};

use super::{
    AmmoLine, BalanceSheet, ID_WEAPONS, MAP_BALANCE, MODES, MapBalance, SKILLS, Weapon,
    damage_per_unit,
};
use crate::map_filter::{GameMode, Skill};
use crate::vm_run::tests::system_defs;
use crate::{Config, OPEN_PROJECT, Workbench, register};
use qs_map_edit::{EntityIndex, MAP_OPEN};

/// A program with a monster whose health its spawn function sets from the skill, id's way of
/// counting monsters and secrets, an item of id's shells, a mod's own ammunition, a health
/// pickup, an armour, a weapon, a trigger that hurts and a spawner that counts two monsters.
const CODE: &str = r#"
void(entity e) remove = #15;
.float ammo_plasma;
float(string s) cvar = #45;
string(string s) precache_model = #20;
void() worldspawn = { precache_model("progs/player.mdl"); };
void() monster_grunt = { self.health = 30 + cvar("skill") * 10; total_monsters = total_monsters + 1; };
void() monster_pair = { self.health = 100; total_monsters = total_monsters + 2; };
void() trigger_secret = { total_secrets = total_secrets + 1; };
void() shells_touch = { other.ammo_shells = other.ammo_shells + 20; remove(self); };
void() item_shells = { self.touch = shells_touch; };
void() plasma_touch = { other.ammo_plasma = other.ammo_plasma + 7; remove(self); };
void() item_plasma = { self.touch = plasma_touch; };
void() health_touch = { other.health = other.health + 25; remove(self); };
void() item_health = { self.touch = health_touch; };
void() armor_touch = { other.armortype = 0.3; other.armorvalue = 100; remove(self); };
void() item_armor1 = { self.touch = armor_touch; };
void() ssg_touch = { other.items = other.items | 2; other.ammo_shells = other.ammo_shells + 5; };
void() weapon_supershotgun = { self.touch = ssg_touch; };
void() hurt_touch = { other.health = other.health - 10; };
void() trigger_hurt = { self.touch = hurt_touch; };
void() bad_touch = { other.ammo_shells = other.ammo_shells + 20; world.think(); };
void() item_bad = { self.touch = bad_touch; };
void() monster_spawner = { total_monsters = total_monsters + 3; remove(self); };
"#;

/// The world's entity, with a brush.
const WORLD: &str = "{\n\"classname\" \"worldspawn\"\n\
    {\n( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) floor 0 0 0 1 1\n\
    ( 0 0 64 ) ( 1 0 64 ) ( 0 1 64 ) floor 0 0 0 1 1\n}\n}\n";

/// A spawnflag that inhibits an entity on easy (EDICT-8.4).
const NOT_EASY: &str = "256";
/// On normal.
const NOT_MEDIUM: &str = "512";
/// On hard and nightmare.
const NOT_HARD: &str = "1024";
/// In deathmatch.
const NOT_DEATHMATCH: &str = "2048";

fn entity(classname: &str, keys: &[(&str, &str)]) -> String {
    let mut text = format!("{{\n\"classname\" \"{classname}\"\n\"origin\" \"0 0 32\"\n");
    for (key, value) in keys {
        _ = writeln!(text, "\"{key}\" \"{value}\"");
    }
    text.push_str("}\n");
    text
}

/// A session with the project of [`CODE`] open and the map of `entities` after the world.
fn session(name: &str, entities: &[String]) -> (Session<Workbench<FsLoader>>, PathBuf) {
    let dir = std::env::temp_dir().join(format!("qs-wb-balance-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(dir.join("defs.qc"), system_defs()).unwrap();
    std::fs::write(dir.join("world.qc"), CODE).unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\ndefs.qc\nworld.qc\n").unwrap();
    std::fs::write(
        dir.join("maps/test.map"),
        format!("{WORLD}{}", entities.concat()),
    )
    .unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute(OPEN_PROJECT, json!({ "path": dir }))
        .unwrap();
    session
        .execute(MAP_OPEN, json!({ "path": dir.join("maps/test.map") }))
        .unwrap();
    (session, dir)
}

fn balance(session: &mut Session<Workbench<FsLoader>>, params: Value) -> MapBalance {
    serde_json::from_value(session.execute(MAP_BALANCE, params).unwrap()).unwrap()
}

fn sheet(balance: &MapBalance, skill: Skill, mode: GameMode) -> &BalanceSheet {
    balance
        .sheets
        .iter()
        .find(|sheet| sheet.skill == skill && sheet.mode == mode)
        .unwrap()
}

/// The places of the monsters a sheet counts.
fn monsters(sheet: &BalanceSheet) -> Vec<usize> {
    let mut places: Vec<usize> = sheet
        .map
        .monsters
        .iter()
        .flat_map(|line| line.entities.iter().map(|entity| entity.get()))
        .collect();
    places.sort_unstable();
    places
}

// spec BALANCE-2.2: an entity is on a sheet exactly when the game keeps it at the sheet's skill
// and kind of game: out of deathmatch spawnflags 256 off easy, 512 off normal and 1024 off hard
// and nightmare, in deathmatch 2048 off and no skill flag tested (EDICT-8.4), so nightmare's
// entities are hard's
#[test]
fn each_skill_flag_is_counted_where_the_game_keeps_it() {
    let (mut session, _dir) = session(
        "flags",
        &[
            entity("monster_grunt", &[("spawnflags", NOT_EASY)]),
            entity("monster_grunt", &[("spawnflags", NOT_MEDIUM)]),
            entity("monster_grunt", &[("spawnflags", NOT_HARD)]),
            entity("monster_grunt", &[("spawnflags", NOT_DEATHMATCH)]),
        ],
    );
    let found = balance(&mut session, json!({}));
    assert_eq!(found.sheets.len(), SKILLS.len() * MODES.len());
    let single = |skill| monsters(sheet(&found, skill, GameMode::Single));
    assert_eq!(single(Skill::Easy), [2, 3, 4]);
    assert_eq!(single(Skill::Normal), [1, 3, 4]);
    assert_eq!(single(Skill::Hard), [1, 2, 4]);
    assert_eq!(single(Skill::Nightmare), single(Skill::Hard));
    assert_eq!(
        monsters(sheet(&found, Skill::Normal, GameMode::Deathmatch)),
        [1, 2, 3],
        "deathmatch tests 2048 and no skill flag"
    );
    assert_eq!(
        monsters(sheet(&found, Skill::Normal, GameMode::Coop)),
        [1, 3, 4],
        "coop is not deathmatch"
    );
}

// spec BALANCE-2.1: a monster is what its spawn function counts in `total_monsters`, with the
// health it sets, which may read the skill; a spawner counting two is two
// spec BALANCE-2.5: the sheet's monster total is the program's `total_monsters` after the spawn
#[test]
fn a_mods_monster_is_counted_with_the_health_its_code_gives() {
    let (mut session, _dir) = session(
        "monsters",
        &[
            entity("monster_grunt", &[]),
            entity("monster_grunt", &[]),
            entity("monster_pair", &[]),
            entity("trigger_secret", &[]),
        ],
    );
    let found = balance(&mut session, json!({}));
    for (skill, grunt) in [
        (Skill::Easy, 30.0),
        (Skill::Normal, 40.0),
        (Skill::Hard, 50.0),
        (Skill::Nightmare, 60.0),
    ] {
        let sheet = sheet(&found, skill, GameMode::Single);
        let lines: Vec<(&str, f32, f32)> = sheet
            .map
            .monsters
            .iter()
            .map(|line| (line.classname.as_str(), line.count, line.health))
            .collect();
        assert_eq!(
            lines,
            [
                ("monster_grunt", 2.0, 2.0 * grunt),
                ("monster_pair", 2.0, 100.0)
            ]
        );
        assert_eq!(sheet.map.monster_count, 4.0);
        assert_eq!(sheet.map.monster_count, sheet.total_monsters);
        assert_eq!(sheet.map.secrets.amount, 1.0);
        assert_eq!(sheet.map.secrets.amount, sheet.total_secrets);
        assert_eq!(sheet.map.secrets.entities, [EntityIndex::new(4)]);
    }
}

// spec BALANCE-3.1: what a pickup gives is what its `touch` gives a player brought into the game
// spec BALANCE-3.2: the player is emptied first, and only a gain counts: a trigger that hurts
// gives nothing
// spec BALANCE-3.3: the engine's `ammo_shells` and a mod's own `ammo_plasma` are both read
// spec BALANCE-4.2: id's ammunition is counted with the most id's weapons do with one unit; a
// mod's own is counted with no damage
#[test]
fn each_pickup_gives_what_its_touch_gives() {
    let (mut session, _dir) = session(
        "pickups",
        &[
            entity("item_shells", &[]),
            entity("item_shells", &[]),
            entity("item_plasma", &[]),
            entity("item_health", &[]),
            entity("item_armor1", &[]),
            entity("weapon_supershotgun", &[]),
            entity("trigger_hurt", &[]),
            entity("monster_grunt", &[]),
        ],
    );
    let found = balance(&mut session, json!({}));
    let sheet = sheet(&found, Skill::Normal, GameMode::Single);
    let shells = damage_per_unit("ammo_shells").unwrap();
    assert_eq!(
        sheet.map.ammo,
        [
            AmmoLine {
                ammo: "ammo_plasma".to_owned(),
                units: 7.0,
                damage: None,
                entities: vec![EntityIndex::new(3)],
            },
            AmmoLine {
                ammo: "ammo_shells".to_owned(),
                units: 45.0,
                damage: Some(45.0 * shells),
                entities: vec![
                    EntityIndex::new(1),
                    EntityIndex::new(2),
                    EntityIndex::new(6)
                ],
            },
        ]
    );
    assert_eq!(sheet.map.health.amount, 25.0);
    assert_eq!(sheet.map.health.entities, [EntityIndex::new(4)]);
    assert_eq!(sheet.map.armour.amount, 100.0);
    assert_eq!(sheet.map.weapons.len(), 1);
    assert_eq!(sheet.map.weapons[0].classname, "weapon_supershotgun");
    assert_eq!(sheet.map.weapons[0].weapons, [Weapon::SuperShotgun]);
    assert_eq!(sheet.map.damage, 45.0 * shells);
    assert_eq!(sheet.map.monster_health, 40.0);
    assert_eq!(sheet.map.ratio, Some(45.0 * shells / 40.0));
    assert_eq!(
        sheet.ratio, sheet.map.ratio,
        "the player starts with nothing"
    );
}

// spec BALANCE-2.3: a sheet gives the selection's own tally, and none without one
#[test]
fn the_selection_has_its_own_tally() {
    let (mut session, _dir) = session(
        "selection",
        &[entity("monster_grunt", &[]), entity("item_health", &[])],
    );
    let found = balance(&mut session, json!({ "selection": [2] }));
    let sheet = sheet(&found, Skill::Normal, GameMode::Single);
    let selected = sheet.selection.as_ref().unwrap();
    assert!(selected.monsters.is_empty());
    assert_eq!(selected.health.amount, 25.0);
    assert_eq!(selected.ratio, None, "no monster health to take");
    assert!(
        selected.damage.is_sign_positive(),
        "no ammunition is 0 damage, not -0"
    );
    let found = balance(&mut session, json!({}));
    assert!(found.sheets.iter().all(|sheet| sheet.selection.is_none()));
}

// spec BALANCE-2.4: each layer and group has the tally of what is in it, however deep
#[test]
fn each_layer_and_group_has_its_tally() {
    let layer = "{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_layer\"\n\
        \"_tb_name\" \"Upstairs\"\n\"_tb_id\" \"1\"\n}\n";
    let group = "{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\
        \"_tb_name\" \"Ambush\"\n\"_tb_id\" \"2\"\n\"_tb_layer\" \"1\"\n}\n";
    let (mut session, _dir) = session(
        "rooms",
        &[
            layer.to_owned(),
            group.to_owned(),
            entity("monster_grunt", &[("_tb_group", "2")]),
            entity("monster_grunt", &[("_tb_layer", "1")]),
            entity("monster_grunt", &[]),
        ],
    );
    let found = balance(&mut session, json!({}));
    let sheet = sheet(&found, Skill::Normal, GameMode::Single);
    let rooms: Vec<(usize, &str, f32)> = sheet
        .rooms
        .iter()
        .map(|room| {
            (
                room.entity.get(),
                room.name.as_str(),
                room.tally.monster_count,
            )
        })
        .collect();
    assert_eq!(rooms, [(1, "Upstairs", 2.0), (2, "Ambush", 1.0)]);
    assert_eq!(sheet.map.monster_count, 3.0);
}

// spec BALANCE-2.6: the sheets of one map are the same on every run
#[test]
fn the_sheets_are_the_same_on_every_run() {
    let (mut session, _dir) = session(
        "again",
        &[entity("monster_grunt", &[]), entity("item_shells", &[])],
    );
    let first = balance(&mut session, json!({}));
    assert_eq!(balance(&mut session, json!({})), first);
}

// spec BALANCE-4.1: the table is id's weapons as docs/agent-reference/weapons.md quotes them
#[test]
fn the_damage_table_is_the_reference_page() {
    let page = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/agent-reference/weapons.md"),
    )
    .unwrap();
    for weapon in &ID_WEAPONS {
        let name = serde_json::to_value(weapon.weapon).unwrap();
        let row = format!(
            "| `{}` | `{}` | {} | {} |",
            name.as_str().unwrap(),
            weapon.ammo,
            weapon.units,
            weapon.damage
        );
        assert!(page.contains(&row), "weapons.md has no row {row}");
    }
    assert_eq!(damage_per_unit("ammo_shells"), Some(28.0));
    assert_eq!(damage_per_unit("ammo_nails"), Some(9.0));
    assert_eq!(damage_per_unit("ammo_rockets"), Some(120.0));
    assert_eq!(damage_per_unit("ammo_cells"), Some(30.0));
    assert_eq!(damage_per_unit("ammo_plasma"), None);
}

// spec BALANCE-1.2: the command is in the registry and needs an open project and map
#[test]
fn the_command_needs_a_project_and_a_map() {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    assert!(session.execute(MAP_BALANCE, json!({})).is_err());
    assert!(
        session
            .execute(MAP_BALANCE, json!({ "unknown": 1 }))
            .is_err()
    );
}

// spec BALANCE-3.4: a map the program cannot put a player in still has its monsters and
// secrets, its pickups giving nothing, and says why
#[test]
fn a_map_with_no_entry_still_counts_its_monsters() {
    let dir = std::env::temp_dir().join(format!("qs-wb-balance-entry-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    let defs = system_defs().replace(
        "void() PutClientInServer = {};",
        "void() PutClientInServer = { world.think(); };",
    );
    std::fs::write(dir.join("defs.qc"), defs).unwrap();
    std::fs::write(dir.join("world.qc"), CODE).unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\ndefs.qc\nworld.qc\n").unwrap();
    let entities = [entity("monster_grunt", &[]), entity("item_health", &[])].concat();
    std::fs::write(dir.join("maps/test.map"), format!("{WORLD}{entities}")).unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute(OPEN_PROJECT, json!({ "path": dir }))
        .unwrap();
    session
        .execute(MAP_OPEN, json!({ "path": dir.join("maps/test.map") }))
        .unwrap();
    let found = balance(&mut session, json!({}));
    let sheet = sheet(&found, Skill::Normal, GameMode::Single);
    assert!(matches!(sheet.entry, super::Entry::NotEntered { .. }));
    assert_eq!(sheet.map.monster_count, 1.0);
    assert_eq!(sheet.map.health.amount, 0.0);
}

// spec BALANCE-2.1: an entity whose spawn function counted monsters and then freed itself still
// counts them, with no health, so the sheet's total stays the program's
// spec BALANCE-3.2: a touch that fails gives nothing, whatever it wrote before it failed
#[test]
fn a_freed_spawner_counts_and_a_failed_touch_gives_nothing() {
    let (mut session, _dir) = session(
        "freed",
        &[entity("monster_spawner", &[]), entity("item_bad", &[])],
    );
    let found = balance(&mut session, json!({}));
    let sheet = sheet(&found, Skill::Normal, GameMode::Single);
    assert_eq!(sheet.map.monster_count, 3.0);
    assert_eq!(sheet.map.monster_count, sheet.total_monsters);
    assert_eq!(sheet.map.monsters[0].classname, "monster_spawner");
    assert_eq!(sheet.map.monster_health, 0.0);
    assert!(sheet.map.ammo.is_empty(), "{:?}", sheet.map.ammo);
}
