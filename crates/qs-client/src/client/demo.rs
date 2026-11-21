// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-2001 Id Software, Inc.
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/cl_demo.c, CL_ClearSignons, CL_StopPlayback, CL_WriteDemoMessage, CL_GetDemoMessage, CL_GetMessage, CL_Stop_f, CL_Record_f, CL_PlayDemo_f, CL_FinishTimeDemo, CL_TimeDemo_f

//! Demos (CLIENT-10): the messages a client reads, kept in a `.dem` file with the view
//! angles at each, and read back through the same parser in whichever protocol the server
//! wrote them (ADR-0026). [`DemoRecorder`] is the recording side, `CL_Record_f`, `CL_Stop_f`
//! and `CL_GetMessage`'s writes; [`Demo`] the file being played, `CL_PlayDemo_f`'s header
//! and `CL_GetDemoMessage`'s reads; and [`Client::demo_frame`] the playing client's frame.

use std::io::{self, Write};

use qs_sim::{Message, Overflow};

use qs_sim::protocol::ProtocolVersion;

use super::{
    CLIENT_MAX_LIGHTSTYLES, CLIENT_SIGNONS, Client, Event, Models, ParseError, Random,
    SVC_DISCONNECT, SVC_LIGHTSTYLE, SVC_NOP, SVC_PRINT, SVC_SERVERINFO, SVC_SETVIEW, SVC_SIGNONNUM,
    SVC_UPDATECOLORS, SVC_UPDATEFRAGS, SVC_UPDATENAME, SVC_UPDATESTAT, stat,
};
use crate::message::Reader;

/// `MAX_MSGLEN` of `quakedef.h`, the longest message a demo may hold (CLIENT-10.4).
pub const DEMO_MAX_MESSAGE: usize = 64000;
/// The CD track a demo forces when its recording named none: `-1` (CLIENT-10.1).
pub const DEMO_NO_TRACK: i32 = -1;
/// The stage of the signon below which a message is kept for a recording started later,
/// "`if (cls.signon < 2)`" (CLIENT-10.2).
const HEAD_SIGNON: i32 = 2;
/// The stage a recording started while connected writes last, "`MSG_WriteByte (&net_message,
/// 3);`" (CLIENT-10.3).
const RECORD_SIGNON: i32 = 3;
/// The bytes of a message's length, and of each of its three view angles.
const WORD: usize = 4;
/// The room the state a recording starts with is written in: `NET_MAXMESSAGE`'s.
const STATE_ROOM: usize = 64000;

/// Why a demo is not played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoError {
    /// Its first line is not a number and a newline: "`ERROR: demo \"%s\" is invalid`".
    Invalid,
    /// It ends where a message's length should be: "`Demo read error`".
    Read,
    /// A message says it is longer than [`DEMO_MAX_MESSAGE`]: "`Demo message > MAX_MSGLEN`".
    TooLong,
}

impl core::fmt::Display for DemoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "the demo is invalid",
            Self::Read => "Demo read error",
            Self::TooLong => "Demo message > MAX_MSGLEN",
        })
    }
}

impl core::error::Error for DemoError {}

/// Why a frame of a playing demo failed.
#[derive(Clone, Debug, PartialEq)]
pub enum DemoFrameError {
    /// The file.
    Demo(DemoError),
    /// A message of it.
    Parse(ParseError),
}

impl core::fmt::Display for DemoFrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Demo(error) => error.fmt(f),
            Self::Parse(error) => error.fmt(f),
        }
    }
}

impl core::error::Error for DemoFrameError {}

/// The whole number `fscanf`'s `%i` reads at the start of `bytes`, and how many bytes it
/// took: white space, a sign, then digits in base 16 after `0x`, 8 after `0`, else 10. A
/// number too great wraps to 32 bits, as the store to an `int` does.
fn scan_int(bytes: &[u8]) -> Option<(i32, usize)> {
    let mut rest = bytes;
    while let Some((first, tail)) = rest.split_first()
        && (first.is_ascii_whitespace() || *first == 0x0b)
    {
        rest = tail;
    }
    let negative = match rest.split_first() {
        Some((b'-', tail)) => {
            rest = tail;
            true
        }
        Some((b'+', tail)) => {
            rest = tail;
            false
        }
        _ => false,
    };
    let (radix, digits) = match rest {
        [b'0', b'x' | b'X', digit, ..] if digit.is_ascii_hexdigit() => (16_u32, rest.get(2..)?),
        [b'0', ..] => (8, rest),
        _ => (10, rest),
    };
    let mut value = 0_i64;
    let mut taken = 0_usize;
    for digit in digits {
        let Some(digit) = char::from(*digit).to_digit(radix) else {
            break;
        };
        value = value
            .wrapping_mul(i64::from(radix))
            .wrapping_add(i64::from(digit));
        taken = taken.saturating_add(1);
    }
    if taken == 0 {
        return None;
    }
    let value = if negative {
        value.wrapping_neg()
    } else {
        value
    };
    let [a, b, c, d, ..] = value.to_le_bytes();
    let used = bytes
        .len()
        .saturating_sub(digits.len().saturating_sub(taken));
    Some((i32::from_le_bytes([a, b, c, d]), used))
}

/// A message of a demo: the view angles when it was read, and its bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DemoMessage<'a> {
    /// `cl.viewangles` when the message was written.
    pub viewangles: [f32; 3],
    /// The message.
    pub data: &'a [u8],
}

/// A `.dem` file being played: its forced CD track, its bytes and how far it has been
/// read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Demo {
    forcetrack: i32,
    bytes: Vec<u8>,
    at: usize,
}

impl Demo {
    /// `CL_PlayDemo_f`'s read of the file's first line: "`if (fscanf (cls.demofile, "%i",
    /// &cls.forcetrack) != 1 || fgetc (cls.demofile) != '\n')`" the demo is invalid.
    ///
    /// Implements CLIENT-10.1.
    ///
    /// # Errors
    ///
    /// [`DemoError::Invalid`].
    pub fn open(bytes: Vec<u8>) -> Result<Self, DemoError> {
        let (forcetrack, used) = scan_int(&bytes).ok_or(DemoError::Invalid)?;
        if bytes.get(used) != Some(&b'\n') {
            return Err(DemoError::Invalid);
        }
        Ok(Self {
            forcetrack,
            bytes,
            at: used.saturating_add(1),
        })
    }

    /// The CD track the demo forces, or [`DEMO_NO_TRACK`].
    #[must_use]
    pub const fn forcetrack(&self) -> i32 {
        self.forcetrack
    }

    /// `CL_GetDemoMessage`'s read of the next message: its length, its three view angles and
    /// its bytes. `None` when the file ends within a message, or the message has no bytes
    /// or a length below 0, where `fread` reads nothing and playback stops.
    ///
    /// Implements CLIENT-10.4.
    ///
    /// # Errors
    ///
    /// [`DemoError::Read`] when the file ends where a length should be, and
    /// [`DemoError::TooLong`] for a length past [`DEMO_MAX_MESSAGE`].
    pub fn next_message(&mut self) -> Result<Option<DemoMessage<'_>>, DemoError> {
        let word = |bytes: &[u8], at: usize| -> Option<[u8; WORD]> {
            bytes.get(at..)?.first_chunk::<WORD>().copied()
        };
        let end = self.bytes.len();
        let length = word(&self.bytes, self.at).ok_or(DemoError::Read)?;
        let length = i32::from_le_bytes(length);
        let mut at = self.at.saturating_add(WORD);
        let mut viewangles = [0.0_f32; 3];
        for angle in &mut viewangles {
            // The file ending within the angles leaves no message to read after them.
            let Some(bytes) = word(&self.bytes, at) else {
                self.at = end;
                return Ok(None);
            };
            *angle = f32::from_le_bytes(bytes);
            at = at.saturating_add(WORD);
        }
        let Ok(length) = usize::try_from(length) else {
            self.at = end;
            return Ok(None);
        };
        if length > DEMO_MAX_MESSAGE {
            return Err(DemoError::TooLong);
        }
        let after = at.saturating_add(length);
        if length == 0 || after > end {
            self.at = end;
            return Ok(None);
        }
        self.at = after;
        Ok(self
            .bytes
            .get(at..after)
            .map(|data| DemoMessage { viewangles, data }))
    }

    /// The map the demo is of: the world's model of its first message's serverinfo,
    /// `maps/<map>.bsp`, which a host that draws it loads. None when the first message
    /// holds no serverinfo after its prints, or the file has none.
    #[must_use]
    pub fn map(&self) -> Option<String> {
        let first = self.clone().next_message().ok()??.data.to_vec();
        let mut reader = Reader::new(&first);
        loop {
            match reader.byte() {
                SVC_NOP => {}
                SVC_PRINT => drop(reader.string()),
                SVC_SERVERINFO => break,
                _ => return None,
            }
        }
        let version = ProtocolVersion::from_number(reader.long())?;
        if version == ProtocolVersion::Rmq {
            // The protocol's flags.
            reader.long();
        }
        // The client slots, the game's kind and the level's name.
        reader.byte();
        reader.byte();
        drop(reader.string());
        let world = reader.string();
        let world = String::from_utf8(world).ok()?;
        let map = world.strip_prefix("maps/")?.strip_suffix(".bsp")?;
        (!map.is_empty()).then(|| map.to_owned())
    }
}

/// What `record` was given for the CD track: none, or a track to force.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoTrack {
    /// No track: the file's first line is `-1`.
    None,
    /// "`track = atoi(Cmd_Argv(3));`".
    Forced(i32),
}

/// The recording side: the messages read before the client is connected, kept so that a
/// recording may start afterwards (from ProQuake), and the file a recording writes.
pub struct DemoRecorder {
    head: Vec<Vec<u8>>,
    out: Option<Box<dyn Write>>,
}

impl core::fmt::Debug for DemoRecorder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DemoRecorder")
            .field("head", &self.head.len())
            .field("recording", &self.out.is_some())
            .finish()
    }
}

impl Default for DemoRecorder {
    fn default() -> Self {
        Self::new()
    }
}

/// `CL_WriteDemoMessage`: the message's length, the view angles and the message, each
/// little-endian, then the file flushed.
fn write_message(out: &mut dyn Write, viewangles: [f32; 3], data: &[u8]) -> io::Result<()> {
    let length = i32::try_from(data.len()).map_err(io::Error::other)?;
    out.write_all(&length.to_le_bytes())?;
    for angle in viewangles {
        out.write_all(&angle.to_le_bytes())?;
    }
    out.write_all(data)?;
    out.flush()
}

impl DemoRecorder {
    /// A recorder that has read nothing and records nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            head: Vec::new(),
            out: None,
        }
    }

    /// Whether a demo is being recorded, `cls.demorecording`.
    #[must_use]
    pub const fn recording(&self) -> bool {
        self.out.is_some()
    }

    /// `CL_ClearSignons`: the kept messages are forgotten, as when the client disconnects.
    pub fn clear_signons(&mut self) {
        self.head.clear();
    }

    /// `CL_GetMessage`'s work on a message the client is about to read at signon stage
    /// `signon` with the view angles `viewangles`: it is written to the demo being
    /// recorded, and kept for a later recording while the stage is below 2.
    ///
    /// Implements CLIENT-10.2.
    ///
    /// # Errors
    ///
    /// The file's, when the message cannot be written; the recording is then over.
    pub fn got(&mut self, signon: i32, viewangles: [f32; 3], data: &[u8]) -> io::Result<()> {
        if signon < HEAD_SIGNON {
            self.head.push(data.to_vec());
        }
        let Some(out) = self.out.as_mut() else {
            return Ok(());
        };
        let written = write_message(out.as_mut(), viewangles, data);
        if written.is_err() {
            self.out = None;
        }
        written
    }

    /// `CL_Record_f`'s file: `out` starts with the forced track's line, and when a client
    /// is given, one already connected, with the messages kept from before it was and the
    /// state it has now ([`Client::demo_state`]), from ProQuake.
    ///
    /// Implements CLIENT-10.3.
    ///
    /// # Errors
    ///
    /// The file's; nothing is then recorded.
    pub fn record(
        &mut self,
        mut out: Box<dyn Write>,
        track: DemoTrack,
        connected: Option<&Client>,
    ) -> io::Result<()> {
        let track = match track {
            DemoTrack::None => DEMO_NO_TRACK,
            DemoTrack::Forced(track) => track,
        };
        writeln!(out, "{track}")?;
        if let Some(client) = connected {
            for message in &self.head {
                write_message(out.as_mut(), client.viewangles, message)?;
            }
            write_message(out.as_mut(), client.viewangles, &client.demo_state())?;
        }
        self.out = Some(out);
        Ok(())
    }

    /// `CL_Stop_f`: a disconnect message ends the demo and the file is closed. Whether a
    /// demo was being recorded; `Not recording a demo.` is the caller's to say when not.
    ///
    /// Implements CLIENT-10.5.
    ///
    /// # Errors
    ///
    /// The file's, when the last message cannot be written; the recording is over all the
    /// same.
    pub fn stop(&mut self, viewangles: [f32; 3]) -> io::Result<bool> {
        let Some(mut out) = self.out.take() else {
            return Ok(false);
        };
        let disconnect = u8::try_from(SVC_DISCONNECT).unwrap_or_default();
        write_message(out.as_mut(), viewangles, &[disconnect]).map(|()| true)
    }
}

/// How a playing demo's messages are taken (CLIENT-10.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoPace {
    /// `playdemo`: another message whenever the client's time has passed the last one's.
    Timed,
    /// `timedemo`: one message each host frame, as fast as frames are drawn.
    EveryFrame,
}

/// A playing demo's state in the client: the view angles of its last two messages,
/// `cl.mviewangles`, and its pace.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Playback {
    pub(super) mviewangles: [[f32; 3]; 2],
    pub(super) pace: DemoPace,
}

/// How a frame of a playing demo left it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoState {
    /// More is to come.
    Playing,
    /// It ended: its disconnect message was read, or the file ran out (`CL_StopPlayback`).
    Ended,
}

/// What a frame of a playing demo did.
#[derive(Clone, Debug, PartialEq)]
pub struct DemoFrame {
    /// The events its messages raised.
    pub events: Vec<Event>,
    /// How many messages it read.
    pub read: usize,
    /// Whether the demo goes on.
    pub state: DemoState,
}

impl Client {
    /// Whether a demo is playing, `cls.demoplayback`.
    #[must_use]
    pub const fn demo_playing(&self) -> bool {
        self.playback.is_some()
    }

    /// `CL_StopPlayback`'s part in the client: no demo is playing.
    pub const fn stop_playback(&mut self) {
        self.playback = None;
    }

    /// The state a recording started while connected writes after the kept messages
    /// (CLIENT-10.3): each client's name, frags and colours, every light style, the four
    /// counts of secrets and monsters, the view entity, and signon stage 3.
    #[must_use]
    pub fn demo_state(&self) -> Vec<u8> {
        let mut message = Message::new(STATE_ROOM, Overflow::Fatal);
        let number = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
        for (slot, score) in self.scores.iter().enumerate().take(self.maxclients) {
            message.write_byte(SVC_UPDATENAME);
            message.write_byte(number(slot));
            message.write_string(&score.name);
            message.write_byte(SVC_UPDATEFRAGS);
            message.write_byte(number(slot));
            message.write_short(score.frags);
            message.write_byte(SVC_UPDATECOLORS);
            message.write_byte(number(slot));
            message.write_byte(score.colors);
        }
        for style in 0..CLIENT_MAX_LIGHTSTYLES {
            message.write_byte(SVC_LIGHTSTYLE);
            message.write_byte(number(style));
            message.write_string(self.lightstyles.get(style).map_or(&[], Vec::as_slice));
        }
        for counted in [
            stat::TOTALSECRETS,
            stat::TOTALMONSTERS,
            stat::SECRETS,
            stat::MONSTERS,
        ] {
            message.write_byte(SVC_UPDATESTAT);
            message.write_byte(number(counted));
            message.write_long(self.stats.get(counted).copied().unwrap_or(0));
        }
        message.write_byte(SVC_SETVIEW);
        message.write_short(number(self.viewentity));
        message.write_byte(SVC_SIGNONNUM);
        message.write_byte(RECORD_SIGNON);
        message.take()
    }

    /// [`Client::frame`] with each packet given to `recorder` before it is read, as
    /// `CL_GetMessage` hands a message on: a keepalive, a packet of `svc_nop` alone, is
    /// neither read nor recorded.
    ///
    /// Implements CLIENT-10.2.
    ///
    /// # Errors
    ///
    /// [`ParseError`] as [`Client::frame`]'s. A demo file that cannot be written ends the
    /// recording and is said in an [`Event::Print`].
    pub fn frame_recorded(
        &mut self,
        frametime: f32,
        packets: &[Vec<u8>],
        recorder: &mut DemoRecorder,
        models: &mut dyn Models,
        rand: &mut dyn Random,
    ) -> Result<Vec<Event>, ParseError> {
        self.advance(frametime);
        let mut events = Vec::new();
        let nop = u8::try_from(SVC_NOP).unwrap_or_default();
        for packet in packets {
            if packet.as_slice() == [nop] {
                events.push(Event::Print(b"<-- server to client keepalive\n".to_vec()));
                continue;
            }
            if let Err(error) = recorder.got(self.signon, self.viewangles, packet) {
                let said = format!("ERROR: the demo was not written: {error}\n");
                events.push(Event::Print(said.into_bytes()));
            }
            self.read_message(packet, models, rand, &mut events)?;
        }
        self.finish_frame(rand, &events);
        Ok(events)
    }

    /// A frame of a client playing `demo` at `pace`: `CL_ReadFromServer` with
    /// `CL_GetDemoMessage` in the place of the network. The clock advances by `frametime`;
    /// until the client is fully connected every message is read; then, timed, one is read
    /// while the client's time is past the last message's, and, every frame, one a frame.
    /// A message read gives its view angles to `cl.mviewangles`, which the relink turns
    /// the view between. The demo ends at its disconnect message, or where the file runs
    /// out.
    ///
    /// Implements CLIENT-10.6.
    ///
    /// # Errors
    ///
    /// [`DemoFrameError`]: the file's `Demo read error` or message too long, or a message
    /// that does not read.
    pub fn demo_frame(
        &mut self,
        frametime: f32,
        demo: &mut Demo,
        pace: DemoPace,
        models: &mut dyn Models,
        rand: &mut dyn Random,
    ) -> Result<DemoFrame, DemoFrameError> {
        let mut playback = self.playback.unwrap_or(Playback {
            mviewangles: [self.viewangles; 2],
            pace,
        });
        playback.pace = pace;
        self.playback = Some(playback);
        self.advance(frametime);
        let mut events = Vec::new();
        let mut read = 0_usize;
        let mut state = DemoState::Playing;
        loop {
            // "decide if it is time to grab the next message": always until fully connected.
            if self.signon == CLIENT_SIGNONS {
                let enough = match pace {
                    DemoPace::EveryFrame => read > 0,
                    DemoPace::Timed => self.time <= self.mtime[0],
                };
                if enough {
                    break;
                }
            }
            let Some(message) = demo.next_message().map_err(DemoFrameError::Demo)? else {
                state = DemoState::Ended;
                break;
            };
            if let Some(playback) = self.playback.as_mut() {
                let [latest, _] = playback.mviewangles;
                playback.mviewangles = [message.viewangles, latest];
            }
            let before = events.len();
            self.read_message(message.data, models, rand, &mut events)
                .map_err(DemoFrameError::Parse)?;
            read = read.saturating_add(1);
            let disconnected = events
                .get(before..)
                .is_some_and(|new| new.contains(&Event::Disconnect));
            if disconnected {
                state = DemoState::Ended;
                break;
            }
        }
        self.finish_frame(rand, &events);
        if state == DemoState::Ended {
            self.stop_playback();
        }
        Ok(DemoFrame {
            events,
            read,
            state,
        })
    }
}

#[cfg(test)]
mod tests;
