// SPDX-License-Identifier: GPL-3.0-or-later

//! A mesh with a skeleton, as the formats that have one give it (MDL-9.2, MDL-10): joints
//! each with a parent and a base pose, vertices blended to joints, and animations of poses.
//! An alias model has no skeleton, so [`Boned::mesh`] makes every frame of every animation a
//! pose of the mesh.

use std::collections::BTreeSet;

use super::obj::Up;
use super::{Corner, Mesh, MeshFrame};

/// The most bytes of a frame's name (MDL-6.3).
const NAME_MAX: usize = 15;

/// Three numbers.
pub(super) type Vec3 = [f64; 3];
/// A matrix of three rows.
pub(super) type Mat3 = [Vec3; 3];

/// A rotation and scale followed by a move: what a pose does to a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Affine {
    pub(super) matrix: Mat3,
    pub(super) moved: Vec3,
}

/// The dot product.
fn dot([ax, ay, az]: Vec3, [bx, by, bz]: Vec3) -> f64 {
    ax * bx + ay * by + az * bz
}

/// A matrix's columns as rows.
const fn transposed([[a, b, c], [d, e, f], [g, h, i]]: Mat3) -> Mat3 {
    [[a, d, g], [b, e, h], [c, f, i]]
}

/// The cross product.
fn cross([ax, ay, az]: Vec3, [bx, by, bz]: Vec3) -> Vec3 {
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

impl Affine {
    pub(super) const IDENTITY: Self = Self {
        matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        moved: [0.0; 3],
    };

    /// Where `point` goes.
    pub(super) fn apply(&self, point: Vec3) -> Vec3 {
        let [x, y, z] = self.matrix.map(|row| dot(row, point));
        let [mx, my, mz] = self.moved;
        [x + mx, y + my, z + mz]
    }

    /// `self` after `first`: what a joint's pose is in its parent's space.
    pub(super) fn after(&self, first: &Self) -> Self {
        let columns = transposed(first.matrix);
        Self {
            matrix: self
                .matrix
                .map(|row| columns.map(|column| dot(row, column))),
            moved: self.apply(first.moved),
        }
    }

    /// What undoes `self`; `None` when it flattens space.
    pub(super) fn inverse(&self) -> Option<Self> {
        let [a, b, c] = self.matrix;
        let determinant = dot(a, cross(b, c));
        if determinant.abs() < f64::MIN_POSITIVE {
            return None;
        }
        // The inverse's columns are the cross products of the rows, over the determinant.
        let matrix = transposed([cross(b, c), cross(c, a), cross(a, b)])
            .map(|row| row.map(|part| part / determinant));
        let [x, y, z] = matrix.map(|row| dot(row, self.moved));
        Some(Self {
            matrix,
            moved: [-x, -y, -z],
        })
    }
}

/// The rotation a quaternion `(x, y, z, w)` is, made a unit one first; one of no length
/// turns nothing.
pub(super) fn turn([x, y, z, w]: [f64; 4]) -> Mat3 {
    let length = (x * x + y * y + z * z + w * w).sqrt();
    let (x, y, z, w) = if length > 0.0 {
        (x / length, y / length, z / length, w / length)
    } else {
        (0.0, 0.0, 0.0, 1.0)
    };
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

/// The pose that scales a point, then turns it by `rotation`, then moves it by `moved`.
pub(super) fn scaled(rotation: Mat3, [sx, sy, sz]: Vec3, moved: Vec3) -> Affine {
    Affine {
        matrix: rotation.map(|[a, b, c]| [a * sx, b * sy, c * sz]),
        moved,
    }
}

/// A single-precision position.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec MDL-9.2, MDL-10.1: a posed position is stored in single precision, the nearest"
)]
pub(super) fn single(point: Vec3) -> [f32; 3] {
    point.map(|part| part as f32)
}

/// Why a mesh with a skeleton is no mesh of poses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RigError {
    /// A joint's base pose flattens space, so nothing can be taken back out of it.
    Flat,
    /// More animations have one name than can be told apart in a frame's name.
    Names,
}

/// A mesh with a skeleton.
pub(super) struct Boned {
    /// Where each vertex is in the base pose.
    pub(super) positions: Vec<Vec3>,
    /// Each vertex's texture coordinate, or none for a mesh without them.
    pub(super) texcoords: Vec<[f32; 2]>,
    /// For each vertex the joints it is blended to, each with its weight; none for a mesh
    /// whose vertices are blended to nothing.
    pub(super) blends: Vec<Vec<(usize, f64)>>,
    /// The triangles, each three vertices going clockwise as seen from outside.
    pub(super) triangles: Vec<[usize; 3]>,
    /// Each joint's parent.
    pub(super) parents: Vec<Option<usize>>,
    /// Each joint's base pose, in its parent's space.
    pub(super) base: Vec<Affine>,
    /// The animations: a name and, for each frame, a pose for each joint in turn, in its
    /// parent's space.
    pub(super) animations: Vec<(String, Vec<Vec<Affine>>)>,
    /// The name of the one pose of a mesh with no animation.
    pub(super) rest: &'static str,
    /// What an animation with no name is called.
    pub(super) unnamed: &'static str,
}

impl Boned {
    /// Each joint's pose in the model's space, of `poses`, each in its parent's: a joint the
    /// frame gives no pose keeps its base pose.
    fn world(&self, poses: &[Affine]) -> Vec<Affine> {
        let mut world: Vec<Affine> = Vec::with_capacity(self.parents.len());
        for (joint, parent) in self.parents.iter().enumerate() {
            let own = poses
                .get(joint)
                .or_else(|| self.base.get(joint))
                .copied()
                .unwrap_or(Affine::IDENTITY);
            let placed = parent
                .and_then(|parent| world.get(parent))
                .map_or(own, |parent| parent.after(&own));
            world.push(placed);
        }
        world
    }

    /// The mesh: a pose for each frame of each animation, named by the animation and the
    /// frame's number from 1, or one pose of the mesh as it is. A vertex is the sum, by its
    /// weights over their total, of where each joint it is blended to takes it from where
    /// the base poses had it; a vertex with no blend stays.
    pub(super) fn mesh(self, up: Up) -> Result<Mesh, RigError> {
        let textured = self.texcoords.len() == self.positions.len();
        let triangles = self
            .triangles
            .iter()
            .map(|face| {
                face.map(|vertex| Corner {
                    position: vertex,
                    texcoord: textured.then_some(vertex),
                })
            })
            .collect();
        let bind = self.world(&self.base);
        let unbind: Vec<Affine> = bind
            .iter()
            .map(|joint| joint.inverse().ok_or(RigError::Flat))
            .collect::<Result<_, _>>()?;
        let no_blends = Vec::new();
        let posed = |poses: &[Affine]| -> Vec<[f32; 3]> {
            let world = self.world(poses);
            // What each joint does to a point of the mesh as it was bound.
            let moves: Vec<Affine> = world
                .iter()
                .zip(&unbind)
                .map(|(now, unbind)| now.after(unbind))
                .collect();
            self.positions
                .iter()
                .enumerate()
                .map(|(vertex, position)| {
                    let blends = self.blends.get(vertex).unwrap_or(&no_blends);
                    let total: f64 = blends.iter().map(|(_, weight)| weight).sum();
                    let mut at = [0.0, 0.0, 0.0];
                    for (joint, weight) in blends {
                        let Some(moved) = moves.get(*joint) else {
                            continue;
                        };
                        for (sum, part) in at.iter_mut().zip(moved.apply(*position)) {
                            *sum += part * weight / total;
                        }
                    }
                    let moved = total > 0.0 && blends.iter().all(|(joint, _)| *joint < moves.len());
                    up.turned(single(if moved { at } else { *position }))
                })
                .collect()
        };
        let mut frames = Vec::new();
        let mut used = BTreeSet::new();
        for (animation, poses) in &self.animations {
            let stem =
                frame_stem(animation, poses.len(), self.unnamed, &used).ok_or(RigError::Names)?;
            for (number, poses) in (1_usize..).zip(poses) {
                let name = format!("{stem}{number}");
                used.insert(name.clone());
                frames.push(MeshFrame {
                    name,
                    positions: posed(poses),
                });
            }
        }
        if frames.is_empty() {
            frames.push(MeshFrame {
                name: self.rest.to_owned(),
                positions: posed(&self.base),
            });
        }
        Ok(Mesh {
            triangles,
            texcoords: if textured { self.texcoords } else { Vec::new() },
            frames,
        })
    }
}

/// What may follow an animation's name to tell it from another whose frames would have the
/// same names: nothing, then a letter.
const MARKS: [&str; 8] = ["", "b", "c", "d", "e", "f", "g", "h"];

/// What the frames of the animation `animation` are named by, before each one's number from
/// 1 (`run1`, `run2`, as QuakeC names frames): its name's letters, digits and underscores,
/// `unnamed` for none, cut so that the name of its last frame, of `count`, fits a frame's name,
/// and with a letter after it when a frame so named is in `used` already. `None` when no
/// letter makes its frames' names its own.
pub(super) fn frame_stem(
    animation: &str,
    count: usize,
    unnamed: &str,
    used: &BTreeSet<String>,
) -> Option<String> {
    let mut letters: String = animation
        .chars()
        .filter(|letter| letter.is_ascii_alphanumeric() || *letter == '_')
        .collect();
    if letters.is_empty() {
        letters.push_str(unnamed);
    }
    let digits = count.max(1).to_string().len();
    MARKS.iter().find_map(|mark| {
        let room = NAME_MAX.saturating_sub(digits).saturating_sub(mark.len());
        let mut stem = letters.clone();
        stem.truncate(room);
        stem.push_str(mark);
        (1..=count.max(1))
            .all(|number| !used.contains(&format!("{stem}{number}")))
            .then_some(stem)
    })
}
