// SPDX-License-Identifier: GPL-3.0-or-later

//! Bots for the preview host (L-87, ENG-18, ADR-0031), as `docs/spec/bots.md` describes them.
//!
//! A bot is a client slot with no connection whose moves come from here: each server frame,
//! [`Bots::drive`] gives each bot the server brought in (`addbot`, BOT-1) a [`Mind`], reads what
//! a client in its place could see, and sets its move. The mod's own player QuakeC plays the
//! bot, so bots work with any mod and the server stays id-exact. A mind routes along the map's
//! [`Graph`] (BOT-2), the playability check's moves of `qs-map-gen`, toward the goal it needs
//! most (BOT-4); fights the nearest foe it sees with an aim its skill sets (BOT-3, BOT-5);
//! holds the best weapon of a table a mod may replace (BOT-6); and presses fire to respawn
//! (BOT-3.3). Each bot draws its own random numbers (BOT-8).

mod aim;
mod goals;
mod mind;
mod rng;
#[cfg(test)]
mod tests;
mod weapons;

use std::collections::BTreeMap;

use qc_progs::Progs;
use qs_bsp::{Bsp, BspLoader};
use qs_map_gen::playable::{Ground, Moves, PlayError, Skill};
use qs_sim::scenario::Driver;
use qs_sim::spawn::Deathmatch;
use qs_sim::{BotChange, BotSkill, ClientId, SlotKind, World, WorldError};

pub use aim::Aim;
pub use goals::{choose, follow_need, item_need, weigh};
pub use mind::{Mind, Mode, View};
pub use qs_map_gen::playable::Graph;
pub use rng::BotRng;
pub use weapons::{Ammo, Weapon, Weapons};

/// The seed a host gives its bots when it has none of its own (BOT-8.1).
pub const DEFAULT_SEED: u64 = 1996;

/// Why a map's graph was not made.
#[derive(Debug)]
pub enum BotError {
    /// The BSP did not read.
    Bsp(String),
    /// The ground did not read from it.
    Ground(PlayError),
    /// A search passed the most places it visits.
    TooLarge,
}

impl core::fmt::Display for BotError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bsp(why) => write!(f, "the map did not read: {why}"),
            Self::Ground(why) => write!(f, "the map's ground did not read: {why}"),
            Self::TooLarge => f.write_str("the map has too many places for the bots' graph"),
        }
    }
}

impl core::error::Error for BotError {}

/// The bots of a game: a mind for each, the map's graph, the weapon table and the seed.
#[derive(Debug)]
pub struct Bots {
    minds: BTreeMap<ClientId, Mind>,
    graph: Option<Graph>,
    weapons: Weapons,
    seed: u64,
}

impl Default for Bots {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Bots {
    /// Bots that route along `graph`, or stand where they are with none (BOT-7.2), with id's
    /// weapons and [`DEFAULT_SEED`].
    #[must_use]
    pub fn new(graph: Option<Graph>) -> Self {
        Self {
            minds: BTreeMap::new(),
            graph,
            weapons: Weapons::id(),
            seed: DEFAULT_SEED,
        }
    }

    /// The bots with a mod's weapon table (BOT-6.1).
    #[must_use]
    pub fn with_weapons(mut self, weapons: Weapons) -> Self {
        self.weapons = weapons;
        self
    }

    /// The weapon table they hold their weapons by (BOT-6.1).
    #[must_use]
    pub const fn weapons(&self) -> &Weapons {
        &self.weapons
    }

    /// The bots with the host's seed (BOT-8.1).
    #[must_use]
    pub const fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// The graph the bots route along from now on (BOT-2).
    pub fn set_graph(&mut self, graph: Graph) {
        self.graph = Some(graph);
    }

    /// Whether they have a graph.
    #[must_use]
    pub const fn has_graph(&self) -> bool {
        self.graph.is_some()
    }

    /// The graph of the compiled map `bsp` (BOT-2.1), as a game with `deathmatch` keeps its
    /// entities, from its spawns, with a player's moves.
    ///
    /// # Errors
    ///
    /// [`BotError`] when the map does not read or is too large.
    pub fn graph_of(bsp: &[u8], deathmatch: Deathmatch) -> Result<Graph, BotError> {
        let bsp = Bsp::read(bsp, BspLoader::default()).map_err(|e| BotError::Bsp(e.to_string()))?;
        let ground = Ground::in_game(&bsp, Moves::Player, Skill::Normal, deathmatch)
            .map_err(BotError::Ground)?;
        ground.graph(ground.spawns()).ok_or(BotError::TooLarge)
    }

    /// The skill of the bot in slot `id`, `None` for a slot with no mind.
    #[must_use]
    pub fn skill(&self, id: ClientId) -> Option<BotSkill> {
        self.minds.get(&id).map(Mind::skill)
    }

    /// How many bots have a mind.
    #[must_use]
    pub fn len(&self) -> usize {
        self.minds.len()
    }

    /// Whether no bot has a mind.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.minds.is_empty()
    }

    /// Before a server frame of `frametime` seconds (BOT-9.1): a mind for each bot the server
    /// brought in, none for each it dropped or whose slot is no longer a bot's, and each bot's
    /// move from its mind.
    ///
    /// # Errors
    ///
    /// [`WorldError::Frame`] when a line of sight stops the engine.
    pub fn drive(
        &mut self,
        world: &mut World,
        progs: &Progs<'_>,
        frametime: f64,
    ) -> Result<(), WorldError> {
        for change in world.take_bot_changes() {
            match change {
                BotChange::Joined { id, skill } => {
                    self.minds.insert(id, Mind::new(id, skill, self.seed));
                }
                BotChange::Left(id) => {
                    self.minds.remove(&id);
                }
            }
        }
        // A bot dropped another way, as `kick` drops one, has no mind either.
        self.minds
            .retain(|&id, _| world.slot_kind(id) == Some(SlotKind::HostBot));
        if self.minds.is_empty() {
            return Ok(());
        }
        let bodies = world.bodies(progs);
        let mode = match world.setting("deathmatch") {
            Some(value) if value != 0.0 => Mode::Deathmatch,
            Some(_) | None => Mode::Coop,
        };
        let time = world.time();
        for (id, mind) in &mut self.minds {
            let Some(me) = bodies.iter().find(|b| b.number == id.edict()) else {
                continue;
            };
            let mut seen = Vec::new();
            for body in bodies.iter().filter(|b| mind::is_foe(me, b, mode)) {
                let Some((from, to)) = mind::in_sight(me, body) else {
                    continue;
                };
                let hit = world.trace_line_past(progs, from, to, me.number)?;
                let met = hit.edict.map(|e| e.index());
                if hit.point.is_none() || met == Some(body.number) {
                    seen.push(body.number);
                }
            }
            let view = View {
                me,
                bodies: &bodies,
                seen: &seen,
                mode,
                time,
                frametime,
            };
            let cmd = mind.decide(&view, self.graph.as_ref(), &self.weapons);
            world.set_command(*id, cmd);
        }
        Ok(())
    }
}

impl Driver for Bots {
    fn before_frame(
        &mut self,
        world: &mut World,
        progs: &Progs<'_>,
        frametime: f64,
    ) -> Result<(), WorldError> {
        self.drive(world, progs, frametime)
    }
}
