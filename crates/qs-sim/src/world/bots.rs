// SPDX-License-Identifier: GPL-3.0-or-later

//! The bots the host drives (BOT-1, `docs/spec/bots.md`): client slots with no connection
//! whose moves the host sends, brought in and dropped by the console's `addbot` and
//! `removebot`, and what a bot's mind reads of the world each frame. The mind itself is the
//! host's (`qs-bots`); the server runs a bot as any other client, so the mod's own player
//! QuakeC plays it.

use qc_progs::Progs;

use super::{Signon, World, WorldError};
use crate::builtins::SimCall;
use crate::clients::{ClientId, SlotKind};
use crate::host_commands::{ClientStage, Words, atoi};
use crate::offsets::FieldOffset;

/// The names `addbot` gives a bot that is not named, the first not taken (BOT-1.3).
pub const BOT_NAMES: [&str; 16] = [
    "Ash", "Birch", "Cedar", "Dune", "Ember", "Flint", "Gale", "Heron", "Iron", "Jade", "Kestrel",
    "Lumen", "Moss", "Nettle", "Onyx", "Pike",
];

/// The shirt and pants colours of each slot's bot, 0 to 13 as `color` takes them (BOT-1.3).
const BOT_COLOURS: [(i32, i32); 16] = [
    (4, 4),
    (13, 13),
    (12, 12),
    (3, 3),
    (6, 6),
    (10, 10),
    (2, 2),
    (11, 11),
    (9, 9),
    (5, 5),
    (7, 7),
    (8, 8),
    (1, 1),
    (0, 0),
    (4, 13),
    (12, 3),
];

/// The longest name a client has, `name`'s 15 bytes (PHYS-13.4).
const NAME_LENGTH: usize = 15;

/// What `from_setting` adds before it rounds down, to round to the nearest.
const ROUND_HALF: f32 = 0.5;
/// The highest skill's number.
const NIGHTMARE_LEVEL: u8 = 3;

/// How well a bot plays, 0 to 3 as the `skill` setting goes (BOT-1.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BotSkill {
    /// 0.
    Easy,
    /// 1.
    #[default]
    Normal,
    /// 2.
    Hard,
    /// 3.
    Nightmare,
}

impl BotSkill {
    /// Every skill, from the easiest.
    pub const ALL: [Self; 4] = [Self::Easy, Self::Normal, Self::Hard, Self::Nightmare];

    /// The skill a number names (BOT-1.4): rounded to the nearest whole number and kept to 0
    /// to 3, as the server reads the `skill` setting (DIFF-3.2).
    #[must_use]
    pub fn from_setting(value: f32) -> Self {
        let rounded = (value + ROUND_HALF).floor();
        Self::ALL
            .into_iter()
            .rev()
            .find(|skill| rounded >= f32::from(skill.level()))
            .unwrap_or(Self::Easy)
    }

    /// The skill's number, 0 to 3.
    #[must_use]
    pub const fn level(self) -> u8 {
        match self {
            Self::Easy => 0,
            Self::Normal => 1,
            Self::Hard => 2,
            Self::Nightmare => NIGHTMARE_LEVEL,
        }
    }
}

/// A bot that came or went, for the host to give its mind to or take it from (BOT-1.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BotChange {
    /// A bot entered the game in slot `id`, at `skill`.
    Joined {
        /// Its slot.
        id: ClientId,
        /// How well it plays.
        skill: BotSkill,
    },
    /// The bot of slot `id` was dropped.
    Left(ClientId),
}

/// An edict as a bot's mind reads it (BOT-1.6): its classname and the system fields a client
/// in its place could make out, each as the QuakeC holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    /// Its number.
    pub number: usize,
    /// Its `classname`, empty when it has none.
    pub classname: Vec<u8>,
    /// `origin`.
    pub origin: [f32; 3],
    /// `velocity`.
    pub velocity: [f32; 3],
    /// `absmin` and `absmax`: its box in the world.
    pub absmin: [f32; 3],
    /// `absmax`.
    pub absmax: [f32; 3],
    /// `view_ofs`: its eye above its origin.
    pub view_ofs: [f32; 3],
    /// `v_angle`: where a client looks.
    pub v_angle: [f32; 3],
    /// `health`.
    pub health: f32,
    /// `armorvalue`.
    pub armorvalue: f32,
    /// `items`, the bits of what it holds.
    pub items: f32,
    /// `weapon`, the bit of the weapon in hand.
    pub weapon: f32,
    /// `ammo_shells`, `ammo_nails`, `ammo_rockets` and `ammo_cells`.
    pub ammo: [f32; 4],
    /// `deadflag`: 0 while alive.
    pub deadflag: f32,
    /// `flags`.
    pub flags: f32,
    /// `solid`.
    pub solid: f32,
    /// `takedamage`: 0 for what cannot be hurt.
    pub takedamage: f32,
    /// `frags`.
    pub frags: f32,
    /// `team`.
    pub team: f32,
    /// `modelindex`: 0 for nothing drawn.
    pub modelindex: f32,
    /// `waterlevel`: 0 out of water to 3 under it.
    pub waterlevel: f32,
}

impl World {
    /// `addbot`'s bot (BOT-1.2): the first slot not active connected as a bot the host drives,
    /// named `name` or the first of [`BOT_NAMES`] no active slot has (BOT-1.3), its colours its
    /// slot's, then brought in as a local client is: `name` and `color`, `prespawn`, `spawn`
    /// and `begin`, so the mod's `ClientConnect` and `PutClientInServer` make its player.
    /// Returns its slot, `None` when every slot is active.
    ///
    /// # Errors
    ///
    /// [`WorldError::Frame`] when a function fails.
    pub fn add_bot(
        &mut self,
        progs: &Progs<'_>,
        skill: BotSkill,
        name: Option<&[u8]>,
    ) -> Result<Option<ClientId>, WorldError> {
        let Some(id) = self.builtins.messages.clients.free() else {
            return Ok(None);
        };
        let name = self.bot_name(name);
        let (top, bottom) = BOT_COLOURS.get(id.slot()).copied().unwrap_or_default();
        self.signon_connect(progs, id, SlotKind::HostBot)?;
        let mut named = b"name \"".to_vec();
        named.extend_from_slice(&name);
        named.extend_from_slice(b"\"\n");
        for text in [named, format!("color {top} {bottom}\n").into_bytes()] {
            SimCall::frame(&mut self.machine, progs, &mut self.builtins)
                .client_command(id.edict(), &text, ClientStage::Connecting)
                .map_err(WorldError::Frame)?;
        }
        for stage in [Signon::Prespawn, Signon::Spawn, Signon::Begin] {
            self.signon(progs, id, stage)?;
            // PHYS-10.8: the spawn stage's frame ends with `SV_UpdateToReliableMessages`.
            if stage == Signon::Spawn {
                SimCall::frame(&mut self.machine, progs, &mut self.builtins)
                    .update_to_reliable_messages();
            }
        }
        self.bot_changes.push(BotChange::Joined { id, skill });
        Ok(Some(id))
    }

    /// `removebot`'s bot (BOT-1.2): the active bot the host drives named `name`, ASCII case
    /// apart, or with no name the one in the highest slot, dropped as `SV_DropClient` drops a
    /// client (NET-7.9). Returns its slot, `None` when there is no such bot.
    ///
    /// # Errors
    ///
    /// [`WorldError::Frame`] when its `ClientDisconnect` fails.
    pub fn remove_bot(
        &mut self,
        progs: &Progs<'_>,
        name: Option<&[u8]>,
    ) -> Result<Option<ClientId>, WorldError> {
        let found = self
            .builtins
            .messages
            .clients
            .iter()
            .filter(|(_, slot)| slot.active && slot.kind == SlotKind::HostBot)
            .filter(|(_, slot)| name.is_none_or(|name| slot.name.eq_ignore_ascii_case(name)))
            .map(|(id, _)| id)
            .last();
        let Some(id) = found else {
            return Ok(None);
        };
        self.disconnect(progs, id)?;
        self.bot_changes.push(BotChange::Left(id));
        Ok(Some(id))
    }

    /// The bots that came and went since the last call, in order (BOT-1.5).
    pub fn take_bot_changes(&mut self) -> Vec<BotChange> {
        core::mem::take(&mut self.bot_changes)
    }

    /// Each active slot the host drives, first to last.
    #[must_use]
    pub fn host_bots(&self) -> Vec<ClientId> {
        self.builtins
            .messages
            .clients
            .iter()
            .filter(|(_, slot)| slot.active && slot.kind == SlotKind::HostBot)
            .map(|(id, _)| id)
            .collect()
    }

    /// Slot `id`'s name, `None` for a slot not active.
    #[must_use]
    pub fn client_name(&self, id: ClientId) -> Option<Vec<u8>> {
        self.builtins
            .messages
            .clients
            .get(id)
            .filter(|slot| slot.active)
            .map(|slot| slot.name.clone())
    }

    /// What joins slot `id`, `None` for a slot not active.
    #[must_use]
    pub fn slot_kind(&self, id: ClientId) -> Option<SlotKind> {
        self.builtins
            .messages
            .clients
            .get(id)
            .filter(|slot| slot.active)
            .map(|slot| slot.kind)
    }

    /// Every edict in use as a bot's mind reads it, the world first (BOT-1.6).
    #[must_use]
    pub fn bodies(&self, progs: &Progs<'_>) -> Vec<Body> {
        let float = |n: usize, name: &str| self.field(n, name).as_f32();
        let vector = |n: usize, name: &str| {
            let first = super::field_slot(name);
            [0, 1, 2].map(|i| {
                self.slot_of(n, FieldOffset::new(first.saturating_add(i)))
                    .as_f32()
            })
        };
        (0..self.num_edicts())
            .filter(|&n| self.in_use(n))
            .map(|n| Body {
                number: n,
                classname: self
                    .string_value(progs, self.field(n, "classname"))
                    .unwrap_or_default(),
                origin: vector(n, "origin"),
                velocity: vector(n, "velocity"),
                absmin: vector(n, "absmin"),
                absmax: vector(n, "absmax"),
                view_ofs: vector(n, "view_ofs"),
                v_angle: vector(n, "v_angle"),
                health: float(n, "health"),
                armorvalue: float(n, "armorvalue"),
                items: float(n, "items"),
                weapon: float(n, "weapon"),
                ammo: [
                    float(n, "ammo_shells"),
                    float(n, "ammo_nails"),
                    float(n, "ammo_rockets"),
                    float(n, "ammo_cells"),
                ],
                deadflag: float(n, "deadflag"),
                flags: float(n, "flags"),
                solid: float(n, "solid"),
                takedamage: float(n, "takedamage"),
                frags: float(n, "frags"),
                team: float(n, "team"),
                modelindex: float(n, "modelindex"),
                waterlevel: float(n, "waterlevel"),
            })
            .collect()
    }

    /// The name a new bot is given (BOT-1.3): `name` with its quotes and control bytes left
    /// out, cut to 15 bytes; with none, or one that is left empty, the first of [`BOT_NAMES`]
    /// no active slot has, ASCII case apart, else `bot` and its slot's number from 1.
    fn bot_name(&self, name: Option<&[u8]>) -> Vec<u8> {
        let given: Vec<u8> = name
            .unwrap_or_default()
            .iter()
            .copied()
            .filter(|&c| c != b'"' && c >= b' ')
            .take(NAME_LENGTH)
            .collect();
        if !given.is_empty() {
            return given;
        }
        let clients = &self.builtins.messages.clients;
        let taken = |name: &str| {
            clients
                .iter()
                .any(|(_, slot)| slot.active && slot.name.eq_ignore_ascii_case(name.as_bytes()))
        };
        if let Some(free) = BOT_NAMES.iter().find(|name| !taken(name)) {
            return free.as_bytes().to_vec();
        }
        let next = clients.free().map_or(0, |id| id.slot().saturating_add(1));
        format!("bot{next}").into_bytes()
    }

    /// The console's `addbot [skill] [name]` (BOT-1.2): a first argument that starts with a
    /// digit is the skill, `atoi` of it kept to 0 to 3, and the next the name; any other first
    /// argument is the name. With no skill the bot plays at the `skill` setting.
    pub(crate) fn addbot_command(&mut self, progs: &Progs<'_>, words: &Words) {
        let first = words.argv.get(1);
        let skill_given = first
            .and_then(|w| w.first())
            .is_some_and(u8::is_ascii_digit);
        let (skill, name) = if skill_given {
            let level = first.map_or(0, |w| atoi(w));
            (
                BotSkill::from_setting(f32::from(i16::try_from(level).unwrap_or(i16::MAX))),
                words.argv.get(2),
            )
        } else {
            let setting = self.setting("skill").unwrap_or(0.0);
            (BotSkill::from_setting(setting), first)
        };
        match self.add_bot(progs, skill, name.map(Vec::as_slice)) {
            Ok(Some(_)) => {}
            Ok(None) => {
                let max = self.maxplayers().get();
                self.print_line(format!("addbot: every client slot is taken ({max} max)\n"));
            }
            Err(error) => self.print_line(format!("{error}\n")),
        }
    }

    /// The console's `removebot [name]` (BOT-1.2).
    pub(crate) fn removebot_command(&mut self, progs: &Progs<'_>, words: &Words) {
        let name = words.argv.get(1).map(Vec::as_slice);
        match self.remove_bot(progs, name) {
            Ok(Some(_)) => {}
            Ok(None) => match name {
                Some(name) => {
                    let mut line = b"removebot: no bot named ".to_vec();
                    line.extend_from_slice(name);
                    line.push(b'\n');
                    self.print_line(line);
                }
                None => self.print_line("removebot: no bots\n"),
            },
            Err(error) => self.print_line(format!("{error}\n")),
        }
    }

    /// `Con_Printf` of a line to the host's console.
    fn print_line(&mut self, text: impl AsRef<[u8]>) {
        self.builtins.console_out.extend_from_slice(text.as_ref());
    }
}
