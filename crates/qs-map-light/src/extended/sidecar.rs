// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/bspfile_common.cc, from_json, to_json, LoadExtendedTexinfoFlags, LoadExtendedContentFlags
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/writebsp.cc, WriteExtendedTexinfoFlags, WriteExtendedContentFlags

//! The two files ericw-tools' qbsp writes beside a BSP for its `light` (LIGHT-9.8,
//! LIGHT-9.9): `<map>.texinfo.json`, each texinfo's lighting flags, and `<map>.content.json`,
//! each leaf's own contents.

use qs_map_bsp::{LeafKind, SurfaceFlags};
use serde_json::{Map, Value, json};

use crate::{AXES, Extras, to_f32};

/// What `<map>.texinfo.json` and `<map>.content.json` have in the place of `bsp`.
pub const TEXINFO_EXTENSION: &str = "texinfo.json";
pub const CONTENT_EXTENSION: &str = "content.json";
/// The name of solid contents in a `.content.json`.
const SOLID: &str = "SOLID";
/// The contents no sample may lie in (LIGHT-9.9).
const CLOSED: [&str; 3] = [SOLID, "DETAIL_WALL", "SKY"];

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
    /// What a compile gave for light: each texinfo's flags and each leaf's kind.
    #[must_use]
    pub const fn new(surfaces: Vec<SurfaceFlags>, leaves: Vec<LeafKind>) -> Self {
        Self { surfaces, leaves }
    }

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

    /// The text of a `.texinfo.json` holding these flags, `WriteExtendedTexinfoFlags`
    /// (LIGHT-9.8): each texinfo by its number, with the flags that are not the default.
    #[must_use]
    pub fn texinfo_json(&self) -> Vec<u8> {
        let all: Map<String, Value> = self
            .surfaces
            .iter()
            .enumerate()
            .map(|(index, flags)| (index.to_string(), surface_to_json(flags)))
            .collect();
        Value::Object(all).to_string().into_bytes()
    }

    /// The text of a `.content.json` holding these leaves, `WriteExtendedContentFlags`
    /// (LIGHT-9.9): a leaf no sample may lie in is written solid, any other with no contents.
    #[must_use]
    pub fn content_json(&self) -> Vec<u8> {
        let all: Vec<Value> = self
            .leaves
            .iter()
            .map(|leaf| match leaf {
                LeafKind::Closed => json!([SOLID]),
                LeafKind::Open | LeafKind::Native => json!([]),
            })
            .collect();
        Value::Array(all).to_string().into_bytes()
    }

    /// The two files to write beside a BSP, each by the extension it takes the place of the
    /// BSP's with; no text for one with nothing in it, which an id dialect compile gives.
    #[must_use]
    pub fn files(&self) -> [(&'static str, Option<Vec<u8>>); 2] {
        [
            (
                TEXINFO_EXTENSION,
                (!self.surfaces.is_empty()).then(|| self.texinfo_json()),
            ),
            (
                CONTENT_EXTENSION,
                (!self.leaves.is_empty()).then(|| self.content_json()),
            ),
        ]
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
            *slot = Sidecars::surface(value);
        }
    }
    Ok(out)
}

impl Sidecars {
    /// `surfflags_t::from_json`: the lighting flags of one texinfo.
    fn surface(value: &Value) -> SurfaceFlags {
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
}

/// `surfflags_t::to_json`: the lighting flags of one texinfo that are not the default.
fn surface_to_json(flags: &SurfaceFlags) -> Value {
    let defaults = SurfaceFlags::default();
    let mut out = Map::new();
    let mut put = |name: &str, value: Value| {
        out.insert(name.to_owned(), value);
    };
    for (name, value, default) in [
        ("phong_angle", flags.phong_angle, defaults.phong_angle),
        (
            "phong_angle_concave",
            flags.phong_angle_concave,
            defaults.phong_angle_concave,
        ),
        ("maxlight", flags.maxlight, defaults.maxlight),
        (
            "lightcolorscale",
            flags.lightcolorscale,
            defaults.lightcolorscale,
        ),
    ] {
        if value.to_bits() != default.to_bits() {
            put(name, json!(value));
        }
    }
    for (name, value) in [
        ("minlight", flags.minlight),
        ("light_alpha", flags.light_alpha),
    ] {
        if let Some(value) = value {
            put(name, json!(value));
        }
    }
    for (name, value) in [
        ("phong_group", flags.phong_group),
        ("surflight_group", flags.surflight_group),
    ] {
        if value != 0 {
            put(name, json!(value));
        }
    }
    for (name, value) in [
        ("no_dirt", flags.no_dirt),
        ("no_shadow", flags.no_shadow),
        ("no_bounce", flags.no_bounce),
        ("no_minlight", flags.no_minlight),
        ("light_ignore", flags.light_ignore),
    ] {
        if value {
            put(name, json!(true));
        }
    }
    if flags.minlight_color != defaults.minlight_color {
        put("minlight_color", json!(flags.minlight_color));
    }
    if let Some(twosided) = flags.light_twosided {
        put("light_twosided", json!(twosided));
    }
    if let Some(mask) = flags.object_channel_mask {
        put("object_channel_mask", json!(mask));
    }
    Value::Object(out)
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

    // spec LIGHT-9.8: the flags written are the flags read back.
    // spec LIGHT-9.9: so are the leaves, a leaf the BSP speaks for read back as open.
    #[test]
    fn what_is_written_is_read_back() {
        let flags = SurfaceFlags {
            phong_angle: 89.0,
            phong_angle_concave: 45.0,
            phong_group: 2,
            minlight: Some(0.0),
            minlight_color: [1, 2, 3],
            maxlight: 300.0,
            lightcolorscale: 0.5,
            light_alpha: Some(0.25),
            light_twosided: Some(true),
            no_dirt: true,
            no_shadow: true,
            no_bounce: true,
            no_minlight: true,
            light_ignore: true,
            object_channel_mask: Some(4),
            surflight_group: 7,
        };
        let written = Sidecars {
            surfaces: vec![SurfaceFlags::default(), flags],
            leaves: vec![LeafKind::Closed, LeafKind::Open, LeafKind::Native],
        };
        assert!(written.texinfo_json().starts_with(br#"{"0":{},"1":{"#));
        assert!(written.files().iter().all(|(_, text)| text.is_some()));
        assert_eq!(
            Sidecars::default().files(),
            [(TEXINFO_EXTENSION, None), (CONTENT_EXTENSION, None)]
        );
        let read =
            Sidecars::read(Some(&written.texinfo_json()), Some(&written.content_json())).unwrap();
        assert_eq!(read.surfaces, written.surfaces);
        assert_eq!(
            read.leaves,
            [LeafKind::Closed, LeafKind::Open, LeafKind::Open]
        );
        let none = Sidecars::default();
        assert_eq!(
            (none.texinfo_json(), none.content_json()),
            (b"{}".to_vec(), b"[]".to_vec())
        );
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
