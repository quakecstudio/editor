// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/entities.cc, AddSun, SetupSun, SetupSkyDome, Dirt_ResolveFlag
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/mathlib.cc, Random

//! Suns (LIGHT-14) and the sky dome (LIGHT-15): each a direction light comes from wherever
//! the sky is seen, a sun with a penumbra being many suns scattered about its direction and
//! the dome a ring of them over each hemisphere.

use crate::extended::keys::Dirt;
use crate::extended::math::{normalize, scale};
use crate::{Jitter, Vec3, to_f32};

/// `sunsamples`: how many suns a sun with a penumbra, or a dome, is made of (LIGHT-14.4).
const SUN_SAMPLES: u32 = 64;
/// How far away a sun is put (`AddSun`).
const SUN_DISTANCE: f32 = -16384.0;
/// Degrees to a half turn, and the degrees of a quarter and a whole one.
const HALF_TURN: f64 = 180.0;
const QUARTER_TURN: f32 = 90.0;
const WHOLE_TURN: f32 = 360.0;
/// The suns of a ring of the dome for each step of elevation.
const RING: u32 = 4;
/// `RAND_MAX` of both generators.
const RAND_MAX: u32 = 0x7fff_ffff;
/// The multiplier and modulus of the minimal standard generator, and the quotient and
/// remainder of the modulus by the multiplier.
const MINSTD: i64 = 16807;
const MINSTD_MODULUS: i64 = 0x7fff_ffff;
const MINSTD_QUOTIENT: i64 = 127_773;
const MINSTD_REMAINDER: i64 = 2836;
/// glibc's additive generator: its state's length, the two taps, and how many values it
/// discards after seeding.
const GLIBC_STATE: usize = 34;
const GLIBC_FAR_TAP: usize = 31;
const GLIBC_NEAR_TAP: usize = 3;
const GLIBC_DISCARD: usize = 310;

/// A direction light comes from (`sun_t`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sun {
    /// Where the sun is from a sample: the way its light goes, turned about and far.
    pub(crate) vec: Vec3,
    pub(crate) light: f32,
    pub(crate) color: Vec3,
    /// Whether dirt darkens it.
    pub(crate) dirt: bool,
    pub(crate) anglescale: f32,
    pub(crate) style: i32,
}

/// `Random`: the C library's `rand` over `RAND_MAX`, which is another sequence on each
/// platform ericw-tools is built for (LIGHT-14.5).
pub(crate) enum Random {
    /// glibc's: `r[i] = r[i-3] + r[i-31]`, its low bit dropped.
    Glibc(Vec<u32>),
    /// The BSDs' and macOS's: `x = 16807 x mod (2^31 - 1)`.
    Bsd(i64),
}

/// One step of the minimal standard generator, without overflow.
fn minstd(x: i64) -> i64 {
    let hi = x.checked_div(MINSTD_QUOTIENT).unwrap_or(0);
    let lo = x.checked_rem(MINSTD_QUOTIENT).unwrap_or(0);
    let next = MINSTD
        .wrapping_mul(lo)
        .wrapping_sub(MINSTD_REMAINDER.wrapping_mul(hi));
    if next < 0 {
        next.wrapping_add(MINSTD_MODULUS)
    } else {
        next
    }
}

impl Random {
    /// The generator as a program that never calls `srand` starts it: seeded with 1.
    pub(crate) fn new(jitter: Jitter) -> Self {
        match jitter {
            Jitter::Bsd => Self::Bsd(1),
            Jitter::Glibc => {
                let mut state: Vec<u32> = Vec::with_capacity(GLIBC_STATE);
                let mut word: i64 = 1;
                state.push(1);
                for _ in 1..GLIBC_FAR_TAP {
                    word = minstd(word);
                    state.push(u32::try_from(word).unwrap_or(0));
                }
                for at in GLIBC_FAR_TAP..GLIBC_STATE {
                    let far = at.checked_sub(GLIBC_FAR_TAP).and_then(|i| state.get(i));
                    state.push(far.copied().unwrap_or(0));
                }
                let mut random = Self::Glibc(state);
                for _ in 0..GLIBC_DISCARD {
                    random.rand();
                }
                random
            }
        }
    }

    /// `rand`.
    fn rand(&mut self) -> u32 {
        match self {
            Self::Bsd(x) => {
                *x = minstd(*x);
                u32::try_from(*x).unwrap_or(0)
            }
            Self::Glibc(state) => {
                let len = state.len();
                let tap = |back: usize| {
                    len.checked_sub(back)
                        .and_then(|i| state.get(i))
                        .copied()
                        .unwrap_or(0)
                };
                let next = tap(GLIBC_FAR_TAP).wrapping_add(tap(GLIBC_NEAR_TAP));
                state.remove(0);
                state.push(next);
                next >> 1
            }
        }
    }

    /// `Random() * 2 - 1`: a number from -1 to 1.
    pub(crate) fn signed_unit(&mut self) -> f64 {
        f64::from(self.rand()) / f64::from(RAND_MAX) * 2.0 - 1.0
    }
}

/// What makes a sun but its direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SunLight {
    pub(crate) light: f32,
    pub(crate) color: Vec3,
    pub(crate) dirt: Dirt,
    pub(crate) anglescale: f32,
    pub(crate) style: i32,
}

/// The suns of a map, as they are added.
pub(crate) struct Suns<'a> {
    pub(crate) all: Vec<Sun>,
    /// Whether the world has dirt on, which a sun with no word of its own follows.
    pub(crate) world_dirt: bool,
    pub(crate) random: &'a mut Random,
}

impl Suns<'_> {
    /// `AddSun`: a sun shining along `direction`; none for a sun of no light.
    fn add(&mut self, direction: Vec3, sun: SunLight) {
        if sun.light == 0.0 {
            return;
        }
        self.all.push(Sun {
            vec: scale(normalize(direction), SUN_DISTANCE),
            light: sun.light,
            color: sun.color,
            dirt: match sun.dirt {
                Dirt::On => true,
                Dirt::Off => false,
                Dirt::World => self.world_dirt,
            },
            anglescale: sun.anglescale,
            style: sun.style,
        });
    }

    /// `SetupSun`: a sun shining along `direction`; with a penumbra of `deviance` degrees,
    /// 64 suns sharing its light, the first along the direction and the rest scattered
    /// within the penumbra (LIGHT-14.3, LIGHT-14.4).
    pub(crate) fn sun(&mut self, direction: Vec3, deviance: f32, mut sun: SunLight) {
        let count = if deviance == 0.0 { 1 } else { SUN_SAMPLES };
        let radians = to_f32(f64::from(deviance) * core::f64::consts::PI / HALF_TURN);
        // The square of the penumbra in degrees, which the angles in radians are held under.
        let limit = deviance * deviance;
        let along = normalize(direction);
        sun.light /= float_of(count);
        for index in 0..count {
            if index == 0 {
                self.add(along, sun);
                continue;
            }
            // The angles are worked in single precision: C++ takes the float `cos`, `sin`,
            // `sqrt` and `atan2` for float arguments.
            let flat = (along[0] * along[0] + along[1] * along[1]).sqrt();
            let mut angle = along[1].atan2(along[0]);
            let mut elevation = along[2].atan2(flat);
            let (da, de) = loop {
                let da = to_f32(self.random.signed_unit() * f64::from(radians));
                let de = to_f32(self.random.signed_unit() * f64::from(radians));
                if da * da + de * de <= limit {
                    break (da, de);
                }
            };
            angle += da;
            elevation += de;
            self.add(
                [
                    angle.cos() * elevation.cos(),
                    angle.sin() * elevation.cos(),
                    elevation.sin(),
                ],
                sun,
            );
        }
    }

    /// `SetupSkyDome`: a dome of 65 suns over the upper hemisphere sharing `upper`'s light,
    /// and one under the lower sharing `lower`'s: four rings of sixteen, each ring turned a
    /// little from the last, and one straight up or down (LIGHT-15).
    pub(crate) fn dome(&mut self, upper: Option<SunLight>, lower: Option<SunLight>) {
        let lit = |sun: &Option<SunLight>| sun.is_some_and(|sun| sun.light > 0.0);
        if !lit(&upper) && !lit(&lower) {
            return;
        }
        // `iterations`: the square root of a quarter of the samples less one, in whole
        // numbers, and one more; at least 2.
        let quarter = SUN_SAMPLES.saturating_sub(1).checked_div(RING).unwrap_or(0);
        let iterations = whole(f64::from(quarter).sqrt().round_ties_even())
            .saturating_add(1)
            .max(2);
        let elevation_steps = iterations.saturating_sub(1);
        let angle_steps = elevation_steps.saturating_mul(RING);
        let radians = |degrees: f32| to_f32(f64::from(degrees) * core::f64::consts::PI / HALF_TURN);
        let elevation_step = radians(QUARTER_TURN / float_of(elevation_steps.saturating_add(1)));
        let angle_step = radians(WHOLE_TURN / float_of(angle_steps));
        let suns = float_of(
            angle_steps
                .saturating_mul(elevation_steps)
                .saturating_add(1),
        );
        let share = |sun: Option<SunLight>| {
            sun.map(|sun| SunLight {
                light: sun.light / suns,
                ..sun
            })
            .filter(|sun| sun.light > 0.0)
        };
        let (upper, lower) = (share(upper), share(lower));
        let mut elevation = elevation_step * 0.5;
        let mut angle = 0.0_f32;
        for _ in 0..elevation_steps {
            for _ in 0..angle_steps {
                let x = angle.cos() * elevation.cos();
                let y = angle.sin() * elevation.cos();
                let z = elevation.sin();
                if let Some(sun) = upper {
                    self.add([x, y, -z], sun);
                }
                if let Some(sun) = lower {
                    self.add([x, y, z], sun);
                }
                angle += angle_step;
            }
            elevation += elevation_step;
            angle += angle_step / float_of(elevation_steps);
        }
        if let Some(sun) = upper {
            self.add([0.0, 0.0, -1.0], sun);
        }
        if let Some(sun) = lower {
            self.add([0.0, 0.0, 1.0], sun);
        }
    }
}

/// A small count as a float.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec LIGHT-15.1: a count of suns is far within a float's whole numbers"
)]
const fn float_of(count: u32) -> f32 {
    count as f32
}

/// A small whole number held in a `double`, as a count.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "spec LIGHT-15.1: rint's value, a small whole number, is kept as an int"
)]
const fn whole(value: f64) -> u32 {
    value as u32
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::float_cmp,
        clippy::unwrap_used,
        reason = "test code: the values are exact in single precision, and a failed unwrap \
                  is a failed test"
    )]

    use super::*;
    use crate::extended::math::{dot, length};

    const WHITE: SunLight = SunLight {
        light: 130.0,
        color: [255.0; 3],
        dirt: Dirt::World,
        anglescale: 0.5,
        style: 0,
    };

    fn suns(random: &mut Random) -> Suns<'_> {
        Suns {
            all: Vec::new(),
            world_dirt: false,
            random,
        }
    }

    // spec LIGHT-14.5: the scatter is the C library's rand, seeded with 1.
    #[test]
    fn each_generator_gives_its_librarys_sequence() {
        let mut bsd = Random::new(Jitter::Bsd);
        let first: Vec<u32> = (0..5).map(|_| bsd.rand()).collect();
        assert_eq!(
            first,
            [
                16807,
                282_475_249,
                1_622_650_073,
                984_943_658,
                1_144_108_930
            ]
        );
        let mut glibc = Random::new(Jitter::Glibc);
        let first: Vec<u32> = (0..5).map(|_| glibc.rand()).collect();
        assert_eq!(
            first,
            [
                1_804_289_383,
                846_930_886,
                1_681_692_777,
                1_714_636_915,
                1_957_747_793
            ]
        );
        let unit = Random::new(Jitter::Bsd).signed_unit();
        assert!((-1.0..=1.0).contains(&unit));
    }

    // spec LIGHT-14.1: a sun is put 16384 units against the way its light goes; one of no
    // light is not made.
    // spec LIGHT-14.3: with no penumbra a sun is one sun.
    #[test]
    fn a_sun_with_no_penumbra_is_one_sun_far_against_its_light() {
        let mut random = Random::new(Jitter::Bsd);
        let mut suns = suns(&mut random);
        suns.sun([0.0, 0.0, -2.0], 0.0, WHITE);
        assert_eq!(suns.all.len(), 1);
        let sun = suns.all.first().copied().unwrap();
        assert_eq!(sun.vec, [0.0, 0.0, 16384.0]);
        assert_eq!(sun.light, 130.0);
        assert!(!sun.dirt);
        suns.sun(
            [0.0, 0.0, -1.0],
            0.0,
            SunLight {
                light: 0.0,
                ..WHITE
            },
        );
        assert_eq!(suns.all.len(), 1, "a sun of no light");
        suns.world_dirt = true;
        suns.sun([1.0, 0.0, 0.0], 0.0, WHITE);
        assert!(suns.all.last().is_some_and(|sun| sun.dirt));
        suns.sun(
            [1.0, 0.0, 0.0],
            0.0,
            SunLight {
                dirt: Dirt::Off,
                ..WHITE
            },
        );
        assert!(suns.all.last().is_some_and(|sun| !sun.dirt));
    }

    // spec LIGHT-14.4: a penumbra makes 64 suns sharing the light, the first along the
    // direction, the rest within the penumbra of it.
    #[test]
    fn a_penumbra_scatters_sixty_four_suns() {
        let mut random = Random::new(Jitter::Bsd);
        let mut suns = suns(&mut random);
        suns.sun([0.0, 0.0, -1.0], 4.0, WHITE);
        assert_eq!(suns.all.len(), 64);
        assert!(suns.all.iter().all(|sun| sun.light == 130.0 / 64.0));
        let first = suns.all.first().map(|sun| sun.vec);
        assert_eq!(first, Some([0.0, 0.0, 16384.0]));
        // Each is within 4 degrees on each of the two angles, so within about 6 in all.
        let widest = (6.0_f32).to_radians().cos();
        for sun in &suns.all {
            let toward = scale(sun.vec, 1.0 / length(sun.vec));
            assert!(dot(toward, [0.0, 0.0, 1.0]) >= widest);
        }
        let distinct = suns.all.iter().filter(|sun| Some(sun.vec) != first).count();
        assert_eq!(distinct, 63);
    }

    // spec LIGHT-15.1: a dome is 65 suns sharing the light: four rings of sixteen and one
    // straight down.
    // spec LIGHT-15.2: the lower dome mirrors it.
    #[test]
    fn a_dome_is_sixty_five_suns_over_a_hemisphere() {
        let mut random = Random::new(Jitter::Bsd);
        let mut suns = suns(&mut random);
        suns.dome(Some(WHITE), None);
        assert_eq!(suns.all.len(), 65);
        assert!(suns.all.iter().all(|sun| sun.light == 2.0));
        assert!(
            suns.all.iter().all(|sun| sun.vec[2] > 0.0),
            "every sun of the upper dome is above"
        );
        assert_eq!(
            suns.all.last().map(|sun| sun.vec),
            Some([0.0, 0.0, 16384.0])
        );
        // The first ring is 9 degrees up, its first sun along x.
        let first = suns.all.first().map_or([0.0; 3], |sun| sun.vec);
        assert!((first[2] / 16384.0 - 9.0_f32.to_radians().sin()).abs() < 1e-6);
        assert!(first[1].abs() < 1e-3);
        suns.all.clear();
        suns.dome(Some(WHITE), Some(WHITE));
        assert_eq!(suns.all.len(), 130);
        assert_eq!(suns.all.iter().filter(|sun| sun.vec[2] < 0.0).count(), 65);
        suns.all.clear();
        suns.dome(None, None);
        suns.dome(
            Some(SunLight {
                light: 0.0,
                ..WHITE
            }),
            None,
        );
        assert!(suns.all.is_empty(), "a dome of no light");
    }
}
