// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2016 Eric Wasylishen
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/settings.cc, find_setting, set_setting, parseInternal, parse
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/entdata.cc, get, set, parse, EntData_Write
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/light.cc, worldspawn_keys, modelinfo_t, FixupGlobalSettings
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/entities.cc, light_t

//! The keys the extended dialect reads (LIGHT-9): an entity's pairs as ericw-tools keeps them,
//! each setting's value and whether the map set it, and the settings of the world, of a light
//! and of a brush model.

use qs_map::cnum::c_strtod;

use crate::extended::math::vec_from_mangle;
use crate::{AXES, Vec3, to_f32};

pub(crate) use crate::entities::FULL;
/// A colour whose channels are all at most this is a fraction of full (LIGHT-9.6).
const FRACTION_MAX: f32 = 1.0;
/// White.
pub(crate) const WHITE: Vec3 = [FULL; AXES];
/// `DEFAULTLIGHTLEVEL` (LIGHT-10.1).
pub(crate) const DEFAULT_LIGHT: f32 = 300.0;
/// `CHANNEL_MASK_DEFAULT`: the channel every light and model is on unless told otherwise.
pub(crate) const CHANNEL_DEFAULT: i32 = 1;
/// `INVALID_LIGHTSTYLE_OLD` less one: the highest style a light may ask for.
const MAX_STYLE: i32 = 254;
/// The numbers of a `light` value with a colour before it.
const LIGHT_WITH_COLOUR: usize = 4;

/// An entity's keys and values in the order read, each key once (`entdict_t`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Dict {
    pairs: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Dict {
    /// `entdict_t::get`: the value of `key`, empty when it has none.
    pub(crate) fn get(&self, key: &[u8]) -> &[u8] {
        self.pairs
            .iter()
            .find(|(k, _)| k == key)
            .map_or(&[], |(_, v)| v.as_slice())
    }

    /// `entdict_t::set`: the value of `key` replaced where it is, or the pair added last.
    pub(crate) fn set(&mut self, key: &[u8], value: Vec<u8>) {
        if let Some(pair) = self.pairs.iter_mut().find(|(k, _)| k == key) {
            pair.1 = value;
        } else {
            self.pairs.push((key.to_vec(), value));
        }
    }

    /// `entdict_t::remove`.
    pub(crate) fn remove(&mut self, key: &[u8]) {
        self.pairs.retain(|(k, _)| k != key);
    }

    /// The pairs, in order.
    pub(crate) fn pairs(&self) -> &[(Vec<u8>, Vec<u8>)] {
        &self.pairs
    }

    /// `entdict_t::get_int`: `atoi` of the value, 0 with none.
    pub(crate) fn int(&self, key: &[u8]) -> i32 {
        Value(self.get(key)).atoi()
    }

    /// `entdict_t::get_float`: `atof` of the value.
    pub(crate) fn float(&self, key: &[u8]) -> f32 {
        to_f32(c_strtod(self.get(key)).0)
    }

    /// `entdict_t::get_vector`: the numbers `sscanf("%f %f %f")` converts, the rest 0.
    pub(crate) fn vector(&self, key: &[u8]) -> Vec3 {
        let mut out = [0.0; AXES];
        let mut rest = self.get(key);
        for slot in &mut out {
            let (number, used) = c_strtod(rest);
            if used == 0 {
                break;
            }
            *slot = to_f32(number);
            rest = rest.get(used..).unwrap_or_default();
        }
        out
    }

    /// `EntData_Write`: the entity's lines between braces, its pairs in order (LIGHT-8.6).
    pub(crate) fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(b"{\n");
        for (key, value) in &self.pairs {
            out.push(b'"');
            out.extend_from_slice(key);
            out.extend_from_slice(b"\" \"");
            out.extend_from_slice(value);
            out.extend_from_slice(b"\"\n");
        }
        out.extend_from_slice(b"}\n");
    }
}

/// A key's value, read as ericw-tools' settings parse it: token by token, each a run of bytes
/// between white space (LIGHT-9.2).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Value<'a>(pub(crate) &'a [u8]);

impl<'a> Value<'a> {
    fn tokens(self) -> impl Iterator<Item = &'a [u8]> {
        self.0
            .split(u8::is_ascii_whitespace)
            .filter(|token| !token.is_empty())
    }

    /// `std::stod` of a token: the number its start converts to, none when no start does.
    fn number(token: &[u8]) -> Option<f32> {
        let (number, used) = c_strtod(token);
        (used != 0).then(|| to_f32(number))
    }

    /// A `setting_scalar`: the first token's number.
    fn scalar(self) -> Option<f32> {
        Self::number(self.tokens().next()?)
    }

    /// A `setting_int32`: `std::stoull` of the first token, cut to 32 bits, so that `-1` is -1.
    fn int(self) -> Option<i32> {
        let token = self.tokens().next()?;
        let (negative, digits) = match token {
            [b'-', rest @ ..] => (true, rest),
            [b'+', rest @ ..] => (false, rest),
            _ => (false, token),
        };
        let count = digits.iter().take_while(|c| c.is_ascii_digit()).count();
        if count == 0 {
            return None;
        }
        let mut total: u64 = 0;
        for digit in digits.iter().take(count) {
            total = total
                .wrapping_mul(DECIMAL)
                .wrapping_add(u64::from(digit.wrapping_sub(b'0')));
        }
        let signed = if negative {
            total.wrapping_neg()
        } else {
            total
        };
        Some(low_word(signed))
    }

    /// `atoi`: the digits at the start, 0 with none.
    fn atoi(self) -> i32 {
        Value(self.0.trim_ascii_start()).int().unwrap_or(0)
    }

    /// A `setting_bool`: `1` is on, `0` and `-1` off, and anything else, or nothing, on.
    fn flag(self) -> bool {
        !matches!(self.tokens().next(), Some(b"0" | b"-1"))
    }

    /// A `setting_vec3`: three tokens' numbers, none when one is missing or is no number.
    fn vec3(self) -> Option<Vec3> {
        let mut tokens = self.tokens();
        let mut out = [0.0; AXES];
        for slot in &mut out {
            *slot = Self::number(tokens.next()?)?;
        }
        Some(out)
    }

    /// A `setting_color`: three numbers, scaled by 255 when each is 0 to 1 (LIGHT-9.6).
    fn colour(self) -> Option<Vec3> {
        self.vec3().map(colour_of)
    }

    /// A `setting_mangle`: up to three numbers, the rest 0, as a direction (LIGHT-14.2).
    fn mangle(self) -> Vec3 {
        let mut out = [0.0; AXES];
        for (slot, token) in out.iter_mut().zip(self.tokens()) {
            match Self::number(token) {
                Some(number) => *slot = number,
                None => break,
            }
        }
        vec_from_mangle(out)
    }

    /// A `setting_string`: the first token.
    fn word(self) -> Option<Vec<u8>> {
        self.tokens().next().map(<[u8]>::to_vec)
    }
}

/// The base of a decimal number.
const DECIMAL: u64 = 10;

/// The low 32 bits of `value` as an `int`, as C's cast keeps them.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec LIGHT-9.2: ericw-tools casts std::stoull's value to int32_t"
)]
const fn low_word(value: u64) -> i32 {
    value as i32
}

/// `qv::normalize_color_format` (LIGHT-9.6).
pub(crate) fn colour_of(colour: Vec3) -> Vec3 {
    if colour.iter().all(|&c| (0.0..=FRACTION_MAX).contains(&c)) {
        colour.map(|c| c * FULL)
    } else {
        colour
    }
}

/// Who gave a setting its value; a later one of no less weight replaces it (LIGHT-9.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Source {
    /// Nothing: the setting has its default.
    #[default]
    Default,
    /// Another setting, as a `light` of four numbers gives the colour.
    Implied,
    /// A key of the map.
    Map,
}

/// A setting: its value and who gave it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Setting<T> {
    value: T,
    source: Source,
}

impl<T: Clone> Setting<T> {
    /// A setting at its default.
    pub(crate) const fn new(value: T) -> Self {
        Self {
            value,
            source: Source::Default,
        }
    }

    /// Its value.
    pub(crate) fn get(&self) -> T {
        self.value.clone()
    }

    /// Whether anything but its default gave its value (`is_changed`).
    pub(crate) fn is_changed(&self) -> bool {
        self.source != Source::Default
    }

    /// `set_value`: the value from `source`, unless a weightier source gave the one it has.
    pub(crate) fn set(&mut self, value: T, source: Source) {
        if source >= self.source {
            self.value = value;
            self.source = source;
        }
    }

    /// The value a key of the map gives, when it reads as one; whether it did.
    fn read(&mut self, value: Option<T>) -> bool {
        match value {
            Some(value) => {
                self.set(value, Source::Map);
                true
            }
            None => false,
        }
    }
}

impl Setting<f32> {
    /// A `setting_scalar` held between `low` and `high`.
    fn read_within(&mut self, value: Value<'_>, low: f32, high: f32) -> bool {
        self.read(value.scalar().map(|v| v.clamp(low, high)))
    }

    /// A `setting_scalar` with no bounds.
    fn read_scalar(&mut self, value: Value<'_>) -> bool {
        self.read(value.scalar())
    }

    /// Whether the value is over 0 (`boolValue`).
    pub(crate) fn is_on(&self) -> bool {
        self.value > 0.0
    }
}

/// A key without its leading underscores, as `find_setting` strips them (LIGHT-9.1).
fn bare(key: &[u8]) -> &[u8] {
    let underscores = key.iter().take_while(|&&c| c == b'_').count();
    key.get(underscores..).unwrap_or_default()
}

/// How a light's brightness falls with distance, `delay` (LIGHT-10.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Formula {
    /// `LF_LINEAR`, 0: the light less the distance.
    #[default]
    Linear,
    /// `LF_INVERSE`, 1.
    Inverse,
    /// `LF_INVERSE2`, 2.
    Inverse2,
    /// `LF_INFINITE`, 3: the same at any distance.
    Infinite,
    /// `LF_LOCALMIN`, 4: a minimum light within sight of the entity.
    LocalMin,
    /// `LF_INVERSE2A`, 5.
    Inverse2A,
    /// A number that names no formula: the light is put out (LIGHT-10.3).
    Unknown,
}

impl Formula {
    /// `setting_enum::parse`: a formula by its name or its number.
    fn parse(value: Value<'_>) -> Option<Self> {
        let token = value.tokens().next()?;
        Some(match token {
            b"linear" => Self::Linear,
            b"inverse" => Self::Inverse,
            b"inverse2" => Self::Inverse2,
            b"infinite" => Self::Infinite,
            b"localmin" => Self::LocalMin,
            b"inverse2a" => Self::Inverse2A,
            _ => match Value(token).int()? {
                0 => Self::Linear,
                1 => Self::Inverse,
                2 => Self::Inverse2,
                3 => Self::Infinite,
                4 => Self::LocalMin,
                5 => Self::Inverse2A,
                _ => Self::Unknown,
            },
        })
    }
}

/// Whether dirt applies: `dirt` of a light or model, 1, -1 or neither (LIGHT-9.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Dirt {
    /// Neither: as the world says.
    #[default]
    World,
    /// 1.
    On,
    /// -1.
    Off,
}

impl Dirt {
    #[expect(
        clippy::float_cmp,
        reason = "spec LIGHT-9.5: ericw-tools compares the dirt value with exactly 1 and -1"
    )]
    pub(crate) fn of(value: f32) -> Self {
        if value == 1.0 {
            Self::On
        } else if value == -1.0 {
            Self::Off
        } else {
            Self::World
        }
    }
}

/// The world's settings, `worldspawn_keys` (LIGHT-9.4).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct World {
    pub(crate) scaledist: Setting<f32>,
    pub(crate) rangescale: Setting<f32>,
    pub(crate) anglescale: Setting<f32>,
    pub(crate) gamma: Setting<f32>,
    pub(crate) addminlight: Setting<bool>,
    pub(crate) minlight: Setting<f32>,
    pub(crate) maxlight: Setting<f32>,
    pub(crate) minlight_color: Setting<Vec3>,
    pub(crate) spotlight_auto_falloff: Setting<bool>,
    pub(crate) style_start: Setting<i32>,
    pub(crate) style_max: Setting<i32>,
    pub(crate) dirt: Setting<bool>,
    pub(crate) dirtmode: Setting<f32>,
    pub(crate) dirtdepth: Setting<f32>,
    pub(crate) dirtscale: Setting<f32>,
    pub(crate) dirtgain: Setting<f32>,
    pub(crate) dirtangle: Setting<f32>,
    pub(crate) minlight_dirt: Setting<bool>,
    pub(crate) minlight_mottle: Setting<bool>,
    pub(crate) phong: Setting<bool>,
    pub(crate) phong_angle: Setting<f32>,
    pub(crate) bounce: Setting<i32>,
    pub(crate) bouncestyled: Setting<bool>,
    pub(crate) bouncescale: Setting<f32>,
    pub(crate) bouncecolorscale: Setting<f32>,
    pub(crate) bounce_subdivision: Setting<f32>,
    pub(crate) sunlight: Setting<f32>,
    pub(crate) sunlight_color: Setting<Vec3>,
    pub(crate) sun2: Setting<f32>,
    pub(crate) sun2_color: Setting<Vec3>,
    pub(crate) sunlight2: Setting<f32>,
    pub(crate) sunlight2_color: Setting<Vec3>,
    pub(crate) sunlight3: Setting<f32>,
    pub(crate) sunlight3_color: Setting<Vec3>,
    pub(crate) sunlight_dirt: Setting<f32>,
    pub(crate) sunlight2_dirt: Setting<f32>,
    pub(crate) sunvec: Setting<Vec3>,
    pub(crate) sun2vec: Setting<Vec3>,
    pub(crate) sun_deviance: Setting<f32>,
}

/// Straight down, the default of a sun's direction.
const DOWN: Vec3 = [0.0, 0.0, -1.0];
/// The widest a setting with a top of 100 goes.
const HUNDRED: f32 = 100.0;
/// The first style ericw-tools gives a named light, and the first it does not.
const STYLE_START: i32 = 32;
const STYLE_MAX: i32 = 64;
/// `dirtdepth`, `dirtangle`, `bouncelightsubdivision` and `sunlight_penumbra`'s defaults and
/// bounds.
const DIRT_DEPTH: f32 = 128.0;
const DIRT_ANGLE: f32 = 88.0;
const RIGHT_ANGLE: f32 = 90.0;
const BOUNCE_SUBDIVISION: f32 = 64.0;
const BOUNCE_SUBDIVISION_MAX: f32 = 8192.0;
const HALF_TURN: f32 = 180.0;
/// `global_anglescale`'s default.
const ANGLESCALE: f32 = 0.5;
/// `range`'s default.
const RANGESCALE: f32 = 0.5;
/// The most bounces.
const BOUNCE_MAX: i32 = 100;

impl Default for World {
    fn default() -> Self {
        Self {
            scaledist: Setting::new(1.0),
            rangescale: Setting::new(RANGESCALE),
            anglescale: Setting::new(ANGLESCALE),
            gamma: Setting::new(1.0),
            addminlight: Setting::new(false),
            minlight: Setting::new(0.0),
            maxlight: Setting::new(0.0),
            minlight_color: Setting::new(WHITE),
            spotlight_auto_falloff: Setting::new(false),
            style_start: Setting::new(STYLE_START),
            style_max: Setting::new(STYLE_MAX),
            dirt: Setting::new(false),
            dirtmode: Setting::new(0.0),
            dirtdepth: Setting::new(DIRT_DEPTH),
            dirtscale: Setting::new(1.0),
            dirtgain: Setting::new(1.0),
            dirtangle: Setting::new(DIRT_ANGLE),
            minlight_dirt: Setting::new(false),
            minlight_mottle: Setting::new(false),
            phong: Setting::new(true),
            phong_angle: Setting::new(0.0),
            bounce: Setting::new(0),
            bouncestyled: Setting::new(false),
            bouncescale: Setting::new(1.0),
            bouncecolorscale: Setting::new(0.0),
            bounce_subdivision: Setting::new(BOUNCE_SUBDIVISION),
            sunlight: Setting::new(0.0),
            sunlight_color: Setting::new(WHITE),
            sun2: Setting::new(0.0),
            sun2_color: Setting::new(WHITE),
            sunlight2: Setting::new(0.0),
            sunlight2_color: Setting::new(WHITE),
            sunlight3: Setting::new(0.0),
            sunlight3_color: Setting::new(WHITE),
            sunlight_dirt: Setting::new(0.0),
            sunlight2_dirt: Setting::new(0.0),
            sunvec: Setting::new(DOWN),
            sun2vec: Setting::new(DOWN),
            sun_deviance: Setting::new(0.0),
        }
    }
}

impl World {
    /// The world's settings from the worldspawn's keys, with `FixupGlobalSettings`'s side
    /// effects of `dirt` (LIGHT-9.4, LIGHT-9.5); the keys whose values do not read, for the
    /// warning.
    pub(crate) fn read(dict: &Dict) -> (Self, Vec<String>) {
        let mut world = Self::default();
        let mut invalid = Vec::new();
        for (key, value) in dict.pairs() {
            if !world.set(bare(key), Value(value)) {
                invalid.push(String::from_utf8_lossy(key).into_owned());
            }
        }
        if world.dirt.get() {
            if !world.minlight_dirt.is_changed() {
                world.minlight_dirt.set(true, Source::Map);
            }
            if !world.sunlight_dirt.is_changed() {
                world.sunlight_dirt.set(1.0, Source::Map);
            }
            if !world.sunlight2_dirt.is_changed() {
                world.sunlight2_dirt.set(1.0, Source::Map);
            }
        }
        (world, invalid)
    }

    /// One key: whether it is no setting of the world's, or read as one.
    fn set(&mut self, name: &[u8], value: Value<'_>) -> bool {
        match name {
            b"dist" => self.scaledist.read_within(value, 0.0, HUNDRED),
            b"range" => self.rangescale.read_within(value, 0.0, HUNDRED),
            b"anglescale" | b"anglesense" => self.anglescale.read_within(value, 0.0, 1.0),
            b"gamma" => self.gamma.read_within(value, 0.0, HUNDRED),
            b"addmin" => self.addminlight.read(Some(value.flag())),
            b"light" | b"minlight" => self.minlight.read_scalar(value),
            b"maxlight" => self.maxlight.read_scalar(value),
            b"minlight_color" | b"mincolor" => self.minlight_color.read(value.colour()),
            b"spotlightautofalloff" => self.spotlight_auto_falloff.read(Some(value.flag())),
            b"compilerstyle_start" => self.style_start.read(value.int()),
            b"compilerstyle_max" => self.style_max.read(value.int()),
            b"dirt" | b"dirty" => self.dirt.read(Some(value.flag())),
            b"dirtmode" => self.dirtmode.read_scalar(value),
            b"dirtdepth" => self.dirtdepth.read_within(value, 1.0, f32::INFINITY),
            b"dirtscale" => self.dirtscale.read_within(value, 0.0, HUNDRED),
            b"dirtgain" => self.dirtgain.read_within(value, 0.0, HUNDRED),
            b"dirtangle" => self.dirtangle.read_within(value, 1.0, RIGHT_ANGLE),
            b"minlight_dirt" => self.minlight_dirt.read(Some(value.flag())),
            b"minlight_mottle" | b"minlightMottle" => self.minlight_mottle.read(Some(value.flag())),
            b"phong" => self.phong.read(Some(value.flag())),
            b"phong_angle" => self.phong_angle.read_scalar(value),
            // `bounce` may be given with no number, which is 1.
            b"bounce" => self
                .bounce
                .read(Some(value.int().unwrap_or(1).clamp(0, BOUNCE_MAX))),
            b"bouncestyled" => self.bouncestyled.read(Some(value.flag())),
            b"bouncescale" => self.bouncescale.read_within(value, 0.0, HUNDRED),
            b"bouncecolorscale" => self.bouncecolorscale.read_within(value, 0.0, 1.0),
            b"bouncelightsubdivision" => {
                self.bounce_subdivision
                    .read_within(value, 1.0, BOUNCE_SUBDIVISION_MAX)
            }
            b"sunlight" | b"sun_light" => self.sunlight.read_scalar(value),
            b"sunlight_color" | b"sun_color" => self.sunlight_color.read(value.colour()),
            b"sun2" => self.sun2.read_scalar(value),
            b"sun2_color" => self.sun2_color.read(value.colour()),
            b"sunlight2" => self.sunlight2.read_scalar(value),
            b"sunlight2_color" | b"sunlight_color2" => self.sunlight2_color.read(value.colour()),
            b"sunlight3" => self.sunlight3.read_scalar(value),
            b"sunlight3_color" | b"sunlight_color3" => self.sunlight3_color.read(value.colour()),
            b"sunlight_dirt" => self.sunlight_dirt.read_scalar(value),
            b"sunlight2_dirt" => self.sunlight2_dirt.read_scalar(value),
            b"sunlight_mangle" | b"sun_mangle" | b"sun_angle" => {
                self.sunvec.read(Some(value.mangle()))
            }
            b"sun2_mangle" => self.sun2vec.read(Some(value.mangle())),
            b"sunlight_penumbra" => self.sun_deviance.read_within(value, 0.0, HALF_TURN),
            _ => true,
        }
    }
}

/// A light entity's settings, `light_t` (LIGHT-10).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LightKeys {
    pub(crate) light: Setting<f32>,
    pub(crate) atten: Setting<f32>,
    pub(crate) formula: Setting<Formula>,
    pub(crate) spotangle: Setting<f32>,
    pub(crate) spotangle2: Setting<f32>,
    pub(crate) style: Setting<i32>,
    pub(crate) anglescale: Setting<f32>,
    pub(crate) dirtscale: Setting<f32>,
    pub(crate) dirtgain: Setting<f32>,
    pub(crate) dirt: Setting<f32>,
    pub(crate) deviance: Setting<f32>,
    pub(crate) samples: Setting<i32>,
    pub(crate) bouncescale: Setting<f32>,
    pub(crate) dirt_off_radius: Setting<f32>,
    pub(crate) dirt_on_radius: Setting<f32>,
    pub(crate) sun: Setting<bool>,
    pub(crate) sunlight2: Setting<bool>,
    pub(crate) sunlight3: Setting<bool>,
    pub(crate) falloff: Setting<f32>,
    pub(crate) bleed: Setting<bool>,
    pub(crate) origin: Setting<Vec3>,
    pub(crate) color: Setting<Vec3>,
    /// `mangle`'s three numbers as given, not yet a direction.
    pub(crate) mangle: Setting<Vec3>,
    pub(crate) nostaticlight: Setting<bool>,
    pub(crate) surflight_group: Setting<i32>,
    pub(crate) light_channel_mask: Setting<i32>,
    pub(crate) shadow_channel_mask: Setting<i32>,
    pub(crate) nonudge: Setting<bool>,
    pub(crate) switchableshadow_target: Setting<Vec<u8>>,
}

/// `angle`'s default: a spotlight's cone in degrees.
const SPOT_ANGLE: f32 = 40.0;
/// `samples`' default.
const SAMPLES: i32 = 16;

impl Default for LightKeys {
    fn default() -> Self {
        Self {
            light: Setting::new(DEFAULT_LIGHT),
            atten: Setting::new(1.0),
            formula: Setting::new(Formula::Linear),
            spotangle: Setting::new(SPOT_ANGLE),
            spotangle2: Setting::new(0.0),
            style: Setting::new(0),
            anglescale: Setting::new(-1.0),
            dirtscale: Setting::new(0.0),
            dirtgain: Setting::new(0.0),
            dirt: Setting::new(0.0),
            deviance: Setting::new(0.0),
            samples: Setting::new(SAMPLES),
            bouncescale: Setting::new(1.0),
            dirt_off_radius: Setting::new(0.0),
            dirt_on_radius: Setting::new(0.0),
            sun: Setting::new(false),
            sunlight2: Setting::new(false),
            sunlight3: Setting::new(false),
            falloff: Setting::new(0.0),
            bleed: Setting::new(false),
            origin: Setting::new([0.0; AXES]),
            color: Setting::new(WHITE),
            mangle: Setting::new([0.0; AXES]),
            nostaticlight: Setting::new(false),
            surflight_group: Setting::new(0),
            light_channel_mask: Setting::new(CHANNEL_DEFAULT),
            shadow_channel_mask: Setting::new(CHANNEL_DEFAULT),
            nonudge: Setting::new(false),
            switchableshadow_target: Setting::new(Vec::new()),
        }
    }
}

impl LightKeys {
    /// A light's settings from its entity's keys, in the keys' order (LIGHT-10.1).
    pub(crate) fn read(dict: &Dict) -> Self {
        let mut keys = Self::default();
        for (key, value) in dict.pairs() {
            keys.set(bare(key), Value(value));
        }
        keys
    }

    /// `setting_light::parse`: one number is the brightness; four are a colour and the
    /// brightness; three are a colour, the brightness back at its default (LIGHT-10.1).
    fn set_light(&mut self, value: Value<'_>) {
        let mut numbers = [0.0; LIGHT_WITH_COLOUR];
        let mut count = 0_usize;
        for (slot, token) in numbers.iter_mut().zip(value.tokens()) {
            let Some(number) = Value::number(token) else {
                return;
            };
            *slot = number;
            count = count.saturating_add(1);
        }
        let [r, g, b, brightness] = numbers;
        match count {
            LIGHT_WITH_COLOUR => {
                self.color.set(colour_of([r, g, b]), Source::Implied);
                self.light.set(brightness, Source::Map);
            }
            AXES => {
                self.color.set(colour_of([r, g, b]), Source::Implied);
                self.light = Setting::new(DEFAULT_LIGHT);
            }
            1 => self.light.set(r, Source::Map),
            _ => {}
        }
    }

    fn set(&mut self, name: &[u8], value: Value<'_>) {
        match name {
            b"light" => self.set_light(value),
            b"wait" => _ = self.atten.read_within(value, 0.0, f32::MAX),
            b"delay" => _ = self.formula.read(Formula::parse(value)),
            b"angle" => _ = self.spotangle.read_scalar(value),
            b"softangle" => _ = self.spotangle2.read_scalar(value),
            b"style" => _ = self.style.read(value.int().map(|v| v.clamp(0, MAX_STYLE))),
            b"anglesense" | b"anglescale" => _ = self.anglescale.read_scalar(value),
            b"dirtscale" => _ = self.dirtscale.read_scalar(value),
            b"dirtgain" => _ = self.dirtgain.read_scalar(value),
            b"dirt" => _ = self.dirt.read_scalar(value),
            b"deviance" => _ = self.deviance.read_scalar(value),
            b"samples" => _ = self.samples.read(value.int().map(|v| v.max(0))),
            b"bouncescale" => _ = self.bouncescale.read_scalar(value),
            b"dirt_off_radius" => _ = self.dirt_off_radius.read_scalar(value),
            b"dirt_on_radius" => _ = self.dirt_on_radius.read_scalar(value),
            b"sun" => _ = self.sun.read(Some(value.flag())),
            b"sunlight2" => _ = self.sunlight2.read(Some(value.flag())),
            b"sunlight3" => _ = self.sunlight3.read(Some(value.flag())),
            b"falloff" => _ = self.falloff.read_within(value, 0.0, f32::MAX),
            b"bleed" => _ = self.bleed.read(Some(value.flag())),
            b"origin" => _ = self.origin.read(value.vec3()),
            b"color" => _ = self.color.read(value.colour()),
            b"mangle" => _ = self.mangle.read(value.vec3()),
            b"nostaticlight" => _ = self.nostaticlight.read(Some(value.flag())),
            b"surflight_group" => _ = self.surflight_group.read(value.int()),
            b"light_channel_mask" => _ = self.light_channel_mask.read(value.int()),
            b"shadow_channel_mask" => _ = self.shadow_channel_mask.read(value.int()),
            b"nonudge" => _ = self.nonudge.read(Some(value.flag())),
            b"switchableshadow_target" => _ = self.switchableshadow_target.read(value.word()),
            _ => {}
        }
    }

    /// Whether dirt applies to the light.
    pub(crate) fn dirt(&self) -> Dirt {
        Dirt::of(self.dirt.get())
    }
}

/// A brush model's settings, `modelinfo_t` (LIGHT-9.7).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ModelKeys {
    pub(crate) minlight: Setting<f32>,
    pub(crate) maxlight: Setting<f32>,
    pub(crate) shadow: Setting<f32>,
    pub(crate) shadowself: Setting<f32>,
    pub(crate) shadowworldonly: Setting<f32>,
    pub(crate) switchableshadow: Setting<f32>,
    pub(crate) switchshadstyle: Setting<i32>,
    pub(crate) dirt: Setting<f32>,
    pub(crate) phong: Setting<f32>,
    pub(crate) phong_angle: Setting<f32>,
    pub(crate) alpha: Setting<f32>,
    pub(crate) minlight_color: Setting<Vec3>,
    pub(crate) lightignore: Setting<bool>,
    pub(crate) lightcolorscale: Setting<f32>,
    pub(crate) object_channel_mask: Setting<i32>,
    pub(crate) autominlight: Setting<bool>,
    pub(crate) autominlight_target: Setting<Vec<u8>>,
    pub(crate) minlight_mottle: Setting<bool>,
}

impl Default for ModelKeys {
    fn default() -> Self {
        Self {
            minlight: Setting::new(0.0),
            maxlight: Setting::new(0.0),
            shadow: Setting::new(0.0),
            shadowself: Setting::new(0.0),
            shadowworldonly: Setting::new(0.0),
            switchableshadow: Setting::new(0.0),
            switchshadstyle: Setting::new(0),
            dirt: Setting::new(0.0),
            phong: Setting::new(0.0),
            phong_angle: Setting::new(0.0),
            alpha: Setting::new(1.0),
            minlight_color: Setting::new(WHITE),
            lightignore: Setting::new(false),
            lightcolorscale: Setting::new(1.0),
            object_channel_mask: Setting::new(CHANNEL_DEFAULT),
            autominlight: Setting::new(false),
            autominlight_target: Setting::new(Vec::new()),
            minlight_mottle: Setting::new(false),
        }
    }
}

/// `DEFAULT_PHONG_ANGLE`: the angle of a model with `_phong` and no `_phong_angle`.
pub(crate) const DEFAULT_PHONG_ANGLE: f32 = 89.0;

impl ModelKeys {
    /// A model's settings from its entity's keys (LIGHT-9.7).
    pub(crate) fn read(dict: &Dict) -> Self {
        let mut keys = Self::default();
        for (key, value) in dict.pairs() {
            keys.set(bare(key), Value(value));
        }
        keys
    }

    fn set(&mut self, name: &[u8], value: Value<'_>) {
        match name {
            b"minlight" => _ = self.minlight.read_scalar(value),
            b"maxlight" => _ = self.maxlight.read_scalar(value),
            b"shadow" => _ = self.shadow.read_scalar(value),
            b"shadowself" | b"selfshadow" => _ = self.shadowself.read_scalar(value),
            b"shadowworldonly" => _ = self.shadowworldonly.read_scalar(value),
            b"switchableshadow" => _ = self.switchableshadow.read_scalar(value),
            b"switchshadstyle" => _ = self.switchshadstyle.read(value.int()),
            b"dirt" => _ = self.dirt.read_scalar(value),
            b"phong" => _ = self.phong.read_scalar(value),
            b"phong_angle" => _ = self.phong_angle.read_scalar(value),
            b"alpha" => _ = self.alpha.read_scalar(value),
            b"minlight_color" | b"mincolor" => _ = self.minlight_color.read(value.colour()),
            b"lightignore" => _ = self.lightignore.read(Some(value.flag())),
            b"lightcolorscale" => _ = self.lightcolorscale.read_scalar(value),
            b"object_channel_mask" => _ = self.object_channel_mask.read(value.int()),
            b"autominlight" => _ = self.autominlight.read(Some(value.flag())),
            b"autominlight_target" => _ = self.autominlight_target.read(value.word()),
            b"minlight_mottle" | b"minlightMottle" => {
                _ = self.minlight_mottle.read(Some(value.flag()));
            }
            _ => {}
        }
    }

    /// `getResolvedPhongAngle`: `_phong_angle`, or 89 with `_phong` over 0, or 0 (LIGHT-13.1).
    pub(crate) fn resolved_phong_angle(&self) -> f32 {
        let angle = self.phong_angle.get();
        if angle != 0.0 {
            angle
        } else if self.phong.is_on() {
            DEFAULT_PHONG_ANGLE
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    fn dict(pairs: &[(&str, &str)]) -> Dict {
        let mut dict = Dict::default();
        for (key, value) in pairs {
            dict.set(key.as_bytes(), value.as_bytes().to_vec());
        }
        dict
    }

    // spec LIGHT-9.1: a key names a setting with any leading underscores taken off.
    #[test]
    fn a_key_names_its_setting_with_or_without_underscores() {
        let world = World::read(&dict(&[
            ("_sunlight", "200"),
            ("__dist", "2"),
            ("range", "1"),
        ]))
        .0;
        assert_eq!(world.sunlight.get(), 200.0);
        assert_eq!(world.scaledist.get(), 2.0);
        assert_eq!(world.rangescale.get(), 1.0);
        assert!(world.sunlight.is_changed());
        assert!(!world.sunlight2.is_changed());
    }

    // spec LIGHT-9.2: a number is the start of the first token that converts; a flag is on
    // unless its token is 0 or -1.
    #[test]
    fn values_read_as_ericw_tools_settings_parse_them() {
        assert_eq!(Value(b" 12.5abc 4").scalar(), Some(12.5));
        assert_eq!(Value(b"abc").scalar(), None);
        assert_eq!(Value(b"").scalar(), None);
        assert_eq!(Value(b"-1").int(), Some(-1));
        assert_eq!(Value(b"+7x").int(), Some(7));
        assert_eq!(Value(b"x").int(), None);
        assert!(Value(b"1").flag());
        assert!(Value(b"2").flag());
        assert!(Value(b"").flag());
        assert!(!Value(b"0").flag());
        assert!(!Value(b"-1").flag());
        assert_eq!(Value(b"1 2").vec3(), None);
        assert_eq!(Value(b"1 2 3 4").vec3(), Some([1.0, 2.0, 3.0]));
        assert_eq!(Value(b"tex name").word(), Some(b"tex".to_vec()));
        assert_eq!(Value(b" 42").atoi(), 42);
        assert_eq!(Value(b"none").atoi(), 0);
    }

    // spec LIGHT-9.4: a world key whose value does not read is named and leaves the default;
    // a value past a setting's bounds is held at them.
    #[test]
    fn a_world_key_that_does_not_read_is_reported_and_bounds_hold() {
        let (world, invalid) = World::read(&dict(&[
            ("_sunlight", "bright"),
            ("_anglescale", "3"),
            ("_sunlight_penumbra", "400"),
            ("_bounce", ""),
            ("classname", "worldspawn"),
        ]));
        assert_eq!(invalid, ["_sunlight"]);
        assert_eq!(world.sunlight.get(), 0.0);
        assert_eq!(world.anglescale.get(), 1.0);
        assert_eq!(world.sun_deviance.get(), 180.0);
        assert_eq!(world.bounce.get(), 1, "bounce with no number is 1");
    }

    // spec LIGHT-9.5: dirt on the world turns dirt on for minimum light and both suns, unless
    // the map set them.
    #[test]
    fn world_dirt_implies_dirt_on_minlight_and_sunlight() {
        let (world, _) = World::read(&dict(&[("_dirt", "1"), ("_sunlight2_dirt", "-1")]));
        assert!(world.minlight_dirt.get());
        assert_eq!(world.sunlight_dirt.get(), 1.0);
        assert_eq!(world.sunlight2_dirt.get(), -1.0);
        let (plain, _) = World::read(&dict(&[]));
        assert!(!plain.minlight_dirt.get());
        assert_eq!(plain.sunlight_dirt.get(), 0.0);
    }

    // spec LIGHT-9.6: a colour whose three numbers are each 0 to 1 is scaled by 255.
    #[test]
    fn a_colour_of_fractions_is_scaled_to_bytes() {
        assert_eq!(colour_of([1.0, 0.5, 0.0]), [255.0, 127.5, 0.0]);
        assert_eq!(colour_of([255.0, 0.5, 0.0]), [255.0, 0.5, 0.0]);
        assert_eq!(colour_of([-0.1, 0.5, 0.0]), [-0.1, 0.5, 0.0]);
    }

    // spec LIGHT-10.1: light is a brightness, or a colour and a brightness, or a colour alone.
    // spec LIGHT-9.3: _color, a key of the map, outweighs the colour a light value implies,
    // whichever comes first.
    #[test]
    fn a_light_value_may_carry_its_colour() {
        let plain = LightKeys::read(&dict(&[("light", "150")]));
        assert_eq!(plain.light.get(), 150.0);
        assert!(plain.light.is_changed());
        let four = LightKeys::read(&dict(&[("_light", "255 128 0 400")]));
        assert_eq!(four.light.get(), 400.0);
        assert_eq!(four.color.get(), [255.0, 128.0, 0.0]);
        let three = LightKeys::read(&dict(&[("light", "1 0 0")]));
        assert_eq!(three.light.get(), 300.0);
        assert!(!three.light.is_changed());
        assert_eq!(three.color.get(), [255.0, 0.0, 0.0]);
        let both = LightKeys::read(&dict(&[("_color", "0 0 1"), ("light", "1 0 0 200")]));
        assert_eq!(both.color.get(), [0.0, 0.0, 255.0]);
        let unread = LightKeys::read(&dict(&[("light", "1 x")]));
        assert!(!unread.light.is_changed());
        assert!(
            !LightKeys::read(&dict(&[("light", "1 2")]))
                .light
                .is_changed()
        );
    }

    // spec LIGHT-10.3: delay names a formula by number or by name; a number that names none
    // is unknown.
    #[test]
    fn delay_is_a_formula_by_number_or_name() {
        let of = |text: &str| LightKeys::read(&dict(&[("delay", text)])).formula.get();
        assert_eq!(of("0"), Formula::Linear);
        assert_eq!(of("1"), Formula::Inverse);
        assert_eq!(of("2"), Formula::Inverse2);
        assert_eq!(of("3"), Formula::Infinite);
        assert_eq!(of("4"), Formula::LocalMin);
        assert_eq!(of("5"), Formula::Inverse2A);
        assert_eq!(of("inverse2a"), Formula::Inverse2A);
        assert_eq!(of("9"), Formula::Unknown);
        assert_eq!(of("fast"), Formula::Linear, "a word that is no formula");
        assert_eq!(LightKeys::read(&dict(&[("style", "999")])).style.get(), 254);
        assert_eq!(LightKeys::read(&dict(&[("_dirt", "-1")])).dirt(), Dirt::Off);
        assert_eq!(LightKeys::read(&dict(&[("_dirt", "1")])).dirt(), Dirt::On);
        assert_eq!(LightKeys::default().dirt(), Dirt::World);
    }

    // spec LIGHT-13.1: a model's phong angle is _phong_angle, or 89 with _phong.
    #[test]
    fn a_models_phong_angle_is_its_key_or_the_default() {
        let angle = |pairs: &[(&str, &str)]| ModelKeys::read(&dict(pairs)).resolved_phong_angle();
        assert_eq!(angle(&[("_phong", "1")]), 89.0);
        assert_eq!(angle(&[("_phong", "1"), ("_phong_angle", "45")]), 45.0);
        assert_eq!(angle(&[("_phong_angle", "30")]), 30.0);
        assert_eq!(angle(&[("_phong", "0")]), 0.0);
        let keys = ModelKeys::read(&dict(&[("_shadow", "1"), ("alpha", "0.5")]));
        assert!(keys.shadow.is_on());
        assert_eq!(keys.alpha.get(), 0.5);
        assert!(keys.alpha.is_changed());
    }

    // spec LIGHT-8.6: an entity is written with its keys in the order read, a key set again
    // keeping its place.
    #[test]
    fn a_dict_keeps_its_keys_in_order() {
        let mut entity = dict(&[("classname", "light"), ("origin", "1 2 3")]);
        entity.set(b"classname", b"light_torch".to_vec());
        entity.set(b"style", b"32".to_vec());
        let mut out = Vec::new();
        entity.write(&mut out);
        assert_eq!(
            out,
            b"{\n\"classname\" \"light_torch\"\n\"origin\" \"1 2 3\"\n\"style\" \"32\"\n}\n"
        );
        assert_eq!(entity.vector(b"origin"), [1.0, 2.0, 3.0]);
        assert_eq!(entity.vector(b"style"), [32.0, 0.0, 0.0]);
        assert_eq!(entity.int(b"style"), 32);
        assert_eq!(entity.float(b"style"), 32.0);
        assert_eq!(entity.get(b"missing"), b"");
        entity.remove(b"origin");
        assert_eq!(entity.pairs().len(), 2);
    }
}
