// SPDX-License-Identifier: GPL-3.0-or-later

//! Where a bot goes (BOT-4): its need of each item and friend, weighed by the route's length.

use qs_sim::Body;

use crate::weapons::{Ammo, Weapons};

/// `SOLID_TRIGGER` (`defs.qc`): an item that lies where it can be picked up.
const SOLID_TRIGGER: f32 = 1.0;
/// How far a route's length halves a need (BOT-4.1).
const HALF_WEIGHT_LENGTH: f32 = 256.0;
/// What a bot needs a weapon it holds, for its ammunition.
const HELD_WEAPON: f32 = 5.0;
/// Full health, above which only a megahealth is taken (`items.qc`, `health_touch`).
const FULL_HEALTH: f32 = 100.0;
/// The armours: classname, the `armorvalue` below which a bot needs it, and how much.
const ARMOURS: [(&[u8], f32, f32); 3] = [
    (b"item_armor1", 100.0, 30.0),
    (b"item_armor2", 150.0, 50.0),
    (b"item_armorInv", 200.0, 80.0),
];
/// What a bot needs an ammunition box for a weapon it holds when it is low, and otherwise.
const LOW_AMMO: f32 = 20.0;
const SPARE_AMMO: f32 = 2.0;
/// The powerups, each needed as much.
const POWERUPS: [&[u8]; 4] = [
    b"item_artifact_super_damage",
    b"item_artifact_invulnerability",
    b"item_artifact_invisibility",
    b"item_artifact_envirosuit",
];
const POWERUP: f32 = 60.0;
/// How far from its nearest friend a bot in coop goes to it, and how much it needs to then
/// (BOT-4.3).
pub(crate) const FOLLOW_DISTANCE: f32 = 256.0;
const FOLLOW: f32 = 100.0;

/// How much a bot of `me` needs the item `item` (BOT-4.2): 0 for one it does not need or that
/// cannot be picked up.
#[must_use]
pub fn item_need(me: &Body, item: &Body, weapons: &Weapons) -> f32 {
    #[expect(
        clippy::float_cmp_const,
        reason = "spec BOT-4.2: solid holds a whole number"
    )]
    let lies = item.solid == SOLID_TRIGGER;
    if !lies || item.modelindex == 0.0 {
        return 0.0;
    }
    let class = item.classname.as_slice();
    if let Some(weapon) = weapons.by_pickup(class) {
        return if weapon.held(me) {
            HELD_WEAPON
        } else {
            weapon.value
        };
    }
    if class == b"item_health" {
        return (FULL_HEALTH - me.health).max(0.0);
    }
    if let Some(&(_, below, need)) = ARMOURS.iter().find(|(name, _, _)| *name == class) {
        return if me.armorvalue < below { need } else { 0.0 };
    }
    if let Some(ammo) = Ammo::of_box(class) {
        let low = ammo.of(me) < ammo.most() / 2.0;
        return if low && weapons.uses(me, ammo) {
            LOW_AMMO
        } else {
            SPARE_AMMO
        };
    }
    if POWERUPS.contains(&class) {
        return POWERUP;
    }
    0.0
}

/// How much a bot `distance` from its nearest friend needs to go to it (BOT-4.3): nothing
/// within [`FOLLOW_DISTANCE`], else 100 and 1 more for each unit past it.
#[must_use]
pub fn follow_need(distance: f32) -> f32 {
    if distance > FOLLOW_DISTANCE {
        FOLLOW + (distance - FOLLOW_DISTANCE)
    } else {
        0.0
    }
}

/// A need weighed by its route's length (BOT-4.1).
#[must_use]
pub fn weigh(need: f32, length: f32) -> f32 {
    need / (1.0 + length / HALF_WEIGHT_LENGTH)
}

/// The goal of the greatest weight above 0 among `goals`, each its need and its route's length,
/// `None` for one with no route; the first of those that weigh the same.
#[must_use]
pub fn choose(goals: &[(f32, Option<f32>)]) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (n, &(need, length)) in goals.iter().enumerate() {
        let Some(length) = length else {
            continue;
        };
        let weight = weigh(need, length);
        if weight > 0.0 && best.is_none_or(|(_, b)| weight > b) {
            best = Some((n, weight));
        }
    }
    best.map(|(n, _)| n)
}
