// SPDX-License-Identifier: GPL-3.0-or-later

use proptest::prelude::*;

use super::{
    PROGS_CRC_INIT, PROGS_CRC_POLY, PROGS_CRC_XOR, crc_block, crc_init, crc_process_byte,
    crc_value, table_entry,
};

/// The CRC computed bit by bit from the polynomial, with no table.
fn bitwise(bytes: &[u8]) -> u16 {
    let mut crc = PROGS_CRC_INIT;
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ PROGS_CRC_POLY
            };
        }
    }
    crc ^ PROGS_CRC_XOR
}

// spec PROGS-4.1: CRC-16/CCITT-FALSE: polynomial 0x1021, initial 0xFFFF, no final XOR.
#[test]
fn constants_are_ccitt_false() {
    assert_eq!(PROGS_CRC_POLY, 0x1021);
    assert_eq!(PROGS_CRC_INIT, 0xFFFF);
    assert_eq!(PROGS_CRC_XOR, 0x0000);
    assert_eq!(crc_init(), 0xFFFF);
    assert_eq!(crc_value(0x1234), 0x1234);
}

// spec PROGS-4.1: table entry i is the CRC of byte i from zero; these are entries 0, 1, 2,
// 128 and 255 of crctable in WinQuake/crc.c.
#[test]
fn table_entries_match_id() {
    assert_eq!(table_entry(0), 0x0000);
    assert_eq!(table_entry(1), 0x1021);
    assert_eq!(table_entry(2), 0x2042);
    assert_eq!(table_entry(128), 0x9188);
    assert_eq!(table_entry(255), 0x1ef0);
}

// spec PROGS-4.1: one byte updates the CRC as (crc << 8) ^ table[(crc >> 8) ^ byte].
#[test]
fn process_byte_follows_the_update_rule() {
    let crc: u16 = 0xABCD;
    let byte: u8 = 0x5A;
    let expected = (crc << 8) ^ table_entry(0xAB ^ byte);
    assert_eq!(crc_process_byte(crc, byte), expected);
}

// spec PROGS-4.2: the CRC of no bytes is 0xFFFF and of "123456789" is 0x29B1.
#[test]
fn check_values() {
    assert_eq!(crc_block(b""), 0xFFFF);
    assert_eq!(crc_block(b"123456789"), 0x29B1);
}

proptest! {
    // spec PROGS-4.1: the table-driven CRC equals the bitwise CRC of the polynomial.
    #[test]
    fn table_crc_equals_bitwise(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        prop_assert_eq!(crc_block(&bytes), bitwise(&bytes));
    }
}
