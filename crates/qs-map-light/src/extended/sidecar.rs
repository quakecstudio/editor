// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/bspfile_common.cc, surfflags_t::from_json, LoadExtendedTexinfoFlags, LoadExtendedContentFlags

//! The two files ericw-tools' qbsp writes beside a BSP for its `light` (LIGHT-9.8,
//! LIGHT-9.9): `<map>.texinfo.json`, each texinfo's lighting flags, and `<map>.content.json`,
//! each leaf's own contents.

use serde_json::Value;

use crate::extended::scene::{LeafKind, SurfaceFlags};
use crate::{AXES, Extras, to_f32};

/// The contents no sample may lie in (LIGHT-9.9).
const CLOSED: [&str; 3] = ["SOLID", "DETAIL_WALL", "SKY"];

/// Which of the two files could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidecarFile {
    /// `<map>.texinfo.json`.
    Texinfo,
    /// `<map>.content.json`.
    Content,
}

/// What ericw-tools' qbsp, or ours, tells light beside the BSP.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sidecars {
    /// Each texinfo's flags, in order (LIGHT-9.8).
    pub surfaces: Vec<SurfaceFlags>,
    /// What each leaf is to a sample, in order (LIGHT-9.9).
    pub leaves: Vec<LeafKind>,
}

impl Sidecars {
    /// The flags of `texinfo`, the text of a `.texinfo.json`, and the leaves of `content`,
    /// the text of a `.content.json`; a file that is not there gives nothing.
    ///
    /// # Errors
    ///
    /// The file that is not the JSON ericw-tools writes.
    pub fn read(texinfo: Option<&[u8]>, content: Option<&[u8]>) -> Result<Self, SidecarFile> {
        Ok(Self {
            surfaces: texinfo.map(surfaces).transpose()?.unwrap_or_default(),
            leaves: content.map(leaves).transpose()?.unwrap_or_default(),
        })
    }

    /// What light takes besides the BSP, with no palette.
    #[must_use]
    pub fn extras(&self) -> Extras<'_> {
        Extras {
            surfaces: &self.surfaces,
            leaves: &self.leaves,
            palette: None,
        }
    }
}

/// `LoadExtendedTexinfoFlags`: an object of texinfo numbers, each with its flags.
fn surfaces(text: &[u8]) -> Result<Vec<SurfaceFlags>, SidecarFile> {
    let json: serde_json::Map<String, Value> =
        serde_json::from_slice(text).map_err(|_| SidecarFile::Texinfo)?;
    let mut out = Vec::new();
    for (key, value) in &json {
        let index: usize = key.parse().map_err(|_| SidecarFile::Texinfo)?;
        if out.len() <= index {
            out.resize(index.saturating_add(1), SurfaceFlags::default());
        }
        if let Some(slot) = out.get_mut(index) {
            *slot = flags(value);
        }
    }
    Ok(out)
}

/// `surfflags_t::from_json`: the lighting flags of one texinfo.
fn flags(value: &Value) -> SurfaceFlags {
    let number = |name: &str| value.get(name).and_then(Value::as_f64).map(to_f32);
    let whole = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_i64)
            .and_then(|n| i32::try_from(n).ok())
    };
    let flag = |name: &str| value.get(name).and_then(Value::as_bool);
    let mut colour = [0; AXES];
    if let Some(channels) = value.get("minlight_color").and_then(Value::as_array) {
        for (slot, channel) in colour.iter_mut().zip(channels) {
            *slot = channel
                .as_u64()
                .and_then(|c| u8::try_from(c).ok())
                .unwrap_or_default();
        }
    }
    let defaults = SurfaceFlags::default();
    SurfaceFlags {
        phong_angle: number("phong_angle").unwrap_or(defaults.phong_angle),
        phong_angle_concave: number("phong_angle_concave").unwrap_or(0.0),
        phong_group: whole("phong_group").unwrap_or(0),
        minlight: number("minlight"),
        minlight_color: colour,
        maxlight: number("maxlight").unwrap_or(0.0),
        lightcolorscale: number("lightcolorscale").unwrap_or(defaults.lightcolorscale),
        light_alpha: number("light_alpha"),
        light_twosided: flag("light_twosided"),
        no_dirt: flag("no_dirt").unwrap_or(false),
        no_shadow: flag("no_shadow").unwrap_or(false),
        no_bounce: flag("no_bounce").unwrap_or(false),
        no_minlight: flag("no_minlight").unwrap_or(false),
        light_ignore: flag("light_ignore").unwrap_or(false),
        object_channel_mask: whole("object_channel_mask"),
        surflight_group: whole("surflight_group").unwrap_or(0),
    }
}

/// `LoadExtendedContentFlags`: an array of each leaf's contents, each an array of names.
fn leaves(text: &[u8]) -> Result<Vec<LeafKind>, SidecarFile> {
    let json: Vec<Vec<String>> = serde_json::from_slice(text).map_err(|_| SidecarFile::Content)?;
    Ok(json
        .iter()
        .map(|contents| {
            if contents.iter().any(|name| CLOSED.contains(&name.as_str())) {
                LeafKind::Closed
            } else {
                LeafKind::Open
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test, and the values are exact"
    )]

    use super::*;

    // spec LIGHT-9.8: each texinfo's flags are read from the object ericw-tools' qbsp writes,
    // keyed by the texinfo's number.
    #[test]
    fn texinfo_flags_are_read_by_number() {
        let text = br#"{
            "0" : {},
            "2" : { "phong_angle" : 89.0, "minlight" : 30.5, "minlight_color" : [255, 128, 0],
                    "no_shadow" : true, "object_channel_mask" : 2, "is_special" : true },
            "1" : { "no_dirt" : true, "light_alpha" : 0.5, "light_twosided" : false,
                    "phong_group" : 3, "lightcolorscale" : 0.25 }
        }"#;
        let read = Sidecars::read(Some(text), None).unwrap();
        assert_eq!(read.surfaces.len(), 3);
        assert_eq!(read.surfaces[0], SurfaceFlags::default());
        let second = read.surfaces[2];
        assert_eq!(
            (second.phong_angle, second.minlight, second.minlight_color),
            (89.0, Some(30.5), [255, 128, 0])
        );
        assert!(second.no_shadow && !second.no_dirt);
        assert_eq!(second.object_channel_mask, Some(2));
        let first = read.surfaces[1];
        assert!(first.no_dirt);
        assert_eq!(
            (first.light_alpha, first.light_twosided),
            (Some(0.5), Some(false))
        );
        assert_eq!((first.phong_group, first.lightcolorscale), (3, 0.25));
        assert!(read.leaves.is_empty());
        assert_eq!(read.extras().surfaces.len(), 3);
        assert_eq!(
            Sidecars::read(Some(b"[1]"), None),
            Err(SidecarFile::Texinfo)
        );
        assert_eq!(
            Sidecars::read(Some(br#"{"x": {}}"#), None),
            Err(SidecarFile::Texinfo)
        );
        assert_eq!(Sidecars::read(None, None), Ok(Sidecars::default()));
    }

    // spec LIGHT-9.9: a leaf whose contents have SOLID, DETAIL_WALL or SKY holds no sample;
    // any other may.
    #[test]
    fn leaf_contents_say_which_leaves_are_closed() {
        let text = br#"[["SOLID"], [], ["WINDOW", "TRANSLUCENT", "DETAIL"], ["SKY", "DETAIL_WALL"],
                       ["WATER"]]"#;
        let read = Sidecars::read(None, Some(text)).unwrap();
        assert_eq!(
            read.leaves,
            [
                LeafKind::Closed,
                LeafKind::Open,
                LeafKind::Open,
                LeafKind::Closed,
                LeafKind::Open
            ]
        );
        assert_eq!(Sidecars::read(None, Some(b"{}")), Err(SidecarFile::Content));
    }
}
