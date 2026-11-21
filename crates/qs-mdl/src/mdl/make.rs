// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qutils/MODELGEN/MODELGEN.C, GrabFrame, WriteModelFile, WriteFrame, WriteGroupBBox
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qutils/COMMON/MATHLIB.C, CrossProduct, VectorNormalize

//! Making a model from where its vertices are (MDL-7): id's modelgen's arithmetic. A
//! [`Shape`] holds each frame's vertices as positions; [`Shape::build`] finds the box they
//! fill, packs each position into the three bytes a frame stores, gives each vertex the entry
//! of the normal table nearest its normal, and fills the header.

use super::write::{MdlWriteError, Model, ModelFrame, ModelFrameEntry, ModelSkin};
use super::{
    MDL_FRAME_GROUP, MDL_FRAME_NAME_SIZE, MDL_VERTEX_NORMALS, MdlHeader, StVert, TriVertX, Triangle,
};

/// What the box the frames fill is divided by for one vertex step, `255.9` in
/// `WriteModelFile`: a little over the 255 steps a byte counts, so the far side packs to 255.
/// A double, as a C constant with no suffix is: the division is in double (MDL-7.2).
const MDL_SCALE_STEPS: f64 = 255.9;

/// The axes a position has.
const AXES: usize = 3;

/// A frame being made: its name and where each vertex is.
#[derive(Clone, Debug, PartialEq)]
pub struct Pose {
    /// Its name, at most 15 bytes with no NUL among them.
    pub name: Vec<u8>,
    /// Where each vertex is, one for each of the model's.
    pub positions: Vec<[f32; 3]>,
}

/// One frame entry being made.
#[derive(Clone, Debug, PartialEq)]
pub enum PoseEntry {
    /// One frame.
    Single(Pose),
    /// A group of frames shown in turn, each for the time it is paired with, in seconds.
    Group(Vec<(Pose, f32)>),
}

impl PoseEntry {
    /// The poses of the entry, in order.
    fn poses(&self) -> Vec<&Pose> {
        match self {
            Self::Single(pose) => vec![pose],
            Self::Group(poses) => poses.iter().map(|(pose, _)| pose).collect(),
        }
    }
}

/// A model being made: its skins, how its vertices lie on them, its triangles, and each
/// frame as positions.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// The skins' width in pixels, a multiple of 4.
    pub skin_width: i32,
    /// The skins' height in pixels, at most 480.
    pub skin_height: i32,
    /// The skin entries.
    pub skins: Vec<ModelSkin>,
    /// One texture coordinate per vertex.
    pub stverts: Vec<StVert>,
    /// The triangles.
    pub triangles: Vec<Triangle>,
    /// The frame entries.
    pub poses: Vec<PoseEntry>,
    /// The header's `eyeposition`.
    pub eye_position: [f32; 3],
    /// The header's `flags`.
    pub flags: i32,
    /// The header's `synctype`.
    pub synctype: i32,
}

/// `CrossProduct (a - b, c - b)`, in single precision.
pub(super) fn corner_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let ([a0, a1, a2], [b0, b1, b2], [c0, c1, c2]) = (a, b, c);
    let [ax, ay, az] = [a0 - b0, a1 - b1, a2 - b2];
    let [cx, cy, cz] = [c0 - b0, c1 - b1, c2 - b2];
    [ay * cz - az * cy, az * cx - ax * cz, ax * cy - ay * cx]
}

/// `VectorNormalize`: the length summed and rooted in double, each part divided by it and
/// stored in single; a vector of no length is left as it is. Answers the length.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec MDL-7.4: id divides a float by the double length and stores a float"
)]
pub(super) fn normalize(v: &mut [f32; 3]) -> f64 {
    let mut length = 0.0_f64;
    for part in v.iter() {
        length += f64::from(part * part);
    }
    let length = length.sqrt();
    // MDL-7.4: `if (length == 0) return 0;`, an exact comparison.
    if length == 0.0 {
        return 0.0;
    }
    for part in v.iter_mut() {
        *part = (f64::from(*part) / length) as f32;
    }
    length
}

/// The entry of the normal table whose dot product with `normal` is greatest, the first of
/// equals: `GrabFrame`'s `maxdot` loop.
pub(super) fn nearest_normal(normal: [f32; 3]) -> u8 {
    let mut best = 0_u8;
    let mut most = f32::NEG_INFINITY;
    for (index, entry) in (0_u8..).zip(MDL_VERTEX_NORMALS.iter()) {
        let ([nx, ny, nz], [ex, ey, ez]) = (normal, *entry);
        let dot = nx * ex + ny * ey + nz * ez;
        if dot > most {
            most = dot;
            best = index;
        }
    }
    best
}

/// A double as C stores it in a float: the nearest single.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec MDL-7.2: C's assignment of a double to a float, the nearest single"
)]
pub(super) const fn single(value: f64) -> f32 {
    value as f32
}

/// A float as C stores it in a byte when it is from 0 to under 256: its whole part. Outside
/// that C's conversion is undefined, and here it is the nearest end.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "spec MDL-7.3: `tarray[j].v[k] = v;` truncates a float to a byte; the value is \
              clamped to a byte's range first"
)]
fn packed(value: f32) -> u8 {
    if value.is_nan() {
        return 0;
    }
    value.clamp(0.0, f32::from(u8::MAX)) as u8
}

impl Shape {
    /// The normal table's entry for each vertex in `positions`: the average of the unit
    /// normals of the triangles it is a corner of, made a unit vector (MDL-7.4).
    ///
    /// # Errors
    ///
    /// [`MdlWriteError::LooseVertex`] for a vertex no triangle with an area has for a corner,
    /// which modelgen stops at.
    fn normals(&self, positions: &[[f32; 3]]) -> Result<Vec<u8>, MdlWriteError> {
        let mut sums = vec![([0.0_f32; AXES], 0_u16); positions.len()];
        for triangle in &self.triangles {
            let [a, b, c] = triangle.vertindex;
            let (Some(&pa), Some(&pb), Some(&pc)) =
                (positions.get(a), positions.get(b), positions.get(c))
            else {
                continue;
            };
            let mut normal = corner_normal(pa, pb, pc);
            normalize(&mut normal);
            for vertex in triangle.vertindex {
                if let Some((sum, count)) = sums.get_mut(vertex) {
                    for (total, part) in sum.iter_mut().zip(normal) {
                        *total += part;
                    }
                    *count = count.saturating_add(1);
                }
            }
        }
        let mut out = Vec::with_capacity(sums.len());
        for (vertex, (sum, count)) in sums.into_iter().enumerate() {
            if count == 0 {
                return Err(MdlWriteError::LooseVertex { vertex });
            }
            let mut normal = sum.map(|total| total / f32::from(count));
            normalize(&mut normal);
            out.push(nearest_normal(normal));
        }
        Ok(out)
    }

    /// The model: id's `WriteModelFile` over frames made as `GrabFrame` makes them.
    ///
    /// Implements MDL-7.
    ///
    /// # Errors
    ///
    /// [`MdlWriteError::FrameVertices`] for a pose without one position for each texture
    /// coordinate, [`MdlWriteError::FrameName`], [`MdlWriteError::LooseVertex`], and what
    /// [`Model::to_bytes`] refuses.
    pub fn build(&self) -> Result<Model, MdlWriteError> {
        let numverts = self.stverts.len();
        let grabbed = |pose: &Pose, frame: usize| -> Result<Grabbed, MdlWriteError> {
            if pose.positions.len() != numverts {
                return Err(MdlWriteError::FrameVertices { frame });
            }
            Ok(Grabbed {
                name: pose.name.clone(),
                positions: pose.positions.clone(),
                normals: self.normals(&pose.positions)?,
            })
        };
        let mut entries = Vec::with_capacity(self.poses.len());
        for (place, entry) in self.poses.iter().enumerate() {
            entries.push(match entry {
                PoseEntry::Single(pose) => GrabbedEntry::Single(grabbed(pose, place)?),
                PoseEntry::Group(poses) => GrabbedEntry::Group(
                    poses
                        .iter()
                        .map(|(pose, interval)| Ok((grabbed(pose, place)?, *interval)))
                        .collect::<Result<_, MdlWriteError>>()?,
                ),
            });
        }
        Parts {
            skin_width: self.skin_width,
            skin_height: self.skin_height,
            skins: self.skins.clone(),
            stverts: self.stverts.clone(),
            triangles: self.triangles.clone(),
            entries,
            eye_position: self.eye_position,
            flags: self.flags,
            synctype: self.synctype,
            size: self.average_size(),
        }
        .model()
    }

    /// The header's `size`: the area of the first frame's triangles, summed and divided by
    /// how many there are (MDL-7.5).
    fn average_size(&self) -> f32 {
        let Some(first) = self
            .poses
            .first()
            .and_then(|entry| entry.poses().into_iter().next())
        else {
            return 0.0;
        };
        let mut total = 0.0_f32;
        for triangle in &self.triangles {
            let [a, b, c] = triangle.vertindex;
            let (Some(&pa), Some(&pb), Some(&pc)) = (
                first.positions.get(a),
                first.positions.get(b),
                first.positions.get(c),
            ) else {
                continue;
            };
            let normal = corner_normal(pa, pb, pc);
            let squared = normal.iter().map(|part| part * part).sum::<f32>();
            total += squared.sqrt() / 2.0;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "spec MDL-7.5: `totsize / model.numtris`, an int made a float"
        )]
        let count = self.triangles.len() as f32;
        if count > 0.0 { total / count } else { 0.0 }
    }
}

#[cfg(test)]
mod tests;

/// A frame as `GrabFrame` leaves it: its name, where each vertex is, and each vertex's entry
/// of the normal table.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Grabbed {
    pub(super) name: Vec<u8>,
    pub(super) positions: Vec<[f32; 3]>,
    pub(super) normals: Vec<u8>,
}

/// One frame entry as grabbed: a frame, or a group's frames each with its interval.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum GrabbedEntry {
    Single(Grabbed),
    Group(Vec<(Grabbed, f32)>),
}

impl GrabbedEntry {
    /// The frames of the entry, in order.
    fn frames(&self) -> Vec<&Grabbed> {
        match self {
            Self::Single(frame) => vec![frame],
            Self::Group(frames) => frames.iter().map(|(frame, _)| frame).collect(),
        }
    }
}

/// What `WriteModelFile` writes a model from: the base, the skins, the grabbed frames and
/// the header's given values.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Parts {
    pub(super) skin_width: i32,
    pub(super) skin_height: i32,
    pub(super) skins: Vec<ModelSkin>,
    pub(super) stverts: Vec<StVert>,
    pub(super) triangles: Vec<Triangle>,
    pub(super) entries: Vec<GrabbedEntry>,
    pub(super) eye_position: [f32; 3],
    pub(super) flags: i32,
    pub(super) synctype: i32,
    /// The header's `size` (MDL-7.5).
    pub(super) size: f32,
}

impl Parts {
    /// The model: `WriteModelFile`, `WriteFrame` and `WriteGroupBBox` (MDL-7.1 to MDL-7.3,
    /// MDL-7.6).
    ///
    /// # Errors
    ///
    /// [`MdlWriteError::FrameVertices`] for a frame without a position and a normal for each
    /// texture coordinate, [`MdlWriteError::FrameName`], and what [`Model::to_bytes`]
    /// refuses.
    pub(super) fn model(self) -> Result<Model, MdlWriteError> {
        let numverts = self.stverts.len();
        // The box the frames fill starts as the origin alone (MDL-7.1).
        let mut mins = [0.0_f32; AXES];
        let mut maxs = [0.0_f32; AXES];
        for (place, entry) in self.entries.iter().enumerate() {
            for pose in entry.frames() {
                if pose.positions.len() != numverts || pose.normals.len() != numverts {
                    return Err(MdlWriteError::FrameVertices { frame: place });
                }
                for position in &pose.positions {
                    for ((low, high), at) in mins.iter_mut().zip(&mut maxs).zip(position) {
                        *low = low.min(*at);
                        *high = high.max(*at);
                    }
                }
            }
        }
        let mut scale = [0.0_f32; AXES];
        let mut far = [0.0_f32; AXES];
        for (((step, reach), low), high) in scale.iter_mut().zip(&mut far).zip(mins).zip(maxs) {
            *step = single(f64::from(high - low) / MDL_SCALE_STEPS);
            *reach = if low.abs() > high.abs() { low } else { high };
        }
        let radius = far.iter().map(|reach| reach * reach).sum::<f32>().sqrt();
        let header = MdlHeader {
            scale,
            scale_origin: mins,
            bounding_radius: radius,
            eye_position: self.eye_position,
            numskins: 0,
            skinwidth: self.skin_width,
            skinheight: self.skin_height,
            numverts: 0,
            numtris: 0,
            numframes: 0,
            synctype: self.synctype,
            flags: self.flags,
            size: self.size,
        };
        let frame = |pose: &Grabbed| -> Result<ModelFrame, MdlWriteError> {
            let mut bboxmin = [u8::MAX; AXES];
            let mut bboxmax = [0_u8; AXES];
            let mut verts = Vec::with_capacity(numverts);
            for (position, &normal) in pose.positions.iter().zip(&pose.normals) {
                let mut v = [0_u8; AXES];
                for ((((byte, at), origin), step), (low, high)) in v
                    .iter_mut()
                    .zip(position)
                    .zip(mins)
                    .zip(scale)
                    .zip(bboxmin.iter_mut().zip(&mut bboxmax))
                {
                    *byte = packed((at - origin) / step);
                    *low = (*low).min(*byte);
                    *high = (*high).max(*byte);
                }
                verts.push(TriVertX {
                    v,
                    lightnormalindex: normal,
                });
            }
            let mut made = ModelFrame {
                bboxmin: TriVertX {
                    v: bboxmin,
                    lightnormalindex: 0,
                },
                bboxmax: TriVertX {
                    v: bboxmax,
                    lightnormalindex: 0,
                },
                name: [0; MDL_FRAME_NAME_SIZE],
                verts,
            };
            made.set_name(&pose.name)?;
            Ok(made)
        };
        let mut frames = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            frames.push(match entry {
                GrabbedEntry::Single(pose) => ModelFrameEntry::Single(frame(pose)?),
                GrabbedEntry::Group(poses) => {
                    let mut made = Vec::with_capacity(poses.len());
                    let mut intervals = Vec::with_capacity(poses.len());
                    let mut bboxmin = [u8::MAX; AXES];
                    let mut bboxmax = [0_u8; AXES];
                    // The time each frame ends at: the intervals summed (MDL-7.6).
                    let mut total = 0.0_f32;
                    for (pose, interval) in poses {
                        let one = frame(pose)?;
                        for ((low, high), (min, max)) in bboxmin
                            .iter_mut()
                            .zip(&mut bboxmax)
                            .zip(one.bboxmin.v.into_iter().zip(one.bboxmax.v))
                        {
                            *low = (*low).min(min);
                            *high = (*high).max(max);
                        }
                        total += interval;
                        intervals.push(total);
                        made.push(one);
                    }
                    ModelFrameEntry::Group {
                        frame_type: MDL_FRAME_GROUP,
                        bboxmin: TriVertX {
                            v: bboxmin,
                            lightnormalindex: 0,
                        },
                        bboxmax: TriVertX {
                            v: bboxmax,
                            lightnormalindex: 0,
                        },
                        intervals,
                        frames: made,
                    }
                }
            });
        }
        let model = Model {
            header,
            skins: self.skins,
            stverts: self.stverts,
            triangles: self.triangles,
            frames,
            trailing: Vec::new(),
        };
        // What is made must be a model that is written and loads.
        model.to_bytes()?;
        Ok(model)
    }
}
