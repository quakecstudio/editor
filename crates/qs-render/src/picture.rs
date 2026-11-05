// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-2001 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/image.c, Image_LoadImage, Image_LoadPCX

//! Picture files outside the WAD: the TGA and PCX faces of a skybox (REND-8.2) and the
//! images of an MD5 model's skins (REND-12.4).

use qs_wad::QPic;

use crate::mesh::{Image, Palette};
use crate::play::pic_image;

/// A picture file's decoder.
type Decoder = fn(&[u8]) -> Option<Image>;

/// QuakeSpasm-Spiked's `Image_LoadImage`, as the MD5 skins are found (REND-12.4): `name`
/// with each extension in turn, `.tga`, `.png`, `.jpeg`, `.jpg` and `.pcx`, as it is, then
/// under `textures/`, then its last path part under `textures/`; at last `name.lmp` through
/// `palette`. The first file found decides: one that does not read gives no image, as
/// QuakeSpasm-Spiked returns its loader's result.
pub(crate) fn load_image(
    name: &str,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
    palette: &Palette,
) -> Option<Image> {
    let base = name.rsplit('/').next().unwrap_or(name);
    let mut places = vec![name.to_owned(), format!("textures/{name}")];
    if base != name {
        places.push(format!("textures/{base}"));
    }
    let formats: [(&str, Decoder); 5] = [
        ("tga", read_tga),
        ("png", |bytes| decode(bytes, image::ImageFormat::Png)),
        ("jpeg", |bytes| decode(bytes, image::ImageFormat::Jpeg)),
        ("jpg", |bytes| decode(bytes, image::ImageFormat::Jpeg)),
        ("pcx", read_pcx),
    ];
    places
        .iter()
        .flat_map(|place| {
            formats
                .iter()
                .map(move |(ext, decoder)| (place, ext, decoder))
        })
        .find_map(|(place, ext, decoder)| {
            read(&format!("{place}.{ext}")).map(|bytes| decoder(&bytes))
        })
        .unwrap_or_else(|| {
            let bytes = read(&format!("{name}.lmp"))?;
            QPic::parse(&bytes).ok().map(|pic| pic_image(&pic, palette))
        })
}

/// A picture `image` decodes from `bytes` as `format`, rows from the top.
fn decode(bytes: &[u8], format: image::ImageFormat) -> Option<Image> {
    let picture = image::load_from_memory_with_format(bytes, format).ok()?;
    let rgba = picture.to_rgba8();
    Some(Image {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

/// A TGA as `Image_LoadTGA` reads it: its rows from the top, whichever way the file stores
/// them. `None` for a file that is not one.
pub(crate) fn read_tga(bytes: &[u8]) -> Option<Image> {
    decode(bytes, image::ImageFormat::Tga)
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
/// as the next row's start overwrites the rest in QuakeSpasm. `None` for any other file, or
/// one that ends before its pixels, where QuakeSpasm stops with an error or reads past its
/// end (REND-8.2).
pub(crate) fn read_pcx(bytes: &[u8]) -> Option<Image> {
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
    // A line shorter than the picture would make it from no data at all; QuakeSpasm leaves
    // the hunk's zeroes there, and the preview host refuses it (REND-8.2).
    if line < usize::from(width) {
        return None;
    }
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
        for &index in row.iter().take(usize::from(width)) {
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
