// SPDX-License-Identifier: GPL-3.0-or-later

//! Inter-Quake Export files (`.iqe`), read as a mesh (MDL-9.2), from the format's own
//! description, `specs/iqe/iqe.txt`: the positions (`vp`), texture coordinates (`vt`) and
//! blends (`vb`) of its vertices, its faces (`fa`, `fm`), its skeleton (`joint`, `pq`, `pm`)
//! and its animations (`animation`, `frame`). An alias model has no skeleton: every frame of
//! every animation becomes a pose of the mesh, each vertex moved by the joints it is
//! blended to.

use core::fmt;
use std::collections::BTreeSet;

use super::obj::Up;
use super::{Corner, Mesh, MeshFrame};

/// What a file's first line starts with: "The first line of an IQE file must be: # Inter-Quake
/// Export".
pub const IQE_HEADER: &str = "# Inter-Quake Export";
/// The name of the one frame of a file without animations.
pub const IQE_FRAME: &str = "base";
/// The name of an animation the file gives none.
pub const IQE_ANIMATION: &str = "anim";
/// The most bytes of a frame's name (MDL-6.3).
const NAME_MAX: usize = 15;
/// The corners of a triangle.
const CORNERS: usize = 3;

/// What is wrong with a line of an `.iqe` file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IqeFault {
    /// The file does not start with [`IQE_HEADER`].
    Header,
    /// An argument is no number.
    Number,
    /// A face has fewer than three corners, or one that is no vertex of the file.
    Face,
    /// The file has more animations of one name than can be told apart in a frame's name.
    Names,
    /// A joint's parent is not a joint above it.
    Joint,
    /// A `pa` pose: the description does not say in what order its three rotations apply.
    Angles,
    /// A joint's base pose flattens space, so nothing can be taken back out of it.
    Flat,
}

/// Why an `.iqe` file was not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IqeError {
    /// The line, from 1; 0 for the file as a whole.
    pub line: usize,
    /// What is wrong.
    pub fault: IqeFault,
}

impl fmt::Display for IqeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.fault {
            IqeFault::Header => "the file does not start with \"# Inter-Quake Export\"",
            IqeFault::Number => "an argument is no number",
            IqeFault::Face => "a face needs three corners, each a vertex of the file",
            IqeFault::Names => "too many animations have one name for their frames to be named",
            IqeFault::Joint => "a joint's parent must be a joint above it",
            IqeFault::Angles => {
                "a pa pose is not read: the format does not say in what order its rotations \
                 apply; write poses as pq"
            }
            IqeFault::Flat => "a joint's base pose has no size on an axis",
        };
        write!(f, "line {}: {what}", self.line)
    }
}

impl core::error::Error for IqeError {}

/// Three numbers.
type Vec3 = [f64; 3];
/// A matrix of three rows.
type Mat3 = [Vec3; 3];

/// A rotation and scale followed by a move: what a pose does to a point.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Affine {
    matrix: Mat3,
    moved: Vec3,
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
    const IDENTITY: Self = Self {
        matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        moved: [0.0; 3],
    };

    /// Where `point` goes.
    fn apply(&self, point: Vec3) -> Vec3 {
        let [x, y, z] = self.matrix.map(|row| dot(row, point));
        let [mx, my, mz] = self.moved;
        [x + mx, y + my, z + mz]
    }

    /// `self` after `first`: what a joint's pose is in its parent's space.
    fn after(&self, first: &Self) -> Self {
        let columns = transposed(first.matrix);
        Self {
            matrix: self
                .matrix
                .map(|row| columns.map(|column| dot(row, column))),
            moved: self.apply(first.moved),
        }
    }

    /// What undoes `self`; `None` when it flattens space.
    fn inverse(&self) -> Option<Self> {
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

/// Where the parts of a `pq` pose's arguments start: its quaternion's x, y and z, its w, and
/// its scale, after the translation's three.
const PQ_ROTATION: usize = 3;
const PQ_W: usize = 6;
const PQ_SCALE: usize = 7;
/// Where the rows of a `pm` pose's rotation matrix start, and its scale.
const PM_ROWS: [usize; 3] = [3, 6, 9];
const PM_SCALE: usize = 12;

/// The three numbers of `values` from `from`, `or` for one not given.
fn three(values: &[f64], from: usize, or: f64) -> Vec3 {
    let at = |place: usize| values.get(place).copied().unwrap_or(or);
    [
        at(from),
        at(from.saturating_add(1)),
        at(from.saturating_add(2)),
    ]
}

/// A `pq` pose: "output = (input*scale)*rotation + translation". A quaternion is made a unit
/// one first; "If Qw is unspecified, it is generated such that it is a negative value".
fn pose_quaternion(values: &[f64]) -> Affine {
    let [x, y, z] = three(values, PQ_ROTATION, 0.0);
    let w = values
        .get(PQ_W)
        .copied()
        .unwrap_or_else(|| -(1.0 - x * x - y * y - z * z).max(0.0).sqrt());
    let length = (x * x + y * y + z * z + w * w).sqrt();
    let (x, y, z, w) = if length > 0.0 {
        (x / length, y / length, z / length, w / length)
    } else {
        (0.0, 0.0, 0.0, 1.0)
    };
    let rotation: Mat3 = [
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
    ];
    scaled(
        rotation,
        three(values, PQ_SCALE, 1.0),
        three(values, 0, 0.0),
    )
}

/// A `pm` pose: its translation, the rows of its rotation matrix and its scale.
fn pose_matrix(values: &[f64]) -> Affine {
    scaled(
        PM_ROWS.map(|row| three(values, row, 0.0)),
        three(values, PM_SCALE, 1.0),
        three(values, 0, 0.0),
    )
}

/// The pose that scales a point, then turns it by `rotation`, then moves it by `moved`.
fn scaled(rotation: Mat3, [sx, sy, sz]: Vec3, moved: Vec3) -> Affine {
    Affine {
        matrix: rotation.map(|[a, b, c]| [a * sx, b * sy, c * sz]),
        moved,
    }
}

/// A face as its line gives it, kept until the whole file is read: a face may come before
/// the vertices it names.
#[derive(Clone, Debug)]
struct Face {
    line: usize,
    /// Its corners' indexes as written.
    indexes: Vec<i64>,
    /// The vertex an index of 0 or more counts from: the file's first for `fa`, the mesh's
    /// first for `fm`.
    from: usize,
    /// How many vertices were defined above the face: an index below 0 counts back from
    /// there.
    above: usize,
}

/// An animation: its name and its frames, each a pose for each joint in turn.
#[derive(Clone, Debug, Default)]
struct Animation {
    name: String,
    frames: Vec<Vec<Affine>>,
}

/// The words of a line: split at spaces, a word in double quotes kept whole without them, as
/// the format's own exporter writes a name.
fn words(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line.trim_start();
    while !rest.is_empty() {
        let (word, after) = match rest.strip_prefix('"') {
            Some(quoted) => quoted.split_once('"').unwrap_or((quoted, "")),
            None => rest.split_once(char::is_whitespace).unwrap_or((rest, "")),
        };
        out.push(word);
        rest = after.trim_start();
    }
    out
}

/// A single-precision position.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec MDL-9.2: a posed position is stored in single precision, the nearest"
)]
fn single(point: Vec3) -> [f32; 3] {
    point.map(|part| part as f32)
}

/// What is read of a file. Its vertex attributes are arrays of their own, which "may be
/// interleaved in any order, or not interleaved at all": the Nth of each is the Nth vertex's.
#[derive(Default)]
struct File {
    positions: Vec<Vec3>,
    texcoords: Vec<[f32; 2]>,
    /// For each `vb` in turn, the joints a vertex is blended to, each with its weight.
    blends: Vec<Vec<(usize, f64)>>,
    faces: Vec<Face>,
    /// Where each mesh's vertices start.
    meshes: Vec<usize>,
    /// Each joint's parent.
    parents: Vec<Option<usize>>,
    /// The base poses, one for each joint in turn.
    base: Vec<Affine>,
    animations: Vec<Animation>,
}

impl File {
    /// Keeps the face of `words`, whose indexes of 0 or more count from `from`: three
    /// corners or more, each a whole number.
    fn face(&mut self, line: usize, words: &[&str], from: usize) -> Option<()> {
        let indexes: Vec<i64> = words
            .iter()
            .map(|word| word.parse().ok())
            .collect::<Option<_>>()?;
        if indexes.len() < CORNERS {
            return None;
        }
        self.faces.push(Face {
            line,
            indexes,
            from,
            above: self.positions.len(),
        });
        Some(())
    }

    /// The triangles of the faces, now that every vertex is read: an index of 0 or more
    /// counts from the face's start, one below 0 back from the last vertex above the face,
    /// "-1" that one; more than three are "a convex, planar polygon", a fan.
    fn triangles(&self) -> Result<Vec<[usize; 3]>, IqeError> {
        let mut triangles = Vec::new();
        for face in &self.faces {
            let place = |index: i64| -> Option<usize> {
                let start = if index < 0 { face.above } else { face.from };
                let place = i64::try_from(start).ok()?.checked_add(index)?;
                usize::try_from(place)
                    .ok()
                    .filter(|at| *at < self.positions.len())
            };
            let corners: Vec<usize> = face
                .indexes
                .iter()
                .map(|index| place(*index))
                .collect::<Option<_>>()
                .ok_or(IqeError {
                    line: face.line,
                    fault: IqeFault::Face,
                })?;
            if let Some((&first, rest)) = corners.split_first() {
                for pair in rest.windows(2) {
                    if let [second, third] = *pair {
                        triangles.push([first, second, third]);
                    }
                }
            }
        }
        Ok(triangles)
    }

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

    /// The mesh.
    fn mesh(self, up: Up) -> Result<Mesh, IqeError> {
        let whole = |fault| IqeError { line: 0, fault };
        // "If there is no triangle data, then all vertex data ... is interpreted as mesh
        // local triangle data, where each group of 3 vertexes defines a triangle."
        let faces = if self.faces.is_empty() {
            let mut starts = self.meshes.clone();
            if starts.first() != Some(&0) {
                starts.insert(0, 0);
            }
            starts.push(self.positions.len());
            let mut faces = Vec::new();
            for pair in starts.windows(2) {
                if let [from, to] = *pair {
                    let mut at = from;
                    while at.saturating_add(CORNERS) <= to {
                        faces.push([at, at.saturating_add(1), at.saturating_add(2)]);
                        at = at.saturating_add(CORNERS);
                    }
                }
            }
            faces
        } else {
            self.triangles()?
        };
        // "Each vertex array must either be absent, or the same length as all other vertex
        // arrays": a vertex has the texture coordinate of its own number.
        let textured = self.texcoords.len() == self.positions.len();
        let blended = self.blends.len() == self.positions.len();
        let no_blends = Vec::new();
        let triangles = faces
            .into_iter()
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
            .map(|joint| joint.inverse().ok_or_else(|| whole(IqeFault::Flat)))
            .collect::<Result<_, _>>()?;
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
                    let blends = self
                        .blends
                        .get(vertex)
                        .filter(|_| blended)
                        .unwrap_or(&no_blends);
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
        for animation in &self.animations {
            let stem = frame_stem(&animation.name, animation.frames.len(), &used)
                .ok_or_else(|| whole(IqeFault::Names))?;
            for (number, poses) in (1_usize..).zip(&animation.frames) {
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
                name: IQE_FRAME.to_owned(),
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
/// `anim` for none, cut so that the name of its last frame, of `count`, fits a frame's name,
/// and with a letter after it when a frame so named is in `used` already. `None` when no
/// letter makes its frames' names its own.
fn frame_stem(animation: &str, count: usize, used: &BTreeSet<String>) -> Option<String> {
    let mut letters: String = animation
        .chars()
        .filter(|letter| letter.is_ascii_alphanumeric() || *letter == '_')
        .collect();
    if letters.is_empty() {
        letters.push_str(IQE_ANIMATION);
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

/// Reads an `.iqe` file as a mesh, its axes turned as `up` says: a pose for each frame of
/// each of its animations, named by the animation and the frame's number from 1, or one pose
/// of the mesh as it is where the file has no animation.
///
/// Implements MDL-9.2.
///
/// # Errors
///
/// [`IqeError`] with the line: a file without the header, an argument that is no number, a
/// face without three vertices of the file, a joint whose parent is not above it, a `pa`
/// pose, and a base pose that flattens space.
pub fn read(text: &str, up: Up) -> Result<Mesh, IqeError> {
    if !text.starts_with(IQE_HEADER) {
        return Err(IqeError {
            line: 1,
            fault: IqeFault::Header,
        });
    }
    let mut file = File::default();
    for (line, text) in (1_usize..).zip(text.lines()) {
        let fault = |fault| IqeError { line, fault };
        // "Any line beginning with # is a comment".
        if text.starts_with('#') {
            continue;
        }
        let words = words(text);
        let Some((&command, arguments)) = words.split_first() else {
            continue;
        };
        let numbers = || -> Result<Vec<f64>, IqeError> {
            arguments
                .iter()
                .map(|word| word.parse::<f64>().ok().filter(|value| value.is_finite()))
                .collect::<Option<_>>()
                .ok_or_else(|| fault(IqeFault::Number))
        };
        let at = |values: &[f64], place: usize| values.get(place).copied().unwrap_or(0.0);
        match command {
            "vp" => {
                let values = numbers()?;
                file.positions
                    .push([at(&values, 0), at(&values, 1), at(&values, 2)]);
            }
            "vt" => {
                let values = numbers()?;
                let [u, v, _] = single([at(&values, 0), at(&values, 1), 0.0]);
                file.texcoords.push([u, v]);
            }
            "vb" => {
                // "Blend indexes ... and blend weights", in pairs; one of no weight is unused.
                let mut blends = Vec::new();
                for [joint, weight] in arguments.as_chunks::<2>().0 {
                    let joint: usize = joint.parse().map_err(|_| fault(IqeFault::Number))?;
                    let weight: f64 = weight.parse().map_err(|_| fault(IqeFault::Number))?;
                    if weight > 0.0 && weight.is_finite() {
                        blends.push((joint, weight));
                    }
                }
                file.blends.push(blends);
            }
            "mesh" => file.meshes.push(file.positions.len()),
            "fa" => file
                .face(line, arguments, 0)
                .ok_or_else(|| fault(IqeFault::Face))?,
            "fm" => {
                let from = file.meshes.last().copied().unwrap_or(0);
                file.face(line, arguments, from)
                    .ok_or_else(|| fault(IqeFault::Face))?;
            }
            "joint" => {
                // "If PARENT is less than 0 or unspecified, then the joint has no parent."
                let parent = match arguments.get(1) {
                    Some(word) => word.parse::<i64>().map_err(|_| fault(IqeFault::Number))?,
                    None => -1,
                };
                let parent = match usize::try_from(parent) {
                    Ok(parent) if parent < file.parents.len() => Some(parent),
                    Ok(_) => return Err(fault(IqeFault::Joint)),
                    Err(_) => None,
                };
                file.parents.push(parent);
            }
            "pq" | "pm" => {
                let values = numbers()?;
                let pose = if command == "pq" {
                    pose_quaternion(&values)
                } else {
                    pose_matrix(&values)
                };
                // Poses before any animation are the base poses; after one they are its
                // current frame's.
                match file.animations.last_mut() {
                    Some(animation) => {
                        if animation.frames.is_empty() {
                            animation.frames.push(Vec::new());
                        }
                        if let Some(frame) = animation.frames.last_mut() {
                            frame.push(pose);
                        }
                    }
                    None => file.base.push(pose),
                }
            }
            "pa" => return Err(fault(IqeFault::Angles)),
            "animation" => file.animations.push(Animation {
                name: arguments.first().copied().unwrap_or("").to_owned(),
                frames: Vec::new(),
            }),
            "frame" => {
                if let Some(animation) = file.animations.last_mut() {
                    animation.frames.push(Vec::new());
                }
            }
            // "All text starting on the line after the "comment" command, up to the end of
            // the file, is treated verbatim as the comment section."
            "comment" => break,
            _ => {}
        }
    }
    file.mesh(up)
}

#[cfg(test)]
mod tests;
