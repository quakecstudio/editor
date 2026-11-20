// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/common.c, MSG_WriteCoord, MSG_WriteAngle, MSG_ReadCoord, MSG_ReadAngle
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/common.c, MSG_WriteCoord16, MSG_WriteCoord24, MSG_WriteCoord, MSG_WriteAngle, MSG_WriteAngle16, MSG_ReadCoord16, MSG_ReadCoord24, MSG_ReadCoord, MSG_ReadAngle, MSG_ReadAngle16

//! The network protocols the server writes and the client reads (NET-6): id's 15,
//! FitzQuake's 666 and RMQ's 999, and how each sends a coordinate and an angle.

use qc_vm::convert::{double_to_float, double_to_int, float_to_int};

/// `PROTOCOL_NETQUAKE`, id's `PROTOCOL_VERSION` (NET-2.2).
pub const NET_PROTOCOL_VERSION: i32 = 15;
/// `PROTOCOL_FITZQUAKE` (NET-6.1).
pub const PROTOCOL_FITZQUAKE: i32 = 666;
/// `PROTOCOL_RMQ` (NET-6.1).
pub const PROTOCOL_RMQ: i32 = 999;

/// `svc_spawnbaseline2`, `svc_spawnstatic2` and `svc_spawnstaticsound2` (NET-6.4, NET-6.5).
pub const SVC_SPAWNBASELINE2: i32 = 42;
pub const SVC_SPAWNSTATIC2: i32 = 43;
pub const SVC_SPAWNSTATICSOUND2: i32 = 44;

/// FitzQuake's entity update bits (NET-6.3): `U_EXTEND1` and `U_EXTEND2` say a third and a
/// fourth byte of bits follow.
pub const U_EXTEND1: i32 = 1 << 15;
pub const U_ALPHA: i32 = 1 << 16;
pub const U_FRAME2: i32 = 1 << 17;
pub const U_MODEL2: i32 = 1 << 18;
pub const U_LERPFINISH: i32 = 1 << 19;
pub const U_SCALE: i32 = 1 << 20;
pub const U_EXTEND2: i32 = 1 << 23;
/// Nehahra's `U_TRANS`, bit 15 of a protocol 15 update (CLIENT-9.3).
pub const U_TRANS: i32 = 1 << 15;

/// FitzQuake's client data bits (NET-6.6).
pub const SU_EXTEND1: i32 = 1 << 15;
pub const SU_WEAPON2: i32 = 1 << 16;
pub const SU_ARMOR2: i32 = 1 << 17;
pub const SU_AMMO2: i32 = 1 << 18;
pub const SU_SHELLS2: i32 = 1 << 19;
pub const SU_NAILS2: i32 = 1 << 20;
pub const SU_ROCKETS2: i32 = 1 << 21;
pub const SU_CELLS2: i32 = 1 << 22;
pub const SU_EXTEND2: i32 = 1 << 23;
pub const SU_WEAPONFRAME2: i32 = 1 << 24;
pub const SU_WEAPONALPHA: i32 = 1 << 25;

/// FitzQuake's sound bits (NET-6.7).
pub const SND_LARGEENTITY: i32 = 1 << 3;
pub const SND_LARGESOUND: i32 = 1 << 4;

/// FitzQuake's baseline bits (NET-6.4).
pub const B_LARGEMODEL: i32 = 1 << 0;
pub const B_LARGEFRAME: i32 = 1 << 1;
pub const B_ALPHA: i32 = 1 << 2;
pub const B_SCALE: i32 = 1 << 3;

/// The bits past the first byte and past the second, `bits >= 65536` and `bits >= 16777216`
/// (NET-6.3).
pub const THIRD_BYTE: i32 = 1 << 16;
pub const FOURTH_BYTE: i32 = 1 << 24;
/// The shifts of the third and fourth bytes of bits.
pub const THIRD_SHIFT: i32 = 16;
pub const FOURTH_SHIFT: i32 = 24;
/// `& 0xFF00`: a value past a byte, and `>> 8` its high byte (NET-6.3).
pub const HIGH_BYTE: i32 = 0xff00;
pub const HIGH_SHIFT: i32 = 8;

/// `ENTALPHA_DEFAULT`, `ENTALPHA_ZERO` and `ENTALPHA_ONE` (NET-6.8).
pub const ENTALPHA_DEFAULT: u8 = 0;
pub const ENTALPHA_ZERO: u8 = 1;
pub const ENTALPHA_ONE: u8 = 255;
/// `ENTSCALE_DEFAULT`, "Equivalent to float 1.0f due to byte packing" (NET-6.8).
pub const ENTSCALE_DEFAULT: u8 = 16;
/// `ENTALPHA_ENCODE`'s 254 steps above `ENTALPHA_ZERO` (NET-6.8).
const ALPHA_STEPS: f32 = 254.0;

/// An entity's alpha as the protocol sends it, `ENTALPHA_ENCODE` and `ENTALPHA_DECODE`
/// (NET-6.8): 0 for the default, else 1 (invisible) to 255 (opaque).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EntAlpha(u8);

impl EntAlpha {
    /// `ENTALPHA_DEFAULT`: drawn as the entity is drawn with no alpha.
    pub const DEFAULT: Self = Self(ENTALPHA_DEFAULT);

    /// The byte `byte`, as a message carries it.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Self {
        Self(byte)
    }

    /// The byte.
    #[must_use]
    pub const fn byte(self) -> u8 {
        self.0
    }

    /// `ENTALPHA_ENCODE(a)`: "`(((a)==0)?ENTALPHA_DEFAULT:Q_rint(CLAMP(1.0f,(a)*254.0f+1,255.0f)))`".
    #[must_use]
    pub fn encode(a: f32) -> Self {
        if a == 0.0 {
            return Self::DEFAULT;
        }
        let clamped = (a * ALPHA_STEPS + 1.0).clamp(1.0, f32::from(ENTALPHA_ONE));
        Self(u8::try_from(q_rint(clamped)).unwrap_or(ENTALPHA_ONE))
    }

    /// `ENTALPHA_DECODE(a)`: "`(((a)==ENTALPHA_DEFAULT)?1.0f:((float)(a)-1)/(254))`".
    #[must_use]
    pub fn decode(self) -> f32 {
        if self.0 == ENTALPHA_DEFAULT {
            1.0
        } else {
            (f32::from(self.0) - 1.0) / ALPHA_STEPS
        }
    }

    /// `ENTALPHA_TOSAVE(a)`: "`(((a)==ENTALPHA_DEFAULT)?0.0f:(((a)==ENTALPHA_ZERO)?-1.0f:((float)(a)-1)/(254)))`",
    /// the value a savegame keeps (NET-6.8).
    #[must_use]
    pub fn to_save(self) -> f32 {
        match self.0 {
            ENTALPHA_DEFAULT => 0.0,
            ENTALPHA_ZERO => -1.0,
            byte => (f32::from(byte) - 1.0) / ALPHA_STEPS,
        }
    }

    /// Whether it is `ENTALPHA_ZERO`, an invisible entity the server does not send.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == ENTALPHA_ZERO
    }
}

/// An entity's scale as the protocol sends it, `ENTSCALE_ENCODE` and `ENTSCALE_DECODE`
/// (NET-6.8): sixteenths, 16 being 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntScale(u8);

impl Default for EntScale {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl EntScale {
    /// `ENTSCALE_DEFAULT`, a scale of 1.
    pub const DEFAULT: Self = Self(ENTSCALE_DEFAULT);

    /// The byte `byte`, as a message carries it.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Self {
        Self(byte)
    }

    /// The byte.
    #[must_use]
    pub const fn byte(self) -> u8 {
        self.0
    }

    /// `ENTSCALE_ENCODE(a)`: "`((a) ? ((a) * ENTSCALE_DEFAULT) : ENTSCALE_DEFAULT)`", stored in
    /// the `unsigned char` `scale`: the float converted to an int, its low byte kept, as x86
    /// does for a value past a byte, which C leaves undefined.
    #[must_use]
    pub fn encode(a: f32) -> Self {
        if a == 0.0 {
            return Self::DEFAULT;
        }
        let [low, ..] = float_to_int(a * f32::from(ENTSCALE_DEFAULT)).to_le_bytes();
        Self(low)
    }

    /// `ENTSCALE_DECODE(a)`: "`((float)(a) / ENTSCALE_DEFAULT)`".
    #[must_use]
    pub fn decode(self) -> f32 {
        f32::from(self.0) / f32::from(ENTSCALE_DEFAULT)
    }
}

/// `MSG_WriteCoord16`'s and id's eighths of a unit (NET-1.1, NET-6.2).
const COORD16_SCALE: f32 = 8.0;
/// `PRFL_INT32COORD`'s sixteenths of a unit, `Q_rint (f * 16)` (NET-6.2).
const COORD32_SCALE: f32 = 16.0;
/// `MSG_WriteCoord24`'s fraction byte, `(int)(f*255)%255` (NET-6.2).
const COORD24_FRACTION: f32 = 255.0;
const COORD24_MODULUS: i32 = 255;
/// A turn in degrees, and id's `f * 256 / 360` in ints (NET-1.1).
const FULL_TURN: f64 = 360.0;
const ID_FULL_TURN: i32 = 360;
const ID_ANGLE_STEPS: i32 = 256;
/// The steps of a byte angle and of a short angle (NET-6.2).
const BYTE_ANGLE_STEPS: f64 = 256.0;
const SHORT_ANGLE_STEPS: f64 = 65536.0;
/// The low byte and the low 16 bits.
const BYTE_MASK: i32 = 0xff;
const SHORT_MASK: i32 = 0xffff;
/// `Q_rint`'s half, the double `0.5` (NET-6.2).
const HALF_F64: f64 = 0.5;

/// Which protocol a server speaks, `sv.protocol` (NET-6.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProtocolVersion {
    /// 15, id's: what id's engine writes and reads.
    NetQuake,
    /// 666, FitzQuake's, QuakeSpasm's default (`int sv_protocol = PROTOCOL_FITZQUAKE`).
    #[default]
    FitzQuake,
    /// 999, RMQ's, with its protocol flags.
    Rmq,
}

impl ProtocolVersion {
    /// The number `svc_serverinfo` sends.
    #[must_use]
    pub const fn number(self) -> i32 {
        match self {
            Self::NetQuake => NET_PROTOCOL_VERSION,
            Self::FitzQuake => PROTOCOL_FITZQUAKE,
            Self::Rmq => PROTOCOL_RMQ,
        }
    }

    /// The protocol of the number `number`, `None` for one QuakeSpasm's `CL_ParseServerInfo`
    /// refuses.
    #[must_use]
    pub const fn from_number(number: i32) -> Option<Self> {
        match number {
            NET_PROTOCOL_VERSION => Some(Self::NetQuake),
            PROTOCOL_FITZQUAKE => Some(Self::FitzQuake),
            PROTOCOL_RMQ => Some(Self::Rmq),
            _ => None,
        }
    }
}

/// `sv.protocolflags`, the RMQ protocol's `PRFL_*` bits (NET-6.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProtocolFlags(u32);

impl ProtocolFlags {
    /// No flags: 15's and 666's.
    pub const NONE: Self = Self(0);
    /// `PRFL_SHORTANGLE`.
    pub const SHORT_ANGLE: Self = Self(1 << 1);
    /// `PRFL_FLOATANGLE`.
    pub const FLOAT_ANGLE: Self = Self(1 << 2);
    /// `PRFL_24BITCOORD`.
    pub const COORD_24: Self = Self(1 << 3);
    /// `PRFL_FLOATCOORD`.
    pub const FLOAT_COORD: Self = Self(1 << 4);
    /// `PRFL_EDICTSCALE`.
    pub const EDICT_SCALE: Self = Self(1 << 5);
    /// `PRFL_INT32COORD`.
    pub const INT32_COORD: Self = Self(1 << 7);

    /// The flags of the bits `bits`, as `svc_serverinfo` sends them.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// The bits.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Both sets of flags.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every flag of `other` is set.
    #[must_use]
    pub const fn has(self, other: Self) -> bool {
        self.0 & other.0 == other.0 && other.0 != 0
    }
}

/// A protocol and its flags: how every message of a server is written (NET-6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Protocol {
    version: ProtocolVersion,
    flags: ProtocolFlags,
}

/// How one coordinate is sent (NET-6.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WireCoord {
    /// A short: id's `(int)(f*8)`, or `Q_rint(f*8)`.
    Short(i32),
    /// A short and a fraction byte, `PRFL_24BITCOORD`.
    Short24(i32, i32),
    /// A long of sixteenths, `PRFL_INT32COORD`.
    Long(i32),
    /// A float, `PRFL_FLOATCOORD`.
    Float(f32),
}

/// How one angle is sent (NET-6.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WireAngle {
    /// A byte of 256 steps a turn.
    Byte(i32),
    /// A short of 65536 steps a turn.
    Short(i32),
    /// A float, `PRFL_FLOATANGLE`.
    Float(f32),
}

impl Protocol {
    /// Id's protocol 15 (NET-1.1).
    pub const NETQUAKE: Self = Self {
        version: ProtocolVersion::NetQuake,
        flags: ProtocolFlags::NONE,
    };
    /// FitzQuake's 666 (NET-6.1).
    pub const FITZQUAKE: Self = Self {
        version: ProtocolVersion::FitzQuake,
        flags: ProtocolFlags::NONE,
    };
    /// RMQ's 999 with the flags QuakeSpasm's `SV_SpawnServer` gives it, `PRFL_INT32COORD |
    /// PRFL_SHORTANGLE` (NET-6.1).
    pub const RMQ: Self = Self {
        version: ProtocolVersion::Rmq,
        flags: ProtocolFlags::INT32_COORD.with(ProtocolFlags::SHORT_ANGLE),
    };

    /// The protocol `version` with `flags`; flags only mean something to 999, and are none
    /// for the others.
    #[must_use]
    pub const fn new(version: ProtocolVersion, flags: ProtocolFlags) -> Self {
        match version {
            ProtocolVersion::Rmq => Self { version, flags },
            ProtocolVersion::NetQuake | ProtocolVersion::FitzQuake => Self {
                version,
                flags: ProtocolFlags::NONE,
            },
        }
    }

    /// The protocol `version` with these flags kept, as `svc_version` sets `cl.protocol` and
    /// leaves `cl.protocolflags` (CLIENT-9.2). The writers read the flags only under 999; the
    /// client reads them under any.
    #[must_use]
    pub const fn with_version(self, version: ProtocolVersion) -> Self {
        Self {
            version,
            flags: self.flags,
        }
    }

    /// Its version.
    #[must_use]
    pub const fn version(self) -> ProtocolVersion {
        self.version
    }

    /// Its flags.
    #[must_use]
    pub const fn flags(self) -> ProtocolFlags {
        self.flags
    }

    /// Whether it is id's 15, which has none of FitzQuake's extensions.
    #[must_use]
    pub const fn is_netquake(self) -> bool {
        matches!(self.version, ProtocolVersion::NetQuake)
    }

    /// `MSG_WriteCoord (sb, f, flags)` (NET-6.2): id's `(int)(f*8)` under 15, and under the
    /// others QuakeSpasm's: a float with `PRFL_FLOATCOORD`, `Q_rint (f * 16)` as a long with
    /// `PRFL_INT32COORD`, `MSG_WriteCoord24` with `PRFL_24BITCOORD`, else `MSG_WriteCoord16`'s
    /// `Q_rint(f*8)`.
    #[must_use]
    pub fn coord(self, f: f32) -> WireCoord {
        if self.is_netquake() {
            return WireCoord::Short(float_to_int(f * COORD16_SCALE));
        }
        if self.flags.has(ProtocolFlags::FLOAT_COORD) {
            WireCoord::Float(f)
        } else if self.flags.has(ProtocolFlags::INT32_COORD) {
            WireCoord::Long(q_rint(f * COORD32_SCALE))
        } else if self.flags.has(ProtocolFlags::COORD_24) {
            // `MSG_WriteShort (sb, f)` converts the float to an int, then the fraction.
            let fraction = float_to_int(f * COORD24_FRACTION).wrapping_rem(COORD24_MODULUS);
            WireCoord::Short24(float_to_int(f), fraction)
        } else {
            WireCoord::Short(q_rint(f * COORD16_SCALE))
        }
    }

    /// `MSG_WriteAngle (sb, f, flags)` (NET-6.2): id's `((int)f*256/360) & 255` under 15, and
    /// under the others QuakeSpasm's: a float with `PRFL_FLOATANGLE`, `Q_rint(f * 65536.0 /
    /// 360.0) & 65535` as a short with `PRFL_SHORTANGLE`, else `Q_rint(f * 256.0 / 360.0) &
    /// 255`.
    #[must_use]
    pub fn angle(self, f: f32) -> WireAngle {
        if self.is_netquake() {
            return WireAngle::Byte(
                float_to_int(f)
                    .wrapping_mul(ID_ANGLE_STEPS)
                    .wrapping_div(ID_FULL_TURN)
                    & BYTE_MASK,
            );
        }
        if self.flags.has(ProtocolFlags::FLOAT_ANGLE) {
            WireAngle::Float(f)
        } else if self.flags.has(ProtocolFlags::SHORT_ANGLE) {
            WireAngle::Short(angle16_steps(f))
        } else {
            WireAngle::Byte(q_rint_f64(f64::from(f) * BYTE_ANGLE_STEPS / FULL_TURN) & BYTE_MASK)
        }
    }

    /// The angle `f` as a client reads it back after [`Protocol::angle`] (NET-6.2): a byte as a
    /// signed char times `360.0/256`, a short as a signed short times `360.0/65536`.
    #[must_use]
    pub fn wire_angle(self, f: f32) -> f32 {
        let turn = FULL_TURN;
        match self.angle(f) {
            WireAngle::Byte(v) => {
                let [low, ..] = v.to_le_bytes();
                double_to_float(f64::from(i8::from_le_bytes([low])) * (turn / BYTE_ANGLE_STEPS))
            }
            WireAngle::Short(v) => {
                let [a, b, ..] = v.to_le_bytes();
                double_to_float(f64::from(i16::from_le_bytes([a, b])) * (turn / SHORT_ANGLE_STEPS))
            }
            WireAngle::Float(v) => v,
        }
    }

    /// `MSG_WriteAngle16 (sb, f, flags)` (NET-6.2): a float with `PRFL_FLOATANGLE`, else
    /// `Q_rint(f * 65536.0 / 360.0) & 65535` as a short.
    #[must_use]
    pub fn angle16(self, f: f32) -> WireAngle {
        if self.flags.has(ProtocolFlags::FLOAT_ANGLE) {
            WireAngle::Float(f)
        } else {
            WireAngle::Short(angle16_steps(f))
        }
    }
}

/// `Q_rint(f * 65536.0 / 360.0) & 65535`, in double (NET-6.2).
fn angle16_steps(f: f32) -> i32 {
    q_rint_f64(f64::from(f) * SHORT_ANGLE_STEPS / FULL_TURN) & SHORT_MASK
}

/// `Q_rint (x)`, "`((x) > 0 ? (int)((x) + 0.5) : (int)((x) - 0.5))`", of a float expression:
/// C's `0.5` is a double, so the sum is taken in double (NET-6.2).
#[must_use]
pub fn q_rint(x: f32) -> i32 {
    q_rint_f64(f64::from(x))
}

/// `Q_rint` of a double expression (NET-6.2).
#[must_use]
pub fn q_rint_f64(x: f64) -> i32 {
    if x > 0.0 {
        double_to_int(x + HALF_F64)
    } else {
        double_to_int(x - HALF_F64)
    }
}

#[cfg(test)]
mod tests;
