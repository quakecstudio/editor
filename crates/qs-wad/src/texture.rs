// SPDX-License-Identifier: GPL-3.0-or-later

//! Textures for a WAD2 (WAD-6, WAD-7): names checked against what the engine reads, the
//! palette, and an RGBA image quantized to the palette and encoded as a `miptex_t` with its four
//! mip levels.

use core::ops::RangeInclusive;
use std::fmt;

use crate::{WAD_MIPLEVELS, WAD_MIPTEX_HEADER_SIZE, WAD_NAME_SIZE};

/// The longest texture name, leaving room for its NUL (WAD-6.1).
pub const WAD_TEXTURE_NAME_MAX: usize = 15;
/// What a texture's width and height must be multiples of (WAD-6.5, BSP-4.4).
pub const WAD_TEXTURE_ALIGN: u32 = 16;
/// The bytes of a palette: 256 colours of red, green and blue (WAD-7.2, REND-1.8).
pub const WAD_PALETTE_SIZE: usize = 768;
/// The first fullbright index (WAD-7.4, REND-13.4).
pub const WAD_FULLBRIGHT_FIRST: u8 = 224;
/// The index a fence texture draws transparent (WAD-6.4).
pub const WAD_TRANSPARENT: u8 = 255;
/// The least alpha that is opaque (WAD-7.5).
pub const WAD_OPAQUE_ALPHA: u8 = 128;

/// The colours of a palette (WAD-7.2).
const COLOURS: usize = 256;
/// The largest channel value.
const CHANNEL_MAX: i32 = 255;
/// WAD-7.3's distance times 512: red's weight is `RED_BASE` plus the two reds' sum, green's
/// `GREEN`, and blue's `BLUE_BASE` less the two reds' sum.
const RED_BASE: i64 = 1024;
const GREEN: i64 = 2048;
const BLUE_BASE: i64 = 1534;

/// The last ordinary index (WAD-7.4).
const ORDINARY_LAST: u8 = WAD_FULLBRIGHT_FIRST - 1;
/// The last fullbright index of a fence texture, whose 255 is transparent (WAD-7.4).
const FENCE_FULLBRIGHT_LAST: u8 = WAD_TRANSPARENT - 1;
/// Bytes per pixel of an RGBA image.
const RGBA: usize = 4;
/// Bytes per palette colour.
const RGB: usize = 3;
/// Where the alpha byte is in an RGBA pixel.
const ALPHA: usize = 3;
/// The prefix of a sky texture's name (BSP-4.7).
const SKY: &[u8] = b"sky";
/// The least length of an animation frame's name: `+`, the frame, a base (WAD-6.2).
const FRAME_NAME_MIN: usize = 3;
/// Floyd and Steinberg's weights, in sixteenths (WAD-7.6).
const DITHER_RIGHT: i32 = 7;
const DITHER_BELOW_LEFT: i32 = 3;
const DITHER_BELOW: i32 = 5;
const DITHER_BELOW_RIGHT: i32 = 1;
const DITHER_DIVISOR: i32 = 16;

/// What is wrong with a texture name (WAD-6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameFault {
    /// Empty, or longer than [`WAD_TEXTURE_NAME_MAX`] bytes (WAD-6.1).
    Length,
    /// A byte that is not printable ASCII, or a space or `"` (WAD-6.1).
    Byte(u8),
    /// An animation frame whose second byte is not `0` to `9` or `a` to `j` (WAD-6.2).
    Frame,
    /// A prefix with nothing after it (WAD-6.2, WAD-6.3).
    NoBase,
}

/// Why a texture could not be encoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureError {
    /// The name is not one the engine reads (WAD-6).
    Name {
        /// The name as given.
        name: Vec<u8>,
        /// What is wrong with it.
        fault: NameFault,
    },
    /// The width or height is 0 or not a multiple of 16 (WAD-6.5).
    Size {
        /// The width.
        width: u32,
        /// The height.
        height: u32,
    },
    /// The image has not `width * height` RGBA pixels, or the fullbright mask not one entry
    /// per pixel.
    PixelCount {
        /// The count the size needs.
        expected: usize,
        /// The count given.
        got: usize,
    },
    /// The palette is not 768 bytes (WAD-7.2).
    Palette {
        /// Its length.
        len: usize,
    },
    /// A texture that is not a fence texture has pixels with alpha below 128 (WAD-7.5).
    Transparent {
        /// How many.
        count: usize,
    },
}

impl fmt::Display for TextureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name { name, fault } => {
                let name = String::from_utf8_lossy(name);
                match fault {
                    NameFault::Length => write!(
                        f,
                        "texture name \"{name}\" must be 1 to {WAD_TEXTURE_NAME_MAX} bytes"
                    ),
                    NameFault::Byte(byte) => write!(
                        f,
                        "texture name \"{name}\" has the byte 0x{byte:02x}, which a map cannot \
                         hold"
                    ),
                    NameFault::Frame => write!(
                        f,
                        "texture name \"{name}\" starts with + but its frame is not 0 to 9 or a \
                         to j"
                    ),
                    NameFault::NoBase => {
                        write!(f, "texture name \"{name}\" has nothing after its prefix")
                    }
                }
            }
            Self::Size { width, height } => write!(
                f,
                "texture size {width}x{height} is not a multiple of {WAD_TEXTURE_ALIGN} each way"
            ),
            Self::PixelCount { expected, got } => {
                write!(f, "{got} pixels given where the size needs {expected}")
            }
            Self::Palette { len } => {
                write!(f, "a palette is {WAD_PALETTE_SIZE} bytes, not {len}")
            }
            Self::Transparent { count } => write!(
                f,
                "{count} pixels are transparent, but only a texture named {{... can be"
            ),
        }
    }
}

impl core::error::Error for TextureError {}

/// Which cycle of an animation a frame belongs to (BSP-4.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cycle {
    /// Frames `+0` to `+9`.
    Primary,
    /// Frames `+a` to `+j`.
    Alternate,
}

/// What the engine makes of a texture by its name (WAD-6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureKind {
    /// An ordinary texture.
    Plain,
    /// A liquid, `*` (WAD-6.3).
    Liquid,
    /// A sky, `sky` (WAD-6.3).
    Sky,
    /// A frame of an animation, `+` (WAD-6.2).
    Frame {
        /// The frame, 0 to 9.
        frame: u8,
        /// Its cycle.
        cycle: Cycle,
    },
    /// A fence texture, `{`, whose index 255 is transparent (WAD-6.4).
    Fence,
}

/// A texture name the engine reads (WAD-6.1 to WAD-6.4).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextureName(Vec<u8>);

impl TextureName {
    /// Checks `name`.
    ///
    /// Implements WAD-6.1 to WAD-6.3.
    ///
    /// # Errors
    ///
    /// [`TextureError::Name`] with what is wrong.
    pub fn parse(name: &[u8]) -> Result<Self, TextureError> {
        let fault = |fault| TextureError::Name {
            name: name.to_vec(),
            fault,
        };
        if name.is_empty() || name.len() > WAD_TEXTURE_NAME_MAX {
            return Err(fault(NameFault::Length));
        }
        if let Some(&byte) = name
            .iter()
            .find(|&&byte| !byte.is_ascii_graphic() || byte == b'"')
        {
            return Err(fault(NameFault::Byte(byte)));
        }
        let parsed = Self(name.to_vec());
        match parsed.kind() {
            Some(_) => Ok(parsed),
            None if name.first() == Some(&b'+') && name.len() >= FRAME_NAME_MIN => {
                Err(fault(NameFault::Frame))
            }
            None => Err(fault(NameFault::NoBase)),
        }
    }

    /// The name's bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// What the engine makes of it; `None` for a prefix with nothing after it, or a bad frame.
    fn kind(&self) -> Option<TextureKind> {
        let name = self.0.as_slice();
        match name {
            [b'+', frame, _, ..] => {
                let frame = frame.to_ascii_lowercase();
                match frame {
                    b'0'..=b'9' => Some(TextureKind::Frame {
                        frame: frame.wrapping_sub(b'0'),
                        cycle: Cycle::Primary,
                    }),
                    b'a'..=b'j' => Some(TextureKind::Frame {
                        frame: frame.wrapping_sub(b'a'),
                        cycle: Cycle::Alternate,
                    }),
                    _ => None,
                }
            }
            [b'+', ..] | [b'*'] => None,
            [b'*', _, ..] => Some(TextureKind::Liquid),
            [b'{', ..] => Some(TextureKind::Fence),
            _ if name == SKY => None,
            _ if name.starts_with(SKY) => Some(TextureKind::Sky),
            _ => Some(TextureKind::Plain),
        }
    }

    /// What the engine makes of the texture (WAD-6).
    #[must_use]
    pub fn texture_kind(&self) -> TextureKind {
        self.kind().unwrap_or(TextureKind::Plain)
    }

    /// The `miptex_t` header of a texture of this name (WAD-7.1).
    fn miptex_header(&self, width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut field = [0_u8; WAD_NAME_SIZE];
        for (slot, &byte) in field.iter_mut().zip(self.as_bytes()) {
            *slot = byte;
        }
        out.extend_from_slice(&field);
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        let area = width.saturating_mul(height);
        let mut offset = u32::try_from(WAD_MIPTEX_HEADER_SIZE).unwrap_or(u32::MAX);
        for level in 0..WAD_MIPLEVELS {
            out.extend_from_slice(&offset.to_le_bytes());
            let shift = u32::try_from(level.saturating_mul(2)).unwrap_or(u32::MAX);
            offset = offset.saturating_add(area.checked_shr(shift).unwrap_or(0));
        }
        out
    }
}

/// The 256 colours texture indices name (WAD-7.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette([[u8; RGB]; COLOURS]);

/// An entry of a palette: what each byte of a texture's pixels names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PaletteIndex(u8);

impl PaletteIndex {
    /// Entry `index`.
    #[must_use]
    pub const fn new(index: u8) -> Self {
        Self(index)
    }

    /// The entry's number, as a pixel stores it.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl Palette {
    /// Reads `gfx/palette.lmp`.
    ///
    /// Implements WAD-7.2.
    ///
    /// # Errors
    ///
    /// [`TextureError::Palette`] for any length but 768.
    pub fn from_lmp(bytes: &[u8]) -> Result<Self, TextureError> {
        if bytes.len() != WAD_PALETTE_SIZE {
            return Err(TextureError::Palette { len: bytes.len() });
        }
        let mut colours = [[0_u8; RGB]; COLOURS];
        let (chunks, _) = bytes.as_chunks::<RGB>();
        for (colour, chunk) in colours.iter_mut().zip(chunks) {
            *colour = *chunk;
        }
        Ok(Self(colours))
    }

    /// The colour of `index`.
    #[must_use]
    pub fn colour(&self, index: PaletteIndex) -> [u8; RGB] {
        self.0
            .get(usize::from(index.get()))
            .copied()
            .unwrap_or_default()
    }

    /// The index in `range` nearest `colour`, the lower of equals.
    ///
    /// Implements WAD-7.3.
    #[must_use]
    pub fn nearest(&self, colour: [u8; RGB], range: RangeInclusive<u8>) -> PaletteIndex {
        let mut best = (u64::MAX, PaletteIndex(*range.start()));
        for entry in range.map(PaletteIndex) {
            let distance = distance(colour, self.colour(entry));
            if distance < best.0 {
                best = (distance, entry);
            }
        }
        best.1
    }
}

/// 512 times WAD-7.3's distance, exact in integers.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "spec WAD-7.3: channels below 256 bound every term well inside i64"
)]
fn distance(a: [u8; RGB], b: [u8; RGB]) -> u64 {
    let [ar, ag, ab] = a.map(i64::from);
    let [br, bg, bb] = b.map(i64::from);
    let (dr, dg, db) = (ar - br, ag - bg, ab - bb);
    let reds = ar + br;
    let scaled = (RED_BASE + reds) * dr * dr + GREEN * dg * dg + (BLUE_BASE - reds) * db * db;
    u64::try_from(scaled).unwrap_or(u64::MAX)
}

/// Whether to spread each pixel's error to its neighbours (WAD-7.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dither {
    /// Each pixel matched alone.
    #[default]
    None,
    /// Floyd and Steinberg's error diffusion.
    FloydSteinberg,
}

/// What index 255 is in a texture (WAD-6.4, WAD-7.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Top {
    /// The last fullbright colour.
    Fullbright,
    /// Transparent, in a fence texture.
    Transparent,
}

/// How a pixel is matched (WAD-7.4, WAD-7.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Ordinary,
    Fullbright,
    Transparent,
}

/// An RGBA image to encode: rows from the top left, four bytes a pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RgbaImage<'a> {
    width: u32,
    height: u32,
    pixels: &'a [u8],
}

impl<'a> RgbaImage<'a> {
    /// An image of `width` by `height` pixels.
    ///
    /// # Errors
    ///
    /// [`TextureError::PixelCount`] when `pixels` is not four bytes for each pixel.
    pub fn new(width: u32, height: u32, pixels: &'a [u8]) -> Result<Self, TextureError> {
        let expected = pixel_count(width, height).saturating_mul(RGBA);
        if pixels.len() != expected {
            return Err(TextureError::PixelCount {
                expected,
                got: pixels.len(),
            });
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// The image as a texture lump named `name`: quantized to `palette` and with its four mip
    /// levels, the pixels `fullbright` marks matched among the fullbright colours.
    ///
    /// Implements WAD-6.5 and WAD-7.
    ///
    /// # Errors
    ///
    /// [`TextureError::Size`], [`TextureError::PixelCount`] for a mask of another size, and
    /// [`TextureError::Transparent`].
    pub fn encode(
        &self,
        name: &TextureName,
        palette: &Palette,
        dither: Dither,
        fullbright: Option<&[bool]>,
    ) -> Result<Vec<u8>, TextureError> {
        let (width, height) = (self.width, self.height);
        if width == 0
            || height == 0
            || !width.is_multiple_of(WAD_TEXTURE_ALIGN)
            || !height.is_multiple_of(WAD_TEXTURE_ALIGN)
        {
            return Err(TextureError::Size { width, height });
        }
        let count = pixel_count(width, height);
        if let Some(mask) = fullbright
            && mask.len() != count
        {
            return Err(TextureError::PixelCount {
                expected: count,
                got: mask.len(),
            });
        }
        let top = if name.texture_kind() == TextureKind::Fence {
            Top::Transparent
        } else {
            Top::Fullbright
        };
        let (pixels, _) = self.pixels.as_chunks::<RGBA>();
        let classes: Vec<Class> = pixels
            .iter()
            .enumerate()
            .map(|(at, pixel)| {
                let marked = fullbright.and_then(|mask| mask.get(at)).copied() == Some(true);
                if pixel.get(ALPHA).copied().unwrap_or(u8::MAX) < WAD_OPAQUE_ALPHA {
                    Class::Transparent
                } else if marked {
                    Class::Fullbright
                } else {
                    Class::Ordinary
                }
            })
            .collect();
        let transparent = classes.iter().filter(|&&c| c == Class::Transparent).count();
        if transparent > 0 && top == Top::Fullbright {
            return Err(TextureError::Transparent { count: transparent });
        }
        let base = self.quantize(&classes, palette, dither, top);
        let mut out = name.miptex_header(width, height);
        out.extend_from_slice(&base);
        for level in 1..WAD_MIPLEVELS {
            out.extend(mip_level(&base, width, height, level, palette, top));
        }
        Ok(out)
    }

    /// Level 0's indices (WAD-7.4 to WAD-7.6).
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::integer_division,
        reason = "spec WAD-7.6: errors in sixteenths of channels below 256, bounded far inside \
                  i32; the division is the diffusion's"
    )]
    fn quantize(&self, classes: &[Class], palette: &Palette, dither: Dither, top: Top) -> Vec<u8> {
        let width = usize::try_from(self.width).unwrap_or_default();
        let mut errors = vec![[0_i32; RGB]; classes.len()];
        let mut out = Vec::with_capacity(classes.len());
        let (pixels, _) = self.pixels.as_chunks::<RGBA>();
        for (at, (pixel, &class)) in pixels.iter().zip(classes).enumerate() {
            let mut colour = [0_u8; RGB];
            for (channel, slot) in colour.iter_mut().enumerate() {
                let value = i32::from(pixel.get(channel).copied().unwrap_or_default());
                let error = errors
                    .get(at)
                    .and_then(|e| e.get(channel))
                    .copied()
                    .unwrap_or(0);
                *slot = u8::try_from((value + error / DITHER_DIVISOR).clamp(0, CHANNEL_MAX))
                    .unwrap_or(0);
            }
            let index = match class {
                Class::Transparent => WAD_TRANSPARENT,
                Class::Fullbright => palette.nearest(colour, top.fullbright_range()).get(),
                Class::Ordinary => palette.nearest(colour, 0..=ORDINARY_LAST).get(),
            };
            out.push(index);
            if class != Class::Ordinary || dither == Dither::None {
                continue;
            }
            let chosen = palette.colour(PaletteIndex(index));
            let error = [0, 1, 2].map(|channel| {
                i32::from(colour.get(channel).copied().unwrap_or_default())
                    - i32::from(chosen.get(channel).copied().unwrap_or_default())
            });
            let column = at % width;
            let mut spread = |target: Option<usize>, weight: i32| {
                let Some(target) = target else {
                    return;
                };
                if classes.get(target) != Some(&Class::Ordinary) {
                    return;
                }
                if let Some(slot) = errors.get_mut(target) {
                    for (sum, part) in slot.iter_mut().zip(error) {
                        *sum += part * weight;
                    }
                }
            };
            let last = column + 1 == width;
            spread((!last).then(|| at + 1), DITHER_RIGHT);
            spread((column > 0).then(|| at + width - 1), DITHER_BELOW_LEFT);
            spread(Some(at + width), DITHER_BELOW);
            spread((!last).then(|| at + width + 1), DITHER_BELOW_RIGHT);
        }
        out
    }
}

impl Top {
    /// The indices a fullbright pixel is matched among (WAD-7.4).
    const fn fullbright_range(self) -> RangeInclusive<u8> {
        match self {
            Self::Transparent => WAD_FULLBRIGHT_FIRST..=FENCE_FULLBRIGHT_LAST,
            Self::Fullbright => WAD_FULLBRIGHT_FIRST..=WAD_TRANSPARENT,
        }
    }
}

fn pixel_count(width: u32, height: u32) -> usize {
    usize::try_from(width)
        .unwrap_or(usize::MAX)
        .saturating_mul(usize::try_from(height).unwrap_or(usize::MAX))
}

/// Level `level` of 1 to 3, from level 0's indices `base` (WAD-7.7).
#[expect(
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    reason = "spec WAD-7.7: block positions inside a size already checked, sums of at most 64 \
              channels below 256, and the means' rounding down"
)]
fn mip_level(
    base: &[u8],
    width: u32,
    height: u32,
    level: usize,
    palette: &Palette,
    top: Top,
) -> Vec<u8> {
    let (width, height) = (
        usize::try_from(width).unwrap_or_default(),
        usize::try_from(height).unwrap_or_default(),
    );
    let step = 1_usize << level;
    let mut out = Vec::with_capacity((width / step) * (height / step));
    for first_row in (0..height).step_by(step) {
        for left in (0..width).step_by(step) {
            let mut transparent = 0_usize;
            let mut sums = [([0_u32; RGB], 0_u32); 2];
            for y in first_row..first_row + step {
                for x in left..left + step {
                    let index = base.get(y * width + x).copied().unwrap_or_default();
                    if top == Top::Transparent && index == WAD_TRANSPARENT {
                        transparent += 1;
                        continue;
                    }
                    let bright = usize::from(index >= WAD_FULLBRIGHT_FIRST);
                    if let Some((sum, count)) = sums.get_mut(bright) {
                        for (total, channel) in
                            sum.iter_mut().zip(palette.colour(PaletteIndex(index)))
                        {
                            *total += u32::from(channel);
                        }
                        *count += 1;
                    }
                }
            }
            if transparent * 2 >= step * step {
                out.push(WAD_TRANSPARENT);
                continue;
            }
            let [(ordinary, ordinary_count), (bright, bright_count)] = sums;
            let (sum, count, range) = if bright_count * 2 > bright_count + ordinary_count {
                (bright, bright_count, top.fullbright_range())
            } else {
                (ordinary, ordinary_count, 0..=ORDINARY_LAST)
            };
            let mean = sum.map(|total| u8::try_from(total / count.max(1)).unwrap_or(u8::MAX));
            out.push(palette.nearest(mean, range).get());
        }
    }
    out
}

#[cfg(test)]
mod tests;
