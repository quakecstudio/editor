// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2016      Spike
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/net_main.c, MaxPlayers_f
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/host.c, Host_FindMaxClients, SV_DropClient
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sv_main.c, SV_ConnectClient, SV_CheckForNewClients
// Ported from Novum/vkQuake@27789d7333a46674474d871137bb71e056719628: Quake/pr_ext.c, PF_spawnclient, PF_dropclient, PF_clienttype

//! The server's client slots, `svs.clients` (NET-7): how many there are, which one a client
//! is, and what the server keeps for each, `client_t`.

use qc_edict::EdictNumber;
use qc_types::Slot;
use qc_vm::RunError;
use qc_vm::convert::{double_to_float, float_to_int, int_to_float};

use crate::builtins::{PARM0, SimCall};
use crate::client::UserCmd;
use crate::host_commands::COLOR_MASK;
use crate::messages::{
    Message, NET_MAX_MSGLEN, Overflow, SVC_DISCONNECT, SVC_UPDATECOLORS, SVC_UPDATEFRAGS,
    SVC_UPDATENAME,
};

/// `MAX_SCOREBOARD` (`WinQuake/quakedef.h`), the most client slots a server has (NET-7.1).
pub const MAX_SCOREBOARD: usize = 16;
/// `NUM_SPAWN_PARMS` (`WinQuake/server.h`): `parm1` to `parm16`.
pub(crate) const NUM_SPAWN_PARMS: usize = 16;
/// The name `SV_ConnectClient` gives a slot, which a client's `name` then replaces.
pub(crate) const UNCONNECTED: &[u8] = b"unconnected";
/// `NUM_PING_TIMES` (`WinQuake/server.h`): the pings `ping` averages.
const NUM_PING_TIMES: usize = 16;
/// `NUM_PING_TIMES` as the float `total /= NUM_PING_TIMES` divides by.
const PING_COUNT: f32 = 16.0;
/// Milliseconds in a second, as `ping` prints a ping time.
const MS_PER_SECOND: f32 = 1000.0;
/// The `old_frags` `SV_DropClient` leaves, so the next frag count is sent whatever it is.
pub(crate) const DROPPED_FRAGS: i32 = -999_999;

/// A client slot: 0 for the first, whose edict is 1 (EDICT-5.1, NET-7.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientId(usize);

impl ClientId {
    /// The first slot, the host's own player.
    pub const FIRST: Self = Self(0);

    /// Slot `n`, from 0; `None` from [`MAX_SCOREBOARD`] up.
    #[must_use]
    pub const fn new(n: usize) -> Option<Self> {
        if n < MAX_SCOREBOARD {
            Some(Self(n))
        } else {
            None
        }
    }

    /// The slot client edict `n` is, `NUM_FOR_EDICT (ent) - 1`; `None` for the world and
    /// from edict [`MAX_SCOREBOARD`] + 1 up.
    #[must_use]
    pub const fn of_edict(n: usize) -> Option<Self> {
        match n.checked_sub(1) {
            Some(slot) => Self::new(slot),
            None => None,
        }
    }

    /// The slot's number, `host_client - svs.clients`, which the messages name it by.
    #[must_use]
    pub const fn slot(self) -> usize {
        self.0
    }

    /// The slot's edict, `EDICT_NUM (clientnum + 1)`.
    #[must_use]
    pub const fn edict(self) -> usize {
        self.0.saturating_add(1)
    }

    /// The slot's number as the byte a message carries.
    pub(crate) fn byte(self) -> i32 {
        i32::try_from(self.0).unwrap_or(0)
    }
}

/// How many client slots the server has, `svs.maxclients` (NET-7.1): 1 to
/// [`MAX_SCOREBOARD`], 1 by default as `Host_FindMaxClients` starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MaxPlayers(usize);

impl MaxPlayers {
    /// One slot: single player.
    pub const SINGLE: Self = Self(1);

    /// `n` slots as `MaxPlayers_f` takes them: below 1 is 1, above [`MAX_SCOREBOARD`] is
    /// [`MAX_SCOREBOARD`] (NET-7.1).
    #[must_use]
    pub fn new(n: i32) -> Self {
        let n = usize::try_from(n).unwrap_or(0);
        Self(n.clamp(1, MAX_SCOREBOARD))
    }

    /// The number of slots.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }

    /// Whether the server has more than one slot, `svs.maxclients > 1`, which never pauses
    /// for the console (PHYS-11.9) and refuses a saved game (PHYS-10.5).
    #[must_use]
    pub const fn is_multiplayer(self) -> bool {
        self.0 > 1
    }

    /// Each slot, first to last.
    pub fn ids(self) -> impl Iterator<Item = ClientId> {
        (0..self.0).map(ClientId)
    }
}

impl Default for MaxPlayers {
    fn default() -> Self {
        Self::SINGLE
    }
}

/// What joins a client to its slot (NET-7.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SlotKind {
    /// A client in this process, joined through the loopback connection: the host's own
    /// player and any other on the same machine.
    #[default]
    Local,
    /// A bot `spawnclient` made, with no connection (BUILTIN-18.45): nothing is sent to it,
    /// and its view angles are its edict's `v_angle`.
    Bot,
    /// A bot the host drives, `addbot` made (BOT-1.1): no connection, as a [`SlotKind::Bot`],
    /// but its moves are the host's, each read as a local client's is.
    HostBot,
}

/// A client slot, `client_t` (NET-1.6, NET-2, NET-7).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClientSlot {
    pub(crate) active: bool,
    pub(crate) spawned: bool,
    pub(crate) sendsignon: bool,
    pub(crate) kind: SlotKind,
    /// `host_client->name`, which `spawn` gives its `netname` (PHYS-10.1): empty for a slot
    /// never connected, as `svs.clients` starts zeroed.
    pub(crate) name: Vec<u8>,
    /// `host_client->colors`, `top * 16 + bottom`, whose bottom `spawn` gives its `team`
    /// (PHYS-10.1).
    pub(crate) colors: i32,
    pub(crate) old_frags: i32,
    /// Its reliable message, which may overflow (BUILTIN-13.4).
    pub(crate) message: Message,
    /// The packets sent to it, oldest first, until the host takes them.
    pub(crate) outbox: Vec<Vec<u8>>,
    /// The last `svc_setangle` sent to it, as its client reads it (NET-4.3).
    pub(crate) set_angle: Option<[f32; 3]>,
    /// `host_client->cmd`: its move, which a frame thinks once it has spawned (PHYS-9.6).
    pub(crate) cmd: UserCmd,
    /// The string commands it sent since the last frame, which the frame reads first
    /// (PHYS-11.1).
    pub(crate) commands: Vec<Vec<u8>>,
    /// Whether it connects in the next frame, at `SV_CheckForNewClients` (PHYS-10.7).
    pub(crate) connecting: bool,
    /// `host_client->spawn_parms`, which `spawn` and `setspawnparms` copy to `parm1` to
    /// `parm16` (PHYS-10.1, BUILTIN-8.7).
    pub(crate) spawn_parms: [Slot; NUM_SPAWN_PARMS],
    /// `ping_times` and `num_pings`: each move's `sv.time` less the time of the last datagram
    /// its client had read, the last 16 kept (NET-7.6).
    pub(crate) ping_times: [f32; NUM_PING_TIMES],
    pub(crate) num_pings: usize,
    /// The `svc_time` of the last datagram sent to it, which its next move carries back as
    /// `cl.mtime[0]` over the loopback connection.
    pub(crate) last_time: f32,
}

impl Default for ClientSlot {
    fn default() -> Self {
        Self {
            active: false,
            spawned: false,
            sendsignon: false,
            kind: SlotKind::Local,
            name: Vec::new(),
            colors: 0,
            old_frags: 0,
            message: Message::new(NET_MAX_MSGLEN, Overflow::Allowed),
            outbox: Vec::new(),
            set_angle: None,
            cmd: UserCmd::default(),
            commands: Vec::new(),
            connecting: false,
            spawn_parms: [Slot::ZERO; NUM_SPAWN_PARMS],
            ping_times: [0.0; NUM_PING_TIMES],
            num_pings: 0,
            last_time: 0.0,
        }
    }
}

impl ClientSlot {
    /// `SV_SendClientMessages` sending its message (NET-1.6): a local client's becomes a
    /// packet, and a bot's is dropped, as it has no one to send it to (NET-7.3).
    pub(crate) fn send_message(&mut self) {
        let packet = self.message.take();
        if self.kind == SlotKind::Local {
            self.outbox.push(packet);
        }
    }

    /// `SV_ReadClientMove`'s ping (NET-7.6): `sv.time` now less the time its move carries
    /// back, `sv.time - MSG_ReadFloat ()` in double and stored as a float, into
    /// `ping_times[num_pings % 16]`.
    pub(crate) fn record_ping(&mut self, now: f64) {
        if let Some(ping) = self.ping_times.get_mut(self.num_pings % NUM_PING_TIMES) {
            *ping = double_to_float(now - f64::from(self.last_time));
        }
        self.num_pings = self.num_pings.wrapping_add(1);
    }

    /// `Host_Ping_f`'s figure (NET-7.6): the 16 ping times summed in order, divided by 16, in
    /// milliseconds, truncated as `(int)` truncates.
    pub(crate) fn mean_ping_ms(&self) -> i32 {
        let mut total = 0.0_f32;
        for ping in self.ping_times {
            total += ping;
        }
        float_to_int(total / PING_COUNT * MS_PER_SECOND)
    }

    /// A packet of its own sent to it: a local client's datagram (NET-1.5).
    pub(crate) fn send_packet(&mut self, packet: Vec<u8>) {
        if self.kind == SlotKind::Local {
            self.outbox.push(packet);
        }
    }
}

/// Where a connecting client's spawn parms come from (PHYS-10.4, PHYS-10.5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ConnectParms {
    /// `SetNewParms`': a new game, or a client new to it.
    New,
    /// What the client carries from the level it left.
    Carried([Slot; NUM_SPAWN_PARMS]),
    /// What the slot holds, a saved game's (`sv.loadgame`).
    Saved,
}

/// `CLIENTTYPE_BOT`, `clienttype`'s answer for a client with no connection (BUILTIN-18.20).
const CLIENTTYPE_BOT: f32 = 2.0;

/// Every client slot, `svs.clients` (NET-7.2).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Clients {
    slots: Vec<ClientSlot>,
}

impl Default for Clients {
    fn default() -> Self {
        Self::new(MaxPlayers::SINGLE)
    }
}

impl Clients {
    /// `max` empty slots.
    pub(crate) fn new(max: MaxPlayers) -> Self {
        Self {
            slots: vec![ClientSlot::default(); max.get()],
        }
    }

    /// How many slots there are, `svs.maxclients`.
    pub(crate) fn max(&self) -> MaxPlayers {
        MaxPlayers(self.slots.len())
    }

    /// Slot `id`, `None` past the last.
    pub(crate) fn get(&self, id: ClientId) -> Option<&ClientSlot> {
        self.slots.get(id.slot())
    }

    /// Slot `id` to change, `None` past the last.
    pub(crate) fn get_mut(&mut self, id: ClientId) -> Option<&mut ClientSlot> {
        self.slots.get_mut(id.slot())
    }

    /// The slot of client edict `n`, `None` for an edict that is no client slot.
    pub(crate) fn of_edict_mut(&mut self, n: usize) -> Option<&mut ClientSlot> {
        ClientId::of_edict(n).and_then(|id| self.get_mut(id))
    }

    /// Each slot with its id, first to last.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (ClientId, &ClientSlot)> {
        self.slots.iter().enumerate().map(|(i, s)| (ClientId(i), s))
    }

    /// Each slot to change, first to last.
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut ClientSlot> {
        self.slots.iter_mut()
    }

    /// Whether slot `id` is active.
    pub(crate) fn is_active(&self, id: ClientId) -> bool {
        self.get(id).is_some_and(|slot| slot.active)
    }

    /// Each slot's `active`, as the mock engine keeps them (`clients_active`).
    pub(crate) fn active(&self) -> Vec<bool> {
        self.slots.iter().map(|slot| slot.active).collect()
    }

    /// How many slots are active, `net_activeconnections`.
    pub(crate) fn active_count(&self) -> usize {
        self.slots.iter().filter(|slot| slot.active).count()
    }

    /// `SV_CheckForNewClients`' choice (NET-7.4): the first slot not active, `None` when
    /// every one is.
    pub(crate) fn free(&self) -> Option<ClientId> {
        self.slots
            .iter()
            .position(|slot| !slot.active)
            .map(ClientId)
    }

    /// `PF_spawnclient`'s choice (BUILTIN-18.45): the last slot not active.
    pub(crate) fn free_from_last(&self) -> Option<ClientId> {
        self.slots
            .iter()
            .rposition(|slot| !slot.active)
            .map(ClientId)
    }
}

impl SimCall<'_, '_, '_> {
    /// `SV_ConnectClient` (PHYS-10.7, NET-7.4): slot `id` starts over as a client of `kind`,
    /// active and not spawned, the serverinfo written to it, and its spawn parms `carried`
    /// when it brings some from the last level (PHYS-10.4), the saved ones in a loaded game
    /// (PHYS-10.5), else `SetNewParms`'.
    pub(crate) fn connect_client(
        &mut self,
        id: ClientId,
        kind: SlotKind,
        parms: ConnectParms,
    ) -> Result<(), RunError> {
        if let Some(active) = self.sim.mock.engine_mut().clients_active.get_mut(id.slot()) {
            *active = true;
        }
        let message = self.sim.messages.new_client_message();
        let Some(slot) = self.sim.messages.clients.get_mut(id) else {
            return Ok(());
        };
        // `memset (client, 0, sizeof(*client))` and `strcpy (client->name, "unconnected")`:
        // the commands the new connection brought stay.
        let commands = core::mem::take(&mut slot.commands);
        let saved = slot.spawn_parms;
        *slot = ClientSlot {
            active: true,
            kind,
            commands,
            message,
            name: UNCONNECTED.to_vec(),
            ..ClientSlot::default()
        };
        // NET-2.2: the serverinfo, sent at the end of the frame the stage ran in.
        self.send_serverinfo(id);
        self.flush_signon(id);
        let parms = match parms {
            ConnectParms::Saved => saved,
            ConnectParms::Carried(carried) => carried,
            ConnectParms::New => {
                let new_parms = self.global("SetNewParms").as_i32();
                self.run(new_parms)?;
                core::array::from_fn(|n| self.global(&format!("parm{}", n.saturating_add(1))))
            }
        };
        if let Some(slot) = self.sim.messages.clients.get_mut(id) {
            slot.spawn_parms = parms;
        }
        Ok(())
    }

    /// `spawnclient()`, vkQuake's `PF_spawnclient` (BUILTIN-18.45): from the last slot down,
    /// the first not active is connected as a bot with no connection, marked spawned, and its
    /// edict cleared as `Host_Spawn_f` clears one, then returned; with every slot active, the
    /// world.
    pub(crate) fn spawn_client(&mut self) -> Result<(), RunError> {
        let Some(id) = self.sim.messages.clients.free_from_last() else {
            return self.set_return_entity(0);
        };
        // `SV_ConnectClient`'s `if (sv.loadgame)`.
        let parms = if self.sim.loaded {
            ConnectParms::Saved
        } else {
            ConnectParms::New
        };
        self.connect_client(id, SlotKind::Bot, parms)?;
        let Some(slot) = self.sim.messages.clients.get_mut(id) else {
            return self.set_return_entity(0);
        };
        slot.spawned = true;
        let (name, colors) = (slot.name.clone(), slot.colors);
        let name = self.sim.mock.engine_mut().strings.make(name);
        self.clear_client_edict(id, name, colors);
        self.set_return_entity(id.edict())
    }

    /// `dropclient(e)`, vkQuake's `PF_dropclient` (BUILTIN-18.46): an active client slot's
    /// edict dropped as `SV_DropClient (false)` drops it; anything else is passed over.
    pub(crate) fn drop_client_builtin(&mut self) -> Result<(), RunError> {
        let n = self.edict_of(self.context.parm_raw(PARM0));
        match ClientId::of_edict(n) {
            Some(id) => self.drop_client(id),
            None => Ok(()),
        }
    }

    /// `clienttype(e)`'s answer for a bot, once the mock has answered (BUILTIN-18.20): 2,
    /// `CLIENTTYPE_BOT`, for an active slot with no connection, where the mock's every
    /// client is a real one.
    pub(crate) fn mark_bot_type(&mut self) -> Result<(), RunError> {
        let n = self.edict_of(self.context.parm_raw(PARM0));
        let bot = ClientId::of_edict(n)
            .and_then(|id| self.sim.messages.clients.get(id))
            .is_some_and(|slot| slot.active && slot.kind == SlotKind::Bot);
        if bot {
            return self.set_return(CLIENTTYPE_BOT);
        }
        Ok(())
    }

    /// `setspawnparms(e)` once the mock has checked `e` is a client slot (BUILTIN-8.7): the
    /// slot's 16 spawn parms copied to `parm1` to `parm16`.
    pub(crate) fn set_spawn_parms(&mut self) {
        let n = self.edict_of(self.context.parm_raw(PARM0));
        let Some(parms) = ClientId::of_edict(n)
            .and_then(|id| self.sim.messages.clients.get(id))
            .map(|slot| slot.spawn_parms)
        else {
            return;
        };
        for (n, parm) in (1..=NUM_SPAWN_PARMS).zip(parms) {
            self.set_global(&format!("parm{n}"), parm);
        }
    }
    /// `SV_DropClient (false)` (NET-7.9): `svc_disconnect` sent to the client with what its
    /// message holds; `ClientDisconnect` run with `self` its edict when it has spawned, `self`
    /// put back after; the slot no longer active, its name empty and its `old_frags`
    /// -999999, its edict left in the world; and every active client sent the slot's empty
    /// name, 0 frags and colours 0. A slot not active is left as it is.
    pub(crate) fn drop_client(&mut self, id: ClientId) -> Result<(), RunError> {
        let Some(slot) = self.sim.messages.clients.get_mut(id) else {
            return Ok(());
        };
        if !slot.active {
            return Ok(());
        }
        slot.message.write_byte(SVC_DISCONNECT);
        slot.send_message();
        if slot.spawned {
            let saved = self.global("self");
            let value = self.value_of(id.edict());
            self.set_global("self", Slot::from_i32(value));
            let disconnect = self.global("ClientDisconnect").as_i32();
            self.run(disconnect)?;
            self.set_global("self", saved);
        }
        if let Some(slot) = self.sim.messages.clients.get_mut(id) {
            slot.active = false;
            slot.name.clear();
            slot.old_frags = DROPPED_FRAGS;
            // `NET_Close`: what the connection sent and the server has not read goes with it.
            slot.commands.clear();
        }
        if let Some(active) = self.sim.mock.engine_mut().clients_active.get_mut(id.slot()) {
            *active = false;
        }
        for client in self.sim.messages.clients.iter_mut() {
            if !client.active {
                continue;
            }
            client.message.write_byte(SVC_UPDATENAME);
            client.message.write_byte(id.byte());
            client.message.write_string(b"");
            client.message.write_byte(SVC_UPDATEFRAGS);
            client.message.write_byte(id.byte());
            client.message.write_short(0);
            client.message.write_byte(SVC_UPDATECOLORS);
            client.message.write_byte(id.byte());
            client.message.write_byte(0);
        }
        Ok(())
    }

    /// `Host_Spawn_f`'s and `PF_spawnclient`'s edict (PHYS-10.1, BUILTIN-18.45): every field
    /// of client `id`'s edict 0, then `colormap` its number, `team` its bottom colour + 1 and
    /// `netname` its name, `name` the string value made of it.
    pub(crate) fn clear_client_edict(&mut self, id: ClientId, name: i32, colors: i32) {
        let ent = id.edict();
        let fields = self.context.machine_view().edicts().layout().entityfields();
        for slot in 0..fields {
            // A slot past the edict's fields is not written.
            let _written = self.context.machine().edicts_mut().set_field(
                EdictNumber::new(ent),
                slot,
                Slot::from_i32(0),
            );
        }
        // `ent->v.colormap = NUM_FOR_EDICT(ent);`
        let colormap = i32::try_from(ent).map_or(0.0, int_to_float);
        self.set_float(ent, "colormap", colormap);
        // PHYS-10.1: "`ent->v.team = (host_client->colors & 15) + 1;`".
        let team = (colors & COLOR_MASK).wrapping_add(1);
        self.set_float(ent, "team", int_to_float(team));
        self.set_entity_raw(ent, "netname", name);
    }
}

#[cfg(test)]
mod tests;
