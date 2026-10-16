// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/crc.c, CRC_Init, CRC_ProcessByte, CRC_Value

//! The 16-bit CRC id's engine and qcc use for the progs.dat header CRC and the file CRC.
//!
//! Implements PROGS-4 of `docs/spec/progs.md`.

#[cfg(test)]
mod tests;

/// The CRC polynomial, from the comment above `crctable` in `WinQuake/crc.c`.
///
/// `PROGS_CRC_POLY` in PROGS-4.
pub const PROGS_CRC_POLY: u16 = 0x1021;

/// The value a CRC starts from, id's `CRC_INIT_VALUE`.
///
/// `PROGS_CRC_INIT` in PROGS-4.
pub const PROGS_CRC_INIT: u16 = 0xFFFF;

/// The value XORed into the final CRC, id's `CRC_XOR_VALUE`.
///
/// `PROGS_CRC_XOR` in PROGS-4.
pub const PROGS_CRC_XOR: u16 = 0x0000;

/// Entry `index` of id's `crctable`: the CRC of the single byte `index` from a zero initial
/// value (PROGS-4.1).
///
/// Computed from [`PROGS_CRC_POLY`] instead of copied from id's table; the tests and the
/// differential run check the results against id's code.
const fn table_entry(index: u8) -> u16 {
    let mut crc = u16::from_be_bytes([index, 0]);
    let mut bit: u8 = 0;
    while bit < 8 {
        let carry = crc & 0x8000 != 0;
        crc = crc.wrapping_shl(1);
        if carry {
            crc ^= PROGS_CRC_POLY;
        }
        bit = bit.wrapping_add(1);
    }
    crc
}

/// The starting value of a CRC, id's `CRC_Init`.
///
/// Implements PROGS-4.1.
#[must_use]
pub const fn crc_init() -> u16 {
    PROGS_CRC_INIT
}

/// Adds one byte to a CRC, id's `CRC_ProcessByte`.
///
/// Implements PROGS-4.1: `crc = (crc << 8) ^ crctable[(crc >> 8) ^ byte]`, truncated to 16
/// bits.
#[must_use]
pub const fn crc_process_byte(crc: u16, byte: u8) -> u16 {
    let [high, low] = crc.to_be_bytes();
    u16::from_be_bytes([low, 0]) ^ table_entry(high ^ byte)
}

/// The final value of a CRC, id's `CRC_Value`.
///
/// Implements PROGS-4.1.
#[must_use]
pub const fn crc_value(crc: u16) -> u16 {
    crc ^ PROGS_CRC_XOR
}

/// The CRC of `bytes`: [`crc_init`], then [`crc_process_byte`] for each byte, then
/// [`crc_value`].
///
/// Implements PROGS-4.
#[must_use]
pub fn crc_block(bytes: &[u8]) -> u16 {
    let mut crc = crc_init();
    for &byte in bytes {
        crc = crc_process_byte(crc, byte);
    }
    crc_value(crc)
}
