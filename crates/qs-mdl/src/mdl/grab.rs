// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qutils/MODELGEN/MODELGEN.C, Cmd_Base, SetSkinValues, Cmd_Skin, GrabFrame, Cmd_Origin, Cmd_Eyeposition
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qutils/COMMON/MATHLIB.C, VectorCompare, Q_rint, CrossProduct

//! Making a model from triangles alone, as id's modelgen does from its script (MDL-8): a
//! base, a list of triangles each three corners, gives the model its vertices, its triangles
//! and where each vertex lies on the skin, the model's front on the skin's left half and its
//! back on the right; each frame is the same triangles with their corners moved.
//!
//! [`Base`] is what `$base` derives, which says how large a skin must be; [`Grab`] is a whole
//! script with its files read, and [`Grab::build`] the model modelgen writes from it.

use core::fmt;

use super::make::{Grabbed, GrabbedEntry, Parts, nearest_normal, normalize, single};
use super::write::{MdlWriteError, Model, ModelSkin};
use super::{MDL_ALIAS_ONSEAM, MDL_MAX_VERTS, StVert, Triangle};

/// A triangle as modelgen reads one from a `.tri` file: its three corners.
pub type Corners = [[f32; 3]; 3];

/// How far apart two positions may be on each axis and be one vertex, `EQUAL_EPSILON` in
/// `MATHLIB.H` (MDL-8.1).
pub const MDL_GRAB_EQUAL_EPSILON: f64 = 0.001;
/// How many skin pixels a unit of the model is given before the skin is fitted, `scale = 8`
/// in `SetSkinValues` (MDL-8.3).
pub const MDL_GRAB_SKIN_SCALE: f32 = 8.0;
/// The most pixels the model's width is laid on, `150` in `SetSkinValues` (MDL-8.3).
pub const MDL_GRAB_SKIN_WIDTH: i32 = 150;
/// The most pixels the model's height is laid on, `190` in `SetSkinValues` (MDL-8.3).
pub const MDL_GRAB_SKIN_HEIGHT: i32 = 190;
/// The pixels left free around each half of the skin, the `+ 4` and the `2` of
/// `SetSkinValues` (MDL-8.3, MDL-8.4).
pub const MDL_GRAB_SKIN_MARGIN: i32 = 2;
/// The most triangles a vertex may be a corner of, the `normals[20][3]` of modelgen's
/// `vertexnormals` (MDL-8.7).
pub const MDL_GRAB_MAX_NORMALS: usize = 20;
/// The time a frame of a group is shown for when the script gives none, `0.1` in `GrabFrame`
/// (MDL-8.8).
pub const MDL_GRAB_INTERVAL: f32 = 0.1;
/// What the box of the base starts as, `9999999` in `SetSkinValues` (MDL-8.3).
const FAR: f32 = 9_999_999.0;
/// The seam flags `SetSkinValues` gives a vertex: a corner of a triangle on the front, of one
/// on the back, and of both, which is a vertex on the seam (MDL-8.4).
const ON_FRONT: u8 = 1;
const ON_BACK: u8 = 2;
const ON_BOTH: u8 = ON_FRONT | ON_BACK;
/// The axes of a position.
const AXES: usize = 3;
/// What a skin's width is a multiple of, "some hardware requires this, and it ensures dword
/// alignment for each scan" (MDL-8.3), and one less, what is added before the low bits are
/// dropped.
const WIDTH_STEP: i32 = 4;
const WIDTH_ROUND: i32 = WIDTH_STEP - 1;
/// The half `Q_rint` adds before it takes the floor.
const HALF: f64 = 0.5;

/// Why a model could not be made from triangles.
#[derive(Debug, PartialEq)]
pub enum GrabError {
    /// The base has no triangle.
    NoTriangles,
    /// A frame does not have a triangle for each of the base's ("number of triangles doesn't
    /// match").
    Triangles {
        /// The frame's name.
        frame: String,
    },
    /// A skin's picture is smaller than the skin the base needs.
    Skin {
        /// The skin entry.
        skin: usize,
        /// The width and height the base needs.
        needs: [i32; 2],
    },
    /// A vertex is a corner of more triangles than modelgen keeps normals for
    /// ([`MDL_GRAB_MAX_NORMALS`]).
    Corners {
        /// The vertex, in the base's order.
        vertex: usize,
    },
    /// The base has more vertices than a model has (MDL-1.4).
    Vertices,
    /// A group has no frame ("Empty group").
    EmptyGroup,
    /// A frame of a group is shown for no time or less ("Non-positive interval").
    Interval {
        /// The frame's name.
        frame: String,
    },
    /// The model cannot be written.
    Write(MdlWriteError),
}

impl fmt::Display for GrabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTriangles => f.write_str("the base has no triangle"),
            Self::Triangles { frame } => write!(
                f,
                "the frame {frame} does not have a triangle for each of the base's"
            ),
            Self::Skin {
                skin,
                needs: [width, height],
            } => write!(
                f,
                "skin {skin} is smaller than the {width} by {height} the model's skin is"
            ),
            Self::Corners { vertex } => write!(
                f,
                "vertex {vertex} is a corner of more than {MDL_GRAB_MAX_NORMALS} triangles"
            ),
            Self::Vertices => write!(
                f,
                "the base has more than the {MDL_MAX_VERTS} vertices a model has"
            ),
            Self::EmptyGroup => f.write_str("a group of frames has no frame"),
            Self::Interval { frame } => write!(
                f,
                "the frame {frame} of a group is shown for no time: its interval is above 0"
            ),
            Self::Write(error) => write!(f, "{error}"),
        }
    }
}

impl core::error::Error for GrabError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Write(error) => Some(error),
            Self::NoTriangles
            | Self::Triangles { .. }
            | Self::Skin { .. }
            | Self::Corners { .. }
            | Self::Vertices
            | Self::EmptyGroup
            | Self::Interval { .. } => None,
        }
    }
}

/// C's conversion of a double to an `int`: its whole part. Where C's is undefined, a value no
/// `int` holds, this is the nearest `int`, and 0 for what is no number.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec MDL-8.3: C's conversion of a floating value to an int, its whole part"
)]
const fn whole(value: f64) -> i32 {
    value as i32
}

/// C's conversion of an `int` to a `float`: the nearest single.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec MDL-8.3: C's conversion of an int to a float, the nearest single"
)]
const fn float(value: i32) -> f32 {
    value as f32
}

/// `VectorCompare`: whether two positions are within [`MDL_GRAB_EQUAL_EPSILON`] of each
/// other on every axis.
fn same(a: [f32; 3], b: [f32; 3]) -> bool {
    a.into_iter()
        .zip(b)
        .all(|(a, b)| f64::from(a - b).abs() <= MDL_GRAB_EQUAL_EPSILON)
}

/// The two edges `GrabFrame` and `SetSkinValues` take a triangle's normal from, `v0 - v1`
/// and `v2 - v1`.
fn edges([a, b, c]: Corners) -> [[f32; 3]; 2] {
    let ([a0, a1, a2], [b0, b1, b2], [c0, c1, c2]) = (a, b, c);
    [[a0 - b0, a1 - b1, a2 - b2], [c0 - b0, c1 - b1, c2 - b2]]
}

/// `CrossProduct`, in single precision.
fn cross([[ax, ay, az], [cx, cy, cz]]: [[f32; 3]; 2]) -> [f32; 3] {
    [ay * cz - az * cy, az * cx - ax * cz, ax * cy - ay * cx]
}

/// `Q_rint` of a float, stored in an `int`: "`return floor (in + 0.5);`", the sum in double,
/// answered as a float.
fn rint(value: f32) -> i32 {
    whole(f64::from(single((f64::from(value) + HALF).floor())))
}

/// What modelgen's `$base` derives from a model's triangles (MDL-8.1 to MDL-8.4): its
/// vertices, which of them each triangle has for corners, which triangles have no area, and
/// where each vertex lies on a skin of the size it works out.
#[derive(Clone, Debug, PartialEq)]
pub struct Base {
    /// `baseverts`: each vertex's position in the base.
    verts: Vec<[f32; 3]>,
    /// Each triangle's corners, as vertices.
    corners: Vec<[usize; 3]>,
    /// Each triangle: whether two of its corners are one vertex's place.
    degenerate: Vec<bool>,
    /// Each triangle: whether it is laid on the front half of the skin.
    front: Vec<bool>,
    /// Where each vertex lies on the skin.
    stverts: Vec<StVert>,
    /// The skin's width and height.
    skin: [i32; 2],
}

impl Base {
    /// The base of `triangles`: `Cmd_Base` and `SetSkinValues`.
    ///
    /// Implements MDL-8.1 to MDL-8.4.
    ///
    /// # Errors
    ///
    /// [`GrabError::NoTriangles`] for no triangles, whose box modelgen leaves as it began,
    /// and [`GrabError::Vertices`] for more vertices than a model has.
    pub fn of(triangles: &[Corners]) -> Result<Self, GrabError> {
        if triangles.is_empty() {
            return Err(GrabError::NoTriangles);
        }
        let most = usize::try_from(MDL_MAX_VERTS).unwrap_or(0);
        let mut verts: Vec<[f32; 3]> = Vec::new();
        let mut corners = Vec::with_capacity(triangles.len());
        let mut degenerate = Vec::with_capacity(triangles.len());
        for triangle in triangles {
            let [a, b, c] = *triangle;
            degenerate.push(same(a, b) || same(b, c) || same(c, a));
            // A corner is the first vertex within the epsilon of it, else a new one.
            corners.push(triangle.map(|corner| {
                verts
                    .iter()
                    .position(|vert| same(corner, *vert))
                    .unwrap_or_else(|| {
                        verts.push(corner);
                        verts.len().saturating_sub(1)
                    })
            }));
            // Ours: modelgen writes past its `MAXVERTS`; more than a model has is refused
            // as soon as it is known.
            if verts.len() > most {
                return Err(GrabError::Vertices);
            }
        }
        // The box of the base, widened to whole units.
        let mut mins = [FAR; AXES];
        let mut maxs = [-FAR; AXES];
        for vert in &verts {
            for ((low, high), at) in mins.iter_mut().zip(&mut maxs).zip(vert) {
                *low = low.min(*at);
                *high = high.max(*at);
            }
        }
        let mins = mins.map(f32::floor);
        let maxs = maxs.map(f32::ceil);
        let [min_x, _, _] = mins;
        let [max_x, _, max_z] = maxs;
        let [_, _, min_z] = mins;
        let width = whole(f64::from(max_x - min_x));
        let height = whole(f64::from(max_z - min_z));
        let mut scale = MDL_GRAB_SKIN_SCALE;
        if float(width) * scale >= float(MDL_GRAB_SKIN_WIDTH) {
            scale = single(f64::from(MDL_GRAB_SKIN_WIDTH) / f64::from(width));
        }
        if float(height) * scale >= float(MDL_GRAB_SKIN_HEIGHT) {
            scale = single(f64::from(MDL_GRAB_SKIN_HEIGHT) / f64::from(height));
        }
        let margin = MDL_GRAB_SKIN_MARGIN.saturating_mul(2);
        let side = |size: i32| whole(f64::from(float(size) * scale).ceil()).saturating_add(margin);
        let (iwidth, iheight) = (side(width), side(height));

        let mut seams = vec![0_u8; verts.len()];
        let mut stverts = vec![
            StVert {
                onseam: 0,
                s: 0,
                t: 0
            };
            verts.len()
        ];
        let mut front = Vec::with_capacity(triangles.len());
        for (triangle, corners) in triangles.iter().zip(&corners) {
            // The triangle as its vertices are in the base, which a corner within the
            // epsilon of an earlier one is not quite.
            let at = corners.map(|vertex| verts.get(vertex).copied().unwrap_or(triangle[0]));
            let [_, normal_y, _] = cross(edges(at));
            // "if (normal[1] > 0)" it is on the back.
            let back = normal_y > 0.0;
            let faces_front = !back;
            let base_x = if faces_front {
                float(MDL_GRAB_SKIN_MARGIN)
            } else {
                float(iwidth.saturating_add(MDL_GRAB_SKIN_MARGIN))
            };
            let base_y = float(MDL_GRAB_SKIN_MARGIN);
            front.push(faces_front);
            for (vertex, [x, _, z]) in corners.iter().zip(at) {
                let (Some(seam), Some(stvert)) = (seams.get_mut(*vertex), stverts.get_mut(*vertex))
                else {
                    continue;
                };
                *seam |= if faces_front { ON_FRONT } else { ON_BACK };
                // "we want the front s value for seam vertices"
                if faces_front || *seam & ON_FRONT == 0 {
                    stvert.s = rint((x - min_x) * scale + base_x);
                    stvert.t = rint((max_z - z) * scale + base_y);
                }
            }
        }
        for (stvert, seam) in stverts.iter_mut().zip(seams) {
            stvert.onseam = if seam == ON_BOTH { MDL_ALIAS_ONSEAM } else { 0 };
        }
        // "make the width a multiple of 4"
        let skin_width = iwidth.saturating_mul(2).saturating_add(WIDTH_ROUND) & !WIDTH_ROUND;
        Ok(Self {
            verts,
            corners,
            degenerate,
            front,
            stverts,
            skin: [skin_width, iheight],
        })
    }

    /// The width and height of the model's skins.
    #[must_use]
    pub const fn skin_size(&self) -> [i32; 2] {
        self.skin
    }

    /// How many vertices the model has.
    #[must_use]
    pub const fn vertices(&self) -> usize {
        self.verts.len()
    }

    /// Each of the base's triangles as vertices of the model, in the base's order, those of
    /// no area among them.
    #[must_use]
    pub fn corners(&self) -> &[[usize; 3]] {
        &self.corners
    }

    /// Where each vertex lies on the skin, in the model's vertex order.
    #[must_use]
    pub fn stverts(&self) -> &[StVert] {
        &self.stverts
    }

    /// The triangles the model keeps, those with an area: modelgen writes `facesfront` as 1
    /// or 0.
    #[must_use]
    pub fn triangles(&self) -> Vec<Triangle> {
        self.corners
            .iter()
            .zip(&self.front)
            .zip(&self.degenerate)
            .filter(|(_, degenerate)| !**degenerate)
            .map(|((vertindex, front), _)| Triangle {
                facesfront: i32::from(*front),
                vertindex: *vertindex,
            })
            .collect()
    }
}

/// A picture a skin is cut from: modelgen's is a 320 by 200 `.lbm`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// How many pixels a row has.
    pub width: usize,
    /// Its pixels, row by row from the top, each a colour of the palette.
    pub pixels: Vec<u8>,
}

/// A frame of a script: its name and its triangles, the base's with their corners moved.
#[derive(Clone, Debug, PartialEq)]
pub struct GrabFrame {
    /// Its name, at most 15 bytes with no NUL among them.
    pub name: Vec<u8>,
    /// Its triangles, one for each of the base's, in the base's order.
    pub triangles: Vec<Corners>,
}

/// One frame entry of a script.
#[derive(Clone, Debug, PartialEq)]
pub enum GrabEntry {
    /// `$frame`: one frame.
    Single(GrabFrame),
    /// `$framegroupstart` to `$framegroupend`: frames shown in turn, each for the seconds it
    /// is paired with ([`MDL_GRAB_INTERVAL`] where the script gives none).
    Group(Vec<(GrabFrame, f32)>),
}

/// A modelgen script with its files read (MDL-8): what `$base`, `$skin`, `$frame`,
/// `$origin`, `$scale`, `$eyeposition`, `$flags` and `$sync` give for one model. A script
/// gives `$origin` and `$scale` before its frames, and they are held once here.
#[derive(Clone, Debug, PartialEq)]
pub struct Grab {
    /// `$base`: the model's triangles.
    pub base: Vec<Corners>,
    /// `$skin`: a picture for each skin, of which the top left is the skin.
    pub skins: Vec<Picture>,
    /// The frame entries.
    pub frames: Vec<GrabEntry>,
    /// `$origin`, as the script writes it.
    pub origin: [f32; 3],
    /// `$scale`: what every position is multiplied by; 1 when the script has none.
    pub scale: f32,
    /// `$eyeposition`, as the script writes it.
    pub eye_position: [f32; 3],
    /// `$flags`.
    pub flags: i32,
    /// The header's `synctype`: [`super::MDL_ST_SYNC`] for a script with `$sync`, else
    /// [`super::MDL_ST_RAND`].
    pub synctype: i32,
}

impl Grab {
    /// One frame as `GrabFrame` makes it of `frame`'s triangles: each vertex where its
    /// triangles' corners put it, turned to face down +x, scaled and moved by the origin, and
    /// its entry of the normal table. `area` is given for the script's first frame, and
    /// gains each triangle's area.
    fn grab(
        &self,
        base: &Base,
        frame: &GrabFrame,
        mut area: Option<&mut f32>,
    ) -> Result<Grabbed, GrabError> {
        if frame.triangles.len() != base.corners.len() {
            return Err(GrabError::Triangles {
                frame: String::from_utf8_lossy(&frame.name).into_owned(),
            });
        }
        let [origin_x, origin_y, origin_z] = self.origin;
        // "rotate points into frame of reference so model points down the positive x axis"
        let adjust = [origin_y, -origin_x, -origin_z];
        let mut positions = vec![[0.0_f32; AXES]; base.verts.len()];
        let mut normals: Vec<Vec<[f32; 3]>> = vec![Vec::new(); base.verts.len()];
        for ((triangle, corners), _) in frame
            .triangles
            .iter()
            .zip(&base.corners)
            .zip(&base.degenerate)
            .filter(|(_, degenerate)| !**degenerate)
        {
            let both = edges(*triangle);
            if let Some(area) = area.as_deref_mut() {
                let [nx, ny, nz] = cross(both.map(|edge| edge.map(|part| part * self.scale)));
                let length = f64::from(nx * nx + ny * ny + nz * nz).sqrt();
                *area = single(f64::from(*area) + length / 2.0);
            }
            let mut normal = cross(both);
            normalize(&mut normal);
            let [nx, ny, nz] = normal;
            let normal = [-ny, nx, nz];
            for (vertex, [x, y, z]) in corners.iter().zip(*triangle) {
                let [adjust_x, adjust_y, adjust_z] = adjust;
                if let Some(position) = positions.get_mut(*vertex) {
                    *position = [
                        (-y) * self.scale + adjust_x,
                        x * self.scale + adjust_y,
                        z * self.scale + adjust_z,
                    ];
                }
                if let Some(kept) = normals.get_mut(*vertex) {
                    if kept.len() >= MDL_GRAB_MAX_NORMALS {
                        return Err(GrabError::Corners { vertex: *vertex });
                    }
                    kept.push(normal);
                }
            }
        }
        let mut indices = Vec::with_capacity(normals.len());
        for (vertex, kept) in normals.iter().enumerate() {
            let Some(count) = u8::try_from(kept.len()).ok().filter(|count| *count > 0) else {
                return Err(GrabError::Write(MdlWriteError::LooseVertex { vertex }));
            };
            let mut normal = [0.0_f32; AXES];
            for one in kept {
                for (sum, part) in normal.iter_mut().zip(one) {
                    *sum += part;
                }
            }
            let mut normal = normal.map(|sum| sum / f32::from(count));
            normalize(&mut normal);
            indices.push(nearest_normal(normal));
        }
        Ok(Grabbed {
            name: frame.name.clone(),
            positions,
            normals: indices,
        })
    }

    /// The model modelgen writes from the script.
    ///
    /// Implements MDL-8.
    ///
    /// # Errors
    ///
    /// [`GrabError`]: a base with no triangle, a frame with other triangles than the base's,
    /// a picture smaller than the skin, a vertex with more triangles than modelgen keeps or
    /// with none that has an area, and a model that cannot be written.
    pub fn build(&self) -> Result<Model, GrabError> {
        let base = Base::of(&self.base)?;
        let [skin_width, skin_height] = base.skin_size();
        let (columns, rows) = (
            usize::try_from(skin_width).unwrap_or(0),
            usize::try_from(skin_height).unwrap_or(0),
        );
        let mut skins = Vec::with_capacity(self.skins.len());
        for (skin, picture) in self.skins.iter().enumerate() {
            // "now copy the part of the texture we care about": the top left of the picture.
            let cut: Option<Vec<&[u8]>> = (0..rows)
                .map(|row| {
                    let from = row.checked_mul(picture.width)?;
                    picture.pixels.get(from..from.checked_add(columns)?)
                })
                .collect();
            let cut = cut
                .filter(|_| picture.width >= columns)
                .ok_or(GrabError::Skin {
                    skin,
                    needs: [skin_width, skin_height],
                })?;
            skins.push(ModelSkin::Single(cut.concat()));
        }
        let mut area = 0.0_f32;
        let mut first = true;
        let mut entries = Vec::with_capacity(self.frames.len());
        for entry in &self.frames {
            let mut grab = |frame: &GrabFrame| {
                let grabbed = self.grab(&base, frame, first.then_some(&mut area));
                first = false;
                grabbed
            };
            entries.push(match entry {
                GrabEntry::Single(frame) => GrabbedEntry::Single(grab(frame)?),
                GrabEntry::Group(frames) => {
                    if frames.is_empty() {
                        return Err(GrabError::EmptyGroup);
                    }
                    GrabbedEntry::Group(
                        frames
                            .iter()
                            .map(|(frame, interval)| {
                                // "if (frames[framecount].interval <= 0.0) Error"; what is
                                // no number is refused with it.
                                if interval.is_nan() || *interval <= 0.0 {
                                    return Err(GrabError::Interval {
                                        frame: String::from_utf8_lossy(&frame.name).into_owned(),
                                    });
                                }
                                Ok((grab(frame)?, *interval))
                            })
                            .collect::<Result<_, GrabError>>()?,
                    )
                }
            });
        }
        let [origin_x, origin_y, origin_z] = self.origin;
        let [eye_x, eye_y, eye_z] = self.eye_position;
        // `Cmd_Eyeposition` turns it as the points are turned, and `WriteModelFile` adds the
        // origin's adjustment.
        let eye_position = [-eye_y + origin_y, eye_x - origin_x, eye_z - origin_z];
        let count = i32::try_from(self.base.len()).unwrap_or(i32::MAX);
        Parts {
            skin_width,
            skin_height,
            skins,
            stverts: base.stverts.clone(),
            triangles: base.triangles(),
            entries,
            eye_position,
            flags: self.flags,
            synctype: self.synctype,
            // "averagesize = totsize / model.numtris", which counts the triangles of no area.
            size: area / float(count),
        }
        .model()
        .map_err(GrabError::Write)
    }
}

#[cfg(test)]
mod tests;
