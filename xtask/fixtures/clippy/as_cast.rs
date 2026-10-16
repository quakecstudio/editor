// SPDX-License-Identifier: GPL-3.0-or-later
// expect: clippy::cast_possible_truncation

pub fn narrow(value: u32) -> u8 {
    value as u8
}
