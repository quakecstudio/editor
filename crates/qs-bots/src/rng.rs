// SPDX-License-Identifier: GPL-3.0-or-later

//! Each bot's own random numbers (BOT-8): SplitMix64, started from the host's seed and the
//! bot's slot.

use qs_sim::ClientId;

/// SplitMix64's increment, the golden ratio's fraction in 64 bits.
const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;
/// SplitMix64's two multipliers.
const MIX_A: u64 = 0xBF58_476D_1CE4_E5B9;
const MIX_B: u64 = 0x94D0_49BB_1331_11EB;
/// SplitMix64's three shifts.
const SHIFT_A: u32 = 30;
const SHIFT_B: u32 = 27;
const SHIFT_C: u32 = 31;
/// The bits of 1.0 as an `f32`, under which 23 random bits make a number from 1 to 2.
const ONE_BITS: u32 = 0x3F80_0000;
/// How far the 64 bits are shifted to leave the 23 an `f32`'s fraction holds.
const FRACTION_SHIFT: u32 = 41;

/// A bot's random source (BOT-8.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BotRng(u64);

impl BotRng {
    /// The source of the bot in slot `id`, from the host's `seed`.
    #[must_use]
    pub fn new(seed: u64, id: ClientId) -> Self {
        let slot = u64::try_from(id.slot()).unwrap_or(0);
        Self(seed ^ GOLDEN.wrapping_mul(slot.wrapping_add(1)))
    }

    /// The next 64 bits.
    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(GOLDEN);
        let mut z = self.0;
        z = (z ^ (z >> SHIFT_A)).wrapping_mul(MIX_A);
        z = (z ^ (z >> SHIFT_B)).wrapping_mul(MIX_B);
        z ^ (z >> SHIFT_C)
    }

    /// A number from 0 up to but not including 1.
    pub fn unit(&mut self) -> f32 {
        let fraction = u32::try_from(self.next_u64() >> FRACTION_SHIFT).unwrap_or(0);
        f32::from_bits(ONE_BITS | fraction) - 1.0
    }

    /// A number from `-1` up to but not including 1.
    pub fn signed(&mut self) -> f32 {
        self.unit().mul_add(2.0, -1.0)
    }

    /// A whole number below `count`, 0 for a count of 0.
    pub fn below(&mut self, count: usize) -> usize {
        let count64 = u64::try_from(count).unwrap_or(u64::MAX);
        self.next_u64()
            .checked_rem(count64)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0)
    }
}
