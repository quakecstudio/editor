// SPDX-License-Identifier: GPL-3.0-or-later
// expect: clippy::indexing_slicing

pub fn first(values: &[u8]) -> u8 {
    values[0]
}
