// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/common.c, MSG_BeginReading, MSG_ReadChar, MSG_ReadByte, MSG_ReadShort, MSG_ReadLong, MSG_ReadFloat, MSG_ReadString, MSG_ReadCoord, MSG_ReadAngle

//! Reading a server message's bytes as id's `MSG_Read*` read them (CLIENT-1.4): a coordinate
//! is a short over 8, an angle a byte over 256 of a turn, and reading past the end is the
//! error `Bad server message`.

use qc_vm::convert::double_to_float;

/// The bytes of a long and a float.
const LONG_BYTES: usize = 4;
/// A coordinate's steps per unit, `MSG_ReadShort() * (1.0/8)`.
const COORD_STEPS: f64 = 8.0;
/// A turn in degrees.
const FULL_TURN: f64 = 360.0;
/// An angle byte's steps per turn, `MSG_ReadChar() * (360.0/256)`.
const ANGLE_STEPS: f64 = 256.0;

/// A message being read, `net_message` with `msg_readcount` (CLIENT-1.4).
#[derive(Clone, Debug)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    /// `msg_badread`: a read ran past the end.
    bad: bool,
}

impl<'a> Reader<'a> {
    /// `MSG_BeginReading`.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            at: 0,
            bad: false,
        }
    }

    /// Whether a read ran past the end (`msg_badread`).
    #[must_use]
    pub const fn bad(&self) -> bool {
        self.bad
    }

    /// Whether every byte has been read.
    #[must_use]
    pub const fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }

    /// The next `count` bytes, or `None` once past the end, which marks the read bad.
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(count)?;
        match self.bytes.get(self.at..end) {
            Some(slice) => {
                self.at = end;
                Some(slice)
            }
            None => {
                self.bad = true;
                None
            }
        }
    }

    /// `MSG_ReadChar`: a signed byte, -1 past the end.
    pub fn char(&mut self) -> i32 {
        self.take(1)
            .and_then(|b| b.first())
            .map_or(-1, |&b| i32::from(i8::from_le_bytes([b])))
    }

    /// `MSG_ReadByte`: an unsigned byte, -1 past the end.
    pub fn byte(&mut self) -> i32 {
        self.take(1)
            .and_then(|b| b.first())
            .map_or(-1, |&b| i32::from(b))
    }

    /// `MSG_ReadShort`: a signed little-endian short, -1 past the end.
    pub fn short(&mut self) -> i32 {
        match self.take(2) {
            Some(&[a, b]) => i32::from(i16::from_le_bytes([a, b])),
            _ => -1,
        }
    }

    /// `MSG_ReadLong`: a signed little-endian long, -1 past the end.
    pub fn long(&mut self) -> i32 {
        match self.take(LONG_BYTES) {
            Some(&[a, b, c, d]) => i32::from_le_bytes([a, b, c, d]),
            _ => -1,
        }
    }

    /// `MSG_ReadFloat`: four little-endian bytes as a float, -1 past the end.
    pub fn float(&mut self) -> f32 {
        match self.take(LONG_BYTES) {
            Some(&[a, b, c, d]) => f32::from_le_bytes([a, b, c, d]),
            _ => -1.0,
        }
    }

    /// `MSG_ReadString`: the bytes up to a NUL or the end (the end is not an error).
    pub fn string(&mut self) -> Vec<u8> {
        let rest = self.bytes.get(self.at..).unwrap_or_default();
        let end = rest.iter().position(|&b| b == 0);
        let text = rest.get(..end.unwrap_or(rest.len())).unwrap_or_default();
        self.at = self
            .at
            .saturating_add(end.map_or(rest.len(), |e| e.saturating_add(1)));
        text.to_vec()
    }

    /// `MSG_ReadCoord`: a short over 8.
    pub fn coord(&mut self) -> f32 {
        double_to_float(f64::from(self.short()) * (1.0 / COORD_STEPS))
    }

    /// `MSG_ReadAngle`: a char times 360 over 256.
    pub fn angle(&mut self) -> f32 {
        double_to_float(f64::from(self.char()) * (FULL_TURN / ANGLE_STEPS))
    }
}
