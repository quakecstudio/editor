// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2010-2016 Lee Salzman
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: iqm/iqm.cpp, loadsmd, skipcomment, skipsection, readname, readnodes, readmaterial, readtriangles, readframes

//! Valve's `.smd` files as a mesh of poses (MDL-10.3), read as FTEQW's model converter reads
//! them: a file with triangles is the model in its base pose, the first frame of its
//! skeleton, and a file without is an animation of the same joints.

use core::fmt;

use super::Mesh;
use super::obj::Up;
use super::rig::{Affine, Boned, RigError, Vec3, blend, from_angles, scaled, turn};

/// The name of the one pose of a model with no animation.
pub const SMD_FRAME: &str = "base";
/// What an animation with no name is called.
pub const SMD_ANIMATION: &str = "anim";
/// The only version read: "`if(version != 1)`".
pub const SMD_VERSION: i64 = 1;
/// The greatest number a joint may have: "`id > 255`" is left out.
pub const SMD_MAX_JOINT: usize = 255;
/// The numbers a vertex's line has before its links: its joint, its position, its normal
/// and its texture coordinate.
const VERTEX_NUMBERS: usize = 9;
/// The numbers of a joint's line of a frame: the joint's own, a position and three angles.
const POSE_NUMBERS: usize = 7;
/// The corners of a triangle.
const CORNERS: usize = 3;
/// A scale of one on each axis: an SMD pose has none.
const NO_SCALE: Vec3 = [1.0, 1.0, 1.0];

/// Why an `.smd` file was not read, or is no mesh of poses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SmdError {
    /// The file's version is not [`SMD_VERSION`].
    Version,
    /// The file has no triangles: it is an animation, not a model.
    NoTriangles,
    /// A joint's base pose has no size on an axis.
    Flat,
    /// More animations have one name than can be told apart in a frame's name.
    Names,
}

impl fmt::Display for SmdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Version => "the file is not version 1",
            Self::NoTriangles => {
                "the file has no triangles: it is an animation, to give with a model's file"
            }
            Self::Flat => "a joint's base pose has no size on an axis",
            Self::Names => "too many animations have one name for their frames to be named",
        })
    }
}

impl core::error::Error for SmdError {}

/// A vertex of a triangle.
#[derive(Clone, Debug, PartialEq)]
struct Vertex {
    position: Vec3,
    texcoord: [f32; 2],
    blends: Vec<(usize, f64)>,
}

/// An `.smd` file as read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Smd {
    /// Each joint's parent, by the joint's number; a number the file names no joint for has
    /// none.
    parents: Vec<Option<usize>>,
    /// The vertices, three for each triangle.
    vertices: Vec<Vertex>,
    /// Whether the file has a `triangles` section.
    model: bool,
    /// The frames of the `skeleton` section, each a pose for each joint in its parent's
    /// space, `None` for a joint the file gives no pose in its first frame.
    frames: Vec<Vec<Option<Affine>>>,
}

/// Whether nothing of a line is read: an empty one, and one starting with `#`, `;` or
/// `//` (`skipcomment`).
fn is_comment(line: &str) -> bool {
    let line = line.trim_start();
    line.is_empty() || line.starts_with(['#', ';']) || line.starts_with("//")
}

/// A name, in double quotes or up to a space, and what follows it (`readname`).
fn name(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    match text.strip_prefix('"') {
        Some(quoted) => quoted.split_once('"').unwrap_or((quoted, "")),
        None => text.split_once(char::is_whitespace).unwrap_or((text, "")),
    }
}

/// The numbers a line starts with, as `sscanf` reads them: up to the first word that is
/// none.
fn numbers(line: &str) -> Vec<f64> {
    line.split_whitespace()
        .map_while(|word| word.parse::<f64>().ok().filter(|value| value.is_finite()))
        .collect()
}

/// A number of a line that names a joint or a count: the whole number it is.
fn whole(value: f64) -> Option<usize> {
    format!("{value}").parse().ok()
}

impl Smd {
    /// Reads an `.smd` file.
    ///
    /// Implements MDL-10.3.
    ///
    /// # Errors
    ///
    /// [`SmdError::Version`] for a file of another version than 1.
    pub fn read(text: &str) -> Result<Self, SmdError> {
        let mut file = Self::default();
        let mut lines = text.lines().filter(|line| !is_comment(line));
        while let Some(line) = lines.next() {
            let line = line.trim_start();
            if let Some(version) = line.strip_prefix("version") {
                if version.trim().parse() != Ok(SMD_VERSION) {
                    return Err(SmdError::Version);
                }
            } else if line.starts_with("nodes") {
                file.nodes(&mut lines);
            } else if line.starts_with("triangles") {
                file.triangles(&mut lines);
                file.model = true;
            } else if line.starts_with("skeleton") {
                file.skeleton(&mut lines);
            } else if line.starts_with("vertexanimation") {
                // `skipsection`.
                for line in lines.by_ref() {
                    if line.trim_start().starts_with("end") {
                        break;
                    }
                }
            }
        }
        Ok(file)
    }

    /// The `nodes` section: a line for each joint, its number, its name and its parent's
    /// number. A number over [`SMD_MAX_JOINT`], a line with no name and a second line for
    /// one number are left out.
    fn nodes<'a>(&mut self, lines: &mut impl Iterator<Item = &'a str>) {
        let mut named: Vec<bool> = Vec::new();
        for line in lines {
            let line = line.trim_start();
            if line.starts_with("end") {
                break;
            }
            let (id, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            let (name, rest) = name(rest);
            let Some(id) = id.parse::<usize>().ok().filter(|id| *id <= SMD_MAX_JOINT) else {
                continue;
            };
            let parent = rest
                .split_whitespace()
                .next()
                .and_then(|word| word.parse::<i64>().ok())
                .unwrap_or(0);
            let too_far = usize::try_from(parent).is_ok_and(|parent| parent > SMD_MAX_JOINT);
            if name.is_empty() || too_far || named.get(id) == Some(&true) {
                continue;
            }
            if self.parents.len() <= id {
                self.parents.resize(id.saturating_add(1), None);
                named.resize(id.saturating_add(1), false);
            }
            if let (Some(slot), Some(known)) = (self.parents.get_mut(id), named.get_mut(id)) {
                *slot = usize::try_from(parent).ok();
                *known = true;
            }
        }
    }

    /// The `triangles` section: a line naming the material, then a line for each of three
    /// vertices: its joint, its position, its normal, its texture coordinate and, after a
    /// count, the joints it is linked to each with a weight. The section ends at a vertex's
    /// line of fewer numbers.
    ///
    /// A vertex's `v` is measured up the picture ("`tc->y = 1 - tc->y;`"). Its links to its
    /// own joint are summed with what the links' weights lack of 1, and it is blended to its
    /// own joint by that and to the others by theirs. The three vertices are kept last
    /// first, which turns the triangle around.
    fn triangles<'a>(&mut self, lines: &mut impl Iterator<Item = &'a str>) {
        while let Some(material) = lines.next() {
            if material.trim_start().starts_with("end") {
                break;
            }
            let mut corners = Vec::with_capacity(CORNERS);
            for _ in 0..CORNERS {
                let Some(values) = lines.next().map(numbers) else {
                    return;
                };
                let Some(&[own, x, y, z, _, _, _, u, v]) = values.first_chunk::<VERTEX_NUMBERS>()
                else {
                    return;
                };
                let own = whole(own);
                let links = values
                    .get(VERTEX_NUMBERS)
                    .copied()
                    .and_then(whole)
                    .unwrap_or(0);
                let (pairs, _) = values
                    .get(VERTEX_NUMBERS.saturating_add(1)..)
                    .unwrap_or(&[])
                    .as_chunks::<2>();
                let mut own_weight = 0.0;
                let mut total = 0.0;
                let mut others = Vec::new();
                for [joint, weight] in pairs.iter().take(links) {
                    total += weight;
                    match whole(*joint) {
                        joint if joint == own => own_weight += weight,
                        Some(joint) => others.push((joint, *weight)),
                        None => {}
                    }
                }
                if total < 1.0 {
                    own_weight += 1.0 - total;
                }
                if let Some(own) = own.filter(|_| own_weight > 0.0) {
                    others.push((own, own_weight));
                }
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "spec MDL-10.3: a texture coordinate is kept in single precision"
                )]
                let texcoord = [u as f32, (1.0 - v) as f32];
                corners.push(Vertex {
                    position: [x, y, z],
                    texcoord,
                    blends: blend(&others),
                });
            }
            corners.reverse();
            self.vertices.extend(corners);
        }
    }

    /// The `skeleton` section: `time N` starts frame `N`, and a line of a joint's number, a
    /// position and three angles is that joint's pose in the frame, in its parent's space. A
    /// frame the file skips, and a joint a frame gives no pose, have the first frame's.
    fn skeleton<'a>(&mut self, lines: &mut impl Iterator<Item = &'a str>) {
        let joints = self.parents.len();
        let mut frame: Option<usize> = None;
        for line in lines {
            let line = line.trim_start();
            if line.starts_with("end") {
                break;
            }
            if let Some(time) = line.strip_prefix("time") {
                frame = time.trim().parse().ok();
                if let Some(frame) = frame
                    && self.frames.len() <= frame
                {
                    self.frames
                        .resize(frame.saturating_add(1), vec![None; joints]);
                }
                continue;
            }
            let values = numbers(line);
            let Some(&[joint, px, py, pz, rx, ry, rz]) = values.first_chunk::<POSE_NUMBERS>()
            else {
                continue;
            };
            let slot = frame
                .and_then(|frame| self.frames.get_mut(frame))
                .zip(whole(joint))
                .and_then(|(poses, joint)| poses.get_mut(joint));
            if let Some(slot) = slot {
                *slot = Some(scaled(
                    turn(from_angles([rx, ry, rz])),
                    NO_SCALE,
                    [px, py, pz],
                ));
            }
        }
    }

    /// The poses of frame `frame`, a joint it has none for taking the first frame's, and one
    /// that has none there standing where its parent is.
    fn poses(&self, frame: &[Option<Affine>]) -> Vec<Affine> {
        let first = self.frames.first();
        frame
            .iter()
            .enumerate()
            .map(|(joint, pose)| {
                pose.or_else(|| first.and_then(|first| first.get(joint).copied().flatten()))
                    .unwrap_or(Affine::IDENTITY)
            })
            .collect()
    }

    /// The mesh of the model `self` posed by `animations`, each a name and its file, its
    /// axes turned as `up` says: a pose for each frame of each animation, or one of the
    /// model as it is. The model's base pose is the first frame of its own skeleton, and its
    /// other frames are not poses of the mesh.
    ///
    /// Implements MDL-10.3.
    ///
    /// # Errors
    ///
    /// [`SmdError`]: a file with no triangles, a joint of no size, and animations whose
    /// frames cannot be named apart.
    pub fn mesh(&self, animations: &[(String, Self)], up: Up) -> Result<Mesh, SmdError> {
        if !self.model {
            return Err(SmdError::NoTriangles);
        }
        let base = self.frames.first().map_or_else(
            || vec![Affine::IDENTITY; self.parents.len()],
            |first| self.poses(first),
        );
        // Each three vertices in turn are a triangle.
        let corners: Vec<usize> = (0..self.vertices.len()).collect();
        let triangles = corners.as_chunks::<CORNERS>().0.to_vec();
        Boned {
            positions: self.vertices.iter().map(|vertex| vertex.position).collect(),
            texcoords: self.vertices.iter().map(|vertex| vertex.texcoord).collect(),
            blends: self
                .vertices
                .iter()
                .map(|vertex| vertex.blends.clone())
                .collect(),
            triangles,
            parents: self.parents.clone(),
            base,
            animations: animations
                .iter()
                .map(|(name, animation)| {
                    let frames = animation
                        .frames
                        .iter()
                        .map(|frame| animation.poses(frame))
                        .collect();
                    (name.clone(), frames)
                })
                .collect(),
            rest: SMD_FRAME,
            unnamed: SMD_ANIMATION,
        }
        .mesh(up)
        .map_err(|error| match error {
            RigError::Flat => SmdError::Flat,
            RigError::Names => SmdError::Names,
        })
    }
}

#[cfg(test)]
mod tests;
