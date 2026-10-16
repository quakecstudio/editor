// SPDX-License-Identifier: GPL-3.0-or-later

//! Below the mutation-score threshold: the test runs every line but checks
//! nothing, so every mutant survives.

pub fn add(left: u32, right: u32) -> u32 {
    left.saturating_add(right)
}

#[cfg(test)]
mod tests {
    #[test]
    fn runs_without_checking() {
        std::hint::black_box(super::add(1, 2));
    }
}
