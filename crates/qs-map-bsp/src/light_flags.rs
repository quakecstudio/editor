// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/common/bspfile_common.hh, surfflags_t

//! What the extended dialect's compile tells the light compiler besides the BSP (QBSP-15.54,
//! QBSP-15.55): each texinfo's lighting flags, from the keys of the entity its faces' brushes
//! are in, and what each leaf is to a light sample. ericw-tools' qbsp writes the same beside
//! a BSP as `<map>.texinfo.json` and `<map>.content.json`.

/// The channels of a colour.
const CHANNELS: usize = 3;

/// The lighting flags of a texinfo, the part of ericw-tools' `surfflags_t` its `light`
/// reads (QBSP-15.54). A brush's keys, such as `_phong` and `_minlight` on a `func_group` or
/// `func_detail`, reach light through these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceFlags {
    /// `phong_angle`: faces whose normals are within this many degrees are smoothed; 0 for
    /// none.
    pub phong_angle: f32,
    /// `phong_angle_concave`: the same for faces that meet in a hollow; 0 for `phong_angle`.
    pub phong_angle_concave: f32,
    /// `phong_group`: faces of different groups are never smoothed together.
    pub phong_group: i32,
    /// `minlight`: the face's minimum light.
    pub minlight: Option<f32>,
    /// `minlight_color`: its colour; black for none.
    pub minlight_color: [u8; CHANNELS],
    /// `maxlight`: the most light the face takes; 0 for no limit.
    pub maxlight: f32,
    /// `lightcolorscale`: how much of the light's colour the face keeps, 1 for all.
    pub lightcolorscale: f32,
    /// `light_alpha`: how much light the face stops, under 1 for glass.
    pub light_alpha: Option<f32>,
    /// `light_twosided`: whether the face is lit from both sides.
    pub light_twosided: Option<bool>,
    /// `no_dirt`: the face takes no dirt.
    pub no_dirt: bool,
    /// `no_shadow`: the face casts no shadow.
    pub no_shadow: bool,
    /// `no_bounce`: the face bounces no light.
    pub no_bounce: bool,
    /// `no_minlight`: the face takes no minimum light.
    pub no_minlight: bool,
    /// `light_ignore`: no light reaches the face but minimum light.
    pub light_ignore: bool,
    /// `object_channel_mask`: the channels the face is lit and casts shadow on.
    pub object_channel_mask: Option<i32>,
    /// `surflight_group`: the group of surface lights the face takes.
    pub surflight_group: i32,
}

impl Default for SurfaceFlags {
    fn default() -> Self {
        Self {
            phong_angle: 0.0,
            phong_angle_concave: 0.0,
            phong_group: 0,
            minlight: None,
            minlight_color: [0; CHANNELS],
            maxlight: 0.0,
            lightcolorscale: 1.0,
            light_alpha: None,
            light_twosided: None,
            no_dirt: false,
            no_shadow: false,
            no_bounce: false,
            no_minlight: false,
            light_ignore: false,
            object_channel_mask: None,
            surflight_group: 0,
        }
    }
}

/// What a leaf is to a light sample (QBSP-15.55): the leaf's own contents, of which the
/// BSP's are only the nearest the format has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeafKind {
    /// As the BSP says: a solid or sky leaf holds no sample.
    #[default]
    Native,
    /// A leaf a sample may lie in whatever the BSP says: the inside of a fence or a window,
    /// which the BSP calls solid.
    Open,
    /// A leaf no sample may lie in: solid, a detail wall or sky.
    Closed,
}
