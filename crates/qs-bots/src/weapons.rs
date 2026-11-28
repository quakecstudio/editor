// SPDX-License-Identifier: GPL-3.0-or-later

//! The weapon table (BOT-6): which weapon a bot holds, by the impulses of id's
//! `W_ChangeWeapon` (`weapons.qc`), and what each weapon's pickup is worth to a bot without it.

use qc_vm::convert::float_to_int;
use qs_sim::Body;

/// `IT_*` of `defs.qc`, the weapons' bits in `items`.
const IT_SHOTGUN: u32 = 1;
const IT_SUPER_SHOTGUN: u32 = 2;
const IT_NAILGUN: u32 = 4;
const IT_SUPER_NAILGUN: u32 = 8;
const IT_GRENADE_LAUNCHER: u32 = 16;
const IT_ROCKET_LAUNCHER: u32 = 32;
const IT_LIGHTNING: u32 = 64;
const IT_AXE: u32 = 4096;

/// The ammunition a weapon uses, one of the four counts of a player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ammo {
    /// None, as the axe.
    None,
    /// `ammo_shells`.
    Shells,
    /// `ammo_nails`.
    Nails,
    /// `ammo_rockets`.
    Rockets,
    /// `ammo_cells`.
    Cells,
}

/// The most of each ammunition id's `bound_other_ammo` lets a player carry (`items.qc`).
const MOST_SHELLS: f32 = 100.0;
const MOST_NAILS: f32 = 200.0;
const MOST_ROCKETS: f32 = 100.0;
const MOST_CELLS: f32 = 100.0;

impl Ammo {
    /// How much of it `body` has; any amount for [`Ammo::None`].
    #[must_use]
    pub fn of(self, body: &Body) -> f32 {
        let [shells, nails, rockets, cells] = body.ammo;
        match self {
            Self::None => f32::INFINITY,
            Self::Shells => shells,
            Self::Nails => nails,
            Self::Rockets => rockets,
            Self::Cells => cells,
        }
    }

    /// The most of it a player carries.
    #[must_use]
    pub const fn most(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Shells => MOST_SHELLS,
            Self::Nails => MOST_NAILS,
            Self::Rockets => MOST_ROCKETS,
            Self::Cells => MOST_CELLS,
        }
    }

    /// The ammunition box of id's `items.qc` that gives it, by classname.
    #[must_use]
    pub fn of_box(classname: &[u8]) -> Option<Self> {
        match classname {
            b"item_shells" => Some(Self::Shells),
            b"item_spikes" => Some(Self::Nails),
            b"item_rockets" => Some(Self::Rockets),
            b"item_cells" => Some(Self::Cells),
            _ => None,
        }
    }
}

/// One weapon of the table (BOT-6.1).
#[derive(Clone, Debug, PartialEq)]
pub struct Weapon {
    /// Its bit in `items`, and its `weapon` while held.
    pub bit: u32,
    /// The impulse that selects it.
    pub impulse: u8,
    /// The ammunition it uses.
    pub ammo: Ammo,
    /// The least of it the weapon fires with, below which its impulse is refused.
    pub least: f32,
    /// The classname of its pickup, empty for one that lies nowhere.
    pub pickup: Vec<u8>,
    /// How much a bot without it needs its pickup (BOT-4.2).
    pub value: f32,
}

impl Weapon {
    /// Whether `body` holds it and has the ammunition it fires with.
    #[must_use]
    pub fn ready(&self, body: &Body) -> bool {
        self.held(body) && self.ammo.of(body) >= self.least
    }

    /// Whether `body` holds it.
    #[must_use]
    pub fn held(&self, body: &Body) -> bool {
        u32::try_from(float_to_int(body.items)).is_ok_and(|items| items & self.bit != 0)
    }

    /// Whether it is `body`'s weapon in hand.
    #[must_use]
    pub fn in_hand(&self, body: &Body) -> bool {
        u32::try_from(float_to_int(body.weapon)).is_ok_and(|weapon| weapon == self.bit)
    }
}

/// The weapons a bot knows, best first (BOT-6.1).
#[derive(Clone, Debug, PartialEq)]
pub struct Weapons(Vec<Weapon>);

impl Default for Weapons {
    fn default() -> Self {
        Self::id()
    }
}

impl Weapons {
    /// A mod's own table, best first.
    #[must_use]
    pub const fn new(weapons: Vec<Weapon>) -> Self {
        Self(weapons)
    }

    /// id's weapons (BOT-6.1), with `W_ChangeWeapon`'s impulses and least ammunition.
    #[must_use]
    pub fn id() -> Self {
        let weapon = |bit, impulse, ammo, least, pickup: &str, value| Weapon {
            bit,
            impulse,
            ammo,
            least,
            pickup: pickup.as_bytes().to_vec(),
            value,
        };
        Self(vec![
            weapon(
                IT_ROCKET_LAUNCHER,
                7,
                Ammo::Rockets,
                1.0,
                "weapon_rocketlauncher",
                100.0,
            ),
            weapon(IT_LIGHTNING, 8, Ammo::Cells, 1.0, "weapon_lightning", 90.0),
            weapon(
                IT_SUPER_NAILGUN,
                5,
                Ammo::Nails,
                2.0,
                "weapon_supernailgun",
                70.0,
            ),
            weapon(
                IT_GRENADE_LAUNCHER,
                6,
                Ammo::Rockets,
                1.0,
                "weapon_grenadelauncher",
                60.0,
            ),
            weapon(
                IT_SUPER_SHOTGUN,
                3,
                Ammo::Shells,
                2.0,
                "weapon_supershotgun",
                50.0,
            ),
            weapon(IT_NAILGUN, 4, Ammo::Nails, 1.0, "weapon_nailgun", 40.0),
            weapon(IT_SHOTGUN, 2, Ammo::Shells, 1.0, "", 0.0),
            weapon(IT_AXE, 1, Ammo::None, 0.0, "", 0.0),
        ])
    }

    /// The weapon `body` should hold: the first it has with the ammunition it fires with
    /// (BOT-6.2).
    #[must_use]
    pub fn best(&self, body: &Body) -> Option<&Weapon> {
        self.0.iter().find(|weapon| weapon.ready(body))
    }

    /// The weapon whose pickup has `classname`.
    #[must_use]
    pub fn by_pickup(&self, classname: &[u8]) -> Option<&Weapon> {
        self.0
            .iter()
            .find(|weapon| !weapon.pickup.is_empty() && weapon.pickup == classname)
    }

    /// Whether `body` holds a weapon that uses `ammo`.
    #[must_use]
    pub fn uses(&self, body: &Body, ammo: Ammo) -> bool {
        self.0
            .iter()
            .any(|weapon| weapon.ammo == ammo && weapon.held(body))
    }
}
