// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.balance` (L-78, MAP-37): a balance sheet of the open map on each skill and in each kind
//! of game, read from the game's own code: the map spawned in `qs-sim` with the project's server
//! program as `map.entities_removed_by_skill` spawns it (`crate::map_filter`), each monster and
//! secret the change its spawn function made to the program's counters, each pickup what its
//! `touch` gives a player brought into the game, and the damage of id's four kinds of ammo from
//! id's weapons (`docs/agent-reference/weapons.md`). Nothing here reads a classname, so a mod's
//! monsters and items are counted as its code spawns them.
//!
//! Implements `docs/spec/balance.md` BALANCE-2 to BALANCE-4.

use std::borrow::Cow;
use std::collections::BTreeMap;

use qc_progs::Progs;
use qc_vm::convert::float_to_int;
use qs_commands::{Command, Outcome, RegisterError, Registry};
use qs_map::{EntityPlace, MapFile, RoomKind};
use qs_map_edit::EntityIndex;
use qs_sim::scenario::{SCN_FRAMETIME, Seconds};
use qs_sim::spawn::SpawnCounts;
use qs_sim::{CLIENT_EDICT, FireBy, FireOptions, FireTrace, MapPlace, QcValue, TraceEnd, World};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::map_filter::{GameMode, MapSpawn, Skill};
use crate::{HasWorkbench, Workbench, WorkbenchError};

/// The id of `map.balance`.
pub const MAP_BALANCE: &str = "map.balance";

/// The skills a sheet is made for, 0 to 3 (EDICT-8.4).
pub const SKILLS: [Skill; 4] = [Skill::Easy, Skill::Normal, Skill::Hard, Skill::Nightmare];
/// The kinds of game a sheet is made for.
pub const MODES: [GameMode; 3] = [GameMode::Single, GameMode::Coop, GameMode::Deathmatch];

/// The player's health while a pickup is touched (BALANCE-3.2): above 0, so the player is alive,
/// and below any `max_health`, so a health pickup gives all it gives.
const PROBE_HEALTH: f32 = 1.0;
/// The prefix of the fields a player's ammunition is kept in (BALANCE-3.3).
const AMMO_PREFIX: &str = "ammo_";

/// `IT_SHOTGUN` of `WinQuake/quakedef.h`.
const IT_SHOTGUN: i32 = 1;
/// `IT_SUPER_SHOTGUN` of `WinQuake/quakedef.h`.
const IT_SUPER_SHOTGUN: i32 = 2;
/// `IT_NAILGUN` of `WinQuake/quakedef.h`.
const IT_NAILGUN: i32 = 4;
/// `IT_SUPER_NAILGUN` of `WinQuake/quakedef.h`.
const IT_SUPER_NAILGUN: i32 = 8;
/// `IT_GRENADE_LAUNCHER` of `WinQuake/quakedef.h`.
const IT_GRENADE_LAUNCHER: i32 = 16;
/// `IT_ROCKET_LAUNCHER` of `WinQuake/quakedef.h`.
const IT_ROCKET_LAUNCHER: i32 = 32;
/// `IT_LIGHTNING` of `WinQuake/quakedef.h`.
const IT_LIGHTNING: i32 = 64;
/// `IT_SUPER_LIGHTNING` of `WinQuake/quakedef.h`.
const IT_SUPER_LIGHTNING: i32 = 128;

/// One of id's weapons, by its bit of the `items` field as the engine reads it for the status
/// bar (`WinQuake/quakedef.h`, `IT_SHOTGUN` to `IT_SUPER_LIGHTNING`).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Weapon {
    /// `IT_SHOTGUN`, 1.
    Shotgun,
    /// `IT_SUPER_SHOTGUN`, 2.
    SuperShotgun,
    /// `IT_NAILGUN`, 4.
    Nailgun,
    /// `IT_SUPER_NAILGUN`, 8.
    SuperNailgun,
    /// `IT_GRENADE_LAUNCHER`, 16.
    GrenadeLauncher,
    /// `IT_ROCKET_LAUNCHER`, 32.
    RocketLauncher,
    /// `IT_LIGHTNING`, 64.
    Lightning,
    /// `IT_SUPER_LIGHTNING`, 128, which id's own QuakeC never gives.
    SuperLightning,
}

impl Weapon {
    /// Every weapon, in the order of its bit.
    pub const ALL: [Self; 8] = [
        Self::Shotgun,
        Self::SuperShotgun,
        Self::Nailgun,
        Self::SuperNailgun,
        Self::GrenadeLauncher,
        Self::RocketLauncher,
        Self::Lightning,
        Self::SuperLightning,
    ];

    /// Its bit of `items` (`WinQuake/quakedef.h`).
    #[must_use]
    pub const fn bit(self) -> i32 {
        match self {
            Self::Shotgun => IT_SHOTGUN,
            Self::SuperShotgun => IT_SUPER_SHOTGUN,
            Self::Nailgun => IT_NAILGUN,
            Self::SuperNailgun => IT_SUPER_NAILGUN,
            Self::GrenadeLauncher => IT_GRENADE_LAUNCHER,
            Self::RocketLauncher => IT_ROCKET_LAUNCHER,
            Self::Lightning => IT_LIGHTNING,
            Self::SuperLightning => IT_SUPER_LIGHTNING,
        }
    }

    /// The weapons whose bits `items` holds.
    fn in_items(items: i32) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|weapon| items & weapon.bit() != 0)
            .collect()
    }
}

/// What one shot of one of id's weapons takes and does, quoted from `weapons.qc` of id 1.01
/// (`id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73`, `qcc/v101qc`) and of the
/// rerelease (`id-Software/quake-rerelease-qc@634eefab09a77eb7b5f5ca7078ba3d8784a91142`), which
/// agree; `docs/agent-reference/weapons.md` gives each line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WeaponDamage {
    /// The weapon.
    pub weapon: Weapon,
    /// The field its ammunition is kept in.
    pub ammo: Cow<'static, str>,
    /// The units of it one shot takes.
    pub units: f32,
    /// The most one shot does to one target, a random part at its least (BALANCE-4.1).
    pub damage: f32,
}

impl WeaponDamage {
    /// The damage of one unit of its ammunition.
    #[must_use]
    pub fn per_unit(&self) -> f32 {
        self.damage / self.units
    }
}

/// id's weapons that take ammunition (BALANCE-4.1), from `weapons.qc`: `W_FireShotgun`'s
/// `FireBullets (6, ...)` of `TraceAttack (4, ...)` for one shell, `W_FireSuperShotgun`'s 14 for
/// two, `spike_touch`'s 9 for one nail, `superspike_touch`'s 18 for two, `GrenadeExplode`'s
/// `T_RadiusDamage (self, self.owner, 120, world)`, `T_MissileTouch`'s `100 + random()*20` and
/// `W_FireLightning`'s `LightningDamage (..., 30)` for one cell.
pub const ID_WEAPONS: [WeaponDamage; 7] = [
    WeaponDamage {
        weapon: Weapon::Shotgun,
        ammo: Cow::Borrowed("ammo_shells"),
        units: 1.0,
        damage: 24.0,
    },
    WeaponDamage {
        weapon: Weapon::SuperShotgun,
        ammo: Cow::Borrowed("ammo_shells"),
        units: 2.0,
        damage: 56.0,
    },
    WeaponDamage {
        weapon: Weapon::Nailgun,
        ammo: Cow::Borrowed("ammo_nails"),
        units: 1.0,
        damage: 9.0,
    },
    WeaponDamage {
        weapon: Weapon::SuperNailgun,
        ammo: Cow::Borrowed("ammo_nails"),
        units: 2.0,
        damage: 18.0,
    },
    WeaponDamage {
        weapon: Weapon::GrenadeLauncher,
        ammo: Cow::Borrowed("ammo_rockets"),
        units: 1.0,
        damage: 120.0,
    },
    WeaponDamage {
        weapon: Weapon::RocketLauncher,
        ammo: Cow::Borrowed("ammo_rockets"),
        units: 1.0,
        damage: 100.0,
    },
    WeaponDamage {
        weapon: Weapon::Lightning,
        ammo: Cow::Borrowed("ammo_cells"),
        units: 1.0,
        damage: 30.0,
    },
];

/// The damage one unit of the ammunition kept in `ammo` can do: the most of any of id's weapons
/// that fire it (BALANCE-4.2); `None` for a mod's own.
#[must_use]
pub fn damage_per_unit(ammo: &str) -> Option<f32> {
    ID_WEAPONS
        .iter()
        .filter(|weapon| weapon.ammo == ammo)
        .map(WeaponDamage::per_unit)
        .reduce(f32::max)
}

/// The parameters of `map.balance`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BalanceParams {
    /// The entities selected, by place in the map, whose own tally each sheet gives; none when
    /// empty.
    #[serde(default)]
    pub selection: Vec<EntityIndex>,
}

/// What a player has, or what a pickup gives one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Inventory {
    /// Health.
    pub health: f32,
    /// Armour, `armorvalue`.
    pub armour: f32,
    /// Each kind of ammunition by its field, and how many units.
    pub ammo: BTreeMap<String, f32>,
    /// id's weapons, by their bits of `items`.
    pub weapons: Vec<Weapon>,
}

impl Inventory {
    /// The damage its ammunition can do (BALANCE-4.2): a mod's own counts nothing.
    fn damage(&self) -> f32 {
        self.ammo
            .iter()
            .filter_map(|(ammo, units)| damage_per_unit(ammo).map(|per_unit| per_unit * units))
            .fold(0.0, |total, damage| total + damage)
    }

    /// The player edict's inventory in `world` (BALANCE-3.3).
    fn of_player(world: &World, progs: &Progs<'_>) -> Self {
        let floats = player_floats(world, progs);
        let read = |name: &str| floats.get(name).copied().unwrap_or(0.0);
        Self {
            health: read("health"),
            armour: read("armorvalue"),
            ammo: floats
                .iter()
                .filter(|(name, _)| name.starts_with(AMMO_PREFIX))
                .map(|(name, &units)| (name.clone(), units))
                .collect(),
            weapons: Weapon::in_items(float_to_int(read("items"))),
        }
    }

    /// What a pickup gave: `after` less [`Inventory::emptied`]'s player, each gain above 0.
    fn given(after: &Self) -> Self {
        Self {
            health: (after.health - PROBE_HEALTH).max(0.0),
            armour: after.armour.max(0.0),
            ammo: after
                .ammo
                .iter()
                .filter(|&(_, &units)| units > 0.0)
                .map(|(name, &units)| (name.clone(), units))
                .collect(),
            weapons: after.weapons.clone(),
        }
    }
}

/// Every float field of the player edict by name.
fn player_floats(world: &World, progs: &Progs<'_>) -> BTreeMap<String, f32> {
    world
        .edict_fields(progs, CLIENT_EDICT)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|field| match field.value {
            QcValue::Float(value) => Some((field.name, value)),
            QcValue::Vector(_)
            | QcValue::String(_)
            | QcValue::Entity(_)
            | QcValue::Field(_)
            | QcValue::Function(_)
            | QcValue::Void => None,
        })
        .collect()
}

/// One of the map's entities as the game spawned it on one sheet.
#[derive(Clone, Debug, PartialEq)]
struct Spawned {
    entity: EntityIndex,
    classname: String,
    /// What its spawn function added to `total_monsters` and `total_secrets`.
    monsters: f32,
    secrets: f32,
    /// Its `health` after its spawn function.
    health: f32,
    /// What its `touch` gives a player.
    gives: Inventory,
}

/// One class of monster on a sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MonsterLine {
    /// The classname the map gives it.
    pub classname: String,
    /// How many the program counted (`total_monsters`).
    pub count: f32,
    /// Their health, each its `health` after its spawn function.
    pub health: f32,
    /// The entities.
    pub entities: Vec<EntityIndex>,
}

/// One class of pickup that gives a weapon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WeaponLine {
    /// The classname the map gives it.
    pub classname: String,
    /// The weapons it gives.
    pub weapons: Vec<Weapon>,
    /// The entities.
    pub entities: Vec<EntityIndex>,
}

/// One kind of ammunition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AmmoLine {
    /// The field it is kept in.
    pub ammo: String,
    /// How many units the pickups give.
    pub units: f32,
    /// The damage they can do with id's weapons; `None` for a mod's own (BALANCE-4.2).
    pub damage: Option<f32>,
    /// The entities that give it.
    pub entities: Vec<EntityIndex>,
}

/// An amount and the entities that make it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AmountLine {
    /// The amount.
    pub amount: f32,
    /// The entities.
    pub entities: Vec<EntityIndex>,
}

/// What some of the map's entities hold on one sheet (BALANCE-2.3).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Tally {
    /// The monsters by class.
    pub monsters: Vec<MonsterLine>,
    /// How many monsters.
    pub monster_count: f32,
    /// Their health together.
    pub monster_health: f32,
    /// The pickups that give a weapon, by class.
    pub weapons: Vec<WeaponLine>,
    /// The ammunition by kind.
    pub ammo: Vec<AmmoLine>,
    /// The health given.
    pub health: AmountLine,
    /// The armour given.
    pub armour: AmountLine,
    /// The secrets (`total_secrets`).
    pub secrets: AmountLine,
    /// The damage the ammunition can do.
    pub damage: f32,
    /// [`Tally::damage`] over [`Tally::monster_health`]; `None` with no monster health.
    pub ratio: Option<f32>,
}

impl Tally {
    /// The tally of `spawned`.
    fn of<'a>(spawned: impl IntoIterator<Item = &'a Spawned>) -> Self {
        let mut tally = Self::default();
        let mut monsters: BTreeMap<String, MonsterLine> = BTreeMap::new();
        let mut weapons: BTreeMap<String, WeaponLine> = BTreeMap::new();
        let mut ammo: BTreeMap<String, AmmoLine> = BTreeMap::new();
        for entity in spawned {
            if entity.monsters != 0.0 {
                let line =
                    monsters
                        .entry(entity.classname.clone())
                        .or_insert_with(|| MonsterLine {
                            classname: entity.classname.clone(),
                            count: 0.0,
                            health: 0.0,
                            entities: Vec::new(),
                        });
                line.count += entity.monsters;
                line.health += entity.health;
                line.entities.push(entity.entity);
                tally.monster_count += entity.monsters;
                tally.monster_health += entity.health;
            }
            if entity.secrets != 0.0 {
                tally.secrets.amount += entity.secrets;
                tally.secrets.entities.push(entity.entity);
            }
            let gives = &entity.gives;
            if !gives.weapons.is_empty() {
                let line = weapons
                    .entry(entity.classname.clone())
                    .or_insert_with(|| WeaponLine {
                        classname: entity.classname.clone(),
                        weapons: gives.weapons.clone(),
                        entities: Vec::new(),
                    });
                line.entities.push(entity.entity);
            }
            for (kind, &units) in &gives.ammo {
                let line = ammo.entry(kind.clone()).or_insert_with(|| AmmoLine {
                    ammo: kind.clone(),
                    units: 0.0,
                    damage: damage_per_unit(kind).map(|_| 0.0),
                    entities: Vec::new(),
                });
                line.units += units;
                line.damage = damage_per_unit(kind).map(|per_unit| per_unit * line.units);
                line.entities.push(entity.entity);
            }
            if gives.health > 0.0 {
                tally.health.amount += gives.health;
                tally.health.entities.push(entity.entity);
            }
            if gives.armour > 0.0 {
                tally.armour.amount += gives.armour;
                tally.armour.entities.push(entity.entity);
            }
        }
        tally.monsters = monsters.into_values().collect();
        tally.weapons = weapons.into_values().collect();
        tally.ammo = ammo.into_values().collect();
        // A fold from 0, since a float `sum` of nothing is -0.
        tally.damage = tally
            .ammo
            .iter()
            .filter_map(|line| line.damage)
            .fold(0.0, |total, damage| total + damage);
        tally.ratio = ratio(tally.damage, tally.monster_health);
        tally
    }
}

/// `damage` over `health`, `None` when there is no health to take.
fn ratio(damage: f32, health: f32) -> Option<f32> {
    (health > 0.0).then(|| damage / health)
}

/// The kind of a room of the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoomKindName {
    /// A layer.
    Layer,
    /// A group.
    Group,
}

impl From<RoomKind> for RoomKindName {
    fn from(kind: RoomKind) -> Self {
        match kind {
            RoomKind::Layer => Self::Layer,
            RoomKind::Group => Self::Group,
        }
    }
}

/// The tally of one layer or group: the entities in it, however deep (BALANCE-2.4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RoomTally {
    /// The layer's or group's own entity.
    pub entity: EntityIndex,
    /// Layer or group.
    pub kind: RoomKindName,
    /// Its `_tb_name`, empty for none.
    pub name: String,
    /// What it holds.
    pub tally: Tally,
}

/// Whether the player was brought into the game, which touching the pickups needs
/// (BALANCE-3.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entry {
    /// The player entered, and each pickup was touched.
    Entered,
    /// The program could not put a player in the map, as id's `PutClientInServer` cannot on a
    /// level with no `info_player_start`: the pickups give nothing on the sheet.
    NotEntered {
        /// Why.
        error: String,
    },
}

/// The sheet of one skill and one kind of game (BALANCE-2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BalanceSheet {
    /// The skill.
    pub skill: Skill,
    /// The kind of game.
    pub mode: GameMode,
    /// Whether the player entered the game and the pickups were touched.
    pub entry: Entry,
    /// What the player has on entering the map, as the program gives it.
    pub start: Inventory,
    /// The whole map.
    pub map: Tally,
    /// Each layer and group, in map order.
    pub rooms: Vec<RoomTally>,
    /// The entities selected; `None` with no selection.
    pub selection: Option<Tally>,
    /// The program's `total_monsters` after the map spawned (EDICT-11.8).
    pub total_monsters: f32,
    /// The program's `total_secrets` after the map spawned (EDICT-11.8).
    pub total_secrets: f32,
    /// The damage the player's own ammunition and the map's can do, over the map's monsters'
    /// health; `None` with no monster health.
    pub ratio: Option<f32>,
}

/// What `map.balance` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapBalance {
    /// A sheet for each skill, 0 to 3, in each kind of game, skill by skill.
    pub sheets: Vec<BalanceSheet>,
    /// The damage of id's weapons the ammunition is counted with (BALANCE-4.1).
    pub weapons: Vec<WeaponDamage>,
}

/// The map's entities as one sheet's game spawned them, with what each gives, whether the
/// player entered, its inventory then, and the program's counters after the spawn.
struct Touched {
    spawned: Vec<Spawned>,
    entry: Entry,
    start: Inventory,
    total_monsters: f32,
    total_secrets: f32,
}

impl MapSpawn {
    /// The map's entities as the game spawns them at `skill` in a game of `mode`, with what
    /// each counted and gives, and the program's counters and the player's inventory after.
    fn balance_of(&self, skill: Skill, mode: GameMode) -> Result<Touched, WorkbenchError> {
        self.world(skill, mode, |progs, mut world| {
            let globals: BTreeMap<String, f32> = world
                .named_globals(progs)
                .into_iter()
                .filter_map(|global| match global.value {
                    QcValue::Float(value) => Some((global.name, value)),
                    QcValue::Vector(_)
                    | QcValue::String(_)
                    | QcValue::Entity(_)
                    | QcValue::Field(_)
                    | QcValue::Function(_)
                    | QcValue::Void => None,
                })
                .collect();
            let counter = |name: &str| globals.get(name).copied().unwrap_or(0.0);
            let (total_monsters, total_secrets) =
                (counter("total_monsters"), counter("total_secrets"));
            // BALANCE-2.1: what the spawn left, before any frame runs. An entity whose spawn
            // function counted itself and then freed its edict still counts: the counters keep
            // it, with no health.
            let classnames = world.map_classnames();
            let mut spawned = Vec::new();
            for (lump, &place) in self.places().iter().enumerate() {
                let map_place = MapPlace::new(lump);
                let counts = world.map_counts(map_place);
                let edict = world.map_edict(map_place);
                if edict.is_none() && counts == SpawnCounts::default() {
                    continue;
                }
                let health = edict.map_or(0.0, |edict| float_field(&world, progs, edict, "health"));
                let classname = edict.map_or_else(
                    || classnames.get(lump).cloned().unwrap_or_default(),
                    |edict| world.classname(progs, edict),
                );
                spawned.push((
                    map_place,
                    Spawned {
                        entity: EntityIndex::new(place),
                        classname,
                        monsters: counts.monsters,
                        secrets: counts.secrets,
                        health,
                        gives: Inventory::default(),
                    },
                ));
            }
            // BALANCE-3.1: the player brought in, then each pickup touched in a copy.
            if let Err(error) = world.enter(progs, SCN_FRAMETIME) {
                let spawned = spawned.into_iter().map(|(_, entity)| entity).collect();
                return Ok(Touched {
                    spawned,
                    entry: Entry::NotEntered {
                        error: error.to_string(),
                    },
                    start: Inventory::default(),
                    total_monsters,
                    total_secrets,
                });
            }
            let start = Inventory::of_player(&world, progs);
            let mut out = Vec::with_capacity(spawned.len());
            for (map_place, mut entity) in spawned {
                // An entity the frames since freed gives nothing: id's `PlaceItem` removes an
                // item that falls out of the level. One with no `touch` is not copied to be
                // touched.
                if entity.monsters == 0.0
                    && let Some(edict) = world.map_edict(map_place)
                    && has_touch(&world, progs, edict)
                {
                    entity.gives = touched(&world, progs, edict);
                }
                out.push(entity);
            }
            Ok(Touched {
                spawned: out,
                entry: Entry::Entered,
                start,
                total_monsters,
                total_secrets,
            })
        })
    }
}

/// What touching edict `edict` gives a player emptied of everything (BALANCE-3.2), in a copy
/// of `world`: nothing for an edict with no `touch`, or whose `touch` fails.
fn touched(world: &World, progs: &Progs<'_>, edict: usize) -> Inventory {
    let mut probe = world.clone();
    let mut empty: Vec<(String, f32)> = player_floats(&probe, progs)
        .into_keys()
        .filter(|name| name.starts_with(AMMO_PREFIX))
        .map(|name| (name, 0.0))
        .collect();
    empty.extend([
        ("health".to_owned(), PROBE_HEALTH),
        ("armorvalue".to_owned(), 0.0),
        ("armortype".to_owned(), 0.0),
        ("items".to_owned(), 0.0),
    ]);
    for (name, value) in &empty {
        // A field a program does not have is not emptied.
        drop(probe.set_field_value(progs, CLIENT_EDICT, name, &QcValue::Float(*value)));
    }
    let options = FireOptions {
        by: FireBy::Touch,
        length: Seconds::new(0.0),
        frametime: SCN_FRAMETIME,
    };
    match probe.fire(progs, edict, options) {
        // BALANCE-3.2: a touch that fails gives nothing, whatever it wrote before it failed.
        Ok(FireTrace {
            end: TraceEnd::Error(_),
            ..
        })
        | Err(_) => Inventory::default(),
        Ok(_) => Inventory::given(&Inventory::of_player(&probe, progs)),
    }
}

/// Whether edict `edict` has a `touch` function.
fn has_touch(world: &World, progs: &Progs<'_>, edict: usize) -> bool {
    world
        .edict_fields(progs, edict)
        .unwrap_or_default()
        .into_iter()
        .any(|field| field.name == "touch" && field.value != QcValue::Function(String::new()))
}

/// Float field `name` of edict `edict`, 0 when it has none.
fn float_field(world: &World, progs: &Progs<'_>, edict: usize, name: &str) -> f32 {
    world
        .edict_fields(progs, edict)
        .unwrap_or_default()
        .into_iter()
        .find_map(|field| match field.value {
            QcValue::Float(value) if field.name == name => Some(value),
            QcValue::Float(_)
            | QcValue::Vector(_)
            | QcValue::String(_)
            | QcValue::Entity(_)
            | QcValue::Field(_)
            | QcValue::Function(_)
            | QcValue::Void => None,
        })
        .unwrap_or(0.0)
}

/// The places of the layers and groups each entity is in, however deep (BALANCE-2.4).
fn rooms_of_each(file: &MapFile) -> Vec<Vec<usize>> {
    let parents = file.parents();
    (0..parents.len())
        .map(|place| {
            let mut rooms = Vec::new();
            let mut at = parents.get(place).copied().flatten();
            // A chain no longer than the map; `parents` has broken every loop.
            for _ in 0..parents.len() {
                let Some(room) = at.map(EntityPlace::get) else {
                    break;
                };
                rooms.push(room);
                at = parents.get(room).copied().flatten();
            }
            rooms
        })
        .collect()
}

impl<L: crate::Loader> Workbench<L> {
    /// The balance sheets of the open map (L-78).
    fn map_balance(&mut self, params: &BalanceParams) -> Result<MapBalance, WorkbenchError> {
        let spawn = self.map_spawn()?;
        let file = self.map().ok_or(WorkbenchError::NoMap)?.file();
        let rooms = rooms_of_each(file);
        let room_entities: Vec<(usize, RoomKind, String)> = file
            .entities
            .iter()
            .enumerate()
            .filter_map(|(place, entity)| {
                entity.container().map(|(kind, _)| {
                    let name = entity.text(b"_tb_name").unwrap_or_default();
                    (place, kind, name)
                })
            })
            .collect();
        let mut sheets = Vec::with_capacity(SKILLS.len().saturating_mul(MODES.len()));
        for skill in SKILLS {
            for mode in MODES {
                let Touched {
                    spawned,
                    entry,
                    start,
                    total_monsters,
                    total_secrets,
                } = spawn.balance_of(skill, mode)?;
                let map = Tally::of(&spawned);
                let in_room = |room: usize, entity: &Spawned| {
                    rooms
                        .get(entity.entity.get())
                        .is_some_and(|of| of.contains(&room))
                };
                let room_tallies = room_entities
                    .iter()
                    .map(|(room, kind, name)| RoomTally {
                        entity: EntityIndex::new(*room),
                        kind: (*kind).into(),
                        name: name.clone(),
                        tally: Tally::of(spawned.iter().filter(|e| in_room(*room, e))),
                    })
                    .collect();
                let selection = (!params.selection.is_empty()).then(|| {
                    Tally::of(
                        spawned
                            .iter()
                            .filter(|entity| params.selection.contains(&entity.entity)),
                    )
                });
                let ratio = ratio(start.damage() + map.damage, map.monster_health);
                sheets.push(BalanceSheet {
                    skill,
                    mode,
                    entry,
                    start,
                    map,
                    rooms: room_tallies,
                    selection,
                    total_monsters,
                    total_secrets,
                    ratio,
                });
            }
        }
        Ok(MapBalance {
            sheets,
            weapons: ID_WEAPONS.to_vec(),
        })
    }
}

/// `map.balance`.
struct MapBalanceCommand;

impl<C: HasWorkbench + 'static> Command<C> for MapBalanceCommand {
    const ID: &'static str = MAP_BALANCE;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "A balance sheet of the open map for each skill, 0 to 3, in \
        single player, coop and deathmatch, read by spawning the map with the project's program: \
        the monsters by class with the health their spawn functions give them, counted as the \
        program counts total_monsters; the pickups that give weapons; the ammunition by kind \
        with the damage id's weapons do with it (none for a mod's own); the health and armour \
        given, each what touching the pickup gives a player; the secrets; and the damage over \
        the monsters' health. Each for the whole map, each layer and group, and the entities \
        in selection when given, each line with its entities.";
    type Params = BalanceParams;
    type Output = MapBalance;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BalanceParams,
    ) -> Result<Outcome<MapBalance, C>, WorkbenchError> {
        let balance = context.workbench().map_balance(&params)?;
        Ok(Outcome::outside_history(balance))
    }
}

/// Registers `map.balance`.
///
/// # Errors
///
/// [`RegisterError`] when it is already registered.
pub(crate) fn register_map_balance<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(MapBalanceCommand)
}

#[cfg(test)]
mod tests;
