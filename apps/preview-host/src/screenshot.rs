// SPDX-License-Identifier: GPL-3.0-or-later

//! `screenshot`'s image (BRIDGE-4.1): the frame drawn, scaled to the size asked, as a PNG in
//! base64.

use image::codecs::png::PngEncoder;
use image::imageops::{FilterType, resize};
use image::{ExtendedColorType, ImageEncoder, RgbaImage};

/// RFC 4648's base64 alphabet.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// A frame of `width` by `height` pixels, RGBA row by row from the top.
pub(crate) struct Frame {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

/// `frame` as `screenshot`'s result: scaled to `size` when one is asked, encoded as a PNG in
/// base64, with its size.
///
/// # Errors
///
/// Why the frame did not encode.
pub(crate) fn encode(frame: Frame, size: Option<(u32, u32)>) -> Result<(String, u32, u32), String> {
    let image = RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
        .ok_or("the frame is not its size")?;
    let image = match size {
        Some((width, height)) if (width, height) != image.dimensions() => {
            resize(&image, width, height, FilterType::Triangle)
        }
        _ => image,
    };
    let (width, height) = image.dimensions();
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(image.as_raw(), width, height, ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok((base64(&png), width, height))
}

/// `bytes` in RFC 4648 base64, padded with `=`.
pub(crate) fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3).saturating_mul(4));
    let letter = |index: u32| {
        ALPHABET
            .get(usize::try_from(index & 63).unwrap_or(0))
            .map_or('A', |&c| char::from(c))
    };
    for chunk in bytes.chunks(3) {
        let [a, b, c] = [0, 1, 2].map(|i| u32::from(chunk.get(i).copied().unwrap_or(0)));
        let group = (a << 16) | (b << 8) | c;
        out.push(letter(group >> 18));
        out.push(letter(group >> 12));
        out.push(if chunk.len() > 1 {
            letter(group >> 6)
        } else {
            '='
        });
        out.push(if chunk.len() > 2 { letter(group) } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests;
