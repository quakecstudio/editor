// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/bspfile_common.cc, set_mirrored, set_clips_same_type, is_any_detail, is_detail_solid, is_detail_wall, is_detail_fence, is_detail_illusionary, is_empty, is_any_solid, is_solid, is_sky, is_liquid, is_clip, is_origin, is_opaque, cluster_contents, combine_contents, portal_can_see_through, portal_visible_contents, is_two_sided, portal_generates_face
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/common/bspfile_common.hh, mirror_inside, clips_same_type
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/bspfile.cc, create_contents_from_native, contents_to_native, contents_remap_for_export, get_hull_sizes, face_get_contents

//! Contents and the game a map is compiled for, ericw-tools' `contentflags_t` and `gamedef_t`
//! (QBSP-15.9 to QBSP-15.13). The contents are a superset for every game ericw-tools compiles;
//! a [`Game`] maps them to its own. Quake is the only game here; Quake II's definition is the
//! second [`Game`] when it comes (`docs/design/extended-qbsp.md`).

use super::vec::{Bounds, Vec3};
use super::winding::Side;

/// One bit of a [`Contents`], `nth_bit`.
const fn bit(n: u32) -> u64 {
    match 1_u64.checked_shl(n) {
        Some(b) => b,
        None => 0,
    }
}

/// A brush's or a leaf's contents, `contentflags_t`: visible contents, strongest first, then
/// invisible contents, then flags (`bspfile_common.hh`, QBSP-15.9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Contents(u64);

impl Contents {
    /// `EWT_VISCONTENTS_EMPTY`.
    pub(crate) const EMPTY: Self = Self(0);
    /// `EWT_VISCONTENTS_SOLID`.
    pub(crate) const SOLID: u64 = bit(0);
    /// `EWT_VISCONTENTS_SKY`.
    pub(crate) const SKY: u64 = bit(1);
    /// `EWT_VISCONTENTS_DETAIL_WALL`.
    pub(crate) const DETAIL_WALL: u64 = bit(2);
    /// `EWT_VISCONTENTS_WINDOW`: a detail fence.
    pub(crate) const WINDOW: u64 = bit(3);
    /// `EWT_VISCONTENTS_ILLUSIONARY_VISBLOCKER`.
    pub(crate) const ILLUSIONARY_VISBLOCKER: u64 = bit(4);
    /// `EWT_VISCONTENTS_AUX`.
    pub(crate) const AUX: u64 = bit(5);
    /// `EWT_VISCONTENTS_LAVA`.
    pub(crate) const LAVA: u64 = bit(6);
    /// `EWT_VISCONTENTS_SLIME`.
    pub(crate) const SLIME: u64 = bit(7);
    /// `EWT_VISCONTENTS_WATER`.
    pub(crate) const WATER: u64 = bit(8);
    /// `EWT_VISCONTENTS_MIST`.
    pub(crate) const MIST: u64 = bit(9);
    /// `EWT_LAST_VISIBLE_CONTENTS_INDEX`.
    const LAST_VISIBLE_INDEX: u32 = 9;
    /// `EWT_INVISCONTENTS_ORIGIN`.
    pub(crate) const ORIGIN: u64 = bit(10);
    /// `EWT_INVISCONTENTS_PLAYERCLIP`.
    pub(crate) const PLAYERCLIP: u64 = bit(11);
    /// `EWT_INVISCONTENTS_MONSTERCLIP`.
    pub(crate) const MONSTERCLIP: u64 = bit(12);
    /// `EWT_INVISCONTENTS_AREAPORTAL`, Quake II's.
    pub(crate) const AREAPORTAL: u64 = bit(13);
    /// `EWT_INVISCONTENTS_PROJECTILECLIP`.
    pub(crate) const PROJECTILECLIP: u64 = bit(15);
    /// `EWT_CFLAG_MIRROR_INSIDE`.
    pub(crate) const MIRROR_INSIDE: u64 = bit(16);
    /// `EWT_CFLAG_MIRROR_INSIDE_SET`.
    pub(crate) const MIRROR_INSIDE_SET: u64 = bit(17);
    /// `EWT_CFLAG_SUPPRESS_CLIPPING_SAME_TYPE`.
    pub(crate) const SUPPRESS_CLIPPING_SAME_TYPE: u64 = bit(18);
    /// `EWT_CFLAG_TRANSLUCENT`.
    pub(crate) const TRANSLUCENT: u64 = bit(25);
    /// `EWT_CFLAG_DETAIL`.
    pub(crate) const DETAIL: u64 = bit(29);
    /// `EWT_ALL_LIQUIDS`.
    const ALL_LIQUIDS: u64 = Self::LAVA | Self::SLIME | Self::WATER;
    /// `EWT_ALL_VISIBLE_CONTENTS`.
    const ALL_VISIBLE: u64 = Self::SOLID
        | Self::SKY
        | Self::DETAIL_WALL
        | Self::WINDOW
        | Self::ILLUSIONARY_VISBLOCKER
        | Self::AUX
        | Self::LAVA
        | Self::SLIME
        | Self::WATER
        | Self::MIST;
    /// `EWT_ALL_INVISCONTENTS`.
    const ALL_INVISIBLE: u64 = Self::ORIGIN
        | Self::PLAYERCLIP
        | Self::MONSTERCLIP
        | Self::AREAPORTAL
        | Self::PROJECTILECLIP;
    /// The index `visible_contents_index` gives player clip, monster clip and projectile clip
    /// when no visible contents are set.
    const CLIP_INDEXES: [(u64, u32); 3] = [
        (Self::PLAYERCLIP, 10),
        (Self::MONSTERCLIP, 11),
        (Self::PROJECTILECLIP, 14),
    ];

    /// The contents with the mirror-inside flag `mirror` says, `set_mirrored` (QBSP-15.15).
    pub(crate) const fn with_mirrored(self, mirror: MirrorInside) -> Self {
        let both = Self::MIRROR_INSIDE_SET | Self::MIRROR_INSIDE;
        Self(match mirror {
            MirrorInside::Unset => self.0 & !both,
            MirrorInside::On => self.0 | both,
            MirrorInside::Off => (self.0 | Self::MIRROR_INSIDE_SET) & !Self::MIRROR_INSIDE,
        })
    }

    /// What its mirror-inside flags say, `mirror_inside` (QBSP-15.22).
    pub(crate) const fn mirror_inside(self) -> MirrorInside {
        if !self.has(Self::MIRROR_INSIDE_SET) {
            MirrorInside::Unset
        } else if self.has(Self::MIRROR_INSIDE) {
            MirrorInside::On
        } else {
            MirrorInside::Off
        }
    }

    /// Whether it does not clip faces of its own type, `clips_same_type() == false`
    /// (QBSP-15.22).
    pub(crate) const fn suppresses_clipping_same_type(self) -> bool {
        self.has(Self::SUPPRESS_CLIPPING_SAME_TYPE)
    }

    /// The contents not clipping faces of their own type, `set_clips_same_type(false)`
    /// (QBSP-15.15).
    pub(crate) const fn without_clipping_same_type(self) -> Self {
        Self(self.0 | Self::SUPPRESS_CLIPPING_SAME_TYPE)
    }

    /// The contents of a `func_illusionary_visblocker`'s brush (QBSP-15.15): no visible
    /// contents, detail or translucency, but the visblocker's.
    pub(crate) const fn as_visblocker(self) -> Self {
        Self(
            (self.0 & !(Self::ALL_VISIBLE | Self::DETAIL | Self::TRANSLUCENT))
                | Self::ILLUSIONARY_VISBLOCKER,
        )
    }

    /// The contents of `flags`, `contentflags_t::make`.
    pub(crate) const fn new(flags: u64) -> Self {
        Self(flags)
    }

    /// Its bits.
    pub(crate) const fn flags(self) -> u64 {
        self.0
    }

    /// Whether any of `bits` is set.
    const fn has(self, bits: u64) -> bool {
        self.0 & bits != 0
    }

    /// `create_detail_illusionary_contents`: not solid, mist, translucent, detail, mirror inside
    /// set off.
    pub(crate) const fn detail_illusionary(self) -> Self {
        Self(
            (self.0 & !(Self::SOLID | Self::MIRROR_INSIDE))
                | Self::MIST
                | Self::TRANSLUCENT
                | Self::DETAIL
                | Self::MIRROR_INSIDE_SET,
        )
    }

    /// `create_detail_fence_contents`: not solid, a window, translucent, detail, mirror inside
    /// set off.
    pub(crate) const fn detail_fence(self) -> Self {
        Self(
            (self.0 & !Self::SOLID)
                | Self::WINDOW
                | Self::TRANSLUCENT
                | Self::DETAIL
                | Self::MIRROR_INSIDE_SET,
        )
    }

    /// `create_detail_wall_contents`: not solid, a detail wall, detail.
    pub(crate) const fn detail_wall(self) -> Self {
        Self((self.0 & !Self::SOLID) | Self::DETAIL_WALL | Self::DETAIL)
    }

    /// `create_detail_solid_contents`: solid and detail.
    pub(crate) const fn detail_solid(self) -> Self {
        Self(self.0 | Self::SOLID | Self::DETAIL)
    }

    /// `get_content_type`: the visible and invisible contents, without the flags.
    pub(crate) const fn content_type(self) -> u64 {
        self.0 & (Self::ALL_VISIBLE | Self::ALL_INVISIBLE)
    }

    /// `types_equal`.
    pub(crate) const fn types_equal(self, other: Self) -> bool {
        self.content_type() == other.content_type()
    }

    /// `is_any_detail`.
    pub(crate) const fn is_any_detail(self) -> bool {
        self.has(Self::DETAIL)
    }

    /// `is_detail_solid`.
    pub(crate) const fn is_detail_solid(self) -> bool {
        self.has(Self::DETAIL) && self.has(Self::SOLID)
    }

    /// `is_detail_wall`: never when solid.
    pub(crate) const fn is_detail_wall(self) -> bool {
        !self.has(Self::SOLID) && self.has(Self::DETAIL) && self.has(Self::DETAIL_WALL)
    }

    /// `is_detail_fence`: never when solid.
    pub(crate) const fn is_detail_fence(self) -> bool {
        !self.has(Self::SOLID) && self.has(Self::DETAIL) && self.has(Self::WINDOW)
    }

    /// `is_detail_illusionary`: mist or aux detail, never when solid.
    pub(crate) const fn is_detail_illusionary(self) -> bool {
        !self.has(Self::SOLID) && self.has(Self::DETAIL) && self.has(Self::MIST | Self::AUX)
    }

    /// `is_empty`: no contents type.
    pub(crate) const fn is_empty(self) -> bool {
        self.content_type() == 0
    }

    /// `is_any_solid`.
    pub(crate) const fn is_any_solid(self) -> bool {
        self.has(Self::SOLID)
    }

    /// `is_solid`: structural solid, not detail.
    pub(crate) const fn is_solid(self) -> bool {
        self.has(Self::SOLID) && !self.has(Self::DETAIL)
    }

    /// `is_sky`.
    pub(crate) const fn is_sky(self) -> bool {
        self.has(Self::SKY)
    }

    /// `is_liquid`: the strongest visible contents are a liquid, or an area portal, which the
    /// CSG treats as one.
    pub(crate) fn is_liquid(self) -> bool {
        let visible = self.visible().0;
        visible & Self::AREAPORTAL != 0 || visible & Self::ALL_LIQUIDS != 0
    }

    /// `is_clip`: player or monster clip.
    pub(crate) const fn is_clip(self) -> bool {
        self.has(Self::PLAYERCLIP | Self::MONSTERCLIP)
    }

    /// `is_origin`.
    pub(crate) const fn is_origin(self) -> bool {
        self.has(Self::ORIGIN)
    }

    /// `is_opaque`: visible and not translucent.
    pub(crate) fn is_opaque(self) -> bool {
        self.visible().0 != Self::EMPTY.0 && !self.has(Self::TRANSLUCENT)
    }

    /// `is_fence`: a detail fence or detail illusionary.
    pub(crate) const fn is_fence(self) -> bool {
        self.is_detail_fence() || self.is_detail_illusionary()
    }

    /// `seals_map`: structural solid or sky.
    pub(crate) const fn seals_map(self) -> bool {
        self.is_solid() || self.is_sky()
    }

    /// `visible_contents_index`: the strongest visible contents' bit, else player clip's 10,
    /// monster clip's 11 or projectile clip's 14, else none.
    pub(crate) fn visible_index(self) -> Option<u32> {
        (0..=Self::LAST_VISIBLE_INDEX)
            .find(|&n| self.has(bit(n)))
            .or_else(|| {
                Self::CLIP_INDEXES
                    .iter()
                    .find(|(b, _)| self.has(*b))
                    .map(|(_, n)| *n)
            })
    }

    /// `visible_contents`: the strongest visible contents alone.
    pub(crate) fn visible(self) -> Self {
        self.visible_index().map_or(Self::EMPTY, |n| Self(bit(n)))
    }

    /// `cluster_contents`: both, but solid only when both are (QBSP-15.10).
    pub(crate) const fn cluster(self, other: Self) -> Self {
        let mut combined = self.0 | other.0;
        if !self.has(Self::SOLID) || !other.has(Self::SOLID) {
            combined &= !Self::SOLID;
        }
        Self(combined)
    }

    /// `combine_contents`: both, structural solid, sky or a visblocker taking away the other's
    /// detail and translucency (QBSP-15.10).
    pub(crate) const fn combine(a: Self, b: Self) -> Self {
        let (mut x, mut y) = (a.0, b.0);
        let strip = Self::DETAIL | Self::TRANSLUCENT;
        if a.is_solid()
            || b.is_solid()
            || a.is_sky()
            || b.is_sky()
            || a.has(Self::ILLUSIONARY_VISBLOCKER)
            || b.has(Self::ILLUSIONARY_VISBLOCKER)
        {
            x &= !strip;
            y &= !strip;
        }
        Self(x | y)
    }

    /// `portal_can_see_through` (QBSP-15.11).
    pub(crate) const fn portal_can_see_through(a: Self, b: Self) -> bool {
        let (mut c0, mut c1) = (a.0, b.0);
        if (c0 | c1) & Self::SOLID != 0 {
            return false;
        }
        if (c0 ^ c1) & Self::ALL_VISIBLE == 0 {
            return true;
        }
        if c0 & (Self::TRANSLUCENT | Self::DETAIL) != 0 {
            c0 = 0;
        }
        if c1 & (Self::TRANSLUCENT | Self::DETAIL) != 0 {
            c1 = 0;
        }
        c0 ^ c1 == 0 || (c0 ^ c1) & Self::ALL_VISIBLE == 0
    }

    /// `portal_visible_contents`: the strongest contents that change across a portal; none
    /// between solid or sky on both sides (QBSP-15.11).
    pub(crate) fn portal_visible(a: Self, b: Self) -> Self {
        let walls = Self::SOLID | Self::SKY;
        if a.has(walls) && b.has(walls) {
            return Self::EMPTY;
        }
        let result = if a.has(Self::SUPPRESS_CLIPPING_SAME_TYPE)
            || b.has(Self::SUPPRESS_CLIPPING_SAME_TYPE)
        {
            a.0 | b.0
        } else {
            a.0 ^ b.0
        };
        Self(result).visible()
    }

    /// `is_two_sided`: a brush mirrored inside by its own setting, or a visblocker, liquid or
    /// mist.
    pub(crate) const fn is_two_sided(self) -> bool {
        if self.has(Self::MIRROR_INSIDE_SET) {
            return self.has(Self::MIRROR_INSIDE);
        }
        self.has(Self::ILLUSIONARY_VISBLOCKER | Self::LAVA | Self::SLIME | Self::WATER | Self::MIST)
    }

    /// `portal_generates_face`: whether a brush of these contents gives a face on `side` of
    /// its side for a portal whose visible contents are `portal` (QBSP-15.11).
    pub(crate) fn portal_generates_face(portal: Self, brush: Self, side: Side) -> bool {
        let Some(index) = portal.visible_index() else {
            return false;
        };
        if !brush.has(bit(index)) {
            return false;
        }
        if side != Side::Back {
            return true;
        }
        if brush.has(Self::MIRROR_INSIDE_SET) {
            return brush.has(Self::MIRROR_INSIDE);
        }
        !portal.has(Self::WINDOW | Self::AUX | Self::DETAIL_WALL)
    }
}

/// `CONTENTS_EMPTY` (`bspfile_q1.hh`).
const CONTENTS_EMPTY: i32 = -1;
/// `CONTENTS_SOLID`.
const CONTENTS_SOLID: i32 = -2;
/// `CONTENTS_WATER`.
const CONTENTS_WATER: i32 = -3;
/// `CONTENTS_SLIME`.
const CONTENTS_SLIME: i32 = -4;
/// `CONTENTS_LAVA`.
const CONTENTS_LAVA: i32 = -5;
/// `CONTENTS_SKY`.
const CONTENTS_SKY: i32 = -6;

/// What an entity's `_mirrorinside` key says of its brushes' insides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MirrorInside {
    /// No key: the contents' own default.
    Unset,
    /// Nonzero: the faces are mirrored inside.
    On,
    /// Zero: they are not.
    Off,
}

/// Quake's native contents, `CONTENTS_EMPTY` to `CONTENTS_SKY` (`bspfile_q1.hh`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeContents {
    Empty,
    Solid,
    Water,
    Slime,
    Lava,
    Sky,
}

impl NativeContents {
    /// Its value in a BSP: -1 to -6.
    pub(crate) const fn value(self) -> i32 {
        match self {
            Self::Empty => CONTENTS_EMPTY,
            Self::Solid => CONTENTS_SOLID,
            Self::Water => CONTENTS_WATER,
            Self::Slime => CONTENTS_SLIME,
            Self::Lava => CONTENTS_LAVA,
            Self::Sky => CONTENTS_SKY,
        }
    }
}

/// Contents a game's BSP cannot hold, `contents_to_native`'s `invalid_argument`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Unrepresentable(pub(crate) Contents);

/// Which of a liquid's contents ericw-tools' `-transwater` makes: detail and translucent, its
/// default, or plain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TransWater {
    #[default]
    On,
    Off,
}

/// The game a map is compiled for, `gamedef_t` (QBSP-15.12, QBSP-15.13).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Game {
    /// Quake, `gamedef_q1_like_t<GAME_QUAKE>`.
    #[default]
    Quake,
}

/// Quake's clipping hulls, `get_hull_sizes`: the point, the player and the shambler.
const QUAKE_HULLS: [([f64; 3], [f64; 3]); 3] = [
    ([0.0; 3], [0.0; 3]),
    ([-16.0, -16.0, -32.0], [16.0, 16.0, 24.0]),
    ([-32.0, -32.0, -64.0], [32.0, 32.0, 24.0]),
];

impl Game {
    /// The contents of a BSP's native value, `create_contents_from_native`.
    pub(crate) const fn contents_of(self, native: NativeContents) -> Contents {
        match (self, native) {
            (Self::Quake, NativeContents::Solid) => Contents(Contents::SOLID),
            (Self::Quake, NativeContents::Sky) => Contents(Contents::SKY),
            (Self::Quake, NativeContents::Lava) => Contents(Contents::LAVA),
            (Self::Quake, NativeContents::Slime) => Contents(Contents::SLIME),
            (Self::Quake, NativeContents::Water) => Contents(Contents::WATER),
            (Self::Quake, NativeContents::Empty) => Contents::EMPTY,
        }
    }

    /// The native value of `contents`, `contents_to_native` (QBSP-15.12): solid, sky, lava,
    /// slime and water in that order; a detail wall, window, visblocker, aux or mist alone
    /// cannot be written; anything else is empty.
    pub(crate) fn to_native(self, contents: Contents) -> Result<NativeContents, Unrepresentable> {
        let Self::Quake = self;
        let order: [(u64, Option<NativeContents>); 10] = [
            (Contents::SOLID, Some(NativeContents::Solid)),
            (Contents::SKY, Some(NativeContents::Sky)),
            (Contents::DETAIL_WALL, None),
            (Contents::WINDOW, None),
            (Contents::ILLUSIONARY_VISBLOCKER, None),
            (Contents::AUX, None),
            (Contents::LAVA, Some(NativeContents::Lava)),
            (Contents::SLIME, Some(NativeContents::Slime)),
            (Contents::WATER, Some(NativeContents::Water)),
            (Contents::MIST, None),
        ];
        match order.into_iter().find(|(b, _)| contents.has(*b)) {
            None => Ok(NativeContents::Empty),
            Some((_, Some(native))) => Ok(native),
            Some((_, None)) => Err(Unrepresentable(contents)),
        }
    }

    /// `contents` as they are written, `contents_remap_for_export` (QBSP-15.12): a window or
    /// detail wall becomes solid, mist and aux are cleared, and a visblocker is written empty.
    pub(crate) const fn remap_for_export(self, contents: Contents) -> Contents {
        let Self::Quake = self;
        let mut c = contents.0;
        if c & Contents::WINDOW != 0 {
            c = (c & !Contents::WINDOW) | Contents::SOLID;
        }
        if c & Contents::DETAIL_WALL != 0 {
            c = (c & !Contents::DETAIL_WALL) | Contents::SOLID;
        }
        c &= !(Contents::MIST | Contents::AUX | Contents::ILLUSIONARY_VISBLOCKER);
        Contents(c)
    }

    /// The clipping hulls' sizes, hull 0 first, `get_hull_sizes`.
    pub(crate) fn hull_sizes(self) -> Vec<Bounds> {
        let Self::Quake = self;
        QUAKE_HULLS
            .iter()
            .map(|&(mins, maxs): &(Vec3, Vec3)| Bounds { mins, maxs })
            .collect()
    }

    /// The box hull `hull`'s brushes are grown by, `get_hull_sizes()[hullnum]`.
    pub(crate) fn hull_size(self, hull: crate::Hull) -> Bounds {
        let index = match hull {
            crate::Hull::Draw => 0,
            crate::Hull::Player => 1,
            crate::Hull::Large => 2,
        };
        let Self::Quake = self;
        let (mins, maxs) = QUAKE_HULLS.get(index).copied().unwrap_or_default();
        Bounds { mins, maxs }
    }

    /// Whether a brush entity keeps its brushes' contents, `allow_contented_bmodels`: not in
    /// Quake, where they become solid.
    pub(crate) const fn bmodels_keep_contents(self) -> bool {
        let Self::Quake = self;
        false
    }

    /// A face's contents from its texture's name, `face_get_contents` (QBSP-15.13): `origin`,
    /// `hint` or `hintskip`, `clip`, a name starting `*` (lava, slime or water, detail and
    /// translucent with `transwater`), a name starting `sky`, each in any case; anything else
    /// solid.
    pub(crate) fn face_contents(self, texture: &[u8], transwater: TransWater) -> Contents {
        let Self::Quake = self;
        let is = |name: &[u8]| texture.eq_ignore_ascii_case(name);
        let starts = |text: &[u8], prefix: &[u8]| {
            text.get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        };
        if is(b"origin") {
            return Contents(Contents::ORIGIN);
        }
        if is(b"hint") || is(b"hintskip") {
            return Contents::EMPTY;
        }
        if is(b"clip") {
            return Contents(Contents::PLAYERCLIP);
        }
        if let Some(rest) = texture.strip_prefix(b"*") {
            let liquid = match transwater {
                TransWater::On => Contents::DETAIL | Contents::TRANSLUCENT,
                TransWater::Off => 0,
            };
            let kind = if starts(rest, b"lava") {
                Contents::LAVA
            } else if starts(rest, b"slime") {
                Contents::SLIME
            } else {
                Contents::WATER
            };
            return Contents(kind | liquid);
        }
        if starts(texture, b"sky") {
            return Contents(Contents::SKY);
        }
        Contents(Contents::SOLID)
    }
}

#[cfg(test)]
mod tests;
