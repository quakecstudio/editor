// SPDX-License-Identifier: GPL-3.0-or-later

//! The mission of a level: what locks the way to the exit and where each lock is opened, as
//! Dormans' mission grammars and Unexplored's cyclic generator have it (PCG 2010; Boris the
//! Brave, "Dungeon Generation in Unexplored", 2021). Locks go forward on the path and what
//! opens them back, off a room before the lock, so every mission can be finished; loops join
//! rooms the same locks have opened, so none goes round a lock. The locks are id's
//! (`qcc/v101qc`): the silver and the gold key (`doors.qc`, `DOOR_SILVER_KEY` 16,
//! `DOOR_GOLD_KEY` 8), a button opening a door elsewhere (`buttons.qc`), and an arena whose
//! door opens when its monsters are dead (`monster_death_use` firing a `trigger_counter`). A
//! secret room behind a door that opens when shot (`func_door_secret`) holds a reward.

use crate::layout::{Doorway, LevelForm, Plan, RoomId};
use crate::rng::Rng;
use crate::{Classes, RoomShape, Size};

/// The classnames of the locks and of what opens them (`qcc/v101qc`).
pub(crate) const GOLD_KEY: &str = "item_key2";
pub(crate) const BUTTON: &str = "func_button";
pub(crate) const COUNTER: &str = "trigger_counter";
pub(crate) const SECRET_DOOR: &str = "func_door_secret";
pub(crate) const SECRET: &str = "trigger_secret";
pub(crate) const TELEPORT: &str = "trigger_teleport";
pub(crate) const DESTINATION: &str = "info_teleport_destination";

/// How many locks a small, a medium and a large level has.
const SMALL_LOCKS: usize = 1;
const MEDIUM_LOCKS: usize = 2;
const LARGE_LOCKS: usize = 3;

/// The shapes of a room off the path: a key's, a button's or a secret's. None has a pit, and
/// each has the middle of its walls clear for a button.
const OFF_PATH: &[RoomShape] = &[RoomShape::Hall, RoomShape::Vault];

/// A key, by its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyColour {
    /// The silver key, `item_key1`.
    Silver,
    /// The gold key, `item_key2`.
    Gold,
}

impl KeyColour {
    /// Its item's classname (`items.qc`).
    pub(crate) const fn classname(self) -> &'static str {
        match self {
            Self::Silver => crate::KEY,
            Self::Gold => GOLD_KEY,
        }
    }

    /// The `spawnflags` of a door it opens (`doors.qc`).
    pub(crate) const fn door_flags(self) -> &'static str {
        match self {
            Self::Silver => crate::DOOR_SILVER_KEY,
            Self::Gold => crate::DOOR_GOLD_KEY,
        }
    }
}

/// What opens a remote door, by number: the door's `targetname` and the opener's `target`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Signal(usize);

impl Signal {
    /// The name that joins the opener to its door.
    pub(crate) fn name(self) -> String {
        format!("gate{}", self.0)
    }
}

/// What a room is to the mission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// A room on the way.
    Way,
    /// The room a key lies in.
    Key(KeyColour),
    /// The room a button is in.
    Button(Signal),
    /// A room whose monsters, all dead, open its door on.
    Arena(Signal),
    /// A secret room.
    Secret,
    /// A room with a teleporter back to the start.
    Shortcut,
}

/// A kind of lock, as a level's report gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LockKind {
    /// A door the silver key opens.
    SilverKey,
    /// A door the gold key opens.
    GoldKey,
    /// A door a button elsewhere opens.
    Button,
    /// A door that opens when the monsters of the room before it are dead.
    Arena,
}

/// What a level's mission came to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// How many floors it is on.
    pub floors: usize,
    /// How many loops its passages make.
    pub loops: usize,
    /// Its locks, in the order they are met.
    pub locks: Vec<LockKind>,
    /// How many spaces are seen before they are reached: each atrium's balcony.
    pub teasers: usize,
    /// How many secret rooms it has.
    pub secrets: usize,
    /// How many teleporters take the player back to the start, one way.
    pub shortcuts: usize,
    /// The form the level takes besides its hook.
    pub form: LevelForm,
}

impl Size {
    /// How many locks a level of this size has.
    const fn locks(self) -> usize {
        match self {
            Self::Small => SMALL_LOCKS,
            Self::Medium => MEDIUM_LOCKS,
            Self::Large => LARGE_LOCKS,
        }
    }
}

impl Classes {
    /// The kinds of lock the game has the classes for.
    fn locks(&self) -> Vec<LockKind> {
        let door = self.has(crate::DOOR);
        let mut kinds = Vec::new();
        if door && self.has(crate::KEY) {
            kinds.push(LockKind::SilverKey);
        }
        if door && self.has(GOLD_KEY) {
            kinds.push(LockKind::GoldKey);
        }
        if door && self.has(BUTTON) {
            kinds.push(LockKind::Button);
        }
        if door && self.has(COUNTER) {
            kinds.push(LockKind::Arena);
        }
        kinds
    }

    /// Whether the game has what a teleporter needs.
    fn shortcuts(&self) -> bool {
        self.has(TELEPORT) && self.has(DESTINATION)
    }

    /// Whether the game has what a secret room needs.
    fn secrets(&self) -> bool {
        self.has(SECRET_DOOR) && self.has(SECRET)
    }
}

/// The mission being arranged on a plan.
pub(crate) struct Mission<'a> {
    plan: &'a mut Plan,
    signals: usize,
}

impl<'a> Mission<'a> {
    /// Arranges the mission of a level of `size` on `plan`, with the locks `classes` has:
    /// the locks spread along the path, what opens each off a room before it, a secret room,
    /// and loops. Returns the report.
    pub(crate) fn arrange(
        plan: &'a mut Plan,
        size: Size,
        classes: &Classes,
        rng: &mut Rng,
    ) -> Report {
        let mut mission = Self { plan, signals: 0 };
        let mut kinds = classes.locks();
        rng.shuffle(&mut kinds);
        let links = mission.plan.path.saturating_sub(1);
        let count = size.locks().min(kinds.len());
        let mut locks = Vec::new();
        let mut gates = Vec::new();
        for place in 0..count {
            // Spread along the path's passages, none on the first: the start has no monsters
            // and is no arena, and a key needs a room before its lock.
            let at = place
                .saturating_add(1)
                .saturating_mul(links)
                .checked_div(count.saturating_add(1))
                .unwrap_or(0)
                .max(1);
            if gates.contains(&at) || at >= links {
                continue;
            }
            // The first kind not yet used that can be put there.
            let Some(index) = (0..kinds.len()).find(|&index| {
                kinds
                    .get(index)
                    .is_some_and(|&kind| mission.lock(at, kind, rng).is_some())
            }) else {
                continue;
            };
            locks.push(kinds.remove(index));
            gates.push(at);
        }
        let secrets = usize::from(classes.secrets() && mission.secret(rng));
        let zones = mission.zones(&gates);
        mission.plan.close_loops(
            |a, b| {
                let zone = |id: RoomId| zones.get(id.place()).copied().flatten();
                zone(a).is_some() && zone(a) == zone(b)
            },
            rng,
        );
        mission.unhub();
        mission.plan.open_canyon();
        Report {
            floors: mission.plan.floors(),
            loops: mission.plan.loops(),
            locks,
            teasers: mission.plan.hooks.len(),
            secrets,
            shortcuts: usize::from(classes.shortcuts() && mission.shortcut(&gates)),
            form: mission.plan.form,
        }
    }

    /// Takes the hub form back from a level no key or button room of which hangs off the hub,
    /// as when its locks all come before the hub, are arenas, or no cell beside it was free: its
    /// great room is then one more great room, and the level is reported as the form it has.
    fn unhub(&mut self) {
        let Some(hub) = self.plan.hub else {
            return;
        };
        let opens = |role: Option<Role>| matches!(role, Some(Role::Key(_) | Role::Button(_)));
        let wing = self
            .plan
            .links
            .iter()
            .any(|link| link.from == hub && opens(self.role(link.to)));
        if !wing {
            self.plan.hub = None;
            self.plan.form = LevelForm::Hook;
        }
    }

    /// A new signal.
    fn signal(&mut self) -> Signal {
        let signal = Signal(self.signals);
        self.signals = self.signals.saturating_add(1);
        signal
    }

    /// The rooms before the path's passage `at`, which a lock there leaves open: the path's
    /// up to it and the rooms off those.
    fn before(&self, at: usize) -> Vec<RoomId> {
        let mut rooms: Vec<RoomId> = (0..=at).map(RoomId).collect();
        for link in &self.plan.links {
            if link.to.place() >= self.plan.path && rooms.contains(&link.from) {
                rooms.push(link.to);
            }
        }
        rooms
    }

    /// Adds a room of `role` off one of `parents`, as [`Plan::sprout`] does, off the hub first
    /// when it is one of them: a hub's wings hold its keys and buttons.
    fn sprout(&mut self, parents: &[RoomId], role: Role, rng: &mut Rng) -> Option<RoomId> {
        if let Some(hub) = self.plan.hub.filter(|hub| parents.contains(hub))
            && let Some(room) = self.plan.sprout(&[hub], OFF_PATH, role, Doorway::Arch, rng)
        {
            return Some(room);
        }
        self.plan
            .sprout(parents, OFF_PATH, role, Doorway::Arch, rng)
    }

    /// Locks the path's passage `at` with a lock of `kind`, what opens it put before it;
    /// the kind it came to, or `None` when it could not be put there.
    fn lock(&mut self, at: usize, kind: LockKind, rng: &mut Rng) -> Option<LockKind> {
        let exit = self.plan.path.saturating_sub(1);
        let parents: Vec<RoomId> = self
            .before(at)
            .into_iter()
            .filter(|room| room.place() != exit && self.role(*room) == Some(Role::Way))
            .collect();
        let doorway = match kind {
            LockKind::SilverKey | LockKind::GoldKey => {
                let colour = if kind == LockKind::SilverKey {
                    KeyColour::Silver
                } else {
                    KeyColour::Gold
                };
                self.sprout(&parents, Role::Key(colour), rng)?;
                Doorway::Locked(colour)
            }
            LockKind::Button => {
                let signal = self.signal();
                self.sprout(&parents, Role::Button(signal), rng)?;
                Doorway::Remote(signal)
            }
            LockKind::Arena => {
                let room = self.plan.rooms.get(at)?;
                // Not a pit nor a lake, whose monsters fall in, an atrium, whose door is its
                // balcony's, nor a junction, a corridor's turn with no room for monsters.
                let fit = room.role == Role::Way
                    && !matches!(
                        room.shape,
                        RoomShape::Pit | RoomShape::Atrium | RoomShape::Junction | RoomShape::Lake
                    );
                if !fit {
                    return None;
                }
                let signal = self.signal();
                self.plan.rooms.get_mut(at)?.role = Role::Arena(signal);
                Doorway::Remote(signal)
            }
        };
        self.plan.links.get_mut(at)?.doorway = doorway;
        Some(kind)
    }

    /// Adds a secret room off a room of the path; whether one was added.
    fn secret(&mut self, rng: &mut Rng) -> bool {
        let exit = self.plan.path.saturating_sub(1);
        let parents: Vec<RoomId> = (0..exit)
            .map(RoomId)
            .filter(|room| self.role(*room) == Some(Role::Way))
            .collect();
        self.plan
            .sprout(&parents, OFF_PATH, Role::Secret, Doorway::Secret, rng)
            .is_some()
    }

    /// Puts a teleporter back to the start in the room before the exit, once a lock is
    /// behind it, as E1M1's way back is: one way, so no lock is gone round. Whether it was
    /// put there.
    fn shortcut(&mut self, gates: &[usize]) -> bool {
        let before_exit = self.plan.path.saturating_sub(2);
        let behind_a_lock = gates.iter().any(|&gate| gate < before_exit);
        let Some(room) = self.plan.rooms.get_mut(before_exit) else {
            return false;
        };
        let fit = !matches!(room.shape, RoomShape::Atrium | RoomShape::Junction);
        if !behind_a_lock || room.role != Role::Way || !fit {
            return false;
        }
        room.role = Role::Shortcut;
        true
    }

    /// The role of `room`.
    fn role(&self, room: RoomId) -> Option<Role> {
        self.plan.room_of(room).map(|plan| plan.role)
    }

    /// Each room's zone: how many of the locks at the path's passages `gates` are passed to
    /// reach it; `None` for a secret room, which no loop joins.
    fn zones(&self, gates: &[usize]) -> Vec<Option<usize>> {
        let mut zones: Vec<Option<usize>> = (0..self.plan.rooms.len())
            .map(|place| {
                (place < self.plan.path).then(|| gates.iter().filter(|&&gate| gate < place).count())
            })
            .collect();
        for link in &self.plan.links {
            let off = link.to.place() >= self.plan.path;
            let secret = self.role(link.to) == Some(Role::Secret);
            if off && !secret {
                let zone = zones.get(link.from.place()).copied().flatten();
                if let Some(slot) = zones.get_mut(link.to.place()) {
                    *slot = zone;
                }
            }
        }
        zones
    }
}
