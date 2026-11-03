// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_rmain.c, R_DrawAliasModel, R_SetupAliasFrame, GL_DrawAliasFrame, R_RotateForEntity
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_model.c, Mod_FloodFillSkin, Mod_LoadAllSkins, Mod_LoadAliasGroup
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_mesh.c, BuildTris
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/cl_main.c, CL_RelinkEntities
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/mathlib.c, anglemod
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/r_alias.c, GL_DrawAliasFrame

//! Alias models, the `.mdl` monsters, weapons, items and torches, as GLQuake draws them
//! (REND-4): each skin through the palette, each frame's vertices turned and moved to the
//! entity and shaded by the light below it.

use qs_client::{DynamicLight, PoseBlend};
use qs_mdl::mdl::{MDL_EF_ROTATE, MDL_HEADER_SIZE, Mdl, MdlFrameEntry};

use crate::anorm_dots::{R_AVERTEXNORMAL_DOTS, SHADEDOT_QUANT};
use crate::mesh::{Image, Palette};

/// `ambientlight > 128` in `R_DrawAliasModel` (REND-4.7).
const AMBIENT_CAP: f32 = 128.0;
/// `ambientlight + shadelight > 192` (REND-4.7).
const SHADE_CAP: f32 = 192.0;
/// The light the torches are given, "make torches full light" (REND-4.7).
const TORCH_LIGHT: f32 = 256.0;
/// `shadelight = shadelight / 200.0` (REND-4.7).
const SHADE_SCALE: f32 = 200.0;
/// The models given full light (REND-4.7).
const TORCHES: [&str; 2] = ["progs/flame2.mdl", "progs/flame.mdl"];
/// `if (e == &cl.viewent && ambientlight < 24)`: the view model's floor (REND-4.11).
const VIEW_MODEL_FLOOR: f32 = 24.0;
/// `if (ambientlight < 8)` for a player: the players' floor (REND-4.11).
const PLAYER_FLOOR: f32 = 8.0;
/// `(int)(cl.time*10) & 3`: a skin group's frames step ten times a second, over four slots
/// (REND-4.2).
const SKIN_RATE: f64 = 10.0;
/// The slots a skin has, `gl_texturenum[i][4]` (REND-4.2).
const SKIN_SLOTS: usize = 4;
/// `anglemod(100*cl.time)`: degrees a second an `EF_ROTATE` model turns (REND-4.9).
const ROTATE_RATE: f64 = 100.0;
/// `anglemod`'s steps in a turn, `65536/360.0` (REND-4.9).
const ANGLE_STEPS: f64 = 65536.0;
/// A turn in degrees.
const TURN: f64 = 360.0;
/// `FLOODFILL_FIFO_SIZE` (REND-4.1).
const FLOODFILL_FIFO_SIZE: usize = 0x1000;
/// The palette index `Mod_FloodFillSkin` uses as its visited marker (REND-4.1).
const TRANSPARENT: u8 = 255;

/// Why an alias model could not be drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AliasError {
    /// The file is not a model `qs-mdl` reads.
    Mdl(String),
}

impl core::fmt::Display for AliasError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Mdl(message) => f.write_str(message),
        }
    }
}

impl core::error::Error for AliasError {}

/// One corner of a model's triangles on the GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AliasVertex {
    /// Its position in the map (REND-4.5).
    pub position: [f32; 3],
    /// Its skin coordinates (REND-4.3).
    pub texture: [f32; 2],
    /// How brightly it is lit, 0 to 1 (REND-4.8).
    pub shade: f32,
}

/// What an entity showing an alias model is drawn with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AliasEntity {
    /// `origin`.
    pub origin: [f32; 3],
    /// `angles`: pitch, yaw and roll in degrees.
    pub angles: [f32; 3],
    /// `frame`.
    pub frame: usize,
    /// `skinnum`.
    pub skin: usize,
    /// `R_LightPoint` at its origin (REND-4.10), with the floors and the dynamic lights
    /// (REND-4.11), as the float `ambientlight`.
    pub light: f32,
    /// The poses it blends between (CLIENT-7.3), `None` to draw `frame` at the time as
    /// GLQuake does (REND-4.4).
    pub blend: Option<PoseBlend>,
    /// What floor its light has (REND-4.11).
    pub kind: AliasKind,
}

/// Which entity an alias model is drawn for, which sets its light's floor (REND-4.11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AliasKind {
    /// Any other entity: no floor.
    #[default]
    Other,
    /// The weapon in the player's hands, `cl.viewent`: a floor of 24.
    ViewModel,
    /// A player, entity 1 to `maxclients`: a floor of 8.
    Player,
}

impl AliasEntity {
    /// An entity at `origin` turned by `angles`, showing `frame` with `skin`, lit by `light`,
    /// drawn as GLQuake draws one.
    #[must_use]
    pub const fn new(
        origin: [f32; 3],
        angles: [f32; 3],
        frame: usize,
        skin: usize,
        light: f32,
    ) -> Self {
        Self {
            origin,
            angles,
            frame,
            skin,
            light,
            blend: None,
            kind: AliasKind::Other,
        }
    }
}

/// A frame's poses, `maliasframedesc_t` (REND-4.4).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame {
    first: usize,
    count: usize,
    interval: f32,
}

/// A triangle's corner: its vertex and its skin coordinates (REND-4.3).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Corner {
    vertex: usize,
    texture: [f32; 2],
}

/// An alias model ready to draw: its skins as images, its poses and its triangles.
#[derive(Clone, Debug, PartialEq)]
pub struct AliasModel {
    images: Vec<Image>,
    skins: Vec<[usize; SKIN_SLOTS]>,
    /// Each pose's vertices: position in the model and normal index.
    poses: Vec<Vec<([f32; 3], u8)>>,
    frames: Vec<Frame>,
    corners: Vec<Corner>,
    rotates: bool,
    torch: bool,
}

impl AliasModel {
    /// The model `name` (such as `progs/soldier.mdl`) read from `bytes`, its skins through
    /// `palette`.
    ///
    /// # Errors
    ///
    /// [`AliasError::Mdl`] when `bytes` is not a model.
    pub fn read(name: &str, bytes: &[u8], palette: &Palette) -> Result<Self, AliasError> {
        let mdl = Mdl::read(bytes).map_err(|e| AliasError::Mdl(e.message(name)))?;
        let header = &mdl.header;
        let width = usize::try_from(header.skinwidth).unwrap_or(0);
        let height = usize::try_from(header.skinheight).unwrap_or(0);
        let (images, skins) = skin_images(bytes, &mdl, palette, (width, height));
        let mut poses = Vec::new();
        let mut frames = Vec::new();
        for entry in &mdl.frames {
            let (list, interval) = match entry {
                MdlFrameEntry::Single(frame) => (core::slice::from_ref(frame), 0.0),
                // REND-4.4: a group's interval is its first.
                MdlFrameEntry::Group {
                    frames, intervals, ..
                } => (frames.as_slice(), intervals.first().copied().unwrap_or(0.0)),
            };
            frames.push(Frame {
                first: poses.len(),
                count: list.len(),
                interval,
            });
            for frame in list {
                poses.push(
                    (0..frame.vertex_count())
                        .filter_map(|i| frame.vertex(i))
                        .map(|v| (mdl.position(v), v.lightnormalindex))
                        .collect(),
                );
            }
        }
        let (fw, fh) = (f32_of(width), f32_of(height));
        let corners = mdl
            .triangles
            .iter()
            .flat_map(|triangle| {
                let mdl = &mdl;
                (0..3).filter_map(move |corner| {
                    let vertex = *triangle.vertindex.get(corner)?;
                    let t = mdl.stverts.get(vertex)?.t;
                    let s = mdl.skin_s(triangle, corner)?;
                    // REND-4.3: the texel's centre over the skin's size.
                    Some(Corner {
                        vertex,
                        texture: [(f32_of_i32(s) + 0.5) / fw, (f32_of_i32(t) + 0.5) / fh],
                    })
                })
            })
            .collect();
        Ok(Self {
            images,
            skins,
            poses,
            frames,
            corners,
            rotates: header.flags & MDL_EF_ROTATE != 0,
            torch: TORCHES.contains(&name),
        })
    }

    /// Every skin image, each a texture to upload as a world texture is (REND-4.3).
    #[must_use]
    pub fn images(&self) -> &[Image] {
        &self.images
    }

    /// Whether the model turns on its own, `EF_ROTATE` (REND-4.9).
    #[must_use]
    pub const fn rotates(&self) -> bool {
        self.rotates
    }

    /// Appends `entity`'s triangles at `time` seconds to `out`, and returns the index into
    /// [`Self::images`] they are drawn with (REND-4.2 to REND-4.8).
    pub fn draw(&self, entity: &AliasEntity, time: f64, out: &mut Vec<AliasVertex>) -> usize {
        let shadelight = shade_light(entity.light, self.torch);
        let dots = shade_dots(entity.angles[1]);
        // REND-4.4 or, blending, CLIENT-7.3: the two poses and how far between them.
        let (from, to, blend) = match entity.blend {
            Some(PoseBlend {
                previous,
                current,
                blend,
            }) => (previous, current, blend),
            None => {
                let pose = self.pose(entity.frame, time);
                (pose, pose, 0.0)
            }
        };
        let from = self.poses.get(from).map_or(&[][..], Vec::as_slice);
        let to = self.poses.get(to).map_or(from, Vec::as_slice);
        let [pitch, yaw, roll] = entity.angles;
        // R_RotateForEntity turns an alias model by -angles[0] about y (REND-4.5).
        let turn = [-pitch, yaw, roll];
        let iblend = 1.0 - blend;
        out.extend(self.corners.iter().filter_map(|corner| {
            let &(position1, normal1) = from.get(corner.vertex)?;
            let &(position2, normal2) = to.get(corner.vertex).unwrap_or(&(position1, normal1));
            let dot = |normal: u8| dots.get(usize::from(normal)).copied().unwrap_or(0.0);
            // REND-4.12: `verts1->v * iblend + verts2->v * blend`, and the shade dots the same.
            let mut position = position1;
            for (p, (a, b)) in position
                .iter_mut()
                .zip(position1.into_iter().zip(position2))
            {
                *p = a * iblend + b * blend;
            }
            let shade = dot(normal1) * iblend + dot(normal2) * blend;
            Some(AliasVertex {
                position: rotate_for_entity(entity.origin, turn, position),
                texture: corner.texture,
                shade: (shade * shadelight).clamp(0.0, 1.0),
            })
        }));
        self.skin(entity.skin, time)
    }

    /// `R_SetupAliasFrame`: the pose of `frame` at `time` seconds, frame 0 for one the model
    /// does not have (REND-4.4).
    fn pose(&self, frame: usize, time: f64) -> usize {
        let Some(frame) = self.frames.get(frame).or_else(|| self.frames.first()) else {
            return 0;
        };
        // REND-4.4: a NaN interval, the one not above 0 that loads, holds the first pose. One
        // pose is its first at any time, `% 1`, so it needs no test of its own.
        if frame.interval.is_nan() {
            return frame.first;
        }
        let step = whole(time / f64::from(frame.interval));
        frame
            .first
            .saturating_add(step.checked_rem(frame.count).unwrap_or(0))
    }

    /// The image of skin `skin` at `time` seconds, skin 0 for one the model does not have
    /// (REND-4.2).
    fn skin(&self, skin: usize, time: f64) -> usize {
        let slots = self.skins.get(skin).or_else(|| self.skins.first());
        let slot = whole(time * SKIN_RATE) & (SKIN_SLOTS - 1);
        slots.and_then(|s| s.get(slot)).copied().unwrap_or(0)
    }
}

/// `R_DrawAliasModel`'s `shadelight`: the light at the entity, capped as GLQuake caps it, the
/// torches full light, over 200 (REND-4.7).
#[must_use]
pub fn shade_light(light: f32, torch: bool) -> f32 {
    let ambient = light.min(AMBIENT_CAP);
    // "if (ambientlight + shadelight > 192) shadelight = 192 - ambientlight"
    let shade = if torch {
        TORCH_LIGHT
    } else {
        light.min(SHADE_CAP - ambient)
    };
    shade / SHADE_SCALE
}

/// `R_DrawAliasModel`'s light before the caps: `R_LightPoint` at `origin`, `light`, raised
/// to the view model's floor of 24, then each dynamic light's `radius - distance` when above
/// 0 added, then a player's floor of 8, all in the float `ambientlight` (REND-4.11). The
/// caps and the torches are [`shade_light`].
#[must_use]
pub fn entity_light(light: i32, kind: AliasKind, origin: [f32; 3], lights: &[DynamicLight]) -> f32 {
    let mut light = f32_of_i32(light);
    if kind == AliasKind::ViewModel {
        light = light.max(VIEW_MODEL_FLOOR);
    }
    for dlight in lights {
        let [dx, dy, dz] = [
            origin[0] - dlight.origin[0],
            origin[1] - dlight.origin[1],
            origin[2] - dlight.origin[2],
        ];
        let add = dlight.radius - (dx * dx + dy * dy + dz * dz).sqrt();
        if add > 0.0 {
            light += add;
        }
    }
    if kind == AliasKind::Player {
        light = light.max(PLAYER_FLOOR);
    }
    light
}

/// The row of `r_avertexnormal_dots` for a yaw of `yaw` degrees,
/// `((int)(yaw * (SHADEDOT_QUANT / 360.0))) & (SHADEDOT_QUANT - 1)` (REND-4.8).
fn shade_dots(yaw: f32) -> &'static [f32; 256] {
    let step = f64::from(yaw) * (f64_of(SHADEDOT_QUANT) / TURN);
    let row = truncate(step) & i64_of(SHADEDOT_QUANT.saturating_sub(1));
    let row = usize::try_from(row).unwrap_or(0);
    R_AVERTEXNORMAL_DOTS
        .get(row)
        .unwrap_or(&R_AVERTEXNORMAL_DOTS[0])
}

/// `bobjrotate = anglemod(100*cl.time)`: the yaw of an `EF_ROTATE` model at `time` seconds,
/// in steps of 360/65536 degrees (REND-4.9).
#[must_use]
pub fn bob_rotate(time: f64) -> f32 {
    // anglemod takes a float.
    let angle = f64::from(f32_of_f64(ROTATE_RATE * time));
    let steps = truncate(angle * (ANGLE_STEPS / TURN)) & 0xffff;
    f32_of_f64(TURN / ANGLE_STEPS * f64_of_i64(steps))
}

/// `R_RotateForEntity`: `position` turned by `turn[2]` about x, `turn[0]` about y and
/// `turn[1]` about z, then moved by `origin`, as `glRotatef` turns (REND-1.9, REND-4.5).
pub(crate) fn rotate_for_entity(origin: [f32; 3], turn: [f32; 3], position: [f32; 3]) -> [f32; 3] {
    let [about_y, about_z, about_x] = turn.map(f32::to_radians);
    let [x, y, z] = position;
    let (y, z) = (
        y * about_x.cos() - z * about_x.sin(),
        y * about_x.sin() + z * about_x.cos(),
    );
    let (x, z) = (
        x * about_y.cos() + z * about_y.sin(),
        z * about_y.cos() - x * about_y.sin(),
    );
    let (x, y) = (
        x * about_z.cos() - y * about_z.sin(),
        x * about_z.sin() + y * about_z.cos(),
    );
    let [dx, dy, dz] = origin;
    [x + dx, y + dy, z + dz]
}

/// Every skin's pixels as `Mod_LoadAllSkins` uploads them, and each skin's four slots
/// (REND-4.1, REND-4.2).
fn skin_images(
    bytes: &[u8],
    mdl: &Mdl<'_>,
    palette: &Palette,
    (width, height): (usize, usize),
) -> (Vec<Image>, Vec<[usize; SKIN_SLOTS]>) {
    let size = width.saturating_mul(height);
    let filled = palette.filled_colour();
    // A copy of the file from the first skin's type on: `Mod_FloodFillSkin` fills the
    // pixels after it, `skin`, before every skin it uploads.
    let mut data = bytes.get(MDL_HEADER_SIZE..).unwrap_or_default().to_vec();
    let first = 4..4_usize.saturating_add(size);
    let mut images = Vec::new();
    let mut skins = Vec::new();
    let mut at = 0_usize;
    let mut upload = |data: &mut Vec<u8>, from: usize| -> Option<usize> {
        if let Some(skin) = data.get_mut(first.clone()) {
            flood_fill_skin(skin, (width, height), filled);
        }
        let pixels = data.get(from..from.checked_add(size)?)?;
        images.push(Image {
            width: u32::try_from(width).ok()?,
            height: u32::try_from(height).ok()?,
            rgba: pixels.iter().flat_map(|&p| palette.rgba(p)).collect(),
        });
        Some(images.len().saturating_sub(1))
    };
    for _ in &mdl.skins {
        let Some(kind) = word(&data, at) else { break };
        let pixels = at.saturating_add(4);
        if kind == 0 {
            let image = upload(&mut data, pixels).unwrap_or(0);
            skins.push([image; SKIN_SLOTS]);
            at = pixels.saturating_add(size);
        } else {
            let count = word(&data, pixels).and_then(|n| usize::try_from(n).ok());
            let count = count.unwrap_or(0);
            let mut from = pixels
                .saturating_add(4)
                .saturating_add(count.saturating_mul(4));
            let mut slots = [None; SKIN_SLOTS];
            for j in 0..count {
                if let Some(slot) = slots.get_mut(j & (SKIN_SLOTS - 1)) {
                    *slot = upload(&mut data, from);
                }
                from = from.saturating_add(size);
            }
            // "for (; j < 4; j++) gl_texturenum[i][j&3] = gl_texturenum[i][j - k]"
            for j in count..SKIN_SLOTS {
                let earlier = j.checked_sub(count).and_then(|k| slots.get(k)).copied();
                if let (Some(slot), Some(value)) = (slots.get_mut(j), earlier) {
                    *slot = value;
                }
            }
            skins.push(slots.map(|s| s.unwrap_or(0)));
            at = from;
        }
    }
    (images, skins)
}

/// `Mod_FloodFillSkin`: the background, the region of `skin`'s first pixel's colour joined to
/// it, filled with the colours at its edge so mipmapping draws no halo (REND-4.1).
fn flood_fill_skin(skin: &mut [u8], (width, _height): (usize, usize), filled: u8) {
    let Some(&fill) = skin.first() else { return };
    if fill == filled || fill == TRANSPARENT {
        return;
    }
    let mut fifo = vec![(0_usize, 0_usize); FLOODFILL_FIFO_SIZE];
    let mask = FLOODFILL_FIFO_SIZE - 1;
    let (mut inpt, mut outpt) = (1_usize, 0_usize);
    while outpt != inpt {
        let (x, y) = fifo.get(outpt).copied().unwrap_or_default();
        let mut fdc = filled;
        outpt = outpt.saturating_add(1) & mask;
        let mut step = |nx: usize, ny: usize, fdc: &mut u8| {
            let Some(at) = ny.checked_mul(width).and_then(|r| r.checked_add(nx)) else {
                return;
            };
            match skin.get(at).copied() {
                Some(p) if p == fill => {
                    if let Some(p) = skin.get_mut(at) {
                        *p = TRANSPARENT;
                    }
                    if let Some(entry) = fifo.get_mut(inpt) {
                        *entry = (nx, ny);
                    }
                    inpt = inpt.saturating_add(1) & mask;
                }
                Some(p) if p != TRANSPARENT => *fdc = p,
                _ => {}
            }
        };
        if let Some(left) = x.checked_sub(1) {
            step(left, y, &mut fdc);
        }
        if let Some(right) = x.checked_add(1).filter(|&r| r < width) {
            step(right, y, &mut fdc);
        }
        if let Some(up) = y.checked_sub(1) {
            step(x, up, &mut fdc);
        }
        // `y < skinheight - 1`: a row past the last is outside the skin, which `step` skips.
        step(x, y.saturating_add(1), &mut fdc);
        if let Some(p) = y
            .checked_mul(width)
            .and_then(|r| r.checked_add(x))
            .and_then(|at| skin.get_mut(at))
        {
            *p = fdc;
        }
    }
}

/// The little-endian int at `at`.
fn word(bytes: &[u8], at: usize) -> Option<i32> {
    let word = bytes.get(at..at.checked_add(4)?)?;
    Some(i32::from_le_bytes(<[u8; 4]>::try_from(word).ok()?))
}

/// C's `(int)` of a non-negative double, as an index; 0 for a negative or NaN value.
fn whole(value: f64) -> usize {
    usize::try_from(truncate(value)).unwrap_or(0)
}

/// C's float to int conversion, toward zero (REND-4.4, REND-4.8, REND-4.9).
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec REND-4.8: id converts the double to an int, which truncates toward zero"
)]
fn truncate(value: f64) -> i64 {
    value as i64
}

fn f32_of(value: usize) -> f32 {
    u16::try_from(value).map_or(f32::MAX, f32::from)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "spec REND-4.7: id converts the int light to a float"
)]
const fn f32_of_i32(value: i32) -> f32 {
    value as f32
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "spec REND-4.9: anglemod returns a float"
)]
const fn f32_of_f64(value: f64) -> f32 {
    value as f32
}

#[expect(
    clippy::cast_precision_loss,
    reason = "spec REND-4.9: the steps are at most 65535, exact in a double"
)]
const fn f64_of_i64(value: i64) -> f64 {
    value as f64
}

fn f64_of(value: usize) -> f64 {
    u32::try_from(value).map_or(f64::MAX, f64::from)
}

fn i64_of(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests;
