// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-2001 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/gl_sky.c, Sky_NewMap, Sky_LoadSkyBox
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/image.c, Image_LoadImage, Image_LoadPCX

//! Skyboxes, the six pictures a map's worldspawn names, which QuakeSpasm draws where the sky
//! faces are instead of id's two scrolling layers (REND-8).

use crate::gpu::REND_SKYFOG;
use crate::mesh::Image;

/// `suf` in `Sky_LoadSkyBox`: the six faces' suffixes, in the order they are loaded.
pub const SKY_SIDES: [&str; 6] = ["rt", "bk", "lf", "ft", "up", "dn"];

/// What `Sky_NewMap` reads from the worldspawn (REND-8.1, REND-8.2).
#[derive(Clone, Debug, PartialEq)]
pub struct SkyKeys {
    /// The skybox's name, the last of `sky`, `skyname` and `qlsky`; empty for none.
    pub name: String,
    /// `skyfog`, the fog's share of the sky.
    pub skyfog: f32,
}

impl Default for SkyKeys {
    fn default() -> Self {
        Self {
            name: String::new(),
            skyfog: REND_SKYFOG,
        }
    }
}

/// `Sky_NewMap`: the first entity's `sky`, `skyname` and `qlsky` keys, each naming the
/// skybox in turn, and its `skyfog` read as `atof` reads it; a key's leading `_` and
/// trailing spaces are dropped (REND-8.2).
#[must_use]
pub fn sky_parse_worldspawn(entities: &[u8]) -> SkyKeys {
    let mut keys = SkyKeys::default();
    let text = String::from_utf8_lossy(entities);
    let Some(block) = text.split('}').next() else {
        return keys;
    };
    let mut fields = block.split('"').skip(1).step_by(2);
    while let (Some(key), Some(value)) = (fields.next(), fields.next()) {
        let key = key.strip_prefix('_').unwrap_or(key).trim_end_matches(' ');
        match key {
            "sky" | "skyname" | "qlsky" => value.clone_into(&mut keys.name),
            "skyfog" => keys.skyfog = atof(value),
            _ => {}
        }
    }
    keys
}

/// C's `atof`: the longest leading number after any white space, 0 for none.
fn atof(value: &str) -> f32 {
    let value = value.trim_start();
    (0..=value.len())
        .rev()
        .filter_map(|end| value.get(..end))
        .find_map(|prefix| prefix.parse().ok())
        .unwrap_or(0.0)
}

/// A map's skybox: its six faces, in [`SKY_SIDES`]' order, `None` for one that could not be
/// read (REND-8.2).
#[derive(Clone, Debug, PartialEq)]
pub struct Skybox {
    faces: [Option<Image>; 6],
}

impl Skybox {
    /// `Sky_LoadSkyBox (name)`: each face `gfx/env/<name><side>` through `read`, as a TGA,
    /// else a PCX (`Image_LoadImage`). `None` for an empty name or when no face is read,
    /// which keeps the scrolling layers (REND-8.2).
    #[must_use]
    pub fn load(name: &str, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<Self> {
        if name.is_empty() {
            return None;
        }
        let faces = SKY_SIDES.map(|side| {
            let file = format!("gfx/env/{name}{side}");
            read(&format!("{file}.tga"))
                .and_then(|bytes| read_tga(&bytes))
                .or_else(|| read(&format!("{file}.pcx")).and_then(|bytes| read_pcx(&bytes)))
        });
        faces.iter().any(Option::is_some).then_some(Self { faces })
    }

    /// The size every face is drawn at: the first face read's.
    #[must_use]
    pub fn size(&self) -> [u32; 2] {
        self.faces
            .iter()
            .flatten()
            .next()
            .map_or([1, 1], |face| [face.width, face.height])
    }

    /// The six faces at [`Self::size`], four bytes a pixel, one after another: a face of
    /// another size taken at its nearest texels, and a face not read black (REND-8.2).
    #[must_use]
    pub fn layers(&self) -> Vec<u8> {
        let [width, height] = self.size();
        let mut out = Vec::new();
        for face in &self.faces {
            for y in 0..height {
                for x in 0..width {
                    let texel = face
                        .as_ref()
                        .and_then(|face| nearest(face, [x, y], [width, height]));
                    out.extend(texel.unwrap_or([0, 0, 0, 255]));
                }
            }
        }
        out
    }
}

/// The texel of `face` nearest to `at` on a picture of `size`.
fn nearest(face: &Image, [x, y]: [u32; 2], [width, height]: [u32; 2]) -> Option<[u8; 4]> {
    let scale = |at: u32, from: u32, to: u32| {
        u64::from(at)
            .checked_mul(u64::from(from))
            .and_then(|n| n.checked_div(u64::from(to)))
    };
    let sx = scale(x, face.width, width)?;
    let sy = scale(y, face.height, height)?;
    let index = sy
        .checked_mul(u64::from(face.width))?
        .checked_add(sx)?
        .checked_mul(4)?;
    let index = usize::try_from(index).ok()?;
    let texel = face.rgba.get(index..index.checked_add(4)?)?;
    texel.try_into().ok()
}

/// A TGA as `Image_LoadTGA` reads it: its rows from the top, whichever way the file stores
/// them. `None` for a file that is not one.
fn read_tga(bytes: &[u8]) -> Option<Image> {
    let picture = image::load_from_memory_with_format(bytes, image::ImageFormat::Tga).ok()?;
    let rgba = picture.to_rgba8();
    Some(Image {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

/// `pcxheader_t`'s size on disk, before the pixels.
const PCX_HEADER: usize = 128;
/// The palette at the file's end, `com_filesize - 768`.
const PCX_PALETTE: usize = 768;
/// A byte from 0xC0 up starts a run of `byte & 0x3F` of the next byte.
const PCX_RUN: u8 = 0xc0;
const PCX_RUN_LENGTH: u8 = 0x3f;

/// `Image_LoadPCX`: an 8-bit, one-plane, run-length PCX of version 5 through the palette at
/// its end. Each row decodes `bytes_per_line` pixels and keeps its first `xmax - xmin + 1`,
/// as the next row's start overwrites the rest in QuakeSpasm, and a line shorter than that
/// leaves the rest of the row zero. `None` for any other file, or
/// one that ends before its pixels, where QuakeSpasm stops with an error or reads past its
/// end (REND-8.2).
fn read_pcx(bytes: &[u8]) -> Option<Image> {
    let header = bytes.get(..PCX_HEADER)?;
    let byte = |at: usize| header.get(at).copied();
    let short = |at: usize| {
        let pair = header.get(at..at.checked_add(2)?)?;
        Some(u16::from_le_bytes(pair.try_into().ok()?))
    };
    let (signature, version, encoding, bits, planes) =
        (byte(0)?, byte(1)?, byte(2)?, byte(3)?, byte(65)?);
    if signature != 0x0a || version != 5 || encoding != 1 || bits != 8 || planes != 1 {
        return None;
    }
    let width = short(8)?.checked_sub(short(4)?)?.checked_add(1)?;
    let height = short(10)?.checked_sub(short(6)?)?.checked_add(1)?;
    let line = usize::from(short(66)?);
    let palette = bytes.get(bytes.len().checked_sub(PCX_PALETTE)?..)?;
    let mut data = bytes.get(PCX_HEADER..)?.iter().copied();
    let mut rgba = Vec::new();
    for _ in 0..height {
        let mut row = Vec::new();
        while row.len() < line {
            let mut value = data.next()?;
            let mut run = 1;
            if value >= PCX_RUN {
                run = value & PCX_RUN_LENGTH;
                value = data.next()?;
            }
            row.extend(core::iter::repeat_n(value, usize::from(run)));
        }
        for x in 0..usize::from(width) {
            // A line shorter than the picture leaves the hunk's zeroes.
            let Some(&index) = row.get(x) else {
                rgba.extend([0; 4]);
                continue;
            };
            let at = usize::from(index).checked_mul(3)?;
            let colour = palette.get(at..at.checked_add(3)?)?;
            rgba.extend(colour);
            rgba.push(255);
        }
    }
    Some(Image {
        width: u32::from(width),
        height: u32::from(height),
        rgba,
    })
}

#[cfg(test)]
pub(crate) mod tests;
