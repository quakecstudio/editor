// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/model.c, Mod_LoadSpriteModel, Mod_LoadSpriteFrame, Mod_LoadSpriteGroup

//! Sprites: id's `.spr` files, read as `Mod_LoadSpriteModel` reads them (SPR-1, SPR-2).

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;

use core::fmt;

/// The four bytes a sprite starts with, id's `IDSPRITEHEADER` (SPR-1.2).
pub const SPR_MAGIC: [u8; 4] = *b"IDSP";

/// The only sprite version the engine loads, id's `SPRITE_VERSION` (SPR-1.3).
pub const SPR_VERSION: i32 = 1;

/// Bytes in the header, id's `sizeof(dsprite_t)` (SPR-1.1).
pub const SPR_HEADER_SIZE: usize = 36;

/// Bytes in a frame's header, id's `sizeof(dspriteframe_t)` (SPR-2.2).
pub const SPR_FRAME_HEADER_SIZE: usize = 16;

/// Sprite type `SPR_VP_PARALLEL_UPRIGHT`.
pub const SPR_VP_PARALLEL_UPRIGHT: i32 = 0;
/// Sprite type `SPR_FACING_UPRIGHT`.
pub const SPR_FACING_UPRIGHT: i32 = 1;
/// Sprite type `SPR_VP_PARALLEL`.
pub const SPR_VP_PARALLEL: i32 = 2;
/// Sprite type `SPR_ORIENTED`.
pub const SPR_ORIENTED: i32 = 3;
/// Sprite type `SPR_VP_PARALLEL_ORIENTED`.
pub const SPR_VP_PARALLEL_ORIENTED: i32 = 4;

/// Sync type `ST_SYNC`.
pub const SPR_ST_SYNC: i32 = 0;
/// Sync type `ST_RAND`.
pub const SPR_ST_RAND: i32 = 1;

/// Frame type `SPR_SINGLE`: one frame (SPR-2.1).
pub const SPR_SINGLE: i32 = 0;
/// Frame type `SPR_GROUP`: a frame group; any non-zero type is read as one (SPR-2.1).
pub const SPR_GROUP: i32 = 1;

/// Why a sprite could not be read.
///
/// [`SprError::WrongVersion`] is written to follow the file's name, as id's is; the other
/// messages are whole.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SprError {
    /// The file does not start with `IDSP` (SPR-1.2).
    WrongId,
    /// The version is not 1 (SPR-1.3).
    WrongVersion {
        /// The stored version.
        version: i32,
    },
    /// The sprite has fewer than one frame entry (SPR-1.4).
    InvalidFrameCount {
        /// The stored `numframes`.
        numframes: i32,
    },
    /// A frame group has fewer than one frame (SPR-2.7).
    InvalidGroupCount {
        /// The stored group `numframes`.
        numframes: i32,
    },
    /// A frame's size is negative or overflows (SPR-2.4).
    BadFrameSize {
        /// The stored width.
        width: i32,
        /// The stored height.
        height: i32,
    },
    /// A frame group's interval is 0 or less (SPR-2.6).
    BadInterval {
        /// The stored interval.
        interval: f32,
    },
    /// A structure runs past the end of the file (SPR-1.6, SPR-2.4, SPR-2.8).
    Truncated {
        /// Offset of the structure that does not fit.
        offset: usize,
    },
}

impl fmt::Display for SprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongId => f.write_str("is not a sprite (no IDSP id)"),
            Self::WrongVersion { version } => write!(
                f,
                "has wrong version number ({version} should be {SPR_VERSION})"
            ),
            Self::InvalidFrameCount { numframes } => {
                write!(f, "Mod_LoadSpriteModel: Invalid # of frames: {numframes}")
            }
            Self::InvalidGroupCount { numframes } => {
                write!(f, "Mod_LoadSpriteGroup: Invalid # of frames: {numframes}")
            }
            Self::BadFrameSize { width, height } => {
                write!(f, "sprite frame of {width} by {height} pixels is invalid")
            }
            Self::BadInterval { .. } => f.write_str("Mod_LoadSpriteGroup: interval<=0"),
            Self::Truncated { offset } => {
                write!(
                    f,
                    "sprite data at offset {offset} runs past the end of the file"
                )
            }
        }
    }
}

impl core::error::Error for SprError {}

/// A sprite's header, id's `dsprite_t` after `Mod_LoadSpriteModel` (SPR-1.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteHeader {
    /// The orientation, one of the `SPR_` type constants; not checked (SPR-1.5).
    pub sprite_type: i32,
    /// Not read by the engine.
    pub bounding_radius: f32,
    /// The largest frame width, `maxwidth`.
    pub width: i32,
    /// The largest frame height, `maxheight`.
    pub height: i32,
    /// The number of frame entries, at least 1.
    pub numframes: i32,
    /// Not used by the renderer.
    pub beam_length: f32,
    /// `ST_SYNC` or `ST_RAND`; not checked.
    pub sync_type: i32,
}

/// One frame of pixels, id's `dspriteframe_t` and its bitmap (SPR-2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpriteFrame<'a> {
    /// `origin[0]`, `origin[1]`.
    pub origin: [i32; 2],
    /// Width in pixels, not negative.
    pub width: i32,
    /// Height in pixels, not negative.
    pub height: i32,
    /// `width * height` palette indices, row by row from the top left.
    pub pixels: &'a [u8],
}

impl SpriteFrame<'_> {
    /// `origin[1]` (SPR-2.3).
    #[must_use]
    pub fn up(&self) -> i32 {
        self.origin[1]
    }

    /// `origin[1] - height`, wrapping as on x86 (SPR-2.3).
    #[must_use]
    pub fn down(&self) -> i32 {
        self.origin[1].wrapping_sub(self.height)
    }

    /// `origin[0]` (SPR-2.3).
    #[must_use]
    pub fn left(&self) -> i32 {
        self.origin[0]
    }

    /// `width + origin[0]`, wrapping as on x86 (SPR-2.3).
    #[must_use]
    pub fn right(&self) -> i32 {
        self.width.wrapping_add(self.origin[0])
    }
}

/// A frame group: an animation of frames and the time each one ends at (SPR-2.5).
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteGroup<'a> {
    /// The stored frame type: [`SPR_GROUP`] or any other non-zero value (SPR-2.1).
    pub frame_type: i32,
    /// One interval per frame, each greater than 0 or NaN (SPR-2.6).
    pub intervals: Vec<f32>,
    /// The frames, at least one.
    pub frames: Vec<SpriteFrame<'a>>,
}

/// One frame entry of a sprite (SPR-2.1).
#[derive(Clone, Debug, PartialEq)]
pub enum SpriteFrameEntry<'a> {
    /// Frame type [`SPR_SINGLE`].
    Single(SpriteFrame<'a>),
    /// Any other frame type.
    Group(SpriteGroup<'a>),
}

/// A loaded sprite: its header and frame entries, whose pixels borrow the file's bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Sprite<'a> {
    /// The header.
    pub header: SpriteHeader,
    /// `numframes` frame entries in file order.
    pub frames: Vec<SpriteFrameEntry<'a>>,
}

impl<'a> Sprite<'a> {
    /// Loads a sprite held in memory, id's `Mod_LoadSpriteModel`.
    ///
    /// Implements SPR-1 and SPR-2: checks the id and version, then walks the frame entries
    /// in order with `Mod_LoadSpriteFrame` and `Mod_LoadSpriteGroup`, checking every size
    /// against the file.
    ///
    /// # Errors
    ///
    /// Any [`SprError`].
    pub fn read(bytes: &'a [u8]) -> Result<Self, SprError> {
        // One 32-bit comparison, which coverage-guided fuzzers can solve.
        if le_i32(bytes, 0) != Some(i32::from_le_bytes(SPR_MAGIC)) {
            return Err(SprError::WrongId);
        }
        let header = bytes
            .get(..SPR_HEADER_SIZE)
            .ok_or(SprError::Truncated { offset: 0 })?;
        let word = |index: usize| le_i32(header, index.saturating_mul(4)).unwrap_or_default();
        let float = |index: usize| f32::from_bits(word(index).cast_unsigned());
        let version = word(1);
        if version != SPR_VERSION {
            return Err(SprError::WrongVersion { version });
        }
        let header = SpriteHeader {
            sprite_type: word(2),
            bounding_radius: float(3),
            width: word(4),
            height: word(5),
            numframes: word(6),
            beam_length: float(7),
            sync_type: word(8),
        };
        let Ok(count @ 1..) = usize::try_from(header.numframes) else {
            return Err(SprError::InvalidFrameCount {
                numframes: header.numframes,
            });
        };
        let mut pos = SPR_HEADER_SIZE;
        let mut frames = Vec::new();
        for _ in 0..count {
            let frame_type = le_i32(bytes, pos).ok_or(SprError::Truncated { offset: pos })?;
            pos = pos.saturating_add(4);
            let entry = if frame_type == SPR_SINGLE {
                SpriteFrameEntry::Single(mod_load_sprite_frame(bytes, &mut pos)?)
            } else {
                SpriteFrameEntry::Group(mod_load_sprite_group(bytes, &mut pos, frame_type)?)
            };
            frames.push(entry);
        }
        Ok(Self { header, frames })
    }

    /// The model's bounds as `Mod_LoadSpriteModel` sets them: `mins` then `maxs`, each x, y,
    /// z, from `width` and `height` with C integer division (SPR-1.7). The engine stores
    /// them as floats.
    #[must_use]
    pub fn bounds(&self) -> ([i32; 3], [i32; 3]) {
        let half = |value: i32| value.checked_div(2).unwrap_or_default();
        let width = half(self.header.width);
        let height = half(self.header.height);
        let min_width = half(self.header.width.wrapping_neg());
        let min_height = half(self.header.height.wrapping_neg());
        ([min_width, min_width, min_height], [width, width, height])
    }
}

/// Reads one frame at `*pos` and moves `*pos` past its pixels, id's `Mod_LoadSpriteFrame`.
///
/// Implements SPR-2.2 and SPR-2.4.
fn mod_load_sprite_frame<'a>(
    bytes: &'a [u8],
    pos: &mut usize,
) -> Result<SpriteFrame<'a>, SprError> {
    let start = *pos;
    let truncated = SprError::Truncated { offset: start };
    let header = start
        .checked_add(SPR_FRAME_HEADER_SIZE)
        .and_then(|end| bytes.get(start..end))
        .ok_or(truncated)?;
    let word = |index: usize| le_i32(header, index.saturating_mul(4)).unwrap_or_default();
    let (width, height) = (word(2), word(3));
    let bad_size = SprError::BadFrameSize { width, height };
    if width < 0 || height < 0 {
        return Err(bad_size);
    }
    let size = width.checked_mul(height).ok_or(bad_size)?;
    let size = usize::try_from(size).map_err(|_| bad_size)?;
    let pixels_start = start.saturating_add(SPR_FRAME_HEADER_SIZE);
    let end = pixels_start.checked_add(size).ok_or(truncated)?;
    let pixels = bytes.get(pixels_start..end).ok_or(truncated)?;
    *pos = end;
    Ok(SpriteFrame {
        origin: [word(0), word(1)],
        width,
        height,
        pixels,
    })
}

/// Reads one frame group at `*pos` and moves `*pos` past it, id's `Mod_LoadSpriteGroup`.
///
/// Implements SPR-2.5 to SPR-2.8.
fn mod_load_sprite_group<'a>(
    bytes: &'a [u8],
    pos: &mut usize,
    frame_type: i32,
) -> Result<SpriteGroup<'a>, SprError> {
    let numframes = le_i32(bytes, *pos).ok_or(SprError::Truncated { offset: *pos })?;
    let Ok(count @ 1..) = usize::try_from(numframes) else {
        return Err(SprError::InvalidGroupCount { numframes });
    };
    *pos = pos.saturating_add(4);
    let mut intervals = Vec::new();
    for _ in 0..count {
        let bits = le_i32(bytes, *pos).ok_or(SprError::Truncated { offset: *pos })?;
        let interval = f32::from_bits(bits.cast_unsigned());
        // SPR-2.6: id's test is `interval <= 0.0`, so NaN passes.
        if interval <= 0.0 {
            return Err(SprError::BadInterval { interval });
        }
        intervals.push(interval);
        *pos = pos.saturating_add(4);
    }
    let mut frames = Vec::new();
    for _ in 0..count {
        frames.push(mod_load_sprite_frame(bytes, pos)?);
    }
    Ok(SpriteGroup {
        frame_type,
        intervals,
        frames,
    })
}

/// The signed little-endian 32-bit integer at `offset`, or `None` past the end of `bytes`.
fn le_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    let word = bytes.get(offset..offset.checked_add(4)?)?;
    Some(i32::from_le_bytes(<[u8; 4]>::try_from(word).ok()?))
}
