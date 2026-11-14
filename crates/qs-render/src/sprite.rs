// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-2001 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/r_sprite.c, R_GetSpriteFrame, R_DrawSpriteModel

//! Sprites, the `.spr` explosions, flames and decals, as QuakeSpasm draws them (REND-11):
//! each frame a quad of the frame's size facing the view by the sprite's type.

use qs_mdl::spr::{
    SPR_FACING_UPRIGHT, SPR_ORIENTED, SPR_VP_PARALLEL, SPR_VP_PARALLEL_ORIENTED,
    SPR_VP_PARALLEL_UPRIGHT, Sprite, SpriteFrame, SpriteFrameEntry,
};

use crate::mesh::{Image, Palette};

/// The palette index a sprite does not draw, `GL_LoadTexture (..., alpha)` with
/// `TEXPREF_ALPHA` (REND-11.2).
const TRANSPARENT: u8 = 255;
/// `SPR_ANGLED`, FTEQW's eight-direction group that QuakeSpasm honours (REND-11.1).
const SPR_ANGLED: i32 = 5;
/// The eight directions of an angled group, `dir & 7` (REND-11.1).
const ANGLED_DIRECTIONS: i64 = 7;
/// `1.125 * M_PI` in `R_GetSpriteFrame`, the offset that puts the first subframe ahead
/// (REND-11.1).
const ANGLED_OFFSET: f64 = 1.125 * core::f64::consts::PI;
/// `4 / M_PI`: the eighths of a turn per radian (REND-11.1).
const ANGLED_SCALE: f64 = 4.0 / core::f64::consts::PI;

/// Why a sprite could not be drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpriteError {
    /// The file is not a sprite `qs-mdl` reads.
    Spr(String),
}

impl core::fmt::Display for SpriteError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Spr(message) => f.write_str(message),
        }
    }
}

impl core::error::Error for SpriteError {}

/// One corner of a sprite's quad on the GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteVertex {
    /// Its position in the map.
    pub position: [f32; 3],
    /// Its texture coordinates.
    pub texture: [f32; 2],
}

/// What an entity showing a sprite is drawn with.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpriteEntity {
    /// `origin`.
    pub origin: [f32; 3],
    /// `angles`: pitch, yaw and roll in degrees.
    pub angles: [f32; 3],
    /// `frame`.
    pub frame: i32,
    /// `syncbase` (CLIENT-2.4).
    pub syncbase: f32,
}

/// The eye the sprites face: its origin and its forward, right and up vectors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Eye {
    /// `r_origin`.
    pub origin: [f32; 3],
    /// `vpn`.
    pub forward: [f32; 3],
    /// `vright`.
    pub right: [f32; 3],
    /// `vup`.
    pub up: [f32; 3],
}

/// A frame's extent and image, `mspriteframe_t` (SPR-2.3).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame {
    up: f32,
    down: f32,
    left: f32,
    right: f32,
    image: usize,
}

/// A frame entry: one frame, or a group with the time each subframe ends at.
#[derive(Clone, Debug, PartialEq)]
enum Entry {
    Single(Frame),
    Group {
        frames: Vec<Frame>,
        intervals: Vec<f32>,
        angled: bool,
    },
}

/// How a sprite faces the eye, `msprite_t.type` (REND-11.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Orientation {
    /// `SPR_VP_PARALLEL_UPRIGHT`: faces the view plane, up is world up.
    ParallelUpright,
    /// `SPR_FACING_UPRIGHT`: faces the eye, up is world up.
    FacingUpright,
    /// `SPR_VP_PARALLEL`: faces the view plane, up is the view's up.
    Parallel,
    /// `SPR_ORIENTED`: the sprite's own angles.
    Oriented,
    /// `SPR_VP_PARALLEL_ORIENTED`: faces the view plane, turned by the sprite's roll.
    ParallelOriented,
    /// Any other type: not drawn (`default: return`).
    Unknown,
}

impl Orientation {
    fn of(sprite_type: i32) -> Self {
        match sprite_type {
            SPR_VP_PARALLEL_UPRIGHT => Self::ParallelUpright,
            SPR_FACING_UPRIGHT => Self::FacingUpright,
            SPR_VP_PARALLEL => Self::Parallel,
            SPR_ORIENTED => Self::Oriented,
            SPR_VP_PARALLEL_ORIENTED => Self::ParallelOriented,
            _ => Self::Unknown,
        }
    }
}

/// A sprite ready to draw: its frames as images, and how it faces the eye.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteModel {
    images: Vec<Image>,
    entries: Vec<Entry>,
    orientation: Orientation,
}

impl SpriteModel {
    /// The sprite `name` (such as `progs/s_explod.spr`) read from `bytes`, its frames through
    /// `palette` with index 255 transparent.
    ///
    /// # Errors
    ///
    /// [`SpriteError::Spr`] when `bytes` is not a sprite.
    pub fn read(name: &str, bytes: &[u8], palette: &Palette) -> Result<Self, SpriteError> {
        let sprite = Sprite::read(bytes).map_err(|e| SpriteError::Spr(format!("{name}: {e}")))?;
        let mut images = Vec::new();
        let mut frame = |f: &SpriteFrame<'_>| {
            images.push(palette.sprite_image(f));
            Frame {
                up: f32_of_i32(f.up()),
                down: f32_of_i32(f.down()),
                left: f32_of_i32(f.left()),
                right: f32_of_i32(f.right()),
                image: images.len().saturating_sub(1),
            }
        };
        let entries = sprite
            .frames
            .iter()
            .map(|entry| match entry {
                SpriteFrameEntry::Single(f) => Entry::Single(frame(f)),
                SpriteFrameEntry::Group(group) => Entry::Group {
                    frames: group.frames.iter().map(&mut frame).collect(),
                    intervals: group.intervals.clone(),
                    angled: group.frame_type == SPR_ANGLED,
                },
            })
            .collect();
        Ok(Self {
            images,
            entries,
            orientation: Orientation::of(sprite.header.sprite_type),
        })
    }

    /// Every frame image, each a texture with transparent texels (REND-11.2).
    #[must_use]
    pub fn images(&self) -> &[Image] {
        &self.images
    }

    /// Appends `entity`'s quad at `time` seconds, as two triangles, to `out`, and returns the
    /// index into [`Self::images`] it is drawn with; `None` for a sprite of an unknown type,
    /// which QuakeSpasm does not draw (REND-11.1, REND-11.2).
    pub fn draw(
        &self,
        entity: &SpriteEntity,
        time: f32,
        eye: &Eye,
        out: &mut Vec<SpriteVertex>,
    ) -> Option<usize> {
        let frame = self.frame(entity, time, eye)?;
        let (s_up, s_right) = self.axes(entity, eye)?;
        let corner = |up: f32, right: f32, texture: [f32; 2]| SpriteVertex {
            position: add(add(entity.origin, scale(s_up, up)), scale(s_right, right)),
            texture,
        };
        // `glBegin (GL_TRIANGLE_FAN)` over the four corners: two triangles from the first.
        let [a, b, c, d] = [
            corner(frame.down, frame.left, [0.0, 1.0]),
            corner(frame.up, frame.left, [0.0, 0.0]),
            corner(frame.up, frame.right, [1.0, 0.0]),
            corner(frame.down, frame.right, [1.0, 1.0]),
        ];
        out.extend([a, b, c, a, c, d]);
        Some(frame.image)
    }

    /// `R_GetSpriteFrame` (REND-11.1).
    fn frame(&self, entity: &SpriteEntity, time: f32, eye: &Eye) -> Option<Frame> {
        // "no such frame": frame 0.
        let entry = usize::try_from(entity.frame)
            .ok()
            .and_then(|f| self.entries.get(f))
            .or_else(|| self.entries.first())?;
        match entry {
            Entry::Single(frame) => Some(*frame),
            Entry::Group {
                frames,
                intervals,
                angled,
            } => {
                if *angled {
                    let (forward, _, _) = angle_vectors(entity.angles);
                    let f = f64::from(dot(eye.forward, forward));
                    let r = f64::from(dot(eye.right, forward));
                    let dir = truncate((r.atan2(f) + ANGLED_OFFSET) * ANGLED_SCALE);
                    let dir = usize::try_from(dir & ANGLED_DIRECTIONS).unwrap_or(0);
                    return frames.get(dir).or_else(|| frames.last()).copied();
                }
                let fullinterval = intervals.last().copied()?;
                let time = time + entity.syncbase;
                // "we guaranteed all interval values are positive"
                let targettime =
                    time - f32_of_i64(truncate(f64::from(time / fullinterval))) * fullinterval;
                let last = frames.len().saturating_sub(1);
                let i = intervals
                    .iter()
                    .take(last)
                    .position(|&interval| interval > targettime)
                    .unwrap_or(last);
                frames.get(i).copied()
            }
        }
    }

    /// `R_DrawSpriteModel`'s `s_up` and `s_right` by the sprite's type (REND-11.2).
    fn axes(&self, entity: &SpriteEntity, eye: &Eye) -> Option<([f32; 3], [f32; 3])> {
        let world_up = [0.0, 0.0, 1.0];
        Some(match self.orientation {
            Orientation::ParallelUpright => (world_up, normalize(cross(eye.forward, world_up))),
            Orientation::FacingUpright => {
                let mut forward = sub(entity.origin, eye.origin);
                forward[2] = 0.0;
                let forward = normalize(forward);
                (world_up, [forward[1], -forward[0], 0.0])
            }
            Orientation::Parallel => (eye.up, eye.right),
            Orientation::Oriented => {
                let (_, right, up) = angle_vectors(entity.angles);
                (up, right)
            }
            Orientation::ParallelOriented => {
                let (sr, cr) = entity.angles[2].to_radians().sin_cos();
                let right = add(scale(eye.right, cr), scale(eye.up, sr));
                let up = add(scale(eye.right, -sr), scale(eye.up, cr));
                (up, right)
            }
            Orientation::Unknown => return None,
        })
    }

    /// Whether the sprite sits on walls and is drawn with a polygon offset, `SPR_ORIENTED`
    /// (REND-11.2).
    #[must_use]
    pub fn is_oriented(&self) -> bool {
        self.orientation == Orientation::Oriented
    }
}

impl Palette {
    /// A sprite frame's pixels through the palette, index 255 transparent (REND-11.2).
    fn sprite_image(&self, frame: &SpriteFrame<'_>) -> Image {
        let width = u32::try_from(frame.width).unwrap_or(0).max(1);
        let height = u32::try_from(frame.height).unwrap_or(0).max(1);
        let rgba = frame
            .pixels
            .iter()
            .flat_map(|&p| {
                if p == TRANSPARENT {
                    [0, 0, 0, 0]
                } else {
                    self.rgba(p)
                }
            })
            .collect();
        Image {
            width,
            height,
            rgba,
        }
    }
}

/// `AngleVectors`: the forward, right and up vectors of `angles` (pitch, yaw, roll).
pub(crate) fn angle_vectors([pitch, yaw, roll]: [f32; 3]) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let (sy, cy) = yaw.to_radians().sin_cos();
    let (sp, cp) = pitch.to_radians().sin_cos();
    let (sr, cr) = roll.to_radians().sin_cos();
    let forward = [cp * cy, cp * sy, -sp];
    let right = [-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp];
    let up = [cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp];
    (forward, right, up)
}

pub(crate) fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn scale(v: [f32; 3], by: f32) -> [f32; 3] {
    [v[0] * by, v[1] * by, v[2] * by]
}

/// `VectorNormalizeFast`'s direction; a zero vector stays zero.
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = dot(v, v).sqrt();
    if length == 0.0 {
        v
    } else {
        scale(v, 1.0 / length)
    }
}

/// C's double to int conversion, toward zero.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec REND-11.1: `(int)` of the quotient and of the angle, toward zero"
)]
fn truncate(value: f64) -> i64 {
    value as i64
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a frame count or an eighth of a turn, exact in f32"
)]
const fn f32_of_i64(value: i64) -> f32 {
    value as f32
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a frame's extent in pixels, exact in f32"
)]
const fn f32_of_i32(value: i32) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests;
