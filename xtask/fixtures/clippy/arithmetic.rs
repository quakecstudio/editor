// SPDX-License-Identifier: GPL-3.0-or-later
// expect: clippy::arithmetic_side_effects

pub fn add(a: u32, b: u32) -> u32 {
    a + b
}
