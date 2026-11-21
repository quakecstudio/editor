// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2010-2016 Lee Salzman
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: iqm/iqm.cpp, buildmd5verts, loadmd5mesh, loadmd5anim, makerelativebasepose

//! An `.md5mesh` with its `.md5anim` files as a mesh of poses (MDL-10.2), as FTEQW's model
//! converter makes one of them. The files are read by [`crate::md5`]; this is what the
//! converter does with what they hold.

use core::fmt;

use super::Mesh;
use super::obj::Up;
use super::rig::{Affine, Boned, RigError, Vec3, blend, scaled, turn};
use crate::md5::{Md5Anim, Md5Mesh, md5_quat_w};

/// The name of the one pose of a mesh with no animation.
pub const MD5_FRAME: &str = "base";
/// What an animation with no name is called.
pub const MD5_ANIMATION: &str = "anim";
/// The flags of a hierarchy entry, one for each component a frame gives: the position's x,
/// y and z and the quaternion's (MD5-3.3).
const COMPONENTS: usize = 6;
/// The most vertices, triangles, weights and frames a file may say it has. Ours: the
/// readers keep a count as the file gives it, and a model has far fewer of each.
pub const MD5_MAX_COUNT: u64 = 65_536;
/// A scale of one on each axis: an MD5 pose has none.
const NO_SCALE: Vec3 = [1.0, 1.0, 1.0];

/// Why an MD5 model is no mesh of poses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Md5MeshError {
    /// An animation is not of as many joints as the mesh.
    Joints {
        /// The animation's name.
        animation: String,
    },
    /// A file says it has more vertices, triangles, weights or frames than
    /// [`MD5_MAX_COUNT`].
    Count,
    /// A joint of the mesh has no size on an axis.
    Flat,
    /// More animations have one name than can be told apart in a frame's name.
    Names,
}

impl fmt::Display for Md5MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Joints { animation } => write!(
                f,
                "the animation {animation} is not of as many joints as the mesh"
            ),
            Self::Count => write!(
                f,
                "the model says it has more than {MD5_MAX_COUNT} vertices, triangles, weights or \
                 frames"
            ),
            Self::Flat => f.write_str("a joint of the mesh has no size on an axis"),
            Self::Names => {
                f.write_str("too many animations have one name for their frames to be named")
            }
        }
    }
}

impl core::error::Error for Md5MeshError {}

/// A pose with no scale: a position and a quaternion's x, y, z and w.
fn pose(position: [f32; 3], [x, y, z, w]: [f32; 4]) -> Affine {
    scaled(
        turn([f64::from(x), f64::from(y), f64::from(z), f64::from(w)]),
        NO_SCALE,
        position.map(f64::from),
    )
}

impl Md5Mesh {
    /// The mesh posed by `animations`, each a name and its file, its axes turned as `up` says: a pose for each frame of each animation, or one of the mesh as it is.
    ///
    /// A vertex is where its weights put it by the mesh's joints (`buildmd5verts`), and is
    /// blended to those joints by its weights' biases. A joint of the mesh is given in the
    /// model's space and is made its parent's (`makerelativebasepose`); a joint of an animation
    /// is its base frame with the components the frame gives put in (`loadmd5anim`).
    ///
    /// Implements MDL-10.2.
    ///
    /// # Errors
    ///
    /// [`Md5MeshError`]: an animation of another number of joints than the mesh, a joint of no
    /// size, and animations whose frames cannot be named apart.
    pub fn mesh(&self, animations: &[(String, Md5Anim)], up: Up) -> Result<Mesh, Md5MeshError> {
        let md5 = self;
        let placed: Vec<Affine> = md5
            .joints
            .iter()
            .map(|joint| pose(joint.position, joint.orientation))
            .collect();
        let parents: Vec<Option<usize>> = md5
            .joints
            .iter()
            .map(|joint| {
                usize::try_from(joint.parent)
                    .ok()
                    .filter(|parent| *parent < md5.joints.len())
            })
            .collect();
        // Each joint in its parent's space: what undoes the parent, then the joint.
        let mut base = Vec::with_capacity(placed.len());
        for (joint, parent) in placed.iter().zip(&parents) {
            let own = match parent.and_then(|parent| placed.get(parent)) {
                Some(parent) => parent.inverse().ok_or(Md5MeshError::Flat)?.after(joint),
                None => *joint,
            };
            base.push(own);
        }

        // A count is the file's word, checked before anything is made of that many.
        let counts = md5
            .meshes
            .iter()
            .flat_map(|surface| [surface.num_verts, surface.num_tris, surface.num_weights])
            .chain(animations.iter().map(|(_, animation)| animation.num_frames));
        if counts.into_iter().any(|count| count > MD5_MAX_COUNT) {
            return Err(Md5MeshError::Count);
        }

        let mut positions = Vec::new();
        let mut texcoords = Vec::new();
        let mut blends = Vec::new();
        let mut triangles = Vec::new();
        for surface in &md5.meshes {
            let first = positions.len();
            for index in 0..surface.num_verts {
                let vert = surface.vert(index).unwrap_or_default();
                let weights: Vec<_> = (0..vert.weight_count.min(surface.num_weights))
                    .filter_map(|at| surface.weight(vert.first_weight.checked_add(at)?))
                    .collect();
                let mut at = [0.0, 0.0, 0.0];
                for weight in &weights {
                    let joint = usize::try_from(weight.joint)
                        .ok()
                        .and_then(|joint| placed.get(joint));
                    if let Some(joint) = joint {
                        let there = joint.apply(weight.position.map(f64::from));
                        for (sum, part) in at.iter_mut().zip(there) {
                            *sum += part * f64::from(weight.bias);
                        }
                    }
                }
                positions.push(at);
                texcoords.push(vert.st);
                let pairs: Vec<(usize, f64)> = weights
                    .iter()
                    .filter_map(|weight| {
                        Some((usize::try_from(weight.joint).ok()?, f64::from(weight.bias)))
                    })
                    .collect();
                blends.push(blend(&pairs));
            }
            for index in 0..surface.num_tris {
                let corners = surface.tri(index).unwrap_or_default();
                let moved = corners.map(|vertex| {
                    usize::try_from(vertex)
                        .ok()
                        .and_then(|vertex| first.checked_add(vertex))
                });
                if let [Some(a), Some(b), Some(c)] = moved {
                    triangles.push([a, b, c]);
                }
            }
        }

        let mut posed = Vec::with_capacity(animations.len());
        for (name, animation) in animations {
            if animation.hierarchy.len() != md5.joints.len() {
                return Err(Md5MeshError::Joints {
                    animation: name.clone(),
                });
            }
            let frames = (0..animation.num_frames)
                .map(|frame| {
                    let given = animation.frame(frame).unwrap_or_default();
                    animation
                        .hierarchy
                        .iter()
                        .zip(&animation.baseframe)
                        .map(|(entry, [position, turned])| {
                            // The base frame's six numbers, each replaced by the frame's where
                            // the joint's flags say the frame gives it, in turn from its first.
                            let ([px, py, pz], [qx, qy, qz]) = (*position, *turned);
                            let mut six: [f32; COMPONENTS] = [px, py, pz, qx, qy, qz];
                            let mut next = usize::try_from(entry.first_component).ok();
                            for (bit, value) in six.iter_mut().enumerate() {
                                if entry.flags >> bit & 1 == 0 {
                                    continue;
                                }
                                if let Some(found) = next.and_then(|at| given.get(at)) {
                                    *value = *found;
                                }
                                next = next.and_then(|at| at.checked_add(1));
                            }
                            let [px, py, pz, x, y, z] = six;
                            pose([px, py, pz], [x, y, z, md5_quat_w([x, y, z])])
                        })
                        .collect()
                })
                .collect();
            posed.push((name.clone(), frames));
        }

        Boned {
            positions,
            texcoords,
            blends,
            triangles,
            parents,
            base,
            animations: posed,
            rest: MD5_FRAME,
            unnamed: MD5_ANIMATION,
        }
        .mesh(up)
        .map_err(|error| match error {
            RigError::Flat => Md5MeshError::Flat,
            RigError::Names => Md5MeshError::Names,
        })
    }
}

#[cfg(test)]
mod tests;
