// SPDX-License-Identifier: GPL-3.0-or-later

//! The level maker's random sequence: SplitMix64, so one seed gives one level on every
//! machine.

/// The increment of SplitMix64's state, the golden ratio's fraction of `2^64`.
const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
/// SplitMix64's first multiplier.
const MIX_A: u64 = 0xBF58_476D_1CE4_E5B9;
/// SplitMix64's second multiplier.
const MIX_B: u64 = 0x94D0_49BB_1331_11EB;
/// SplitMix64's first shift.
const SHIFT_A: u32 = 30;
/// SplitMix64's second shift.
const SHIFT_B: u32 = 27;
/// SplitMix64's last shift.
const SHIFT_C: u32 = 31;

/// A random sequence.
#[derive(Clone, Debug)]
pub(crate) struct Rng(u64);

impl Rng {
    /// The sequence of `seed`.
    pub(crate) const fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next 64 bits.
    pub(crate) const fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(GAMMA);
        let mut z = self.0;
        z = (z ^ (z >> SHIFT_A)).wrapping_mul(MIX_A);
        z = (z ^ (z >> SHIFT_B)).wrapping_mul(MIX_B);
        z ^ (z >> SHIFT_C)
    }

    /// A number below `bound`; 0 for a bound of 0.
    pub(crate) fn below(&mut self, bound: usize) -> usize {
        let Ok(wide) = u64::try_from(bound) else {
            return 0;
        };
        let drawn = self.next().checked_rem(wide).unwrap_or(0);
        usize::try_from(drawn).unwrap_or(0)
    }

    /// One of `choices`; `None` when there are none.
    pub(crate) fn pick<'a, T>(&mut self, choices: &'a [T]) -> Option<&'a T> {
        choices.get(self.below(choices.len()))
    }

    /// Whether a chance of `times` in `out_of` comes up.
    pub(crate) fn chance(&mut self, times: usize, out_of: usize) -> bool {
        self.below(out_of) < times
    }

    /// `items` in a random order.
    pub(crate) fn shuffle<T>(&mut self, items: &mut [T]) {
        let mut end = items.len();
        while end > 1 {
            let other = self.below(end);
            end = end.saturating_sub(1);
            items.swap(end, other);
        }
    }
}
