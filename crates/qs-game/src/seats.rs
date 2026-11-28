// SPDX-License-Identifier: GPL-3.0-or-later

//! The local players (ADR-0031, L-86): each a client joined to its server slot through a
//! loopback connection in the host's process, as id's `net_loop.c` joins its one local client
//! (NET-7.3), so a network connection could later join a slot beside them with no change to
//! the server. The first player is the window's or the Game tab's; each other one is a seat,
//! with a client of its own that reads only what its slot was sent.

use qc_progs::Progs;
use qs_client::{Client, Event, Models, Random};
use qs_sim::{ClientId, UserCmd, World, WorldError};

/// The most local players a game takes: the first and three more, each in a window of its own.
pub(crate) const MAX_LOCAL_PLAYERS: u8 = 4;

/// A local client's connection to its server slot (NET-7.3): the moves it sends are its slot's
/// moves, and the messages it reads are the ones its slot was sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Loopback(ClientId);

impl Loopback {
    /// The first local player's, slot 0, which the host connects as it loads a map.
    pub(crate) const FIRST: Self = Self(ClientId::FIRST);

    /// A new local client in the first free slot, every stage of its signon at once
    /// (NET-7.4); `None` when every slot is active.
    ///
    /// # Errors
    ///
    /// [`WorldError::Frame`] when `ClientConnect` or `PutClientInServer` fails.
    pub(crate) fn join(world: &mut World, progs: &Progs<'_>) -> Result<Option<Self>, WorldError> {
        Ok(world.connect(progs)?.map(Self))
    }

    /// The slot it joins.
    pub(crate) const fn id(self) -> ClientId {
        self.0
    }

    /// The move for the slot's next frame (PHYS-9.6).
    pub(crate) fn send(self, world: &mut World, cmd: UserCmd) {
        world.set_command(self.0, cmd);
    }

    /// The packets the slot was sent since the last call, oldest first (NET-1.6).
    pub(crate) fn receive(self, world: &mut World) -> Vec<Vec<u8>> {
        world.take_messages(self.0).packets
    }

    /// Drops what every slot was sent since the last call: a frame stopped part way never
    /// reaches a client, since it replays from before it (WORLD-9.2).
    pub(crate) fn discard_all(world: &mut World) {
        for id in world.maxplayers().ids() {
            drop(world.take_messages(id));
        }
    }
}

/// Why a local player did not join.
#[derive(Debug)]
pub(crate) enum SeatError {
    /// The progs did not read.
    Progs(String),
    /// Its `ClientConnect` or `PutClientInServer` stopped with a run error.
    Spawn(WorldError),
    /// Its client could not read what its signon wrote: the player's number, and why.
    Client(u8, qs_client::ParseError),
}

impl core::fmt::Display for SeatError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Progs(why) => write!(f, "the progs did not read: {why}"),
            Self::Spawn(error) => write!(f, "{error}"),
            Self::Client(number, error) => write!(f, "player {number}'s client: {error}"),
        }
    }
}

/// A local player after the first: its connection and the client that reads its slot's
/// messages.
pub(crate) struct Seat {
    loopback: Loopback,
    client: Client,
    /// Its place among the local players, the first being 1: player 2 is the first seat.
    number: u8,
}

impl Seat {
    /// Local player `number` joined in the first free slot, its name `player<number>` sent as
    /// a client sends its own (NET-7.4), and its client given what the signon wrote; `None`
    /// when every slot is active.
    ///
    /// # Errors
    ///
    /// [`SeatError`]: a run error in its spawn, or a message its client could not read.
    pub(crate) fn join(
        world: &mut World,
        progs: &Progs<'_>,
        number: u8,
        (models, rand): (&mut dyn Models, &mut dyn Random),
    ) -> Result<Option<Self>, SeatError> {
        let Some(loopback) = Loopback::join(world, progs).map_err(SeatError::Spawn)? else {
            return Ok(None);
        };
        world.client_command(loopback.id(), &format!("name \"player{number}\"\n"));
        let mut seat = Self {
            loopback,
            client: Client::new(qs_client::Game::Standard),
            number,
        };
        seat.read(world, 0.0, (models, rand))
            .map_err(|error| SeatError::Client(number, error))?;
        Ok(Some(seat))
    }

    /// The client's frame of `frametime` seconds: it reads what its slot was sent; the events
    /// the messages raised.
    ///
    /// # Errors
    ///
    /// A message the client could not read.
    pub(crate) fn read(
        &mut self,
        world: &mut World,
        frametime: f32,
        (models, rand): (&mut dyn Models, &mut dyn Random),
    ) -> Result<Vec<Event>, qs_client::ParseError> {
        let packets = self.loopback.receive(world);
        self.client.frame(frametime, &packets, models, rand)
    }

    /// Its connection.
    pub(crate) const fn loopback(&self) -> Loopback {
        self.loopback
    }

    /// Its client.
    #[cfg(test)]
    pub(crate) const fn client(&self) -> &Client {
        &self.client
    }

    /// Its client, to steer and to draw.
    pub(crate) const fn client_mut(&mut self) -> &mut Client {
        &mut self.client
    }

    /// Its place among the local players, the first being 1.
    pub(crate) const fn number(&self) -> u8 {
        self.number
    }
}
