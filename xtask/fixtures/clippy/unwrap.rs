// SPDX-License-Identifier: GPL-3.0-or-later
// expect: clippy::unwrap_used

pub fn first(value: Option<u8>) -> u8 {
    value.unwrap()
}
