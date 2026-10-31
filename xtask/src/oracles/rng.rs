// SPDX-License-Identifier: GPL-3.0-or-later

//! The seeded generator the differentials' program generators share.

/// A small deterministic generator (xorshift64*), so every host makes the same programs.
pub(crate) struct Rng(pub(crate) u64);

impl Rng {
    pub(crate) fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number below `n`, which is above 0.
    pub(crate) fn below(&mut self, n: usize) -> usize {
        let n = u64::try_from(n).unwrap_or(1).max(1);
        usize::try_from(self.next().checked_rem(n).unwrap_or(0)).unwrap_or(0)
    }

    pub(crate) fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    pub(crate) fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items.get(self.below(items.len())).copied().unwrap_or("")
    }
}
