// SPDX-License-Identifier: GPL-3.0-or-later

//! One face lit as `LightFace` lights it: its texture vectors and extents, its sample points
//! (LIGHT-3), each light's contribution (LIGHT-4) and the lightmaps written from them (LIGHT-5,
//! LIGHT-6).

use std::sync::atomic::{AtomicUsize, Ordering};

use qs_bsp::{BSP_MAXLIGHTMAPS, BSP_TEX_SPECIAL, Bsp};

use crate::entities::{Entities, Entity, FULL};
use crate::lump::Colour;
use crate::trace::Tree;
use crate::{AXES, LightError, LightWarning, NO_STYLE, Rgb, Sampling, Scale, Vec3, to_f32};

/// Texels to a luxel (LIGHT-3.2).
const LUXEL: f32 = 16.0;
/// [`LUXEL`] as an `int`, as `texmins` is multiplied by it (LIGHT-3.3).
const LUXEL_TEXELS: i32 = 16;
/// Half a luxel, in luxels (LIGHT-3.3).
const HALF: f64 = 0.5;
/// Where a texture axis keeps its offset, after its three components (BSP-8.1).
const OFFSET: usize = 3;
/// The most luxels a lightmap spans on an axis, less one (LIGHT-3.2).
const MAX_EXTENT: i32 = 17;
/// Texels between samples in the full mode, and how far a sample point moves toward the middle
/// each try (LIGHT-3.3, LIGHT-3.4).
const HALF_LUXEL: f32 = 8.0;
/// How many places a sample point is tried at (LIGHT-3.4).
const TRIES: usize = 6;
/// A sample of more than this gives its light's style a lightmap (LIGHT-4.5).
const REAL_LIGHT: f32 = 1.0;
/// The cone of a spotlight with no `angle`, in degrees (LIGHT-4.4).
const DEFAULT_CONE: f64 = 40.0;
/// Degrees to a half turn.
const HALF_TURN: f64 = 180.0;
/// The brightest byte (LIGHT-5.2).
const BRIGHTEST: f32 = 255.0;
/// The full mode's samples per luxel on each axis (LIGHT-1.3).
const EXTRA: usize = 2;
/// A mean of four samples (LIGHT-5.2).
const QUARTER: f64 = 0.25;
/// How far the extents start, before any vertex widens them (`CalcFaceExtents`).
const FAR_MIN: f32 = 999_999.0;
const FAR_MAX: f32 = -99_999.0;
/// A sample point's distance in front of the face's plane (LIGHT-3.5).
const IN_FRONT: f32 = 1.0;

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `VectorMA`: `a + scale * b`, worked in `double` as its `double` scale makes it.
fn ma(a: Vec3, scale: f64, b: Vec3) -> Vec3 {
    [0, 1, 2].map(|i| {
        let (x, y) = (
            a.get(i).copied().unwrap_or_default(),
            b.get(i).copied().unwrap_or_default(),
        );
        to_f32(f64::from(x) + scale * f64::from(y))
    })
}

/// `VectorNormalize`: `v` divided by its length, summed and divided in `double`; as it is when
/// it has none.
fn normalized(v: Vec3) -> Vec3 {
    let length = v
        .iter()
        .fold(0.0_f64, |sum, &c| sum + f64::from(c * c))
        .sqrt();
    if length == 0.0 {
        return v;
    }
    v.map(|c| to_f32(f64::from(c) / length))
}

/// `VectorLength`, summed in `double` and stored in a `float`.
fn length(v: Vec3) -> f32 {
    to_f32(
        v.iter()
            .fold(0.0_f64, |sum, &c| sum + f64::from(c * c))
            .sqrt(),
    )
}

/// A float as C stores it in an `int`, truncating.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec LIGHT-3.2: light stores floor and ceil of the extents in an int"
)]
const fn int_of(value: f32) -> i32 {
    value as i32
}

/// An `int` as a `float`.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec LIGHT-4.2: C converts the int light value to float to compare and subtract"
)]
const fn float_of(value: i32) -> f32 {
    value as f32
}

/// A clamped sample as its byte, truncating (LIGHT-5.2).
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "spec LIGHT-5.2: light stores the clamped total, 0 to 255, in a byte"
)]
const fn byte_of(value: f32) -> u8 {
    value as u8
}

/// A face's lighting: up to four lightmaps, each its style, grey bytes and, for coloured
/// light, its red, green and blue.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Lit {
    pub(crate) styles: Vec<u8>,
    pub(crate) grey: Vec<u8>,
    pub(crate) rgb: Vec<Rgb>,
}

/// The face's frame (`lightinfo_t`): its plane, texture axes, extents and sample points, and
/// the lightmaps so far.
struct Info {
    normal: Vec3,
    dist: f32,
    texorg: Vec3,
    to_world: [Vec3; 2],
    exact_mins: [f32; 2],
    exact_maxs: [f32; 2],
    texmins: [i32; 2],
    texsize: [i32; 2],
    points: Vec<Vec3>,
    styles: Vec<i32>,
    grey: [Vec<f32>; BSP_MAXLIGHTMAPS],
    colour: [Vec<Vec3>; BSP_MAXLIGHTMAPS],
}

/// What a face needs from the map: the BSP, the tree, the lights and the settings.
pub(crate) struct Scene<'a> {
    pub(crate) bsp: &'a Bsp<'a>,
    pub(crate) tree: &'a Tree,
    pub(crate) entities: &'a Entities,
    pub(crate) sampling: Sampling,
    pub(crate) scale: Scale,
    pub(crate) colour: Colour,
}

/// A face's place in the faces lump.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceId(usize);

impl FaceId {
    /// The face at `index`.
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Its place in the faces lump.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// How many faces a thread lights before taking more.
const FACE_BATCH: usize = 16;

/// One face's lighting and its warnings.
type FaceResult = (Option<Lit>, Vec<LightWarning>);

impl Scene<'_> {
    /// Every face lit, in face order, on every thread the machine has.
    pub(crate) fn lit_faces(&self) -> Result<Vec<FaceResult>, LightError> {
        let count = self.bsp.faces.len();
        let threads = std::thread::available_parallelism().map_or(1, usize::from);
        let batches = count.div_ceil(FACE_BATCH);
        let next = AtomicUsize::new(0);
        // The first face that failed: no face after it is lit, as `light` stops there.
        let failed = AtomicUsize::new(usize::MAX);
        let mut results: Vec<Option<Result<FaceResult, LightError>>> =
            std::iter::repeat_with(|| None).take(count).collect();
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..threads.clamp(1, batches.max(1)))
                .map(|_| {
                    s.spawn(|| {
                        let mut mine = Vec::new();
                        loop {
                            let batch = next.fetch_add(1, Ordering::Relaxed);
                            let start = batch.saturating_mul(FACE_BATCH);
                            if start >= count || start > failed.load(Ordering::Relaxed) {
                                break;
                            }
                            for face in start..start.saturating_add(FACE_BATCH).min(count) {
                                let mut warnings = Vec::new();
                                let lit = self.light_face(FaceId::new(face), &mut warnings);
                                if lit.is_err() {
                                    failed.fetch_min(face, Ordering::Relaxed);
                                }
                                mine.push((face, lit.map(|lit| (lit, warnings))));
                            }
                        }
                        mine
                    })
                })
                .collect();
            for handle in handles {
                if let Ok(mine) = handle.join() {
                    for (face, result) in mine {
                        if let Some(slot) = results.get_mut(face) {
                            *slot = Some(result);
                        }
                    }
                }
            }
        });
        results
            .into_iter()
            .map(|result| result.unwrap_or(Err(LightError::Thread)))
            .collect()
    }

    /// `LightFace`: face `index`'s lightmaps, none when it is special or no light reaches it,
    /// with the warnings it prints (LIGHT-3 to LIGHT-6).
    pub(crate) fn light_face(
        &self,
        FaceId(index): FaceId,
        warnings: &mut Vec<LightWarning>,
    ) -> Result<Option<Lit>, LightError> {
        let face = self.bsp.faces.get(index).ok_or(LightError::Format)?;
        let texinfo = self
            .bsp
            .texinfo
            .get(face.texinfo)
            .ok_or(LightError::Format)?;
        if texinfo.flags & BSP_TEX_SPECIAL != 0 {
            return Ok(None);
        }
        let plane = self
            .bsp
            .planes
            .get(face.planenum)
            .ok_or(LightError::Format)?;
        let (normal, dist) = if face.side == 0 {
            (plane.normal, plane.dist)
        } else {
            (plane.normal.map(|c| 0.0 - c), -plane.dist)
        };
        let mut info = Info::new(normal, dist, texinfo.vecs, index)?;
        info.extents(self.bsp, index, texinfo.vecs)?;
        info.points(self.tree, self.sampling, self.colour);
        for light in self.entities.all.iter().filter(|e| e.is_light()) {
            if let Some(warning) = info.single_light(self, light) {
                warnings.push(warning);
            }
        }
        if info.styles.is_empty() {
            return Ok(None);
        }
        Ok(Some(info.written(self.sampling, self.scale, self.colour)))
    }
}

impl Info {
    /// `CalcFaceVectors`: the texture axes back into the world along the texture normal, and
    /// the texture origin one unit in front of the face (LIGHT-3.5).
    fn new(normal: Vec3, dist: f32, vecs: [[f32; 4]; 2], face: usize) -> Result<Self, LightError> {
        let [s, t] = vecs;
        let to_tex = [[s[0], s[1], s[2]], [t[0], t[1], t[2]]];
        let texnormal = normalized([
            t[1] * s[2] - t[2] * s[1],
            t[2] * s[0] - t[0] * s[2],
            t[0] * s[1] - t[1] * s[0],
        ]);
        let mut distscale = dot(texnormal, normal);
        if distscale == 0.0 {
            return Err(LightError::TextureAxis { face: FaceId(face) });
        }
        let mut texnormal = texnormal;
        if distscale < 0.0 {
            distscale = -distscale;
            texnormal = texnormal.map(|c| 0.0 - c);
        }
        let distscale = 1.0 / distscale;
        let to_world = to_tex.map(|axis| {
            let len = length(axis);
            let along = dot(axis, normal) * distscale;
            let back = ma(axis, f64::from(-along), texnormal);
            let scale = (1.0 / len) * (1.0 / len);
            back.map(|c| c * scale)
        });
        let [w0, w1] = to_world;
        let texorg = [0, 1, 2].map(|i| {
            let (a, b) = (
                w0.get(i).copied().unwrap_or_default(),
                w1.get(i).copied().unwrap_or_default(),
            );
            -s[OFFSET] * a - t[OFFSET] * b
        });
        let off = (dot(texorg, normal) - dist - IN_FRONT) * distscale;
        let texorg = ma(texorg, f64::from(-off), texnormal);
        Ok(Self {
            normal,
            dist,
            texorg,
            to_world,
            exact_mins: [0.0; 2],
            exact_maxs: [0.0; 2],
            texmins: [0; 2],
            texsize: [0; 2],
            points: Vec::new(),
            styles: Vec::new(),
            grey: Default::default(),
            colour: Default::default(),
        })
    }

    /// `CalcFaceExtents`: the face's texture coordinates' bounds, and its lightmap's in luxels;
    /// over 17 luxels stops the run (LIGHT-3.2).
    fn extents(
        &mut self,
        bsp: &Bsp<'_>,
        face: usize,
        vecs: [[f32; 4]; 2],
    ) -> Result<(), LightError> {
        let found = bsp.faces.get(face).ok_or(LightError::Format)?;
        let mut mins = [FAR_MIN; 2];
        let mut maxs = [FAR_MAX; 2];
        let edges = bsp
            .surfedges
            .iter()
            .skip(found.firstedge)
            .take(found.numedges);
        for &e in edges {
            let vertex = if let Ok(e) = usize::try_from(e) {
                bsp.edges.get(e).map(|edge| edge[0])
            } else {
                usize::try_from(e.unsigned_abs())
                    .ok()
                    .and_then(|e| bsp.edges.get(e))
                    .map(|edge| edge[1])
            };
            let point = vertex
                .and_then(|v| usize::try_from(v).ok())
                .and_then(|v| bsp.vertexes.get(v))
                .ok_or(LightError::Format)?;
            for (axis, (min, max)) in vecs.iter().zip(mins.iter_mut().zip(&mut maxs)) {
                let value =
                    point[0] * axis[0] + point[1] * axis[1] + point[2] * axis[2] + axis[OFFSET];
                if value < *min {
                    *min = value;
                }
                if value > *max {
                    *max = value;
                }
            }
        }
        self.exact_mins = mins;
        self.exact_maxs = maxs;
        for i in 0..2 {
            let (min, max) = (
                mins.get(i).copied().unwrap_or_default(),
                maxs.get(i).copied().unwrap_or_default(),
            );
            let low = int_of(to_f32(f64::from(min / LUXEL).floor()));
            let high = int_of(to_f32(f64::from(max / LUXEL).ceil()));
            let size = high.saturating_sub(low);
            if size > MAX_EXTENT {
                return Err(LightError::Extents { face: FaceId(face) });
            }
            if let (Some(m), Some(s)) = (self.texmins.get_mut(i), self.texsize.get_mut(i)) {
                *m = low;
                *s = size;
            }
        }
        Ok(())
    }

    /// The lightmap's width and height in luxels.
    fn luxels(&self) -> [usize; 2] {
        self.texsize
            .map(|s| usize::try_from(s.saturating_add(1)).unwrap_or_default())
    }

    /// The point of the face's plane at texture coordinates `us`, `ut`.
    fn at(&self, us: f32, ut: f32) -> Vec3 {
        let [a, b] = self.to_world;
        [0, 1, 2].map(|j| {
            let (o, x, y) = (
                self.texorg.get(j).copied().unwrap_or_default(),
                a.get(j).copied().unwrap_or_default(),
                b.get(j).copied().unwrap_or_default(),
            );
            o + x * us + y * ut
        })
    }

    /// `CalcPoints`: each sample's point, moved toward the face's middle until a line reaches
    /// it from there (LIGHT-3.3, LIGHT-3.4).
    fn points(&mut self, tree: &Tree, sampling: Sampling, colour: Colour) {
        let mids = (self.exact_maxs[0] + self.exact_mins[0]) / 2.0;
        let midt = (self.exact_maxs[1] + self.exact_mins[1]) / 2.0;
        let middle = self.at(mids, midt);
        let [w, h] = self.luxels();
        let [smin, tmin] = self.texmins;
        let (w, h, starts, startt, step) = match sampling {
            Sampling::Fast => (
                w,
                h,
                float_of(smin.saturating_mul(LUXEL_TEXELS)),
                float_of(tmin.saturating_mul(LUXEL_TEXELS)),
                LUXEL,
            ),
            Sampling::Extra => (
                w.saturating_mul(EXTRA),
                h.saturating_mul(EXTRA),
                half_before(smin),
                half_before(tmin),
                HALF_LUXEL,
            ),
        };
        let mut points = Vec::with_capacity(w.saturating_mul(h));
        for t in 0..h {
            for s in 0..w {
                let mut us = starts + float_of(i32::try_from(s).unwrap_or(i32::MAX)) * step;
                let mut ut = startt + float_of(i32::try_from(t).unwrap_or(i32::MAX)) * step;
                let mut point = self.at(us, ut);
                for i in 0..TRIES {
                    point = self.at(us, ut);
                    if tree.cast_ray(middle, point).is_some() {
                        break;
                    }
                    if i % 2 == 1 {
                        us = toward(us, mids);
                    } else {
                        ut = toward(ut, midt);
                    }
                    let move_ = normalized(sub(middle, point));
                    point = ma(point, f64::from(HALF_LUXEL), move_);
                }
                points.push(point);
            }
        }
        let count = points.len();
        self.points = points;
        self.grey = Default::default();
        self.colour = Default::default();
        let colour_kept = colour;
        for (grey, colour) in self.grey.iter_mut().zip(&mut self.colour) {
            *grey = vec![0.0; count];
            if colour_kept == Colour::Coloured {
                *colour = vec![[0.0; AXES]; count];
            }
        }
    }

    /// `SingleLightFace`: `light`'s contribution to each sample of its style's lightmap
    /// (LIGHT-4).
    fn single_light(&mut self, scene: &Scene<'_>, light: &Entity) -> Option<LightWarning> {
        let scale = scene.scale;
        let dist = scale.dist * (dot(light.origin, self.normal) - self.dist);
        if dist <= 0.0 || dist > float_of(light.light) {
            return None;
        }
        let spot = light
            .target_entity
            .and_then(|t| scene.entities.all.get(t.index()))
            .map(|target| {
                let aim = normalized(sub(target.origin, light.origin));
                let cone = if light.angle == 0.0 {
                    DEFAULT_CONE
                } else {
                    f64::from(light.angle)
                };
                let falloff = to_f32(
                    -(f64::from(to_f32(cone / 2.0)) * core::f64::consts::PI / HALF_TURN).cos(),
                );
                (aim, falloff)
            });
        let style = light.style;
        let slot = self.styles.iter().position(|&s| s == style);
        let new = slot.is_none();
        let slot = slot.unwrap_or(self.styles.len());
        if new {
            if slot == BSP_MAXLIGHTMAPS {
                return Some(LightWarning::TooManyStyles);
            }
            let [w, h] = self.luxels();
            let size = w.saturating_mul(h);
            // Only the first `size` samples are cleared, a quarter of the full mode's (LIGHT-4.7).
            if let (Some(grey), Some(colour)) = (self.grey.get_mut(slot), self.colour.get_mut(slot))
            {
                grey.iter_mut().take(size).for_each(|g| *g = 0.0);
                colour.iter_mut().take(size).for_each(|c| *c = [0.0; AXES]);
            }
        }
        let tint = light.colour.unwrap_or([FULL; AXES]);
        let mut hit = false;
        let (Some(grey), Some(colour)) = (self.grey.get_mut(slot), self.colour.get_mut(slot))
        else {
            return None;
        };
        // An all-white map keeps no colour: the slot's colour samples are empty then.
        for (i, (point, sample)) in self.points.iter().zip(grey).enumerate() {
            let Some(reach) = scene.tree.cast_ray(light.origin, *point) else {
                continue;
            };
            let dist = reach * scale.dist;
            if dist < 0.0 {
                continue;
            }
            let incoming = normalized(sub(light.origin, *point));
            if let Some((aim, falloff)) = spot
                && dot(aim, incoming) > falloff
            {
                continue;
            }
            let cos = dot(incoming, self.normal);
            let angle = to_f32((1.0 - f64::from(scale.cos)) + f64::from(scale.cos * cos));
            let add = (float_of(light.light) - dist) * angle;
            if add < 0.0 {
                continue;
            }
            *sample += add;
            if let Some(rgb) = colour.get_mut(i) {
                for (channel, &c) in rgb.iter_mut().zip(&tint) {
                    *channel += c * (add / FULL);
                }
            }
            if *sample > REAL_LIGHT {
                hit = true;
            }
        }
        if new && hit {
            self.styles.push(style);
        }
        None
    }

    /// The lightmaps as bytes, each luxel the mean of its four samples in the full mode
    /// (LIGHT-5, LIGHT-6.3, LIGHT-6.4).
    fn written(&self, sampling: Sampling, scale: Scale, colour_kept: Colour) -> Lit {
        let [w, h] = self.luxels();
        let mut lit = Lit::default();
        for (slot, &style) in self.styles.iter().enumerate() {
            lit.styles.push(u8::try_from(style).unwrap_or(NO_STYLE));
            let (Some(grey), Some(colour)) = (self.grey.get(slot), self.colour.get(slot)) else {
                continue;
            };
            for t in 0..h {
                for s in 0..w {
                    let (total, rgb) = match sampling {
                        Sampling::Fast => {
                            let c = t.saturating_mul(w).saturating_add(s);
                            (
                                grey.get(c).copied().unwrap_or_default(),
                                colour.get(c).copied().unwrap_or_default(),
                            )
                        }
                        Sampling::Extra => {
                            let row = w.saturating_mul(EXTRA);
                            let at = |dt: usize, ds: usize| {
                                t.saturating_mul(EXTRA)
                                    .saturating_add(dt)
                                    .saturating_mul(row)
                                    .saturating_add(s.saturating_mul(EXTRA).saturating_add(ds))
                            };
                            let four = [at(0, 0), at(0, 1), at(1, 0), at(1, 1)];
                            let g = four
                                .iter()
                                .map(|&i| grey.get(i).copied().unwrap_or_default())
                                .fold(0.0_f32, |sum, v| sum + v);
                            let c = four
                                .iter()
                                .map(|&i| colour.get(i).copied().unwrap_or_default())
                                .fold([0.0_f32; AXES], |sum, v| {
                                    [sum[0] + v[0], sum[1] + v[1], sum[2] + v[2]]
                                });
                            (
                                to_f32(f64::from(g) * QUARTER),
                                c.map(|v| to_f32(f64::from(v) * QUARTER)),
                            )
                        }
                    };
                    let total = (total * scale.range).min(BRIGHTEST);
                    lit.grey.push(byte_of(total.max(0.0)));
                    if colour_kept == Colour::Coloured {
                        lit.rgb.push(clamped_colour(rgb, scale.range));
                    }
                }
            }
        }
        lit
    }
}

/// A sample's colour as bytes: clamped at 0, scaled by `rangescale`, then scaled down so its
/// brightest channel is at most 255 (LIGHT-6.3).
fn clamped_colour(rgb: Vec3, range: f32) -> Rgb {
    let rgb = rgb.map(|c| c.max(0.0) * range);
    let brightest = rgb.iter().copied().fold(0.0_f32, f32::max);
    let rgb = if brightest > BRIGHTEST {
        rgb.map(|c| c * (BRIGHTEST / brightest))
    } else {
        rgb
    };
    rgb.map(|c| byte_of(c.min(BRIGHTEST)))
}

/// `(texmin - 0.5) * 16`: the full mode's first sample, half a luxel before the lightmap
/// (LIGHT-3.3).
fn half_before(texmin: i32) -> f32 {
    to_f32((f64::from(texmin) - HALF) * f64::from(LUXEL))
}

/// A texture coordinate moved 8 texels toward `middle`, stopping there (LIGHT-3.4).
fn toward(value: f32, middle: f32) -> f32 {
    if value > middle {
        (value - HALF_LUXEL).max(middle)
    } else {
        (value + HALF_LUXEL).min(middle)
    }
}
