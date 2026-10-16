// SPDX-License-Identifier: GPL-3.0-or-later

//! Below the flake threshold: the test passes or fails at random, so 20
//! consecutive runs all pass with probability 2^-20.

use std::hash::{BuildHasher, RandomState};

pub fn coin() -> bool {
    RandomState::new().hash_one(0_u8) % 2 == 0
}

#[cfg(test)]
mod tests {
    #[test]
    fn passes_half_the_time() {
        assert!(super::coin());
    }
}
